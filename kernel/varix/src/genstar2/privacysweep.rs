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

// ===========================================================================
// 深化 v2（F487）：四类清除逐类执行账 / 关机自动清模式 / 清除前条数
// 预览对总 / 不可恢复确认三重闸 / 总开关语义
// ===========================================================================

/// 清除前条数预览对总（主册「将清除：搜索 23 条、文件 41 条…」——
/// 预览总数 = 各勾选类之和：多算少算都是预览说谎）。
pub fn preview_total(counts: [u32; 4], selected: [bool; 4]) -> u32 {
    counts.iter().zip(selected.iter()).filter(|(_, &sel)| sel).map(|(&c, _)| c).sum()
}

/// 不可恢复确认三重闸（主册「清除不可恢复（确认框明说）」的执行闸：
/// ① 勾选类非空 ② 不可恢复文案已展示 ③ 用户确认——三闸齐才执行）。
pub struct SweepGate {
    pub selection_nonempty: bool,
    pub warning_shown: bool,
    pub user_confirmed: bool,
}

impl SweepGate {
    pub fn all_open(&self) -> bool {
        self.selection_nonempty && self.warning_shown && self.user_confirmed
    }
}

/// 关机自动清模式（主册「计划清除可选（每次关机自动清勾选类）」——
/// 模式开启时关机钩子按勾选执行；重启后验证全空 = 勾选类计数归零）。
pub struct ShutdownAutoSweep {
    pub enabled: bool,
    pub selected: [bool; 4],
}

impl ShutdownAutoSweep {
    /// 关机钩子执行（返回各勾选类清除条数——未勾选类原样保留）。
    pub fn on_shutdown(&self, counts: &mut [u32; 4], confirmed: bool) -> [u32; 4] {
        let mut cleared = [0u32; 4];
        if !self.enabled || !confirmed {
            return cleared;
        }
        for i in 0..4 {
            if self.selected[i] {
                cleared[i] = counts[i];
                counts[i] = 0;
            }
        }
        cleared
    }

    /// 重启后验证：勾选类全空（主册「重启后验证全空」的机械判定）。
    pub fn verify_after_reboot(&self, counts: [u32; 4]) -> bool {
        (0..4).all(|i| !self.selected[i] || counts[i] == 0)
    }
}

/// 总开关语义（主册「『全部清除』总开关」：全选 = 四类全勾；
/// 总开关关 = 四类全不勾——一键语义与逐类勾选共存不冲突）。
pub fn select_all_semantics(selected: &mut [bool; 4], on: bool) {
    for s in selected.iter_mut() {
        *s = on;
    }
}

/// 清除执行账（勾选类清零、未勾选类不动——「只动该动的」审计面）。
pub fn execute_sweep(counts: &mut [u32; 4], selected: [bool; 4], gate: &SweepGate) -> Option<[u32; 4]> {
    if !gate.all_open() {
        return None; // 闸门不齐：诚实拒绝执行。
    }
    let mut cleared = [0u32; 4];
    for i in 0..4 {
        if selected[i] {
            cleared[i] = counts[i];
            counts[i] = 0;
        }
    }
    Some(cleared)
}

// ---------------------------------------------------------------------------
// 深化自检（F487 v2）
// ---------------------------------------------------------------------------

pub fn run_privacysweep_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F487-v2");
    // 1) 预览对总：勾选类求和精确。
    let counts = [23, 41, 7, 15];
    let sel = [true, true, false, true];
    cs.add("preview_total", preview_total(counts, sel) == 23 + 41 + 15, "");
    cs.add("preview_none", preview_total(counts, [false; 4]) == 0, "");
    // 2) 三重闸：缺一拒执行。
    let gate_open = SweepGate { selection_nonempty: true, warning_shown: true, user_confirmed: true };
    let gate_no_warn = SweepGate { warning_shown: false, ..gate_open };
    let mut counts2 = counts;
    cs.add("gate_all_open", execute_sweep(&mut counts2, sel, &gate_open) == Some([23, 41, 0, 15]), "");
    cs.add("gate_missing_rejected", execute_sweep(&mut counts2, sel, &gate_no_warn).is_none(), "");
    // 3) 执行账：勾选清零、未勾保留（gate_no_warn 尝试零副作用）。
    cs.add("sweep_selective", counts2[0] == 0 && counts2[1] == 0 && counts2[2] == 7 && counts2[3] == 0, "");
    // 4) 关机自动清：重启后勾选类全空。
    let auto = ShutdownAutoSweep { enabled: true, selected: sel };
    let mut counts3 = counts;
    let _ = auto.on_shutdown(&mut counts3, true);
    cs.add("auto_sweep_verify", auto.verify_after_reboot(counts3), "");
    // 5) 总开关一键语义。
    let mut s4 = [false; 4];
    select_all_semantics(&mut s4, true);
    cs.add("select_all_on", s4.iter().all(|&x| x), "");
    select_all_semantics(&mut s4, false);
    cs.add("select_all_off", s4.iter().all(|&x| !x), "");
    cs.add("irreversible_text", IRREVERSIBLE_TEXT == "清除后不可恢复", "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn sweep_preserves_unselected() {
        let mut counts = [10, 20, 30, 40];
        let gate = SweepGate { selection_nonempty: true, warning_shown: true, user_confirmed: true };
        let cleared = execute_sweep(&mut counts, [false, true, false, true], &gate).unwrap();
        assert_eq!((cleared[1], cleared[3]), (20, 40));
        assert_eq!((counts[0], counts[1], counts[2], counts[3]), (10, 0, 30, 0));
    }

    #[test]
    fn auto_sweep_disabled_noop() {
        let auto = ShutdownAutoSweep { enabled: false, selected: [true; 4] };
        let mut counts = [5, 5, 5, 5];
        let cleared = auto.on_shutdown(&mut counts, true);
        assert_eq!(cleared, [0; 4]);
        assert_eq!(counts, [5, 5, 5, 5]);
    }

    #[test]
    fn preview_matches_execution() {
        // 预览承诺与实际清除相等（预览不说谎的闭环）。
        let counts = [3, 9, 27, 81];
        let sel = [true, false, true, false];
        let promised = preview_total(counts, sel);
        let gate = SweepGate { selection_nonempty: true, warning_shown: true, user_confirmed: true };
        let mut c2 = counts;
        let cleared = execute_sweep(&mut c2, sel, &gate).unwrap();
        let actual: u32 = cleared.iter().sum();
        assert_eq!(promised, actual);
    }
}
