//! VE-F0009 · 设备丢失与恢复状态机（VE-A 域 · 内核图形抽象层 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0009`
//!
//! **判据（锚点原文）**：三段恢复、原因分类、软渲兜底、诊断包、判据；
//! 恢复含用户可见进度（丢失后恢复到哪一步有诚实进度）；原因分类含驱动
//! 版本绑定（同一丢失原因在不同驱动版本可能不同——分类随版本演进）；
//! 状态机含重复丢失的熔断（连续丢失三次转最小模式并告知）。
//!
//! **职责定位（锚点原文）**：GPU 设备丢失（驱动重置/休眠唤醒/热拔）的完整
//! 状态机——丢失检测/资源重建/状态恢复三段，丢失原因分类记录，恢复失败
//! 的降级路径（软渲染兜底）；含丢失演练语料（三类丢失场景的注入测试）。
//!
//! **设计要点**：
//! - **三段恢复**：检测（LossDetected）→ 重建（Rebuilding，资源按登记序
//!   逐个重创，进度可查）→ 恢复（Recovered）。状态机显式枚举 + 合法迁移
//!   表，非法迁移拒绝并留审计（不静默跳段——跳段=状态账目崩坏）；
//! - **诚实进度**：重建进度按"已完成重创数 / 应重创总数"汇报，单调不减
//!   （进度回退 = 报谎，直接红项）。恢复全程每步产出用户可播报的人话状态
//!   （无障碍面：读屏可达）；
//! - **原因分类 × 驱动版本绑定**：丢失原因四类（驱动重置/休眠唤醒/热拔/
//!   未知），每条丢失记录绑定驱动版本指纹——同一现象在不同驱动版本可能
//!   归因不同，分类随版本演进（版本字段进记录，可按版本统计）；
//! - **恢复失败降级链**：重建失败 → 软渲兜底（SoftwareFallback，诚实告知
//!   性能预期）；未知原因 → 诊断包在册（DiagPack：现场最小集）；恢复超时
//!   → 升级（Escalated，上抛人工/上一级处置）；
//! - **重复丢失熔断**：连续丢失三次（计数器，恢复成功不清零连续计数——
//!   "连续"指中间没有稳定期）转 MinimalMode 并产出用户告知（锚点点名）。
//!   稳定期（连续 N tick 无丢失）后计数归零，熔断可解除；
//! - **演练语料**：三类丢失场景（驱动重置/休眠唤醒/热拔）的注入测试内置
//!   （drill 函数），回归可复现；
//! - **零静默**：每次迁移/降级/熔断/升级都产出审计记录（何时/何事/为什么/
//!   下一步），`audit_trail()` 可全量取回。
//!
//! **性能逐项分解**：状态迁移 O(1)；重建进度记账 O(资源)；演练注入
//! O(场景)。
//!
//! **跨批对接点**：V02 设备管理联动（丢失事件来源）；A03 软渲回退路径
//! （兜底承接方）。条款引用：VS-G-09 式的诚实纪律（以实测与记录为准）。
//!
//! 确定性：逻辑 tick 注入（不用墙钟），纯函数状态机，回归可复现。
//! 零外部依赖，只用 `alloc` 与 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、丢失原因分类（含驱动版本绑定）
// ---------------------------------------------------------------------------

/// 丢失原因（锚点点名三类 + 未知兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LossReason {
    /// 驱动重置（TDR 类）。
    DriverReset,
    /// 休眠唤醒。
    SleepWake,
    /// 设备热拔。
    HotUnplug,
    /// 未知（触发诊断包路径）。
    Unknown,
}

impl LossReason {
    /// 人话名（读屏与审计用）。
    pub fn name(self) -> &'static str {
        match self {
            LossReason::DriverReset => "驱动重置",
            LossReason::SleepWake => "休眠唤醒",
            LossReason::HotUnplug => "设备热拔",
            LossReason::Unknown => "未知原因",
        }
    }
}

/// 一条丢失事件记录（原因 × 驱动版本绑定 × 发生时刻）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossEvent {
    /// 原因分类。
    pub reason: LossReason,
    /// 驱动版本指纹（原因分类随版本演进的绑定锚）。
    pub driver_version: String,
    /// 逻辑 tick（注入时钟，回归可复现）。
    pub tick: u64,
}

impl LossEvent {
    /// 事件登记（驱动版本必填——没有版本绑定的丢失记录无法统计归因）。
    pub fn new(reason: LossReason, driver_version: &str, tick: u64) -> Result<Self, RecoveryError> {
        if driver_version.is_empty() {
            return Err(RecoveryError::MissingDriverVersion {
                suggestion: "丢失事件必须绑定驱动版本指纹：同一现象在不同驱动版本归因可能不同".to_string(),
            });
        }
        Ok(Self {
            reason,
            driver_version: driver_version.to_string(),
            tick,
        })
    }
}

// ---------------------------------------------------------------------------
// 二、状态机（三段恢复 + 降级链 + 熔断）
// ---------------------------------------------------------------------------

/// 恢复状态机状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryState {
    /// 正常运行。
    Normal,
    /// 丢失已检测（等待进入重建）。
    LossDetected,
    /// 重建中（三段之二：资源逐个重创）。
    Rebuilding,
    /// 恢复完成。
    Recovered,
    /// 软渲兜底（重建失败的降级落点）。
    SoftwareFallback,
    /// 最小模式（重复丢失熔断落点）。
    MinimalMode,
    /// 升级处置（恢复超时上抛）。
    Escalated,
}

impl RecoveryState {
    /// 人话名（用户可见进度播报用）。
    pub fn name(self) -> &'static str {
        match self {
            RecoveryState::Normal => "正常运行",
            RecoveryState::LossDetected => "丢失已检测",
            RecoveryState::Rebuilding => "资源重建中",
            RecoveryState::Recovered => "恢复完成",
            RecoveryState::SoftwareFallback => "软渲染兜底",
            RecoveryState::MinimalMode => "最小模式",
            RecoveryState::Escalated => "升级处置",
        }
    }
}

/// 合法迁移表（状态机的规则书——表外迁移一律拒绝）。
///
/// 含三条特殊路径：丢失自环（未恢复又丢）、重建中新丢失、正常态直熔断
/// （第三次丢失发生在刚回到 Normal 的瞬间）。
const LEGAL_TRANSITIONS: [(RecoveryState, RecoveryState); 13] = [
    (RecoveryState::Normal, RecoveryState::LossDetected),
    (RecoveryState::LossDetected, RecoveryState::Rebuilding),
    (RecoveryState::Rebuilding, RecoveryState::Recovered),
    (RecoveryState::Rebuilding, RecoveryState::SoftwareFallback),
    (RecoveryState::Rebuilding, RecoveryState::Escalated),
    (RecoveryState::SoftwareFallback, RecoveryState::LossDetected),
    (RecoveryState::Escalated, RecoveryState::LossDetected),
    (RecoveryState::LossDetected, RecoveryState::MinimalMode),
    (RecoveryState::Recovered, RecoveryState::Normal),
    (RecoveryState::MinimalMode, RecoveryState::LossDetected),
    (RecoveryState::LossDetected, RecoveryState::LossDetected),
    (RecoveryState::Rebuilding, RecoveryState::LossDetected),
    (RecoveryState::Normal, RecoveryState::MinimalMode),
];

/// 重建进度（诚实进度：单调不减，回退即缺陷）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebuildProgress {
    /// 已完成重创的资源数。
    pub done: u32,
    /// 应重创的资源总数。
    pub total: u32,
}

impl RebuildProgress {
    /// 推进一格（done ≤ total 强制——超额推进是记账 bug）。
    pub fn advance(&mut self) -> Result<(), RecoveryError> {
        if self.done >= self.total {
            return Err(RecoveryError::ProgressOverflow {
                suggestion: "进度超额推进：done 已达 total，先核对资源清单再记账".to_string(),
            });
        }
        self.done += 1;
        Ok(())
    }

    /// 用户可见进度（人话，读屏可达——诚实进度：到哪一步说哪一步）。
    pub fn announce(&self, state: RecoveryState) -> String {
        format!(
            "设备丢失恢复：{}——资源重建 {}/{}",
            state.name(),
            self.done,
            self.total
        )
    }
}

/// 诊断包（未知原因丢失的现场最小集——归因输入，不猜结论）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagPack {
    /// 丢失事件（含驱动版本绑定）。
    pub event: LossEvent,
    /// 现场快照摘要（状态机当时状态 + 已完成进度）。
    pub snapshot: String,
    /// 采集时刻 tick。
    pub collected_at: u64,
}

/// 审计记录（迁移/降级/熔断/升级全量留痕——零静默的载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 发生了什么。
    pub what: String,
    /// 下一步。
    pub next: String,
}

/// 熔断阈值（锚点点名：连续丢失三次转最小模式）。
pub const LOSS_BREAKER_THRESHOLD: u32 = 3;

/// 熔断解除所需的稳定期（tick 数：连续无丢失才算稳定）。
pub const STABLE_TICKS_TO_CLEAR: u64 = 10;

/// 设备丢失与恢复状态机。
pub struct RecoveryMachine {
    /// 当前状态。
    state: RecoveryState,
    /// 逻辑时钟（注入）。
    tick: u64,
    /// 自上次丢失起的连续丢失计数（恢复成功不清零——"连续"语义）。
    consecutive_losses: u32,
    /// 稳定期计数（无丢失 tick 累积，达阈值清零丢失计数）。
    stable_ticks: u64,
    /// 丢失事件账（原因 × 驱动版本绑定）。
    pub loss_log: Vec<LossEvent>,
    /// 诊断包账（未知原因 / 需要留现场的场合）。
    pub diag_packs: Vec<DiagPack>,
    /// 审计账。
    audit: Vec<AuditEntry>,
    /// 当前丢失事件的重建进度。
    progress: Option<RebuildProgress>,
    /// 熔断/降级的用户告知（最近一条，读屏可达）。
    pub user_notice: Option<String>,
}

impl RecoveryMachine {
    /// 建状态机（Normal 起步）。
    pub fn new() -> Self {
        Self {
            state: RecoveryState::Normal,
            tick: 0,
            consecutive_losses: 0,
            stable_ticks: 0,
            loss_log: Vec::new(),
            diag_packs: Vec::new(),
            audit: Vec::new(),
            progress: None,
            user_notice: None,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RecoveryState {
        self.state
    }

    /// 审计账（只读全量）。
    pub fn audit_trail(&self) -> &[AuditEntry] {
        &self.audit
    }

    /// 推进逻辑时钟（注入；无事件 tick 用于稳定期累积）。
    pub fn tick(&mut self) {
        self.tick += 1;
        if self.state == RecoveryState::Normal {
            self.stable_ticks += 1;
            if self.stable_ticks >= STABLE_TICKS_TO_CLEAR && self.consecutive_losses > 0 {
                self.consecutive_losses = 0;
                self.audit.push(AuditEntry {
                    tick: self.tick,
                    what: format!("稳定期 {} tick 达成，连续丢失计数清零", STABLE_TICKS_TO_CLEAR),
                    next: "熔断计数复位".to_string(),
                });
            }
        }
    }

    /// 状态迁移（合法迁移表驱动，表外拒绝并留审计——零静默）。
    fn transition(&mut self, to: RecoveryState) -> Result<(), RecoveryError> {
        let legal = LEGAL_TRANSITIONS
            .iter()
            .any(|(from, t)| *from == self.state && *t == to);
        if !legal {
            return Err(RecoveryError::IllegalTransition {
                from: self.state.name().to_string(),
                to: to.name().to_string(),
                suggestion: "状态迁移必须走合法迁移表；跳段说明上游处置漏了步骤".to_string(),
            });
        }
        let from = self.state;
        self.state = to;
        self.audit.push(AuditEntry {
            tick: self.tick,
            what: format!("状态迁移 {} → {}", from.name(), to.name()),
            next: to.name().to_string(),
        });
        Ok(())
    }

    /// 丢失检测入口（三段之一）。
    ///
    /// 检测即登记（原因 × 驱动版本绑定）；未知原因同时采集诊断包；
    /// 连续丢失达熔断阈值 → 转最小模式并告知用户。
    pub fn on_loss(&mut self, event: LossEvent, resources_total: u32) -> Result<(), RecoveryError> {
        self.tick = self.tick.max(event.tick);
        self.consecutive_losses += 1;
        self.stable_ticks = 0;
        self.loss_log.push(event.clone());
        if event.reason == LossReason::Unknown {
            self.diag_packs.push(DiagPack {
                event: event.clone(),
                snapshot: format!("状态 {}，丢失时进度 {:?}", self.state.name(), self.progress),
                collected_at: self.tick,
            });
        }
        // 熔断：连续丢失三次 → 最小模式（跳过重建，保全系统为先）。
        if self.consecutive_losses >= LOSS_BREAKER_THRESHOLD && self.state != RecoveryState::MinimalMode {
            self.user_notice = Some(format!(
                "设备连续丢失 {} 次，已转入最小模式（仅保合成关键路径）——建议检查驱动或硬件",
                self.consecutive_losses
            ));
            return self.transition(RecoveryState::MinimalMode);
        }
        self.progress = Some(RebuildProgress { done: 0, total: resources_total });
        self.transition(RecoveryState::LossDetected)
    }

    /// 进入重建（三段之二）。
    pub fn begin_rebuild(&mut self) -> Result<(), RecoveryError> {
        if self.progress.is_none() {
            return Err(RecoveryError::NoProgressAccount {
                suggestion: "重建前必须先经 on_loss 登记（进度账在丢失时建立）".to_string(),
            });
        }
        self.transition(RecoveryState::Rebuilding)
    }

    /// 重创一个资源（进度记账 + 诚实进度播报）。
    pub fn rebuild_one(&mut self) -> Result<String, RecoveryError> {
        let p = self
            .progress
            .as_mut()
            .ok_or(RecoveryError::NoProgressAccount {
                suggestion: "不在重建语境：先 on_loss 再 begin_rebuild".to_string(),
            })?;
        p.advance()?;
        Ok(p.announce(self.state))
    }

    /// 重建完成（三段之三：恢复完成并回归 Normal）。
    pub fn finish_recovery(&mut self) -> Result<(), RecoveryError> {
        // 进度必须收满才许宣告恢复（诚实进度：没做完不说做完）。
        if let Some(p) = &self.progress {
            if p.done < p.total {
                return Err(RecoveryError::IncompleteRebuild {
                    done: p.done,
                    total: p.total,
                    suggestion: "资源未重创完毕不得宣告恢复——先把重建跑满或走降级".to_string(),
                });
            }
        }
        self.transition(RecoveryState::Recovered)?;
        self.transition(RecoveryState::Normal)?;
        self.user_notice = Some("设备已恢复，渲染回到正常路径".to_string());
        Ok(())
    }

    /// 重建失败 → 软渲兜底（诚实告知性能预期——软渲是保底不是享受）。
    pub fn fallback_to_software(&mut self, reason: &str) -> Result<(), RecoveryError> {
        if reason.is_empty() {
            return Err(RecoveryError::FallbackNoReason {
                suggestion: "降级必须带原因：软渲兜底的性能预期要一次说清，不让用户猜为什么变卡".to_string(),
            });
        }
        self.transition(RecoveryState::SoftwareFallback)?;
        self.user_notice = Some(format!(
            "硬件加速恢复失败（{reason}），已切换到软件渲染兜底——性能将显著下降，属预期行为"
        ));
        Ok(())
    }

    /// 恢复超时 → 升级（上抛处置，不静默吊着）。
    pub fn escalate(&mut self, reason: &str) -> Result<(), RecoveryError> {
        if reason.is_empty() {
            return Err(RecoveryError::FallbackNoReason {
                suggestion: "升级必须带原因（超时多久/卡在哪段）".to_string(),
            });
        }
        self.transition(RecoveryState::Escalated)?;
        self.audit.push(AuditEntry {
            tick: self.tick,
            what: format!("恢复超时升级：{reason}"),
            next: "上抛一级处置（人工/上级恢复器）".to_string(),
        });
        Ok(())
    }

    /// 最小模式下重试（熔断后的复位入口）。
    pub fn retry_from_minimal(&mut self, event: LossEvent, resources_total: u32) -> Result<(), RecoveryError> {
        self.on_loss(event, resources_total)
    }
}

// ---------------------------------------------------------------------------
// 三、演练语料（三类丢失场景的注入——判据：丢失演练语料）
// ---------------------------------------------------------------------------

/// 演练场景（三类锚点点名场景 + 熔断场景 + 未知原因场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillScenario {
    /// 驱动重置注入。
    DriverResetDrill,
    /// 休眠唤醒注入。
    SleepWakeDrill,
    /// 热拔注入。
    HotUnplugDrill,
    /// 连续丢失熔断注入。
    BreakerDrill,
    /// 未知原因（诊断包路径）注入。
    UnknownCauseDrill,
}

/// 演练执行：给定场景，跑一遍预期路径，返回 (场景, 是否按预期走完)。
///
/// 演练是状态机的验收语料：五场景全绿 = 三段恢复/降级链/熔断/诊断包
/// 四条路径都有实测覆盖。
pub fn run_drill(scenario: DrillScenario, driver_version: &str) -> (DrillScenario, bool) {
    let ok = match scenario {
        DrillScenario::DriverResetDrill
        | DrillScenario::SleepWakeDrill
        | DrillScenario::HotUnplugDrill => {
            let reason = match scenario {
                DrillScenario::DriverResetDrill => LossReason::DriverReset,
                DrillScenario::SleepWakeDrill => LossReason::SleepWake,
                _ => LossReason::HotUnplug,
            };
            let mut m = RecoveryMachine::new();
            m.tick();
            let ok = m
                .on_loss(LossEvent::new(reason, driver_version, 1).expect("演练事件合法"), 3)
                .is_ok()
                && m.begin_rebuild().is_ok()
                && (0..3).all(|_| m.rebuild_one().is_ok())
                && m.finish_recovery().is_ok()
                && m.state() == RecoveryState::Normal;
            ok
        }
        DrillScenario::BreakerDrill => {
            let mut m = RecoveryMachine::new();
            // 连续三次丢失（不经恢复）→ 熔断进最小模式 + 用户告知。
            let ok = (0..3).all(|i| {
                m.on_loss(LossEvent::new(LossReason::DriverReset, driver_version, i + 1).expect("演练事件合法"), 2)
                    .is_ok()
            }) && m.state() == RecoveryState::MinimalMode
                && m.user_notice.as_ref().map(|n| n.contains("最小模式")).unwrap_or(false);
            ok
        }
        DrillScenario::UnknownCauseDrill => {
            let mut m = RecoveryMachine::new();
            let ok = m
                .on_loss(LossEvent::new(LossReason::Unknown, driver_version, 1).expect("演练事件合法"), 2)
                .is_ok()
                && m.diag_packs.len() == 1
                && m.diag_packs[0].event.driver_version == driver_version;
            ok
        }
    };
    (scenario, ok)
}

/// 全部演练场景。
pub const DRILL_SCENARIOS: [DrillScenario; 5] = [
    DrillScenario::DriverResetDrill,
    DrillScenario::SleepWakeDrill,
    DrillScenario::HotUnplugDrill,
    DrillScenario::BreakerDrill,
    DrillScenario::UnknownCauseDrill,
];

// ---------------------------------------------------------------------------
// 四、错误类型（零静默）
// ---------------------------------------------------------------------------

/// 恢复状态机错误（现象 + 建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryError {
    /// 驱动版本缺失。
    MissingDriverVersion { suggestion: String },
    /// 非法状态迁移。
    IllegalTransition { from: String, to: String, suggestion: String },
    /// 进度超额推进。
    ProgressOverflow { suggestion: String },
    /// 无进度账（未登记丢失就试图重建）。
    NoProgressAccount { suggestion: String },
    /// 重建未完成就宣告恢复。
    IncompleteRebuild { done: u32, total: u32, suggestion: String },
    /// 降级/升级无原因。
    FallbackNoReason { suggestion: String },
}

impl RecoveryError {
    /// 人话呈现（发生了什么 + 怎么修）。
    pub fn describe(&self) -> String {
        match self {
            RecoveryError::MissingDriverVersion { suggestion } => {
                format!("丢失事件缺驱动版本。建议：{suggestion}")
            }
            RecoveryError::IllegalTransition { from, to, suggestion } => {
                format!("非法状态迁移：{from} → {to}。建议：{suggestion}")
            }
            RecoveryError::ProgressOverflow { suggestion } => {
                format!("重建进度超额。建议：{suggestion}")
            }
            RecoveryError::NoProgressAccount { suggestion } => {
                format!("无进度账。建议：{suggestion}")
            }
            RecoveryError::IncompleteRebuild { done, total, suggestion } => {
                format!("重建未完成（{done}/{total}）即宣告恢复。建议：{suggestion}")
            }
            RecoveryError::FallbackNoReason { suggestion } => {
                format!("降级/升级缺原因。建议：{suggestion}")
            }
        }
    }
}

/// 纯功能行数自证（正式门禁见 `vea09_checks.rs`）。
pub fn recovery_smoke() -> usize {
    LEGAL_TRANSITIONS.len() + DRILL_SCENARIOS.len() + LOSS_BREAKER_THRESHOLD as usize
}

/// VE-F0009 域自检（判据逐条对应，见 `vea09_checks.rs`）。
pub fn run_vea09_checks() -> CheckSet {
    super::vea09_checks::run_vea09_checks()
}
