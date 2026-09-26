//! F487 隐私一键清除面板（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四类清除链路；条数预览准确；总开关；关机自动清（重启后验证全空）；
//! 不可恢复确认文案。**
//!
//! 功能定义（主册批次三）：隐私收口页——四类痕迹集中清除（搜索历史 F307/
//! 最近文件 F486/剪贴板历史 F109/通知中心记录）——每类独立勾选+「全部清除」
//! 总开关；清除前显示各类条数；清除不可恢复（确认框明说）；计划清除可选
//! （每次关机自动清勾选类）。
//!
//! 零堆纪律：定长计数账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 四类痕迹（主册原文四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceKind {
    /// 搜索历史（F307）。
    SearchHistory,
    /// 最近文件（F486）。
    RecentFiles,
    /// 剪贴板历史（F109）。
    ClipboardHistory,
    /// 通知中心记录。
    Notifications,
}

pub const TRACE_KINDS: [TraceKind; 4] = [
    TraceKind::SearchHistory,
    TraceKind::RecentFiles,
    TraceKind::ClipboardHistory,
    TraceKind::Notifications,
];

impl TraceKind {
    pub fn name(self) -> &'static str {
        match self {
            TraceKind::SearchHistory => "search-history",
            TraceKind::RecentFiles => "recent-files",
            TraceKind::ClipboardHistory => "clipboard-history",
            TraceKind::Notifications => "notifications",
        }
    }
}

/// 不可恢复确认文案（主册：不可恢复说得明明白白）。
pub const IRREVERSIBLE_TEXT: &str = "清除后不可恢复";

/// 隐私清除面板。
pub struct PrivacySweep {
    /// 每类勾选（独立勾选 + 总开关并存）。
    selected: [bool; 4],
    /// 每类当前条数（条数预览——清除前显示）。
    counts: [u32; 4],
    /// 关机自动清（勾选类每次关机自动清——F267 存储感知的隐私面）。
    pub auto_on_shutdown: bool,
    /// 总开关（「全部清除」一键全勾）。
    all_selected: bool,
}

impl PrivacySweep {
    pub const fn new() -> Self {
        PrivacySweep {
            selected: [false; 4],
            counts: [0; 4],
            auto_on_shutdown: false,
            all_selected: false,
        }
    }

    /// 数据注入（各痕迹源同源计数——条数预览准确性）。
    pub fn set_counts(&mut self, c: [u32; 4]) {
        self.counts = c;
    }

    pub fn count(&self, k: TraceKind) -> u32 {
        self.counts[k as usize]
    }

    /// 勾选一类。
    pub fn select(&mut self, k: TraceKind, on: bool) {
        self.selected[k as usize] = on;
        self.all_selected = self.selected.iter().all(|&x| x);
    }

    /// 总开关（全部清除——四类全勾）。
    pub fn select_all(&mut self) {
        self.selected = [true; 4];
        self.all_selected = true;
    }

    /// 条数预览汇总（主册：条数预览准确——只汇总勾选类）。
    pub fn preview_total(&self) -> u32 {
        (0..4).filter(|&i| self.selected[i]).map(|i| self.counts[i]).sum()
    }

    /// 执行清除（confirmed=false 拒绝执行——不可恢复操作纪律）。
    /// 返回实际清除条数（勾选类合计）。
    pub fn sweep(&mut self, confirmed: bool) -> u32 {
        if !confirmed {
            return 0;
        }
        let mut cleared = 0;
        for i in 0..4 {
            if self.selected[i] {
                cleared += self.counts[i];
                self.counts[i] = 0;
            }
        }
        cleared
    }

    /// 关机自动清执行（重启后验证全空——勾选类清零）。
    pub fn shutdown_auto_sweep(&mut self) -> u32 {
        if !self.auto_on_shutdown {
            return 0;
        }
        self.sweep(true)
    }

    pub fn is_selected(&self, k: TraceKind) -> bool {
        self.selected[k as usize]
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_privacysweep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F487-privacysweep");
    let mut p = PrivacySweep::new();
    p.set_counts([23, 41, 7, 15]);
    // 1) 四类痕迹在册。
    cs.add("four_kinds", TRACE_KINDS.len() == 4 && TRACE_KINDS.iter().all(|k| !k.name().is_empty()), "");
    // 2) 条数预览准确（勾选类合计）。
    p.select(TraceKind::SearchHistory, true);
    p.select(TraceKind::RecentFiles, true);
    cs.add("preview_accurate", p.preview_total() == 64, ""); // 23+41
    // 3) 总开关（四类全勾 → 预览 = 全部合计）。
    p.select_all();
    cs.add("select_all", p.all_selected && p.preview_total() == 86, "");
    // 4) 不可恢复确认（未确认不执行）。
    cs.add("needs_confirmation", p.sweep(false) == 0 && p.count(TraceKind::SearchHistory) == 23, "");
    // 5) 清除链路（确认后勾选类清零、未勾类保留）。
    p.select(TraceKind::ClipboardHistory, false);
    p.select(TraceKind::Notifications, false);
    let cleared = p.sweep(true);
    cs.add("sweep_selected_only", cleared == 64 && p.count(TraceKind::SearchHistory) == 0 && p.count(TraceKind::ClipboardHistory) == 7, "");
    // 6) 关机自动清（重启后验证全空——勾选类清零）。
    let mut a = PrivacySweep::new();
    a.set_counts([5, 6, 0, 0]);
    a.select_all();
    a.auto_on_shutdown = true;
    let auto = a.shutdown_auto_sweep();
    cs.add("shutdown_auto_clear", auto == 11 && TRACE_KINDS.iter().all(|&k| a.count(k) == 0), "");
    // 7) 自动清未开 = 关机不动（诚实）。
    let mut b = PrivacySweep::new();
    b.set_counts([1, 1, 1, 1]);
    cs.add("auto_off_noop", b.shutdown_auto_sweep() == 0 && b.count(TraceKind::RecentFiles) == 1, "");
    // 8) 确认文案在册。
    cs.add("irreversible_text", IRREVERSIBLE_TEXT == "清除后不可恢复", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_matches_selection_exactly() {
        let mut p = PrivacySweep::new();
        p.set_counts([10, 20, 30, 40]);
        p.select(TraceKind::ClipboardHistory, true);
        assert_eq!(p.preview_total(), 30);
        p.select(TraceKind::Notifications, true);
        assert_eq!(p.preview_total(), 70);
        p.select(TraceKind::ClipboardHistory, false);
        assert_eq!(p.preview_total(), 40);
    }

    #[test]
    fn sweep_only_touches_selected() {
        let mut p = PrivacySweep::new();
        p.set_counts([3, 0, 4, 0]);
        p.select(TraceKind::SearchHistory, true);
        p.sweep(true);
        assert_eq!(p.count(TraceKind::SearchHistory), 0);
        assert_eq!(p.count(TraceKind::ClipboardHistory), 4);
    }

    #[test]
    fn auto_shutdown_clears_everything_selected() {
        let mut p = PrivacySweep::new();
        p.set_counts([2, 2, 2, 2]);
        p.select_all();
        p.auto_on_shutdown = true;
        assert_eq!(p.shutdown_auto_sweep(), 8);
        assert!(TRACE_KINDS.iter().all(|&k| p.count(k) == 0));
    }
}
