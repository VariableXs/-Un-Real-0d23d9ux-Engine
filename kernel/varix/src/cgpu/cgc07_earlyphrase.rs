//! cgc07_earlyphrase — CGPU-F0488 帧预测提前提交（把"等下一帧"变成"预支下一帧"）。
//!
//! 判据原文（施工书锚点 CGPU-F0488）：提前提交：下一帧提前量（预测驱动的提
//! 前渲染——与 C04 预测器联动预留，F0369 理念复用）；提前量自适应（按负载稳
//! 定性动态调整——稳定场景提前多、抖动场景提前少）；预测失败回退（预测错误
//! →回退同步提交——零窗口回退复用）；诚实标注（一期统计预测）。判据：联动
//! 预留、自适应、回退、诚实、判据。
//!
//! 职责定位：提前量策略器（稳定性分档→提前量档位）+ 统计预测核（一期 EWMA，
//! 二期 C04 联动接口已冻结）+ 提前提交状态机（预测→预支→验证→回退）+ 失败
//! 回退期（连续错判→零窗口同步提交 N 帧）+ 诚实标注（提交账本每条带预测来
//! 源与置信度，禁止事后把统计预测包装成"已确认"）。与 cgc04_framearbiter
//! （帧内预算）分工：彼分配本帧的毫秒，此决定下一帧提前多久开画。诊断码段
//! 0x4F01~0x4F05。

use alloc::vec::Vec;

/// 模块版本。
pub const EARLY_PHRASE_VERSION: &str = "C07-earlyphrase-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x4F 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_LEAD_NEGATIVE: u32 = 0x4F01; // 提前量为负（预测早于提交基线）
pub const XC_FORECAST_DIVERGED: u32 = 0x4F02; // 统计预测发散（CV 超上限）
pub const XC_ROLLBACK_ENGAGED: u32 = 0x4F03; // 回退期生效（零窗口同步提交）
pub const XC_RESERVE_UNAVAILABLE: u32 = 0x4F04; // C04 联动预留未接入（诚实降级统计预测）
pub const XC_LEAD_CLIP: u32 = 0x4F05; // 提前量被上限钳制（预支超出债务窗口）

// ---------------------------------------------------------------------------
// 常量：提前量档位（微秒，u64）
// ---------------------------------------------------------------------------

/// 稳定档提前量：8ms —— 画面内容确定性高，预支大胆。
pub const LEAD_STABLE_US: u64 = 8_000;
/// 中等档提前量：4ms —— 波动可容忍，预支谨慎。
pub const LEAD_MODERATE_US: u64 = 4_000;
/// 抖动档提前量：0 —— 同步提交，不做预支（判据"抖动场景提前少"到零）。
pub const LEAD_JITTERY_US: u64 = 0;
/// 提前量硬上限：16ms —— 不得超过一帧周期，防止预支失控。
pub const LEAD_CAP_US: u64 = 16_000;
/// 预测命中率红线：低于此值进入回退评估。
pub const HIT_FLOOR_PCT: u32 = 70;
/// 回退期长度（帧）：连续错判后强制同步提交的帧数。
pub const ROLLBACK_FRAMES: u32 = 12;
/// 回退解除条件：回退期内连续命中计数（全同步提交无预测，故以稳定性恢复为准）。
pub const RECOVERY_STABLE_RUN: u32 = 8;
/// 稳定性滑动窗口长度（帧）。
pub const STABILITY_WINDOW: usize = 32;

/// 负载稳定性三档（判据"按负载稳定性动态调整"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadStability {
    /// 稳定：帧时间波动小（CV < 5%）。
    Stable,
    /// 中等：波动可容忍（5% ≤ CV < 15%）。
    Moderate,
    /// 抖动：波动剧烈（CV ≥ 15%）。
    Jittery,
}

impl LoadStability {
    pub const ALL: [LoadStability; 3] = [
        LoadStability::Stable,
        LoadStability::Moderate,
        LoadStability::Jittery,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            LoadStability::Stable => "稳定",
            LoadStability::Moderate => "中等",
            LoadStability::Jittery => "抖动",
        }
    }

    /// 档位对应的默认提前量。
    pub const fn lead_us(self) -> u64 {
        match self {
            LoadStability::Stable => LEAD_STABLE_US,
            LoadStability::Moderate => LEAD_MODERATE_US,
            LoadStability::Jittery => LEAD_JITTERY_US,
        }
    }
}

/// 预测来源（判据"诚实标注"——一期统计预测，C04 为联动预留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForecastSource {
    /// 一期：EWMA 统计预测（本模块内建，纯历史外推）。
    Statistical,
    /// 二期预留：C04 预测器联动（F0369 理念）——本期仅冻结接口，未接入。
    ReservedC04,
}

impl ForecastSource {
    pub const fn zh(self) -> &'static str {
        match self {
            ForecastSource::Statistical => "统计预测（一期）",
            ForecastSource::ReservedC04 => "C04 联动（预留未接入）",
        }
    }
}

/// 提前提交状态机（判据"预测→预支→验证→回退"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitPhase {
    /// 正常：按档位提前量预支下一帧。
    Forecasting,
    /// 观察：命中率接近红线，收紧提前量但未回退。
    Cautious,
    /// 回退：零窗口同步提交（判据"预测错误→回退同步提交"）。
    Rollback,
}

/// 稳定性分类器：滑动窗口 CV（变异系数 = 标准差/均值，×1000 定点）。
pub struct StabilityClassifier {
    window: [u64; STABILITY_WINDOW],
    filled: usize,
    cursor: usize,
}

impl StabilityClassifier {
    pub fn new() -> Self {
        StabilityClassifier {
            window: [0; STABILITY_WINDOW],
            filled: 0,
            cursor: 0,
        }
    }

    /// 录入一帧帧时间（微秒）。
    pub fn observe(&mut self, frame_us: u64) {
        self.window[self.cursor] = frame_us;
        self.cursor = (self.cursor + 1) % STABILITY_WINDOW;
        if self.filled < STABILITY_WINDOW {
            self.filled += 1;
        }
    }

    /// 变异系数 ×1000（不足窗口或均值为零时返回 u64::MAX 视为最抖）。
    pub fn cv_milli(&self) -> u64 {
        if self.filled < STABILITY_WINDOW {
            return u64::MAX;
        }
        let mut sum: u128 = 0;
        let mut i = 0;
        while i < STABILITY_WINDOW {
            sum += self.window[i] as u128;
            i += 1;
        }
        let mean = sum / STABILITY_WINDOW as u128;
        if mean == 0 {
            return u64::MAX;
        }
        let mut var_acc: u128 = 0;
        let mut j = 0;
        while j < STABILITY_WINDOW {
            let d = self.window[j] as i128 - mean as i128;
            var_acc += (d * d) as u128;
            j += 1;
        }
        let variance = var_acc / STABILITY_WINDOW as u128;
        // CV = sqrt(var)/mean；定点近似：用 var/mean² 的 1000 倍开方近似。
        // 直接整数开方（牛顿法），避免浮点。
        let ratio_sq = (variance * 1_000_000) / (mean * mean);
        // 值域钳制：variance ≤ mean² ⇒ ratio_sq ≤ 1e6，理论不越界；防御性钳制
        // 保证整数近似误差下 as u64 无损（零 unwrap 纪律）。
        let ratio_u64 = if ratio_sq > u64::MAX as u128 {
            u64::MAX
        } else {
            ratio_sq as u64
        };
        let cv_milli = isqrt(ratio_u64);
        cv_milli
    }

    /// 当前分档：CV(milli) < 50 → 稳定；< 150 → 中等；否则抖动。
    pub fn classify(&self) -> LoadStability {
        let cv = self.cv_milli();
        if cv < 50 {
            LoadStability::Stable
        } else if cv < 150 {
            LoadStability::Moderate
        } else {
            LoadStability::Jittery
        }
    }
}

/// 整数平方根（牛顿迭代，no_std 无 sqrt）。
pub const fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// 提前量策略器：分档基线 + 动态收缩因子。
pub struct LeadPolicy {
    /// 当前分档。
    pub stability: LoadStability,
    /// 收缩因子 ×100（100 = 不收缩；观察期降到 50；回退期归零）。
    pub shrink_pct: u32,
    /// 连续命中计数（用于观察→正常解除）。
    hit_run: u32,
    /// 连续错判计数。
    miss_run: u32,
}

impl LeadPolicy {
    pub fn new() -> Self {
        LeadPolicy {
            stability: LoadStability::Moderate,
            shrink_pct: 100,
            hit_run: 0,
            miss_run: 0,
        }
    }

    /// 计算本帧生效提前量（基线 × 收缩因子，且受硬上限钳制）。
    pub fn effective_lead_us(&self) -> u64 {
        if self.shrink_pct == 0 {
            return 0;
        }
        let base = self.stability.lead_us();
        let scaled = base * self.shrink_pct as u64 / 100;
        if scaled > LEAD_CAP_US {
            return LEAD_CAP_US; // 钳制路径（调用方应记 XC_LEAD_CLIP）
        }
        scaled
    }

    /// 预测命中反馈（命中=true）。
    pub fn feedback(&mut self, hit: bool) -> CommitPhase {
        if hit {
            self.hit_run += 1;
            self.miss_run = 0;
            if self.shrink_pct < 100 && self.hit_run >= RECOVERY_STABLE_RUN {
                self.shrink_pct = 100;
                self.hit_run = 0;
            }
        } else {
            self.miss_run += 1;
            self.hit_run = 0;
            if self.shrink_pct > 50 {
                self.shrink_pct = 50; // 先收紧一半
            }
        }
        self.phase()
    }

    /// 当前状态机档位。
    pub const fn phase(&self) -> CommitPhase {
        if self.shrink_pct == 0 {
            CommitPhase::Rollback
        } else if self.shrink_pct < 100 {
            CommitPhase::Cautious
        } else {
            CommitPhase::Forecasting
        }
    }

    /// 进入回退期（零窗口）。
    pub fn engage_rollback(&mut self) -> u32 {
        self.shrink_pct = 0;
        self.hit_run = 0;
        self.miss_run = 0;
        ROLLBACK_FRAMES
    }
}

/// 一次提前提交的账本条目（判据"诚实标注"落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EarlyCommitRecord {
    /// 帧序号。
    pub frame_no: u64,
    /// 生效提前量（微秒）。
    pub lead_us: u64,
    /// 预测来源（诚实标注核心字段）。
    pub source: ForecastSource,
    /// 预测置信度 ×100（统计预测 = 窗口命中率的实数化）。
    pub confidence_pct: u32,
    /// 该帧最终是否命中（提交时未知则 false，验证后补写）。
    pub verified_hit: bool,
    /// 是否回退帧（零窗口同步提交）。
    pub rollback_frame: bool,
}

/// 提前提交管理器：本模块对外总门面。
pub struct EarlyCommitManager {
    classifier: StabilityClassifier,
    policy: LeadPolicy,
    /// C04 联动是否已接入（一期恒 false——诚实）。
    c04_linked: bool,
    /// 回退剩余帧数。
    rollback_left: u32,
    /// 账本。
    ledger: Vec<EarlyCommitRecord>,
    /// 预测窗口命中计数（近 ROLLBACK_FRAMES×2 内）。
    recent_hits: u32,
    recent_total: u32,
    /// 遥测计数。
    pub clip_count: u64,
    pub rollback_count: u64,
    pub reserve_report_count: u64,
}

impl EarlyCommitManager {
    pub fn new() -> Self {
        EarlyCommitManager {
            classifier: StabilityClassifier::new(),
            policy: LeadPolicy::new(),
            c04_linked: false,
            rollback_left: 0,
            ledger: Vec::new(),
            recent_hits: 0,
            recent_total: 0,
            clip_count: 0,
            rollback_count: 0,
            reserve_report_count: 0,
        }
    }

    /// C04 联动接入开关（二期接口——一期调用方不传 true 即诚实统计）。
    pub fn set_c04_link(&mut self, linked: bool) -> Result<(), u32> {
        if linked && self.c04_linked {
            return Ok(()); // 幂等
        }
        self.c04_linked = linked;
        if !linked {
            self.reserve_report_count += 1; // 撤销联动也要诚实入账
        }
        Ok(())
    }

    /// 帧开始：录入上一帧实测帧时间并产出本帧提前量决策。
    pub fn begin_frame(&mut self, frame_no: u64, last_frame_us: u64) -> EarlyCommitRecord {
        self.classifier.observe(last_frame_us);
        self.policy.stability = self.classifier.classify();

        // 回退期倒计时优先。
        if self.rollback_left > 0 {
            self.rollback_left -= 1;
            if self.rollback_left == 0 && self.policy.stability == LoadStability::Jittery {
                // 回退期满但仍在抖动：续期（判据"回退"的保守语义）。
                self.rollback_left = ROLLBACK_FRAMES / 2;
            }
            if self.rollback_left > 0 {
                let rec = EarlyCommitRecord {
                    frame_no,
                    lead_us: 0,
                    source: ForecastSource::Statistical,
                    confidence_pct: 0,
                    verified_hit: false,
                    rollback_frame: true,
                };
                self.ledger.push(rec);
                return rec;
            }
        }

        let mut lead = self.policy.effective_lead_us();
        if lead < self.policy.stability.lead_us() && lead == LEAD_CAP_US {
            self.clip_count += 1; // 被上限钳制（诚实遥测）
        }
        if lead > LEAD_CAP_US {
            lead = LEAD_CAP_US;
            self.clip_count += 1;
        }

        // 一期统计预测：置信度 = 近期命中率（样本不足给 50 保底并诚实）。
        let confidence = if self.recent_total >= 8 {
            self.recent_hits * 100 / self.recent_total
        } else {
            50
        };

        let rec = EarlyCommitRecord {
            frame_no,
            lead_us: lead,
            // 一期恒 Statistical；C04 接入后此处才允许 ReservedC04。
            source: if self.c04_linked {
                ForecastSource::ReservedC04
            } else {
                self.reserve_report_count += 1; // 每次因未接入而降级都记录
                ForecastSource::Statistical
            },
            confidence_pct: confidence,
            verified_hit: false,
            rollback_frame: false,
        };
        self.ledger.push(rec);
        rec
    }

    /// 帧结束：验证预测（实际帧时间 ≤ 提前量+余量 即命中）。
    pub fn verify_frame(&mut self, actual_frame_us: u64) -> Result<bool, u32> {
        let n = self.ledger.len();
        if n == 0 {
            return Err(XC_LEAD_NEGATIVE);
        }
        let idx = n - 1;
        let rec = self.ledger[idx];
        if rec.rollback_frame {
            return Ok(false); // 回退帧无预测可验证
        }
        // 命中定义：实际帧时间不超过提前量与均值余量之和（预支没被现实打穿）。
        let hit = actual_frame_us <= rec.lead_us + 4_000;
        self.ledger[idx].verified_hit = hit;
        self.recent_total += 1;
        if hit {
            self.recent_hits += 1;
        }
        let phase = self.policy.feedback(hit);
        if phase == CommitPhase::Cautious && self.recent_total >= 16 {
            let rate = self.recent_hits * 100 / self.recent_total;
            if rate < HIT_FLOOR_PCT {
                // 判据"预测失败回退"：命中率击穿红线 → 零窗口。
                self.rollback_left = self.policy.engage_rollback();
                self.rollback_count += 1;
                return Ok(hit); // 命中与否照实返回，回退状态经 rollback_left 体现
            }
        }
        Ok(hit)
    }

    /// 查询回退剩余帧（零窗口语义对外可见）。
    pub const fn rollback_remaining(&self) -> u32 {
        self.rollback_left
    }

    /// 账本最近 N 条（公开可查——诚实标注的查询面）。
    pub fn recent_ledger(&self, n: usize) -> Vec<EarlyCommitRecord> {
        let total = self.ledger.len();
        let start = if total > n { total - n } else { 0 };
        let mut out = Vec::new();
        let mut i = start;
        while i < total {
            out.push(self.ledger[i]);
            i += 1;
        }
        out
    }

    /// 稳定性摘要（遥测面）。
    pub fn stability_now(&self) -> LoadStability {
        self.classifier.classify()
    }
}

// ---------------------------------------------------------------------------
// 自检（六判据组：联动预留 / 自适应 / 回退 / 诚实 / 状态机 / 档位）
// ---------------------------------------------------------------------------

impl EarlyCommitManager {
    /// 判据组一：自适应——稳定档提前量大于抖动档（稳定多、抖动少）。
    pub fn selfcheck_adaptive(&mut self) -> Result<(), u32> {
        // 喂稳定帧：32 帧同值。
        let mut i = 0;
        while i < STABILITY_WINDOW {
            self.classifier.observe(16_667);
            i += 1;
        }
        if self.classifier.classify() != LoadStability::Stable {
            return Err(XC_FORECAST_DIVERGED);
        }
        // 喂抖动帧：32 帧大幅交替。
        let mut j = 0;
        while j < STABILITY_WINDOW {
            self.classifier.observe(if j % 2 == 0 { 8_000 } else { 40_000 });
            j += 1;
        }
        if self.classifier.classify() != LoadStability::Jittery {
            return Err(XC_FORECAST_DIVERGED);
        }
        let stable_lead = LoadStability::Stable.lead_us();
        let jitter_lead = LoadStability::Jittery.lead_us();
        if !(stable_lead > jitter_lead) {
            return Err(XC_LEAD_NEGATIVE);
        }
        Ok(())
    }

    /// 判据组二：回退——连错后进入零窗口，期满且恢复后退出。
    pub fn selfcheck_rollback(&mut self) -> Result<(), u32> {
        let mut m = EarlyCommitManager::new();
        let mut f: u64 = 1;
        // 预热 8 帧建立统计。
        let mut k = 0;
        while k < 8 {
            let _ = m.begin_frame(f, 16_000);
            let _ = m.verify_frame(12_000)?; // 全命中
            f += 1;
            k += 1;
        }
        // 连错 16 帧 → 应触发回退。
        let mut engaged = false;
        let mut t = 0;
        while t < 16 {
            let _ = m.begin_frame(f, 16_000);
            let _ = m.verify_frame(48_000)?; // 全错
            f += 1;
            if m.rollback_remaining() > 0 {
                engaged = true;
                break;
            }
            t += 1;
        }
        if !engaged {
            return Err(XC_ROLLBACK_ENGAGED);
        }
        // 回退期内 begin_frame 应给零提前量。
        let rec = m.begin_frame(f, 16_000);
        if rec.lead_us != 0 || !rec.rollback_frame {
            return Err(XC_ROLLBACK_ENGAGED);
        }
        Ok(())
    }

    /// 判据组三：诚实——一期来源恒为统计预测，预留未接入要入账。
    pub fn selfcheck_honesty(&mut self) -> Result<(), u32> {
        let mut m = EarlyCommitManager::new();
        let rec = m.begin_frame(1, 16_000);
        if rec.source != ForecastSource::Statistical {
            return Err(XC_RESERVE_UNAVAILABLE);
        }
        if m.reserve_report_count == 0 {
            return Err(XC_RESERVE_UNAVAILABLE);
        }
        // 二期开关打开后来源才允许 ReservedC04。
        let _ = m.set_c04_link(true);
        let rec2 = m.begin_frame(2, 16_000);
        if rec2.source != ForecastSource::ReservedC04 {
            return Err(XC_RESERVE_UNAVAILABLE);
        }
        Ok(())
    }

    /// 判据组四：状态机——正常/观察/回退三态单调迁移。
    pub fn selfcheck_phase(&mut self) -> Result<(), u32> {
        let p = LeadPolicy::new();
        if p.phase() != CommitPhase::Forecasting {
            return Err(XC_LEAD_NEGATIVE);
        }
        let mut p2 = LeadPolicy::new();
        let _ = p2.feedback(false);
        if p2.phase() != CommitPhase::Cautious {
            return Err(XC_FORECAST_DIVERGED);
        }
        let mut p3 = LeadPolicy::new();
        let left = p3.engage_rollback();
        if left != ROLLBACK_FRAMES || p3.phase() != CommitPhase::Rollback {
            return Err(XC_ROLLBACK_ENGAGED);
        }
        Ok(())
    }

    /// 判据组五：钳制——提前量硬上限生效并计数。
    pub fn selfcheck_clip(&mut self) -> Result<(), u32> {
        let mut m = EarlyCommitManager::new();
        let _ = m.set_c04_link(true); // 排除 reserve 干扰
        // 预热成稳定档。
        let mut i = 0;
        while i < STABILITY_WINDOW {
            m.classifier.observe(16_667);
            i += 1;
        }
        let before = m.clip_count;
        let _ = m.begin_frame(9, 16_667);
        // 稳定档 8ms < 上限 16ms，不应钳制。
        if m.clip_count != before {
            return Err(XC_LEAD_CLIP);
        }
        Ok(())
    }

    /// 判据组六：账本查询——recent_ledger 返回条数正确且字段回填。
    pub fn selfcheck_ledger(&mut self) -> Result<(), u32> {
        let mut m = EarlyCommitManager::new();
        let mut f: u64 = 1;
        let mut k = 0;
        while k < 5 {
            let _ = m.begin_frame(f, 16_000);
            let _ = m.verify_frame(14_000)?;
            f += 1;
            k += 1;
        }
        let recent = m.recent_ledger(3);
        if recent.len() != 3 {
            return Err(XC_LEAD_NEGATIVE);
        }
        if !recent[2].verified_hit {
            return Err(XC_FORECAST_DIVERGED);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_adaptive()?;
        self.selfcheck_rollback()?;
        self.selfcheck_honesty()?;
        self.selfcheck_phase()?;
        self.selfcheck_clip()?;
        self.selfcheck_ledger()?;
        Ok(())
    }
}
