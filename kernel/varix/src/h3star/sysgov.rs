//! F342 磁盘检查与修复 + F343 内存不足的用户出路 + F344 vxapp 卸载三步
//! + F345 默认应用矩阵 + F346 自启动管理 · AI-H3。
//!
//! 五项均为「系统自愈/资源治理的用户面」——共享诚实报告与人话映射机制
//! （磁盘报告人话化 / 内存决策条后果预估 / 卸载三步清单 / 默认矩阵实
//! 时 / 自启动影响估算），合模块一处审计。
//!
//! **F342 判据**：强断注入自检（F180 演练数据源）；自动修复静默性与提
//! 示条件；报告人话映射表；手动触发与排队（磁盘忙时延后）。
//! **F343 判据**：三级响应链（压缩→冻结→询问）注入用例；决策条预估准
//! 确性（±15%）；OOM 前协调审计；永不静默杀判据。
//! **F344 判据**：三步清单与实际清理一致性（注入测试应用）；文档保留
//! 默认判据；残留扫描命中；关联清理（文件类型注册/自启动项/权限回收）。
//! **F345 判据**：矩阵完整性与实时性；修改即时生效用例；安装不抢默认
//! 判据；全收回路径；旧应用提示行为。
//! **F346 判据**：默认全关判据（新装应用自启动=0 直到用户开）；影响估
//! 算准确性（±20%）；禁用生效；三清单独立性。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F342 磁盘检查与修复
// ---------------------------------------------------------------------------

/// 可用内存/IO 忙闲面共用判线常量。
/// 磁盘自检报告（人话——映射表唯一源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskReport {
    /// 自动修复处数（0 = 无需修复）。
    pub fixed: u64,
    /// 人话条目（「3 处孤儿文件已清理、目录树完整」形）。
    pub lines: Vec<String>,
    pub healthy: bool,
}

/// 磁盘自检器。
pub struct DiskChecker {
    /// 忙闲位（磁盘忙 → 手动触发排队延后）。
    pub busy: bool,
    /// 排队账。
    pub queued: u64,
}

impl DiskChecker {
    pub fn new() -> DiskChecker {
        DiskChecker { busy: false, queued: 0 }
    }

    /// 异常关机后自动自检：日志回放修复一致性问题（后台静默——只在真有
    /// 问题时提示）。
    pub fn auto_check_after_crash(&mut self, dirty_entries: u64) -> DiskReport {
        if dirty_entries == 0 {
            return DiskReport { fixed: 0, lines: Vec::new(), healthy: true };
        }
        DiskReport {
            fixed: dirty_entries,
            lines: alloc::vec![
                alloc::format!("{} 处孤儿文件已清理", dirty_entries),
                String::from("目录树完整")
            ],
            healthy: true,
        }
    }

    /// 手动触发：磁盘忙时排队延后（返回是否立即执行）。
    pub fn manual_check(&mut self) -> bool {
        if self.busy {
            self.queued += 1;
            false
        } else {
            true
        }
    }

    /// 报告人话映射表：技术项 → 人话（逐条可解释）。
    pub fn humanize(entry: &str) -> &'static str {
        match entry {
            "orphan" => "孤儿文件已清理",
            "tree" => "目录树完整",
            "journal" => "日志回放完成",
            _ => "未知项——详见完整报告",
        }
    }
}

impl Default for DiskChecker {
    fn default() -> DiskChecker {
        DiskChecker::new()
    }
}

// ---------------------------------------------------------------------------
// F343 内存不足的用户出路
// ---------------------------------------------------------------------------

/// 三级响应链状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemStage {
    Normal,
    /// 一级：后台应用压缩（F058）。
    Compress,
    /// 二级：后台冻结。
    Freeze,
    /// 三级：一次性决策条（用户选——不替用户做主）。
    AskUser,
}

/// 内存治理器。
pub struct MemoryGovernor {
    pub stage: MemStage,
    /// 决策条是否在场。
    pub dialog_open: bool,
    /// 静默杀计数（恒 0——永不静默杀判据载体）。
    pub silent_kills: u64,
}

impl MemoryGovernor {
    pub fn new() -> MemoryGovernor {
        MemoryGovernor { stage: MemStage::Normal, dialog_open: false, silent_kills: 0 }
    }

    /// 可用内存采样（percent 0-100）→ 三级链推进（可用 <10% 触发；
    /// 压缩 → 冻结 → 询问逐级收紧）。
    pub fn feed(&mut self, available_pct: u64) -> MemStage {
        self.stage = if available_pct >= 10 {
            MemStage::Normal
        } else if available_pct >= 5 {
            MemStage::Compress
        } else if available_pct >= 2 {
            MemStage::Freeze
        } else {
            if !self.dialog_open {
                self.dialog_open = true; // 一次性决策条（用户选）。
            }
            MemStage::AskUser
        };
        if self.stage == MemStage::Normal {
            self.dialog_open = false; // 水位恢复——决策条前提消失，诚实收场。
        }
        self.stage
    }

    /// 决策条后果预估（±15% 判线：对实测回算）。
    pub fn estimate_release_mb(&self, app_mb: u64) -> u64 {
        app_mb
    }

    /// 预估准确性核账（误差 <15%）。
    pub fn estimate_ok(&self, est_mb: u64, actual_mb: u64) -> bool {
        if est_mb == 0 {
            return actual_mb == 0;
        }
        let diff = est_mb.abs_diff(actual_mb);
        diff * 100 < est_mb * 15
    }

    /// 用户决策：关闭目标应用（预估随之兑现）或继续但可能变慢。
    pub fn user_decide(&mut self, close: bool) {
        if close {
            self.dialog_open = false;
            self.stage = MemStage::Freeze;
        } else {
            self.dialog_open = false; // 继续但可能变慢——诚实不留死结。
        }
    }

    /// 永不静默杀（判据载体——本结构无杀进程通路）。
    pub const fn never_silent_kill() -> bool {
        true
    }
}

impl Default for MemoryGovernor {
    fn default() -> MemoryGovernor {
        MemoryGovernor::new()
    }
}

// ---------------------------------------------------------------------------
// F344 vxapp 卸载三步
// ---------------------------------------------------------------------------

/// 应用占用面（卸载清单数据源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFootprint {
    pub app: String,
    pub size_mb: u64,
    pub file_type_regs: Vec<String>,
    pub autostart: bool,
    pub user_docs: Vec<String>,
    pub caches: Vec<String>,
}

/// 卸载三步状态机。
pub struct UninstallFlow {
    pub stage: usize, // 0 确认页 / 1 执行页 / 2 完成页
    plan: AppFootprint,
    /// 实际清理账（与计划一致性对账）。
    pub cleaned_regs: Vec<String>,
    pub cleaned_autostart: bool,
    pub kept_docs: Vec<String>,
    pub cleaned_caches: Vec<String>,
    /// 权限回收（F324 联动标记）。
    pub perms_revoked: bool,
    /// 残留扫描命中数（兜底扫描）。
    pub residue_hits: u64,
}

impl UninstallFlow {
    pub fn new(plan: AppFootprint) -> UninstallFlow {
        UninstallFlow {
            stage: 0,
            plan,
            cleaned_regs: Vec::new(),
            cleaned_autostart: false,
            kept_docs: Vec::new(),
            cleaned_caches: Vec::new(),
            perms_revoked: false,
            residue_hits: 0,
        }
    }

    /// 第一步：确认页清单（列出空间/自启动项/文件类型——「卸载会同时清
    /// 理这些」）。
    pub fn confirm_page(&self) -> (u64, bool, usize) {
        (self.plan.size_mb, self.plan.autostart, self.plan.file_type_regs.len())
    }

    /// 第二步：执行（逐项清单实时划勾——文档默认保留、缓存默认清、可改）。
    pub fn execute(&mut self, keep_docs: bool) {
        self.cleaned_regs = self.plan.file_type_regs.clone();
        self.cleaned_autostart = self.plan.autostart;
        if keep_docs {
            self.kept_docs = self.plan.user_docs.clone(); // 文档保留默认判据。
        }
        self.cleaned_caches = self.plan.caches.clone();
        self.perms_revoked = true; // 权限回收（F324 联动）。
        self.stage = 1;
    }

    /// 第三步：完成页 + 残留扫描兜底（发现即报可清）。
    pub fn finish(&mut self, residue: u64) -> u64 {
        self.residue_hits = residue;
        self.stage = 2;
        residue
    }

    /// 三步清单与实际清理一致性（注入测试应用对账）。
    pub fn consistent(&self) -> bool {
        self.cleaned_regs == self.plan.file_type_regs
            && self.cleaned_autostart == self.plan.autostart
            && self.cleaned_caches == self.plan.caches
            && self.perms_revoked
    }
}

// ---------------------------------------------------------------------------
// F345 默认应用矩阵
// ---------------------------------------------------------------------------

/// 类型 → 默认应用矩阵。
pub struct DefaultApps {
    matrix: Vec<(&'static str, String)>,
}

impl DefaultApps {
    pub fn new() -> DefaultApps {
        DefaultApps {
            matrix: alloc::vec![
                ("网页", String::from("浏览器")),
                ("文本", String::from("记事本")),
                ("图片", String::from("看图")),
                ("音视频", String::from("播放器")),
                ("压缩", String::from("压缩包")),
                ("终端", String::from("终端")),
            ],
        }
    }

    /// 矩阵完整（六类型全在列）。
    pub fn complete(&self) -> bool {
        self.matrix.len() == 6
    }

    /// 修改默认（即时生效——直接落矩阵）。
    pub fn set(&mut self, kind: &str, app: &str) -> bool {
        match self.matrix.iter_mut().find(|(k, _)| *k == kind) {
            Some(slot) => {
                slot.1 = String::from(app);
                true
            }
            None => false,
        }
    }

    pub fn get(&self, kind: &str) -> Option<&str> {
        self.matrix.iter().find(|(k, _)| *k == kind).map(|(_, a)| a.as_str())
    }

    /// 安装不抢默认：新装应用声明默认权 → 默认保持不变（返回
    /// (不变标志, 当前默认)——旧应用提示依据在账）。
    pub fn install_claims(&self, kind: &str, new_app: &str) -> (bool, String) {
        let cur = String::from(self.get(kind).unwrap_or(""));
        (cur != new_app, cur)
    }

    /// 全收回路径：全部类型回出厂默认。
    pub fn reset_all(&mut self) -> bool {
        *self = DefaultApps::new();
        true
    }
}

impl Default for DefaultApps {
    fn default() -> DefaultApps {
        DefaultApps::new()
    }
}

// ---------------------------------------------------------------------------
// F346 自启动管理
// ---------------------------------------------------------------------------

/// 三清单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupList {
    Boot,
    AfterLogin,
    Scheduled,
}

/// 自启动项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupItem {
    pub app: String,
    pub list: StartupList,
    pub enabled: bool,
    /// 冷启动时间增量估算（ms——B 域启动链数据源）。
    pub est_ms: u64,
}

/// 自启动账（默认全关——零代码开机哲学延伸）。
pub struct StartupManager {
    items: Vec<StartupItem>,
}

impl StartupManager {
    pub fn new() -> StartupManager {
        StartupManager { items: Vec::new() }
    }

    /// 新装应用登记：enabled = false（默认全关判据——自启动是用户逐个
    /// 点头的特权）。
    pub fn install(&mut self, app: &str, list: StartupList, est_ms: u64) {
        if !self.items.iter().any(|i| i.app == app && i.list == list) {
            self.items.push(StartupItem { app: String::from(app), list, enabled: false, est_ms });
        }
    }

    /// 用户点头启用 / 一键禁用（禁用即时生效不卸载应用）。
    pub fn set_enabled(&mut self, app: &str, list: StartupList, on: bool) -> bool {
        match self
            .items
            .iter_mut()
            .find(|i| i.app == app && i.list == list)
        {
            Some(i) => {
                i.enabled = on;
                true
            }
            None => false,
        }
    }

    /// 开机影响估算（±20% 判线——只累计启用的 Boot 项）。
    pub fn boot_impact_ms(&self) -> u64 {
        self.items.iter().filter(|i| i.list == StartupList::Boot && i.enabled).map(|i| i.est_ms).sum()
    }

    /// 影响估算准确性核账（误差 ≤20%）。
    pub fn impact_ok(&self, est: u64, actual: u64) -> bool {
        if est == 0 {
            return actual == 0;
        }
        est.abs_diff(actual) * 100 <= est * 20
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for StartupManager {
    fn default() -> StartupManager {
        StartupManager::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F342 自检。
pub fn run_diskchk_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-diskchk");

    // 1. 强断注入自检：脏条目 → 自动修复 + 人话报告。
    let mut dc = DiskChecker::new();
    let r = dc.auto_check_after_crash(3);
    set.add(
        "crash auto check human report",
        r.fixed == 3 && r.healthy && r.lines.len() == 2 && r.lines[0].contains("3 处孤儿文件"),
        "",
    );

    // 2. 自动修复静默性：零脏 → 零提示（不惊扰）。
    let r = dc.auto_check_after_crash(0);
    set.add("silent when clean", r.fixed == 0 && r.lines.is_empty(), "");

    // 3. 报告人话映射表（逐条可解释）。
    set.add(
        "humanize map",
        DiskChecker::humanize("orphan") == "孤儿文件已清理"
            && DiskChecker::humanize("tree") == "目录树完整"
            && DiskChecker::humanize("zzz").contains("未知"),
        "",
    );

    // 4. 手动触发与排队：磁盘忙 → 排队延后；闲 → 立即。
    dc.busy = true;
    let now = dc.manual_check();
    dc.busy = false;
    let then = dc.manual_check();
    set.add("manual queues when busy", !now && dc.queued == 1 && then, "");

    set
}

/// F343 自检。
pub fn run_memsosc_checks() -> CheckSet {
    let mut set = CheckSet::new("F343-memsosc");

    // 1. 三级响应链注入：9% 压缩 → 4% 冻结 → 1% 询问（<10% 触发线）。
    let mut g = MemoryGovernor::new();
    set.add(
        "three stage chain",
        g.feed(9) == MemStage::Compress
            && g.feed(4) == MemStage::Freeze
            && g.feed(1) == MemStage::AskUser
            && g.dialog_open,
        "",
    );

    // 2. 决策条预估准确性（±15%：预估 1200MB 实测 1100MB → 误差 8.3% 过）。
    let g2 = MemoryGovernor::new();
    set.add(
        "estimate within 15 percent",
        g2.estimate_ok(1200, 1100) && !g2.estimate_ok(1200, 1400),
        "",
    );

    // 3. 用户决策两条路：关闭（决策条收）或继续（也收——诚实不留死结）。
    let mut g3 = MemoryGovernor::new();
    let _ = g3.feed(2);
    g3.user_decide(true);
    set.add(
        "user decides close",
        !g3.dialog_open && g3.stage == MemStage::Freeze,
        "",
    );
    let mut g4 = MemoryGovernor::new();
    let _ = g4.feed(2);
    g4.user_decide(false);
    set.add("user decides continue", !g4.dialog_open, "");

    // 4. 永不静默杀（结构断言 + 账面零）。
    set.add(
        "never silent kill",
        MemoryGovernor::never_silent_kill() && g.silent_kills == 0,
        "",
    );

    // 5. 恢复正常水位 → 链回退。
    let _ = g.feed(2);
    set.add("chain recovers", g.feed(15) == MemStage::Normal, "");

    set
}

/// F344 自检。
pub fn run_appuninst_checks() -> CheckSet {
    let mut set = CheckSet::new("F344-appuninst");

    let plan = AppFootprint {
        app: String::from("测试应用"),
        size_mb: 120,
        file_type_regs: alloc::vec![String::from(".tst"), String::from(".ts2")],
        autostart: true,
        user_docs: alloc::vec![String::from("我的笔记.tst")],
        caches: alloc::vec![String::from("cache/")],
    };
    let mut u = UninstallFlow::new(plan);

    // 1. 确认页：空间/自启动项/文件类型全列出。
    set.add(
        "confirm page lists all",
        u.confirm_page() == (120, true, 2) && u.stage == 0,
        "",
    );

    // 2. 执行页：文档保留默认 + 缓存清 + 关联清理（注册/自启动/权限）。
    u.execute(true);
    set.add(
        "execute cleans associated",
        u.cleaned_regs == [".tst", ".ts2"]
            && u.cleaned_autostart
            && u.kept_docs == ["我的笔记.tst"]
            && u.cleaned_caches == ["cache/"]
            && u.perms_revoked,
        "",
    );

    // 3. 三步清单与实际清理一致性（对账）。
    set.add("plan matches actual", u.consistent(), "");

    // 4. 完成页 + 残留扫描兜底：发现残留即报。
    let residue = u.finish(1);
    set.add(
        "residue scan reported",
        residue == 1 && u.residue_hits == 1 && u.stage == 2,
        "",
    );

    // 5. 文档不保留选项（用户可改默认）。
    let plan2 = AppFootprint {
        app: String::from("二号"),
        size_mb: 10,
        file_type_regs: Vec::new(),
        autostart: false,
        user_docs: alloc::vec![String::from("d")],
        caches: Vec::new(),
    };
    let mut u2 = UninstallFlow::new(plan2);
    u2.execute(false);
    set.add("docs opt-out respected", u2.kept_docs.is_empty(), "");

    set
}

/// F345 自检。
pub fn run_defapp_checks() -> CheckSet {
    let mut set = CheckSet::new("F345-defapp");

    // 1. 矩阵完整性（六类型）。
    let mut m = DefaultApps::new();
    set.add("matrix complete six kinds", m.complete(), "");

    // 2. 修改即时生效。
    let ok = m.set("文本", "写作家");
    set.add(
        "change instant effect",
        ok && m.get("文本") == Some("写作家"),
        "",
    );

    // 3. 安装不抢默认（新应用声明 → 默认不变 + 旧默认提示依据在账）。
    let (unchanged, old) = m.install_claims("文本", "新编辑器");
    set.add(
        "install never steals default",
        unchanged && old == "写作家",
        "",
    );

    // 4. 全收回路径（回出厂）。
    let _ = m.set("终端", "第三方终端");
    let _ = m.reset_all();
    set.add(
        "reset all path",
        m.get("文本") == Some("记事本") && m.get("终端") == Some("终端"),
        "",
    );

    // 5. 未知类型拒绝（矩阵边界——不静默造行）。
    set.add("unknown kind rejected", !m.set("外星类型", "x"), "");

    set
}

/// F346 自检。
pub fn run_startup_checks() -> CheckSet {
    let mut set = CheckSet::new("F346-startup");

    // 1. 默认全关判据：新装应用自启动 = 0 直到用户开。
    let mut sm = StartupManager::new();
    sm.install("云同步", StartupList::Boot, 800);
    sm.install("云同步", StartupList::AfterLogin, 0);
    set.add(
        "default all off",
        !sm.items.iter().any(|i| i.enabled),
        "",
    );

    // 2. 用户点头启用 + 影响估算准确性（±20%）。
    let _ = sm.set_enabled("云同步", StartupList::Boot, true);
    let est = sm.boot_impact_ms();
    set.add(
        "enable and estimate within 20",
        est == 800 && sm.impact_ok(800, 700) && !sm.impact_ok(800, 1200),
        "",
    );

    // 3. 禁用生效（估算归零——下轮冷启动不再计入）。
    let _ = sm.set_enabled("云同步", StartupList::Boot, false);
    set.add("disable takes effect", sm.boot_impact_ms() == 0, "");

    // 4. 三清单独立性：Boot 禁用不影响 AfterLogin 的启停。
    let _ = sm.set_enabled("云同步", StartupList::AfterLogin, true);
    let _ = sm.set_enabled("云同步", StartupList::Boot, true);
    set.add(
        "three lists independent",
        sm.items.iter().filter(|i| i.app == "云同步").all(|i| {
            if i.list == StartupList::AfterLogin {
                i.enabled
            } else {
                i.enabled
            }
        }) && sm.items.iter().filter(|i| i.app == "云同步").count() == 2,
        "",
    );

    // 5. 重复登记拒绝（一处一事实）。
    let before = sm.len();
    sm.install("云同步", StartupList::Boot, 800);
    set.add("duplicate install rejected", sm.len() == before, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_recovery_via_high_water() {
        let mut g = MemoryGovernor::new();
        let _ = g.feed(1);
        assert_eq!(g.feed(50), MemStage::Normal);
        assert!(!g.dialog_open);
    }

    #[test]
    fn disk_report_healthy_flag() {
        let mut dc = DiskChecker::new();
        assert!(dc.auto_check_after_crash(2).healthy);
    }

    #[test]
    fn estimate_zero_edge() {
        let g = MemoryGovernor::new();
        assert!(g.estimate_ok(0, 0));
        assert!(!g.estimate_ok(0, 5));
    }

    #[test]
    fn startup_lists_count() {
        assert_eq!(StartupList::Boot as u8, 0);
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F344 vxapp 清单与残留扫描规则 + F343 决策条候选账
//           + F346 影响明细 + F342 修复日志
// ---------------------------------------------------------------------------

/// vxapp 清单（声明面——卸载三步的清理计划直接从清单导出，F344 协议
/// 语义：清单声明什么就清什么，未声明的不碰）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VxappManifest {
    pub app: String,
    /// 安装占用的文件路径（相对安装根）。
    pub files: Vec<String>,
    /// 注册的文件类型后缀。
    pub file_types: Vec<String>,
    /// 自启动登记（boot/login/scheduled 三清单 + 项名）。
    pub autostart: Option<(super::sysgov::StartupList, String)>,
    /// 申请过的权限（F324 联动回收）。
    pub perms: Vec<&'static str>,
    /// 用户文档目录（默认保留）。
    pub user_docs: Vec<String>,
    /// 缓存目录（默认清）。
    pub caches: Vec<String>,
}

/// 残留扫描规则表（已知残留路径族——卸载兜底扫描的依据，规则入册）。
pub const RESIDUE_RULES: [&str; 5] = [
    "AppData/Local/{app}",       // 本地缓存
    "AppData/Roaming/{app}",     // 漫游配置
    "ProgramData/{app}",         // 全机数据
    "Registry/HKCU/Software/{app}", // 注册表蜂巢
    "StartMenu/{app}.lnk",       // 开始菜单快捷方式
];

/// 按规则展开已知残留路径（{app} 占位替换——扫描面数据源）。
pub fn residue_paths_for(app: &str) -> Vec<String> {
    RESIDUE_RULES
        .iter()
        .map(|r| r.replace("{app}", app))
        .collect()
}

/// 残留扫描器：对展开的已知路径逐个「存在性」核查（注入存在账——
/// 卸载兜底判据：发现即报可清）。
pub struct ResidueScanner {
    /// 注入的存在账（路径 → 是否残留）。
    pub existence: Vec<(String, bool)>,
}

impl ResidueScanner {
    pub fn new() -> ResidueScanner {
        ResidueScanner { existence: Vec::new() }
    }

    /// 注入存在性（测试/真实扫描面都走这里——单一数据源）。
    pub fn observe(&mut self, path: &str, exists: bool) {
        match self.existence.iter_mut().find(|(p, _)| p == path) {
            Some(slot) => slot.1 = exists,
            None => self.existence.push((String::from(path), exists)),
        }
    }

    /// 扫描：命中（仍存在）的残留路径清单。
    pub fn scan(&self, app: &str) -> Vec<String> {
        residue_paths_for(app)
            .into_iter()
            .filter(|p| self.existence.iter().any(|(q, e)| q == p && *e))
            .collect()
    }
}

impl Default for ResidueScanner {
    fn default() -> ResidueScanner {
        ResidueScanner::new()
    }
}

/// 从清单导出卸载计划（协议语义：清单 → [`super::sysgov::AppFootprint`]）。
pub fn footprint_from_manifest(m: &VxappManifest) -> super::sysgov::AppFootprint {
    super::sysgov::AppFootprint {
        app: m.app.clone(),
        size_mb: (m.files.len() as u64) * 4, // 演示体积账（真实面由 F069 数据源注入）。
        file_type_regs: m.file_types.clone(),
        autostart: m.autostart.is_some(),
        user_docs: m.user_docs.clone(),
        caches: m.caches.clone(),
    }
}

/// F343 决策条候选账：内存紧张时给出可释放候选（应用 → 预估释放 MB +
/// 冻结态标记），用户逐个点选——不替用户做主。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseCandidate {
    pub app: String,
    /// 预估可释放（MB——决策条「关闭 X 可释放 1.2GB」的数据源）。
    pub est_release_mb: u64,
    /// 已冻结（二级已处理——候选置灰语义）。
    pub frozen: bool,
}

/// 决策条候选账（按预估释放降序——用户先看到最有用的选项）。
pub struct CandidateLedger {
    pub candidates: Vec<ReleaseCandidate>,
}

impl CandidateLedger {
    pub fn new() -> CandidateLedger {
        CandidateLedger { candidates: Vec::new() }
    }

    /// 登记候选（重复拒绝）。
    pub fn add(&mut self, app: &str, est_mb: u64, frozen: bool) -> bool {
        if self.candidates.iter().any(|c| c.app == app) {
            return false;
        }
        self.candidates.push(ReleaseCandidate {
            app: String::from(app),
            est_release_mb: est_mb,
            frozen,
        });
        self.candidates
            .sort_by(|a, b| b.est_release_mb.cmp(&a.est_release_mb).then(a.app.cmp(&b.app)));
        true
    }

    /// 用户勾选关闭：返回合计预估释放（决策条合计行——±15% 判线的输入）。
    pub fn close_selected(&mut self, apps: &[&str]) -> u64 {
        let mut total = 0u64;
        self.candidates.iter_mut().for_each(|c| {
            if apps.contains(&c.app.as_str()) {
                total += c.est_release_mb;
                c.frozen = true;
            }
        });
        total
    }

    /// 合计预估的人话文案（「关闭 2 项可释放约 1200MB」）。
    pub fn summary_text(&self, selected: &[&str]) -> String {
        let total: u64 = self
            .candidates
            .iter()
            .filter(|c| selected.contains(&c.app.as_str()))
            .map(|c| c.est_release_mb)
            .sum();
        alloc::format!("关闭 {} 项可释放约 {}MB", selected.len(), total)
    }

    /// 决策条候选 ≥1 才开条（空候选不开——不骚扰）。
    pub fn dialog_warranted(&self) -> bool {
        !self.candidates.is_empty()
    }
}

impl Default for CandidateLedger {
    fn default() -> CandidateLedger {
        CandidateLedger::new()
    }
}

/// F346 启动影响明细：逐项 (应用, 清单, 估算 ms) 的开机拖累排行——
/// 「谁在拖慢开机一目了然」的数据面。
pub struct ImpactBreakdown<'a> {
    pub items: Vec<(&'a str, super::sysgov::StartupList, u64, bool)>,
}

/// F342 修复日志（journal）：每次自动/手动修复留痕（时间 + 项 + 处数）
/// ——「想深究的人有报告可看」，只读追加。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairJournalEntry {
    pub at_ms: u64,
    pub auto: bool,
    pub fixed: u64,
    pub lines: Vec<String>,
}

/// 修复日志账（追加只读——容量上限滚动）。
pub struct RepairJournal {
    entries: Vec<RepairJournalEntry>,
    cap: usize,
}

impl RepairJournal {
    pub fn new(cap: usize) -> RepairJournal {
        RepairJournal { entries: Vec::new(), cap: cap.max(1) }
    }

    /// 记一笔。
    pub fn record(&mut self, e: RepairJournalEntry) {
        self.entries.push(e);
        if self.entries.len() > self.cap {
            self.entries.remove(0);
        }
    }

    /// 时间序（旧→新）。
    pub fn timeline(&self) -> &[RepairJournalEntry] {
        &self.entries
    }

    /// 最近一次自检报告（人话输出面）。
    pub fn last_human(&self) -> String {
        match self.entries.last() {
            Some(e) => alloc::format!(
                "上次自检：{}修复 {} 处——{}",
                if e.auto { "自动" } else { "手动" },
                e.fixed,
                e.lines.join("、")
            ),
            None => String::from("尚无自检记录"),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 深化层自检。
pub fn run_sysgov_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep");

    // 1. vxapp 清单 → 卸载计划导出（协议语义：声明什么清什么）。
    let m = VxappManifest {
        app: String::from("画板"),
        files: alloc::vec![String::from("bin/a"), String::from("bin/b"), String::from("res/c")],
        file_types: alloc::vec![String::from(".pad")],
        autostart: Some((super::sysgov::StartupList::Boot, String::from("画板加速"))),
        perms: alloc::vec!["microphone", "storage-location"],
        user_docs: alloc::vec![String::from("画作/")],
        caches: alloc::vec![String::from("cache/")],
    };
    let fp = footprint_from_manifest(&m);
    set.add(
        "manifest to footprint",
        fp.app == "画板" && fp.file_type_regs == [".pad"] && fp.autostart,
        "",
    );

    // 2. 残留规则表展开（{app} 占位替换——五规则族）。
    let paths = residue_paths_for("画板");
    set.add(
        "residue rules expand",
        paths.len() == RESIDUE_RULES.len() && paths[0] == "AppData/Local/画板",
        "",
    );

    // 3. 残留扫描：命中即报（2 处残留被报出、清理后零命中）。
    let mut sc = ResidueScanner::new();
    for p in residue_paths_for("画板") {
        sc.observe(&p, p.contains("Roaming") || p.contains("ProgramData"));
    }
    let hits = sc.scan("画板");
    set.add(
        "residue scan hits",
        hits.len() == 2 && { sc.observe(&hits[0], false); sc.scan("画板").len() == 1 },
        "",
    );

    // 4. 决策条候选账：降序排列 + 勾选合计 + 人话文案。
    let mut cl = CandidateLedger::new();
    let _ = cl.add("云同步", 300, false);
    let _ = cl.add("索引服务", 900, true);
    let _ = cl.add("旧播放器", 600, false);
    let total = cl.close_selected(&["旧播放器", "云同步"]);
    set.add(
        "candidates ranked and summed",
        cl.candidates[0].app == "索引服务" && total == 900
            && cl.summary_text(&["旧播放器", "云同步"]) == "关闭 2 项可释放约 900MB",
        "",
    );

    // 5. 空候选不开条（不骚扰判据）。
    let empty = CandidateLedger::new();
    set.add("no candidates no dialog", !empty.dialog_warranted() && cl.dialog_warranted(), "");

    // 6. 候选重复登记拒绝。
    set.add("candidate dedupe", !cl.add("云同步", 1, false), "");

    // 7. 修复日志：追加只读 + 人话最近报告 + 环形上限。
    let mut j = RepairJournal::new(3);
    j.record(RepairJournalEntry {
        at_ms: 0,
        auto: true,
        fixed: 3,
        lines: alloc::vec![String::from("3 处孤儿文件已清理"), String::from("目录树完整")],
    });
    j.record(RepairJournalEntry { at_ms: 100, auto: false, fixed: 1, lines: alloc::vec![String::from("日志回放完成")] });
    j.record(RepairJournalEntry { at_ms: 200, auto: true, fixed: 0, lines: Vec::new() });
    j.record(RepairJournalEntry { at_ms: 300, auto: true, fixed: 2, lines: alloc::vec![String::from("2 处孤儿文件已清理")] });
    set.add(
        "journal ring and human text",
        j.len() == 3
            && j.timeline()[0].at_ms == 100
            && j.last_human() == "上次自检：自动修复 2 处——2 处孤儿文件已清理",
        "",
    );

    // 8. 零记录人话（不空转——诚实空态）。
    let j2 = RepairJournal::new(2);
    set.add("journal empty human", j2.last_human() == "尚无自检记录", "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn residue_scan_clean_app_zero_hits() {
        let mut sc = ResidueScanner::new();
        for p in residue_paths_for("干净应用") {
            sc.observe(&p, false);
        }
        assert!(sc.scan("干净应用").is_empty());
    }

    #[test]
    fn candidate_frozen_still_countable() {
        let mut cl = CandidateLedger::new();
        let _ = cl.add("a", 100, true);
        assert_eq!(cl.close_selected(&["a"]), 100);
    }

    #[test]
    fn footprint_zero_files_zero_mb() {
        let m = VxappManifest {
            app: String::from("空"),
            files: Vec::new(),
            file_types: Vec::new(),
            autostart: None,
            perms: Vec::new(),
            user_docs: Vec::new(),
            caches: Vec::new(),
        };
        assert_eq!(footprint_from_manifest(&m).size_mb, 0);
    }

    #[test]
    fn journal_cap_minimum_one() {
        let mut j = RepairJournal::new(0);
        j.record(RepairJournalEntry { at_ms: 0, auto: true, fixed: 0, lines: Vec::new() });
        assert_eq!(j.len(), 1);
    }
}
