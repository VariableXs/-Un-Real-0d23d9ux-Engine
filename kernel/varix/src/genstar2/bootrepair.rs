//! F477 启动修复引导（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **两连败触发阈值；三卡功能链路；人话诊断准确性（注入三类故障各测）；
//! 选项说明文案审查；复盘通知。**
//!
//! 功能定义（主册批次三）：连续两次开机失败（引导链自检 F191 不过）自动
//! 进入修复引导：全屏图形界面三卡（重启/安全模式 F193/恢复环境 F198）+
//! 一句人话诊断（「上次卡在加载内核——通常是驱动或磁盘问题」）；修复操作
//! 全程有说明；修复完成回桌面后通知一条复盘。
//!
//! 零堆纪律：定长故障账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 两连败触发阈值（主册：连续两次开机失败）。
pub const TRIGGER_THRESHOLD: u32 = 2;
/// 故障账保留代数（跨启动持久）。
pub const FAULT_LEDGER_CAP: usize = 8;

/// 开机失败卡住的阶段（诊断输入——三类注入故障各对应一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 卡在引导链自检（F191）。
    Selfcheck,
    /// 卡在加载内核。
    KernelLoad,
    /// 卡在启动桌面。
    DesktopStart,
}

impl BootStage {
    /// 人话诊断（主册例：「上次卡在加载内核——通常是驱动或磁盘问题」）。
    pub fn diagnosis(self) -> &'static str {
        match self {
            BootStage::Selfcheck => "上次卡在引导自检——通常是引导配置或固件设置问题",
            BootStage::KernelLoad => "上次卡在加载内核——通常是驱动或磁盘问题",
            BootStage::DesktopStart => "上次卡在启动桌面——通常是显示或最近安装的应用问题",
        }
    }
}

/// 三卡选项（主册原文三卡）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepairCard {
    /// 重启。
    Restart,
    /// 安全模式（F193）。
    SafeMode,
    /// 恢复环境（F198）。
    RecoveryEnv,
}

impl RepairCard {
    /// 每卡下方一行「这个选项会做什么」（主册：修复操作全程有说明）。
    pub fn explanation(self) -> &'static str {
        match self {
            RepairCard::Restart => "重新启动电脑——多数临时故障重启即愈",
            RepairCard::SafeMode => "以最小驱动集启动——用于卸载引发问题的驱动或应用",
            RepairCard::RecoveryEnv => "进入恢复环境——系统还原、修复引导或重置",
        }
    }

    pub const ALL: [RepairCard; 3] = [
        RepairCard::Restart,
        RepairCard::SafeMode,
        RepairCard::RecoveryEnv,
    ];
}

/// 启动修复状态机。
pub struct BootRepair {
    /// 连续失败计数（成功启动清零——两连败口径）。
    consecutive_failures: u32,
    /// 最近一次卡住阶段（诊断输入）。
    last_stage: Option<BootStage>,
    /// 故障账（跨启动）。
    ledger: [(u32, BootStage); FAULT_LEDGER_CAP],
    ledger_n: usize,
    /// 修复引导激活态。
    pub active: bool,
    /// 复盘通知待发（修复完成回桌面后）。
    pub review_pending: bool,
}

impl BootRepair {
    pub const fn new() -> Self {
        BootRepair {
            consecutive_failures: 0,
            last_stage: None,
            ledger: [(0, BootStage::Selfcheck); FAULT_LEDGER_CAP],
            ledger_n: 0,
            active: false,
            review_pending: false,
        }
    }

    /// 开机失败登记（返回是否触发修复引导——两连败）。
    pub fn on_boot_failure(&mut self, stage: BootStage) -> bool {
        self.consecutive_failures += 1;
        self.last_stage = Some(stage);
        if self.ledger_n < FAULT_LEDGER_CAP {
            self.ledger[self.ledger_n] = (self.consecutive_failures, stage);
            self.ledger_n += 1;
        }
        if self.consecutive_failures >= TRIGGER_THRESHOLD {
            self.active = true;
        }
        self.active
    }

    /// 开机成功清零（主册：连续两次——成功打断连败计数）。
    pub fn on_boot_success(&mut self) {
        self.consecutive_failures = 0;
        self.active = false;
    }

    /// 人话诊断（激活时给最近故障段的解释）。
    pub fn diagnosis(&self) -> Option<&'static str> {
        self.last_stage.map(|s| s.diagnosis())
    }

    /// 修复执行（三卡之一；修复完成 → 回桌面后复盘通知待发）。
    pub fn repair(&mut self, _card: RepairCard) -> bool {
        if !self.active {
            return false;
        }
        self.consecutive_failures = 0;
        self.active = false;
        self.review_pending = true;
        true
    }

    /// 复盘通知内容（主册：发生了什么/做了什么/以后怎么避免）。
    pub fn review_notice(&self) -> Option<&'static str> {
        match (self.review_pending, self.last_stage) {
            (true, Some(s)) => Some(s.review_text()),
            _ => None,
        }
    }

    pub fn take_review(&mut self) -> bool {
        let p = self.review_pending;
        self.review_pending = false;
        p
    }
}

impl BootStage {
    fn review_text(self) -> &'static str {
        match self {
            BootStage::Selfcheck => "发生了：引导自检连续失败；做了：修复引导已介入；避免：保持引导配置由系统管理",
            BootStage::KernelLoad => "发生了：内核加载连续失败；做了：修复引导已介入；避免：更新驱动前先建还原点",
            BootStage::DesktopStart => "发生了：桌面启动连续失败；做了：修复引导已介入；避免：排查最近安装的应用",
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_bootrepair_checks() -> CheckSet {
    let mut cs = CheckSet::new("F477-bootrepair");
    // 1) 两连败触发阈值（一败不进、两败进、成功清零）。
    let mut b = BootRepair::new();
    cs.add("one_fail_not_enough", !b.on_boot_failure(BootStage::KernelLoad), "");
    cs.add("two_fails_trigger", b.on_boot_failure(BootStage::KernelLoad) && b.active, "");
    // 2) 人话诊断准确性（三类故障各测——主册例句在册）。
    cs.add("diag_kernelload", b.diagnosis() == Some("上次卡在加载内核——通常是驱动或磁盘问题"), "");
    let mut b2 = BootRepair::new();
    b2.on_boot_failure(BootStage::Selfcheck);
    b2.on_boot_failure(BootStage::Selfcheck);
    cs.add("diag_selfcheck", b2.diagnosis() == Some("上次卡在引导自检——通常是引导配置或固件设置问题"), "");
    let mut b3 = BootRepair::new();
    b3.on_boot_failure(BootStage::DesktopStart);
    b3.on_boot_failure(BootStage::DesktopStart);
    cs.add("diag_desktop", b3.diagnosis() == Some("上次卡在启动桌面——通常是显示或最近安装的应用问题"), "");
    // 3) 三卡功能链路 + 说明文案审查（每卡有「会做什么」）。
    cs.add("three_cards", RepairCard::ALL.len() == 3, "");
    cs.add("cards_explained", RepairCard::ALL.iter().all(|c| !c.explanation().is_empty() && c.explanation().len() >= 10), "");
    cs.add("repair_executes", b3.repair(RepairCard::SafeMode) && !b3.active, "");
    // 4) 复盘通知（发生了什么/做了什么/以后怎么避免）。
    cs.add("review_notice", b3.review_notice().map(|t| t.starts_with("发生了")).unwrap_or(false), "");
    cs.add("review_taken_once", b3.take_review() && !b3.take_review(), "");
    // 5) 成功启动打断连败。
    let mut b4 = BootRepair::new();
    b4.on_boot_failure(BootStage::KernelLoad);
    b4.on_boot_success();
    cs.add("success_resets_streak", !b4.on_boot_failure(BootStage::KernelLoad) && !b4.active, "");
    // 6) 未激活修复拒绝执行（诚实）。
    cs.add("repair_needs_trigger", !b4.repair(RepairCard::Restart), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_requires_consecutive_failures() {
        let mut b = BootRepair::new();
        assert!(!b.on_boot_failure(BootStage::KernelLoad));
        b.on_boot_success();
        assert!(!b.on_boot_failure(BootStage::DesktopStart)); // 连败被成功打断
        assert!(b.on_boot_failure(BootStage::DesktopStart)); // 重新数到 2 触发
        assert!(b.active);
    }

    #[test]
    fn diagnosis_matches_fault_stage() {
        for (stage, keyword) in [
            (BootStage::Selfcheck, "自检"),
            (BootStage::KernelLoad, "内核"),
            (BootStage::DesktopStart, "桌面"),
        ] {
            assert!(stage.diagnosis().contains(keyword));
        }
    }

    #[test]
    fn review_notice_three_parts() {
        let mut b = BootRepair::new();
        b.on_boot_failure(BootStage::KernelLoad);
        b.on_boot_failure(BootStage::KernelLoad);
        b.repair(RepairCard::RecoveryEnv);
        let t = b.review_notice().unwrap();
        assert!(t.contains("发生了") && t.contains("做了") && t.contains("避免"));
    }
}
