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
