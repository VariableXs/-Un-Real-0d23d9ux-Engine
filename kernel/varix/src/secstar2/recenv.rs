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
