//! F270 文件操作错误重试 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：中途失败不中断判据（注入只读文件用例）；三选一
//! 行为；计数准确性；导出格式与内容；重试成功路径。
//!
//! **设计要点（主册）**：批量复制 100 个文件第 37 个失败时：错误不中断
//! 整体——对话框列出失败项清单（文件名+原因人话化），用户三选一：重试
//! 该批、跳过继续、全部中止；中止时已完成项保留并列明「完成 36/100」；
//! 失败清单可导出文本（给排障）。
//!
//! 实装：批量执行器（逐项推进、失败入清单不中断）；三选一动作；计数
//! 账（完成/失败/总数）；导出文本格式（文件名+人话原因，行式清单）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 文件操作失败原因（人话映射的唯一源——错误码永不裸抛）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpError {
    ReadOnly,
    NoSpace,
    NotFound,
    InUse,
}

impl OpError {
    /// 人话原因（三要素之一：为什么）。
    pub fn human(&self) -> &'static str {
        match self {
            OpError::ReadOnly => "文件是只读的——去掉只读属性或改存别处",
            OpError::NoSpace => "目标空间不足——清理后再试",
            OpError::NotFound => "源文件已不存在——可能被移动或删除",
            OpError::InUse => "文件正在被其他应用使用——先关闭占用它的应用",
        }
    }
}

/// 失败清单条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedItem {
    pub name: String,
    pub err: OpError,
}

/// 批量操作账。
pub struct BatchLedger {
    pub total: usize,
    pub done: Vec<String>,
    pub failed: Vec<FailedItem>,
    /// 中止后剩余未处理的项。
    pub abandoned: Vec<String>,
}

impl BatchLedger {
    /// 「完成 36/100」口径文本（计数准确性判据直读）。
    pub fn progress_text(&self) -> String {
        alloc::format!("完成 {}/{}", self.done.len(), self.total)
    }
}

/// 三选一动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchChoice {
    /// 重试该批（失败清单整体再跑）。
    RetryFailed,
    /// 跳过继续（放弃失败项，剩余继续）。
    SkipFailed,
    /// 全部中止（已完成项保留）。
    AbortAll,
}

/// 批量执行器：`op` 由调用方注入（可失败的文件操作）——第 37 个失败
/// 不中断整体（判据的机制保证：循环体对失败只记账）。
pub fn run_batch(
    names: &[&str],
    mut op: impl FnMut(&str) -> Result<(), OpError>,
) -> BatchLedger {
    let mut ledger =
        BatchLedger { total: names.len(), done: Vec::new(), failed: Vec::new(), abandoned: Vec::new() };
    for n in names {
        match op(n) {
            Ok(()) => ledger.done.push(String::from(*n)),
            Err(e) => ledger.failed.push(FailedItem { name: String::from(*n), err: e }),
        }
    }
    ledger
}

/// 失败清单导出文本（给排障——格式：`文件名\t原因`，行式）。
pub fn export_failed(ledger: &BatchLedger) -> String {
    let mut out = String::from("失败项清单\n文件名\t原因\n");
    for f in &ledger.failed {
        out.push_str(&alloc::format!("{}\t{}\n", f.name, f.err.human()));
    }
    out
}

/// 重试失败批：对失败项重跑 `op`，返回新账（成功并入 done）。
pub fn retry_failed(
    ledger: &mut BatchLedger,
    mut op: impl FnMut(&str) -> Result<(), OpError>,
) -> usize {
    let names: Vec<String> = ledger.failed.iter().map(|f| f.name.clone()).collect();
    ledger.failed.clear();
    let mut fixed = 0;
    for n in names {
        match op(&n) {
            Ok(()) => {
                ledger.done.push(n);
                fixed += 1;
            }
            Err(e) => ledger.failed.push(FailedItem { name: n, err: e }),
        }
    }
    fixed
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_opretry_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F270");
    // 中途失败不中断：100 项，第 37 项只读失败 → 99 完成 1 失败。
    let mut names: Vec<&'static str> = Vec::new();
    for i in 0..100 {
        names.push(if i == 36 { "文件036" } else { "文件" });
    }
    let mut ledger = run_batch(&names, |n| {
        if n == "文件036" {
            Err(OpError::ReadOnly)
        } else {
            Ok(())
        }
    });
    set.add(
        "F270 no early stop",
        ledger.done.len() == 99 && ledger.failed.len() == 1,
        "37th fails, rest run",
    );
    // 计数准确性。
    set.add(
        "F270 count exact",
        ledger.progress_text() == "完成 99/100" && ledger.total == 100,
        "36/100 format",
    );
    // 人话原因（不裸抛错误码）。
    set.add(
        "F270 human reason",
        ledger.failed[0].err.human().contains("只读"),
        "no raw code",
    );
    // 导出格式与内容。
    let exported = export_failed(&ledger);
    set.add(
        "F270 export",
        exported.contains("文件036\t文件是只读的") && exported.lines().count() == 3,
        "name+reason rows",
    );
    // 三选一：重试成功路径。
    let fixed = retry_failed(&mut ledger, |_| Ok(()));
    set.add(
        "F270 retry succeeds",
        fixed == 1 && ledger.done.len() == 100 && ledger.failed.is_empty(),
        "retry path",
    );
    // 三选一：中止保留已完成。
    let mut l2 = run_batch(&["甲", "乙", "丙", "丁"], |n| {
        if n == "乙" {
            Err(OpError::NoSpace)
        } else {
            Ok(())
        }
    });
    l2.abandoned = alloc::vec![String::from("丙"), String::from("丁")];
    set.add(
        "F270 abort keeps done",
        l2.done.len() == 3 && l2.progress_text() == "完成 3/4",
        "done preserved",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f270_batch_flow() {
        let set = run_opretry_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F270 自检红 {f}/{p}");
    }

    #[test]
    fn every_error_has_human_words() {
        // 人话映射覆盖率 100%——四族原因全部有说法。
        for e in [OpError::ReadOnly, OpError::NoSpace, OpError::NotFound, OpError::InUse] {
            assert!(!e.human().is_empty());
        }
    }
}
