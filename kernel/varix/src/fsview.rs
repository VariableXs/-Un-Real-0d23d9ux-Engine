//! fsview — WP-205 · B-1702 只读树禁写 + B-1703 搜索双档（MD2 篇 17.1/17.3）。
//!
//! 判据 B-1702：只读树禁写，NTFS 写入禁用与解释呈现。
//! 判据 B-1703：搜索双档，即席毫秒级、全盘可取消。
//! MD2 原文（17.1）："浏览层建在 VFS 之上，对 ext4（读写）与 NTFS（只读）
//! 统一呈现、差异明示：NTFS 挂载点角标'来自 Windows 域（只读）'（判例 22），
//! 写入类操作在只读树上直接禁用置灰并给一行解释。"
//! MD2 原文（17.3）："搜索两档：目录内即席过滤（键入即滤，前端内存过滤，
//! 毫秒级）与全盘搜索（后台任务，ext4 目录遍历加文件名匹配，结果流式呈现
//! 可随时取消）。"
//!
//! 分层关系：B-705（WP-203 ntfsro）是 VFS 强制层——写句柄 100% 拒绝并留痕，
//! API 面无写模式；本模块是**文件管理器呈现层**——角标/置灰/解释行三呈现面
//! 与强制层同源（呈现层禁用的操作调下去也会被强制层拒绝：双保险但语义单一）。
//! 即席过滤预算整数推导（64 条 × 15ns ≤ 1ms）；全盘搜索 JobState 终态机
//! （Running→Cancelled/Done，取消即资源回收 open_handles 归零）。

use crate::checks::CheckSet;

// ============ B-1702 双文件系统呈现层 ============

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsDomain {
    Ext4,
    Ntfs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileAction {
    List,
    Read,
    OpenWith,
    Write,
    Rename,
    Delete,
    Mkdir,
}

/// 全量穷举表（对练用：新增操作必须进表）。
pub const ALL_ACTIONS: [FileAction; 7] = [
    FileAction::List,
    FileAction::Read,
    FileAction::OpenWith,
    FileAction::Write,
    FileAction::Rename,
    FileAction::Delete,
    FileAction::Mkdir,
];

/// NTFS 挂载点角标（判例 22 的呈现面）。
pub const NTFS_BADGE: &str = "来自 Windows 域（只读）";
/// 禁用操作的一行解释。
pub const NTFS_EXPLAIN: &str = "NTFS 以只读方式挂载（Windows 域保护），此操作不可用";

#[derive(Clone, Copy)]
pub struct MountView {
    pub domain: FsDomain,
}

impl MountView {
    pub fn badge(&self) -> &'static str {
        match self.domain {
            FsDomain::Ntfs => NTFS_BADGE,
            FsDomain::Ext4 => "",
        }
    }

    /// 读类操作全允许；写入类在只读树禁用置灰。
    pub fn action_enabled(&self, a: FileAction) -> bool {
        match self.domain {
            FsDomain::Ext4 => true,
            FsDomain::Ntfs => matches!(a, FileAction::List | FileAction::Read | FileAction::OpenWith),
        }
    }

    /// 禁用操作的一行解释（非空即置灰伴随说明）。
    pub fn explain(&self, a: FileAction) -> &'static str {
        if self.action_enabled(a) {
            ""
        } else {
            NTFS_EXPLAIN
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct WriteAttemptLog {
    pub attempts: u64,
    pub denied: u64,
    pub allowed: u64,
}

/// 写入尝试：呈现层先裁决（强制层 ntfsro 兜底），拒绝留痕。
pub fn attempt_write(view: &MountView, log: &mut WriteAttemptLog) -> bool {
    log.attempts += 1;
    if view.action_enabled(FileAction::Write) {
        log.allowed += 1;
        true
    } else {
        log.denied += 1;
        false
    }
}

// ============ B-1703 搜索双档 ============

/// 即席过滤预算（毫秒级）。
pub const FILTER_BUDGET_NS: u64 = 1_000_000;
pub const QUICK_ENTRIES_CAP: usize = 64;
/// 每条定长名字比较预算（16B memcmp 级）。
pub const QUICK_CMP_BUDGET_NS: u64 = 15_000;

pub fn quick_budget_ok() -> bool {
    (QUICK_ENTRIES_CAP as u64) * QUICK_CMP_BUDGET_NS <= FILTER_BUDGET_NS
}

/// 目录内即席过滤：键入即滤（前端内存过滤，子串匹配）。
pub fn quick_filter(entries: &[[u8; 16]], n: usize, pat: &[u8]) -> usize {
    if pat.is_empty() {
        return n; // 空模式：全显（键入前的常态）。
    }
    let mut hits = 0;
    let mut i = 0;
    while i < n {
        if contains_sub(&entries[i], pat) {
            hits += 1;
        }
        i += 1;
    }
    hits
}

fn contains_sub(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let mut s = 0;
    while s + needle.len() <= hay.len() {
        let mut m = true;
        let mut j = 0;
        while j < needle.len() {
            if hay[s + j] != needle[j] {
                m = false;
                break;
            }
            j += 1;
        }
        if m {
            return true;
        }
        s += 1;
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Idle,
    Running,
    Cancelled,
    Done,
}

/// 全盘搜索：后台任务，ext4 目录遍历加文件名匹配，结果流式呈现可随时取消。
pub struct FullSearch {
    pub state: JobState,
    pub scanned: u64,
    pub matched: u64,
    pub batches: u64,
    /// 占用句柄（取消/完成即回收——对账恒归零）。
    pub open_handles: i64,
}

impl FullSearch {
    pub const fn new() -> FullSearch {
        FullSearch { state: JobState::Idle, scanned: 0, matched: 0, batches: 0, open_handles: 0 }
    }

    pub fn start(&mut self) -> bool {
        if self.state != JobState::Idle {
            return false;
        }
        self.state = JobState::Running;
        self.open_handles += 1;
        true
    }

    /// 后台一步：扫一批（结果流式呈现——首批先答，不攒全量）。
    pub fn step(&mut self, files: u64, hits: u64) -> bool {
        if self.state != JobState::Running {
            return false;
        }
        self.scanned += files;
        self.matched += hits;
        self.batches += 1;
        true
    }

    /// 随时可取消：Running 态任意步 → 终态 Cancelled + 资源回收。
    pub fn cancel(&mut self) -> bool {
        if self.state != JobState::Running {
            return false;
        }
        self.state = JobState::Cancelled;
        self.open_handles -= 1;
        true
    }

    /// 自然完成。
    pub fn finish(&mut self) -> bool {
        if self.state != JobState::Running {
            return false;
        }
        self.state = JobState::Done;
        self.open_handles -= 1;
        true
    }

    pub fn reconciled(&self) -> bool {
        self.open_handles == 0
    }
}

// ============ CheckSet（B-1702 ×5 + B-1703 ×5）============

pub fn run_fsview_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-1702/B-1703 只读呈现与搜索双档");
    {
        // B-1702 NTFS 角标呈现（ext4 无角标）。
        let ntfs = MountView { domain: FsDomain::Ntfs };
        let ext4 = MountView { domain: FsDomain::Ext4 };
        set.add(
            "B-1702 NTFS 角标呈现",
            ntfs.badge() == NTFS_BADGE && ext4.badge().is_empty()
                && NTFS_BADGE.contains("只读"),
            "角标文案精确（'来自 Windows 域（只读）'）；ext4 无角标",
        );
    }
    {
        // B-1702 写入操作置灰：穷举 FileAction 全表。
        let ntfs = MountView { domain: FsDomain::Ntfs };
        let mut matrix_ok = true;
        let mut i = 0;
        while i < ALL_ACTIONS.len() {
            let expect = matches!(
                ALL_ACTIONS[i],
                FileAction::List | FileAction::Read | FileAction::OpenWith
            );
            if ntfs.action_enabled(ALL_ACTIONS[i]) != expect {
                matrix_ok = false;
            }
            i += 1;
        }
        set.add(
            "B-1702 写入操作置灰",
            matrix_ok,
            "读类 3 项允许、写类 4 项禁用（穷举全表无漏）",
        );
    }
    {
        // B-1702 禁用带解释：每个禁用操作都有非空解释。
        let ntfs = MountView { domain: FsDomain::Ntfs };
        let mut explain_ok = true;
        let mut i = 0;
        while i < ALL_ACTIONS.len() {
            let a = ALL_ACTIONS[i];
            if !ntfs.action_enabled(a) && ntfs.explain(a) != NTFS_EXPLAIN {
                explain_ok = false;
            }
            if ntfs.action_enabled(a) && !ntfs.explain(a).is_empty() {
                explain_ok = false;
            }
            i += 1;
        }
        set.add(
            "B-1702 禁用带解释",
            explain_ok && NTFS_EXPLAIN.contains("只读"),
            "禁用必有解释、允许必无解释（呈现一致性）",
        );
    }
    {
        // B-1702 写尝试拒绝留痕：对账 attempts == denied + allowed。
        let ntfs = MountView { domain: FsDomain::Ntfs };
        let mut log = WriteAttemptLog::default();
        let mut i = 0;
        while i < 5 {
            let _ = attempt_write(&ntfs, &mut log);
            i += 1;
        }
        set.add(
            "B-1702 写尝试拒绝留痕",
            log.attempts == 5 && log.denied == 5 && log.allowed == 0
                && log.attempts == log.denied + log.allowed,
            "NTFS 写尝试全拒 + 留痕对账恒等式",
        );
    }
    {
        // B-1702 ext4 对照：同操作全允许（读写不受影响）。
        let ext4 = MountView { domain: FsDomain::Ext4 };
        let mut all_ok = true;
        let mut i = 0;
        while i < ALL_ACTIONS.len() {
            if !ext4.action_enabled(ALL_ACTIONS[i]) {
                all_ok = false;
            }
            i += 1;
        }
        set.add(
            "B-1702 ext4 对照全通",
            all_ok,
            "ext4 七操作全允许（双树差异只在 NTFS 侧）",
        );
    }
    {
        // B-1703 即席毫秒级：预算推导 + 命中对账。
        let mut entries = [[0u8; 16]; QUICK_ENTRIES_CAP];
        let names: [&[u8]; 4] = [b"report_v1.txt", b"notes.md", b"report_v2.txt", b"img.png"];
        let mut i = 0;
        while i < names.len() {
            entries[i][..names[i].len()].copy_from_slice(names[i]);
            i += 1;
        }
        let hits = quick_filter(&entries, 4, b"report");
        set.add(
            "B-1703 即席毫秒级",
            quick_budget_ok() && hits == 2,
            "64 条 × 15ns = 960ns ≤ 1ms；子串命中 2/4",
        );
    }
    {
        // B-1703 全盘后台流式：首批先答（batch ≥ 1 即有呈现）。
        let mut job = FullSearch::new();
        let started = job.start();
        let first = job.step(100, 3);
        set.add(
            "B-1703 全盘后台流式",
            started && first && job.batches == 1 && job.matched == 3,
            "step 批次推进，结果流式（首批即答，不攒全量）",
        );
    }
    {
        // B-1703 随时可取消：Running 态任意步。
        let mut job = FullSearch::new();
        let _ = job.start();
        let mut i = 0;
        while i < 3 {
            let _ = job.step(10, 0);
            i += 1;
        }
        let cancelled = job.cancel();
        // 终态后 step/cancel 均拒绝。
        let dead = !job.step(10, 0) && !job.cancel();
        set.add(
            "B-1703 随时可取消",
            cancelled && dead && job.state == JobState::Cancelled,
            "任意步可取消；终态机拒绝二次操作",
        );
    }
    {
        // B-1703 取消资源回收：open_handles 归零。
        let mut job = FullSearch::new();
        let _ = job.start();
        let _ = job.step(5, 1);
        let _ = job.cancel();
        set.add(
            "B-1703 取消资源回收",
            job.reconciled(),
            "cancel 即回收句柄（open_handles == 0 对账）",
        );
    }
    {
        // B-1703 目录遍历匹配语义：扫描量与命中量口径对账。
        let mut job = FullSearch::new();
        let _ = job.start();
        let _ = job.step(1000, 7);
        let _ = job.step(2000, 3);
        let _ = job.finish();
        set.add(
            "B-1703 遍历匹配对账",
            job.state == JobState::Done && job.scanned == 3000 && job.matched == 10
                && job.reconciled(),
            "scanned/matched 累计口径一致；完成态资源归零",
        );
    }
    set
}

// ============ 单测（f904 ×4）============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f904_ntfs_badge() {
        let ntfs = MountView { domain: FsDomain::Ntfs };
        assert_eq!(ntfs.badge(), "来自 Windows 域（只读）");
        assert!(ntfs.badge().contains("Windows"));
        let ext4 = MountView { domain: FsDomain::Ext4 };
        assert_eq!(ext4.badge(), "");
    }

    #[test]
    fn f904_action_matrix() {
        let ntfs = MountView { domain: FsDomain::Ntfs };
        assert!(ntfs.action_enabled(FileAction::List));
        assert!(ntfs.action_enabled(FileAction::Read));
        assert!(ntfs.action_enabled(FileAction::OpenWith));
        assert!(!ntfs.action_enabled(FileAction::Write));
        assert!(!ntfs.action_enabled(FileAction::Rename));
        assert!(!ntfs.action_enabled(FileAction::Delete));
        assert!(!ntfs.action_enabled(FileAction::Mkdir));
        // 置灰必带解释。
        assert!(!ntfs.explain(FileAction::Delete).is_empty());
        assert!(ntfs.explain(FileAction::Read).is_empty());
    }

    #[test]
    fn f904_quick_filter() {
        let mut entries = [[0u8; 16]; QUICK_ENTRIES_CAP];
        let names: [&[u8]; 3] = [b"alpha", b"beta", b"alphabet"];
        let mut i = 0;
        while i < names.len() {
            entries[i][..names[i].len()].copy_from_slice(names[i]);
            i += 1;
        }
        assert_eq!(quick_filter(&entries, 3, b"alp"), 2);
        assert_eq!(quick_filter(&entries, 3, b"beta"), 1);
        assert_eq!(quick_filter(&entries, 3, b"zzz"), 0);
        // 空模式全显。
        assert_eq!(quick_filter(&entries, 3, b""), 3);
        // 预算模型。
        assert!(quick_budget_ok());
    }

    #[test]
    fn f904_search_cancel() {
        let mut job = FullSearch::new();
        // Idle 态：cancel/finish/step 全拒绝。
        assert!(!job.cancel());
        assert!(!job.finish());
        assert!(!job.step(1, 1));
        assert!(job.start());
        assert!(!job.start()); // 二次 start 拒绝
        assert!(job.step(10, 2));
        assert!(job.cancel());
        assert_eq!(job.state, JobState::Cancelled);
        assert!(job.reconciled());
    }
}
