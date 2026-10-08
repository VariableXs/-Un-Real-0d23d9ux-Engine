//! 深化层三 · F131 上游回馈通道（2026-09-26 深化批次三）。
//!
//! 补深主册【设计细节】工程化工具面（主册 G-D-06）：PR 拆分规划器
//! （一个逻辑一个 PR 的机器面）、评审轮次模型（改-再审-升级）、stale
//! 检测（90 天无响应线）、commit 标题 lint（上游礼仪机器化）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// PR 拆分规划器：按组件聚类提交，跨组件 PR 拒绝送出
// ---------------------------------------------------------------------------

/// 一条待送提交：归属组件 + 标题。
#[derive(Clone, Copy)]
pub struct PendingCommit {
    pub component: &'static str,
    pub subject: &'static str,
    pub day: u32,
}

/// 拆分规划结论：每个组件一个 PR。跨组件 PR 由 `pr_component_single`
/// 在送出预检拦截——提交只归属单组件，规划层不存在跨界集合。
pub struct SplitPlan {
    /// (组件, 提交数, 最早日) —— 逐组件 PR。
    pub groups: alloc::vec::Vec<(&'static str, usize, u32)>,
}

/// 组件聚类：同组件提交合并为一个 PR。
pub fn plan_split(commits: &[PendingCommit]) -> SplitPlan {
    let mut groups: alloc::vec::Vec<(&'static str, usize, u32)> = alloc::vec::Vec::new();
    for c in commits {
        if let Some(g) = groups.iter_mut().find(|g| g.0 == c.component) {
            g.1 += 1;
            if c.day < g.2 {
                g.2 = c.day;
            }
        } else {
            groups.push((c.component, 1, c.day));
        }
    }
    SplitPlan { groups }
}

/// PR 送出预检：单 PR 只准一个组件（主册纪律的机器面）。
pub fn pr_component_single(commits: &[PendingCommit]) -> Result<(), &'static str> {
    if commits.is_empty() {
        return Err("空 PR：不送");
    }
    let first = commits[0].component;
    if commits.iter().any(|c| c.component != first) {
        return Err("PR 跨组件：违反一个逻辑一个 PR，需拆分");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 评审轮次模型：Reviewing → Changes → Re-review → Approved（三轮升级）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReviewStage {
    Reviewing,
    ChangesRequested,
    ReReview,
    Approved,
    Escalated,
}

pub struct ReviewLoop {
    pub stage: ReviewStage,
    pub rounds: u32,
}

impl ReviewLoop {
    pub fn new() -> ReviewLoop {
        ReviewLoop { stage: ReviewStage::Reviewing, rounds: 0 }
    }

    /// 状态机：Changes 必须经 Re-review 才能到 Approved；三轮仍 Changes 升级人工。
    pub fn transition(&mut self, to: ReviewStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (ReviewStage::Reviewing, ReviewStage::ChangesRequested)
            | (ReviewStage::Reviewing, ReviewStage::Approved)
            | (ReviewStage::ChangesRequested, ReviewStage::ReReview)
            | (ReviewStage::ReReview, ReviewStage::ChangesRequested)
            | (ReviewStage::ReReview, ReviewStage::Approved) => true,
            _ => false,
        };
        if !legal {
            return Err("非法评审迁移");
        }
        if to == ReviewStage::ChangesRequested {
            self.rounds += 1;
            if self.rounds >= 3 {
                self.stage = ReviewStage::Escalated;
                return Ok(());
            }
        }
        self.stage = to;
        Ok(())
    }

    pub fn escalated(&self) -> bool {
        self.stage == ReviewStage::Escalated
    }
}

// ---------------------------------------------------------------------------
// stale 检测：90 天无响应线（超过即 StaleForked 预警）
// ---------------------------------------------------------------------------

pub const STALE_GRACE_DAYS: u32 = 90;

/// 上次活动日 + 宽限线 → stale 判定；poke 刷新活动日。
pub fn stale_since(last_activity_day: u32, today: u32) -> bool {
    today >= last_activity_day && today - last_activity_day > STALE_GRACE_DAYS
}

// ---------------------------------------------------------------------------
// commit 标题 lint：≤72 字节 / 无句号结尾 / 无 WIP 前缀 / 非空
// ---------------------------------------------------------------------------

pub struct SubjectLint {
    pub issues: alloc::vec::Vec<&'static str>,
}

pub fn lint_subject(s: &str) -> SubjectLint {
    let mut issues = alloc::vec::Vec::new();
    if s.is_empty() {
        issues.push("标题为空");
        return SubjectLint { issues };
    }
    if s.len() > 72 {
        issues.push("标题超 72 字节");
    }
    if s.as_bytes()[s.len() - 1] == b'.' {
        issues.push("标题以句号结尾");
    }
    if s.len() >= 4 && &s.as_bytes()[0..4] == b"WIP:" {
        issues.push("WIP 前缀不得送出");
    }
    // 祈使语气启发式：过去式常见尾缀 -ed 收尾提示（不改写、只提示）。
    if s.len() >= 3 && &s.as_bytes()[s.len() - 3..] == b"ed " {
        issues.push("疑似过去式：建议祈使语气");
    }
    SubjectLint { issues }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131F_TAG: &str = "stareco-F131-deep3";

pub fn run_f131_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F131F_TAG);

    // 拆分规划：同组件聚合 / 跨组件退回 / 单组件预检
    let commits = [
        PendingCommit { component: "smoltcp", subject: "fix tcp window", day: 100 },
        PendingCommit { component: "smoltcp", subject: "add tests", day: 101 },
        PendingCommit { component: "ksha256", subject: "speed up digest", day: 99 },
    ];
    let plan = plan_split(&commits);
    set.add("f131f plan groups", plan.groups.len() == 2, "两组件两 PR");
    set.add("f131f plan order", plan.groups[1].2 == 99, "组内最早日正确");
    set.add("f131f single ok", pr_component_single(&commits[0..2]).is_ok(), "单组件放行");
    set.add("f131f cross reject", pr_component_single(&commits).is_err(), "跨组件拒绝");
    set.add("f131f empty reject", pr_component_single(&[]).is_err(), "空 PR 拒绝");

    // 评审轮次：改→再审→过 / 三轮升级 / 非法迁移
    let mut loop_a = ReviewLoop::new();
    let _ = loop_a.transition(ReviewStage::ChangesRequested);
    let _ = loop_a.transition(ReviewStage::ReReview);
    set.add("f131f re-review ok", loop_a.transition(ReviewStage::Approved).is_ok(), "再审后可过");
    let mut loop_b = ReviewLoop::new();
    for _ in 0..3 {
        let _ = loop_b.transition(ReviewStage::ChangesRequested);
        let _ = loop_b.transition(ReviewStage::ReReview);
    }
    set.add("f131f escalate 3 rounds", loop_b.escalated(), "三轮仍改→升级");
    let mut loop_c = ReviewLoop::new();
    set.add("f131f illegal skip", loop_c.transition(ReviewStage::ReReview).is_err(), "跳过 Changes 拒绝");

    // stale：89 天不 stale / 91 天 stale / 边界 90 不 stale
    set.add("f131f fresh", !stale_since(100, 189), "宽限内不 stale");
    set.add("f131f boundary", !stale_since(100, 190), "90 天整不 stale");
    set.add("f131f stale", stale_since(100, 191), "91 天 stale");

    // 标题 lint
    let good = lint_subject("fix: clamp receive window");
    set.add("f131f lint clean", good.issues.is_empty(), "合规标题零问题");
    let long = lint_subject("a very long commit subject that definitely exceeds the seventy two byte limit imposed");
    set.add("f131f lint long", long.issues.iter().any(|i| *i == "标题超 72 字节"), "超长检出");
    let wip = lint_subject("WIP: stuff");
    set.add("f131f lint wip", !wip.issues.is_empty(), "WIP 检出");
    let period = lint_subject("fix bug.");
    set.add("f131f lint period", period.issues.iter().any(|i| *i == "标题以句号结尾"), "句号检出");
    set.add("f131f lint empty", lint_subject("").issues.len() == 1, "空标题单问题");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn split_plan_groups_by_component() {
        let cs = [
            PendingCommit { component: "a", subject: "x", day: 5 },
            PendingCommit { component: "a", subject: "y", day: 6 },
            PendingCommit { component: "b", subject: "z", day: 4 },
        ];
        let p = plan_split(&cs);
        assert_eq!(p.groups.len(), 2);
        assert_eq!(p.groups[0], ("a", 2, 5));
        assert_eq!(p.groups[1], ("b", 1, 4));
    }

    #[test]
    fn review_loop_full_lifecycle() {
        let mut r = ReviewLoop::new();
        assert!(r.transition(ReviewStage::Approved).is_ok());
        let mut r2 = ReviewLoop::new();
        assert!(r2.transition(ReviewStage::ChangesRequested).is_ok());
        assert!(r2.transition(ReviewStage::Approved).is_err()); // 必须过 Re-review
        assert!(r2.transition(ReviewStage::ReReview).is_ok());
        assert!(r2.transition(ReviewStage::Approved).is_ok());
        assert_eq!(r2.rounds, 1);
    }
}
