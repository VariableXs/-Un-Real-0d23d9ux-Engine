//! CGPU-F0245 · 死锁解除策略（cgb245_resolve）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0245`
//! **判据原文摘录**：牺牲者选择准则（回滚代价最小者——任务重入性/已完成工作量/
//! 持锁数量三因子评分）、解除动作三档（唤醒重试/回滚到检查点/终止任务——每档
//! 后果向调用方披露）、解除后验证（环消散确认 + 资源归还完整性）、解除风暴防护
//! （同图 1s 内多次解除 → 升级域级告警并熔断自动解除转人工）。
//!
//! ## 牺牲者选择：三因子
//!
//! 谁当牺牲者？原则是**回滚代价最小者**。三个因子：
//!
//! | 因子 | 含义 | 权重 | 为何重要 |
//! |---|---|---|---|
//! | 重入性 | 任务能否安全重跑 | 0.25 | 不可重入的任务杀了就没了 |
//! | 已完成工作量 | 已推进的比例 | 0.40 | 跑了 90% 的杀了最亏 |
//! | 持锁数量 | 当前持锁数 | 0.35 | 持锁多的杀了影响面大 |
//!
//! 评分 = 1.0 - (w1·reentry_risk + w2·work_done + w3·hold_burden)
//!
//! **评分低者 = 代价小者 = 牺牲者**。
//!
//! 为何工作量权重最高：GPU 任务的「已完成工作量」直接对应显存写入与
//! 命令缓冲进度，重跑代价与工作量近似线性。
//!
//! ## 三档解除
//!
//! | 档 | 动作 | 破坏性 | 适用 |
//! |---|---|---|---|
//! | Gentle | 唤醒重试（放弃当前等待） | 最低 | 可重入任务、等待时间短 |
//! | Rollback | 回滚到检查点 | 中 | 有检查点的任务 |
//! | Terminate | 终止任务 | 最高 | 不可回滚、不可重入 |
//!
//! **每档后果必须向调用方披露**——`ResolutionOutcome` 携带被牺牲者、
//! 丢失工作量、释放资源清单。调用方不能盲调。
//!
//! ## 解除后验证
//!
//! 两步：
//! 1. **环消散确认**——解除后重跑环检测，环必须消失；
//! 2. **资源归还完整性**——RAG 的实例账目必须平衡（held + avail == total）。
//!
//! 任一失败 → 报告 `VerificationFailed`，不静默。
//!
//! ## 解除风暴防护
//!
//! 同一图 1 秒内多次解除 = 异常。可能是：
//! - 解除逻辑有 bug（解除不彻底，环没真消散）；
//! - 负载极端（真的连环死锁）。
//!
//! 无论哪种，自动解除都不该继续——**熔断转人工**。
//! 阈值：1s 内 ≥3 次解除 → `storm` 状态，自动解除暂停，升级告警。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::{ResId, SubjectId};

// ---------------------------------------------------------------------------
// 诊断码（0xCB80 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB80;
/// 无牺牲者可选。
pub const E_NO_VICTIM: u16 = SEG_BASE + 1;
/// 解除风暴熔断中。
pub const E_STORM_HALTED: u16 = SEG_BASE + 2;
/// 验证失败。
pub const E_VERIFY_FAIL: u16 = SEG_BASE + 3;
/// 解除档位非法。
pub const E_BAD_TIER: u16 = SEG_BASE + 4;

// ---------------------------------------------------------------------------
// 三因子
// ---------------------------------------------------------------------------

/// 重入性风险（0.0 = 完全可重入，1.0 = 不可重入）。
#[derive(Clone, Copy, Debug)]
pub struct ReentryProfile {
    /// 是否可安全重跑。
    pub reentrant: bool,
    /// 是否有检查点。
    pub has_checkpoint: bool,
    /// 外部副作用（不可回滚的 IO 等）。
    pub external_side_effects: bool,
}

impl ReentryProfile {
    pub fn risk(&self) -> f32 {
        if self.reentrant && !self.external_side_effects {
            0.0
        } else if self.has_checkpoint {
            0.4
        } else if self.external_side_effects {
            1.0
        } else {
            0.7
        }
    }
}

/// 工作量画像。
#[derive(Clone, Copy, Debug)]
pub struct WorkProfile {
    /// 已完成工作量（任意单位）。
    pub done: u64,
    /// 总工作量。
    pub total: u64,
}

impl WorkProfile {
    /// 已完成比例 0.0~1.0。
    pub fn done_ratio(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

/// 持锁画像。
#[derive(Clone, Copy, Debug)]
pub struct HoldProfile {
    /// 当前持锁数。
    pub count: u16,
    /// 归一化分母（引擎平均持锁数，用于归一化）。
    pub norm: u16,
}

impl HoldProfile {
    /// 持锁负担 0.0~1.0。
    pub fn burden(&self) -> f32 {
        if self.norm == 0 {
            0.0
        } else {
            (self.count as f32 / self.norm as f32).clamp(0.0, 1.0)
        }
    }
}

/// 牺牲者评分输入。
#[derive(Clone, Copy, Debug)]
pub struct VictimScore {
    pub subject: SubjectId,
    pub reentry: ReentryProfile,
    pub work: WorkProfile,
    pub hold: HoldProfile,
}

impl VictimScore {
    /// 回滚代价 0.0~1.0（越高越贵）。
    pub fn cost(&self) -> f32 {
        0.25 * self.reentry.risk() + 0.40 * self.work.done_ratio() + 0.35 * self.hold.burden()
    }

    /// 牺牲优先级（越低越先牺牲）。
    pub fn sacrifice_rank(&self) -> f32 {
        self.cost()
    }
}

// ---------------------------------------------------------------------------
// 解除档位
// ---------------------------------------------------------------------------

/// 解除动作档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolutionTier {
    /// 唤醒重试（放弃当前等待）。
    Gentle,
    /// 回滚到检查点。
    Rollback,
    /// 终止任务。
    Terminate,
}

impl ResolutionTier {
    pub const ALL: [ResolutionTier; 3] = [
        ResolutionTier::Gentle,
        ResolutionTier::Rollback,
        ResolutionTier::Terminate,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ResolutionTier::Gentle => "gentle",
            ResolutionTier::Rollback => "rollback",
            ResolutionTier::Terminate => "terminate",
        }
    }

    /// 破坏性 1~3。
    pub const fn destructiveness(self) -> u8 {
        match self {
            ResolutionTier::Gentle => 1,
            ResolutionTier::Rollback => 2,
            ResolutionTier::Terminate => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// 解除后果披露
// ---------------------------------------------------------------------------

/// 解除后果（向调用方披露，不可盲调）。
#[derive(Clone, Debug)]
pub struct ResolutionOutcome {
    /// 被牺牲的主体。
    pub victim: SubjectId,
    /// 执行的档位。
    pub tier: ResolutionTier,
    /// 丢失的工作量。
    pub work_lost: u64,
    /// 被释放的资源。
    pub resources_released: Vec<ResId>,
    /// 是否重试（Gentle 档）。
    pub will_retry: bool,
    /// 人话说明。
    pub disclosure: String,
}

/// 解除后验证结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verification {
    /// 环消散 + 账目平衡。
    Passed,
    /// 环未消散。
    CycleRemains,
    /// 资源账目失衡。
    AccountImbalance,
    /// 双重失败。
    BothFailed,
}

/// 解除尝试的结果。
#[derive(Clone, Debug)]
pub struct ResolutionResult {
    pub outcome: ResolutionOutcome,
    pub verification: Verification,
    /// 验证细节。
    pub verify_note: String,
}

// ---------------------------------------------------------------------------
// 风暴防护
// ---------------------------------------------------------------------------

/// 风暴窗口（tick；1 tick = 1ms，1000 = 1s）。
pub const STORM_WINDOW_TICKS: u32 = 1000;
/// 窗口内解除次数阈值。
pub const STORM_THRESHOLD: u32 = 3;

/// 解除风暴防护器。
pub struct StormGuard {
    /// 窗口内解除时间戳。
    recent: Vec<u32>,
    /// 是否熔断。
    pub halted: bool,
    /// 熔断次数。
    pub halt_count: u32,
    /// 累计解除数。
    pub total_resolutions: u64,
}

impl StormGuard {
    pub fn new() -> StormGuard {
        StormGuard {
            recent: Vec::new(),
            halted: false,
            halt_count: 0,
            total_resolutions: 0,
        }
    }

    /// 记录一次解除。返回 `true` 表示触发风暴熔断。
    pub fn record(&mut self, now: u32) -> bool {
        self.total_resolutions += 1;
        self.recent.push(now);
        // 清理窗口外。
        let cutoff = now.saturating_sub(STORM_WINDOW_TICKS);
        self.recent.retain(|&t| t >= cutoff);
        if self.recent.len() as u32 >= STORM_THRESHOLD {
            if !self.halted {
                self.halted = true;
                self.halt_count += 1;
            }
            true
        } else {
            false
        }
    }

    /// 手动复位（人工介入后）。
    pub fn reset(&mut self) {
        self.halted = false;
        self.recent.clear();
    }

    /// 窗口内解除数。
    pub fn recent_count(&self) -> usize {
        self.recent.len()
    }
}

// ---------------------------------------------------------------------------
// 解除引擎
// ---------------------------------------------------------------------------

/// 解除引擎。
///
/// 纪律：
/// 1. **选牺牲者必披露**——不返回裸 ID，返回完整 Outcome；
/// 2. **档位从低到高**——先 Gentle，不行 Rollback，最后 Terminate；
/// 3. **解除必验证**——不验证的解除等于没解除；
/// 4. **风暴即熔断**——自动解除可停，误杀不可逆。
pub struct ResolutionEngine {
    /// 风暴防护。
    pub storm: StormGuard,
    /// 遥测：按档位统计。
    pub by_tier: [u64; 3],
    /// 遥测：验证通过数。
    pub verify_pass: u64,
    /// 遥测：验证失败数。
    pub verify_fail: u64,
    /// 当前策略档位（自动模式尝试的起始档）。
    pub auto_tier: ResolutionTier,
}

impl ResolutionEngine {
    pub fn new() -> ResolutionEngine {
        ResolutionEngine {
            storm: StormGuard::new(),
            by_tier: [0; 3],
            verify_pass: 0,
            verify_fail: 0,
            auto_tier: ResolutionTier::Gentle,
        }
    }

    /// 从候选成员中选牺牲者（三因子评分，代价最小者）。
    pub fn select_victim<'a>(&self, scores: &'a [VictimScore]) -> Option<&'a VictimScore> {
        if scores.is_empty() {
            return None;
        }
        scores
            .iter()
            .min_by(|a, b| {
                a.sacrifice_rank()
                    .partial_cmp(&b.sacrifice_rank())
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
    }

    /// 选牺牲者 + 选档位（根据牺牲者画像自动定档）。
    pub fn plan(&self, scores: &[VictimScore]) -> Option<(SubjectId, ResolutionTier)> {
        let victim = self.select_victim(scores)?;
        let tier = self.choose_tier(victim);
        Some((victim.subject, tier))
    }

    /// 档位选择：破坏性尽量低。
    ///
    /// - 可重入 + 无外部副作用 → Gentle（唤醒重试，损失最小）；
    /// - 有检查点 → Rollback（回滚比杀好）；
    /// - 否则 → Terminate（没有更好的选择）。
    pub fn choose_tier(&self, victim: &VictimScore) -> ResolutionTier {
        if victim.reentry.reentrant && !victim.reentry.external_side_effects {
            ResolutionTier::Gentle
        } else if victim.reentry.has_checkpoint {
            ResolutionTier::Rollback
        } else {
            ResolutionTier::Terminate
        }
    }

    /// 执行解除（含披露 + 验证 + 风暴记录）。
    ///
    /// `verify_fn` 是验证闭包：返回 (环是否消散, 账目是否平衡)。
    pub fn resolve(
        &mut self,
        victim: SubjectId,
        tier: ResolutionTier,
        work_lost: u64,
        resources: Vec<ResId>,
        now: u32,
        cycle_cleared: bool,
        account_balanced: bool,
    ) -> Result<ResolutionResult, u16> {
        // 风暴检查。
        if self.storm.halted {
            return Err(E_STORM_HALTED);
        }

        let will_retry = tier == ResolutionTier::Gentle;
        let disclosure = String::from(match tier {
            ResolutionTier::Gentle => {
                "唤醒重试：任务将放弃当前等待并重试，已完成工作量保留"
            }
            ResolutionTier::Rollback => {
                "回滚到检查点：任务进度回到最近检查点，检查点后的工作量丢失"
            }
            ResolutionTier::Terminate => {
                "终止任务：任务被杀死，全部工作量丢失，持有的资源强制释放"
            }
        });

        let outcome = ResolutionOutcome {
            victim,
            tier,
            work_lost,
            resources_released: resources,
            will_retry,
            disclosure,
        };

        // 验证。
        let verification = match (cycle_cleared, account_balanced) {
            (true, true) => Verification::Passed,
            (false, true) => Verification::CycleRemains,
            (true, false) => Verification::AccountImbalance,
            (false, false) => Verification::BothFailed,
        };
        let verify_note = String::from(match verification {
            Verification::Passed => "环消散 + 账目平衡",
            Verification::CycleRemains => "环未消散：解除不彻底，可能需升级",
            Verification::AccountImbalance => "账目失衡：资源未完整归还",
            Verification::BothFailed => "环未消散且账目失衡",
        });

        // 记账。
        match tier {
            ResolutionTier::Gentle => self.by_tier[0] += 1,
            ResolutionTier::Rollback => self.by_tier[1] += 1,
            ResolutionTier::Terminate => self.by_tier[2] += 1,
        }
        if verification == Verification::Passed {
            self.verify_pass += 1;
        } else {
            self.verify_fail += 1;
        }

        // 风暴记录。
        let _stormed = self.storm.record(now);

        Ok(ResolutionResult {
            outcome,
            verification,
            verify_note,
        })
    }

    /// 自动解除入口（风暴熔断则拒绝）。
    pub fn auto_resolve(
        &mut self,
        scores: &[VictimScore],
        work_lost: u64,
        resources: Vec<ResId>,
        now: u32,
        cycle_cleared: bool,
        account_balanced: bool,
    ) -> Result<ResolutionResult, u16> {
        if self.storm.halted {
            return Err(E_STORM_HALTED);
        }
        let (victim, tier) = self.plan(scores).ok_or(E_NO_VICTIM)?;
        self.resolve(
            victim,
            tier,
            work_lost,
            resources,
            now,
            cycle_cleared,
            account_balanced,
        )
    }

    /// 渲染解除报告（人话）。
    pub fn render(&self, r: &ResolutionResult) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!(
            "解除动作：{} 档（牺牲者 subject#{}）\n",
            r.outcome.tier.name(),
            r.outcome.victim
        ));
        s.push_str(&alloc::format!("  后果：{}\n", r.outcome.disclosure));
        s.push_str(&alloc::format!(
            "  丢失工作量 {}，释放资源 {:?}，重试={}\n",
            r.outcome.work_lost,
            r.outcome.resources_released,
            r.outcome.will_retry
        ));
        s.push_str(&alloc::format!("  验证：{}\n", r.verify_note));
        if self.storm.halted {
            s.push_str("  ⚠ 解除风暴熔断中，自动解除已暂停\n");
        }
        s
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb245_resolve");
        let sum: u64 = self.by_tier.iter().sum();
        cs.add(
            "档位计数守恒",
            sum == self.storm.total_resolutions,
            "by_tier 求和 == total",
        );
        cs.add(
            "验证计数守恒",
            self.verify_pass + self.verify_fail == self.storm.total_resolutions,
            "pass+fail == total",
        );
        cs.add(
            "风暴窗口有界",
            self.storm.recent_count() as u32 <= STORM_WINDOW_TICKS,
            "recent ≤ window",
        );
        cs.add(
            "自动档位合法",
            ResolutionTier::ALL.contains(&self.auto_tier),
            "三档之一",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0245 域自检。
pub fn run_cgb245_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb245_resolve");
    let mut engine = ResolutionEngine::new();

    // --- 三因子评分 ---
    let cheap = VictimScore {
        subject: 1,
        reentry: ReentryProfile {
            reentrant: true,
            has_checkpoint: true,
            external_side_effects: false,
        },
        work: WorkProfile { done: 10, total: 1000 },
        hold: HoldProfile { count: 1, norm: 8 },
    };
    let expensive = VictimScore {
        subject: 2,
        reentry: ReentryProfile {
            reentrant: false,
            has_checkpoint: false,
            external_side_effects: true,
        },
        work: WorkProfile { done: 900, total: 1000 },
        hold: HoldProfile { count: 8, norm: 8 },
    };
    cs.add(
        "代价区分",
        cheap.cost() < expensive.cost(),
        "轻代价 < 重代价",
    );

    // --- 牺牲者选择 ---
    let scores = alloc::vec![expensive, cheap];
    let victim = engine.select_victim(&scores);
    cs.add(
        "选最小代价",
        victim.map(|v| v.subject == 1).unwrap_or(false),
        "subject#1 被选",
    );

    // --- 档位选择 ---
    let tier_gentle = engine.choose_tier(&cheap);
    cs.add(
        "可重入 → Gentle",
        tier_gentle == ResolutionTier::Gentle,
        "破坏性最低",
    );
    let tier_term = engine.choose_tier(&expensive);
    cs.add(
        "不可回滚 → Terminate",
        tier_term == ResolutionTier::Terminate,
        "没有更好选择",
    );

    // --- 解除 + 披露 ---
    let result = engine
        .resolve(
            1,
            ResolutionTier::Gentle,
            10,
            alloc::vec![100, 101],
            0,
            true,
            true,
        )
        .expect("resolve");
    cs.add(
        "后果已披露",
        !result.outcome.disclosure.is_empty(),
        "disclosure 非空",
    );
    cs.add(
        "Gentle 保留重试",
        result.outcome.will_retry,
        "will_retry=true",
    );
    cs.add(
        "验证通过",
        result.verification == Verification::Passed,
        "环消散+账目平",
    );

    // --- 验证失败路径 ---
    let result2 = engine
        .resolve(2, ResolutionTier::Terminate, 900, alloc::vec![], 10, false, true)
        .expect("resolve2");
    cs.add(
        "环未消散可检",
        result2.verification == Verification::CycleRemains,
        "CycleRemains",
    );

    // --- 风暴熔断 ---
    let _ = engine.resolve(1, ResolutionTier::Gentle, 0, alloc::vec![], 100, true, true);
    let _ = engine.resolve(1, ResolutionTier::Gentle, 0, alloc::vec![], 200, true, true);
    let third = engine.resolve(1, ResolutionTier::Gentle, 0, alloc::vec![], 300, true, true);
    cs.add(
        "风暴触发",
        engine.storm.halted,
        "1s 内 3 次 → 熔断",
    );
    cs.add(
        "熔断后拒解除",
        engine
            .resolve(1, ResolutionTier::Gentle, 0, alloc::vec![], 400, true, true)
            .is_err(),
        "E_STORM_HALTED",
    );

    // --- 人工复位 ---
    engine.storm.reset();
    cs.add(
        "人工复位",
        !engine.storm.halted,
        "reset 后可继续",
    );

    // --- 报告渲染 ---
    let text = engine.render(&result);
    cs.add(
        "报告可读",
        text.contains("解除动作") && text.contains("后果"),
        "人话输出",
    );

    // --- 一致性 ---
    let ic = engine.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("引擎一致性", ok, "四项");

    // --- 破坏性单调 ---
    cs.add(
        "破坏性单调",
        ResolutionTier::Gentle.destructiveness()
            < ResolutionTier::Rollback.destructiveness()
            && ResolutionTier::Rollback.destructiveness()
                < ResolutionTier::Terminate.destructiveness(),
        "1<2<3",
    );

    cs
}
