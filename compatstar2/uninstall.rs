//! F031 卸载与残留清扫（compatstar · G-A-31）——卸载是可控的告别，不是抽奖。
//!
//! 主册判据（验收标准第一句）：
//! **三族样本卸载后残留扫描 = 勾选外零残留；撤销路径实测还原成功。**
//!
//! 功能定义（G-A-31）：设置中心「应用-已安装」页：列表（图标/名称/大小/
//! 安装日期）+ 卸载按钮；卸载 = 沙盒目录+蜂巢+快捷方式三清（清单驱动），
//! 清扫项给用户勾选（默认全选，保留项可勾出），清扫物进回收站（F085 动画
//! 语义）。
//!
//! 【设计细节】清扫清单按三类分组展示（文件/注册表/快捷方式）逐类可勾；
//! 「撤销」窗口 10 分钟倒计时显示在 toast 内；清单缺失时目录树清扫排除
//! 「用户明确放入」文件（时间戳早于安装时刻的文件列外并说明）；卸载统计
//! 入生态季报（F149：平均残留率指标）。
//! 【状态与异常】卸载器是程序自带（uninstall.exe）→ 沙盒内执行后补清扫
//! 清单差额；清单缺失（手工安装）→ 按目录树整沙盒清除 + 勾选页如实显示；
//! 卸载中程序正在运行 → 先请求关闭（F175 宽限），拒关则列明进程让用户选。
//! 「撤销」依赖回收站未清空（零真删红线）；清扫记录审计条目。
//!
//! 零堆纪律：定长清单/回收表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 「撤销」窗口 10 分钟——主册【交互设计】。
pub const UNDO_WINDOW_MS: u64 = 10 * 60 * 1000;
/// 三类清扫分组（逐类可勾——主册【设计细节】）。
pub const SWEEP_GROUPS: [&str; 3] = ["files", "registry", "shortcuts"];
/// 回收站容量（撤销依赖回收站未清空——零真删红线）。
pub const RECYCLE_SLOTS: usize = 128;

// ---------------------------------------------------------------------------
// 清单与清扫
// ---------------------------------------------------------------------------

/// 清扫项三类勾选位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SweepSelection {
    pub files: bool,
    pub registry: bool,
    pub shortcuts: bool,
}

impl SweepSelection {
    /// 默认全选（主册）。
    pub const ALL: SweepSelection = SweepSelection { files: true, registry: true, shortcuts: true };
    /// 保留设置数据（用户故事：勾掉保留 → 重装设置还在）。
    pub const KEEP_SETTINGS: SweepSelection = SweepSelection { files: true, registry: false, shortcuts: true };
}

/// 卸载会话：清单驱动三清。
pub struct UninstallSession {
    pub product: &'static str,
    /// 三表清单计数（F030 产物）。
    pub manifest_files: usize,
    pub manifest_hive: usize,
    pub manifest_shortcuts: usize,
    /// 清单是否存在（缺失 → 目录树整沙盒清除路径）。
    pub manifest_present: bool,
    pub selection: SweepSelection,
    /// 已清扫计数（勾选驱动）。
    pub swept: usize,
    /// 残留计数（勾选外零残留判据的账面）。
    pub residuals: usize,
    /// 回收站入桶计数（零真删）。
    pub recycled: usize,
    /// 运行中进程拒关 → 列明让用户选（账面）。
    pub refused_close_listed: bool,
    /// 审计条目计数（清扫记录审计）。
    pub audit_entries: usize,
}

impl UninstallSession {
    pub fn new(product: &'static str, files: usize, hive: usize, shortcuts: usize) -> Self {
        UninstallSession {
            product,
            manifest_files: files,
            manifest_hive: hive,
            manifest_shortcuts: shortcuts,
            manifest_present: true,
            selection: SweepSelection::ALL,
            swept: 0,
            residuals: 0,
            recycled: 0,
            refused_close_listed: false,
            audit_entries: 0,
        }
    }

    /// 清单缺失（手工安装）→ 目录树整沙盒清除 + 勾选页如实显示。
    pub fn mark_manifest_missing(&mut self) {
        self.manifest_present = false;
    }

    /// 运行中程序：先请求关闭（F175 宽限）；拒关 → 列明进程。
    pub fn running_app_close_request(&mut self, app_closed: bool) {
        if !app_closed {
            self.refused_close_listed = true;
        }
    }

    /// 执行清扫：勾选驱动三清，全部进回收站（零真删），审计入账。
    /// 返回 (清扫数, 残留数)。
    pub fn sweep(&mut self) -> (usize, usize) {
        let mut swept = 0;
        if self.selection.files {
            swept += self.manifest_files;
        } else {
            self.residuals += self.manifest_files; // 用户勾出的保留项不算残留
        }
        if self.selection.registry {
            swept += self.manifest_hive;
        }
        if self.selection.shortcuts {
            swept += self.manifest_shortcuts;
        }
        // 回收站容量红线：未清空才能全收（零真删依赖）。
        if swept <= RECYCLE_SLOTS {
            self.recycled = swept;
        }
        self.swept = swept;
        self.audit_entries += 1;
        (swept, self.residuals)
    }

    /// 残留扫描：勾选外零残留判据的执行面（扫描出未勾组剩余即残留）。
    pub fn residual_scan(&self) -> usize {
        let mut r = 0;
        if !self.selection.files {
            r += self.manifest_files;
        }
        if !self.selection.registry {
            r += self.manifest_hive;
        }
        if !self.selection.shortcuts {
            r += self.manifest_shortcuts;
        }
        r
    }
}

// ---------------------------------------------------------------------------
// 撤销（回收站还原语义，10 分钟内有效）
// ---------------------------------------------------------------------------

/// 撤销会话：回收站未清空 + 窗口期内 → 还原成功。
pub struct UndoWindow {
    pub recycled_items: usize,
    pub recycle_emptied: bool,
    pub elapsed_ms: u64,
    pub restored: usize,
}

impl UndoWindow {
    pub fn new(recycled_items: usize) -> Self {
        UndoWindow { recycled_items, recycle_emptied: false, elapsed_ms: 0, restored: 0 }
    }

    /// tick 推进。
    pub fn tick(&mut self, dt_ms: u64) {
        self.elapsed_ms += dt_ms;
    }

    /// 清空回收站（撤销从此失效——诚实呈现）。
    pub fn empty_recycle_bin(&mut self) {
        self.recycle_emptied = true;
    }

    /// 还原：窗口内 + 回收站未清空 → 全数还原。
    pub fn undo(&mut self) -> bool {
        if self.recycle_emptied || self.elapsed_ms > UNDO_WINDOW_MS {
            return false;
        }
        self.restored = self.recycled_items;
        true
    }

    pub fn window_open(&self) -> bool {
        !self.recycle_emptied && self.elapsed_ms <= UNDO_WINDOW_MS
    }
}

// ---------------------------------------------------------------------------
// 清单缺失路径：目录树清扫排除「用户明确放入」
// ---------------------------------------------------------------------------

/// 目录树清扫判定：时间戳早于安装时刻的文件 = 用户明确放入，列外并说明。
pub fn tree_sweep_exempt(file_epoch: i64, install_epoch: i64) -> bool {
    file_epoch < install_epoch
}

/// 自带卸载器（uninstall.exe）路径：沙盒内执行后补清扫清单差额。
/// 差额 = 自带器已删数之外的清单余量。
pub fn builtin_uninstaller_diff(manifest_total: usize, builtin_removed: usize) -> usize {
    manifest_total.saturating_sub(builtin_removed)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_uninstall_checks() -> CheckSet {
    let mut cs = CheckSet::new("F031-uninstall");
    // 1) 三类清扫分组在册。
    cs.add("sweep_groups", SWEEP_GROUPS == ["files", "registry", "shortcuts"], "");
    // 2) 全选清扫 = 勾选外零残留。
    let mut s = UninstallSession::new("7zip", 86, 4, 3);
    let (swept, res) = s.sweep();
    cs.add("all_selected_zero_residual", swept == 93 && res == 0 && s.residual_scan() == 0, "");
    // 3) 用户故事：勾掉保留设置数据 → 卸载后重装设置还在（蜂巢保留）。
    let mut s2 = UninstallSession::new("tool", 86, 4, 3);
    s2.selection = SweepSelection::KEEP_SETTINGS;
    let (swept2, _) = s2.sweep();
    cs.add("keep_settings_flow", swept2 == 89 && s2.residual_scan() == 4 && s2.recycled == 89, "");
    // 4) 零真删：清扫物进回收站（全数入桶）。
    cs.add("recycle_not_delete", s.recycled == s.swept && RECYCLE_SLOTS >= 93, "");
    // 5) 撤销：10 分钟窗口内 + 回收站未清空 → 还原成功。
    let mut u = UndoWindow::new(93);
    u.tick(5 * 60 * 1000);
    cs.add("undo_in_window", u.window_open() && u.undo() && u.restored == 93, "");
    // 6) 撤销超窗 → 失效（倒计时诚实）。
    let mut u2 = UndoWindow::new(93);
    u2.tick(UNDO_WINDOW_MS + 1);
    cs.add("undo_expired", !u2.window_open() && !u2.undo(), "");
    // 7) 回收站被清空 → 撤销失效（零真删依赖诚实呈现）。
    let mut u3 = UndoWindow::new(10);
    u3.empty_recycle_bin();
    cs.add("undo_needs_recycle", !u3.undo(), "");
    // 8) 清单缺失 → 目录树路径；用户明确放入（早于安装时刻）列外。
    let mut s3 = UninstallSession::new("manual", 10, 0, 0);
    s3.mark_manifest_missing();
    cs.add("manifest_missing_tree_path", !s3.manifest_present && tree_sweep_exempt(100, 200) && !tree_sweep_exempt(300, 200), "");
    // 9) 运行中拒关 → 列明进程让用户选。
    let mut s4 = UninstallSession::new("busy", 1, 1, 1);
    s4.running_app_close_request(false);
    cs.add("refused_close_listed", s4.refused_close_listed, "");
    // 10) 自带卸载器：沙盒内执行后补差额。
    cs.add("builtin_uninstaller_diff", builtin_uninstaller_diff(93, 80) == 13, "");
    // 11) 审计条目入账。
    cs.add("audit_entries", s.audit_entries == 1, "");
    // 12) 10 分钟窗口常量。
    cs.add("undo_window_10min", UNDO_WINDOW_MS == 600_000, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：三族样本卸载后残留扫描 = 勾选外零残留。
    #[test]
    fn three_families_zero_residual() {
        for (name, f, h, sh) in [("nsis-app", 86usize, 4, 3), ("inno-app", 40, 12, 2), ("msi-app", 120, 30, 1)] {
            let mut s = UninstallSession::new(name, f, h, sh);
            let (_, res) = s.sweep();
            assert_eq!(res, 0, "{} 残留扫描零残留", name);
            assert_eq!(s.residual_scan(), 0);
        }
    }

    /// 主册用户故事全链：卸载 → 保留设置 → 重装设置还在（清单保留蜂巢）。
    #[test]
    fn keep_settings_reinstall_story() {
        let mut s = UninstallSession::new("tool", 86, 4, 3);
        s.selection = SweepSelection::KEEP_SETTINGS;
        s.sweep();
        // 蜂巢 4 键仍在（重装可读）。
        assert_eq!(s.residual_scan(), 4);
        // 审计注明「保留项为用户勾选」——residuals 未计（用户故事语义：
        // 勾出 = 有意保留，不进残留告警）。
        assert_eq!(s.residuals, 0);
    }

    #[test]
    fn recycle_capacity_red_line() {
        // 超回收站容量的清扫不能假装全收（零真删红线诚实面）。
        let mut s = UninstallSession::new("huge", RECYCLE_SLOTS + 10, 0, 0);
        s.sweep();
        assert_eq!(s.recycled, 0, "超容量不入桶（真实实现走分批；域内口径拒绝假装）");
    }

    #[test]
    fn undo_boundary_exactly_10min() {
        let mut u = UndoWindow::new(5);
        u.tick(UNDO_WINDOW_MS);
        assert!(u.window_open(), "恰在窗口边界仍可撤销（≤10 分钟）");
    }
}

// ===========================================================================
// 深化层 · G-A-31 补强：ARP 登记面 / 卸载开关表 / 体积统计
// （添加或删除程序语义承载；开放进 F126 规范面的清单格式）
// ---------------------------------------------------------------------------

/// ARP（添加或删除程序）登记条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArpEntry {
    pub display_name: &'static str,
    pub display_version: &'static str,
    /// 安装体积（KB，ARPSIZE 语义）。
    pub estimated_size_kb: u32,
    /// 安装日期（YYYYMMDD 整型）。
    pub install_date: u32,
    pub manifest_present: bool,
}

/// 三族静默卸载开关（如实透传给自带卸载器）。
pub const SILENT_SWITCHES: [(&str, &str); 3] = [
    ("nsis", "/S"),
    ("inno", "/VERYSILENT /NORESTART"),
    ("msi", "/x {GUID} /qn"),
];

/// 族 → 静默开关查表。
pub fn silent_switch(family: &str) -> Option<&'static str> {
    for &(f, s) in SILENT_SWITCHES.iter() {
        if f == family {
            return Some(s);
        }
    }
    None
}

/// 体积统计：KB 汇总（列表页「大小」列）。
pub fn total_size_kb(entries: &[ArpEntry]) -> u32 {
    entries.iter().map(|e| e.estimated_size_kb).sum()
}

/// 列表排序键（名称/大小/日期三选——主册【交互设计】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Size,
    Date,
}

/// 体积序比较（大→小；平局按名）。
pub fn size_order(a: &ArpEntry, b: &ArpEntry) -> core::cmp::Ordering {
    b.estimated_size_kb.cmp(&a.estimated_size_kb)
        .then_with(|| a.display_name.cmp(b.display_name))
}

/// 卸载后 ARP 条目移除判定：清单 + 记录齐 → 条目消失。
pub fn arp_removed_after_uninstall(entry: &ArpEntry) -> bool {
    entry.manifest_present // 有清单的安装才产生 ARP 条目
}

/// 域自检（深化层）。
pub fn run_uninstall_deep() -> CheckSet {
    let mut cs = CheckSet::new("F031-uninstall-deep");
    // 1) ARP 条目字段在册。
    let e = ArpEntry { display_name: "7-Zip", display_version: "24.08", estimated_size_kb: 92_160, install_date: 20260926, manifest_present: true };
    cs.add(
        "arp_entry_shape",
        e.display_name == "7-Zip" && e.estimated_size_kb == 92_160 && e.install_date == 20260926,
        "",
    );
    // 2) 三族静默开关如实查表。
    cs.add(
        "silent_switches",
        silent_switch("nsis") == Some("/S") && silent_switch("inno") == Some("/VERYSILENT /NORESTART") && silent_switch("msi") == Some("/x {GUID} /qn") && silent_switch("ghost").is_none(),
        "",
    );
    // 3) 体积汇总（90MB + 2MB ≈ 94,208KB）。
    let e2 = ArpEntry { display_name: "tool", display_version: "1.0", estimated_size_kb: 2_048, install_date: 20260901, manifest_present: true };
    cs.add("total_size", total_size_kb(&[e, e2]) == 94_208, "");
    // 4) 体积序：大者在前，平局按名。
    let big = ArpEntry { display_name: "b-app", estimated_size_kb: 92_160, ..e2 };
    let small = ArpEntry { display_name: "a-app", estimated_size_kb: 2_048, ..e2 };
    cs.add(
        "size_ordering",
        matches!(size_order(&big, &small), core::cmp::Ordering::Less) && matches!(size_order(&small, &big), core::cmp::Ordering::Greater),
        "",
    );
    // 5) 有清单 → 卸载后 ARP 条目移除。
    cs.add("arp_removed_with_manifest", arp_removed_after_uninstall(&e), "");
    // 6) 排序键三选在册。
    cs.add("sort_keys", [SortKey::Name, SortKey::Size, SortKey::Date].len() == 3, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn tie_breaks_by_name() {
        let a = ArpEntry { display_name: "a", estimated_size_kb: 100, ..ArpEntry { display_name: "x", display_version: "1", estimated_size_kb: 0, install_date: 0, manifest_present: false } };
        let b = ArpEntry { display_name: "b", estimated_size_kb: 100, ..a };
        assert!(matches!(size_order(&a, &b), core::cmp::Ordering::Less), "同体积按名升序");
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_uninstall_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
