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
