//! 深化层 · F557 换机迁移助手（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F557 节）：
//! ①「迁移失败可重试（断点续传 F269 同源）」的**断点账**——失败时
//!   快照「已完成资产序号」，重试从第一个未完成资产继续（不许从头
//!   重搬已完成件）；断点与基础层 moved 位对账；
//! ②「迁移中源盘只读（防双写事故）」的**守卫账**——只读窗口内的每
//!   一次写尝试都拦下并逐笔记账（结构性保护要有拦截证据，不是口号）；
//! ③「迁移报告」的**结构化卡**——六类资产 × (扫描/勾选/已迁/首开校验)
//!   四态矩阵，报告的每一格都有账（报告不是一句「完成」，是可抽查的格）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::migmate::{AssetKind, MigMate, MigState, ASSET_KINDS};

// ---------------------------------------------------------------------------
// 断点账（断点续传）
// ---------------------------------------------------------------------------

/// 断点账：失败时刻的续传起点 = 第一个未迁完的勾选资产。
pub struct ResumeLedger {
    /// 勾选资产名（勾选序）。
    names: alloc::vec::Vec<String>,
    /// 对应 moved 位。
    moved: alloc::vec::Vec<bool>,
    /// 续传游标（下一待迁下标）。
    cursor: usize,
    taken: u32,
}

impl ResumeLedger {
    pub fn new() -> ResumeLedger {
        ResumeLedger { names: alloc::vec::Vec::new(), moved: alloc::vec::Vec::new(), cursor: 0, taken: 0 }
    }

    /// 失败时打快照：游标落在**第一个未完成**的勾选资产上。
    pub fn snapshot_on_fail(&mut self, picked_names: &[&str], moved_flags: &[bool]) {
        self.names = picked_names.iter().map(|s| String::from(*s)).collect();
        self.moved = moved_flags.to_vec();
        self.cursor = self
            .moved
            .iter()
            .position(|&m| !m)
            .unwrap_or(self.moved.len());
        self.taken = 0;
    }

    /// 重试起点（None = 全部已完成，无可续传）。
    pub fn resume_from(&self) -> Option<usize> {
        if self.cursor < self.names.len() {
            Some(self.cursor)
        } else {
            None
        }
    }

    /// 续传执行一步：从游标起找下一个未完成件（**跳过已完成**——
    /// 不从头重搬），返回其下标。
    pub fn take_next(&mut self) -> Option<usize> {
        while self.cursor < self.names.len() && self.moved[self.cursor] {
            self.cursor += 1;
        }
        if self.cursor < self.names.len() {
            let idx = self.cursor;
            self.cursor += 1;
            self.taken += 1;
            Some(idx)
        } else {
            None
        }
    }

    /// 已完成清单（断点前的既迁资产——对账口）。
    pub fn done_names(&self) -> alloc::vec::Vec<&str> {
        self.names
            .iter()
            .zip(self.moved.iter())
            .filter(|(_, m)| **m)
            .map(|(n, _)| n.as_str())
            .collect()
    }
}

impl Default for ResumeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 源盘只读守卫账
// ---------------------------------------------------------------------------

/// 源盘写尝试拦截账（只读窗口内每次写尝试都留痕）。
pub struct SourceGuard {
    attempts: u32,
    blocked: u32,
    /// 窗口开着吗（Transferring 态 = 开）。
    window_open: bool,
}

impl SourceGuard {
    pub fn new() -> SourceGuard {
        SourceGuard { attempts: 0, blocked: 0, window_open: false }
    }

    /// 窗口开关（与 MigState::Transferring 同步）。
    pub fn set_window(&mut self, open: bool) {
        self.window_open = open;
    }

    /// 源盘写尝试：窗口开着 → 一律拦下并记账（返回 false = 拒绝写入）。
    pub fn write_attempt(&mut self, _path: &str) -> bool {
        self.attempts += 1;
        if self.window_open {
            self.blocked += 1;
            false
        } else {
            true
        }
    }

    /// 守卫有效性：窗口内全部写尝试都被拦（一条漏网即假保护）。
    pub fn all_blocked(&self) -> bool {
        !self.window_open || (self.attempts > 0 && self.blocked == self.attempts)
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }
}

impl Default for SourceGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 结构化迁移报告
// ---------------------------------------------------------------------------

/// 六类 × 四态矩阵的一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportCell {
    pub kind: AssetKind,
    pub scanned: bool,
    pub picked: u32,
    pub transferred: u32,
    pub verified: bool,
}

/// 迁移报告（六行，每行四态——报告的每一格都有账）。
pub struct ReportCard {
    pub rows: [ReportCell; 6],
}

impl ReportCard {
    pub fn build(mate: &MigMate) -> ReportCard {
        let mut rows = [ReportCell {
            kind: ASSET_KINDS[0],
            scanned: false,
            picked: 0,
            transferred: 0,
            verified: false,
        }; 6];
        for (i, &k) in ASSET_KINDS.iter().enumerate() {
            let stats = mate.kind_stats(k);
            rows[i] = ReportCell {
                kind: k,
                scanned: stats.0,
                picked: stats.1,
                transferred: stats.2,
                verified: stats.3,
            };
        }
        ReportCard { rows }
    }

    /// 完整性：六类全部扫描过（缺一类 = 报告不完整，立红）。
    pub fn scan_complete(&self) -> bool {
        self.rows.iter().all(|r| r.scanned)
    }

    /// 按类取行。
    pub fn row(&self, kind: AssetKind) -> &ReportCell {
        self.rows.iter().find(|r| r.kind == kind).unwrap()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f557_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 断点账：3 勾 2 完成 → 续传从第 3 个起；跳过已完成件。
    let mut rl = ResumeLedger::new();
    rl.snapshot_on_fail(&["主题包", "词库", "文档"], &[true, true, false]);
    let first = rl.take_next();
    let second = rl.take_next();
    cs.add(
        "resume from first undone",
        first == Some(2) && second.is_none() && rl.done_names().len() == 2,
        "",
    );

    // 2) 全完成时无可续传（诚实：没有断点就没有「重搬」）。
    let mut rl2 = ResumeLedger::new();
    rl2.snapshot_on_fail(&["a", "b"], &[true, true]);
    cs.add("all done nothing to resume", rl2.resume_from().is_none() && rl2.take_next().is_none(), "");

    // 3) 源盘只读守卫：窗口内写尝试全拦、窗口外放行；守卫账可证明。
    let mut guard = SourceGuard::new();
    guard.set_window(true);
    let b1 = guard.write_attempt("源盘:/文档/a.docx");
    let b2 = guard.write_attempt("源盘:/主题/x.vpth");
    guard.set_window(false);
    let b3 = guard.write_attempt("目标盘:/a.docx");
    cs.add(
        "source guard blocks all writes in window",
        !b1 && !b2 && b3 && guard.all_blocked() && guard.blocked() == 2 && guard.attempts() == 3,
        "",
    );

    // 4) 报告卡：六类齐扫 → scan_complete；缺类立红。
    let mut mate = MigMate::new();
    for (i, &k) in ASSET_KINDS.iter().enumerate() {
        let _ = mate.scan(k, &alloc::format!("资产{}", i), 100);
    }
    let card_full = ReportCard::build(&mate);
    let mut mate2 = MigMate::new();
    for (i, &k) in ASSET_KINDS.iter().enumerate() {
        if i < 5 {
            let _ = mate2.scan(k, &alloc::format!("资产{}", i), 100);
        }
    }
    let card_hole = ReportCard::build(&mate2);
    cs.add(
        "report card completeness",
        card_full.scan_complete() && !card_hole.scan_complete(),
        "",
    );

    // 5) 报告卡行对账：文件类的 picked/transferred 与基础层账一致。
    let _ = mate.pick("资产0", true);
    let _ = mate.begin_transfer();
    while mate.transfer_one() {}
    let card = ReportCard::build(&mate);
    cs.add(
        "report row mirrors base ledger",
        card.row(ASSET_KINDS[0]).picked == 1 && card.row(ASSET_KINDS[0]).transferred == 1,
        "",
    );

    // 6) 失败重试后状态回 Transferring（断点保留的语义前提）。
    //    begin_transfer 有完整门槛（六类齐扫）——完整走一遍前置。
    let mut mate3 = MigMate::new();
    for (i, &k) in ASSET_KINDS.iter().enumerate() {
        let _ = mate3.scan(k, &alloc::format!("资产{}", i), 100);
    }
    let _ = mate3.pick("资产0", true);
    let _ = mate3.begin_transfer();
    let _ = mate3.fail();
    cs.add(
        "failed state kept for retry",
        mate3.state == MigState::Failed && mate3.retry() && mate3.retry_count() == 1,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_ledger_skips_done() {
        let mut rl = ResumeLedger::new();
        rl.snapshot_on_fail(&["a", "b", "c", "d"], &[true, false, true, false]);
        assert_eq!(rl.take_next(), Some(1));
        assert_eq!(rl.take_next(), Some(3));
        assert_eq!(rl.take_next(), None);
    }
}
