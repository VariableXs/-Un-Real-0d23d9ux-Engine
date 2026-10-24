//! cgd07_earlysubmit — CGPU-F0488 帧预测提前提交（预测驱动的提前渲染）。
//!
//! 判据原文（施工书锚点 CGPU-F0488）：提前提交：下一帧提前量（预测驱动
//! 的提前渲染——与 C04 预测器联动预留，F0369 理念复用）；提前量自适应
//! （按负载稳定性动态调整——稳定场景提前多、抖动场景提前少）；预测失败
//! 回退（预测错误→回退同步提交——零窗口回退复用）；诚实标注（一期统计
//! 预测）。判据：联动预留、自适应、回退、诚实、判据。
//!
//! 职责定位：提前量控制器（稳定性滑窗 EWMA α=1/8 对齐 cgd06 采样纪律+
//! 滞回升降轨）+预测器联动预留（PredictorHook trait 接口冻结——C04 未接
//! 时 NoPredictor 桩恒回 EWMA）+偏差回退（|预测-实测|>带 → 零窗口回退同
//! 步提交——回退不冻结次帧照常评估）+诚实标注（每份提交计划强制携带统
//! 计口径声明——一期统计预测非保证）。与 cgd05（速率契约）分工——彼管
//! 帧率承诺、此管帧间提前量；与 cgd06（延迟账本）共享 50ms 目标线口径。
//! 诊断码段 0x4Fxx（五码独占）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const EARLY_SUBMIT_VERSION: &str = "D07-earlysubmit-v1";

/// 提前量上限（帧——一期封顶 2：提前两帧即“渲染好了等着呈现”）。
pub const MAX_LEAD: u32 = 2;

/// 升轨连续稳定帧门槛（滞回带——升难降易：稳定要连续 4 帧确认，抖动
/// 一帧即归零——防提前量在临界负载来回振荡）。
pub const PROMOTE_STREAK: u32 = 4;

/// 预测偏差带（µs——|预测-实测| 超此带即预测失败回退同步提交）。
pub const PREDICT_BAND_US: u32 = 12_000;

/// 稳定偏差带（µs——实测帧时延对 EWMA 的容差；与滞回带同一防线）。
pub const STABLE_BAND_US: u32 = 6_000;

/// 端到端目标线（µs——与 cgd06 TARGET_E2E_US 同口径 50ms：提前量的
/// 前提是帧时延本身达标，超线帧没有资格谈提前）。
pub const TARGET_FRAME_US: u32 = 50_000;

/// EWMA 平滑分母（α=1/8——与 cgd06 同款整数无损耗口径）。
pub const EWMA_DENOM: u32 = 8;

/// 诚实标注（判据原文「诚实标注（一期统计预测）」——每份提交计划
/// 强制携带，提前量是统计推断不是硬承诺）。
pub const HONEST_NOTE: &str = "一期统计预测：提前量基于历史负载统计推断，非保证";

// ---------------------------------------------------------------------------
// 诊断码（0x4Fxx 段独占——五码互异）
// ---------------------------------------------------------------------------

/// 诊断码（0x4Fxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EsCode {
    /// 0x4F01 提前量越界（超 MAX_LEAD——一期封顶 2）。
    EsLeadOver,
    /// 0x4F02 抖动未收敛仍加提前（稳定性闸失效——抖动帧只许归零）。
    EsJitterPromote,
    /// 0x4F03 回退未归零（回退计划 lead≠0——零窗口回退被破坏）。
    EsFallbackNonZero,
    /// 0x4F04 预测偏差超带未回退（回退闸失效——错预测还敢提前）。
    EsMissNoFallback,
    /// 0x4F05 诚实标注缺失（提交计划无统计口径声明）。
    EsNoteMissing,
}

impl EsCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            EsCode::EsLeadOver => "提前量越界：一期封顶 2 帧",
            EsCode::EsJitterPromote => "抖动未收敛仍加提前：稳定性闸失效",
            EsCode::EsFallbackNonZero => "回退未归零：零窗口回退被破坏",
            EsCode::EsMissNoFallback => "预测偏差超带未回退：回退闸失效",
            EsCode::EsNoteMissing => "诚实标注缺失：提交计划必须携带统计口径",
        }
    }

    /// 五码闭集（判据侧独立复核）。
    pub const ALL: [EsCode; 5] = [
        EsCode::EsLeadOver,
        EsCode::EsJitterPromote,
        EsCode::EsFallbackNonZero,
        EsCode::EsMissNoFallback,
        EsCode::EsNoteMissing,
    ];
}

// ---------------------------------------------------------------------------
// 稳定性追踪：EWMA 均值 + 偏差带判定（整数无损耗——α=1/8）
// ---------------------------------------------------------------------------

/// 负载稳定性判定（单帧粒度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    /// 实测时延在 EWMA±带内且 EWMA ≤ 目标线——可升轨。
    Stable,
    /// 偏差超带或均值超目标线——立即归零。
    Jittery,
}

/// 稳定性追踪器：EWMA 滚动均值 + 帧判定 + 连续稳定计数（升轨凭据）。
#[derive(Clone, Debug)]
pub struct StabilityTracker {
    /// EWMA 均值（µs，整数无损耗：`new = old + (sample-old)/8` 向零取整
    /// 余量并入——与 cgd06 同款推导）。
    pub ewma_us: u32,
    /// 已喂样本数（首样本直接锚定 EWMA）。
    pub samples: u32,
    /// 连续 Stable 帧数（升轨凭据——Jittery 即清零）。
    pub stable_streak: u32,
    /// Jittery 帧累计。
    pub jitter_frames: u32,
}

impl StabilityTracker {
    /// 空追踪器。
    pub fn new() -> Self {
        StabilityTracker { ewma_us: 0, samples: 0, stable_streak: 0, jitter_frames: 0 }
    }

    /// **喂一帧实测时延**并回判定（O(1)——锚点口径）。
    ///
    /// 手算口径：EWMA 首样本锚定；此后对称平滑 `ewma += (sample-ewma)/8`
    /// （i64 差值除法向零取整，负回落对称于正爬升——防抖后永久卡高位，
    /// 提前量必须可恢复）。Stable 条件双闸：偏差 |sample-ewma| ≤ 带且
    /// ewma ≤ 目标线——均值超线时即使单帧平稳也算抖动（超线帧没有
    /// 资格谈提前——50ms 目标线口径复用）。
    pub fn feed(&mut self, sample_us: u32) -> Stability {
        if self.samples == 0 {
            self.ewma_us = sample_us;
        } else {
            let delta = (sample_us as i64 - self.ewma_us as i64) / EWMA_DENOM as i64;
            let next = self.ewma_us as i64 + delta;
            self.ewma_us = if next < 0 { 0 } else { next as u32 };
        }
        self.samples += 1;
        let dev = if sample_us >= self.ewma_us {
            sample_us - self.ewma_us
        } else {
            self.ewma_us - sample_us
        };
        if dev <= STABLE_BAND_US && self.ewma_us <= TARGET_FRAME_US {
            self.stable_streak += 1;
            Stability::Stable
        } else {
            self.stable_streak = 0;
            self.jitter_frames += 1;
            Stability::Jittery
        }
    }
}

impl Default for StabilityTracker {
    fn default() -> Self {
        StabilityTracker::new()
    }
}

// ---------------------------------------------------------------------------
// 预测器联动预留：PredictorHook trait（C04 接口位冻结）
// ---------------------------------------------------------------------------

/// **预测器联动预留**（判据「与 C04 预测器联动预留」——接口位冻结，
/// C04 接入时实现本 trait 注入即可，控制器不改一行）。
///
/// 语义：对第 `frame_no` 帧给出负载预测（µs）。F0369 理念复用——预测
/// 只用于提前量决策，不用于 correctness 依赖（预测错了有回退兜底）。
pub trait PredictorHook {
    /// 预测第 `frame_no` 帧负载（µs）。
    fn predict(&mut self, frame_no: u64) -> u32;
    /// 预测器名（诚实标注联动——计划里记录是谁做的预测）。
    fn name(&self) -> &'static str;
}

/// NoPredictor 桩（C04 未接时缺省——恒回当前 EWMA，即「预测=历史均值」
/// 的保守统计面；此桩保证控制器零依赖可独立运行）。
#[derive(Clone, Debug, Default)]
pub struct NoPredictor {
    /// 桩返回值（通常接 StabilityTracker::ewma_us 快照）。
    pub ewma_snapshot: u32,
}

impl PredictorHook for NoPredictor {
    fn predict(&mut self, _frame_no: u64) -> u32 {
        self.ewma_snapshot
    }
    fn name(&self) -> &'static str {
        "no_predictor"
    }
}

// ---------------------------------------------------------------------------
// 提交计划：lead + 模式 + 诚实标注（判据四要素逐帧落账）
// ---------------------------------------------------------------------------

/// 提交模式（提前 or 回退同步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubmitMode {
    /// 提前提交（lead 帧——渲染好了等着呈现）。
    Early,
    /// 回退同步提交（零窗口——预测失败即回退复用）。
    SyncFallback,
}

/// 单帧提交计划（诚实标注强制携带——无标注计划构建即拒）。
#[derive(Clone, Debug)]
pub struct SubmitPlan {
    /// 帧号。
    pub frame_no: u64,
    /// 提前量（帧；0=同步提交）。
    pub lead: u32,
    /// 提交模式。
    pub mode: SubmitMode,
    /// 诚实标注（判据原文口径——恒为 [`HONEST_NOTE`]）。
    pub note: &'static str,
    /// 预测器名（联动留痕）。
    pub predictor: &'static str,
    /// 预测值（µs——回退审计面）。
    pub predicted_us: u32,
    /// 实测值（µs）。
    pub actual_us: u32,
}

impl SubmitPlan {
    /// 计划自检（构建后复核——越界/缺标注即诊断码）。
    pub fn audit(&self) -> Option<EsCode> {
        if self.lead > MAX_LEAD {
            return Some(EsCode::EsLeadOver);
        }
        if self.mode == SubmitMode::SyncFallback && self.lead != 0 {
            return Some(EsCode::EsFallbackNonZero);
        }
        if self.note != HONEST_NOTE || self.note.is_empty() {
            return Some(EsCode::EsNoteMissing);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 提前量控制器：升轨滞回 + 降轨即时 + 偏差回退（主流程编排）
// ---------------------------------------------------------------------------

/// 提前量控制器：稳定性滑窗 × 预测偏差双闸——自适应升降轨+失败回退。
#[derive(Clone, Debug)]
pub struct LeadController {
    /// 稳定性追踪器。
    pub tracker: StabilityTracker,
    /// 当前提前量。
    pub lead: u32,
    /// 升轨次数。
    pub promotions: u32,
    /// 降轨次数（抖动归零）。
    pub demotions: u32,
    /// 回退次数（预测失败）。
    pub fallbacks: u32,
    /// 计划总数。
    pub plans: u32,
    /// 历史最大提前量。
    pub max_lead_seen: u32,
    /// 上一帧是否回退（次帧恢复评估用——回退不冻结）。
    pub last_fallback: bool,
    /// 立案簿（诊断码逐条留痕）。
    pub cases: Vec<(u64, EsCode)>,
}

impl LeadController {
    /// 空控制器（lead=0 同步起步——没证据不提前）。
    pub fn new() -> Self {
        LeadController {
            tracker: StabilityTracker::new(),
            lead: 0,
            promotions: 0,
            demotions: 0,
            fallbacks: 0,
            plans: 0,
            max_lead_seen: 0,
            last_fallback: false,
            cases: Vec::new(),
        }
    }

    /// **规划下一帧提交**（主流程 O(1)——锚点口径）。
    ///
    /// 步序：① 稳定性判定（实测喂滑窗）；② 预测偏差闸（|预测-实测|>
    /// 带 → 回退：lead 归零+SyncFallback+计数，回退不冻结次帧照常评
    /// 估——零窗口回退复用）；③ 无回退时自适应升降轨：Stable 且连续
    /// 达 PROMOTE_STREAK → lead+1 封顶 MAX_LEAD（升轨滞回）；Jittery →
    /// lead 即时归零（降轨无滞回——抖动不等人）；④ 产出计划+诚实标
    /// 注+自检（audit 不通过即立案——防线自审）。
    pub fn plan_next(
        &mut self,
        frame_no: u64,
        actual_us: u32,
        predictor: &mut dyn PredictorHook,
    ) -> SubmitPlan {
        let stability = self.tracker.feed(actual_us);
        let predicted_us = predictor.predict(frame_no);
        self.plans += 1;
        let (mode, new_lead) = if predicted_us.abs_diff(actual_us) > PREDICT_BAND_US {
            // 预测偏差超带——零窗口回退（判据三）。
            self.fallbacks += 1;
            self.last_fallback = true;
            (SubmitMode::SyncFallback, 0)
        } else {
            self.last_fallback = false;
            match stability {
                Stability::Stable => {
                    if self.tracker.stable_streak >= PROMOTE_STREAK && self.lead < MAX_LEAD {
                        self.lead += 1;
                        self.promotions += 1;
                    }
                    if self.lead > 0 {
                        (SubmitMode::Early, self.lead)
                    } else {
                        (SubmitMode::SyncFallback, 0)
                    }
                }
                Stability::Jittery => {
                    if self.lead > 0 {
                        self.demotions += 1;
                    }
                    self.lead = 0;
                    (SubmitMode::SyncFallback, 0)
                }
            }
        };
        self.lead = new_lead;
        if self.lead > self.max_lead_seen {
            self.max_lead_seen = self.lead;
        }
        let plan = SubmitPlan {
            frame_no,
            lead: new_lead,
            mode,
            note: HONEST_NOTE,
            predictor: predictor.name(),
            predicted_us,
            actual_us,
        };
        // 防线自审：计划违约即立案（自审不通过的计划照发——但留痕，
        // 判据侧对拍立案簿）。
        if let Some(code) = plan.audit() {
            self.cases.push((frame_no, code));
        }
        plan
    }

    /// **偏差闸独立复核**（判据侧直接调用——回退闸单独可测）。
    pub fn miss_should_fallback(predicted_us: u32, actual_us: u32) -> bool {
        (predicted_us as i64 - actual_us as i64).abs() > PREDICT_BAND_US as i64
    }

    /// 立案数。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 一行摘要（读屏面——诚实标注同步输出）。
    pub fn summary(&self) -> String {
        alloc::format!(
            "D07 提前量: 当前={} 最大={} 升轨={} 降轨={} 回退={} 计划={} 立案={}",
            self.lead,
            self.max_lead_seen,
            self.promotions,
            self.demotions,
            self.fallbacks,
            self.plans,
            self.case_len()
        )
    }
}

impl Default for LeadController {
    fn default() -> Self {
        LeadController::new()
    }
}

// ---------------------------------------------------------------------------
// 静态一致性核验：策略常量自洽（防调参漂移——判据侧独立复核对拍）
// ---------------------------------------------------------------------------

/// **策略自洽核验**（判据「自适应」的静态面）：升轨门槛 ≤ 帧预算内
/// 可达成（4 帧确认在 50ms×4=200ms 内完成——提前量响应不过钝）；
/// 偏差带必须严于稳定带（预测错比负载抖更不可原谅——回退更敏感）；
/// 封顶非零（纯同步实现没有存在意义）。
pub fn policy_selfcheck() -> Option<EsCode> {
    if MAX_LEAD == 0 {
        return Some(EsCode::EsLeadOver);
    }
    if PREDICT_BAND_US <= STABLE_BAND_US {
        return Some(EsCode::EsMissNoFallback);
    }
    if PROMOTE_STREAK == 0 {
        return Some(EsCode::EsJitterPromote);
    }
    None
}
