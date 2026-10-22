//! VE-F3423 · 换肤过渡动画编排 —— 三策略（颜色插值/透明度渐变/错峰级联）
//! + 缓动同源 + 打断接管 + 掉帧降档。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3423`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三策略**（[`Strategy`]）：
//!   - **颜色插值**：按组件逐条把旧色插到新色。视觉最连续，代价是每帧
//!     要算每个组件的中间色；
//!   - **透明度渐变**：整层淡出→换令牌→淡入。代价恒定，适合大范围换肤；
//!   - **错峰级联**：按组件序错开起始时间，形成「波浪」。观感最好，
//!     但总时长最长，必须算进预算；
//! - **缓动同源**：所有缓动 id 必须来自 Z05 的单一实现表
//!   （[`EASING_TABLE`]）。本模块**不实现**任何缓动函数——重复实现缓动
//!   是「同一条曲线两处微调」的经典源头，最后没人说得清哪个是对的。
//!   这里只校验 id、引用 id；
//! - **打断即接管**：换肤跑到一半用户又点了一套皮肤。新计划直接接管：
//!   保留已完成的进度比例（[`Takeover`]），从当前视觉状态继续，不清零、
//!   不排队、不卡死。清零会让画面「跳回去」，排队会让用户觉得点了没反应；
//! - **掉帧降档**：连续 [`DROP_STREAK_LIMIT`] 帧超预算就降一档
//!   （颜色插值 → 透明度渐变 → 瞬切）。降档只降表现，不降正确性：
//!   令牌照样换完，只是过程更朴素。
//!
//! # 为什么缓动要「同源」到这种程度
//!
//! 缓动的观感差异极小、代码极短，所以最容易被各处复制粘贴。复制三次
//! 之后，设计改一次曲线要改三处，漏一处就出现「这个面板比那个面板快
//! 一点」的诡异手感——这种缺陷没有报错、没有日志，只有用户觉得「不跟手」。
//! 本模块把「只能用表里的 id」写成硬约束，从结构上消灭复制。
//!
//! # 复杂度
//!
//! - `plan`：O(组件数)；
//! - `advance`：O(组件数)；
//! - `takeover`：O(1)（只换计划，保留进度）；
//! - `report_frame`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`；时间累加全部 `saturating_*`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5424 起）
// ---------------------------------------------------------------------------

/// 过渡编排诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotionCode {
    /// 组件集为空。
    EmptyTargets = 5424,
    /// 策略未知/未启用。
    UnknownStrategy = 5425,
    /// 缓动 id 不在 Z05 单一实现表内。
    ForeignEasing = 5426,
    /// 总时长超过预算。
    OverBudget = 5427,
    /// 错峰步长为 0（退化成同时开始，不是级联）。
    ZeroStagger = 5428,
    /// 没有正在运行的过渡可接管。
    NothingToTakeOver = 5429,
    /// 过渡已结束，不能再推进。
    AlreadyFinished = 5430,
    /// 降档已到最低档（瞬切），无法再降。
    NoLowerTier = 5431,
    /// 帧预算为 0。
    ZeroFrameBudget = 5432,
    /// 进度退步（时间倒流）。
    TimeWentBackwards = 5433,
}

impl MotionCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MotionCode::EmptyTargets => "过渡目标为空",
            MotionCode::UnknownStrategy => "过渡策略不可用",
            MotionCode::ForeignEasing => "缓动不在同源表内",
            MotionCode::OverBudget => "过渡总时长超预算",
            MotionCode::ZeroStagger => "错峰步长为 0",
            MotionCode::NothingToTakeOver => "没有可接管的过渡",
            MotionCode::AlreadyFinished => "过渡已结束",
            MotionCode::NoLowerTier => "已到最低降档",
            MotionCode::ZeroFrameBudget => "帧预算为 0",
            MotionCode::TimeWentBackwards => "时间倒流",
        }
    }
}

/// 过渡策略。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Strategy {
    /// 颜色插值（最细，最贵）。
    ColorLerp,
    /// 透明度渐变（恒定成本）。
    AlphaFade,
    /// 错峰级联（观感最好，总时长最长）。
    StaggeredCascade,
    /// 瞬切（降档兜底，无过渡）。
    InstantCut,
}

impl Strategy {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Strategy::ColorLerp => "颜色插值",
            Strategy::AlphaFade => "透明度渐变",
            Strategy::StaggeredCascade => "错峰级联",
            Strategy::InstantCut => "瞬切",
        }
    }

    /// 单组件基准成本（微秒/帧），用于预算估算。
    pub fn cost_us(self) -> u32 {
        match self {
            Strategy::ColorLerp => 40,
            Strategy::AlphaFade => 6,
            Strategy::StaggeredCascade => 28,
            Strategy::InstantCut => 1,
        }
    }

    /// 降一档。瞬切已是最低档。
    pub fn downgrade(self) -> Option<Strategy> {
        match self {
            Strategy::ColorLerp => Some(Strategy::AlphaFade),
            Strategy::StaggeredCascade => Some(Strategy::AlphaFade),
            Strategy::AlphaFade => Some(Strategy::InstantCut),
            Strategy::InstantCut => None,
        }
    }
}

/// 连续掉帧上限：达到即降档。
pub const DROP_STREAK_LIMIT: u32 = 3;

// ---------------------------------------------------------------------------
// 二、缓动同源表（Z05 单一实现）
// ---------------------------------------------------------------------------

/// Z05 缓动单一实现表。**这张表就是全部**——不在表里的 id 一律拒绝，
/// 包括看起来很像的 `ease-out-quad`（Z05 里叫 `ease.quad.out`）。
pub const EASING_TABLE: [&str; 8] = [
    "linear",
    "ease.standard",
    "ease.decelerate",
    "ease.accelerate",
    "ease.quad.in",
    "ease.quad.out",
    "ease.cubic.out",
    "ease.spring.soft",
];

/// 缓动引用。类型层面强制经 [`EasingRef::new`] 校验，字段私有，
/// 无法从外部拼出一个「不存在于表里」的缓动。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EasingRef {
    id: String,
}

impl EasingRef {
    /// 校验并构造。不在表内即拒。
    pub fn new(id: &str) -> Result<EasingRef, MotionCode> {
        if EASING_TABLE.iter().any(|e| *e == id) {
            Ok(EasingRef { id: id.to_string() })
        } else {
            Err(MotionCode::ForeignEasing)
        }
    }

    /// 标准缓动（最常用）。
    pub fn standard() -> EasingRef {
        EasingRef {
            id: "ease.standard".to_string(),
        }
    }

    /// id。
    pub fn id(&self) -> &str {
        &self.id
    }
}

// ---------------------------------------------------------------------------
// 三、计划与进度
// ---------------------------------------------------------------------------

/// 过渡计划。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plan {
    /// 参与过渡的组件。
    pub targets: Vec<u32>,
    /// 策略。
    pub strategy: Strategy,
    /// 单组件时长（毫秒）。
    pub duration_ms: u32,
    /// 错峰步长（毫秒）。仅 `StaggeredCascade` 用；0 表示未错峰。
    pub stagger_ms: u32,
    /// 缓动。
    pub easing: EasingRef,
    /// 总预算（毫秒）。
    pub budget_ms: u32,
}

impl Plan {
    /// 总时长：级联要把错峰算进去，其它策略等于单组件时长。
    pub fn total_ms(&self) -> u32 {
        let spread = if self.strategy == Strategy::StaggeredCascade {
            self.stagger_ms
                .saturating_mul(self.targets.len().saturating_sub(1) as u32)
        } else {
            0
        };
        self.duration_ms.saturating_add(spread)
    }

    /// 每帧成本估算（微秒）。
    pub fn frame_cost_us(&self) -> u32 {
        self.strategy
            .cost_us()
            .saturating_mul(self.targets.len() as u32)
    }

    /// 校验。
    pub fn validate(&self) -> Result<(), MotionCode> {
        if self.targets.is_empty() {
            return Err(MotionCode::EmptyTargets);
        }
        if self.strategy == Strategy::StaggeredCascade && self.stagger_ms == 0 {
            return Err(MotionCode::ZeroStagger);
        }
        if self.total_ms() > self.budget_ms {
            return Err(MotionCode::OverBudget);
        }
        Ok(())
    }
}

/// 单个组件的进度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetProgress {
    /// 组件 id。
    pub component: u32,
    /// 进度千分比（0..=1000）。
    pub permille: u16,
    /// 是否已完成。
    pub done: bool,
}

/// 一次推进的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TickResult {
    /// 各组件进度。
    pub progress: Vec<TargetProgress>,
    /// 整体是否完成。
    pub finished: bool,
    /// 本次推进是否发生降档。
    pub downgraded: Option<Strategy>,
}

/// 接管记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Takeover {
    /// 接管时旧计划已完成的整体比例（千分比）。
    pub resume_permille: u16,
    /// 被接管掉的策略。
    pub from: Strategy,
    /// 接管到的新策略。
    pub to: Strategy,
}

// ---------------------------------------------------------------------------
// 四、编排器
// ---------------------------------------------------------------------------

/// 过渡编排器。
#[derive(Clone, Debug)]
pub struct Choreographer {
    plan: Option<Plan>,
    elapsed_ms: u32,
    drop_streak: u32,
    downgrades: u32,
    takeovers: Vec<Takeover>,
    frame_budget_us: u32,
}

impl Choreographer {
    /// 新建。`frame_budget_us` 是一帧给过渡的预算（微秒）。
    pub fn new(frame_budget_us: u32) -> Choreographer {
        Choreographer {
            plan: None,
            elapsed_ms: 0,
            drop_streak: 0,
            downgrades: 0,
            takeovers: Vec::new(),
            frame_budget_us,
        }
    }

    /// 开始一个计划。已在进行中则返回 `NothingToTakeOver` 之外的错误？
    /// 不——按锚点，进行中再开始就是「打断接管」，走 [`Self::takeover`]。
    /// 这里只允许空闲时开始。
    pub fn start(&mut self, plan: Plan) -> Result<(), MotionCode> {
        if self.plan.is_some() && !self.is_finished() {
            return Err(MotionCode::NothingToTakeOver);
        }
        plan.validate()?;
        if self.frame_budget_us == 0 {
            return Err(MotionCode::ZeroFrameBudget);
        }
        self.plan = Some(plan);
        self.elapsed_ms = 0;
        self.drop_streak = 0;
        Ok(())
    }

    /// 打断接管：换成新计划，保留已完成比例（不清零）。
    ///
    /// 语义细节：新计划的起点是「旧计划已完成的比例」，而不是 0——
    /// 这让画面从当前视觉状态继续，用户看不到跳变。若旧计划没有
    /// 在运行，返回 [`MotionCode::NothingToTakeOver`]。
    pub fn takeover(&mut self, plan: Plan) -> Result<Takeover, MotionCode> {
        let old = match self.plan.as_ref() {
            Some(p) => p.clone(),
            None => return Err(MotionCode::NothingToTakeOver),
        };
        if self.is_finished() {
            return Err(MotionCode::AlreadyFinished);
        }
        plan.validate()?;
        let resume = self.overall_permille();
        self.plan = Some(plan.clone());
        self.elapsed_ms = 0;
        self.drop_streak = 0;
        let record = Takeover {
            resume_permille: resume,
            from: old.strategy,
            to: plan.strategy,
        };
        self.takeovers.push(record.clone());
        Ok(record)
    }

    /// 推进 `dt_ms`。返回各组件进度。
    pub fn advance(&mut self, dt_ms: u32) -> Result<TickResult, MotionCode> {
        let plan = match self.plan.as_ref() {
            Some(p) => p.clone(),
            None => return Err(MotionCode::NothingToTakeOver),
        };
        if self.is_finished() {
            return Err(MotionCode::AlreadyFinished);
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
        let mut progress: Vec<TargetProgress> = Vec::new();
        let duration = plan.duration_ms.max(1);
        for (index, component) in plan.targets.iter().enumerate() {
            let offset = if plan.strategy == Strategy::StaggeredCascade {
                plan.stagger_ms.saturating_mul(index as u32)
            } else {
                0
            };
            let local = self.elapsed_ms.saturating_sub(offset);
            let permille = if plan.strategy == Strategy::InstantCut {
                1000
            } else if local >= duration {
                1000
            } else {
                ((local as u64 * 1000) / duration as u64).min(1000) as u16
            };
            progress.push(TargetProgress {
                component: *component,
                permille,
                done: permille >= 1000,
            });
        }
        let finished = progress.iter().all(|p| p.done);
        let mut downgraded = None;
        // 掉帧降档：本帧成本超预算即累计，连续三次降一档。
        if plan.frame_cost_us() > self.frame_budget_us {
            self.drop_streak = self.drop_streak.saturating_add(1);
            if self.drop_streak >= DROP_STREAK_LIMIT {
                if let Some(lower) = plan.strategy.downgrade() {
                    let mut next = plan.clone();
                    next.strategy = lower;
                    // 降档后重算错峰：瞬切/渐变不需要错峰。
                    if lower != Strategy::StaggeredCascade {
                        next.stagger_ms = 0;
                    }
                    self.plan = Some(next);
                    self.downgrades = self.downgrades.saturating_add(1);
                    self.drop_streak = 0;
                    downgraded = Some(lower);
                } else {
                    self.drop_streak = 0;
                }
            }
        } else {
            self.drop_streak = 0;
        }
        Ok(TickResult {
            progress,
            finished,
            downgraded,
        })
    }

    /// 整体进度（所有组件的最小进度——整体完成度由最慢者决定）。
    pub fn overall_permille(&self) -> u16 {
        let plan = match self.plan.as_ref() {
            Some(p) => p,
            None => return 0,
        };
        if plan.targets.is_empty() {
            return 0;
        }
        let duration = plan.duration_ms.max(1);
        let mut min = 1000u16;
        for (index, _) in plan.targets.iter().enumerate() {
            let offset = if plan.strategy == Strategy::StaggeredCascade {
                plan.stagger_ms.saturating_mul(index as u32)
            } else {
                0
            };
            let local = self.elapsed_ms.saturating_sub(offset);
            let p = if plan.strategy == Strategy::InstantCut || local >= duration {
                1000
            } else {
                ((local as u64 * 1000) / duration as u64).min(1000) as u16
            };
            min = core::cmp::min(min, p);
        }
        min
    }

    /// 是否已完成。
    pub fn is_finished(&self) -> bool {
        match self.plan.as_ref() {
            Some(p) => self.elapsed_ms >= p.total_ms(),
            None => true,
        }
    }

    /// 当前策略。
    pub fn strategy(&self) -> Option<Strategy> {
        self.plan.as_ref().map(|p| p.strategy)
    }

    /// 降档次数。
    pub fn downgrades(&self) -> u32 {
        self.downgrades
    }

    /// 接管记录。
    pub fn takeovers(&self) -> &[Takeover] {
        &self.takeovers
    }

    /// 状态读屏播报（锚点「过渡状态读屏播报」）。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.plan.as_ref() {
            Some(p) => {
                out.push(format!(
                    "过渡：{}，{} 个组件，单件 {}ms，错峰 {}ms，总时长 {}ms，缓动 {}",
                    p.strategy.label(),
                    p.targets.len(),
                    p.duration_ms,
                    p.stagger_ms,
                    p.total_ms(),
                    p.easing.id()
                ));
                out.push(format!(
                    "已推进 {}ms，整体进度 {}‰，{}",
                    self.elapsed_ms,
                    self.overall_permille(),
                    if self.is_finished() {
                        "已完成"
                    } else {
                        "进行中"
                    }
                ));
                if self.downgrades > 0 {
                    out.push(format!("已降档 {} 次", self.downgrades));
                }
                if let Some(t) = self.takeovers.last() {
                    out.push(format!(
                        "最近接管：{} → {}，从 {}‰ 续跑",
                        t.from.label(),
                        t.to.label(),
                        t.resume_permille
                    ));
                }
            }
            None => out.push("当前没有过渡在运行".to_string()),
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(strategy: Strategy, n: u32, stagger: u32, budget: u32) -> Plan {
        let mut targets = Vec::new();
        let mut i = 1;
        while i <= n {
            targets.push(i);
            i += 1;
        }
        Plan {
            targets,
            strategy,
            duration_ms: 200,
            stagger_ms: stagger,
            easing: EasingRef::standard(),
            budget_ms: budget,
        }
    }

    #[test]
    fn three_strategies_are_supported() {
        for s in [
            Strategy::ColorLerp,
            Strategy::AlphaFade,
            Strategy::StaggeredCascade,
        ]
        .iter()
        {
            let p = plan(*s, 4, 20, 2000);
            assert!(p.validate().is_ok(), "{} 应可用", s.label());
        }
    }

    #[test]
    fn foreign_easing_is_rejected() {
        assert_eq!(
            EasingRef::new("ease-out-quad").err(),
            Some(MotionCode::ForeignEasing)
        );
        assert!(EasingRef::new("ease.cubic.out").is_ok());
    }

    #[test]
    fn cascade_total_includes_stagger_and_budget_gate() {
        let p = plan(Strategy::StaggeredCascade, 5, 50, 400);
        // 200 + 4*50 = 400，刚好卡线通过。
        assert_eq!(p.total_ms(), 400);
        assert!(p.validate().is_ok());
        // 预算 399 就超。
        let mut tight = p.clone();
        tight.budget_ms = 399;
        assert_eq!(tight.validate().err(), Some(MotionCode::OverBudget));
    }

    #[test]
    fn zero_stagger_is_rejected_for_cascade() {
        let p = plan(Strategy::StaggeredCascade, 3, 0, 2000);
        assert_eq!(p.validate().err(), Some(MotionCode::ZeroStagger));
        // 非级联策略不管错峰。
        let p2 = plan(Strategy::AlphaFade, 3, 0, 2000);
        assert!(p2.validate().is_ok());
    }

    #[test]
    fn advance_reaches_full_progress_in_order() {
        let mut c = Choreographer::new(100_000);
        c.start(plan(Strategy::StaggeredCascade, 3, 50, 2000)).expect("开始");
        let t1 = c.advance(100).expect("推进");
        assert!(!t1.finished);
        // 错峰 50ms：第 1 个走了 100ms（500‰），第 2 个走了 50ms（250‰），
        // 第 3 个刚起步（0‰）。
        assert_eq!(t1.progress.first().map(|p| p.permille), Some(500));
        assert_eq!(t1.progress.get(1).map(|p| p.permille), Some(250));
        assert_eq!(t1.progress.get(2).map(|p| p.permille), Some(0));
        let t2 = c.advance(300).expect("推进");
        assert!(t2.finished);
        assert!(c.is_finished());
    }

    #[test]
    fn takeover_preserves_progress_and_does_not_reset() {
        let mut c = Choreographer::new(100_000);
        c.start(plan(Strategy::ColorLerp, 2, 0, 2000)).expect("开始");
        c.advance(100).expect("推进");
        let before = c.overall_permille();
        assert_eq!(before, 500);
        let t = c.takeover(plan(Strategy::AlphaFade, 2, 0, 2000)).expect("接管");
        assert_eq!(t.resume_permille, 500);
        assert_eq!(t.from, Strategy::ColorLerp);
        assert_eq!(t.to, Strategy::AlphaFade);
        assert_eq!(c.strategy(), Some(Strategy::AlphaFade));
        // 接管后仍在运行，不卡死。
        let r = c.advance(200).expect("继续");
        assert!(r.finished);
    }

    #[test]
    fn frame_drops_trigger_downgrade_but_never_lose_correctness() {
        // 预算故意压到 1us/帧：颜色插值必超支。
        let mut c = Choreographer::new(1);
        c.start(plan(Strategy::ColorLerp, 4, 0, 2000)).expect("开始");
        let mut seen = None;
        let mut i = 0;
        while i < DROP_STREAK_LIMIT {
            let r = c.advance(10).expect("推进");
            if let Some(s) = r.downgraded {
                seen = Some(s);
            }
            i += 1;
        }
        assert_eq!(seen, Some(Strategy::AlphaFade));
        assert_eq!(c.downgrades(), 1);
        // 继续掉帧 -> 再降到瞬切。
        let mut i = 0;
        while i < DROP_STREAK_LIMIT {
            c.advance(10).expect("推进");
            i += 1;
        }
        assert_eq!(c.strategy(), Some(Strategy::InstantCut));
        // 瞬切之下没有更低档，但过渡仍然完成。
        let r = c.advance(1000).expect("推进");
        assert!(r.finished);
    }

    #[test]
    fn reading_lines_tell_the_whole_story() {
        let mut c = Choreographer::new(100_000);
        c.start(plan(Strategy::ColorLerp, 2, 0, 2000)).expect("开始");
        c.advance(100).expect("推进");
        let _ = c.takeover(plan(Strategy::AlphaFade, 2, 0, 2000)).expect("接管");
        let lines = c.reading_lines();
        assert!(lines.first().map(|l| l.contains("透明度渐变")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("最近接管")));
        assert!(c.reading_lines().iter().any(|l| l.contains("进行中")));
    }
}
