//! 深化层 · F131 上游回馈通道（2026-09-26 回炉补深化）。
//!
//! 补深主册【设计细节】未实装机制：PR 拆分纪律（一个逻辑一个 PR）、
//! commit 规范对齐（入乡随俗四条）、升级窗冲突处置（回馈与锁版本
//! 冲突统一窗内处理）、90 天无响应维护分支公告（fork 责任三件）、
//! 回馈记录公开页数据源（F130 分区）。

use crate::checks::CheckSet;
use crate::stareco::upstream::{PrState, UpstreamBook, UpstreamPriority, UpstreamPr};

// ---------------------------------------------------------------------------
// PR 拆分纪律：一个逻辑一个 PR
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SplitCheck {
    /// 改动触达的逻辑变更域数（驱动/文档/运行时…）。
    pub domains_touched: u32,
    /// 逻辑变更数（修一个边界 = 1；顺手重构再 +1）。
    pub logical_changes: u32,
}

impl SplitCheck {
    /// 上游友好判定：单一变更域 + 单一逻辑才送出。
    pub fn split_verdict(&self) -> Result<(), &'static str> {
        if self.logical_changes == 0 {
            return Err("无逻辑变更：空 PR 不送");
        }
        if self.domains_touched == 0 {
            return Err("变更域未声明：送出前必须归类");
        }
        if self.logical_changes > 1 || self.domains_touched > 1 {
            return Err("一个逻辑一个 PR：请按变更域拆分后再送");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// commit 规范对齐（入乡随俗四条）
// ---------------------------------------------------------------------------

pub struct CommitLint {
    pub subject_len: usize,
    pub has_body: bool,
    pub references_issue: bool,
    pub signed_off: bool,
}

impl CommitLint {
    /// 主题 ≤72 字节 / 有正文 / 引用 issue / 带 sign-off。
    pub fn aligned(&self) -> bool {
        self.subject_len > 0
            && self.subject_len <= 72
            && self.has_body
            && self.references_issue
            && self.signed_off
    }

    /// 逐条诊断（修图指南式报错）。
    pub fn diagnostics(&self) -> alloc::vec::Vec<&'static str> {
        let mut out = alloc::vec::Vec::new();
        if self.subject_len == 0 || self.subject_len > 72 {
            out.push("主题行长度越界（1-72 字节）");
        }
        if !self.has_body {
            out.push("缺正文：动机与验证方式写进 body");
        }
        if !self.references_issue {
            out.push("未引用 issue：上游要求可溯");
        }
        if !self.signed_off {
            out.push("缺 Signed-off-by：CLA 纪律");
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 升级窗冲突：回馈与锁版本冲突 → 升级窗统一处理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionConflict {
    pub component: &'static str,
    pub pr_upstream_ver: u32,
    pub locked_ver: u32,
    pub next_window_day: u32,
}

impl VersionConflict {
    pub fn is_conflict(&self) -> bool {
        self.pr_upstream_ver != self.locked_ver
    }

    /// 冲突处置：只许排进下一升级窗（提前单独升版本 = 破节奏，拒绝）。
    pub fn resolution(&self, requested_day: u32) -> Result<u32, &'static str> {
        if !self.is_conflict() {
            return Ok(0);
        }
        if requested_day < self.next_window_day {
            return Err("锁版本冲突统一升级窗处理：不允许提前单独升");
        }
        Ok(self.next_window_day)
    }
}

// ---------------------------------------------------------------------------
// 维护分支公告（90 天静默 → fork 责任三件）
// ---------------------------------------------------------------------------

pub struct ForkAnnouncement {
    pub component: &'static str,
    pub upstream_last_response_day: u32,
    pub fork_branch: &'static str,
    pub has_diff_note: bool,
    pub has_resync_pledge: bool,
}

impl ForkAnnouncement {
    pub const SILENCE_DAYS: u32 = 90;

    pub fn publishable(&self, today: u32) -> Result<(), &'static str> {
        if today.saturating_sub(self.upstream_last_response_day) <= Self::SILENCE_DAYS {
            return Err("未满 90 天静默期：不发维护分支公告");
        }
        if self.fork_branch.is_empty() {
            return Err("分支名必填");
        }
        if !self.has_diff_note || !self.has_resync_pledge {
            return Err("公告三件缺一：最后响应/差异说明/回同步承诺");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 回馈记录公开页数据源（F130 分区）
// ---------------------------------------------------------------------------

pub struct PublicRow {
    pub component: &'static str,
    pub pr_url: &'static str,
    pub state: PrState,
    pub merged_in: Option<u32>,
}

/// 渲染公开行：Merged 带合入日，其余不带（诚实面）。
pub fn public_rows(book: &UpstreamBook) -> alloc::vec::Vec<PublicRow> {
    book.items_ref()
        .iter()
        .flatten()
        .map(|p| PublicRow {
            component: p.component,
            pr_url: p.pr_url,
            state: p.state,
            merged_in: if p.state == PrState::Merged { Some(p.closed_day) } else { None },
        })
        .collect()
}

/// 回馈优先级排程：安全件先送，同级按提交日先到先送。
pub fn schedule_order(items: &[(UpstreamPriority, u32)]) -> alloc::vec::Vec<usize> {
    let mut idx: alloc::vec::Vec<usize> = (0..items.len()).collect();
    idx.sort_by(|&a, &b| items[b].0.cmp(&items[a].0).then(items[a].1.cmp(&items[b].1)));
    idx
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131D_TAG: &str = "stareco-F131-deep";

pub fn run_f131_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F131D_TAG);

    set.add(
        "f131d single-logic passes",
        SplitCheck { domains_touched: 1, logical_changes: 1 }.split_verdict().is_ok(),
        "one logic one pr",
    );
    set.add(
        "f131d multi-logic split",
        SplitCheck { domains_touched: 2, logical_changes: 3 }.split_verdict().is_err(),
        "must split",
    );
    set.add(
        "f131d empty pr refused",
        SplitCheck { domains_touched: 1, logical_changes: 0 }.split_verdict().is_err(),
        "no empty pr",
    );

    let good = CommitLint { subject_len: 50, has_body: true, references_issue: true, signed_off: true };
    set.add("f131d commit aligned", good.aligned() && good.diagnostics().is_empty(), "four rules");
    let bad = CommitLint { subject_len: 90, has_body: false, references_issue: false, signed_off: false };
    set.add("f131d commit four diagnostics", bad.diagnostics().len() == 4, "逐条指出");

    let vc = VersionConflict { component: "smoltcp", pr_upstream_ver: 5, locked_ver: 4, next_window_day: 1000 };
    set.add("f131d conflict detected", vc.is_conflict(), "ver differs");
    set.add(
        "f131d early bump refused",
        vc.resolution(900).is_err() && vc.resolution(1000) == Ok(1000),
        "window-only",
    );
    let aligned = VersionConflict { pr_upstream_ver: 4, ..vc };
    set.add("f131d no conflict passthrough", aligned.resolution(1) == Ok(0), "aligned ver");

    let ann = ForkAnnouncement {
        component: "ext4-rs",
        upstream_last_response_day: 0,
        fork_branch: "varix/ext4-maint",
        has_diff_note: true,
        has_resync_pledge: true,
    };
    set.add(
        "f131d silence window enforced",
        ann.publishable(60).is_err() && ann.publishable(91).is_ok(),
        "90d line",
    );
    let mut incomplete = ann;
    incomplete.has_resync_pledge = false;
    set.add(
        "f131d three-piece announcement",
        incomplete.publishable(200).is_err(),
        "diff+resync required",
    );

    let mut book = UpstreamBook::new();
    book.record(UpstreamPr {
        component: "smoltcp",
        pr_url: "u1",
        priority: UpstreamPriority::Correctness,
        state: PrState::Merged,
        opened_day: 10,
        closed_day: 20,
        note: "",
    })
    .ok();
    book.record(UpstreamPr {
        component: "limine",
        pr_url: "u2",
        priority: UpstreamPriority::Perf,
        state: PrState::Ready,
        opened_day: 10,
        closed_day: 0,
        note: "",
    })
    .ok();
    let rows = public_rows(&book);
    set.add(
        "f131d public rows honest",
        rows.len() == 2 && rows[0].merged_in.is_some() && rows[1].merged_in.is_none(),
        "merged carries day; draft does not",
    );

    // 排程：安全 > 正确性 > 性能，同级先到先送
    let order = schedule_order(&[
        (UpstreamPriority::Perf, 1),
        (UpstreamPriority::Security, 9),
        (UpstreamPriority::Security, 2),
        (UpstreamPriority::Correctness, 3),
    ]);
    set.add(
        "f131d schedule s>c>p then fifo",
        order == alloc::vec![2usize, 1, 3, 0],
        "security first, fifo within tier",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn split_window_schedule() {
        assert!(SplitCheck { domains_touched: 1, logical_changes: 1 }.split_verdict().is_ok());
        let vc = VersionConflict { component: "c", pr_upstream_ver: 9, locked_ver: 8, next_window_day: 500 };
        assert!(vc.resolution(499).is_err());
        assert_eq!(vc.resolution(500), Ok(500));
        let order = schedule_order(&[(UpstreamPriority::Perf, 1), (UpstreamPriority::Security, 2)]);
        assert_eq!(order, alloc::vec![1usize, 0]);
    }
}
