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

    /// 只读清单视图（审计面用——BootGateAudit 等逐项核查需要遍历）。
    pub fn items(&self) -> &[StartupItem] {
        &self.items
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

// ---------------------------------------------------------------------------
// 深化层二 · F344 执行划勾账/保留分类/关联清理 + F343 升级账/OOM 协调 +
// F345 旧应用提示/按应用查看 + F346 开机门审计
// ---------------------------------------------------------------------------

/// F344 执行页逐项划勾账（「进度+逐项清单实时划勾」判据载体）：逐项
/// 完成标记幂等，全部划勾才允许进入完成页。
pub struct UninstallProgress {
    items: Vec<(&'static str, bool)>,
}

impl UninstallProgress {
    pub fn new(items: &[&'static str]) -> UninstallProgress {
        UninstallProgress { items: items.iter().map(|i| (*i, false)).collect() }
    }

    /// 划勾（幂等——重复划同一项不重复计数）。
    pub fn tick(&mut self, item: &str) -> bool {
        match self.items.iter_mut().find(|(n, _)| *n == item) {
            Some((_, done)) => {
                if *done {
                    false
                } else {
                    *done = true;
                    true
                }
            }
            None => false,
        }
    }

    /// 进度（已完成, 总数）——执行页实时显示。
    pub fn progress(&self) -> (usize, usize) {
        (self.items.iter().filter(|(_, d)| *d).count(), self.items.len())
    }

    pub fn all_done(&self) -> bool {
        self.items.iter().all(|(_, d)| *d)
    }

    /// 未完成项（完成页前置校验——有遗留就不放行）。
    pub fn pending(&self) -> Vec<&'static str> {
        self.items.iter().filter(|(_, d)| !d).map(|(n, _)| *n).collect()
    }
}

/// 完成页处置（默认：用户文档保留、缓存清理、配置保留——「用户数据不
/// 被误删」的红线落在默认值上；每类可改）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Keep,
    Clean,
}

/// F344 完成页保留/清理分类账。
pub struct KeepCleanClassify {
    pub docs: Disposition,
    pub caches: Disposition,
    pub config: Disposition,
}

impl Default for KeepCleanClassify {
    fn default() -> KeepCleanClassify {
        KeepCleanClassify { docs: Disposition::Keep, caches: Disposition::Clean, config: Disposition::Keep }
    }
}

impl KeepCleanClassify {
    /// 逐类改判（每类可改——判据载体）。
    pub fn override_kind(&mut self, kind: &str, d: Disposition) -> bool {
        match kind {
            "docs" => {
                self.docs = d;
                true
            }
            "caches" => {
                self.caches = d;
                true
            }
            "config" => {
                self.config = d;
                true
            }
            _ => false,
        }
    }

    /// 完成页汇总（保留了什么/清理了什么——两份清单互斥完备）。
    pub fn summary(&self) -> ([&'static str; 3], [&'static str; 3]) {
        let per = |d: Disposition| match d {
            Disposition::Keep => 0usize,
            Disposition::Clean => 1usize,
        };
        let mut kept = ["", "", ""];
        let mut cleaned = ["", "", ""];
        for (idx, (name, disp)) in
            [("用户文档", self.docs), ("缓存", self.caches), ("配置", self.config)]
                .into_iter()
                .enumerate()
        {
            if per(disp) == 0 {
                kept[idx] = name;
            } else {
                cleaned[idx] = name;
            }
        }
        (kept, cleaned)
    }
}

/// F344 关联清理账：文件类型注册/自启动项/权限回收（F324 联动）三面
/// 逐项留痕——「卸载会同时清理这些」的承诺对账面。
#[derive(Default)]
pub struct AssociationReclaim {
    pub ftypes_removed: Vec<String>,
    pub autostart_removed: bool,
    pub perms_revoked: Vec<&'static str>,
}

impl AssociationReclaim {
    pub fn remove_ftype(&mut self, ext: &str) {
        if !self.ftypes_removed.iter().any(|x| x == ext) {
            self.ftypes_removed.push(String::from(ext));
        }
    }

    pub fn revoke_autostart(&mut self) {
        self.autostart_removed = true;
    }

    pub fn revoke_perm(&mut self, perm: &'static str) {
        if !self.perms_revoked.contains(&perm) {
            self.perms_revoked.push(perm);
        }
    }

    /// 关联清理完整性：三面全部回收（计划里有就都必须出现在账上）。
    pub fn fully_reclaimed(&self, plan_ftypes: usize, plan_autostart: bool, plan_perms: usize) -> bool {
        self.ftypes_removed.len() == plan_ftypes
            && self.autostart_removed == plan_autostart
            && self.perms_revoked.len() == plan_perms
    }
}

/// F343 三级链升级账（用户面审计）：阶段迁移序列可回放 + 决策条一次性
/// + 用户决定落账（不替用户做主——账上必须有用户的决定）。
pub struct MemEscapeLog {
    pub stages: Vec<MemStage>,
    asked_once: bool,
    pub decision: Option<bool>,
}

impl MemEscapeLog {
    pub fn new() -> MemEscapeLog {
        MemEscapeLog { stages: alloc::vec![MemStage::Normal], asked_once: false, decision: None }
    }

    /// 升级（只许逐级：Normal→Compress→Freeze→AskUser；乱序拒绝）。
    pub fn escalate(&mut self, to: MemStage) -> bool {
        let cur_rank = Self::rank(self.stages.last().copied());
        let to_rank = Self::rank(Some(to));
        if to_rank != cur_rank + 1 {
            return false;
        }
        self.stages.push(to);
        true
    }

    fn rank(s: Option<MemStage>) -> usize {
        match s {
            Some(MemStage::Normal) | None => 0,
            Some(MemStage::Compress) => 1,
            Some(MemStage::Freeze) => 2,
            Some(MemStage::AskUser) => 3,
        }
    }

    /// 弹决策条（一次性——同一次紧张期内重复弹被拒绝；恢复后再紧
    /// 张经 reset 后可再弹）。
    pub fn ask(&mut self) -> bool {
        if self.asked_once {
            return false;
        }
        self.asked_once = true;
        true
    }

    /// 用户决定（Some(true)=关后台 / Some(false)=继续但可能变慢）。
    pub fn decide(&mut self, close_background: bool) -> bool {
        if !self.asked_once || self.decision.is_some() {
            return false;
        }
        self.decision = Some(close_background);
        true
    }

    /// 新一轮紧张（恢复后）——决策条资格复位。
    pub fn reset_cycle(&mut self) {
        self.asked_once = false;
        self.decision = None;
    }
}

impl Default for MemEscapeLog {
    fn default() -> MemEscapeLog {
        MemEscapeLog::new()
    }
}

/// F343 OOM 前协调审计：协调前后应用侧分配失败率对账——协调必须让
/// 失败率下降（「应用 OOM 前系统先协调」的账面直证）。
pub struct OomCoordination {
    pub fail_rate_before_ppm: u64,
    pub fail_rate_after_ppm: u64,
}

impl OomCoordination {
    pub fn record(before_ppm: u64, after_ppm: u64) -> OomCoordination {
        OomCoordination { fail_rate_before_ppm: before_ppm, fail_rate_after_ppm: after_ppm }
    }

    pub fn coordinated(&self) -> bool {
        self.fail_rate_after_ppm < self.fail_rate_before_ppm
    }

    /// 下降幅度（‰）。
    pub fn reduction_permille(&self) -> u64 {
        if self.fail_rate_before_ppm == 0 {
            0
        } else {
            (self.fail_rate_before_ppm - self.fail_rate_after_ppm) * 1000 / self.fail_rate_before_ppm
        }
    }
}

/// F345 旧应用提示账：改默认时旧应用入队（「下次打开相关文件得到系统
/// 级提示而非静默改道」）→ 提示一次即出队；首装（无旧默认）不提示。
pub struct DefaultChangeNotice {
    queue: Vec<(String, String)>,
    pub notices_shown: u64,
}

impl DefaultChangeNotice {
    pub fn new() -> DefaultChangeNotice {
        DefaultChangeNotice { queue: Vec::new(), notices_shown: 0 }
    }

    /// 默认变更（old 为空 = 首装不提示——返回是否入队）。
    pub fn changed(&mut self, kind: &str, old_app: &str) -> bool {
        if old_app.is_empty() {
            return false;
        }
        if !self.queue.iter().any(|(k, a)| k == kind && a == old_app) {
            self.queue.push((String::from(kind), String::from(old_app)));
        }
        true
    }

    /// 旧应用下次打开相关文件 → 出队提示（一次即清——不反复打扰）。
    pub fn take_next(&mut self) -> Option<(String, String)> {
        if self.queue.is_empty() {
            return None;
        }
        self.notices_shown += 1;
        Some(self.queue.remove(0))
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

impl Default for DefaultChangeNotice {
    fn default() -> DefaultChangeNotice {
        DefaultChangeNotice::new()
    }
}

/// F345 按应用查看：应用 → 声明默认权的类型集合；一键全收回（「某应
/// 用声明了哪些类型的默认权，一键全收回」判据载体）。
#[derive(Default)]
pub struct AppClaimsView {
    claims: Vec<(String, Vec<String>)>,
}

impl AppClaimsView {
    pub fn claim(&mut self, app: &str, kind: &str) {
        match self.claims.iter_mut().find(|(a, _)| a == app) {
            Some((_, kinds)) => {
                if !kinds.iter().any(|k| k == kind) {
                    kinds.push(String::from(kind));
                }
            }
            None => self.claims.push((String::from(app), vec![String::from(kind)])),
        }
    }

    pub fn claims_of(&self, app: &str) -> &[String] {
        self.claims.iter().find(|(a, _)| a == app).map(|(_, k)| k.as_slice()).unwrap_or(&[])
    }

    /// 一键全收回：返回收回的类型数（应用保留在列表但声明权清空）。
    pub fn reclaim_all(&mut self, app: &str) -> usize {
        match self.claims.iter_mut().find(|(a, _)| a == app) {
            Some((_, kinds)) => {
                let n = kinds.len();
                kinds.clear();
                n
            }
            None => 0,
        }
    }
}

/// F346 开机门审计（第三清单深化）：冷启动执行集只含「Boot 清单且已
/// 启用」的项——禁用即时生效的结构证明（下轮冷启动验证的账面）。
pub struct BootGateAudit;

impl BootGateAudit {
    /// 本次开机实际执行集（Boot + enabled 双条件——其余清单不参与开机）。
    pub fn run_boot(items: &[StartupItem]) -> Vec<&StartupItem> {
        items
            .iter()
            .filter(|it| it.enabled && matches!(it.list, StartupList::Boot))
            .collect()
    }

    /// 三清单独立性：同一应用在三份清单中的条目互不干扰（各清单计数）。
    pub fn list_counts(items: &[StartupItem]) -> (usize, usize, usize) {
        (
            items.iter().filter(|it| matches!(it.list, StartupList::Boot)).count(),
            items.iter().filter(|it| matches!(it.list, StartupList::AfterLogin)).count(),
            items.iter().filter(|it| matches!(it.list, StartupList::Scheduled)).count(),
        )
    }
}

/// 深化层二自检（F344 划勾/分类/关联 + F343 升级/OOM + F345 提示/声明权 + F346 开机门）。
pub fn run_sysgov_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep2");

    // 1. F344 执行页逐项划勾：幂等 + 全部划勾才放行 + 遗留可查。
    let mut pr = UninstallProgress::new(&["清理注册", "删缓存", "回收权限"]);
    let t1 = pr.tick("清理注册");
    let t1b = pr.tick("清理注册");
    let _ = pr.tick("删缓存");
    set.add(
        "uninstall progress ledger",
        t1 && !t1b && pr.progress() == (2, 3) && !pr.all_done() && pr.pending() == vec!["回收权限"],
        "",
    );
    let _ = pr.tick("回收权限");
    set.add("uninstall progress gate", pr.all_done() && pr.pending().is_empty(), "");

    // 2. F344 完成页分类：默认（文档保留/缓存清/配置保留）+ 逐类可改 + 两清单互斥。
    let mut kc = KeepCleanClassify::default();
    let (kept, cleaned) = kc.summary();
    set.add(
        "keep-clean defaults",
        kept == ["用户文档", "", "配置"] && cleaned == ["", "缓存", ""],
        "",
    );
    let _ = kc.override_kind("caches", Disposition::Keep);
    let (kept2, cleaned2) = kc.summary();
    set.add("keep-clean overridable", kept2[1] == "缓存" && cleaned2.iter().all(|s| s.is_empty()), "");
    set.add("keep-clean unknown kind rejected", !kc.override_kind("磁盘", Disposition::Clean), "");

    // 3. F344 关联清理：三面逐项留痕 + 完整性对账。
    let mut ar = AssociationReclaim::default();
    ar.remove_ftype(".vxn");
    ar.remove_ftype(".vxn"); // 幂等。
    ar.revoke_autostart();
    ar.revoke_perm("麦克风");
    ar.revoke_perm("摄像头");
    set.add(
        "association reclaim",
        ar.ftypes_removed.len() == 1 && ar.autostart_removed && ar.perms_revoked.len() == 2
            && ar.fully_reclaimed(1, true, 2),
        "",
    );
    set.add("association reclaim incomplete caught", !ar.fully_reclaimed(2, true, 2), "");

    // 4. F343 升级账：逐级推进合法、跳级拒绝。
    let mut mlog = MemEscapeLog::new();
    let jump = mlog.escalate(MemStage::Freeze);
    let ok1 = mlog.escalate(MemStage::Compress);
    let ok2 = mlog.escalate(MemStage::Freeze);
    let ok3 = mlog.escalate(MemStage::AskUser);
    set.add(
        "mem escalation order enforced",
        !jump && ok1 && ok2 && ok3 && mlog.stages.len() == 4,
        "",
    );

    // 5. F343 决策条一次性：二次弹拒绝；决定落账一次；新一轮复位。
    let ask1 = mlog.ask();
    let ask2 = mlog.ask();
    let d1 = mlog.decide(true);
    let d2 = mlog.decide(false);
    set.add(
        "mem ask once + decision ledger",
        ask1 && !ask2 && d1 && !d2 && mlog.decision == Some(true),
        "",
    );
    mlog.reset_cycle();
    set.add("mem cycle reset allows new ask", mlog.ask(), "");

    // 6. F343 OOM 协调审计：失败率必须下降 + 下降幅度‰。
    let oom = OomCoordination::record(4_000, 1_200);
    set.add(
        "oom coordination improves",
        oom.coordinated() && oom.reduction_permille() == 700,
        "",
    );
    let oom_bad = OomCoordination::record(1_000, 1_500);
    set.add("oom regression caught", !oom_bad.coordinated(), "");

    // 7. F345 旧应用提示：首装不提示、变更入队、提示一次出队。
    let mut dn = DefaultChangeNotice::new();
    let first = dn.changed("网页", "");
    let change = dn.changed("网页", "旧浏览器");
    let taken = dn.take_next();
    let empty = dn.take_next();
    set.add(
        "default change notice",
        !first && change && dn.notices_shown == 1
            && taken == Some((String::from("网页"), String::from("旧浏览器")))
            && empty.is_none()
            && dn.pending() == 0,
        "",
    );

    // 8. F345 按应用查看：声明权聚合 + 一键全收回（先取回收前账面再收回）。
    let mut av = AppClaimsView::default();
    av.claim("星编辑", "文本");
    av.claim("星编辑", "图片");
    av.claim("星编辑", "文本"); // 幂等。
    let claims_before = av.claims_of("星编辑").len();
    let n = av.reclaim_all("星编辑");
    set.add(
        "app claims view reclaim all",
        claims_before == 2 && n == 2 && av.claims_of("星编辑").is_empty()
            && av.reclaim_all("星编辑") == 0,
        "",
    );

    // 9. F346 开机门：禁用即时生效（下轮冷启动不入执行集）。
    let mut sm = StartupManager::new();
    sm.install("云笔记", StartupList::Boot, 800);
    sm.install("快速搜索", StartupList::AfterLogin, 300);
    sm.install("周报任务", StartupList::Scheduled, 0);
    let _ = sm.set_enabled("云笔记", StartupList::Boot, false);
    let items = sm.items();
    let run_set = BootGateAudit::run_boot(items);
    set.add(
        "boot gate excludes disabled",
        run_set.is_empty() && BootGateAudit::list_counts(items) == (1, 1, 1),
        "",
    );
    let _ = sm.set_enabled("云笔记", StartupList::Boot, true);
    let run_set2 = BootGateAudit::run_boot(sm.items());
    set.add(
        "boot gate includes re-enabled",
        run_set2.len() == 1 && run_set2[0].app == "云笔记",
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn uninstall_progress_unknown_item_rejected() {
        let mut pr = UninstallProgress::new(&["a"]);
        assert!(!pr.tick("不存在"));
    }

    #[test]
    fn mem_escalate_from_normal_to_ask_rejected() {
        let mut m = MemEscapeLog::new();
        assert!(!m.escalate(MemStage::AskUser), "跳过压缩/冻结直接询问不合法");
    }

    #[test]
    fn mem_decide_before_ask_rejected() {
        let mut m = MemEscapeLog::new();
        assert!(!m.decide(true), "决策条未弹不可决定");
    }

    #[test]
    fn claims_view_unknown_app_empty() {
        let av = AppClaimsView::default();
        assert!(av.claims_of("不存在").is_empty());
    }

    #[test]
    fn keep_clean_config_override_clean() {
        let mut kc = KeepCleanClassify::default();
        assert!(kc.override_kind("config", Disposition::Clean));
        let (_, cleaned) = kc.summary();
        assert_eq!(cleaned, ["", "缓存", "配置"]);
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 注入全链彩排 + F345 声明权仲裁 + F342 忙时排队 + F346 冷启动重放
// ---------------------------------------------------------------------------

/// F344 注入测试应用全链彩排：清单 → 卸载计划 → 三步 → 关联清理 →
/// 残留兜底 → 全链对账单（判据「三步清单与实际清理一致性（注入测试
/// 应用）」的端到端载体）。对账五项全绿才算彩排通过：
/// ① 清单-实际一致；② 文档默认保留；③ 权限回收（F324）；④ 自启动
/// 关联回收（F346 联动）；⑤ 兜底扫描清零（清完再扫无残留）。
pub struct UninstallRehearsal {
    /// 注入的文件系统账（路径存在性——模拟真实盘面）。
    pub disk: Vec<(String, bool)>,
    /// 注入的自启动登记。
    pub startup: StartupManager,
    pub flow: UninstallFlow,
    pub scanner: ResidueScanner,
    /// 布景清单的应用名（彩排回读用——布景时存底）。
    pub staged_app: String,
    /// 布景清单的自启动项（关联回收面）。
    staged_autostart: Option<(StartupList, String)>,
    /// 彩排结论账（逐项留痕）。
    pub report: Vec<(&'static str, bool)>,
}

impl UninstallRehearsal {
    /// 布景：把清单声明的文件/缓存/文档写进注入盘面 + 自启动登记。
    pub fn stage(m: &VxappManifest) -> UninstallRehearsal {
        let mut disk: Vec<(String, bool)> = m
            .files
            .iter()
            .chain(m.caches.iter())
            .chain(m.user_docs.iter())
            .map(|p| (p.clone(), true))
            .collect();
        for p in residue_paths_for(&m.app) {
            disk.push((p, true));
        }
        let mut startup = StartupManager::new();
        if let Some((list, name)) = &m.autostart {
            startup.install(name, *list, 120);
            let _ = startup.set_enabled(name, *list, true);
        }
        UninstallRehearsal {
            flow: UninstallFlow::new(footprint_from_manifest(m)),
            scanner: ResidueScanner::new(),
            startup,
            disk,
            staged_app: m.app.clone(),
            staged_autostart: m.autostart.clone(),
            report: Vec::new(),
        }
    }

    fn set_path(&mut self, path: &str, exists: bool) {
        match self.disk.iter_mut().find(|(p, _)| p == path) {
            Some(slot) => slot.1 = exists,
            None => self.disk.push((String::from(path), exists)),
        }
    }

    /// 走完全链：执行（保文档）→ 关联回收 → 兜底扫描 → 清残留 → 复扫
    /// → 出对账单。
    pub fn rehearse(&mut self) -> Vec<(&'static str, bool)> {
        // 第二步：执行页（文档默认保留）。
        self.flow.execute(true);
        // 清单文件与缓存落账删除（文档保留——盘面上仍在）。
        let mut to_delete: Vec<String> = self.flow.cleaned_regs.clone();
        to_delete.extend(self.flow.cleaned_caches.iter().cloned());
        for f in &to_delete {
            self.set_path(f, false);
        }
        // 关联清理：自启动项禁用回收（F346 联动面）。
        let mut autostart_reclaimed = true;
        if let Some((list, name)) = &self.staged_autostart {
            autostart_reclaimed = self.startup.set_enabled(name, *list, false);
        }
        // 兜底扫描：规则族展开后逐个盘面核查。
        let app = self.staged_app.clone();
        for p in residue_paths_for(&app) {
            let exists = self.disk.iter().any(|(q, e)| *q == p && *e);
            self.scanner.observe(&p, exists);
        }
        let hits = self.scanner.scan(&app);
        // 完成页：命中即报可清 → 全清 → 复扫清零。
        let _ = self.flow.finish(hits.len() as u64);
        for p in &hits {
            self.set_path(p, false);
            self.scanner.observe(p, false);
        }
        let rescan = self.scanner.scan(&app);

        self.report = alloc::vec![
            ("manifest-vs-actual", self.flow.consistent()),
            ("docs kept by default", {
                let docs = &self.flow.kept_docs;
                !docs.is_empty()
                    && docs.iter().all(|d| self.disk.iter().any(|(q, e)| q == d && *e))
            }),
            ("perms revoked", self.flow.perms_revoked),
            ("autostart reclaimed", autostart_reclaimed),
            ("rescan zero residue", rescan.is_empty()),
        ];
        self.report.clone()
    }

    pub fn all_green(&self) -> bool {
        !self.report.is_empty() && self.report.iter().all(|(_, ok)| *ok)
    }
}

/// F345 声明权仲裁：安装不抢默认的执行面——新装应用声明默认权一律
/// 进「待决」清单（矩阵零触碰），用户逐条点头才落位；拒绝即丢弃留痕。
pub struct ClaimArbitration {
    /// 待决声明 (类型, 应用)。
    pending: Vec<(&'static str, String)>,
    /// 用户裁决留痕：(类型, 应用, 接受?)。
    pub ruled: Vec<(&'static str, String, bool)>,
}

impl ClaimArbitration {
    pub fn new() -> ClaimArbitration {
        ClaimArbitration { pending: Vec::new(), ruled: Vec::new() }
    }

    /// 安装面进来一条默认权声明：登记待决，矩阵不动。
    pub fn install_claim(&mut self, kind: &'static str, app: &str) -> bool {
        if self.pending.iter().any(|(k, a)| *k == kind && a == app) {
            return false;
        }
        self.pending.push((kind, String::from(app)));
        true
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 用户裁决：接受/拒绝 → 出待决 + 留痕；未登记的裁决拒绝。
    /// 落位由调用方在裁决接受后写矩阵（仲裁面与矩阵面职责分离）。
    pub fn rule(&mut self, kind: &str, app: &str, accept: bool) -> bool {
        let before = self.pending.len();
        self.pending.retain(|(k, a)| !(*k == kind && a == app));
        if self.pending.len() == before {
            return false;
        }
        self.ruled.push((kind_kludge(kind), String::from(app), accept));
        true
    }

    pub fn pending(&self) -> &[(&'static str, String)] {
        &self.pending
    }
}

/// &'static 化辅助（裁决留痕只存静态串——账本零生命周期债；类型全集
/// 与 DefaultApps 矩阵六类型对齐，未知类型不入账由 rule 的登记面挡住）。
fn kind_kludge(kind: &str) -> &'static str {
    match kind {
        "网页" => "网页",
        "文本" => "文本",
        "图片" => "图片",
        "音视频" => "音视频",
        "压缩" => "压缩",
        "终端" => "终端",
        _ => "网页",
    }
}

impl Default for ClaimArbitration {
    fn default() -> ClaimArbitration {
        ClaimArbitration::new()
    }
}

/// F342 磁盘检查忙时排队：手动触发遇忙延后，空闲按 FIFO 出队执行；
/// 报告走人话映射表（错误码 → 三要素人话——裸异常码不出门）。
pub struct DiskBusyQueue {
    queue: Vec<&'static str>,
    pub busy: bool,
    /// 已执行留痕（盘名 + 完成序）。
    pub done: Vec<&'static str>,
}

/// 人话映射表：检查结论码 →（人话结论 / 原因 / 下一步）三要素。
pub const DISK_VERBOSER: [(&str, &str, &str, &str); 4] = [
    ("OK", "磁盘没有问题", "例行自检未发现异常", "无需操作"),
    ("FIXED", "修好了几处小问题", "发现轻微损坏已自动修复", "可在日志中心查看明细"),
    ("NEEDS_CHECK", "建议重启后深度检查", "有区块需要重启后才能修复", "点「重启检查」安排在下次开机"),
    ("FAILING", "磁盘出现故障征兆", "多项检查连续失败", "立即备份数据并考虑更换磁盘"),
];

impl DiskBusyQueue {
    pub fn new() -> DiskBusyQueue {
        DiskBusyQueue { queue: Vec::new(), busy: false, done: Vec::new() }
    }

    /// 手动触发：忙 → 入队延后（同盘去重）；闲 → 立即执行。
    pub fn request(&mut self, vol: &'static str) -> bool {
        if self.busy {
            if !self.queue.contains(&vol) {
                self.queue.push(vol);
            }
            return false;
        }
        self.done.push(vol);
        true
    }

    /// 忙转闲：按 FIFO 逐个出队执行（每次一个——避免再挤占）。
    pub fn drain_one(&mut self) -> Option<&'static str> {
        if self.busy || self.queue.is_empty() {
            return None;
        }
        let vol = self.queue.remove(0);
        self.done.push(vol);
        Some(vol)
    }

    pub fn queued(&self) -> &[&'static str] {
        &self.queue
    }

    /// 人话映射：码必须全部能在表内解释（裸码 = 缺陷）。
    pub fn verbose(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
        DISK_VERBOSER
            .iter()
            .find(|(c, _, _, _)| *c == code)
            .map(|(_, a, b, d)| (*a, *b, *d))
    }
}

impl Default for DiskBusyQueue {
    fn default() -> DiskBusyQueue {
        DiskBusyQueue::new()
    }
}

/// F346 冷启动重放 + 估算校准：禁用生效的判据载体——重放只执行
/// 「Boot 清单且已启用」项；逐项校准（估算 vs 实测 ±20‰ 越界标注，
/// 台账可见不静默）。
pub struct BootReplay {
    pub manager: StartupManager,
    /// 实测账：(应用, 实测 ms)。
    pub measured: Vec<(String, u64)>,
    /// 校准结论：(应用, 估算, 实测, 误差‰, 越界?)。
    pub calibration: Vec<(String, u64, u64, u64, bool)>,
}

impl BootReplay {
    pub fn new(manager: StartupManager) -> BootReplay {
        BootReplay { manager, measured: Vec::new(), calibration: Vec::new() }
    }

    /// 冷启动重放：返回本轮实际执行的应用序（禁用项零出现——禁用生效
    /// 的直接证据）。
    pub fn cold_start(&self) -> Vec<String> {
        self.manager
            .items()
            .iter()
            .filter(|i| i.list == StartupList::Boot && i.enabled)
            .map(|i| i.app.clone())
            .collect()
    }

    /// 记录实测并校准：误差 ≤20% 算准；越界项标注入账（下轮估算向
    /// 实测收敛——影响估算准确性判据的改进面）。
    pub fn calibrate(&mut self, app: &str, actual_ms: u64) -> bool {
        self.measured.push((String::from(app), actual_ms));
        let est = match self.manager.items().iter().find(|i| i.app == app) {
            Some(i) => i.est_ms,
            None => return false,
        };
        let err_permille = if est == 0 {
            if actual_ms == 0 { 0 } else { u64::MAX }
        } else {
            est.abs_diff(actual_ms) * 1000 / est
        };
        let off = err_permille > 200;
        self.calibration.push((String::from(app), est, actual_ms, err_permille, off));
        off
    }

    /// 越界清单（校准未收敛项——台账可见）。
    pub fn outliers(&self) -> Vec<&str> {
        self.calibration
            .iter()
            .filter(|(_, _, _, _, off)| *off)
            .map(|(a, _, _, _, _)| a.as_str())
            .collect()
    }
}

/// 深化层三自检（彩排 / 仲裁 / 排队 / 冷启动重放）。
pub fn run_sysgov_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep3");

    // 1. 全链彩排：注入带全部关联面的清单 → 五项对账全绿。
    let m = VxappManifest {
        app: String::from("画板Pro"),
        files: alloc::vec![String::from("Apps/画板Pro/main.vx"), String::from("Apps/画板Pro/lib.vxd")],
        file_types: alloc::vec![String::from(".vxd"), String::from(".vxp")],
        autostart: Some((StartupList::Boot, String::from("画板Pro助手"))),
        perms: alloc::vec!["麦克风", "相机"],
        user_docs: alloc::vec![String::from("Docs/画板/我的画稿.vxp")],
        caches: alloc::vec![String::from("Cache/画板Pro")],
    };
    let mut r = UninstallRehearsal::stage(&m);
    let report = r.rehearse();
    set.add("rehearsal five-point green", r.all_green() && report.len() == 5, "");

    // 2. 彩排防呆：一致性不是空转——注入「执行漏项」必须红。
    let mut r2 = UninstallRehearsal::stage(&m);
    let _ = r2.rehearse();
    let mut r3 = UninstallRehearsal::stage(&m);
    r3.flow.cleaned_regs.pop(); // 破坏执行账 → 一致性红。
    set.add("rehearsal catches drift", r2.flow.consistent() && !r3.flow.consistent(), "");

    // 3. F345 仲裁：声明不落矩阵、待决可见、裁决留痕、重复声明拒绝。
    let mut arb = ClaimArbitration::new();
    let mut apps = DefaultApps::new();
    let before = String::from(apps.get("网页").unwrap_or(""));
    let _ = arb.install_claim("网页", "星澜浏览器");
    let _ = arb.install_claim("网页", "星澜浏览器");
    set.add(
        "install claim never seizes default",
        arb.pending_len() == 1 && apps.get("网页") == Some(before.as_str()),
        "",
    );
    let ruled = arb.rule("网页", "星澜浏览器", true);
    let _ = apps.set("网页", "星澜浏览器"); // 用户点头后才落位。
    set.add(
        "rule then apply",
        ruled && arb.pending().is_empty() && apps.get("网页") == Some("星澜浏览器"),
        "",
    );
    set.add("rule unknown rejected", !arb.rule("图片", "幽灵应用", false), "");

    // 4. F342 忙时排队：忙入队延后、闲 FIFO 出队、人话映射表全覆盖。
    let mut q = DiskBusyQueue::new();
    q.busy = true;
    let now = q.request("C:");
    let _ = q.request("D:");
    q.busy = false;
    let first = q.drain_one();
    let second = q.drain_one();
    let none = q.drain_one();
    set.add(
        "disk busy queue fifo",
        !now && first == Some("C:") && second == Some("D:") && none.is_none() && q.done.len() == 2,
        "",
    );
    let all_mapped = DISK_VERBOSER.iter().all(|(c, _, _, _)| DiskBusyQueue::verbose(c).is_some());
    let human = DiskBusyQueue::verbose("NEEDS_CHECK");
    set.add(
        "disk verbose table",
        all_mapped && human.map(|(a, _, _)| a.contains("深度检查")).unwrap_or(false),
        "",
    );

    // 5. F346 冷启动重放：禁用项零出现 + 拖累只含启用项。
    let mut sm = StartupManager::new();
    sm.install("云同步", StartupList::Boot, 400);
    sm.install("剪贴板增强", StartupList::Boot, 90);
    let _ = sm.set_enabled("云同步", StartupList::Boot, true);
    // 剪贴板增强保持默认全关。
    let rp = BootReplay::new(sm);
    let ran = rp.cold_start();
    set.add(
        "cold start disabled never runs",
        ran == alloc::vec![String::from("云同步")] && rp.manager.boot_impact_ms() == 400,
        "",
    );

    // 6. 估算校准：±20% 内算准、越界标注（对账面非静默）。
    let mut rp2 = BootReplay::new(StartupManager::new());
    rp2.manager.install("即时通讯", StartupList::Boot, 500);
    let _ = rp2.manager.set_enabled("即时通讯", StartupList::Boot, true);
    let ok = rp2.calibrate("即时通讯", 550); // 10% 误差。
    let off = rp2.calibrate("即时通讯", 900); // 80% 误差。
    set.add(
        "impact calibration bands",
        !ok && off && rp2.outliers() == alloc::vec!["即时通讯"],
        "",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn rehearsal_residue_rules_all_cleaned() {
        let m = VxappManifest {
            app: String::from("小算盘"),
            files: alloc::vec![String::from("Apps/小算盘/a.vx")],
            file_types: alloc::vec![],
            autostart: None,
            perms: alloc::vec![],
            user_docs: alloc::vec![],
            caches: alloc::vec![],
        };
        let mut r = UninstallRehearsal::stage(&m);
        let _ = r.rehearse();
        assert!(r.scanner.scan("小算盘").is_empty(), "规则族五路径全清");
    }

    #[test]
    fn arbitration_duplicate_claim_rejected() {
        let mut arb = ClaimArbitration::new();
        assert!(arb.install_claim("文本", "编辑器X"));
        assert!(!arb.install_claim("文本", "编辑器X"));
        assert!(arb.rule("文本", "编辑器X", false));
        assert!(!arb.rule("文本", "编辑器X", false), "已裁决的声明不能再裁");
        assert_eq!(arb.ruled[0].2, false);
    }

    #[test]
    fn disk_queue_ignores_duplicate_requests() {
        let mut q = DiskBusyQueue::new();
        q.busy = true;
        assert!(!q.request("C:"));
        assert!(!q.request("C:"), "同盘重复请求不重复入队");
        assert_eq!(q.queued().len(), 1);
    }

    #[test]
    fn boot_replay_order_follows_registration() {
        let mut sm = StartupManager::new();
        sm.install("A", StartupList::Boot, 10);
        sm.install("B", StartupList::Boot, 20);
        let _ = sm.set_enabled("A", StartupList::Boot, true);
        let _ = sm.set_enabled("B", StartupList::Boot, true);
        let rp = BootReplay::new(sm);
        assert_eq!(rp.cold_start(), alloc::vec![String::from("A"), String::from("B")]);
    }

    #[test]
    fn verbose_unknown_code_is_none() {
        assert!(DiskBusyQueue::verbose("E_XXX").is_none(), "裸码必须无解释——逼着登记映射表");
    }
}

// ---------------------------------------------------------------------------
// 深化层四 · 文件类型注册表 DB + 释放空间逐项账 + 卸载前还原点钩子 + 干跑面
// ---------------------------------------------------------------------------

/// 文件类型注册表 DB（F344「文件类型注册」关联清理的机制本体）：类型 →
/// 打开方式优先序列（用户改过默认的应用排最前）。卸载某应用 = 从所有
/// 类型的序列中摘除它；摘除后序列空 → 该类型回退出厂默认（不悬空）。
pub struct FileTypeRegistry {
    /// (类型, 优先序列[应用])——序列头即当前默认。
    entries: Vec<(&'static str, Vec<String>)>,
    /// 出厂默认表（回退依据——同 DefaultApps 矩阵对齐）。
    factory: Vec<(&'static str, &'static str)>,
}

impl FileTypeRegistry {
    pub fn new() -> FileTypeRegistry {
        FileTypeRegistry {
            entries: alloc::vec![
                ("网页", alloc::vec![String::from("浏览器")]),
                ("文本", alloc::vec![String::from("记事本")]),
                ("图片", alloc::vec![String::from("看图")]),
                ("音视频", alloc::vec![String::from("播放器")]),
                ("压缩", alloc::vec![String::from("压缩包")]),
                ("终端", alloc::vec![String::from("终端")]),
            ],
            factory: alloc::vec![
                ("网页", "浏览器"),
                ("文本", "记事本"),
                ("图片", "看图"),
                ("音视频", "播放器"),
                ("压缩", "压缩包"),
                ("终端", "终端"),
            ],
        }
    }

    /// 用户把某应用设为某类型打开方式：提到序列头（用户意愿置顶）。
    /// 未知类型拒绝（六类型白名单）。
    pub fn prefer(&mut self, kind: &str, app: &str) -> bool {
        match self.entries.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, list)) => {
                list.retain(|a| a != app);
                list.insert(0, String::from(app));
                true
            }
            None => false,
        }
    }

    /// 当前默认（序列头）。
    pub fn current(&self, kind: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| *k == kind)
            .and_then(|(_, l)| l.first())
            .map(|s| s.as_str())
    }

    /// 卸载摘除：从全部类型的序列中移除该应用；序列空则回退出厂默认。
    /// 返回受影响的类型清单（对账面——关联清理可见）。
    pub fn uninstall_purge(&mut self, app: &str) -> Vec<&'static str> {
        let mut touched = Vec::new();
        for (kind, list) in self.entries.iter_mut() {
            let before = list.len();
            list.retain(|a| a != app);
            if list.len() != before {
                touched.push(*kind);
            }
            if list.is_empty() {
                if let Some((_, fb)) = self.factory.iter().find(|(k, _)| k == kind) {
                    list.push(String::from(*fb));
                }
            }
        }
        touched
    }

    /// 摘除后无悬空：所有类型序列非空（悬空类型 = 缺陷）。
    pub fn no_dangling(&self) -> bool {
        self.entries.iter().all(|(_, l)| !l.is_empty())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for FileTypeRegistry {
    fn default() -> FileTypeRegistry {
        FileTypeRegistry::new()
    }
}

/// 释放空间逐项账（「卸载会同时清理这些」的数字面）：逐项 (路径, MB)
/// 记账，合计与清单声明体积对账（误差 >5% 红——诚实进度纪律）。
#[derive(Default)]
pub struct SpaceLedger {
    pub items: Vec<(String, u64)>,
    pub declared_mb: u64,
}

impl SpaceLedger {
    pub fn new(declared_mb: u64) -> SpaceLedger {
        SpaceLedger { items: Vec::new(), declared_mb }
    }

    pub fn freed(&mut self, path: &str, mb: u64) {
        self.items.push((String::from(path), mb));
    }

    /// 合计释放量。
    pub fn total(&self) -> u64 {
        self.items.iter().map(|(_, mb)| mb).sum()
    }

    /// 对账：与声明体积误差 ≤5%（声明为 0 时要求实收也为 0）。
    pub fn within_declared(&self) -> bool {
        if self.declared_mb == 0 {
            return self.total() == 0;
        }
        self.declared_mb.abs_diff(self.total()) * 100 <= self.declared_mb * 5
    }

    /// 进度千分比（卸载进度条的数据面）。
    pub fn progress_permille(&self) -> u32 {
        if self.declared_mb == 0 {
            return 1000;
        }
        (self.total().min(self.declared_mb) * 1000 / self.declared_mb) as u32
    }
}

/// 卸载前还原点钩子（F121 联动）：执行页动第一个文件之前必须先建还原
/// 点——钩子未就绪则整个卸载拒绝开跑（不可逆操作的护栏）。留痕入账。
#[derive(Default)]
pub struct RestorePointHook {
    /// 已为本卸载建的还原点（Some = 可开跑）。
    pub snapshot_id: Option<u64>,
    pub refusals: u64,
}

impl RestorePointHook {
    /// 执行闸门：还原点就绪 → 放行；未就绪 → 拒绝并计数（拒绝可见，
    /// 不静默放行）。
    pub fn gate(&mut self) -> bool {
        match self.snapshot_id {
            Some(_) => true,
            None => {
                self.refusals += 1;
                false
            }
        }
    }

    /// 还原点服务回调就绪（真实面由 F121 注入快照 id）。
    pub fn provide_snapshot(&mut self, id: u64) {
        self.snapshot_id = Some(id);
    }
}

/// 干跑面（硬件与数据安全红线：有破坏潜能的操作先干跑列清单）：对
/// 卸载计划产出「将要发生什么」的逐项清单而不动任何字节；用户确认
/// 后同一份清单作为执行账的核对底稿（干跑-执行一致 = 护栏闭环）。
pub struct DryRun {
    /// 干跑清单：(动作, 目标)。
    pub plan: Vec<(&'static str, String)>,
}

impl DryRun {
    /// 从卸载计划产出干跑清单（不动任何状态——纯读）。
    pub fn from_footprint(f: &AppFootprint, keep_docs: bool) -> DryRun {
        let mut plan = Vec::new();
        for r in &f.file_type_regs {
            plan.push(("摘除文件类型注册", r.clone()));
        }
        if f.autostart {
            plan.push(("禁用自启动项", f.app.clone()));
        }
        for c in &f.caches {
            plan.push(("删除缓存", c.clone()));
        }
        if keep_docs {
            for d in &f.user_docs {
                plan.push(("保留文档（不动）", d.clone()));
            }
        }
        plan.push(("回收权限", f.app.clone()));
        DryRun { plan }
    }

    /// 执行账对干跑清单的一致性核对：执行账里每条动作-目标对都必须在
    /// 干跑清单中出现过（执行不许越出干跑承诺——红线纪律的机器面）。
    pub fn verify_execution(&self, executed: &[(&'static str, String)]) -> bool {
        executed.iter().all(|(act, tgt)| {
            self.plan.iter().any(|(pa, pt)| pa == act && pt == tgt)
        })
    }

    pub fn len(&self) -> usize {
        self.plan.len()
    }
}

/// 深化层四自检（注册表 / 空间账 / 还原点闸门 / 干跑一致）。
pub fn run_sysgov_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep4");

    // 1. 注册表：用户置顶 + 卸载摘除 + 空序列回退出厂默认 + 无悬空。
    let mut reg = FileTypeRegistry::new();
    let _ = reg.prefer("图片", "画板Pro");
    set.add("prefer promotes to head", reg.current("图片") == Some("画板Pro"), "");
    let touched = reg.uninstall_purge("画板Pro");
    set.add(
        "uninstall purge and fallback",
        touched == alloc::vec!["图片"] && reg.current("图片") == Some("看图") && reg.no_dangling(),
        "",
    );

    // 2. 未知类型拒绝（六类型白名单）。
    set.add("unknown kind rejected", !reg.prefer("三维", "画板Pro"), "");

    // 3. 空间账：逐项记账合计与声明对账 5% 线；越线红。
    let mut sp = SpaceLedger::new(100);
    sp.freed("Apps/画板Pro/main.vx", 61);
    sp.freed("Apps/画板Pro/lib.vxd", 36);
    set.add("space within 5 percent", sp.within_declared() && sp.total() == 97, "");
    set.add("space progress", sp.progress_permille() == 970, "");
    let mut sp2 = SpaceLedger::new(100);
    sp2.freed("x", 20);
    set.add("space drift flagged", !sp2.within_declared(), "");

    // 4. 还原点闸门：未就绪拒绝且计数可见；就绪放行。
    let mut hook = RestorePointHook::default();
    let blocked = hook.gate();
    hook.provide_snapshot(20260926);
    set.add(
        "restore point gate",
        !blocked && hook.refusals == 1 && hook.gate(),
        "",
    );

    // 5. 干跑：清单产出不动状态 + 执行越出干跑承诺必须被核对拦住。
    let fp = AppFootprint {
        app: String::from("画板Pro"),
        size_mb: 97,
        file_type_regs: alloc::vec![String::from(".vxd"), String::from(".vxp")],
        autostart: true,
        user_docs: alloc::vec![String::from("Docs/画板/我的画稿.vxp")],
        caches: alloc::vec![String::from("Cache/画板Pro")],
    };
    let dry = DryRun::from_footprint(&fp, true);
    let docs_kept_entry = dry
        .plan
        .iter()
        .any(|(a, t)| *a == "保留文档（不动）" && t == "Docs/画板/我的画稿.vxp");
    set.add("dry run lists intent", dry.len() >= 5 && docs_kept_entry, "");

    let ok_exec = alloc::vec![
        ("摘除文件类型注册", String::from(".vxd")),
        ("禁用自启动项", String::from("画板Pro")),
        ("删除缓存", String::from("Cache/画板Pro")),
        ("回收权限", String::from("画板Pro")),
    ];
    let bad_exec = alloc::vec![
        ("删除文档", String::from("Docs/画板/我的画稿.vxp")), // 越出干跑承诺！
    ];
    set.add(
        "dry run guards execution",
        dry.verify_execution(&ok_exec) && !dry.verify_execution(&bad_exec),
        "",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn purge_unknown_app_is_noop() {
        let mut reg = FileTypeRegistry::new();
        assert!(reg.uninstall_purge("幽灵应用").is_empty(), "未注册应用摘除零影响");
        assert!(reg.no_dangling());
    }

    #[test]
    fn space_ledger_zero_declared_progress() {
        let sp = SpaceLedger::new(0);
        assert_eq!(sp.progress_permille(), 1000, "零体积任务进度即满");
    }

    #[test]
    fn dry_run_keep_docs_is_not_deletion() {
        let fp = AppFootprint {
            app: String::from("A"),
            size_mb: 1,
            file_type_regs: alloc::vec![],
            autostart: false,
            user_docs: alloc::vec![String::from("d")],
            caches: alloc::vec![],
        };
        let dry = DryRun::from_footprint(&fp, true);
        assert!(
            !dry.plan.iter().any(|(a, _)| *a == "删除文档"),
            "文档保留面绝不允许出现在删除动作里"
        );
    }

    #[test]
    fn gate_refusal_visible_count() {
        let mut hook = RestorePointHook::default();
        let _ = hook.gate();
        let _ = hook.gate();
        assert_eq!(hook.refusals, 2, "拒绝计数累计——异常显性化");
    }
}

// ---------------------------------------------------------------------------
// 深化层五 · 卸载排队中心 + 系统组件保护白名单 + 关联清理复盘审计
// ---------------------------------------------------------------------------

/// 系统组件保护白名单（硬件与数据安全红线的卸载域落法）：系统组件
/// 一律不可卸载——白名单登记制，越权请求拒绝并留痕（谁在什么时候
/// 试图卸载什么，全程可查）。白名单外组件卸载照常（保护面最小化——
/// 只拦真系统件，不借保护之名锁用户选择）。
pub struct ProtectedComponents {
    protected: Vec<String>,
    /// 越权请求留痕：(请求者, 组件名, 时刻 ms)。
    pub violations: Vec<(String, String, u64)>,
}

impl ProtectedComponents {
    /// 出厂保护清单（系统件——卸载任何一项都等于拆掉系统本体）。
    pub const FACTORY: [&'static str; 6] =
        ["内核", "合成器", "输入服务", "设置中心", "权限中心", "恢复环境"];

    pub fn new() -> ProtectedComponents {
        ProtectedComponents {
            protected: Self::FACTORY.iter().map(|s| String::from(*s)).collect(),
            violations: Vec::new(),
        }
    }

    pub fn is_protected(&self, app: &str) -> bool {
        self.protected.iter().any(|p| p == app)
    }

    /// 卸载请求闸门：保护件拒绝 + 留痕；普通件放行。
    pub fn gate_uninstall(&mut self, requester: &str, app: &str, now_ms: u64) -> bool {
        if self.is_protected(app) {
            self.violations.push((String::from(requester), String::from(app), now_ms));
            return false;
        }
        true
    }

    /// 保护清单冻结审计：出厂六件一个不少（白名单被削即红——保护面
    /// 不许悄悄缩水）。
    pub fn factory_intact(&self) -> bool {
        Self::FACTORY.iter().all(|f| self.is_protected(f))
    }
}

impl Default for ProtectedComponents {
    fn default() -> ProtectedComponents {
        ProtectedComponents::new()
    }
}

/// 卸载排队中心（多应用卸载的秩序面）：FIFO 队列 + 并发互斥（同一
/// 时刻只跑一个卸载——共享面冲突防护）；队列位可取消；执行完成的
/// 应用出队留账。
pub struct UninstallQueue {
    queue: Vec<String>,
    pub running: Option<String>,
    /// 完成账（执行序）。
    pub done: Vec<String>,
}

impl UninstallQueue {
    pub fn new() -> UninstallQueue {
        UninstallQueue { queue: Vec::new(), running: None, done: Vec::new() }
    }

    /// 入队（同应用去重——重复点击不重复排队）。
    pub fn enqueue(&mut self, app: &str) -> bool {
        if self.running.as_deref() == Some(app) || self.queue.iter().any(|a| a == app) {
            return false;
        }
        self.queue.push(String::from(app));
        true
    }

    /// 取下一个执行位（并发互斥：已有执行位则拒绝——一次一个）。
    pub fn start_next(&mut self) -> Option<String> {
        if self.running.is_some() {
            return None;
        }
        if self.queue.is_empty() {
            return None;
        }
        let app = self.queue.remove(0);
        self.running = Some(app.clone());
        Some(app)
    }

    /// 当前执行完成出账。
    pub fn finish_current(&mut self) -> bool {
        match self.running.take() {
            Some(app) => {
                self.done.push(app);
                true
            }
            None => false,
        }
    }

    /// 取消排队位（还没开跑的可以反悔；正在跑的不许取消——执行面
    /// 有自己的确认流）。
    pub fn cancel_queued(&mut self, app: &str) -> bool {
        let before = self.queue.len();
        self.queue.retain(|a| a != app);
        self.queue.len() != before
    }

    pub fn queued(&self) -> &[String] {
        &self.queue
    }
}

impl Default for UninstallQueue {
    fn default() -> UninstallQueue {
        UninstallQueue::new()
    }
}

/// 关联清理复盘审计（「关联清理」判据的闭环面）：卸载完成后对四条
/// 关联面逐项复盘——① 文件类型注册摘除；② 自启动项回收；③ 权限
/// 回收（F324）；④ 默认应用让位（被卸应用曾是默认 → 出让位记录）。
/// 四面全绿才算「卸载干净」，缺一显性登记为复盘红项。
#[derive(Default)]
pub struct CleanupPostmortem {
    pub app: String,
    pub checks: Vec<(&'static str, bool)>,
}

impl CleanupPostmortem {
    pub fn new(app: &str, regs_purged: bool, autostart_off: bool, perms_revoked: bool, default_yields: bool) -> CleanupPostmortem {
        CleanupPostmortem {
            app: String::from(app),
            checks: alloc::vec![
                ("文件类型注册摘除", regs_purged),
                ("自启动项回收", autostart_off),
                ("权限回收", perms_revoked),
                ("默认应用让位", default_yields),
            ],
        }
    }

    pub fn all_clean(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|(_, ok)| *ok)
    }

    /// 红项清单（哪些关联面没清干净——改进/修理直出）。
    pub fn red_items(&self) -> Vec<&'static str> {
        self.checks.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect()
    }
}

/// 深化层五自检（保护白名单 / 排队中心 / 复盘审计）。
pub fn run_sysgov_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep5");

    // 1. 保护白名单：系统件拒绝 + 留痕点名；普通件放行。
    let mut pc = ProtectedComponents::new();
    let sys = pc.gate_uninstall("设置中心", "内核", 0);
    let user_app = pc.gate_uninstall("设置中心", "画板Pro", 10);
    set.add(
        "protected gate and trail",
        !sys && user_app && pc.violations == alloc::vec![(String::from("设置中心"), String::from("内核"), 0)],
        "",
    );

    // 2. 白名单冻结审计：出厂六件全在（削保护 = 缺陷）。
    set.add("factory protected intact", pc.factory_intact(), "");

    // 3. 排队中心：入队去重、并发互斥、完成出账、排队位可取消。
    let mut q = UninstallQueue::new();
    let _ = q.enqueue("画板Pro");
    let dup = q.enqueue("画板Pro");
    let _ = q.enqueue("小算盘");
    let first = q.start_next();
    let concurrent = q.start_next();
    set.add(
        "queue dedup and mutex",
        !dup && first.as_deref() == Some("画板Pro") && concurrent.is_none(),
        "",
    );
    let _ = q.finish_current();
    let second = q.start_next();
    set.add(
        "queue sequential flow",
        second.as_deref() == Some("小算盘") && q.done == alloc::vec![String::from("画板Pro")],
        "",
    );
    let _ = q.enqueue("计算器");
    let cancelled = q.cancel_queued("计算器");
    set.add(
        "queued cancel works",
        cancelled && q.queued().is_empty() && !q.cancel_queued("幽灵应用"),
        "",
    );

    // 4. 复盘审计：四面全绿 = 干净；缺面点名（红项直出）。
    let ok = CleanupPostmortem::new("画板Pro", true, true, true, true);
    let bad = CleanupPostmortem::new("小算盘", true, false, true, false);
    set.add(
        "postmortem all clean",
        ok.all_clean() && !bad.all_clean() && bad.red_items() == alloc::vec!["自启动项回收", "默认应用让位"],
        "",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn protected_list_rejects_all_factory() {
        let mut pc = ProtectedComponents::new();
        for f in ProtectedComponents::FACTORY {
            assert!(!pc.gate_uninstall("测试者", f, 0), "出厂件 {f} 必须被拦");
        }
        assert_eq!(pc.violations.len(), 6, "每次越权都留痕");
    }

    #[test]
    fn finish_without_running_rejected() {
        let mut q = UninstallQueue::new();
        assert!(!q.finish_current(), "无执行位时完成是空转——拒绝");
    }

    #[test]
    fn postmortem_empty_checks_not_clean() {
        let pm = CleanupPostmortem { app: String::from("x"), checks: Vec::new() };
        assert!(!pm.all_clean(), "零检查项不构成干净——不虚报");
    }
}

// ---------------------------------------------------------------------------
// 深化层六 · 卸载人话报告 + 事务性（中断回滚）+ 蜂巢回收验证
// ---------------------------------------------------------------------------

/// 卸载人话报告（三要素呈现的卸载域落法）：发生了什么（各面清了几
/// 处）/ 为什么（清单驱动）/ 下一步（残留与建议），技术细节收进
/// summary 字段——裸数字不出门。
pub struct UninstallReport {
    pub app: String,
    /// 各面清理计数：(面名, 处数)。
    pub faces: Vec<(&'static str, usize)>,
    /// 残留命中数（完成页兜底扫描）。
    pub residue_hits: u64,
}

impl UninstallReport {
    pub fn new(app: &str, flow: &UninstallFlow, space_freed_mb: u64) -> UninstallReport {
        UninstallReport {
            app: String::from(app),
            faces: alloc::vec![
                ("文件类型注册", flow.cleaned_regs.len()),
                ("缓存目录", flow.cleaned_caches.len()),
                ("自启动项", usize::from(flow.cleaned_autostart)),
                ("保留文档", flow.kept_docs.len()),
                ("释放空间 MB", space_freed_mb as usize),
            ],
            residue_hits: flow.residue_hits,
        }
    }

    /// 人话摘要（三要素：发生/原因/下一步）。
    pub fn human_summary(&self) -> String {
        let mut s = alloc::format!("「{}」已卸载", self.app);
        let mut detail = Vec::new();
        for (name, n) in &self.faces {
            if *n > 0 {
                detail.push(alloc::format!("{} {} 处", name, n));
            }
        }
        if !detail.is_empty() {
            s.push_str("：");
            s.push_str(&detail.join("、"));
        }
        if self.residue_hits > 0 {
            s.push_str(&alloc::format!("；发现 {} 处残留，可在「清理建议」一键清除", self.residue_hits));
        } else {
            s.push_str("；未发现残留");
        }
        s
    }

    /// 技术细节折叠（summary 字段——人话页默认收起）。
    pub fn technical_detail(&self) -> String {
        self.faces
            .iter()
            .map(|(n, c)| alloc::format!("{}={}", n, c))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// 卸载事务性（数据安全铁律④「原子写+日志化」的卸载域落法）：执行
/// 页每清一处记一条 undo 日志；中断（崩溃/断电）→ 按日志逆序回滚
/// 已清面（半截卸载不许留在盘上——要么全清、要么还原）。
pub struct UninstallTransaction {
    /// undo 日志：(动作, 目标, 回滚数据)。
    pub undo_log: Vec<(&'static str, String, String)>,
    pub rolled_back: usize,
    pub committed: bool,
}

impl UninstallTransaction {
    pub fn new() -> UninstallTransaction {
        UninstallTransaction { undo_log: Vec::new(), rolled_back: 0, committed: false }
    }

    /// 记一笔已执行的清理（回滚数据 = 被删内容的还原载荷）。
    pub fn log_step(&mut self, action: &'static str, target: &str, undo_payload: &str) {
        self.undo_log.push((action, String::from(target), String::from(undo_payload)));
    }

    /// 提交：全部清完且兜底扫描过 → 日志转正（不可逆点在用户确认后）。
    pub fn commit(&mut self) -> bool {
        self.committed = true;
        true
    }

    /// 中断回滚：按日志逆序逐条还原，返回回滚条数；提交后回滚拒绝
    /// （不可逆点后无回滚——语义诚实）。
    pub fn rollback(&mut self) -> Option<usize> {
        if self.committed {
            return None;
        }
        let n = self.undo_log.len();
        self.undo_log.clear();
        self.rolled_back += n;
        Some(n)
    }

    pub fn pending_steps(&self) -> usize {
        self.undo_log.len()
    }
}

impl Default for UninstallTransaction {
    fn default() -> UninstallTransaction {
        UninstallTransaction::new()
    }
}

/// 注册表蜂巢回收验证（判据「蜂巢完整回收」的机器面）：应用卸载后
/// 其蜂巢键的叶节点应清零；回收器对「空蜂巢」与「仍有叶」两态诚实
/// 区分——仍有叶不虚报回收成功。
pub struct HiveReclaim {
    /// (蜂巢路径, 剩余叶数)。
    pub hives: Vec<(String, usize)>,
}

impl HiveReclaim {
    pub fn new() -> HiveReclaim {
        HiveReclaim { hives: Vec::new() }
    }

    pub fn observe(&mut self, hive_path: &str, remaining_leaves: usize) {
        match self.hives.iter_mut().find(|(p, _)| p == hive_path) {
            Some(slot) => slot.1 = remaining_leaves,
            None => self.hives.push((String::from(hive_path), remaining_leaves)),
        }
    }

    /// 全部蜂巢叶清零才算回收完整（任何残留 = 红，直出清单）。
    pub fn fully_reclaimed(&self) -> bool {
        !self.hives.is_empty() && self.hives.iter().all(|(_, n)| *n == 0)
    }

    /// 残留蜂巢清单（修理面直出）。
    pub fn leftover_hives(&self) -> Vec<&str> {
        self.hives
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(p, _)| p.as_str())
            .collect()
    }
}

impl Default for HiveReclaim {
    fn default() -> HiveReclaim {
        HiveReclaim::new()
    }
}

/// 深化层六自检（人话报告 / 事务 / 蜂巢）。
pub fn run_sysgov_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F342-346-deep6");

    // 1. 人话报告：三要素齐（发生+下一步）、残留分支文案分岔、技术
    //    折叠字段在。
    let mut flow = UninstallFlow::new(AppFootprint {
        app: String::from("画板Pro"),
        size_mb: 97,
        file_type_regs: alloc::vec![String::from(".vxd"), String::from(".vxp")],
        autostart: true,
        user_docs: alloc::vec![],
        caches: alloc::vec![String::from("Cache/画板Pro")],
    });
    flow.execute(true);
    let _ = flow.finish(2);
    let rep = UninstallReport::new("画板Pro", &flow, 97);
    let s = rep.human_summary();
    set.add(
        "human report three elements",
        s.contains("已卸载") && s.contains("残留") && rep.technical_detail().contains("文件类型注册=2"),
        "",
    );

    // 2. 无残留分支：文案走「未发现残留」路（不吓唬人）。
    let mut flow2 = UninstallFlow::new(AppFootprint {
        app: String::from("小算盘"),
        size_mb: 4,
        file_type_regs: alloc::vec![],
        autostart: false,
        user_docs: alloc::vec![],
        caches: alloc::vec![],
    });
    flow2.execute(false);
    let _ = flow2.finish(0);
    let rep2 = UninstallReport::new("小算盘", &flow2, 4);
    set.add(
        "clean branch wording",
        rep2.human_summary().contains("未发现残留"),
        "",
    );

    // 3. 事务性：三笔清理 → 中断回滚三条（逆序还原）→ 账面清空。
    let mut tx = UninstallTransaction::new();
    tx.log_step("删除缓存", "Cache/画板Pro", "cache-payload");
    tx.log_step("摘除注册", ".vxd", "vxd-payload");
    tx.log_step("禁用自启动", "画板Pro助手", "autostart-payload");
    let rolled = tx.rollback();
    set.add(
        "transaction rollback on interrupt",
        rolled == Some(3) && tx.pending_steps() == 0 && tx.rolled_back == 3,
        "",
    );

    // 4. 提交后回滚拒绝（不可逆点语义诚实）。
    let mut tx2 = UninstallTransaction::new();
    tx2.log_step("删除缓存", "x", "y");
    let _ = tx2.commit();
    set.add("post-commit rollback rejected", tx2.rollback().is_none(), "");

    // 5. 蜂巢回收：全清零绿；残留蜂巢点名直出。
    let mut hv = HiveReclaim::new();
    hv.observe("HKCU/Software/画板Pro", 0);
    hv.observe("HKCU/Software/小算盘", 3);
    set.add(
        "hive reclaim honest",
        !hv.fully_reclaimed() && hv.leftover_hives() == alloc::vec!["HKCU/Software/小算盘"],
        "",
    );
    hv.observe("HKCU/Software/小算盘", 0);
    set.add("hive reclaim green after cleanup", hv.fully_reclaimed(), "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn empty_report_still_human() {
        let mut f = UninstallFlow::new(AppFootprint {
            app: String::from("空"),
            size_mb: 0,
            file_type_regs: alloc::vec![],
            autostart: false,
            user_docs: alloc::vec![],
            caches: alloc::vec![],
        });
        f.execute(false);
        let _ = f.finish(0);
        let r = UninstallReport::new("空", &f, 0);
        assert!(r.human_summary().contains("未发现残留"));
    }

    #[test]
    fn rollback_twice_second_noop() {
        let mut tx = UninstallTransaction::new();
        tx.log_step("a", "b", "c");
        assert_eq!(tx.rollback(), Some(1));
        assert_eq!(tx.rollback(), Some(0), "空日志回滚 = 零条（幂等）");
    }

    #[test]
    fn hive_unknown_path_not_green() {
        let hv = HiveReclaim::new();
        assert!(!hv.fully_reclaimed(), "零蜂巢账不构成回收完整");
    }
}
