//! VE-F6002 · 物理世界与调度（VE-AD 域 · AD01 批次 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6002`
//!
//! **判据（锚点原文）**：固定步长、插值解耦、参数诚实、判据。
//!
//! **职责定位（锚点原文）**：物理世界管理器与固定步长调度（固定步长是
//! 确定性的地基，插值渲染解耦），步长参数诚实标注（改步长=改世界行为）；
//! 参数变更走流程→影响评估→影响面清单。
//!
//! ## 一、固定步长是确定性的地基，插值是渲染的事
//!
//! 模拟只按 [`DEFAULT_DT_US`] 的**固定步长**推进（整数微秒，零浮点——
//! 同输入同序列即 F6001 可预期律的调度面兑现）；渲染帧率与模拟步长
//! **彻底解耦**：累积器收账、按步消费，渲染拿 [`StepPlan::alpha`]
//! （余数/步长，整数交叉相乘可查）做插值。浮点墙钟永远进不了模拟态。
//!
//! ## 二、步长参数诚实标注：改步长=改世界行为
//!
//! 步长不是调优旋钮而是**世界行为参数**：变更快、碰撞判定口径变、
//! 弹道积分路径变。所以变更走流程（[`FixedStepScheduler::request_dt`]）：
//! 影响评估（[`ImpactAssessment`]）→ 影响面清单逐条列 → 确认位落章
//! 才生效——没有评估单的步长变更是对可预期律的静默背叛。
//!
//! ## 三、越界拒绝 / 累积误差重锚定 / 过载告警
//!
//! 步长越出 [`DT_MIN_US`]/[`DT_MAX_US`] → [`SchedErr::DtOutOfRange`]
//! 显性拒绝；累积余数超 [`REANCHOR_ACCUM_US`] → **重锚定**（丢弃累积
//! 尾差对账到整步，[`WorldLedger::reanchors`] 记账——误差不滚雪球）；
//! 单帧步数撞 [`MAX_SUBSTEPS`] → [`SchedErr::Overload`] 告警（丢帧诚实
//! 呈现不悄悄追帧）。
//!
//! **对接**：上游 F6001 三律（可预期/不失真在此落地）；下游 AD02/AD03
//! 消费（世界状态按 StepPlan 交付）。零 panic 面、零 IO、零墙钟
//! （elapsed 由调用方喂入的账面微秒）、无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源；整数微秒，零浮点）
// ---------------------------------------------------------------------------

/// 默认固定步长（16667 μs ≈ 60 Hz；整数口径不凑整）。
pub const DEFAULT_DT_US: u64 = 16_667;

/// 步长下界（1000 μs = 1 kHz；防止 步长→0 的积分发散）。
pub const DT_MIN_US: u64 = 1_000;

/// 步长上界（50_000 μs = 20 Hz；防止大步长穿透碰撞）。
pub const DT_MAX_US: u64 = 50_000;

/// 单帧最大子步数（过载阈：撞线即告警丢帧）。
pub const MAX_SUBSTEPS: u32 = 5;

/// 重锚定累积阈（μs）：尾差超过即重锚定，误差不滚雪球。
pub const REANCHOR_ACCUM_US: u64 = 250_000;

/// 影响面清单固定条目数（调度/碰撞/弹道/复现/录放——参数诚实五面）。
pub const IMPACT_FACES: usize = 5;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x3Fxx 段；0x3Exx 归 F6001）
// ---------------------------------------------------------------------------

/// F6002 诊断码。独占 `0x3Fxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedCode(pub u16);

impl SchedCode {
    /// 步长越界。
    pub const DT_OUT_OF_RANGE: SchedCode = SchedCode(0x3F01);
    /// 步长变更未过影响评估流程。
    pub const NO_ASSESSMENT: SchedCode = SchedCode(0x3F02);
    /// 调度过载（单帧步数撞线，丢帧告警）。
    pub const OVERLOAD: SchedCode = SchedCode(0x3F03);
    /// 累积误差重锚定（可观测，非错误）。
    pub const REANCHORED: SchedCode = SchedCode(0x3F04);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            SchedCode::DT_OUT_OF_RANGE => "步长越界：DT_MIN_US..=DT_MAX_US 之外显性拒绝".into(),
            SchedCode::NO_ASSESSMENT => "步长变更未过影响评估流程：改步长=改世界行为".into(),
            SchedCode::OVERLOAD => "调度过载：单帧步数撞线，丢帧告警不悄悄追帧".into(),
            SchedCode::REANCHORED => "累积误差重锚定：尾差对账到整步".into(),
            SchedCode(_) => "未知调度诊断码".into(),
        }
    }
}

/// 调度错误路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedErr {
    /// 携 [`SchedCode`] 的失败。
    Code(SchedCode),
}

impl SchedErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            SchedErr::Code(c) => c.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、影响评估 / 步计划（数据结构：世界管理器×固定步长调度）
// ---------------------------------------------------------------------------

/// 步长变更影响评估（锚点：流程含影响评估，评估含影响面清单）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactAssessment {
    /// 影响面清单（逐条列，空清单=评估未做）。
    pub faces: Vec<&'static str>,
    /// 评估落章（确认位；未落章的变更不生效）。
    pub signed: bool,
}

/// 影响面标准五条（参数诚实：这五面都随步长变）。
pub const IMPACT_FACE_LIST: [&str; IMPACT_FACES] = [
    "调度节奏（每秒步数变）",
    "碰撞判定口径（步内位移变）",
    "弹道积分路径（数值积分序列变）",
    "复现对账（历史摘要序列失效）",
    "录放时间轴（步号↔时间映射变）",
];

impl ImpactAssessment {
    /// 标准五面评估（清单逐条落 + 未落章）。
    pub fn standard() -> Self {
        ImpactAssessment {
            faces: IMPACT_FACE_LIST.to_vec(),
            signed: false,
        }
    }

    /// 评估完备性：清单齐五面且已落章。
    pub const fn complete(&self) -> bool {
        self.faces.len() == IMPACT_FACES && self.signed
    }
}

/// 单帧步计划（插值解耦的交付面：模拟走了几步、渲染拿多少 alpha）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepPlan {
    /// 本帧推进的整步数。
    pub steps: u32,
    /// 插值系数（余数×1000/步长，千分整数——渲染侧插值用，零浮点）。
    pub alpha_permille: u32,
    /// 是否过载丢帧（诚实呈现）。
    pub overloaded: bool,
    /// 是否发生重锚定。
    pub reanchored: bool,
}

// ---------------------------------------------------------------------------
// 四、WorldManager / FixedStepScheduler 主结构（调度 O(步长)）
// ---------------------------------------------------------------------------

/// 物理世界账面（世界管理器：步数/重锚定/过载/摘要序列）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorldLedger {
    /// 累计推进整步数。
    pub steps: u64,
    /// 累计重锚定次数。
    pub reanchors: u32,
    /// 累计过载告警次数。
    pub overloads: u32,
    /// 累计丢帧步数（过载时未消费的累积）。
    pub dropped_steps: u32,
}

/// 固定步长调度器（累积器模式；同输入同序列——可预期律调度面）。
#[derive(Debug)]
pub struct FixedStepScheduler {
    dt_us: u64,
    accum_us: u64,
    /// 步长变更评估单（None=从未变更；Some 未落章=变更挂起不生效）。
    assessment: Option<ImpactAssessment>,
    ledger: WorldLedger,
    world_steps: u64,
}

impl FixedStepScheduler {
    /// 默认步长构造。
    pub fn new() -> Self {
        FixedStepScheduler {
            dt_us: DEFAULT_DT_US,
            accum_us: 0,
            assessment: None,
            ledger: WorldLedger::default(),
            world_steps: 0,
        }
    }

    /// 当前步长（μs）。
    pub const fn dt_us(&self) -> u64 {
        self.dt_us
    }

    /// 世界账面只读视图。
    pub const fn ledger(&self) -> &WorldLedger {
        &self.ledger
    }

    /// 世界已推进步数（世界管理器状态面）。
    pub const fn world_steps(&self) -> u64 {
        self.world_steps
    }

    /// 步长变更请求：越界拒绝；评估单未齐/未落章拒绝——流程不齐不生效。
    /// 全部通过才真正切步长（参数诚实的流程面）。
    pub fn request_dt(
        &mut self,
        new_dt_us: u64,
        assessment: ImpactAssessment,
    ) -> Result<(), SchedErr> {
        if new_dt_us < DT_MIN_US || new_dt_us > DT_MAX_US {
            return Err(SchedErr::Code(SchedCode::DT_OUT_OF_RANGE));
        }
        if !assessment.complete() {
            return Err(SchedErr::Code(SchedCode::NO_ASSESSMENT));
        }
        self.assessment = Some(assessment);
        self.dt_us = new_dt_us;
        Ok(())
    }

    /// 当前评估单只读视图（步长参数可查——无障碍面）。
    pub const fn assessment(&self) -> Option<&ImpactAssessment> {
        self.assessment.as_ref()
    }

    /// 推进一帧（elapsed_us 为调用方喂入的账面微秒；调度 O(步长)）。
    /// 累积器收账→整步消费→余数给渲染插值；过载丢帧告警；尾差超阈重锚定。
    pub fn advance(&mut self, elapsed_us: u64) -> StepPlan {
        self.accum_us = self.accum_us.saturating_add(elapsed_us);
        let mut steps = 0u32;
        while self.accum_us >= self.dt_us && steps < MAX_SUBSTEPS {
            self.accum_us -= self.dt_us;
            self.world_steps = self.world_steps.saturating_add(1);
            steps += 1;
        }
        let mut overloaded = false;
        let mut reanchored = false;
        if steps == MAX_SUBSTEPS && self.accum_us >= self.dt_us {
            // 过载：剩余累积按丢帧记账，显性告警不悄悄追帧
            let dropped = (self.accum_us / self.dt_us).min(u32::MAX as u64) as u32;
            self.accum_us %= self.dt_us;
            self.ledger.dropped_steps = self.ledger.dropped_steps.saturating_add(dropped);
            self.ledger.overloads = self.ledger.overloads.saturating_add(1);
            overloaded = true;
            // 重锚定：丢帧漂移（dropped×dt）累积超阈即对账——补推进丢掉的步，
            // 世界追上真实时间，漂移清零（累积误差不滚雪球）
            let drift_us = (self.ledger.dropped_steps as u64).saturating_mul(self.dt_us);
            if drift_us >= REANCHOR_ACCUM_US {
                self.world_steps = self
                    .world_steps
                    .saturating_add(self.ledger.dropped_steps as u64);
                self.ledger.dropped_steps = 0;
                self.ledger.reanchors = self.ledger.reanchors.saturating_add(1);
                reanchored = true;
            }
        }
        self.ledger.steps = self.ledger.steps.saturating_add(steps as u64);
        // 插值 alpha：余数×1000/步长（整数交叉相乘可查，零浮点零除零——dt≥DT_MIN_US）
        let alpha_permille = ((self.accum_us.min(self.dt_us) * 1000) / self.dt_us) as u32;
        StepPlan {
            steps,
            alpha_permille,
            overloaded,
            reanchored,
        }
    }

    /// 判据访问器：累积余数（重锚定/插值对账用）。
    pub const fn accum_probe(&self) -> u64 {
        self.accum_us
    }

    /// 可预期律对账面：同账面状态跑同一 elapsed 序列，步数序列必等。
    /// （判据侧两实例独立推进对账——同输入同结果的可调度面证据。）
    pub fn replay_digest(&self) -> u64 {
        // 账面摘要：步数×31 + 余数 + 步长（确定性组合，零随机）
        self.ledger
            .steps
            .wrapping_mul(31)
            .wrapping_add(self.accum_us)
            .wrapping_add(self.dt_us)
            .wrapping_mul(3)
            .wrapping_add(self.world_steps)
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：固定步长、插值解耦、参数诚实、判据）
// ---------------------------------------------------------------------------

/// VE-F6002 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vead02_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vead02_worldsched");

    // —— 判据一 · 固定步长：整步消费、余数留账 ——
    let mut sc = FixedStepScheduler::new();
    let plan = sc.advance(DEFAULT_DT_US * 2 + 8_000);
    s.add(
        "AD02-固定步长-整步消费余数留账",
        plan.steps == 2
            && sc.world_steps() == 2
            && sc.ledger().steps == 2
            && plan.alpha_permille == (8_000u64 * 1000 / DEFAULT_DT_US) as u32,
        "两整步推进+余数 8000μs 留账；alpha 千分整数与独立重算相符（零浮点）",
    );

    // —— 判据一 · 反向：同输入同序列（可预期律调度面，双实例对账） ——
    let mut a = FixedStepScheduler::new();
    let mut b = FixedStepScheduler::new();
    for e in [16_667u64, 8_333, 33_334, 16_666] {
        a.advance(e);
        b.advance(e);
    }
    s.add(
        "AD02-固定步长-同输入同序列",
        a.replay_digest() == b.replay_digest()
            && a.world_steps() == b.world_steps()
            && a.world_steps() == 4,
        "同 elapsed 序列双实例独立推进：摘要/步数全等（固定步长是确定性的地基）",
    );

    // —— 判据二 · 插值解耦：渲染 alpha 与模拟步数互不牵扯 ——
    let mut c = FixedStepScheduler::new();
    let p1 = c.advance(1_000); // 不足一步：0 步但 alpha>0
    let p2 = c.advance(DEFAULT_DT_US); // 补足一步
    s.add(
        "AD02-插值-渲染解耦零浮点",
        p1.steps == 0 && p1.alpha_permille > 0 && p1.alpha_permille < 1000
            && p2.steps == 1
            && p2.alpha_permille == (1_000u64 * 1000 / DEFAULT_DT_US) as u32,
        "不足步不硬推但 alpha 供渲染插值；补足后整步走——模拟与渲染两本账",
    );

    // —— 判据二 · 反向：alpha 上界钉死（满步前恰 <1000，步缘归零） ——
    let mut d = FixedStepScheduler::new();
    let edge = d.advance(DEFAULT_DT_US - 1);
    let _ = d.advance(1);
    let at_step = d.advance(0);
    s.add(
        "AD02-插值-alpha界含端点",
        edge.alpha_permille < 1000 && at_step.alpha_permille == 0 && at_step.steps == 0,
        "步缘前 alpha 恰 <1000；步缘后余数归零（0 步 alpha 0——插值系数不过冲）",
    );

    // —— 判据三 · 参数诚实：越界拒绝 + 无评估拒绝 + 落章生效 ——
    let mut e = FixedStepScheduler::new();
    let oob = e.request_dt(DT_MAX_US + 1, ImpactAssessment::standard());
    let oob2 = e.request_dt(DT_MIN_US - 1, ImpactAssessment::standard());
    let unsigned = ImpactAssessment::standard();
    let no_sign = e.request_dt(8_333, unsigned); // 五面齐但未落章 → 拒
    let mut incomplete2 = ImpactAssessment::standard();
    incomplete2.faces.pop();
    let short_list = e.request_dt(8_333, incomplete2);
    let signed = e.request_dt(8_333, ImpactAssessment { signed: true, ..ImpactAssessment::standard() });
    s.add(
        "AD02-参数诚实-流程不齐不生效",
        oob == Err(SchedErr::Code(SchedCode::DT_OUT_OF_RANGE))
            && oob2 == Err(SchedErr::Code(SchedCode::DT_OUT_OF_RANGE))
            && no_sign == Err(SchedErr::Code(SchedCode::NO_ASSESSMENT))
            && short_list == Err(SchedErr::Code(SchedCode::NO_ASSESSMENT))
            && signed.is_ok()
            && e.dt_us() == 8_333,
        "上下界含端点外拒绝；缺落章/缺清单面拒绝；五面齐+落章才切步长（改步长=改世界行为）",
    );

    // —— 判据三 · 反向：评估单影响面五条齐备可查（参数可查） ——
    let asm = ImpactAssessment::standard();
    let query_ok = asm.faces.len() == IMPACT_FACES
        && asm.faces.contains(&"复现对账（历史摘要序列失效）")
        && asm.signed == false
        && e.assessment().map(|a| a.complete()) == Some(true);
    s.add(
        "AD02-参数诚实-影响面清单可查",
        query_ok,
        "标准五面（含复现/录放）齐备；评估单落章状态可查（步长参数可查——无障碍面）",
    );

    // —— 判据四 · 过载告警：撞线丢帧显性不悄悄追帧 ——
    let mut f = FixedStepScheduler::new();
    // 一帧喂 10 步量级（dt=16667，10×16667=166670 > 5×16667）
    let plan = f.advance(DEFAULT_DT_US * 10);
    s.add(
        "AD02-过载-撞线告警丢帧记账",
        plan.steps == MAX_SUBSTEPS
            && plan.overloaded
            && f.ledger().overloads == 1
            && f.ledger().dropped_steps == 10 - MAX_SUBSTEPS,
        "单帧只走 MAX_SUBSTEPS 步；超量丢帧显性告警+逐步记账（不悄悄追帧）",
    );

    // —— 判据四 · 反向：重锚定（丢帧漂移超阈对账，世界追上真实时间） ——
    let mut g = FixedStepScheduler::new();
    // 连续大帧喂入：每帧过载丢 5 步，丢帧漂移攒到 REANCHOR_ACCUM_US 触发重锚定
    let mut last = StepPlan { steps: 0, alpha_permille: 0, overloaded: false, reanchored: false };
    let mut frames = 0u32;
    while !last.reanchored && frames < 100 {
        last = g.advance(DEFAULT_DT_US * 10);
        frames += 1;
    }
    let world_consistent = g.world_steps() >= g.ledger().steps
        && g.ledger().dropped_steps * (DEFAULT_DT_US as u32) < REANCHOR_ACCUM_US as u32;
    s.add(
        "AD02-重锚定-丢帧漂移对账",
        last.reanchored
            && g.ledger().reanchors == 1
            && world_consistent
            && g.accum_probe() < DEFAULT_DT_US,
        "丢帧漂移超阈触发重锚定：补推进丢掉的步（世界追上真实时间）、漂移清零、记账 1 次",
    );

    // —— 判据五 · 元数据：码段互异 + 常量口径 ——
    let codes = [
        SchedCode::DT_OUT_OF_RANGE.code(),
        SchedCode::NO_ASSESSMENT.code(),
        SchedCode::OVERLOAD.code(),
        SchedCode::REANCHORED.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let dt_ok = DT_MIN_US <= DEFAULT_DT_US && DEFAULT_DT_US <= DT_MAX_US;
    s.add(
        "AD02-判据-码段互异且步长口径自洽",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x3F00)
            && dt_ok
            && IMPACT_FACES == IMPACT_FACE_LIST.len()
            && MAX_SUBSTEPS >= 1,
        "四码全落 0x3Fxx 两两互异；默认步长在界内；影响面清单条目数与常量对账",
    );

    s
}

/// 判据访问器：累积余数（重锚定对账用）。
pub fn accum_of(sc: &FixedStepScheduler) -> u64 {
    sc.accum_probe()
}
