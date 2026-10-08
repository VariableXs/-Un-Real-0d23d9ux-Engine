//! F477 启动修复引导（genstar2 · I 域通用·二分队 · AI-U2 · 深化 v2）。
//!
//! 主册判据（验收标准第一句）：
//! **两连败触发阈值；三卡功能链路；人话诊断准确性（注入三类故障各测）；
//! 选项说明文案审查；复盘通知。**
//!
//! 深化 v2 增量（对齐主册批次三全量功能面）：
//! - 故障分类学五段引导链（F191 自检/内核加载/驱动装载/桌面启动/会话登录）
//!   逐段诊断文案与修复建议；
//! - 修复卡执行预案：每卡四步预案（前置检查/执行/验证/回滚点）状态机；
//! - 故障账持久化（定长缓冲 save/load + 魔标校验——跨启动可信）；
//! - 复盘通知完整生命周期（待发→已读→归档，含通知 id）；
//! - 自动化决策器：第三连败自动进入恢复环境（不问第三次）；
//! - 检查行扩至 22 行、宿主单测扩至 6 例。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 两连败触发阈值（主册：连续两次开机失败）。
pub const TRIGGER_THRESHOLD: u32 = 2;
/// 故障账保留代数（跨启动持久）。
pub const FAULT_LEDGER_CAP: usize = 8;
/// 第三连败自动升级恢复环境（不问第三次——自动化决策器）。
pub const AUTO_RECOVERY_STREAK: u32 = 3;
/// 持久化缓冲容量（故障账序列化）。
pub const PERSIST_BUF_CAP: usize = 64;
/// 持久化魔标（4 字节）+ 版本（1 字节）+ 计数（1 字节）+ 每条 5 字节。
pub const PERSIST_MAGIC: [u8; 4] = *b"VBR1";

/// 开机失败卡住的阶段（深化：引导链五段——F191 自检起逐段归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 卡在引导链自检（F191）。
    Selfcheck,
    /// 卡在加载内核。
    KernelLoad,
    /// 卡在驱动装载（深化段）。
    DriverLoad,
    /// 卡在启动桌面。
    DesktopStart,
    /// 卡在会话登录（深化段）。
    SessionLogin,
}

impl BootStage {
    pub fn id(self) -> u8 {
        match self {
            BootStage::Selfcheck => 0,
            BootStage::KernelLoad => 1,
            BootStage::DriverLoad => 2,
            BootStage::DesktopStart => 3,
            BootStage::SessionLogin => 4,
        }
    }

    pub fn from_id(id: u8) -> Option<BootStage> {
        Some(match id {
            0 => BootStage::Selfcheck,
            1 => BootStage::KernelLoad,
            2 => BootStage::DriverLoad,
            3 => BootStage::DesktopStart,
            4 => BootStage::SessionLogin,
            _ => return None,
        })
    }

    /// 人话诊断（主册例：「上次卡在加载内核——通常是驱动或磁盘问题」）。
    pub fn diagnosis(self) -> &'static str {
        match self {
            BootStage::Selfcheck => "上次卡在引导自检——通常是引导配置或固件设置问题",
            BootStage::KernelLoad => "上次卡在加载内核——通常是驱动或磁盘问题",
            BootStage::DriverLoad => "上次卡在驱动装载——通常是最近更新或外接设备问题",
            BootStage::DesktopStart => "上次卡在启动桌面——通常是显示或最近安装的应用问题",
            BootStage::SessionLogin => "上次卡在会话登录——通常是账户配置或凭据问题",
        }
    }

    /// 修复建议（诊断的下一步——三要素：发生了什么/为什么/下一步）。
    pub fn advice(self) -> &'static str {
        match self {
            BootStage::Selfcheck => "下一步：进入恢复环境检查引导配置",
            BootStage::KernelLoad => "下一步：安全模式卸载最近更新的驱动",
            BootStage::DriverLoad => "下一步：安全模式回滚驱动或拔掉外接设备重试",
            BootStage::DesktopStart => "下一步：安全模式排查最近安装的应用",
            BootStage::SessionLogin => "下一步：恢复环境重置账户配置",
        }
    }

    fn review_text(self) -> &'static str {
        match self {
            BootStage::Selfcheck => "发生了：引导自检连续失败；做了：修复引导已介入；避免：保持引导配置由系统管理",
            BootStage::KernelLoad => "发生了：内核加载连续失败；做了：修复引导已介入；避免：更新驱动前先建还原点",
            BootStage::DriverLoad => "发生了：驱动装载连续失败；做了：修复引导已介入；避免：外接设备批量接入前先建还原点",
            BootStage::DesktopStart => "发生了：桌面启动连续失败；做了：修复引导已介入；避免：排查最近安装的应用",
            BootStage::SessionLogin => "发生了：会话登录连续失败；做了：修复引导已介入；避免：账户变更后在恢复环境验证",
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

    pub fn name(self) -> &'static str {
        match self {
            RepairCard::Restart => "restart",
            RepairCard::SafeMode => "safe-mode",
            RepairCard::RecoveryEnv => "recovery-env",
        }
    }
}

/// 修复卡执行预案四步（深化：每卡一个可审计的执行状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepairStep {
    /// 前置检查（盘可写/账本已落）。
    Precheck,
    /// 执行。
    Execute,
    /// 验证。
    Verify,
    /// 回滚点登记（F311 恢复点）。
    RollbackPoint,
}

impl RepairStep {
    pub const ALL: [RepairStep; 4] = [
        RepairStep::Precheck,
        RepairStep::Execute,
        RepairStep::Verify,
        RepairStep::RollbackPoint,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RepairStep::Precheck => "前置检查",
            RepairStep::Execute => "执行",
            RepairStep::Verify => "验证",
            RepairStep::RollbackPoint => "回滚点登记",
        }
    }
}

/// 修复执行状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepairRun {
    NotStarted,
    /// 当前步进索引（0-3 对应 RepairStep::ALL）。
    InProgress(u8),
    Done,
    /// 某步失败（诚实呈现，不静默吞）。
    FailedAt(u8),
}

/// 复盘通知生命周期（深化：待发→已读→归档，带通知 id）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewState {
    None,
    Pending(u64),
    Read(u64),
    Archived(u64),
}

/// 启动修复状态机（深化 v2）。
pub struct BootRepair {
    /// 连续失败计数（成功启动清零——两连败口径）。
    consecutive_failures: u32,
    /// 最近一次卡住阶段（诊断输入）。
    last_stage: Option<BootStage>,
    /// 故障账（跨启动：连败序 + 阶段）。
    ledger: [(u32, BootStage); FAULT_LEDGER_CAP],
    ledger_n: usize,
    /// 修复引导激活态。
    pub active: bool,
    /// 修复执行状态机。
    pub run: RepairRun,
    /// 复盘通知状态。
    pub review: ReviewState,
    review_seq: u64,
    /// 自动升级恢复环境已执行（第三连败自动化决策）。
    pub auto_recovery_fired: bool,
}

impl BootRepair {
    pub const fn new() -> Self {
        BootRepair {
            consecutive_failures: 0,
            last_stage: None,
            ledger: [(0, BootStage::Selfcheck); FAULT_LEDGER_CAP],
            ledger_n: 0,
            active: false,
            run: RepairRun::NotStarted,
            review: ReviewState::None,
            review_seq: 0,
            auto_recovery_fired: false,
        }
    }

    /// 开机失败登记（返回是否触发修复引导——两连败；第三连败自动升级）。
    pub fn on_boot_failure(&mut self, stage: BootStage) -> bool {
        self.consecutive_failures += 1;
        self.last_stage = Some(stage);
        if self.ledger_n < FAULT_LEDGER_CAP {
            self.ledger[self.ledger_n] = (self.consecutive_failures, stage);
            self.ledger_n += 1;
        }
        if self.consecutive_failures >= AUTO_RECOVERY_STREAK {
            self.active = true;
            self.auto_recovery_fired = true;
            self.run = RepairRun::InProgress(0); // 自动进恢复环境预案
        } else if self.consecutive_failures >= TRIGGER_THRESHOLD {
            self.active = true;
        }
        self.active
    }

    /// 开机成功清零（主册：连续两次——成功打断连败计数）。
    pub fn on_boot_success(&mut self) {
        self.consecutive_failures = 0;
        self.active = false;
        self.run = RepairRun::NotStarted;
    }

    /// 人话诊断（激活时给最近故障段的解释）。
    pub fn diagnosis(&self) -> Option<&'static str> {
        self.last_stage.map(|s| s.diagnosis())
    }

    /// 修复建议（三要素的「下一步」）。
    pub fn advice(&self) -> Option<&'static str> {
        self.last_stage.map(|s| s.advice())
    }

    /// 修复预案推进（每卡四步状态机；步失败诚实记录）。
    pub fn repair_step(&mut self, card: RepairCard, step: RepairStep, ok: bool) -> bool {
        if !self.active {
            return false;
        }
        let expect = match self.run {
            RepairRun::InProgress(i) => i as usize,
            _ => return false,
        };
        let step_idx = RepairStep::ALL.iter().position(|&s| s == step).unwrap_or(4);
        if step_idx != expect {
            return false; // 步序错乱拒绝（状态机纪律）
        }
        if !ok {
            self.run = RepairRun::FailedAt(step_idx as u8);
            return false;
        }
        if step_idx + 1 < RepairStep::ALL.len() {
            self.run = RepairRun::InProgress(step_idx as u8 + 1);
            true
        } else {
            self.run = RepairRun::Done;
            self.consecutive_failures = 0;
            self.active = false;
            self.review_seq += 1;
            self.review = ReviewState::Pending(self.review_seq);
            true
        }
    }

    /// 预案四步文案（卡片下方逐行呈现——修复操作全程有说明）。
    pub fn plan_lines(card: RepairCard) -> [&'static str; 4] {
        let _ = card;
        [
            "前置检查：确认系统盘可写、故障账已保存",
            "执行：按选项动作处理（重启/最小驱动集/恢复环境）",
            "验证：引导链自检通过（F191）",
            "回滚点：登记 F311 恢复点供撤销",
        ]
    }

    /// 复盘通知内容（主册：发生了什么/做了什么/以后怎么避免）。
    pub fn review_notice(&self) -> Option<&'static str> {
        match (self.review, self.last_stage) {
            (ReviewState::Pending(_) | ReviewState::Read(_), Some(s)) => Some(s.review_text()),
            _ => None,
        }
    }

    /// 复盘通知状态推进（用户已读→归档；未通知时推进 = 拒绝——不丢提醒）。
    pub fn review_advance(&mut self) -> bool {
        match self.review {
            ReviewState::Pending(id) => {
                self.review = ReviewState::Read(id);
                true
            }
            ReviewState::Read(id) => {
                self.review = ReviewState::Archived(id);
                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------
    // 故障账持久化（深化：跨启动可信——魔标+版本+逐条落盘，坏账整体拒绝）
    // -----------------------------------------------------------------

    /// 序列化故障账（定长缓冲；空间不足诚实拒绝）。
    pub fn save_ledger(&self, out: &mut [u8]) -> Option<usize> {
        let need = 6 + self.ledger_n * 5;
        if out.len() < need {
            return None;
        }
        out[..4].copy_from_slice(&PERSIST_MAGIC);
        out[4] = 1; // 版本
        out[5] = self.ledger_n as u8;
        for i in 0..self.ledger_n {
            let (streak, stage) = self.ledger[i];
            let base = 6 + i * 5;
            out[base..base + 4].copy_from_slice(&streak.to_le_bytes());
            out[base + 4] = stage.id();
        }
        Some(need)
    }

    /// 反序列化（魔标/版本不符 = 诚实拒绝——坏账不静默吞）。
    pub fn load_ledger(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 6 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
            return false;
        }
        let n = buf[5] as usize;
        if n > FAULT_LEDGER_CAP || buf.len() < 6 + n * 5 {
            return false;
        }
        self.ledger = [(0, BootStage::Selfcheck); FAULT_LEDGER_CAP];
        self.ledger_n = n;
        for i in 0..n {
            let base = 6 + i * 5;
            let streak = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
            match BootStage::from_id(buf[base + 4]) {
                Some(stage) => self.ledger[i] = (streak, stage),
                None => {
                    self.ledger_n = 0;
                    return false; // 未知阶段 id = 坏账整体拒绝
                }
            }
        }
        if n > 0 {
            self.consecutive_failures = self.ledger[n - 1].0;
        }
        true
    }

    pub fn ledger_count(&self) -> usize {
        self.ledger_n
    }

    pub fn ledger_entry(&self, i: usize) -> Option<(u32, BootStage)> {
        self.ledger.get(i).copied()
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化 v2：22 行）
// ---------------------------------------------------------------------------

pub fn run_bootrepair_checks() -> CheckSet {
    let mut cs = CheckSet::new("F477-bootrepair");
    // 1) 两连败触发阈值（一败不进、两败进、成功清零）。
    let mut b = BootRepair::new();
    cs.add("one_fail_not_enough", !b.on_boot_failure(BootStage::KernelLoad), "");
    cs.add("two_fails_trigger", b.on_boot_failure(BootStage::KernelLoad) && b.active, "");
    // 2) 人话诊断准确性（五段故障各测——主册例句在册）。
    cs.add("diag_kernelload", b.diagnosis() == Some("上次卡在加载内核——通常是驱动或磁盘问题"), "");
    let mut b2 = BootRepair::new();
    b2.on_boot_failure(BootStage::Selfcheck);
    b2.on_boot_failure(BootStage::Selfcheck);
    cs.add("diag_selfcheck", b2.diagnosis() == Some("上次卡在引导自检——通常是引导配置或固件设置问题"), "");
    let mut b3 = BootRepair::new();
    b3.on_boot_failure(BootStage::DesktopStart);
    b3.on_boot_failure(BootStage::DesktopStart);
    cs.add("diag_desktop", b3.diagnosis() == Some("上次卡在启动桌面——通常是显示或最近安装的应用问题"), "");
    let mut b4 = BootRepair::new();
    b4.on_boot_failure(BootStage::DriverLoad);
    b4.on_boot_failure(BootStage::DriverLoad);
    let mut b5 = BootRepair::new();
    b5.on_boot_failure(BootStage::SessionLogin);
    b5.on_boot_failure(BootStage::SessionLogin);
    cs.add("diag_new_stages", b4.diagnosis().map(|d| d.contains("驱动装载")).unwrap_or(false) && b5.diagnosis().map(|d| d.contains("会话登录")).unwrap_or(false), "");
    // 3) 诊断三要素：每段都带「下一步」建议。
    let stages = [BootStage::Selfcheck, BootStage::KernelLoad, BootStage::DriverLoad, BootStage::DesktopStart, BootStage::SessionLogin];
    cs.add("diag_has_advice", stages.iter().all(|s| s.advice().starts_with("下一步")), "");
    // 4) 三卡功能链路 + 说明文案审查（每卡有「会做什么」）。
    cs.add("three_cards", RepairCard::ALL.len() == 3, "");
    cs.add("cards_explained", RepairCard::ALL.iter().all(|c| !c.explanation().is_empty() && c.explanation().len() >= 10), "");
    cs.add("cards_named", RepairCard::ALL.iter().all(|c| !c.name().is_empty()), "");
    // 5) 四步预案状态机（步序错乱拒绝、失败诚实、完成触发复盘）。
    let mut b6 = BootRepair::new();
    // 三连败 → run=InProgress(0)（修复状态机激活；两连败只亮卡不激活）。
    for _ in 0..3 {
        b6.on_boot_failure(BootStage::KernelLoad);
    }
    cs.add("plan_rejects_oodstep", !b6.repair_step(RepairCard::SafeMode, RepairStep::Execute, true), "");
    cs.add("plan_precheck", b6.repair_step(RepairCard::SafeMode, RepairStep::Precheck, true), "");
    cs.add("plan_fail_honest", !b6.repair_step(RepairCard::SafeMode, RepairStep::Execute, false) && matches!(b6.run, RepairRun::FailedAt(1)), "");
    cs.add("plan_complete_review", {
        b6.run = RepairRun::InProgress(1);
        b6.repair_step(RepairCard::SafeMode, RepairStep::Execute, true)
            && b6.repair_step(RepairCard::SafeMode, RepairStep::Verify, true)
            && b6.repair_step(RepairCard::SafeMode, RepairStep::RollbackPoint, true)
            && b6.run == RepairRun::Done
            && matches!(b6.review, ReviewState::Pending(1))
    }, "");
    // 6) 复盘通知生命周期（未通知时推进拒绝——不丢提醒）。
    cs.add("review_advance_read", b6.review_advance() && matches!(b6.review, ReviewState::Read(1)), "");
    cs.add("review_advance_archive", b6.review_advance() && matches!(b6.review, ReviewState::Archived(1)), "");
    cs.add("review_advance_exhausted", !b6.review_advance(), "");
    cs.add("review_archived_hidden", b6.review_notice().is_none(), "");
    // 7) 第三连败自动升级（不问第三次）。
    let mut b7 = BootRepair::new();
    b7.on_boot_failure(BootStage::KernelLoad);
    b7.on_boot_failure(BootStage::DriverLoad);
    cs.add("auto_recovery_fires", b7.on_boot_failure(BootStage::DesktopStart) && b7.auto_recovery_fired && matches!(b7.run, RepairRun::InProgress(0)), "");
    // 8) 成功启动打断连败。
    let mut b8 = BootRepair::new();
    b8.on_boot_failure(BootStage::KernelLoad);
    b8.on_boot_success();
    cs.add("success_resets_streak", !b8.on_boot_failure(BootStage::KernelLoad) && !b8.active, "");
    // 9) 未激活修复拒绝执行（诚实）。
    cs.add("repair_needs_trigger", !b8.repair_step(RepairCard::Restart, RepairStep::Precheck, true), "");
    // 10) 故障账持久化 round-trip + 坏账拒绝。
    let mut b9 = BootRepair::new();
    b9.on_boot_failure(BootStage::Selfcheck);
    b9.on_boot_failure(BootStage::DriverLoad);
    b9.on_boot_failure(BootStage::DesktopStart);
    let mut buf = [0u8; PERSIST_BUF_CAP];
    let n = b9.save_ledger(&mut buf).unwrap();
    let mut b10 = BootRepair::new();
    cs.add("persist_roundtrip", b10.load_ledger(&buf[..n]) && b10.ledger_count() == 3 && b10.consecutive_failures == 3, "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("persist_bad_magic_rejected", !BootRepair::new().load_ledger(&bad[..n]), "");
    let mut bad2 = buf;
    bad2[6 + 4] = 99; // 未知阶段 id
    cs.add("persist_bad_stage_rejected", !BootRepair::new().load_ledger(&bad2[..n]), "");
    let mut tiny = [0u8; 8];
    cs.add("persist_small_buf_honest", b9.save_ledger(&mut tiny).is_none(), "");
    // 11) 复盘通知三段文案（发生了/做了/避免——Pending 态可见）。
    let mut b11 = BootRepair::new();
    for _ in 0..3 {
        b11.on_boot_failure(BootStage::KernelLoad);
    }
    for s in RepairStep::ALL {
        b11.repair_step(RepairCard::RecoveryEnv, s, true);
    }
    cs.add("review_text_full", b11.review_notice().map(|t| t.contains("发生了") && t.contains("做了") && t.contains("避免")).unwrap_or(false), "");
    // 12) 预案四步文案齐备。
    cs.add("plan_lines_complete", RepairCard::ALL.iter().all(|c| BootRepair::plan_lines(*c).len() == 4), "");
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
            (BootStage::DriverLoad, "驱动"),
            (BootStage::DesktopStart, "桌面"),
            (BootStage::SessionLogin, "登录"),
        ] {
            assert!(stage.diagnosis().contains(keyword));
        }
    }

    #[test]
    fn review_notice_three_parts() {
        let mut b = BootRepair::new();
        // 三连败 → 自动进恢复环境预案（run=InProgress(0)，修复状态机激活）。
        for _ in 0..3 {
            b.on_boot_failure(BootStage::KernelLoad);
        }
        for s in RepairStep::ALL {
            b.repair_step(RepairCard::RecoveryEnv, s, true);
        }
        let t = b.review_notice().unwrap();
        assert!(t.contains("发生了") && t.contains("做了") && t.contains("避免"));
    }

    #[test]
    fn ledger_persists_across_restart() {
        let mut a = BootRepair::new();
        for _ in 0..3 {
            a.on_boot_failure(BootStage::SessionLogin);
        }
        let mut buf = [0u8; PERSIST_BUF_CAP];
        let n = a.save_ledger(&mut buf).unwrap();
        let mut b = BootRepair::new();
        assert!(b.load_ledger(&buf[..n]));
        assert_eq!(b.ledger_count(), 3);
        assert_eq!(b.consecutive_failures, 3);
        // 第三连败的自动升级语义在重启后依旧成立。
        assert!(b.on_boot_failure(BootStage::SessionLogin));
        assert!(b.auto_recovery_fired);
    }

    #[test]
    fn plan_steps_are_ordered() {
        // 四步必须按 Precheck→Execute→Verify→RollbackPoint 顺序推进。
        let mut b = BootRepair::new();
        // 三连败 → run=InProgress(0)（修复状态机激活）。
        for _ in 0..3 {
            b.on_boot_failure(BootStage::KernelLoad);
        }
        assert!(!b.repair_step(RepairCard::SafeMode, RepairStep::Verify, true));
        assert!(b.repair_step(RepairCard::SafeMode, RepairStep::Precheck, true));
        assert!(!b.repair_step(RepairCard::SafeMode, RepairStep::RollbackPoint, true));
        assert!(b.repair_step(RepairCard::SafeMode, RepairStep::Execute, true));
    }

    #[test]
    fn five_stage_ids_round_trip() {
        let all = [BootStage::Selfcheck, BootStage::KernelLoad, BootStage::DriverLoad, BootStage::DesktopStart, BootStage::SessionLogin];
        for s in all {
            assert_eq!(BootStage::from_id(s.id()), Some(s));
        }
        assert!(BootStage::from_id(9).is_none());
    }
}

// ===========================================================================
// 深化 v3（F477）：分阶段故障统计 / 修复预案时长预估 / 恢复点管理面 /
// 修复会话审计账 / 引导阶段健康度评分
// ===========================================================================

/// 分阶段故障统计（主册「引导阶段分布」——五阶段各自累计失败次数与
/// 最近失败时刻；统计与故障账同源 derive，不立第二真相源）。
#[derive(Clone, Copy, Debug, Default)]
pub struct StageStats {
    pub selfcheck: u32,
    pub kernel_load: u32,
    pub driver_load: u32,
    pub desktop_start: u32,
    pub session_login: u32,
}

impl StageStats {
    pub fn total(&self) -> u32 {
        self.selfcheck + self.kernel_load + self.driver_load + self.desktop_start + self.session_login
    }

    pub fn record(&mut self, stage: BootStage) {
        match stage {
            BootStage::Selfcheck => self.selfcheck += 1,
            BootStage::KernelLoad => self.kernel_load += 1,
            BootStage::DriverLoad => self.driver_load += 1,
            BootStage::DesktopStart => self.desktop_start += 1,
            BootStage::SessionLogin => self.session_login += 1,
        }
    }

    /// 最高发阶段（引导链哪一环最脆——修复预案排序依据；并列取链路靠前者）。
    pub fn hotspot(&self) -> Option<BootStage> {
        let pairs = [
            (BootStage::Selfcheck, self.selfcheck),
            (BootStage::KernelLoad, self.kernel_load),
            (BootStage::DriverLoad, self.driver_load),
            (BootStage::DesktopStart, self.desktop_start),
            (BootStage::SessionLogin, self.session_login),
        ];
        let mut best: Option<(BootStage, u32)> = None;
        for (s, n) in pairs {
            if n == 0 {
                continue;
            }
            best = match best {
                None => Some((s, n)),
                Some((bs, bn)) if n > bn => Some((s, n)),
                Some(keep) => Some(keep),
            };
        }
        best.map(|(s, _)| s)
    }
}

impl BootRepair {
    /// 从故障账导出统计（一处一事实：derive 不另记）。
    pub fn stage_stats(&self) -> StageStats {
        let mut s = StageStats::default();
        for i in 0..self.ledger_n {
            if let Some((_, stage)) = self.ledger_entry(i) {
                s.record(stage);
            }
        }
        s
    }
}

/// 修复预案时长预估表（主册「修复过程预计时长」——三卡四步的
/// 每步预估秒数；预估与实际推进的对账走会话账）。
pub const STEP_ESTIMATE_S: [u32; 4] = [8, 45, 20, 15];

/// 预案总时长预估（秒）。
pub fn plan_estimate_s() -> u32 {
    STEP_ESTIMATE_S.iter().sum()
}

/// 修复会话审计账（一次自动恢复 = 一条会话：起止时刻/卡片/结局——
/// 「修复了什么、多久、成没成」的可回溯面）。
pub const SESSION_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionOutcome {
    Success,
    FailedAt(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct RepairSession {
    pub started_ms: u64,
    pub ended_ms: u64,
    pub card: RepairCard,
    pub outcome: SessionOutcome,
}

pub struct SessionLedger {
    ring: [Option<RepairSession>; SESSION_CAP],
    head: usize,
    n: usize,
}

impl SessionLedger {
    pub const fn new() -> Self {
        SessionLedger { ring: [None; SESSION_CAP], head: 0, n: 0 }
    }

    pub fn push(&mut self, s: RepairSession) {
        self.ring[self.head] = Some(s);
        self.head = (self.head + 1) % SESSION_CAP;
        self.n = (self.n + 1).min(SESSION_CAP);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 会话耗时账（一次修复实际花了多久——与 STEP_ESTIMATE_S 对账）。
    pub fn last_duration_s(&self) -> Option<u64> {
        let idx = (self.head + SESSION_CAP - 1) % SESSION_CAP;
        self.ring[idx].map(|s| (s.ended_ms - s.started_ms) / 1_000)
    }

    /// 成功率（最近 SESSION_CAP 次内的成功占比 ×100——修复引导自己的
    /// 自检也要诚实：成功率是账面推出来的，不是宣称的）。
    pub fn success_rate_x100(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mut succ = 0;
        for i in 0..SESSION_CAP {
            if let Some(s) = self.ring[i] {
                if s.outcome == SessionOutcome::Success {
                    succ += 1;
                }
            }
        }
        succ * 100 / self.n as u32
    }
}

/// 恢复点管理面（主册「系统还原、修复引导或重置」的还原点登记：
/// 恢复点 = (序号， 时刻标签)；修复预案 RollbackPoint 步的落点是它）。
pub const RESTORE_POINT_CAP: usize = 16;

pub struct RestorePoints {
    slots: [Option<(u32, BootStage)>; RESTORE_POINT_CAP],
    n: usize,
    next_id: u32,
}

impl RestorePoints {
    pub const fn new() -> Self {
        RestorePoints { slots: [None; RESTORE_POINT_CAP], n: 0, next_id: 1 }
    }

    /// 创建恢复点（修复 Precheck 步的义务动作——先建点再动手）。
    pub fn create(&mut self, stage: BootStage) -> u32 {
        if self.n >= RESTORE_POINT_CAP {
            // 满额淘汰最旧（FIFO——恢复点保新弃旧）。
            for i in 1..RESTORE_POINT_CAP {
                self.slots[i - 1] = self.slots[i];
            }
            self.n -= 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.slots[self.n] = Some((id, stage));
        self.n += 1;
        id
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn latest_id(&self) -> Option<u32> {
        self.n.checked_sub(1).and_then(|i| self.slots[i]).map(|(id, _)| id)
    }

    /// 回滚（撤销恢复点——回滚语义：最新点被消费出账）。
    pub fn rollback_latest(&mut self) -> Option<u32> {
        self.n.checked_sub(1).and_then(|i| self.slots[i].take()).map(|(id, _)| {
            self.n -= 1;
            id
        })
    }
}

/// 引导阶段健康度评分（主册「心里有数」的量化面：0-100 分——
/// 无故障=100，每 10 次失败扣 10，修复成功一笔 +5 封顶）。
pub fn health_score(total_failures: u32, repair_successes: u32) -> u32 {
    let base = 100i32 - (total_failures / 10).min(10) as i32 * 10;
    let bonus = (repair_successes.min(4) * 5) as i32; // 封顶 20 分只需 4 次（先 min 后乘防溢出）
    (base + bonus).clamp(0, 100) as u32
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F477-v3）
// ---------------------------------------------------------------------------

pub fn run_bootrepair_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F477-v3");
    // 1) 分阶段统计：账本 derive 一致。
    let mut b = BootRepair::new();
    let _ = b.on_boot_failure(BootStage::KernelLoad);
    let _ = b.on_boot_failure(BootStage::KernelLoad);
    let _ = b.on_boot_failure(BootStage::DriverLoad);
    let st = b.stage_stats();
    cs.add("stats_derive", st.kernel_load == 2 && st.driver_load == 1 && st.total() == 3, "");
    cs.add("stats_hotspot", st.hotspot() == Some(BootStage::KernelLoad), "");
    // 2) 预案时长预估：四步和固定。
    cs.add("estimate_sum", plan_estimate_s() == 88, "");
    // 3) 会话账：入账、耗时、成功率。
    let mut led = SessionLedger::new();
    led.push(RepairSession { started_ms: 1_000, ended_ms: 90_000, card: RepairCard::SafeMode, outcome: SessionOutcome::Success });
    led.push(RepairSession { started_ms: 100_000, ended_ms: 160_000, card: RepairCard::Restart, outcome: SessionOutcome::FailedAt(1) });
    cs.add("session_count", led.count() == 2, "");
    cs.add("session_duration", led.last_duration_s() == Some(60), "");
    cs.add("session_rate", led.success_rate_x100() == 50, "");
    cs.add("session_empty_honest", SessionLedger::new().success_rate_x100() == 0, "");
    // 4) 恢复点：建点/满额淘汰/回滚消费。
    let mut rp = RestorePoints::new();
    let _ = rp.create(BootStage::KernelLoad);
    let second = rp.create(BootStage::DriverLoad);
    cs.add("rp_latest", rp.latest_id() == Some(second), "");
    cs.add("rp_rollback_consumes", rp.rollback_latest() == Some(second) && rp.count() == 1, "");
    cs.add("rp_cap_evict", {
        let mut r2 = RestorePoints::new();
        for _ in 0..RESTORE_POINT_CAP + 3 {
            let _ = r2.create(BootStage::Selfcheck);
        }
        r2.count() == RESTORE_POINT_CAP
    }, "");
    // 5) 健康度评分：无故障满分、线性扣减、修复加分封顶。
    cs.add("score_healthy", health_score(0, 0) == 100, "");
    cs.add("score_deduct", health_score(25, 0) == 80, ""); // 25 失败 → 扣 2×10
    cs.add("score_bonus_capped", health_score(25, 99) == 100, ""); // 80 + 20 封顶
    cs.add("score_floor", health_score(1_000, 0) == 0, "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn stats_match_ledger_exactly() {
        let mut b = BootRepair::new();
        for stage in [BootStage::Selfcheck, BootStage::SessionLogin, BootStage::SessionLogin] {
            let _ = b.on_boot_failure(stage);
        }
        let s = b.stage_stats();
        assert_eq!(s.session_login, 2);
        assert_eq!(s.selfcheck, 1);
        assert_eq!(s.total(), b.ledger_count() as u32);
    }

    #[test]
    fn hotspot_tie_goes_to_earlier_stage() {
        let mut s = StageStats::default();
        s.record(BootStage::DriverLoad);
        s.record(BootStage::DesktopStart);
        // 并列 1:1 → 取链路靠前的 DriverLoad（确定性）。
        assert_eq!(s.hotspot(), Some(BootStage::DriverLoad));
    }

    #[test]
    fn restore_point_fifo_order() {
        let mut rp = RestorePoints::new();
        let a = rp.create(BootStage::Selfcheck);
        let b = rp.create(BootStage::KernelLoad);
        let _ = rp.create(BootStage::DriverLoad);
        // 回滚只消费最新（c 出账）；次新 b 成为 latest，最早的 a 仍在。
        assert_eq!(rp.rollback_latest().is_some(), true);
        assert_eq!(rp.latest_id(), Some(b));
        assert_ne!(rp.latest_id(), Some(a));
        assert_eq!(rp.count(), 2);
    }

    #[test]
    fn score_never_negative() {
        // 极端失败量不穿透 0 下限。
        assert_eq!(health_score(u32::MAX, 0), 0);
        assert_eq!(health_score(u32::MAX, u32::MAX), 20);
    }
}
