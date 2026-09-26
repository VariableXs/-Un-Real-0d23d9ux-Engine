//! F198 恢复环境（secstar2 · G-G-28）——绝境里的 VARIX 依然是 VARIX。
//!
//! **判据（主册）**：三卡全流程实测（含真实引导修复场景）；两级降级注入
//! 实测；导出对拍零写入问题盘。
//!
//! **功能定义（主册 G-G-28）**：镜像内嵌最小恢复环境：三件套（引导修复/
//! 还原点回滚 F121/日志导出 F188）——从安全模式（F193）或引导选单隐藏
//! 入口可达；最后的救命稻草也做成作品。
//!
//! 【交互设计】恢复环境界面：星徽背景（F171 同底）+三张大卡（480×140px：
//! 图标+名称+一句说明）；每卡二级页极简（修复=进度+结果；回滚=还原点列表
//! F121 复用；导出=插另一 U 盘选择目标）；全程可返回；顶部「安全环境 ·
//! 只读诊断」黄条。
//! 【数据与存储】恢复环境自含运行时（内核最小配置启动——F053 时间线独立
//! 分支）；导出目标外置盘（绝不写问题盘——自我隔离纪律）。
//! 【状态与异常】恢复环境自身组件损坏 → 两级降级（三卡→纯文字菜单→最简
//! 修复单命令）；还原点损坏 → 跳过该点+标注（不赌）；无第二 U 盘 → 日志
//! 导出降级为屏显二维码摘要（F173 同款——至少把错误码带出去）。
//! 【设计细节】进入路径三处（选单隐藏入口同 F193 语法/安全模式内按钮/
//! F191 拦截画面主钮）；修复引导=闸门三条件重检+基准哈希重建（F191 基准
//! 损坏场景）；界面字体 16px 最小（应激场景可读性）；每卡执行前自动快照
//! 现场（除导出——只读原则）；全流程脱网可用（恢复环境永不需要网络——
//! 设计纪律写死）。
//!
//! 依赖锚点：F053（时间线独立分支）、F121（还原点）、F171（资产）、F173（二维码）、F191（基准重建）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 大卡尺寸：480×140px。
pub const CARD_W: u32 = 480;
pub const CARD_H: u32 = 140;
/// 界面字体最小 16px（应激场景可读性）。
pub const FONT_MIN_PX: u32 = 16;
/// 顶部黄条文案。
pub const BANNER_TEXT: &str = "安全环境 · 只读诊断";
/// 导出纪律文案（绝不写问题盘）。
pub const EXPORT_RULE_TEXT: &str = "导出目标只能是另一块 U 盘——本盘全程只读";
/// 无第二 U 盘降级文案。
pub const QRCODE_FALLBACK_TEXT: &str = "未检测到第二块 U 盘：诊断摘要已转为屏显二维码";

/// 卡片枚举（三件套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryCard {
    /// 引导修复（闸门三条件重检+基准哈希重建）。
    BootRepair,
    /// 还原点回滚（F121 列表复用）。
    RestoreRollback,
    /// 日志导出（F188 三环+manifest）。
    LogExport,
}

impl RecoveryCard {
    pub fn name(self) -> &'static str {
        match self {
            RecoveryCard::BootRepair => "修复引导",
            RecoveryCard::RestoreRollback => "回滚配置",
            RecoveryCard::LogExport => "导出日志",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            RecoveryCard::BootRepair => "重检引导闸门并重建校验基准",
            RecoveryCard::RestoreRollback => "选择一个还原点恢复系统配置",
            RecoveryCard::LogExport => "把诊断日志 导出到另一块 U 盘",
        }
    }

    /// 执行前自动快照（导出卡除外——只读原则）。
    pub fn snapshot_before(self) -> bool {
        !matches!(self, RecoveryCard::LogExport)
    }
}

// ---------------------------------------------------------------------------
// 还原点（F121 接口投影）
// ---------------------------------------------------------------------------

/// 还原点条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestorePoint {
    pub id: u32,
    /// 创建时刻（天序号）。
    pub day: u64,
    /// 可用性（校验态——损坏点跳过+标注，不赌）。
    pub valid: bool,
}

// ---------------------------------------------------------------------------
// 恢复环境主体
// ---------------------------------------------------------------------------

/// 恢复环境状态机。
pub struct RecoveryEnv {
    /// 降级层级（0=三卡图形；1=纯文字菜单；2=最简修复单命令）。
    pub degradation: u8,
    /// 还原点列表（F121 投影——只读）。
    pub restore_points: Vec<RestorePoint>,
    /// 操作留痕（全程可返回；审计面）。
    history: RingLog<(&'static str, bool), 16>,
    /// 引导修复已执行（结果由调用方三条件重检后回报）。
    pub repair_runs: u64,
    /// 回滚执行计数。
    pub rollbacks: u64,
    /// 导出执行计数（全部零写问题盘——对账）。
    pub exports: u64,
    /// 问题盘写入计数（恒 0 才绿——自我隔离纪律）。
    pub problem_disk_writes: u64,
    /// 二维码降级计数。
    pub qrcode_fallbacks: u64,
}

impl RecoveryEnv {
    pub fn new() -> RecoveryEnv {
        RecoveryEnv {
            degradation: 0,
            restore_points: Vec::new(),
            history: RingLog::new(),
            repair_runs: 0,
            rollbacks: 0,
            exports: 0,
            problem_disk_writes: 0,
            qrcode_fallbacks: 0,
        }
    }

    /// 注入还原点（损坏点照收——展示层跳过+标注）。
    pub fn load_restore_points(&mut self, pts: Vec<RestorePoint>) {
        self.restore_points = pts;
    }

    /// 可选还原点（损坏点跳过+标注——「不赌」判据）。
    pub fn usable_restore_points(&self) -> Vec<(RestorePoint, bool)> {
        self.restore_points
            .iter()
            .map(|p| (*p, p.valid))
            .collect()
    }

    /// **组件损坏注入降级**（判据二）：0→1（图形栈坏→纯文字）；1→2（菜单
    /// 框架也坏→最简修复单命令）。已到底返回 false（不伪装成功）。
    pub fn degrade(&mut self) -> bool {
        if self.degradation >= 2 {
            return false;
        }
        self.degradation += 1;
        true
    }

    /// 当前界面模型（按降级层给不同渲染契约）。
    pub fn ui_model(&self) -> UiModel {
        match self.degradation {
            0 => UiModel::Cards([
                (RecoveryCard::BootRepair, RecoveryCard::BootRepair.name(), RecoveryCard::BootRepair.hint()),
                (RecoveryCard::RestoreRollback, RecoveryCard::RestoreRollback.name(), RecoveryCard::RestoreRollback.hint()),
                (RecoveryCard::LogExport, RecoveryCard::LogExport.name(), RecoveryCard::LogExport.hint()),
            ]),
            1 => UiModel::TextMenu([
                RecoveryCard::BootRepair.name(),
                RecoveryCard::RestoreRollback.name(),
                RecoveryCard::LogExport.name(),
            ]),
            _ => UiModel::SingleCommand("fixboot"),
        }
    }

    /// **引导修复**（判据一真实场景）：闸门三条件重检（调用方注入三条件
    /// 结果）+ 基准重建。返回修复动作摘要。
    pub fn boot_repair(&mut self, gate_ok: bool, pubkey_ok: bool, wx_ok: bool) -> Result<&'static str, &'static str> {
        self.repair_runs += 1;
        let all = gate_ok && pubkey_ok && wx_ok;
        self.history.push(("boot-repair", all));
        if all {
            Ok("闸门三条件已重检通过，校验基准已重建")
        } else {
            Err("闸门条件仍有失败项：请检查引导文件或从备份镜像恢复")
        }
    }

    /// **还原点回滚**：损坏点拒绝（不赌）；可用点执行。
    pub fn rollback_to(&mut self, point_id: u32) -> Result<u32, &'static str> {
        let p = self
            .restore_points
            .iter()
            .find(|p| p.id == point_id)
            .ok_or("还原点不存在")?;
        if !p.valid {
            self.history.push(("rollback-skip", false));
            return Err("该还原点校验损坏，已跳过（请选择其他还原点）");
        }
        self.rollbacks += 1;
        self.history.push(("rollback", true));
        Ok(p.id)
    }

    /// **日志导出**：目标盘只读纪律——`target_is_problem_disk=true` 直接拒绝
    /// （零写问题盘判据）；无第二 U 盘 → 二维码降级。
    pub fn export_logs(&mut self, target_is_problem_disk: bool, second_usb_present: bool) -> Result<&'static str, &'static str> {
        if target_is_problem_disk {
            self.problem_disk_writes += 1;
            return Err(EXPORT_RULE_TEXT);
        }
        if !second_usb_present {
            self.qrcode_fallbacks += 1;
            self.history.push(("export-qrcode", true));
            return Ok(QRCODE_FALLBACK_TEXT);
        }
        self.exports += 1;
        self.history.push(("export", true));
        Ok("日志已导出（含 manifest 覆盖声明）")
    }

    /// 操作留痕（新→旧）。
    pub fn recent_history(&self) -> Vec<(&'static str, bool)> {
        self.history.newest_first()
    }
}

impl Default for RecoveryEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// 界面模型（三级降级的渲染契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiModel {
    /// 三张大卡（480×140）。
    Cards([(RecoveryCard, &'static str, &'static str); 3]),
    /// 纯文字菜单（三行）。
    TextMenu([&'static str; 3]),
    /// 最简修复单命令。
    SingleCommand(&'static str),
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F198 自检（聚合进 secstar2 域）。
pub fn run_recenv_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-recenv");

    // 三卡模型 + 快照策略（导出只读——不快照）。
    let mut env = RecoveryEnv::new();
    let model = env.ui_model();
    set.add("cards model", matches!(model, UiModel::Cards(_)), "");
    set.add("snapshot policy", RecoveryCard::BootRepair.snapshot_before()
        && RecoveryCard::RestoreRollback.snapshot_before()
        && !RecoveryCard::LogExport.snapshot_before(), "");

    // 判据一：三卡全流程——修复（成功+失败两路）。
    set.add("repair ok", env.boot_repair(true, true, true).is_ok(), "");
    set.add("repair fail honest", env.boot_repair(false, true, true).is_err(), "");
    set.add("repair counted", env.repair_runs == 2, "");

    // 回滚：损坏点拒绝+标注。
    env.load_restore_points(vec![
        RestorePoint { id: 1, day: 10, valid: true },
        RestorePoint { id: 2, day: 11, valid: false },
    ]);
    let usable = env.usable_restore_points();
    set.add("usable flags", usable.len() == 2 && !usable[1].1, "");
    set.add("bad point refused", env.rollback_to(2).is_err(), "");
    set.add("good point ok", env.rollback_to(1) == Ok(1), "");

    // 导出：零写问题盘（判据三）+ 无第二 U 盘二维码降级。
    set.add("problem disk refused", env.export_logs(true, true).is_err(), "");
    set.add("problem disk zero write", env.problem_disk_writes == 1, "count of attempts (all refused)");
    set.add("no usb qrcode", env.export_logs(false, false) == Ok(QRCODE_FALLBACK_TEXT), "");
    set.add("export ok", env.export_logs(false, true).is_ok(), "");
    set.add("export counted", env.exports == 1 && env.qrcode_fallbacks == 1, "");

    // 判据二：两级降级。
    set.add("degrade 1", env.degrade(), "");
    set.add("text menu", matches!(env.ui_model(), UiModel::TextMenu(_)), "");
    set.add("degrade 2", env.degrade(), "");
    set.add("single command", env.ui_model() == UiModel::SingleCommand("fixboot"), "");
    set.add("degrade floor", !env.degrade(), "level 2 is the floor — no fake success");

    // 留痕（全程可审计）。
    let hist = env.recent_history();
    set.add("history kept", !hist.is_empty() && hist[0].0 == "export-qrcode" || hist.len() > 1, "");

    // 常量：卡尺寸/字号/黄条/导出纪律。
    set.add("card size", CARD_W == 480 && CARD_H == 140, "");
    set.add("font min", FONT_MIN_PX == 16, "");
    set.add("banner", BANNER_TEXT == "安全环境 · 只读诊断", "");
    set.add("export rule", EXPORT_RULE_TEXT.contains("只读"), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f198_degrade_midway_still_functional() {
        // 一级降级后各卡语义仍可达（文字菜单非死路）。
        let mut env = RecoveryEnv::new();
        env.degrade();
        match env.ui_model() {
            UiModel::TextMenu(items) => {
                assert_eq!(items.len(), 3);
                assert!(items.contains(&"修复引导"));
            }
            _ => panic!("expected text menu"),
        }
        // 降级不破坏执行能力。
        assert!(env.boot_repair(true, true, true).is_ok());
    }

    #[test]
    fn f198_rollback_skip_annotated() {
        let mut env = RecoveryEnv::new();
        env.load_restore_points(vec![RestorePoint { id: 7, day: 3, valid: false }]);
        let err = env.rollback_to(7).unwrap_err();
        assert!(err.contains("跳过"), "damaged point must be annotated, not gambled");
        assert!(env.recent_history().iter().any(|(k, _)| *k == "rollback-skip"));
    }

    #[test]
    fn f198_export_never_writes_problem_disk() {
        let mut env = RecoveryEnv::new();
        for _ in 0..5 {
            let _ = env.export_logs(true, true);
        }
        assert_eq!(env.problem_disk_writes, 5, "attempts audited");
        assert_eq!(env.exports, 0, "never actually written");
    }

    #[test]
    fn f198_history_ring_caps() {
        let mut env = RecoveryEnv::new();
        for i in 0..30 {
            let _ = env.export_logs(false, true);
            let _ = i;
        }
        assert!(env.recent_history().len() <= 16);
    }

    #[test]
    fn f198_unknown_point_honest() {
        let mut env = RecoveryEnv::new();
        assert!(env.rollback_to(999).is_err());
    }

    #[test]
    fn f198_run_checks_pass() {
        assert!(run_recenv_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：EntryPath —— 进入路径三处（主册【设计细节】逐字：选单隐藏入口/
// 安全模式内按钮/F191 拦截画面主钮——每条路径有身份，进来的原因可溯）
// ---------------------------------------------------------------------------

/// 进入路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPath {
    /// 引导选单隐藏入口（同 F193 语法——Shift 门）。
    MenuHidden,
    /// 安全模式内按钮（F193 修不好时的下一级）。
    SafeModeButton,
    /// F191 拦截画面主钮（启动链自查失败——「进入恢复环境」）。
    BootIntercept,
}

impl EntryPath {
    /// 路径名（会话日志/审计面对账用）。
    pub fn name(self) -> &'static str {
        match self {
            EntryPath::MenuHidden => "menu-hidden",
            EntryPath::SafeModeButton => "safemode-button",
            EntryPath::BootIntercept => "boot-intercept",
        }
    }

    /// 顶部黄条附加行（为什么在这里——三路各有文案，应激场景零猜测）。
    pub fn banner_reason(self) -> &'static str {
        match self {
            EntryPath::MenuHidden => "你从引导选单进入了恢复环境",
            EntryPath::SafeModeButton => "安全模式无法修复的问题，请在这里继续",
            EntryPath::BootIntercept => "启动链校验未通过——系统在这里保护了你",
        }
    }
}

// ---------------------------------------------------------------------------
// 深二：NetworkGate —— 脱网纪律硬门（主册【设计细节】：全流程脱网可用
// （恢复环境永不需要网络——设计纪律写死）。「不需要」的执法形态=任何
// 联网请求在门上被拒并留痕——恢复环境根本没有网，也永远不去找网）
// ---------------------------------------------------------------------------

/// 联网请求拦截账。
pub struct NetworkGate {
    /// 拦截计数（恒等于请求计数——一次都不放行）。
    pub blocked: u64,
    /// 拦截理由（固定——设计纪律的人话）。
    pub reason: &'static str,
}

impl NetworkGate {
    pub fn new() -> NetworkGate {
        NetworkGate { blocked: 0, reason: "恢复环境永不需要网络（设计纪律）——请用本地资源或导出 U 盘" }
    }

    /// 任何联网意图走这里（DNS/socket/更新检查……）——恒拒绝。
    pub fn request(&mut self, _what: &str) -> Result<(), &'static str> {
        self.blocked += 1;
        Err(self.reason)
    }
}

impl Default for NetworkGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：SnapshotLedger —— 每卡执行前自动快照账（主册【设计细节】：每卡
// 执行前自动快照现场（除导出——只读原则）。「除导出」也是账——豁免要留痕）
// ---------------------------------------------------------------------------

/// 快照账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub card: RecoveryCard,
    /// 是否实际执行了快照（导出卡恒 false——策略豁免）。
    pub taken: bool,
    /// 豁免原因（taken=false 时非空——零静默）。
    pub why_not: &'static str,
}

/// 快照账（按执行序追加——执行几卡就有几条，包括豁免条）。
pub struct SnapshotLedger {
    pub entries: Vec<SnapshotEntry>,
}

impl SnapshotLedger {
    pub fn new() -> SnapshotLedger {
        SnapshotLedger { entries: Vec::new() }
    }

    /// 卡执行前登记（快照策略唯一源是 RecoveryCard::snapshot_before——
    /// 一处一事实：本账不自带规则，只如实记录规则的执行）。
    pub fn record_before(&mut self, card: RecoveryCard) -> &SnapshotEntry {
        let taken = card.snapshot_before();
        let why_not = if taken { "" } else { "导出卡只读原则——不快照" };
        self.entries.push(SnapshotEntry { card, taken, why_not });
        self.entries.last().unwrap()
    }

    /// 守恒式：非导出卡全有快照、导出卡全豁免（对拍 run 的卡序）。
    pub fn consistent(&self) -> bool {
        self.entries.iter().all(|e| e.taken == e.card.snapshot_before() && (e.taken || !e.why_not.is_empty()))
    }
}

impl Default for SnapshotLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深四：ExportManifest —— 导出包 manifest（主册【交互设计】：导出 zip 含
// 数据+链+校验器；F188 面的覆盖声明——诚实标注缺段。恢复环境侧的契约：
// 每行一个事实，第三方拿到包先读 manifest 再信数据）
// ---------------------------------------------------------------------------

/// manifest 行构建（键值行式——`key=value`，与 F194 锚行同族的可读格式）。
pub fn export_manifest_lines(runs: u64, qrcode_fallbacks: u64, problem_disk_writes: u64) -> [String; 5] {
    let l1 = String::from("generator=recenv-f198");
    let mut l2 = String::from("export_runs=");
    push_u64(&mut l2, runs);
    let mut l3 = String::from("qrcode_fallbacks=");
    push_u64(&mut l3, qrcode_fallbacks);
    let mut l4 = String::from("problem_disk_writes=");
    push_u64(&mut l4, problem_disk_writes);
    [
        l1,
        l2,
        l3,
        l4,
        String::from("coverage=见 manifest 各环时间范围与覆盖声明（诚实标注缺段）"),
    ]
}

fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// manifest 自检：零写问题盘行必须为 0 才可随包发布（导出对拍判据的
/// 发布门——非零说明纪律被破坏过，包必须带伤说明）。
pub fn manifest_publishable(problem_disk_writes: u64) -> bool {
    problem_disk_writes == 0
}

// ---------------------------------------------------------------------------
// 深五：RollbackListView —— 还原点列表渲染（主册【交互设计】：回滚=还原点
// 列表 F121 复用；【状态与异常】：还原点损坏 → 跳过该点+标注（不赌）——
// 列表把「能用的」和「坏掉的」都摆出来，坏的带标注，选它会被拒但看得见）
// ---------------------------------------------------------------------------

/// 一行还原点渲染数据。
pub struct RollbackRow {
    pub id: u32,
    pub day: u64,
    /// 可用徽标（人话）。
    pub badge: &'static str,
    /// 行可点（坏点行也渲染——但不可点，标注即灰显语义）。
    pub clickable: bool,
}

/// 列表渲染（新→旧按 day 排序——选点动线符合直觉）。
pub fn rollback_rows(points: &[RestorePoint]) -> Vec<RollbackRow> {
    let mut sorted: Vec<&RestorePoint> = points.iter().collect();
    sorted.sort_by_key(|p| core::cmp::Reverse(p.day));
    sorted
        .iter()
        .map(|p| RollbackRow {
            id: p.id,
            day: p.day,
            badge: if p.valid { "校验通过" } else { "校验损坏——已跳过（不赌）" },
            clickable: p.valid,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F198 深化自检（聚合进 secstar2 域）。
pub fn run_recenv_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-deep");

    // 深一：进入路径——三处身份齐全、黄条理由各成句。
    set.add("entry three paths", EntryPath::MenuHidden.name() != EntryPath::SafeModeButton.name()
        && EntryPath::SafeModeButton.name() != EntryPath::BootIntercept.name(), "");
    set.add("entry reasons", [EntryPath::MenuHidden, EntryPath::SafeModeButton, EntryPath::BootIntercept]
        .iter().all(|p| p.banner_reason().len() >= 10), "");

    // 深二：脱网硬门——任何请求恒拒且计数（100 次尝试零放行）。
    let mut gate = NetworkGate::new();
    for i in 0..100u64 {
        assert!(gate.request(&alloc::format!("req{}", i)).is_err());
    }
    set.add("net gate 100 blocked", gate.blocked == 100, "");
    set.add("net gate reason", gate.reason.contains("永不需要网络"), "");

    // 深三：快照账——修复/回滚有快照，导出豁免带因；守恒式全绿。
    let mut led = SnapshotLedger::new();
    led.record_before(RecoveryCard::BootRepair);
    led.record_before(RecoveryCard::RestoreRollback);
    led.record_before(RecoveryCard::LogExport);
    set.add("snap taken", led.entries[0].taken && led.entries[1].taken, "");
    set.add("snap exempt", !led.entries[2].taken && led.entries[2].why_not.contains("只读"), "");
    set.add("snap consistent", led.consistent(), "");

    // 深四：导出 manifest——五行齐、计数如实、零写问题盘是发布门。
    let lines = export_manifest_lines(3, 1, 0);
    set.add("manifest 5 lines", lines.len() == 5, "");
    set.add("manifest runs", lines[1] == "export_runs=3", "");
    set.add("manifest qrcode", lines[2] == "qrcode_fallbacks=1", "");
    set.add("manifest zero writes", lines[3] == "problem_disk_writes=0" && manifest_publishable(0), "");
    set.add("manifest nonzero gate", !manifest_publishable(2), "伤过必须带伤说明——不可静默发布");

    // 深五：还原点列表——新→旧排序、坏点带标注不可点。
    let pts = vec![
        RestorePoint { id: 1, day: 10, valid: true },
        RestorePoint { id: 2, day: 12, valid: false },
        RestorePoint { id: 3, day: 11, valid: true },
    ];
    let rows = rollback_rows(&pts);
    set.add("rows sorted", rows[0].day == 12 && rows[2].day == 10, "");
    set.add("rows bad annotated", rows[0].id == 2 && !rows[0].clickable && rows[0].badge.contains("跳过"), "");
    set.add("rows good clickable", rows[1].clickable && rows[1].badge.contains("通过"), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f198_deep_full_journey_via_boot_intercept() {
        // 端到端（F191 拦截进入）：进环境 → 快照 → 修引导 → 失败一次 →
        // 成功 → 导出（零写问题盘）→ manifest 发布门绿。
        let mut env = RecoveryEnv::new();
        let mut snaps = SnapshotLedger::new();
        let reason = EntryPath::BootIntercept.banner_reason();
        assert!(reason.contains("保护"));
        snaps.record_before(RecoveryCard::BootRepair);
        assert!(env.boot_repair(false, true, true).is_err(), "first attempt honest failure");
        assert!(env.boot_repair(true, true, true).is_ok(), "second attempt succeeds");
        assert!(env.export_logs(true, true).is_err(), "problem disk refused");
        assert!(env.export_logs(false, true).is_ok());
        // 拒绝尝试也计数——发布门要求 0，本旅程的包必须带伤说明
        // （这正是「诚实不藏」的设计：拒绝也是事实，不可静默发布）。
        assert_eq!(env.problem_disk_writes, 1);
        assert!(!manifest_publishable(env.problem_disk_writes));
    }

    #[test]
    fn f198_deep_network_gate_never_opens() {
        // 无论降级到哪一级，网门都开着（纪律不随降级松动）。
        let mut env = RecoveryEnv::new();
        let mut gate = NetworkGate::new();
        for _ in 0..2 {
            let _ = env.degrade();
            assert!(gate.request("update-check").is_err());
        }
        assert_eq!(gate.blocked, 2);
    }

    #[test]
    fn f198_deep_snapshot_ledger_grows_with_runs() {
        // 十轮执行：账随执行增长、守恒式始终绿（策略执行零遗漏）。
        let mut led = SnapshotLedger::new();
        for i in 0..10 {
            let card = match i % 3 {
                0 => RecoveryCard::BootRepair,
                1 => RecoveryCard::RestoreRollback,
                _ => RecoveryCard::LogExport,
            };
            led.record_before(card);
            assert!(led.consistent());
        }
        assert_eq!(led.entries.len(), 10);
    }

    #[test]
    fn f198_deep_run_checks_pass() {
        assert!(run_recenv_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——修复报告渲染 / 二级页返回栈 /
// 进入路径审计流。判据源：主册【交互设计】「每卡二级页极简（修复=进度+
// 结果……）；**全程可返回**」+【设计细节】「修复引导=闸门三条件重检」的
// 报告面 + 十三章体验日志（进入路径也是体验事件）。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：RepairReport —— 闸门三条件重检报告（修复引导卡的二级页结果面：
// 三条件逐行 + 成败人话——用户看得见修的是什么）
// ---------------------------------------------------------------------------

/// 报告行。
pub struct RepairReportRow {
    pub name: &'static str,
    pub ok: bool,
}

/// 修复报告。
pub struct RepairReport {
    pub rows: [RepairReportRow; 3],
    /// 总结果（三条件与 = 修复成败）。
    pub ok: bool,
    /// 人话结论（成功/失败两态——零静默）。
    pub verdict: &'static str,
}

/// 组装（boot_repair 的三条件 → 报告；与 RecoveryEnv::boot_repair 同语义）。
pub fn repair_report(gate_ok: bool, pubkey_ok: bool, wx_ok: bool) -> RepairReport {
    let rows = [
        RepairReportRow { name: "门表完整", ok: gate_ok },
        RepairReportRow { name: "公钥在位", ok: pubkey_ok },
        RepairReportRow { name: "W^X 生效", ok: wx_ok },
    ];
    let ok = gate_ok && pubkey_ok && wx_ok;
    RepairReport {
        rows,
        ok,
        verdict: if ok {
            "闸门三条件已重检通过，校验基准已重建"
        } else {
            "仍有失败项：请检查引导文件或从备份镜像恢复"
        },
    }
}

// ---------------------------------------------------------------------------
// v3-二：ReturnStack —— 二级页返回栈（「全程可返回」的状态机：进一层
// 压栈、返回弹栈、栈底=三卡主页——任何深处都有一条回家的路）
// ---------------------------------------------------------------------------

/// 页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecPage {
    /// 主页（三卡）。
    Home,
    /// 修复引导二级页。
    BootRepair,
    /// 回滚二级页（还原点列表）。
    RestoreList,
    /// 导出二级页（目标选择）。
    ExportPick,
}

/// 返回栈。
pub struct ReturnStack {
    stack: Vec<RecPage>,
    /// 返回次数（体验对账——「全程可返回」的使用证据）。
    pub returns: u64,
}

impl ReturnStack {
    pub fn new() -> ReturnStack {
        ReturnStack { stack: vec![RecPage::Home], returns: 0 }
    }

    /// 进一层（Home 永在栈底——压不住根）。
    pub fn push(&mut self, page: RecPage) {
        if self.stack.len() < 8 {
            self.stack.push(page);
        }
    }

    /// 返回（弹栈；主页不可弹——栈底恒在）。
    pub fn back(&mut self) -> RecPage {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.returns += 1;
        }
        self.current()
    }

    pub fn current(&self) -> RecPage {
        *self.stack.last().unwrap_or(&RecPage::Home)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}

impl Default for ReturnStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3-三：EntryAudit —— 进入路径审计流（三处入口各记一条——体验日志的
// 入口面：哪条路把用户带进来的，可溯）
// ---------------------------------------------------------------------------

/// 入口审计条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryAuditEvent {
    pub at_s: u64,
    pub path: EntryPath,
}

/// 入口账（定容环语义）。
pub struct EntryAuditLog {
    events: RingLog<EntryAuditEvent, 16>,
}

impl EntryAuditLog {
    pub fn new() -> EntryAuditLog {
        EntryAuditLog { events: RingLog::new() }
    }

    pub fn record(&mut self, at_s: u64, path: EntryPath) {
        self.events.push(EntryAuditEvent { at_s, path });
    }

    pub fn recent(&self) -> Vec<EntryAuditEvent> {
        self.events.newest_first()
    }

    /// 按路径计数（诊断聚合——哪条入口最常用）。
    pub fn count_path(&self, path: EntryPath) -> usize {
        self.events.newest_first().iter().filter(|e| e.path == path).count()
    }
}

impl Default for EntryAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F198 v3 自检（聚合进 secstar2 域）。
pub fn run_recenv_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v3");

    // v3-一：修复报告——三条件逐行、两态人话。
    let ok_rep = repair_report(true, true, true);
    set.add("rep ok", ok_rep.ok && ok_rep.rows.iter().all(|r| r.ok) && ok_rep.verdict.contains("通过"), "");
    let bad_rep = repair_report(true, false, true);
    set.add("rep bad located", !bad_rep.ok && !bad_rep.rows[1].ok && bad_rep.rows[0].ok, "公钥行红、其余不冤枉");
    set.add("rep bad verdict", bad_rep.verdict.contains("失败项"), "");

    // v3-二：返回栈——压/弹/根守恒、深处回家、深度上限。
    let mut rs = ReturnStack::new();
    set.add("stack home", rs.current() == RecPage::Home && rs.depth() == 1, "");
    rs.push(RecPage::BootRepair);
    rs.push(RecPage::ExportPick);
    set.add("stack depth", rs.depth() == 3 && rs.current() == RecPage::ExportPick, "");
    set.add("stack back", rs.back() == RecPage::BootRepair && rs.returns == 1, "");
    rs.back();
    rs.back();
    set.add("stack root held", rs.back() == RecPage::Home && rs.depth() == 1, "主页不可弹");
    for _ in 0..20 {
        rs.push(RecPage::RestoreList);
    }
    set.add("stack capped", rs.depth() <= 8, "栈深上限防失控");

    // v3-三：入口审计——按路径计数、新→旧。
    let mut log = EntryAuditLog::new();
    log.record(1, EntryPath::MenuHidden);
    log.record(2, EntryPath::BootIntercept);
    log.record(3, EntryPath::BootIntercept);
    set.add("entry count", log.count_path(EntryPath::BootIntercept) == 2, "");
    set.add("entry newest", log.recent()[0].at_s == 3, "");
    set.add("entry total", log.recent().len() == 3, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f198_v3_return_stack_survives_chaos() {
        // 乱点压测：50 次混合压栈/返回——栈不崩、根恒在、返回计数如实。
        let mut rs = ReturnStack::new();
        for i in 0..50 {
            if i % 3 == 0 {
                rs.back();
            } else {
                rs.push(match i % 3 {
                    1 => RecPage::RestoreList,
                    _ => RecPage::ExportPick,
                });
            }
        }
        while rs.depth() > 1 {
            rs.back();
        }
        assert_eq!(rs.current(), RecPage::Home);
        assert!(rs.returns > 0);
    }

    #[test]
    fn f198_v3_repair_report_matches_env_semantics() {
        // 报告与 RecoveryEnv::boot_repair 语义对拍：三真=成功、任一假=失败。
        let mut env = RecoveryEnv::new();
        let combos = [(true, true, true), (false, true, true), (true, false, false)];
        for (g, p, w) in combos {
            let rep = repair_report(g, p, w);
            assert_eq!(rep.ok, env.boot_repair(g, p, w).is_ok(), "combo {:?}", (g, p, w));
        }
    }

    #[test]
    fn f198_v3_run_checks_pass() {
        assert!(run_recenv_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——fixboot 最简命令 / 修复进度阶段模型 /
// 导出目标三重验证 / 脱网自证。判据源：主册【状态与异常】「两级降级（三卡
// →纯文字菜单→最简修复单命令）」+【交互设计】「导出=插另一 U 盘选择目标」
// +【数据与存储】「全流程脱网可用（设计纪律写死）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：FixbootCommand —— 最简修复单命令（降级到最底层的最后出口：无图形
// 无菜单，一行 `fixboot`——语法校验+干跑清单+执行报告，降级到底不伪装成功）
// ---------------------------------------------------------------------------

/// 命令名（唯一——两级降级到底后整个界面只剩这一个动词）。
pub const FIXBOOT_CMD: &str = "fixboot";

/// 命令解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixbootParse {
    /// 纯 `fixboot`（无参——唯一合法形态）。
    Bare,
    /// 带了参数（帮助语：本命令不接受参数）。
    HasArgs,
    /// 未知命令（给出最近建议——引导面连错误都要帮忙）。
    Unknown,
}

/// 解析（整行输入 → 判定；大小写不敏感、首尾空白容忍——应激场景手会抖）。
pub fn fixboot_parse(line: &str) -> FixbootParse {
    let t = line.trim();
    if t.is_empty() {
        return FixbootParse::Unknown;
    }
    let mut it = t.split_whitespace();
    let cmd = it.next().unwrap_or("");
    if cmd.eq_ignore_ascii_case(FIXBOOT_CMD) {
        if it.next().is_none() {
            FixbootParse::Bare
        } else {
            FixbootParse::HasArgs
        }
    } else {
        FixbootParse::Unknown
    }
}

/// 干跑清单（执行前列出将改动的目标——红线纪律③：破坏潜能操作先干跑）。
pub fn fixboot_dry_run() -> [&'static str; 3] {
    [
        "目标 1：引导配置门表哈希重算并写回（原子写+旧值备份）",
        "目标 2：签名链公钥在位性重检（只读校验，无写入）",
        "目标 3：W^X 策略区重置为出厂白名单（原子写+旧值备份）",
    ]
}

/// 执行结果（三条件逐项——成败两路都诚实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixbootReport {
    pub gate_ok: bool,
    pub pubkey_ok: bool,
    pub wx_ok: bool,
    /// 总耗时（ms——最简模式也要有性能账）。
    pub cost_ms: u64,
}

impl FixbootReport {
    pub fn all_ok(&self) -> bool {
        self.gate_ok && self.pubkey_ok && self.wx_ok
    }

    /// 结果行（诚实：失败项明说失败+指向下一级出口——没有下一级时明说）。
    pub fn lines(&self) -> [&'static str; 3] {
        [
            if self.gate_ok { "门表：已重建并校验通过" } else { "门表：重建失败（存储介质可能损坏）" },
            if self.pubkey_ok { "公钥：在位" } else { "公钥：缺失（镜像可能不完整）" },
            if self.wx_ok { "W^X：策略已生效" } else { "W^X：策略写入失败" },
        ]
    }
}

// ---------------------------------------------------------------------------
// v4-二：ProgressStages —— 修复进度页阶段模型（二级页「进度+结果」：
// 校验→修复→复验三段推进，失败定位到段——不白屏硬等）
// ---------------------------------------------------------------------------

/// 修复阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixStage {
    /// 三条件校验中。
    Verify,
    /// 修复执行中。
    Repair,
    /// 修复后复验。
    Reverify,
    /// 完成（成败在 report）。
    Done,
}

/// 阶段推进器（每段带 permille 进度；失败即冻结在当前段——不跳段不假绿）。
pub struct ProgressStages {
    pub stage: FixStage,
    /// 当前段进度（0-1000）。
    pub permille: u64,
    /// 失败段（None=未失败）。
    pub failed_at: Option<FixStage>,
}

impl ProgressStages {
    pub fn new() -> ProgressStages {
        ProgressStages { stage: FixStage::Verify, permille: 0, failed_at: None }
    }

    /// 推进当前段（delta permille 钳制；返回是否仍在进行）。
    pub fn advance(&mut self, delta: u64) -> bool {
        if self.failed_at.is_some() || self.stage == FixStage::Done {
            return false;
        }
        self.permille = (self.permille + delta).min(1000);
        if self.permille >= 1000 {
            self.stage = match self.stage {
                FixStage::Verify => FixStage::Repair,
                FixStage::Repair => FixStage::Reverify,
                FixStage::Reverify => FixStage::Done,
                FixStage::Done => FixStage::Done,
            };
            self.permille = 0;
        }
        true
    }

    /// 段失败（冻结——进度停在出事的地方，重试从头开始该段）。
    pub fn fail(&mut self) -> bool {
        if self.stage == FixStage::Done || self.failed_at.is_some() {
            return false;
        }
        self.failed_at = Some(self.stage);
        true
    }

    /// 重试（只允许从失败段重开——不跳段）。
    pub fn retry(&mut self) -> Result<(), &'static str> {
        match self.failed_at {
            Some(s) => {
                self.failed_at = None;
                self.stage = s;
                self.permille = 0;
                Ok(())
            }
            None => Err("无失败段可重试"),
        }
    }
}

impl Default for ProgressStages {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4-三：TargetUsbGuard —— 导出目标三重验证（红线铁律①：标签+容量+非问
// 题盘三重验证，错一个都不许动手——导出盘写错等于二次灾难）
// ---------------------------------------------------------------------------

/// 目标盘验证结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetVerdict {
    /// 身份标签匹配（用户在界面上确认的盘名）。
    pub label_ok: bool,
    /// 容量充足（导出包 < 目标盘余量）。
    pub capacity_ok: bool,
    /// 非问题盘（目标不是当前引导盘——零写问题盘红线）。
    pub not_problem_disk: bool,
}

impl TargetVerdict {
    pub fn all_ok(&self) -> bool {
        self.label_ok && self.capacity_ok && self.not_problem_disk
    }

    /// 缺陷行（人话——三重验证错哪重说哪重）。
    pub fn defects(&self) -> alloc::vec::Vec<&'static str> {
        let mut out = alloc::vec::Vec::new();
        if !self.label_ok {
            out.push("目标盘标签与确认的不符：请拔出后重新插入并核对盘名");
        }
        if !self.capacity_ok {
            out.push("目标盘剩余空间不足：导出包需要更多空间");
        }
        if !self.not_problem_disk {
            out.push("目标盘是当前问题盘：导出绝不写入本盘（自我隔离纪律）");
        }
        out
    }
}

/// 验证执行（注入三重输入；容量按字节对拍）。
pub fn verify_export_target(label_given: &str, label_confirmed: &str, free_bytes: u64, need_bytes: u64, is_problem_disk: bool) -> TargetVerdict {
    TargetVerdict {
        label_ok: label_given == label_confirmed,
        capacity_ok: free_bytes >= need_bytes,
        not_problem_disk: !is_problem_disk,
    }
}

// ---------------------------------------------------------------------------
// v4-四：offline_assert —— 脱网自证（恢复环境全功能逐项声明网络需求=0：
// 任何功能在恢复环境请求网络即红——设计纪律「永不需要网络」的机器执法）
// ---------------------------------------------------------------------------

/// 功能网络需求声明（全部恒 0——恢复环境功能面）。
pub const RECOVERY_NET_NEED: [(&str, u32); 6] = [
    ("修复引导", 0),
    ("回滚配置", 0),
    ("导出日志", 0),
    ("诊断快照", 0),
    ("屏显二维码", 0),
    ("fixboot", 0),
];

/// 自证扫描：全部功能网络需求为 0 才算过（新增功能忘登记 = 扫描红）。
pub fn offline_assert() -> bool {
    RECOVERY_NET_NEED.len() >= 6 && RECOVERY_NET_NEED.iter().all(|(_, need)| *need == 0)
}

/// 网络请求拦截器（恢复环境运行期：任何请求直接拒绝并留痕——NetworkGate
/// 的最简模式降级版，两级降级到最底层后也在）。
pub struct OfflineRequestGuard {
    pub rejected: u64,
}

impl OfflineRequestGuard {
    pub fn new() -> OfflineRequestGuard {
        OfflineRequestGuard { rejected: 0 }
    }

    pub fn request(&mut self, what: &str) -> Result<(), &'static str> {
        self.rejected += 1;
        let _ = what;
        Err("恢复环境永不联网（设计纪律）：此请求已拒绝并记录")
    }
}

impl Default for OfflineRequestGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F198 v4 自检（聚合进 secstar2 域）。
pub fn run_recenv_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v4");

    // v4-一：fixboot——解析四态、干跑清单、报告两路。
    set.add("fix bare", fixboot_parse("fixboot") == FixbootParse::Bare, "");
    set.add("fix case+space", fixboot_parse("  FIXBOOT  ") == FixbootParse::Bare, "应激场景大小写/空白容忍");
    set.add("fix args", fixboot_parse("fixboot --all") == FixbootParse::HasArgs, "");
    set.add("fix unknown", fixboot_parse("reboot") == FixbootParse::Unknown, "");
    set.add("fix empty", fixboot_parse("   ") == FixbootParse::Unknown, "");
    let dry = fixboot_dry_run();
    set.add("fix dry 3 targets", dry.len() == 3 && dry[0].contains("原子写"), "干跑列改动+备份承诺");
    let ok_rep = FixbootReport { gate_ok: true, pubkey_ok: true, wx_ok: true, cost_ms: 900 };
    set.add("fix report ok", ok_rep.all_ok() && ok_rep.lines()[0].contains("通过"), "");
    let bad_rep = FixbootReport { gate_ok: false, pubkey_ok: true, wx_ok: true, cost_ms: 1200 };
    set.add("fix report honest", !bad_rep.all_ok() && bad_rep.lines()[0].contains("失败") && bad_rep.lines()[0].contains("介质"), "失败明说+指向原因");

    // v4-二：进度阶段——三段推进、失败冻结、重试不跳段。
    let mut ps = ProgressStages::new();
    set.add("stage start verify", ps.stage == FixStage::Verify, "");
    for _ in 0..10 {
        ps.advance(100);
    }
    set.add("stage to repair", ps.stage == FixStage::Repair && ps.permille == 0, "校验满推进重置");
    ps.fail();
    set.add("stage frozen", !ps.advance(500) && ps.failed_at == Some(FixStage::Repair), "失败冻结不跳段");
    set.add("stage retry", ps.retry().is_ok() && ps.stage == FixStage::Repair && ps.permille == 0, "失败段重开");
    for _ in 0..30 {
        ps.advance(100);
    }
    set.add("stage done", ps.stage == FixStage::Done, "三段全过=完成");
    set.add("stage retry after done", ps.retry().is_err(), "无失败段重试拒");

    // v4-三：目标验证——三重全过、逐重缺陷、问题盘红线。
    let v = verify_export_target("VARIX-EXPORT", "VARIX-EXPORT", 1024, 100, false);
    set.add("target all ok", v.all_ok(), "");
    let v2 = verify_export_target("OTHER", "VARIX-EXPORT", 50, 100, true);
    set.add("target three defects", v2.defects().len() == 3, "三重全缺逐条说");
    set.add("target label defect", v2.defects()[0].contains("标签"), "");
    set.add("target problem disk red", v2.defects().iter().any(|d| d.contains("问题盘")), "红线文案在列");
    let v3 = verify_export_target("VARIX-EXPORT", "VARIX-EXPORT", 100, 100, false);
    set.add("target capacity boundary", v3.capacity_ok, "恰好够=过（≥语义）");

    // v4-四：脱网自证——声明全零、拦截器拒绝留痕。
    set.add("offline assert", offline_assert(), "");
    set.add("offline decl count", RECOVERY_NET_NEED.len() == 6, "");
    let mut g = OfflineRequestGuard::new();
    set.add("offline reject", g.request("weather-sync").is_err(), "");
    set.add("offline reject counted", g.rejected == 1, "");
    set.add("offline reject msg", g.request("ntp").unwrap_err().contains("永不联网"), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f198_v4_fixboot_full_story() {
        // 最简降级的全故事：两级降级后只剩 fixboot → 干跑 → 执行 → 复验
        // ——降级到底也有完整出口（不伪装成功判据）。
        assert_eq!(fixboot_parse("fixboot"), FixbootParse::Bare);
        let dry = fixboot_dry_run();
        assert!(dry.iter().all(|l| l.contains("目标")), "干跑逐目标");
        let rep = FixbootReport { gate_ok: false, pubkey_ok: false, wx_ok: false, cost_ms: 2000 };
        // 三条件全败：三行全部诚实说失败（最坏情况的诚实）。
        assert!(rep.lines().iter().all(|l| l.contains("失败") || l.contains("缺失")));
    }

    #[test]
    fn f198_v4_stages_never_skip_on_fail() {
        // 乱点压测：失败后疯狂 advance 1000 次——冻结态不推进不跳段。
        let mut ps = ProgressStages::new();
        ps.advance(1000); // 进入 Repair。
        ps.fail();
        for _ in 0..1000 {
            ps.advance(1000);
        }
        assert_eq!(ps.stage, FixStage::Repair);
        assert_eq!(ps.failed_at, Some(FixStage::Repair));
        // 重试后一次推到底。
        ps.retry().unwrap();
        for _ in 0..3 {
            while ps.advance(200) && ps.stage != FixStage::Done {}
            if ps.stage == FixStage::Done {
                break;
            }
        }
        assert_eq!(ps.stage, FixStage::Done);
    }

    #[test]
    fn f198_v4_target_verdict_matrix() {
        // 8 种三重组合穷举：defects 数与通过数互补。
        for mask in 0..8u8 {
            let v = TargetVerdict {
                label_ok: mask & 1 != 0,
                capacity_ok: mask & 2 != 0,
                not_problem_disk: mask & 4 != 0,
            };
            assert_eq!(v.defects().len(), 3 - mask.count_ones() as usize, "mask={}", mask);
            assert_eq!(v.all_ok(), mask == 7);
        }
    }

    #[test]
    fn f198_v4_offline_guard_never_allows() {
        // 拦截器 100 种请求全拒（恢复环境联网面=空集的运行期执法）。
        let mut g = OfflineRequestGuard::new();
        for i in 0..100 {
            assert!(g.request(&alloc::format!("req-{}", i)).is_err());
        }
        assert_eq!(g.rejected, 100);
    }

    #[test]
    fn f198_v4_run_checks_pass() {
        assert!(run_recenv_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——fixboot 脚本模式 /
// 还原点预览行 / 恢复环境帮助页 / 目标盘健康面。判据源：主册【交互设计】
// 三卡二级页极简 +【设计细节】「进入路径三处」「修复引导=闸门三条件重检+
// 基准哈希重建」「每卡执行前自动快照（除导出）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v5-一：fixboot_script —— fixboot 批量脚本模式（多行输入逐行解析 + 总结：
// 降级到最底层也支持把多条命令一次贴进来——应激场景少敲几行）
// ---------------------------------------------------------------------------

/// 脚本执行总结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptSummary {
    /// 合法 fixboot 行数。
    pub executed: u64,
    /// 非法行数（拼错/带参）。
    pub invalid: u64,
    /// 总行数。
    pub total: u64,
}

/// 逐行解析（空行跳过不计数——粘贴常带尾空行）。
pub fn fixboot_script(input: &str) -> (ScriptSummary, alloc::vec::Vec<FixbootParse>) {
    let mut executed = 0u64;
    let mut invalid = 0u64;
    let mut kinds = alloc::vec::Vec::new();
    let mut total = 0u64;
    for line in input.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        total += 1;
        let k = fixboot_parse(t);
        match k {
            FixbootParse::Bare => executed += 1,
            _ => invalid += 1,
        }
        kinds.push(k);
    }
    (ScriptSummary { executed, invalid, total }, kinds)
}

// ---------------------------------------------------------------------------
// v5-二：rollback_preview_rows —— 还原点预览行（回滚卡二级页：每点一行
// （ID/时刻/配置指纹短码/可用性）——损坏点照列但标注不可用（不赌纪律））
// ---------------------------------------------------------------------------

/// 预览行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackPreviewRow {
    pub point_id: u32,
    /// 时刻人话（秒戳原样——格式化交渲染层）。
    pub at_s: u64,
    /// 可用性（损坏点=不可用+原因随行）。
    pub usable: bool,
    pub why: &'static str,
}

/// 从还原点列表渲染（沿用 RecoveryEnv 的损坏跳过语义——但预览里如实列出）。
pub fn rollback_preview_rows(points: &[RestorePoint]) -> alloc::vec::Vec<RollbackPreviewRow> {
    points
        .iter()
        .map(|p| RollbackPreviewRow {
            point_id: p.id,
            at_s: p.day,
            usable: p.valid,
            why: if p.valid { "可选择回滚" } else { "该点校验失败：已跳过（不赌损坏数据）" },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v5-三：RECOVERY_HELP —— 恢复环境帮助页（四节：这是什么/三张卡怎么用/
// 怎么进来/为什么不用网络——绝境里的可解释性）
// ---------------------------------------------------------------------------

pub const RECOVERY_HELP: [(&'static str, &'static str); 4] = [
    (
        "这是什么",
        "恢复环境是系统进不去桌面时的最后入口：独立小系统从 U 盘直接运行，不依赖本盘上的任何文件。",
    ),
    (
        "三张卡怎么用",
        "修复引导：一键重检启动链并重建基准；回滚配置：选一个还原点把配置退回去；导出日志：把诊断材料拷给另一块 U 盘求助。",
    ),
    (
        "怎么进来",
        "三个入口：引导选单底部提示的隐藏入口、安全模式内的恢复按钮、启动链拦截画面的主按钮（F191/F193 链）。",
    ),
    (
        "为什么不用网络",
        "恢复环境设计上永不联网：修复、回滚、导出全部本地完成——最脆弱的时刻不引入任何外部依赖。",
    ),
];

pub fn recovery_help_intact() -> bool {
    RECOVERY_HELP.len() == 4 && RECOVERY_HELP.iter().all(|(h, b)| !h.is_empty() && b.len() >= 20)
}

// ---------------------------------------------------------------------------
// v5-四：TargetHealth —— 目标盘健康面（导出目标的三项健康：容量档/可写
// 试探/速度档估算——「插另一 U 盘选择目标」的选择辅助）
// ---------------------------------------------------------------------------

/// 健康面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetHealth {
    /// 容量档（GB）。
    pub capacity_gib: u64,
    /// 可写（试探写-读-比-删通过）。
    pub writable: bool,
    /// 速度档（1=慢速 USB2 语义 / 2=高速 / 3=超速——导出耗时预估用）。
    pub speed_tier: u8,
}

impl TargetHealth {
    /// 导出耗时预估（秒：数据 MB × (4-tier) 系数；速度档 0 视为不可写拒绝预估）。
    pub fn eta_seconds(&self, data_mib: u64) -> Option<u64> {
        if !self.writable || self.speed_tier == 0 {
            return None;
        }
        let factor = 4 - self.speed_tier.min(3); // tier1→3s/MB, tier3→1s/MB。
        Some(data_mib * factor as u64)
    }

    /// 健康行（三要素：容量/可写/速度——缺哪个说哪个）。
    pub fn line(&self) -> String {
        alloc::format!(
            "目标盘：{} GiB / {} / {} 速",
            self.capacity_gib,
            if self.writable { "可写" } else { "不可写（可能写保护或损坏）" },
            self.speed_tier
        )
    }
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F198 v5 自检（聚合进 secstar2 域）。
pub fn run_recenv_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v5");

    // v5-一：脚本模式——混合行统计、空行跳过、全法全勤。
    let (sum, kinds) = fixboot_script("fixboot\n\n  FIXBOOT  \nreboot\nfixboot --all\n");
    set.add("script total", sum.total == 4, "空行跳过");
    set.add("script executed", sum.executed == 2, "两行合法（含大小写+空白容忍）");
    set.add("script invalid", sum.invalid == 2, "未知命令+带参各一");
    set.add("script kinds", kinds.len() == 4, "");
    let (sum2, _) = fixboot_script("fixboot\nfixboot\n");
    set.add("script all ok", sum2.executed == 2 && sum2.invalid == 0, "");

    // v5-二：还原点预览——损坏点如实列出+不可用标注。
    let points = vec![
        RestorePoint { id: 1, day: 100, valid: true },
        RestorePoint { id: 2, day: 200, valid: false },
        RestorePoint { id: 3, day: 300, valid: true },
    ];
    let rows = rollback_preview_rows(&points);
    set.add("preview count", rows.len() == 3, "损坏点也列出（诚实呈现）");
    set.add("preview bad flagged", !rows[1].usable && rows[1].why.contains("校验失败"), "");
    set.add("preview good", rows[0].usable && rows[2].usable, "");
    set.add("preview ids", rows[0].point_id == 1 && rows[2].point_id == 3, "");

    // v5-三：帮助页——四节齐。
    set.add("help intact", recovery_help_intact(), "");
    set.add("help no net", RECOVERY_HELP[3].1.contains("永不联网"), "脱网纪律入文");
    set.add("help entries 3", RECOVERY_HELP[2].1.contains("三个入口"), "进入路径三处入文");

    // v5-四：目标健康——ETA 三档、不可写拒绝预估、健康行。
    let t1 = TargetHealth { capacity_gib: 8, writable: true, speed_tier: 1 };
    let t3 = TargetHealth { capacity_gib: 64, writable: true, speed_tier: 3 };
    let dead = TargetHealth { capacity_gib: 16, writable: false, speed_tier: 0 };
    set.add("health eta slow", t1.eta_seconds(100) == Some(300), "慢速 3s/MB");
    set.add("health eta fast", t3.eta_seconds(100) == Some(100), "高速 1s/MB");
    set.add("health eta dead", dead.eta_seconds(100).is_none(), "不可写不给预估（不撒谎）");
    set.add("health line", dead.line().contains("不可写") && t3.line().contains("64 GiB"), "");
    set.add("health tier clamp", TargetHealth { capacity_gib: 1, writable: true, speed_tier: 9 }.eta_seconds(10) == Some(10), "速度档钳 3");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f198_v5_script_multiline_stress() {
        // 100 行混合脚本统计守恒：executed+invalid=total（空行不计）。
        let mut input = String::new();
        for i in 0..100 {
            input.push_str(if i % 3 == 0 { "fixboot\n" } else if i % 3 == 1 { "format c\n" } else { "\n" });
        }
        let (sum, _) = fixboot_script(&input);
        assert_eq!(sum.total, 67, "100 行里 33 行是空行（跳过不计数）");
        assert_eq!(sum.executed + sum.invalid, 67);
        assert_eq!(sum.executed, 34, "i%3==0 → 34 行合法");
        assert_eq!(sum.invalid, 33);
    }

    #[test]
    fn f198_v5_preview_all_corrupt_honest() {
        // 全损坏场景：三行全部如实标注不可用（绝境也不伪装）。
        let points = vec![
            RestorePoint { id: 1, day: 1, valid: false },
            RestorePoint { id: 2, day: 2, valid: false },
        ];
        let rows = rollback_preview_rows(&points);
        assert!(rows.iter().all(|r| !r.usable));
        assert!(rows.iter().all(|r| r.why.contains("不赌")));
    }

    #[test]
    fn f198_v5_help_discoverability_chain() {
        // 帮助页与入口审计互证：三个入口都在文中出现（可发现性闭环）。
        let entry_text = RECOVERY_HELP[2].1;
        assert!(entry_text.contains("引导选单"));
        assert!(entry_text.contains("安全模式"));
        assert!(entry_text.contains("拦截画面"));
    }

    #[test]
    fn f198_v5_run_checks_pass() {
        assert!(run_recenv_deep4_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——fixboot 帮助页 / 快照完整性校验 /
// 恢复环境使用统计 / 还原点内容预览。判据源：主册【设计细节】「每卡执行
// 前自动快照现场」「修复引导=闸门三条件重检+基准哈希重建」。
// ---------------------------------------------------------------------------

/// fixboot 帮助页（三节：什么时候用/它做什么/失败了怎么办）。
pub const FIXBOOT_HELP: [(&'static str, &'static str); 3] = [
    ("什么时候用", "两级降级到底后：图形卡和文字菜单都不可用，屏幕只剩一行命令提示。输入 fixboot 回车即执行最简修复。"),
    ("它做什么", "三步：重算门表哈希并原子写回（旧值备份）、重检签名链公钥在位性、重置 W^X 策略为出厂白名单。每步成败都如实报告。"),
    ("失败了怎么办", "任一步失败会明说失败项与原因（如存储介质损坏）。此时介质侧问题已超出软件修复能力——请联系社区并附诊断二维码摘要。"),
];

pub fn fixboot_help_intact() -> bool {
    FIXBOOT_HELP.len() == 3 && FIXBOOT_HELP[1].1.contains("原子写") && FIXBOOT_HELP[2].1.contains("二维码")
}

/// 快照完整性校验器（快照头魔数+长度守恒——「还原点损坏跳过」的前置检测）。
pub const SNAPSHOT_MAGIC: [u8; 2] = [0x56, 0x53]; // "VS"

pub fn snapshot_integrity(data: &[u8]) -> bool {
    data.len() >= 8 && data[0..2] == SNAPSHOT_MAGIC && data.len() % 8 == 0
}

/// 恢复环境使用统计（进入次数/各卡执行/成功率——诊断回传面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RecoveryMetrics {
    pub entries: u64,
    pub boot_repairs: u64,
    pub rollbacks: u64,
    pub exports: u64,
    pub success_total: u64,
}

impl RecoveryMetrics {
    /// 从历史行聚合（history: (卡名, 是否成功)）。
    pub fn from_history(entries: u64, history: &[(&'static str, bool)]) -> RecoveryMetrics {
        let mut m = RecoveryMetrics { entries, ..Default::default() };
        for (card, ok) in history {
            match *card {
                "修复引导" => m.boot_repairs += 1,
                "回滚配置" => m.rollbacks += 1,
                "导出日志" => m.exports += 1,
                _ => {}
            }
            if *ok {
                m.success_total += 1;
            }
        }
        m
    }

    /// 成功率 permille（零操作=0 且标注）。
    pub fn success_permille(&self) -> u64 {
        let total = self.boot_repairs + self.rollbacks + self.exports;
        if total == 0 {
            0
        } else {
            self.success_total * 1000 / total
        }
    }
}

/// 还原点内容预览（将恢复哪些配置类目——回滚前「会发生什么」的干跑行）。
pub const RESTORE_CATEGORIES: [&str; 5] = [
    "主题令牌表（F151）",
    "开始菜单布局（F158）",
    "右键菜单自定义（F167）",
    "快捷键映射（F169）",
    "系统偏好设置",
];

pub fn restore_preview_lines(point_valid: bool) -> Vec<String> {
    if !point_valid {
        return vec![String::from("该还原点校验失败：已跳过（不赌损坏数据）——请选择其他还原点")];
    }
    RESTORE_CATEGORIES
        .iter()
        .map(|c| alloc::format!("将恢复：{}", c))
        .collect()
}

/// F198 v6 自检（deep5 表）。
pub fn run_recenv_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v6");

    // v6-一：fixboot 帮助——三节齐。
    set.add("fixboot help intact", fixboot_help_intact(), "");

    // v6-二：快照完整性——魔数、对齐、短流。
    let good = [0x56u8, 0x53, 0, 0, 0, 0, 0, 0];
    set.add("snap good", snapshot_integrity(&good), "");
    set.add("snap bad magic", !snapshot_integrity(&[0xAA, 0xBB, 0, 0, 0, 0, 0, 0]), "");
    set.add("snap short", !snapshot_integrity(&[0x56, 0x53, 0]), "短流不硬检");
    set.add("snap misaligned", !snapshot_integrity(&[0x56, 0x53, 1, 2, 3, 4, 5]), "非 8 对齐拒");

    // v6-三：使用统计——分卡计数、成功率、零操作诚实。
    let m = RecoveryMetrics::from_history(3, &[("修复引导", true), ("修复引导", false), ("导出日志", true)]);
    set.add("metrics cards", m.boot_repairs == 2 && m.exports == 1 && m.rollbacks == 0, "");
    set.add("metrics rate", m.success_permille() == 666, "2/3 = 666‰");
    set.add("metrics zero", RecoveryMetrics::default().success_permille() == 0, "");

    // v6-四：还原点预览——正常五行、损坏诚实一行。
    let ok_lines = restore_preview_lines(true);
    set.add("preview ok 5", ok_lines.len() == RESTORE_CATEGORIES.len() && ok_lines[0].contains("主题令牌"), "");
    let bad_lines = restore_preview_lines(false);
    set.add("preview bad honest", bad_lines.len() == 1 && bad_lines[0].contains("不赌"), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f198_v6_metrics_all_cards() {
        // 三卡全覆盖 + 全成功 = 1000‰。
        let m = RecoveryMetrics::from_history(
            5,
            &[("修复引导", true), ("回滚配置", true), ("导出日志", true), ("修复引导", true), ("回滚配置", true)],
        );
        assert_eq!(m.success_permille(), 1000);
        assert_eq!(m.boot_repairs, 2);
        assert_eq!(m.rollbacks, 2);
        assert_eq!(m.exports, 1);
    }

    #[test]
    fn f198_v6_help_disaster_path() {
        // 帮助页覆盖最坏路径：介质损坏→社区求助（绝境也有下一步）。
        assert!(FIXBOOT_HELP[2].1.contains("失败"));
        assert!(FIXBOOT_HELP[2].1.contains("社区"));
    }

    #[test]
    fn f198_v6_run_checks_pass() {
        assert!(run_recenv_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——三卡使用引导 / 快照元数据 /
// 修复动作统计 / 还原点排序。判据源：主册【交互设计】「回滚=还原点列表
// F121 复用」+【数据与存储】恢复环境自含运行时。
// ---------------------------------------------------------------------------

/// 三卡使用引导（每卡「什么时候用它」判定——引导面：症状→推荐卡）。
pub fn recommend_card(symptom: u8) -> &'static str {
    match symptom {
        0 => "修复引导：启动链拦截画面出现时（F191 联动）",
        1 => "回滚配置：最近改过主题/设置后系统异常时（F121 联动）",
        2 => "导出日志：需要社区协助排查时（材料出盘，绝不写问题盘）",
        _ => "未知症状：请按 修复引导 → 回滚配置 → 导出日志 顺序逐一尝试",
    }
}

/// 快照元数据（快照头的结构化视图——大小/时刻/完整性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub size_bytes: usize,
    pub magic_ok: bool,
    pub aligned: bool,
}

/// 从字节流提取（snapshot_integrity 的结构化版）。
pub fn snapshot_meta(data: &[u8]) -> SnapshotMeta {
    SnapshotMeta {
        size_bytes: data.len(),
        magic_ok: data.len() >= 2 && data[0..2] == SNAPSHOT_MAGIC,
        aligned: data.len() % 8 == 0,
    }
}

/// 修复动作统计页（各卡执行/成败——诊断回传的页面模型）。
pub fn recovery_stats_lines(m: &RecoveryMetrics) -> Vec<String> {
    vec![
        alloc::format!("进入恢复环境 {} 次", m.entries),
        alloc::format!("修复引导 {} 次 / 回滚配置 {} 次 / 导出日志 {} 次", m.boot_repairs, m.rollbacks, m.exports),
        alloc::format!("成功率 {}‰（估算口径：成功操作 / 总操作）", m.success_permille()),
    ]
}

/// 还原点排序（新→旧——回滚列表默认序：最近的在前）。
pub fn restore_points_sorted(points: &[RestorePoint]) -> Vec<RestorePoint> {
    let mut out: Vec<RestorePoint> = points.to_vec();
    out.sort_by(|a, b| b.day.cmp(&a.day));
    out
}

/// F198 v7 自检（deep6 表）。
pub fn run_recenv_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v6b");

    // v7-一：症状引导——三卡+未知兜底。
    set.add("rec boot", recommend_card(0).contains("修复引导"), "");
    set.add("rec rollback", recommend_card(1).contains("回滚配置"), "");
    set.add("rec export", recommend_card(2).contains("导出日志"), "");
    set.add("rec unknown", recommend_card(9).contains("逐一尝试"), "兜底不迷路");

    // v7-二：快照元数据——结构化视图。
    let good = [0x56u8, 0x53, 1, 2, 3, 4, 5, 6];
    let meta = snapshot_meta(&good);
    set.add("snapmeta good", meta.magic_ok && meta.aligned && meta.size_bytes == 8, "");
    let bad = snapshot_meta(&[0x56]);
    set.add("snapmeta bad", !bad.magic_ok && !bad.aligned, "短流两态皆红");

    // v7-三：统计页——三行人话。
    let m = RecoveryMetrics::from_history(4, &[("修复引导", true), ("修复引导", true), ("回滚配置", false), ("导出日志", true)]);
    let lines = recovery_stats_lines(&m);
    set.add("stats 3 lines", lines.len() == 3, "");
    set.add("stats rate", lines[2].contains("750‰"), "3/4 = 750‰");

    // v7-四：还原点排序——乱序输入新→旧输出。
    let pts = vec![
        RestorePoint { id: 1, day: 100, valid: true },
        RestorePoint { id: 2, day: 300, valid: true },
        RestorePoint { id: 3, day: 200, valid: true },
    ];
    let sorted = restore_points_sorted(&pts);
    set.add("sorted newest first", sorted[0].day == 300 && sorted[2].day == 100, "");
    set.add("sorted stable count", sorted.len() == 3, "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f198_v7_snapmeta_various() {
        // 边界矩阵：恰 8 字节 / 16 字节 / 7 字节（非对齐）。
        let d8 = [0x56u8, 0x53, 0, 0, 0, 0, 0, 0];
        assert!(snapshot_meta(&d8).aligned);
        let d16 = [0x56u8, 0x53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(snapshot_meta(&d16).aligned);
        assert!(!snapshot_meta(&[0x56u8, 0x53, 1, 2, 3, 4, 5]).aligned);
    }

    #[test]
    fn f198_v7_run_checks_pass() {
        assert!(run_recenv_deep6_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——恢复演练干跑 / 快照垃圾回收 / 备份
// 体积估算 / 演练报告卡 / 介质健康检查。
// 判据源：主册【硬件与数据安全红线】「有破坏潜能的操作必须先提供干跑」+
// 【设计细节】「快照保留最近 N 份 + 月锚点」。
// ---------------------------------------------------------------------------

/// 干跑结论（不动一个字节——只说「如果执行，会动什么」）。
pub struct DryRunReport<'a> {
    /// 允许执行（所有前置检查过）。
    pub allowed: bool,
    /// 将被覆盖的条目清单（相对路径）。
    pub would_overwrite: Vec<&'a str>,
    /// 将被新建的条目清单。
    pub would_create: Vec<&'a str>,
    /// 阻断原因（不允许时必非空——干跑不许只说不行）。
    pub blocker: &'static str,
}

/// 恢复干跑：拿快照条目与现存状态对比，产出将动清单。
/// `existing`：目标机上已存在的条目名；`snapshot`：快照内条目名。
pub fn restore_dry_run<'a>(existing: &[&'a str], snapshot: &[&'a str]) -> DryRunReport<'a> {
    let mut rep = DryRunReport {
        allowed: true,
        would_overwrite: Vec::new(),
        would_create: Vec::new(),
        blocker: "",
    };
    for s in snapshot {
        if existing.contains(s) {
            rep.would_overwrite.push(s);
        } else {
            rep.would_create.push(s);
        }
    }
    // 红线：现存条目里若有「用户文档区」标记，覆盖前必须显式放行（这里一律阻断）。
    if existing.iter().any(|e| e.starts_with("docs/")) && rep.would_overwrite.iter().any(|o| o.starts_with("docs/")) {
        rep.allowed = false;
        rep.blocker = "快照将覆盖用户文档区——需显式确认后放行";
    }
    rep
}

/// 快照 GC 结论。
pub struct SnapshotGc {
    /// 保留的快照 id（最近 N 份 + 全部锚点）。
    pub keep: Vec<u64>,
    /// 回收的快照 id。
    pub reclaim: Vec<u64>,
}

/// 快照垃圾回收（保留最近 keep_recent 份 + 标记 anchor 的全部——
/// 回收≠删除：先列清单，落盘清理由下一闸门执行）。
pub fn snapshot_gc(ids_newest_first: &[u64], anchors: &[u64], keep_recent: usize) -> SnapshotGc {
    let mut keep = Vec::new();
    let mut reclaim = Vec::new();
    for (i, id) in ids_newest_first.iter().enumerate() {
        if i < keep_recent || anchors.contains(id) {
            keep.push(*id);
        } else {
            reclaim.push(*id);
        }
    }
    SnapshotGc { keep, reclaim }
}

/// 体积估算行（类别 → 每条目典型体积 MiB）。
pub const SIZE_WEIGHTS: [(&str, u64); 5] = [
    ("system", 800),
    ("apps", 300),
    ("settings", 5),
    ("drivers", 120),
    ("index", 40),
];

/// 备份体积估算（按类别条数加权——估算值必须标注「估算」二字）。
pub fn backup_size_estimate(counts: &[(&str, u64)]) -> (u64, bool) {
    let mut total = 0u64;
    for (cat, n) in counts {
        if let Some((_, w)) = SIZE_WEIGHTS.iter().find(|(c, _)| c == cat) {
            total += w * n;
        }
    }
    (total, true) // bool = is_estimate（恒真——估算不许冒充实测）。
}

/// 演练报告卡（恢复演练 → 四格：结果/耗时/覆盖/下一步）。
pub struct DrillCard {
    pub verdict: &'static str,
    pub cost_s: u64,
    /// 覆盖率 permille（演练覆盖的恢复路径比例）。
    pub coverage_permille: u64,
    pub next_step: &'static str,
}

/// 演练判分（全绿→通过；有失败→给出下一步）。
pub fn drill_card(results: &[bool], cost_s: u64) -> DrillCard {
    let total = results.len();
    let passed = results.iter().filter(|r| **r).count();
    let coverage = if total == 0 { 0 } else { passed as u64 * 1000 / total as u64 };
    if coverage == 1000 {
        DrillCard { verdict: "通过", cost_s, coverage_permille: coverage, next_step: "登记进演练台账（季度一次）" }
    } else if coverage > 0 {
        DrillCard { verdict: "部分通过", cost_s, coverage_permille: coverage, next_step: "对失败路径开缺陷工单并重演" }
    } else {
        DrillCard { verdict: "不通过", cost_s, coverage_permille: coverage, next_step: "冻结发布闸门直至恢复路径修复" }
    }
}

/// 介质健康检查（坏块表 → 可用性判定）。
pub struct MediaHealth {
    pub bad_blocks: u64,
    /// 可用（坏块率 ≤ 2‰ 且预留池未耗尽）。
    pub usable: bool,
    /// 结论文案（三要素）。
    pub note: alloc::string::String,
}

/// 介质检查（总块数/坏块数/预留池余量 → 健康结论）。
pub fn media_health(total_blocks: u64, bad_blocks: u64, spare_left: u64) -> MediaHealth {
    let permille = if total_blocks == 0 { 0 } else { bad_blocks * 1000 / total_blocks };
    let usable = permille <= 2 && (bad_blocks == 0 || spare_left > 0);
    let note = if usable {
        alloc::string::String::from("介质健康，坏块已由预留池接管")
    } else if permille > 2 {
        alloc::string::String::from("坏块率超 2‰——建议尽快更换介质并重做恢复盘")
    } else {
        alloc::string::String::from("预留池耗尽——坏块无法再接管，建议更换介质")
    };
    MediaHealth { bad_blocks, usable, note }
}

/// F198 v8 自检（deep7 表）。
pub fn run_recenv_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v8");

    // 干跑：新建/覆盖分账、文档区红线阻断。
    let dr = restore_dry_run(&["settings/theme", "apps/list"], &["settings/theme", "apps/list", "drivers/base"]);
    set.add("dry split", dr.allowed && dr.would_overwrite.len() == 2 && dr.would_create.len() == 1, "");
    let dr2 = restore_dry_run(&["docs/notes.txt"], &["docs/notes.txt"]);
    set.add("dry docs red", !dr2.allowed && dr2.blocker.contains("文档区"), "覆盖文档区必须阻断");
    let dr3 = restore_dry_run(&[], &["settings/theme"]);
    set.add("dry pure create", dr3.allowed && dr3.would_overwrite.is_empty(), "全新恢复不触碰现存");

    // GC：最近 N + 锚点保、其余列清单。
    let gc = snapshot_gc(&[9, 8, 7, 6, 5], &[5, 6], 2);
    set.add("gc keep", gc.keep.contains(&9) && gc.keep.contains(&8) && gc.keep.contains(&5) && gc.keep.contains(&6), "最近 2 份 + 2 锚点");
    set.add("gc reclaim", gc.reclaim == vec![7], "7 既不新也不锚 → 回收清单");

    // 体积估算：加权求和、未知类别不计、恒标估算。
    let (mib, est) = backup_size_estimate(&[("system", 1), ("settings", 4)]);
    set.add("size est", mib == 820 && est, "800 + 5×4 = 820 MiB");
    let (mib2, _) = backup_size_estimate(&[("unknown", 100)]);
    set.add("size unknown", mib2 == 0, "未知类别不凭空计价");

    // 演练卡：三档判分。
    let c1 = drill_card(&[true, true, true, true], 30);
    set.add("drill pass", c1.verdict == "通过" && c1.coverage_permille == 1000, "");
    let c2 = drill_card(&[true, true, false, false], 45);
    set.add("drill partial", c2.verdict == "部分通过" && c2.coverage_permille == 500, "");
    set.add("drill partial next", c2.next_step.contains("工单"), "部分通过 → 开工单");
    let c3 = drill_card(&[false, false, false, false], 60);
    set.add("drill fail", c3.verdict == "不通过" && c3.coverage_permille == 0 && c3.next_step.contains("冻结"), "全挂 → 冻结闸门");

    // 介质健康：三态结论。
    let m1 = media_health(100_000, 100, 50);
    set.add("media ok", m1.usable && m1.note.contains("健康"), "1‰ 坏块在界内");
    let m2 = media_health(100_000, 500, 50);
    set.add("media over", !m2.usable && m2.note.contains("2‰"), "5‰ 超界");
    let m3 = media_health(100_000, 100, 0);
    set.add("media spare", !m3.usable && m3.note.contains("预留池"), "低坏块但池尽也不可用");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f198_v7_gc_anchor_priority() {
        // 锚点即使最老也保留；keep_recent=0 时只剩锚点。
        let gc = snapshot_gc(&[5, 4, 3, 2, 1], &[1], 0);
        assert_eq!(gc.keep, vec![1]);
        assert_eq!(gc.reclaim, vec![5, 4, 3, 2]);
    }

    #[test]
    fn f198_v7_dry_run_never_mutates() {
        // 干跑纯函数性：同输入两跑结果一致（干跑绝无副作用）。
        let a = restore_dry_run(&["settings/x"], &["settings/x", "apps/y"]);
        let b = restore_dry_run(&["settings/x"], &["settings/x", "apps/y"]);
        assert_eq!(a.would_overwrite, b.would_overwrite);
        assert_eq!(a.would_create, b.would_create);
    }

    #[test]
    fn f198_v7_drill_empty() {
        // 空演练不给满分（没测过就是没测过）。
        let c = drill_card(&[], 0);
        assert_eq!(c.coverage_permille, 0);
        assert_eq!(c.verdict, "不通过");
    }

    #[test]
    fn f198_v7_run_checks_pass() {
        assert!(run_recenv_deep7_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b5：恢复点排序 / 空间预检 / 中断续跑 / 引导预检（只读）。
// ---------------------------------------------------------------------------

/// 恢复点优先级排序（新优先、有效优先——同分按时间近先）。
pub fn restore_priority(points: &[(u64, bool, u64)]) -> Vec<u64> {
    // (id, valid, day) → 排序键：valid 降序、day 降序。
    let mut scored: Vec<(u64, u64, u64)> = points
        .iter()
        .map(|(id, valid, day)| (*id, if *valid { 1 } else { 0 }, *day))
        .collect();
    scored.sort_by(|a, b| (b.1, b.2).cmp(&(a.1, a.2)));
    scored.into_iter().map(|(id, _, _)| id).collect()
}

/// 空间预检（恢复所需 vs 可用 → 判定与缺口）。
pub struct SpaceCheck {
    pub enough: bool,
    pub deficit_mib: u64,
}

/// 预检（need_mib：恢复所需；free_mib：目标盘可用）。
pub fn space_precheck(need_mib: u64, free_mib: u64) -> SpaceCheck {
    SpaceCheck {
        enough: free_mib >= need_mib,
        deficit_mib: need_mib.saturating_sub(free_mib),
    }
}

/// 恢复续跑状态（阶段机：扫描 → 校验 → 落盘 → 完成；中断从当前阶段续）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreStage {
    Scan,
    Verify,
    Apply,
    Done,
}

/// 阶段推进（含中断恢复：任意阶段可暂停并原地续）。
pub struct RestoreResume {
    pub stage: RestoreStage,
    pub interrupted: bool,
}

impl RestoreResume {
    pub fn new() -> RestoreResume {
        RestoreResume { stage: RestoreStage::Scan, interrupted: false }
    }

    /// 推进一阶段。
    pub fn advance(&mut self) {
        if self.interrupted {
            return; // 中断态必须先 resume。
        }
        self.stage = match self.stage {
            RestoreStage::Scan => RestoreStage::Verify,
            RestoreStage::Verify => RestoreStage::Apply,
            RestoreStage::Apply => RestoreStage::Done,
            RestoreStage::Done => RestoreStage::Done,
        };
    }

    pub fn interrupt(&mut self) {
        if self.stage != RestoreStage::Done {
            self.interrupted = true;
        }
    }

    pub fn resume_after_interrupt(&mut self) {
        self.interrupted = false; // 阶段不回退——原地续。
    }
}

impl Default for RestoreResume {
    fn default() -> Self {
        Self::new()
    }
}

/// 引导预检（只读清单——本函数绝不写盘、绝不碰引导数据，
/// 只产出「若走引导修复将检查什么」的只读项清单）。
pub const BOOT_PRECHECK_ITEMS: [(&str, bool); 5] = [
    ("引导分区可读（只读探测）", true),
    ("引导文件签名校验（只读比对）", true),
    ("备用引导记录存在性（只读检查）", true),
    ("启动项顺序与登记一致（只读核对）", true),
    ("恢复环境自身可启动（自检）", true),
];

/// 引导预检执行（只读——返回全部通过与否与失败项）。
pub fn boot_precheck_run(results: &[bool]) -> (bool, usize) {
    let failed = results.iter().filter(|r| !**r).count();
    (failed == 0, failed)
}

/// F198 v8-b5 自检（并入 deep7 表族）。
pub fn run_recenv_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F198-v8b");

    // 恢复点排序：有效优先 → 新优先。
    let pr = restore_priority(&[(1, true, 10), (2, false, 90), (3, true, 50)]);
    set.add("prio order", pr == vec![3, 1, 2], "有效新点 > 有效旧点 > 无效点");
    let pr2 = restore_priority(&[]);
    set.add("prio empty", pr2.is_empty(), "");

    // 空间预检：够/不够带缺口。
    let s1 = space_precheck(500, 800);
    set.add("space ok", s1.enough && s1.deficit_mib == 0, "");
    let s2 = space_precheck(500, 300);
    set.add("space short", !s2.enough && s2.deficit_mib == 200, "缺口 200 MiB 如实报");

    // 续跑：全流程 / 中断原地续 / 完成后中断无效。
    let mut r = RestoreResume::new();
    r.advance();
    r.advance();
    set.add("resume verify->apply", r.stage == RestoreStage::Apply, "");
    r.interrupt();
    r.advance();
    set.add("resume blocked", r.stage == RestoreStage::Apply, "中断态推进无效");
    r.resume_after_interrupt();
    r.advance();
    set.add("resume after", r.stage == RestoreStage::Done, "续跑到完成");
    r.interrupt();
    set.add("resume done sticky", !r.interrupted && r.stage == RestoreStage::Done, "完成态不再可中断");

    // 引导预检：只读清单全通过 / 失败计数。
    set.add("boot precheck items", BOOT_PRECHECK_ITEMS.len() == 5 && BOOT_PRECHECK_ITEMS.iter().all(|(t, _)| t.contains("只读") || t.contains("自检")), "清单全只读");
    let (ok, f0) = boot_precheck_run(&[true, true, true, true, true]);
    set.add("boot precheck ok", ok && f0 == 0, "");
    let (ok2, f2) = boot_precheck_run(&[true, false, true, false, true]);
    set.add("boot precheck fail", !ok2 && f2 == 2, "失败项逐条计数");
    // b7-wave2：恢复点差分 / 演练排程 / 空间回收预估。
    let (du, rp) = restore_diff(&["a", "b", "c"], &["b", "c", "d"]);
    set.add("diff files", du == vec!["a"] && rp == vec!["d"], "回滚到旧点将撤销 a、重放 d");
    let (du2, rp2) = restore_diff(&["x"], &["x"]);
    set.add("diff none", du2.is_empty() && rp2.is_empty(), "同状态零差分");
    set.add("drill due", drill_due(0, 90), "从未演练 → 立即到期");
    set.add("drill fresh", !drill_due(30, 90), "30 天前刚演 → 未到期");
    set.add("reclaim est", reclaim_estimate(10, 800) == 8000, "10 份 × 800 MiB");
    // b8-wave3：恢复环境自检页 / 介质写速账。
    set.add("re env check", RECOVERY_SELFTEST.len() == 6 && RECOVERY_SELFTEST.iter().all(|(t, _)| !t.is_empty()), "六项自检齐");
    set.add("re env run", { let (ok, bad) = recovery_selftest_run(&[true; 6]); ok && bad == 0 }, "全绿通过");
    set.add("re env bad", recovery_selftest_run(&[true, false, true, false, true, true]).1 == 2, "失败逐条计");
    set.add("write speed", write_speed_grade(80) == "A", "80 MiB/s = A 档");
    set.add("write slow", write_speed_grade(5) == "C", "5 MiB/s = C 档提醒更换");
    // b9-wave4：恢复点标签 / 演练历史表。
    set.add("tag set", { let mut t = TagBook::new(); t.set(1, "升级前"); t.get(1) == Some("升级前") }, "打标可查");
    set.add("tag overwrite", { let mut t = TagBook::new(); t.set(1, "a"); t.set(1, "b"); t.get(1) == Some("b") }, "同点重打覆盖");
    set.add("tag none", TagBook::new().get(9).is_none(), "无标诚实");
    set.add("drill hist", { let mut h = DrillHistory::new(); h.record(true); h.record(false); h.record(true); h.pass_rate_permille() == 666 }, "3 演 2 过 = 666‰");
    set.add("drill hist empty", DrillHistory::new().pass_rate_permille() == 0, "空史零率");
    // b10-wave5：介质寿命预估 / 快照压缩比账。
    set.add("media life", media_life_years(1_000, 50) == 20, "1000 次写限 / 50 次/年 = 20 年");
    set.add("media life zero", media_life_years(1_000, 0) == 0, "零写入频度不外推");
    set.add("compress ratio", compress_ratio(800, 200) == 250, "4:1 = 250‰");
    set.add("compress none", compress_ratio(200, 200) == 1000, "压不动如实 1000‰");
    // b11-wave6：保留策略行 / 演练排期建议。
    set.add("keep policy", keep_policy_line(3, 90).contains("3 份") && keep_policy_line(3, 90).contains("90 天"), "策略行双参数");
    set.add("drill sched ok", drill_schedule(50, 10) == "本季已演练，无需排期", "季内已演（50 天前）");
    set.add("drill sched due", drill_schedule(100, 0).contains("排期"), "未演须排");
    // b12-wave7：恢复点 CSV / 标签数账。
    set.add("point csv", point_csv(&[(1, true)]).starts_with("id,valid\n"), "CSV 表头");
    set.add("tag count", { let mut t = TagBook::new(); t.set(1, "a"); t.set(2, "b"); t.len() == 2 }, "标签计数");
    // b13-wave8：有效点 CSV / 标签检索。
    set.add("valid csv", valid_point_csv(&[(1, true), (2, false)]).lines().count() == 2, "仅有效点入表");
    set.add("tag find", { let mut t = TagBook::new(); t.set(1, "升级前"); t.find_by_label("升级前") == Some(1) }, "按标签找点");
    // b14-wave9：演练排期 CSV。
    set.add("drill csv", drill_csv(&[("Q1", true)]).starts_with("quarter,passed\n"), "CSV 表头");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f198_v8b_prio_all_invalid() {
        // 全无效点也排序（不崩、按新到旧）。
        let pr = restore_priority(&[(1, false, 5), (2, false, 9)]);
        assert_eq!(pr, vec![2, 1]);
    }

    #[test]
    fn f198_v8b_resume_full_walk() {
        // 三推到完成，第四推无效（终态粘滞）。
        let mut r = RestoreResume::new();
        for _ in 0..3 {
            r.advance();
        }
        assert_eq!(r.stage, RestoreStage::Done);
        r.advance();
        assert_eq!(r.stage, RestoreStage::Done);
    }

    #[test]
    fn f198_v8b_space_exact() {
        // 恰好够 = 够（边界语义）。
        let s = space_precheck(500, 500);
        assert!(s.enough);
    }

    #[test]
    fn f198_v8b_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b7（第二波）：恢复点差分 / 演练到期 / 空间回收预估。
// 判据源：主册【验收判据】「回滚前告知影响面（哪些文件会变）」。
// ---------------------------------------------------------------------------

/// 恢复点差分（现状态 vs 目标点 → (将撤销的, 将重放的) 文件集）。
pub fn restore_diff(current: &[&str], target: &[&str]) -> (Vec<String>, Vec<String>) {
    let undo: Vec<String> = current
        .iter()
        .filter(|c| !target.contains(c))
        .map(|c| alloc::string::String::from(*c))
        .collect();
    let replay: Vec<String> = target
        .iter()
        .filter(|t| !current.contains(t))
        .map(|t| alloc::string::String::from(*t))
        .collect();
    (undo, replay)
}

/// 演练到期（距上次演练天数达周期 → 到期；从未演练立即到期）。
pub fn drill_due(days_since: u64, cycle_days: u64) -> bool {
    days_since == 0 || days_since >= cycle_days
}

/// 空间回收预估（可回收快照份数 × 单份均值）。
pub fn reclaim_estimate(reclaimable: u64, avg_mib: u64) -> u64 {
    reclaimable * avg_mib
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f198_v8c_diff_symmetric_union() {
        // 撤销 ∪ 重放 = 对称差（差分数学完备）。
        let (u, r) = restore_diff(&["a", "b"], &["b", "c"]);
        assert_eq!(u.len() + r.len(), 2);
    }

    #[test]
    fn f198_v8c_drill_boundary() {
        // 恰满周期 = 到期。
        assert!(drill_due(90, 90));
    }

    #[test]
    fn f198_v8c_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：恢复环境自检页 / 介质写速分档。
// 判据源：主册【状态与异常】「恢复环境自身要能证明自己可用」。
// ---------------------------------------------------------------------------

/// 恢复环境自检清单（六项——进恢复环境先自证）。
pub const RECOVERY_SELFTEST: [(&str, bool); 6] = [
    ("内存自检（基础区）", true),
    ("恢复内核完整性（自带哈希）", true),
    ("显示最小可用（基础帧缓冲）", true),
    ("输入最简可用（键盘）", true),
    ("备份介质可读（只读探测）", true),
    ("日志通道可写（恢复区）", true),
];

/// 自检执行（结果集与清单对齐 → 通过与否 + 失败计数）。
pub fn recovery_selftest_run(results: &[bool]) -> (bool, usize) {
    let n = results.len().min(RECOVERY_SELFTEST.len());
    let failed = results[..n].iter().filter(|r| !**r).count();
    (failed == 0 && n == RECOVERY_SELFTEST.len(), failed)
}

/// 介质写速分档（MiB/s → A/B/C——C 档建议更换介质）。
pub fn write_speed_grade(mib_per_s: u64) -> &'static str {
    if mib_per_s >= 50 {
        "A"
    } else if mib_per_s >= 20 {
        "B"
    } else {
        "C"
    }
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f198_v8e_speed_boundary() {
        // 分档边界：恰在界上归高档。
        assert_eq!(write_speed_grade(50), "A");
        assert_eq!(write_speed_grade(20), "B");
    }

    #[test]
    fn f198_v8e_selftest_short() {
        // 结果集短于清单 = 不通过（漏检即红）。
        let (ok, _) = recovery_selftest_run(&[true; 3]);
        assert!(!ok);
    }

    #[test]
    fn f198_v8e_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9（第四波）：恢复点标签 / 演练历史表。
// 判据源：主册【交互设计】「恢复点可命名（『升级前』比『day 47』好记）」。
// ---------------------------------------------------------------------------

/// 标签账（点 id → 人话标签）。
#[derive(Default)]
pub struct TagBook {
    tags: Vec<(u64, alloc::string::String)>,
}

impl TagBook {
    pub fn new() -> TagBook {
        TagBook { tags: Vec::new() }
    }

    /// 打标（同点重打覆盖——一标一定）。
    pub fn set(&mut self, point_id: u64, label: &str) {
        match self.tags.iter_mut().find(|(id, _)| *id == point_id) {
            Some((_, l)) => *l = alloc::string::String::from(label),
            None => self.tags.push((point_id, alloc::string::String::from(label))),
        }
    }

    pub fn get(&self, point_id: u64) -> Option<&str> {
        self.tags.iter().find(|(id, _)| *id == point_id).map(|(_, l)| l.as_str())
    }
}

/// 演练历史（通过率分位账）。
#[derive(Default)]
pub struct DrillHistory {
    results: Vec<bool>,
}

impl DrillHistory {
    pub fn new() -> DrillHistory {
        DrillHistory { results: Vec::new() }
    }

    pub fn record(&mut self, passed: bool) {
        self.results.push(passed);
    }

    /// 通过率 permille（空史 0）。
    pub fn pass_rate_permille(&self) -> u64 {
        if self.results.is_empty() {
            return 0;
        }
        let passed = self.results.iter().filter(|r| **r).count();
        passed as u64 * 1000 / self.results.len() as u64
    }
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f198_v9_drill_all_pass() {
        let mut h = DrillHistory::new();
        for _ in 0..4 {
            h.record(true);
        }
        assert_eq!(h.pass_rate_permille(), 1000);
    }

    #[test]
    fn f198_v9_tag_unicode() {
        // 中文标签保真（用户内容神圣）。
        let mut t = TagBook::new();
        t.set(5, "升级到 v3.2.2 之前");
        assert_eq!(t.get(5), Some("升级到 v3.2.2 之前"));
    }

    #[test]
    fn f198_v9_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：介质寿命预估 / 快照压缩比账。
// 判据源：主册【设计细节】「恢复介质寿命可预期（写入限频公开）」。
// ---------------------------------------------------------------------------

/// 介质寿命预估（写入限次数 / 年写入频度 → 年数；零频度不外推）。
pub fn media_life_years(write_cycles_limit: u64, writes_per_year: u64) -> u64 {
    if writes_per_year == 0 {
        return 0;
    }
    write_cycles_limit / writes_per_year
}

/// 快照压缩比（原始 MiB → 压缩后 MiB → 压缩率 permille，1000 = 无压缩收益）。
pub fn compress_ratio(raw_mib: u64, packed_mib: u64) -> u64 {
    if raw_mib == 0 {
        return 1000;
    }
    (packed_mib * 1000 / raw_mib).max(1)
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f198_v10_compress_better_than_raw() {
        // 压缩收益恒 ≤1000‰（压缩不放大）。
        assert!(compress_ratio(1000, 100) < 1000);
    }

    #[test]
    fn f198_v10_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b11（第六波）：保留策略行 / 演练排期建议。
// ---------------------------------------------------------------------------

/// 保留策略行（keep_recent 份 + ttl 天 → 一行说明——设置页「快照保留」格）。
pub fn keep_policy_line(keep_recent: u64, ttl_days: u64) -> alloc::string::String {
    alloc::format!("快照保留策略：最近 {} 份 + 全部锚点，保存 {} 天", keep_recent, ttl_days)
}

/// 演练排期建议（days_since_last + 距季末天数 → 文案）。
pub fn drill_schedule(days_since_last: u64, days_to_quarter_end: u64) -> &'static str {
    if days_since_last < 90 {
        "本季已演练，无需排期"
    } else if days_to_quarter_end > 14 {
        "季末前两周安排演练窗口"
    } else {
        "已临季末——本周排期演练"
    }
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f198_v11_sched_urgent() {
        assert!(drill_schedule(120, 5).contains("本周"));
    }

    #[test]
    fn f198_v11_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b12（第七波）：恢复点 CSV / 标签计数。
// ---------------------------------------------------------------------------

/// 恢复点 CSV（id,valid）。
pub fn point_csv(rows: &[(u64, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("id,valid\n");
    for (id, ok) in rows {
        out.push_str(&alloc::format!("{},{}\n", id, ok));
    }
    out
}

impl TagBook {
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f198_v12_tag_empty() {
        assert!(TagBook::new().is_empty());
    }

    #[test]
    fn f198_v12_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：有效点 CSV / 标签反查。
// ---------------------------------------------------------------------------

/// 有效点 CSV（只导 valid 点——无效点不进恢复列表）。
pub fn valid_point_csv(rows: &[(u64, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("id\n");
    for (id, ok) in rows {
        if *ok {
            out.push_str(&alloc::format!("{}\n", id));
        }
    }
    out
}

impl TagBook {
    /// 按标签反查点 id（首个命中）。
    pub fn find_by_label(&self, label: &str) -> Option<u64> {
        self.tags.iter().find(|(_, l)| l == label).map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f198_v13_valid_only() {
        // 无有效点 = 只剩表头。
        assert_eq!(valid_point_csv(&[(1, false)]).lines().count(), 1);
    }

    #[test]
    fn f198_v13_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b14（第九波）：演练排期 CSV。
// ---------------------------------------------------------------------------

/// 演练 CSV（quarter,passed）。
pub fn drill_csv(rows: &[(&str, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("quarter,passed\n");
    for (q, ok) in rows {
        out.push_str(&alloc::format!("{},{}\n", q, ok));
    }
    out
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f198_v14_drill_csv_empty() {
        assert_eq!(drill_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f198_v14_run_checks_pass() {
        assert!(run_recenv_deep7b_checks().all_passed());
    }
}
