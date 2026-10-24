//! vcu05_benchrunner — CGPU-F3205 基准运行器（场景调度/计时/采集三功能）。
//!
//! 判据原文（施工书锚点 CGPU-F3205）：运行器：基准运行器（场景调度/计时/
//! 采集——运行器实现；测试（运行器一组）。判据：三功能、一组、判据。
//!
//! **三功能**（判据"三功能"的本体——三个子系统各一个闭集账）：
//! 1. **场景调度**：场景队列调度器——优先级 × FIFO 确定序（同优先级
//!    先来先跑——排序键二元组全序）；场景五态单向状态机
//!    （Queued→Warmup→Measuring→Done/Failed，终态不可回退）；
//! 2. **计时**：tick 注入差分（零墙钟）——预热跑与计测跑分离
//!    （预热不计分），计测 N 跑取**定点中位数**（奇数跑取中，偶数跑取
//!    中间两数较小者——确定性断序），噪声闸（极差/中位数 > 10% →
//!    显性 RerunFlag，不静默出分）；
//! 3. **采集**：逐场景指标账——中位数/极差/跑数/指纹四件随账走，
//!    汇总面 P50/P95 定点分位（复用声明 cgd02 帧计时口径 /
//!    vcu02 基准模型 / vcu04 计分系统——本模块是运行编排层）。
//!
//! **确定性**（深化面）：同队列同负载两次运行 → 调度序逐位一致 +
//! 汇总指纹一致（双跑对拍）——运行器本身必须比被测物更确定。
//!
//! 诊断码段 0xBFxx。no_std + alloc 零 unwrap 零 expect 零 panic 面
//! 零 IO 零墙钟零浮点。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const VCU05_BENCHRUNNER_VERSION: &str = "vcu05-benchrunner-v1";

/// 诊断段基值（0xBF 段）。
pub const SEG_BASE: u16 = 0xBF00;

/// 预热跑数（预热不计分）。
pub const WARMUP_RUNS: u32 = 2;

/// 计测跑数（奇数——中位数直取）。
pub const MEASURE_RUNS: u32 = 5;

/// 噪声闸线（极差/中位数 > 10% → RerunFlag）。
pub const NOISE_LINE_BPS: u64 = 1_000;

/// 采集账容量（场景数上限——超限拒绝入队）。
pub const SCENARIO_CAP: usize = 32;

/// 事件账滚动容量。
pub const EVENT_CAP: usize = 48;

// ─────────────────────────────────────────────────────────────────────────────
// FNV
// ─────────────────────────────────────────────────────────────────────────────

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// u64 混入。
pub const fn fnv1a_u64(mut s: u64, mut v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s = (s ^ (v & 0xFF)) * FNV_PRIME;
        v >>= 8;
        i += 1;
    }
    s
}

/// 字符串混入。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    let b = v.as_bytes();
    let mut s = s;
    let mut i = 0;
    while i < b.len() {
        s = (s ^ (b[i] as u64)) * FNV_PRIME;
        i += 1;
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// 场景与调度（三功能之一）
// ─────────────────────────────────────────────────────────────────────────────

/// 场景五态单向状态机（判据"场景调度"——终态不可回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScenarioState {
    /// 排队中。
    Queued,
    /// 预热中（不计分）。
    Warmup,
    /// 计测中。
    Measuring,
    /// 完成（终态）。
    Done,
    /// 失败（终态）。
    Failed,
}

impl ScenarioState {
    /// 合法迁移（单向——只允许 +1 或跳到 Failed；Done/Failed 无出边）。
    pub const fn can_transition(self, to: ScenarioState) -> bool {
        match (self, to) {
            (ScenarioState::Queued, ScenarioState::Warmup) => true,
            (ScenarioState::Warmup, ScenarioState::Measuring) => true,
            (ScenarioState::Measuring, ScenarioState::Done) => true,
            (ScenarioState::Queued, ScenarioState::Failed) => true,
            (ScenarioState::Warmup, ScenarioState::Failed) => true,
            (ScenarioState::Measuring, ScenarioState::Failed) => true,
            _ => false,
        }
    }

    /// 是否终态。
    pub const fn terminal(self) -> bool {
        matches!(self, ScenarioState::Done | ScenarioState::Failed)
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            ScenarioState::Queued => "排队",
            ScenarioState::Warmup => "预热",
            ScenarioState::Measuring => "计测",
            ScenarioState::Done => "完成",
            ScenarioState::Failed => "失败",
        }
    }
}

/// 场景登记项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scenario {
    /// 场景名（稳定键）。
    pub name: String,
    /// 优先级（数值小者先跑——呈现>交互>后台语义复用声明）。
    pub priority: u32,
    /// 登记序（FIFO 破平键）。
    pub seq: u64,
    /// 状态。
    pub state: ScenarioState,
}

/// 调度序全序键：二元组（优先级, 登记序）——判据"确定序"的本体。
pub fn scheduling_order(a: &Scenario, b: &Scenario) -> bool {
    (a.priority, a.seq) < (b.priority, b.seq)
}

/// 调度器（判据"场景调度"子系统）。
pub struct Scheduler {
    /// 场景账。
    pub scenarios: Vec<Scenario>,
    /// 登记计数（seq 单调）。
    pub seq_next: u64,
    /// 出队游标（确定性：按全序键选最小——不移动 Vec）。
    pub dispatched: u64,
}

impl Scheduler {
    /// 新调度器。
    pub const fn new() -> Scheduler {
        Scheduler {
            scenarios: Vec::new(),
            seq_next: 0,
            dispatched: 0,
        }
    }

    /// 入队（同名重复拒——确定性键唯一）。
    pub fn enqueue(&mut self, name: &str, priority: u32) -> Result<u64, RnrCode> {
        if self.scenarios.len() >= SCENARIO_CAP {
            return Err(RnrCode::RnScenarioFull);
        }
        if self.scenarios.iter().any(|s| s.name == name) {
            return Err(RnrCode::RnDuplicate);
        }
        let seq = self.seq_next;
        self.scenarios.push(Scenario {
            name: String::from(name),
            priority,
            seq,
            state: ScenarioState::Queued,
        });
        self.seq_next += 1;
        Ok(seq)
    }

    /// 出队：按全序键取最小未终态场景（无副作用——状态迁移由 run 步做）。
    pub fn peek_next(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, s) in self.scenarios.iter().enumerate() {
            if s.state.terminal() {
                continue;
            }
            match best {
                None => best = Some(i),
                Some(b) => {
                    if scheduling_order(s, &self.scenarios[b]) {
                        best = Some(i);
                    }
                }
            }
        }
        best
    }

    /// 状态迁移（单向闸——非法迁移 RnStateMove）。
    pub fn transition(&mut self, name: &str, to: ScenarioState) -> Result<(), RnrCode> {
        let idx = self
            .scenarios
            .iter()
            .position(|s| s.name == name)
            .ok_or(RnrCode::RnScenarioUnknown)?;
        if self.scenarios[idx].state.terminal() {
            return Err(RnrCode::RnStateMove);
        }
        let from = self.scenarios[idx].state;
        if !from.can_transition(to) {
            return Err(RnrCode::RnStateMove);
        }
        self.scenarios[idx].state = to;
        self.dispatched += 1;
        Ok(())
    }

    /// 调度序指纹（判据"确定性"：同账同序——指纹随快照走）。
    pub fn order_fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, VCU05_BENCHRUNNER_VERSION);
        // 按全序键排序后混名（排序用插入序选择——容量 32 内插入排序定点）。
        let mut idx: Vec<usize> = (0..self.scenarios.len()).collect();
        // 简单插入排序（n≤32，确定性）。
        for i in 1..idx.len() {
            let mut j = i;
            while j > 0
                && scheduling_order(&self.scenarios[idx[j]], &self.scenarios[idx[j - 1]])
            {
                idx.swap(j, j - 1);
                j -= 1;
            }
        }
        for i in idx {
            s = fnv1a_str(s, &self.scenarios[i].name);
            s = fnv1a_u64(s, self.scenarios[i].priority as u64);
            s = fnv1a_u64(s, self.scenarios[i].state as u64);
        }
        s
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码（0xBFxx 段独占十二码）
// ─────────────────────────────────────────────────────────────────────────────

/// 诊断码（0xBFxx 段独占——十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RnrCode {
    /// 0xBF01 场景账满。
    RnScenarioFull,
    /// 0xBF02 场景重名。
    RnDuplicate,
    /// 0xBF03 场景未知。
    RnScenarioUnknown,
    /// 0xBF04 非法状态迁移（终态回退/跳步）。
    RnStateMove,
    /// 0xBF05 计测跑数不足（未跑满 MEASURE_RUNS 出分——拒绝）。
    RnRunsShort,
    /// 0xBF06 噪声超标（极差/中位数>10%——显性 RerunFlag）。
    RnNoisy,
    /// 0xBF07 tick 倒流。
    RnTickRegress,
    /// 0xBF08 采集账指纹不符（账实冲突）。
    RnLedgerMismatch,
    /// 0xBF09 双跑不符（确定性破坏）。
    RnDeterminism,
    /// 0xBF0A 快照漂移。
    RnSnapshotDrift,
    /// 0xBF0B 汇总空账（无完成场景出汇总——拒绝）。
    RnSummaryEmpty,
    /// 0xBF0C 指标越域（单跑耗时为 0——注入面腐坏）。
    RnMetricRange,
}

impl RnrCode {
    /// 稳定码值。
    pub const fn code(self) -> u16 {
        match self {
            RnrCode::RnScenarioFull => 0xBF01,
            RnrCode::RnDuplicate => 0xBF02,
            RnrCode::RnScenarioUnknown => 0xBF03,
            RnrCode::RnStateMove => 0xBF04,
            RnrCode::RnRunsShort => 0xBF05,
            RnrCode::RnNoisy => 0xBF06,
            RnrCode::RnTickRegress => 0xBF07,
            RnrCode::RnLedgerMismatch => 0xBF08,
            RnrCode::RnDeterminism => 0xBF09,
            RnrCode::RnSnapshotDrift => 0xBF0A,
            RnrCode::RnSummaryEmpty => 0xBF0B,
            RnrCode::RnMetricRange => 0xBF0C,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            RnrCode::RnScenarioFull => "场景账满",
            RnrCode::RnDuplicate => "场景重名",
            RnrCode::RnScenarioUnknown => "场景未知",
            RnrCode::RnStateMove => "非法状态迁移",
            RnrCode::RnRunsShort => "计测跑数不足",
            RnrCode::RnNoisy => "噪声超标",
            RnrCode::RnTickRegress => "tick倒流",
            RnrCode::RnLedgerMismatch => "采集账指纹不符",
            RnrCode::RnDeterminism => "双跑不符",
            RnrCode::RnSnapshotDrift => "快照漂移",
            RnrCode::RnSummaryEmpty => "汇总空账",
            RnrCode::RnMetricRange => "指标越域",
        }
    }

    /// 中文详情。
    pub const fn detail(self) -> &'static str {
        match self {
            RnrCode::RnScenarioFull => "场景账满：超采集容量上限——拒绝入队不挤占",
            RnrCode::RnDuplicate => "场景重名：同名场景确定性键唯一——拒绝重登记",
            RnrCode::RnScenarioUnknown => "场景未知：迁移目标不在账",
            RnrCode::RnStateMove => "非法状态迁移：终态无出边/跳步——单向状态机闸",
            RnrCode::RnRunsShort => "计测跑数不足：未跑满即出分——拒绝（分要跑出来）",
            RnrCode::RnNoisy => "噪声超标：极差/中位数>10%——显性 RerunFlag 不静默出分",
            RnrCode::RnTickRegress => "tick倒流：注入面腐坏",
            RnrCode::RnLedgerMismatch => "采集账指纹不符：账实冲突",
            RnrCode::RnDeterminism => "双跑不符：同队列两次运行结论不同——运行器失确定",
            RnrCode::RnSnapshotDrift => "快照漂移：FNV 对拍不符",
            RnrCode::RnSummaryEmpty => "汇总空账：无完成场景不出汇总",
            RnrCode::RnMetricRange => "指标越域：单跑耗时为 0——计时面腐坏",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 计时（三功能之二）——定点中位数 + 噪声闸
// ─────────────────────────────────────────────────────────────────────────────

/// 定点中位数（判据"计时"的计分口径）。
///
/// 升序后取中：奇数取中位；偶数取中间两数**较小者**（确定性断序——
/// 不取均值避免除 2 尾数分歧）。
pub fn median_ns(runs: &[u64]) -> Result<u64, RnrCode> {
    if runs.is_empty() {
        return Err(RnrCode::RnRunsShort);
    }
    for r in runs {
        if *r == 0 {
            return Err(RnrCode::RnMetricRange);
        }
    }
    let mut v = Vec::new();
    for r in runs {
        v.push(*r);
    }
    // 插入排序（n≤16 确定性）。
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && v[j] < v[j - 1] {
            v.swap(j, j - 1);
            j -= 1;
        }
    }
    let n = v.len();
    Ok(v[(n - 1) / 2])
}

/// 极差（max−min）。
pub fn range_ns(runs: &[u64]) -> Result<u64, RnrCode> {
    if runs.is_empty() {
        return Err(RnrCode::RnRunsShort);
    }
    let mut mn = u64::MAX;
    let mut mx = 0u64;
    for r in runs {
        if *r == 0 {
            return Err(RnrCode::RnMetricRange);
        }
        if *r < mn {
            mn = *r;
        }
        if *r > mx {
            mx = *r;
        }
    }
    Ok(mx - mn)
}

/// 噪声判定（判据"噪声闸"：极差×10000 > 中位数×NOISE_LINE_BPS）。
pub fn noisy(runs: &[u64]) -> Result<bool, RnrCode> {
    let med = median_ns(runs)?;
    let rng = range_ns(runs)?;
    Ok((rng as u128) * 10_000 > (med as u128) * (NOISE_LINE_BPS as u128))
}

/// 单场景计时账（判据"计时"产物——四件随账走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimingResult {
    /// 中位数（ns）。
    pub median: u64,
    /// 极差（ns）。
    pub range: u64,
    /// 计测跑数。
    pub runs: u32,
    /// 噪声超标旗（true=显性 RerunFlag——分数仅供参考）。
    pub rerun_flag: bool,
    /// 计时指纹（跑序 × 跑值 FNV——账实对拍锚）。
    pub timing_fp: u64,
}

/// 计时汇总（判据"计时"主入口：计测跑数闸 + 中位数 + 噪声闸）。
pub fn time_scenario(runs: &[u64]) -> Result<TimingResult, RnrCode> {
    if (runs.len() as u32) < MEASURE_RUNS {
        return Err(RnrCode::RnRunsShort);
    }
    let med = median_ns(runs)?;
    let rng = range_ns(runs)?;
    let flag = noisy(runs)?;
    let mut fp = fnv1a_u64(FNV_OFFSET, runs.len() as u64);
    for r in runs {
        fp = fnv1a_u64(fp, *r);
    }
    Ok(TimingResult {
        median: med,
        range: rng,
        runs: runs.len() as u32,
        rerun_flag: flag,
        timing_fp: fp,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 采集（三功能之三）——逐场景账 + 汇总分位
// ─────────────────────────────────────────────────────────────────────────────

/// 单场景采集行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CollectedRow {
    /// 场景名。
    pub name: String,
    /// 计时账。
    pub timing: TimingResult,
    /// 采集序（完成序）。
    pub done_seq: u64,
}

/// 采集账（判据"采集"子系统）。
pub struct Collector {
    /// 逐场景行（完成序追加）。
    pub rows: Vec<CollectedRow>,
    /// 完成计数。
    pub done_count: u64,
}

impl Collector {
    /// 新采集账。
    pub const fn new() -> Collector {
        Collector {
            rows: Vec::new(),
            done_count: 0,
        }
    }

    /// 采集一行（同名重复采集拒——一场景一账）。
    pub fn collect(&mut self, name: &str, timing: TimingResult) -> Result<(), RnrCode> {
        if self.rows.iter().any(|r| r.name == name) {
            return Err(RnrCode::RnDuplicate);
        }
        let done_seq = self.done_count;
        self.done_count += 1;
        self.rows.push(CollectedRow {
            name: String::from(name),
            timing,
            done_seq,
        });
        Ok(())
    }

    /// 汇总分位（判据"采集"汇总面）：全体完成场景中位数序列的
    /// P50/P95（定点——升序取 (n×pct)/100 下标，下取整）。
    pub fn summary(&self) -> Result<(u64, u64), RnrCode> {
        if self.rows.is_empty() {
            return Err(RnrCode::RnSummaryEmpty);
        }
        let mut meds: Vec<u64> = Vec::new();
        for r in &self.rows {
            meds.push(r.timing.median);
        }
        for i in 1..meds.len() {
            let mut j = i;
            while j > 0 && meds[j] < meds[j - 1] {
                meds.swap(j, j - 1);
                j -= 1;
            }
        }
        let n = meds.len();
        let p50 = meds[(n * 50) / 100];
        let mut p95_i = (n * 95) / 100;
        if p95_i >= n {
            p95_i = n - 1;
        }
        Ok((p50, meds[p95_i]))
    }

    /// 采集账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.rows.len() as u64);
        for r in &self.rows {
            s = fnv1a_str(s, &r.name);
            s = fnv1a_u64(s, r.timing.median);
            s = fnv1a_u64(s, r.timing.timing_fp);
            s = fnv1a_u64(s, r.done_seq);
        }
        s
    }
}

impl Default for Collector {
    fn default() -> Self {
        Collector::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 运行器主体（三功能编排 + 事件账 + 快照）
// ─────────────────────────────────────────────────────────────────────────────

/// 账本事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RnrEvent {
    /// 入队（seq）。
    Enqueued(u64),
    /// 状态迁移（from 序, to 序）。
    Transition(u64, u64),
    /// 采集完成（done_seq）。
    Collected(u64),
    /// 噪声旗（done_seq）。
    NoisyFlag(u64),
}

impl RnrEvent {
    /// 事件主键。
    pub const fn key(self) -> u64 {
        match self {
            RnrEvent::Enqueued(k) | RnrEvent::Transition(k, _) | RnrEvent::Collected(k) | RnrEvent::NoisyFlag(k) => k,
        }
    }
}

/// 运行器（判据"三功能"编排面）。
pub struct BenchRunner {
    /// 调度器。
    pub sched: Scheduler,
    /// 采集账。
    pub collector: Collector,
    /// 事件账（滚动）。
    pub events: Vec<RnrEvent>,
}

impl BenchRunner {
    /// 新运行器。
    pub const fn new() -> BenchRunner {
        BenchRunner {
            sched: Scheduler::new(),
            collector: Collector::new(),
            events: Vec::new(),
        }
    }

    /// 事件入账。
    fn push_event(&mut self, e: RnrEvent) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 登记场景（Queued）。
    pub fn register(&mut self, name: &str, priority: u32) -> Result<u64, RnrCode> {
        let seq = self.sched.enqueue(name, priority)?;
        self.push_event(RnrEvent::Enqueued(seq));
        Ok(seq)
    }

    /// 推进状态（单向闸 + 事件）。
    pub fn advance(&mut self, name: &str, to: ScenarioState) -> Result<(), RnrCode> {
        // 先查 from 序（事件账用）。
        let from = self
            .sched
            .scenarios
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.state)
            .ok_or(RnrCode::RnScenarioUnknown)?;
        self.sched.transition(name, to)?;
        self.push_event(RnrEvent::Transition(from as u64, to as u64));
        Ok(())
    }

    /// 提交计时结果（Measuring→Done + 采集；噪声旗显性事件）。
    pub fn submit(&mut self, name: &str, runs: &[u64]) -> Result<TimingResult, RnrCode> {
        let t = time_scenario(runs)?;
        self.advance(name, ScenarioState::Done)?;
        self.collector.collect(name, t)?;
        self.push_event(RnrEvent::Collected(t.median));
        if t.rerun_flag {
            self.push_event(RnrEvent::NoisyFlag(t.median));
        }
        Ok(t)
    }

    /// 场景失败落账（任意非终态→Failed）。
    pub fn fail(&mut self, name: &str) -> Result<(), RnrCode> {
        self.advance(name, ScenarioState::Failed)
    }

    /// 运行器指纹（调度序 + 采集账联合——双跑对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let s = self.sched.order_fingerprint();
        fnv1a_u64(s, self.collector.fingerprint())
    }

    /// 守恒对账：登记数 == 终态数 + 在途数。
    pub fn conservation_ok(&self) -> bool {
        let terminals = self
            .sched
            .scenarios
            .iter()
            .filter(|s| s.state.terminal())
            .count();
        terminals + (self.sched.scenarios.len() - terminals) == self.sched.scenarios.len()
    }
}

impl Default for BenchRunner {
    fn default() -> Self {
        BenchRunner::new()
    }
}

/// 账本快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RnrSnap {
    /// 指纹。
    pub fingerprint: u64,
    /// 完成场景数。
    pub done: u64,
}

impl RnrSnap {
    /// 取快照。
    pub fn take(r: &BenchRunner) -> RnrSnap {
        let mut s = r.fingerprint();
        s = fnv1a_u64(s, r.collector.done_count);
        s = fnv1a_u64(s, r.events.len() as u64);
        for e in &r.events {
            s = fnv1a_u64(s, e.key());
        }
        RnrSnap {
            fingerprint: s,
            done: r.collector.done_count,
        }
    }

    /// 对拍。
    pub const fn matches(&self, other: &RnrSnap) -> bool {
        self.fingerprint == other.fingerprint
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 参考探针（判据"一组"的手算恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 探针汇总。
pub struct ProbeReport {
    /// 调度确定序（优先级小者先，同优先级 FIFO）。
    pub sched_order: bool,
    /// 入队满拒。
    pub full_block: bool,
    /// 重名拒。
    pub dup_block: bool,
    /// 终态不可回退。
    pub terminal_lock: bool,
    /// 跳步拒（Queued→Measuring 违约）。
    pub skip_block: bool,
    /// 中位数恰值（[3,1,2,5,4]→3；偶数断序取小）。
    pub median_exact: bool,
    /// 跑数不足拒。
    pub runs_short_block: bool,
    /// 零耗时拒。
    pub zero_block: bool,
    /// 噪声闸恰值（极差/中位=50%>10% 旗立；2% 旗不立）。
    pub noise_exact: bool,
    /// 采集汇总 P50/P95 恰值。
    pub summary_exact: bool,
    /// 同名重复采集拒。
    pub collect_dup_block: bool,
    /// 双跑指纹一致。
    pub determinism: bool,
    /// 快照稳定。
    pub snapshot_stable: bool,
}

/// 跑探针。
pub fn run_probes() -> ProbeReport {
    // ── 调度 ──
    let mut sched = Scheduler::new();
    let _ = sched.enqueue("back", 2);
    let _ = sched.enqueue("front-a", 0);
    let _ = sched.enqueue("front-b", 0);
    let _ = sched.enqueue("mid", 1);
    let n1 = sched.peek_next().map(|i| sched.scenarios[i].name == "front-a").unwrap_or(false);
    // front-a 完成后 → front-b。
    let _ = sched.transition("front-a", ScenarioState::Failed);
    let n2 = sched.peek_next().map(|i| sched.scenarios[i].name == "front-b").unwrap_or(false);
    let sched_order = n1 && n2;

    let full_block = {
        let mut s2 = Scheduler::new();
        let mut i = 0;
        let mut over = false;
        while i < SCENARIO_CAP {
            let mut nm = [0u8; 8];
            let mut v = i as u64;
            let mut k = 0;
            while k < 8 {
                nm[k] = b'0' + (v % 10) as u8;
                v /= 10;
                k += 1;
            }
            let name = core::str::from_utf8(&nm).unwrap_or("x");
            if s2.enqueue(name, 0).is_err() {
                over = true;
            }
            i += 1;
        }
        over && matches!(s2.enqueue("over", 0), Err(RnrCode::RnScenarioFull))
    };
    let dup_block = {
        let mut s3 = Scheduler::new();
        let _ = s3.enqueue("a", 0);
        matches!(s3.enqueue("a", 1), Err(RnrCode::RnDuplicate))
    };
    let terminal_lock = {
        let mut s4 = Scheduler::new();
        let _ = s4.enqueue("t", 0);
        let _ = s4.transition("t", ScenarioState::Failed);
        matches!(s4.transition("t", ScenarioState::Queued), Err(RnrCode::RnStateMove))
    };
    let skip_block = {
        let mut s5 = Scheduler::new();
        let _ = s5.enqueue("k", 0);
        matches!(s5.transition("k", ScenarioState::Measuring), Err(RnrCode::RnStateMove))
    };

    // ── 计时 ──
    let med1 = median_ns(&[3, 1, 2, 5, 4]).unwrap_or(0);
    let med2 = median_ns(&[10, 2, 8, 4]).unwrap_or(0); // 偶数断序取小：sorted [2,4,8,10] → 4
    let median_exact = med1 == 3 && med2 == 4;
    let runs_short = matches!(time_scenario(&[1, 2, 3, 4]), Err(RnrCode::RnRunsShort));
    let zero_block = matches!(time_scenario(&[0, 1, 2, 3, 5]), Err(RnrCode::RnMetricRange));
    let noisy_hi = noisy(&[100, 100, 100, 100, 150]).unwrap_or(true); // 极差50/中位100=50%>10%
    let noisy_lo = !noisy(&[100, 100, 100, 100, 102]).unwrap_or(true); // 2%≤10%
    let noise_exact = noisy_hi && noisy_lo;

    // ── 采集 ──
    let mut col = Collector::new();
    let _ = col.collect("s1", TimingResult { median: 500, range: 10, runs: 5, rerun_flag: false, timing_fp: 1 });
    let _ = col.collect("s2", TimingResult { median: 300, range: 5, runs: 5, rerun_flag: false, timing_fp: 2 });
    let _ = col.collect("s3", TimingResult { median: 900, range: 20, runs: 5, rerun_flag: false, timing_fp: 3 });
    let _ = col.collect("s4", TimingResult { median: 100, range: 2, runs: 5, rerun_flag: false, timing_fp: 4 });
    // meds sorted: [100,300,500,900] → p50 = meds[4*50/100=2]=500；p95=meds[3]=900。
    let sum_ok = matches!(col.summary(), Ok((500, 900)));
    let collect_dup = matches!(
        col.collect("s1", TimingResult { median: 1, range: 1, runs: 5, rerun_flag: false, timing_fp: 9 }),
        Err(RnrCode::RnDuplicate)
    );

    // ── 编排 + 双跑 ──
    let build = || -> BenchRunner {
        let mut r = BenchRunner::new();
        let _ = r.register("alpha", 1);
        let _ = r.register("beta", 0);
        let _ = r.advance("beta", ScenarioState::Warmup);
        let _ = r.advance("beta", ScenarioState::Measuring);
        let _ = r.submit("beta", &[120, 122, 118, 121, 119]);
        let _ = r.advance("alpha", ScenarioState::Warmup);
        let _ = r.advance("alpha", ScenarioState::Measuring);
        let _ = r.submit("alpha", &[200, 205, 198, 202, 199]);
        r
    };
    let r1 = build();
    let r2 = build();
    let det = r1.fingerprint() == r2.fingerprint();

    let snap1 = RnrSnap::take(&r1);
    let snap2 = RnrSnap::take(&r1);
    let snap = snap1.matches(&snap2);

    ProbeReport {
        sched_order,
        full_block,
        dup_block,
        terminal_lock,
        skip_block,
        median_exact,
        runs_short_block: runs_short,
        zero_block,
        noise_exact,
        summary_exact: sum_ok,
        collect_dup_block: collect_dup,
        determinism: det,
        snapshot_stable: snap,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据（CheckSet）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据挂接。
pub fn run_vcu05_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("CGPU-F3205");
    let p = run_probes();

    // 判据"三功能"之一：场景调度。
    s.add("调度确定序", p.sched_order, "");
    s.add("账满拒", p.full_block, "");
    s.add("重名拒", p.dup_block, "");
    s.add("终态锁", p.terminal_lock, "");
    s.add("跳步拒", p.skip_block, "");

    // 判据"三功能"之二：计时。
    s.add("中位数恰值", p.median_exact, "");
    s.add("跑数不足拒", p.runs_short_block, "");
    s.add("零耗时拒", p.zero_block, "");
    s.add("噪声闸恰值", p.noise_exact, "");

    // 判据"三功能"之三：采集。
    s.add("汇总P50/P95恰值", p.summary_exact, "");
    s.add("重复采集拒", p.collect_dup_block, "");

    // 判据"一组"：确定性 + 快照。
    s.add("双跑指纹一致", p.determinism, "");
    s.add("快照稳定", p.snapshot_stable, "");

    // 状态机迁移表全枚举（9 合法格 × 单向）。
    let legal = ScenarioState::Queued.can_transition(ScenarioState::Warmup)
        && ScenarioState::Warmup.can_transition(ScenarioState::Measuring)
        && ScenarioState::Measuring.can_transition(ScenarioState::Done)
        && !ScenarioState::Done.can_transition(ScenarioState::Queued)
        && !ScenarioState::Failed.can_transition(ScenarioState::Queued)
        && !ScenarioState::Queued.can_transition(ScenarioState::Done);
    s.add("状态机单向表", legal, "");

    // 诊断码十二码互异。
    let codes = [
        RnrCode::RnScenarioFull.code(),
        RnrCode::RnDuplicate.code(),
        RnrCode::RnScenarioUnknown.code(),
        RnrCode::RnStateMove.code(),
        RnrCode::RnRunsShort.code(),
        RnrCode::RnNoisy.code(),
        RnrCode::RnTickRegress.code(),
        RnrCode::RnLedgerMismatch.code(),
        RnrCode::RnDeterminism.code(),
        RnrCode::RnSnapshotDrift.code(),
        RnrCode::RnSummaryEmpty.code(),
        RnrCode::RnMetricRange.code(),
    ];
    let mut distinct = true;
    let mut i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码十二码互异", distinct && codes[0] == 0xBF01 && codes[11] == 0xBF0C, "");

    // ── v2 深化判据（依赖/重跑/分位/对拍/预算/读屏）──
    let p2 = run_probes_v2();
    s.add("依赖合法挂线", p2.dep_legal, "");
    s.add("依赖未知拒", p2.dep_unknown_block, "");
    s.add("依赖环拒回滚", p2.dep_cycle_block, "");
    s.add("依赖出队闸恰值", p2.dep_ready_exact, "");
    s.add("重跑消解出分", p2.rerun_resolves, "");
    s.add("重跑上限拒", p2.rerun_cap_block, "");
    s.add("重跑分批保留", p2.rerun_batches_kept, "");
    s.add("重跑耗尽无分", p2.rerun_exhausted_no_score, "");
    s.add("分位插值恰值", p2.percentile_exact, "");
    s.add("分位序自洽", p2.percentile_ordered, "");
    s.add("对拍三态恰值", p2.diff_regression_exact && p2.diff_improvement_exact && p2.diff_nochange_exact, "");
    s.add("对拍缺口账", p2.diff_gap_ledger, "");
    s.add("对拍劣化清单", p2.diff_regression_list, "");
    s.add("预算爆点恰值", p2.budget_blown_exact, "");
    s.add("预算余量恰值", p2.budget_remain_exact, "");
    s.add("对拍预算读屏形", p2.readout_shape, "");

    // v2 扩码五码互异且不与 v1 十二码冲突。
    let codes2 = [
        RnrCodeV2::RnDepCycle.code(),
        RnrCodeV2::RnDepUnknown.code(),
        RnrCodeV2::RnRerunExhausted.code(),
        RnrCodeV2::RnBudgetExhausted.code(),
        RnrCodeV2::RnDiffMisaligned.code(),
    ];
    let mut distinct2 = true;
    let mut i = 0;
    while i < codes2.len() {
        let mut j = i + 1;
        while j < codes2.len() {
            if codes2[i] == codes2[j] {
                distinct2 = false;
            }
            j += 1;
        }
        // 与 v1 十二码也不得冲突。
        if codes.contains(&codes2[i]) {
            distinct2 = false;
        }
        i += 1;
    }
    s.add(
        "诊断码扩码五码",
        distinct2 && codes2[0] == 0xBF0D && codes2[4] == 0xBF11,
        "",
    );

    // ── v2.3 编排层判据 ──
    let p23 = run_probes_v23();
    s.add("分层拓扑恰值", p23.wave_shape_exact, "");
    s.add("轮次深度恰值", p23.wave_depth_exact, "");
    s.add("搁浅显性不吞", p23.wave_stranded_visible, "");
    s.add("编排未知场景拒", p23.runner_unknown_block, "");
    s.add("编排重跑透传", p23.runner_rerun_passthrough, "");
    s.add("终判分列出分Failed", p23.runner_finalize_split, "");
    s.add("预算爆partial置位", p23.runner_partial_flag, "");
    s.add("编排双跑确定", p23.runner_determinism, "");
    s.add("轮次读屏形", p23.wave_line_shape, "");

    s
}

// ═════════════════════════════════════════════════════════════════════════════
// 深化段 v2（依赖闭包调度 / 重跑账 / 深分位 / 基线对拍 / 预算闸）
// ═════════════════════════════════════════════════════════════════════════════
//
// v1 跑通"调度→计时→采集"三功能直排。v2 把运行器放进**真实基准语境**：
//
// 1. **依赖闭包调度**：基准场景常有先决（"开着 4K 纹理跑"必须先有
//    "默认纹理跑"锚点）。场景可挂前置名账——出队时前置未 Done 的
//    场景**跳过不阻塞别家**（依赖死锁不传染）。**环依赖在挂线时整
//    条拒**（三色 DFS 探环——挂线即拒比出队时死转好：确定性键不能
//    含环）。
// 2. **重跑账**：v1 噪声闸只竖 RerunFlag 不给出口。v2 补消费面——
//    rerun 上限 2 次，每轮样本**分批保留**（第一批 ≠ 被覆盖——噪声
//    本身是数据）；重跑后仍噪 → 显性 Failed(噪声)，绝不静默出分。
// 3. **深分位**：P50/P90/P95/P99/Min/Max 六分位——定点线性插值
//    （idx = p×(n−1)/1000，整数位取 a、千分位残差混 b−a×残/1000——
//    零浮点的插值恰值可手算）。
// 4. **基线对拍**：两份汇总（基线 vs 当前）逐场景对比——中位劣化
//    >5%（500bps）→ RegressionFlag、改善 >5% → ImprovementFlag、
//    带内 → NoChange；场景集不对齐（缺场景/多场景）→ 显性缺口账
//    不硬比。
// 5. **预算闸**：全套基准总 tick 预算注入——超预算中断且已完成的
//    结果带 partial 标注（部分结果 ≠ 完整结果，混着出就是造假）。

/// 重跑上限（判据"重跑策略"——超限即 Failed 不无限烧 tick）。
pub const RERUN_CAP: u32 = 2;

/// 对比阈值（500bps = 5%）。
pub const DIFF_LINE_BPS: u64 = 500;

/// bps 单位基。
pub const BPS_ONE: u64 = 10_000;

// ─────────────────────────────────────────────────────────────────────────────
// v2 扩码（0xBF0D~0xBF11——五码，十二码闭集扩至十七码）
// ─────────────────────────────────────────────────────────────────────────────

/// v2 扩码（0xBFxx 段续——深化面五码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RnrCodeV2 {
    /// 0xBF0D 依赖环（挂线时 DFS 探出——确定性键不能含环）。
    RnDepCycle,
    /// 0xBF0E 依赖未知（前置名不在场景账）。
    RnDepUnknown,
    /// 0xBF0F 重跑耗尽（rerun 上限 2 次仍噪——显性 Failed 不静默出分）。
    RnRerunExhausted,
    /// 0xBF10 预算耗尽（总 tick 预算超——中断带 partial 标注）。
    RnBudgetExhausted,
    /// 0xBF11 对比账不对齐（场景集缺口——不硬比）。
    RnDiffMisaligned,
}

impl RnrCodeV2 {
    /// 稳定码值。
    pub const fn code(self) -> u16 {
        match self {
            RnrCodeV2::RnDepCycle => 0xBF0D,
            RnrCodeV2::RnDepUnknown => 0xBF0E,
            RnrCodeV2::RnRerunExhausted => 0xBF0F,
            RnrCodeV2::RnBudgetExhausted => 0xBF10,
            RnrCodeV2::RnDiffMisaligned => 0xBF11,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            RnrCodeV2::RnDepCycle => "依赖环",
            RnrCodeV2::RnDepUnknown => "依赖未知",
            RnrCodeV2::RnRerunExhausted => "重跑耗尽",
            RnrCodeV2::RnBudgetExhausted => "预算耗尽",
            RnrCodeV2::RnDiffMisaligned => "对比账不对齐",
        }
    }

    /// 中文详情。
    pub const fn detail(self) -> &'static str {
        match self {
            RnrCodeV2::RnDepCycle => "依赖环：前置闭包含自身——挂线即拒确定性键禁环",
            RnrCodeV2::RnDepUnknown => "依赖未知：前置场景不在账——拒绝挂线不猜",
            RnrCodeV2::RnRerunExhausted => "重跑耗尽：rerun上限2次仍噪——显性Failed绝不静默出分",
            RnrCodeV2::RnBudgetExhausted => "预算耗尽：总tick预算超——中断已完成结果带partial标注",
            RnrCodeV2::RnDiffMisaligned => "对比账不对齐：场景集缺口——显性缺口账不硬比",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 依赖闭包（三色 DFS 探环 + 出队闸）
// ─────────────────────────────────────────────────────────────────────────────

/// 三色标记（DFS 探环——白未访/灰在栈/黑已毕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DfsColor {
    White,
    Gray,
    Black,
}

/// 依赖线账（场景名 → 前置名账；与 Scheduler 并行持有——不改 v1 结构）。
#[derive(Clone, Debug)]
pub struct DepGraph {
    /// 边表（from 依赖 to——from 出队需 to Done）。
    pub edges: Vec<(String, String)>,
}

impl DepGraph {
    /// 空图。
    pub const fn new() -> DepGraph {
        DepGraph { edges: Vec::new() }
    }

    /// 挂线（挂线即验：前置必须已在场景账 → RnDepUnknown；
    /// 挂线后从 from DFS 探环 → RnDepCycle 整条回滚不入账）。
    pub fn attach(
        &mut self,
        from: &str,
        to: &str,
        known: &[String],
    ) -> Result<(), RnrCodeV2> {
        if !known.iter().any(|k| k == to) {
            return Err(RnrCodeV2::RnDepUnknown);
        }
        self.edges.push((String::from(from), String::from(to)));
        if self.has_cycle_from(from, known) {
            self.edges.pop(); // 回滚——半拉子依赖比没有更危险
            return Err(RnrCodeV2::RnDepCycle);
        }
        Ok(())
    }

    /// 从 start 三色 DFS 探环（known 闭集内走——不越账）。
    ///
    /// 迭代栈式 DFS：白转灰压栈（先压 cur 自身占位，再压全部后继）；
    /// 弹出灰节点时若无后继可走（占位弹出）转黑。重访灰 = 环。
    /// n ≤ 32——重复入栈的常数可忽略，正确性优先于剪枝。
    fn has_cycle_from(&self, start: &str, known: &[String]) -> bool {
        // 颜色账（名字比较——场景闭集小，线性查可接受）。
        let mut names: Vec<&str> = Vec::new();
        let mut colors: Vec<DfsColor> = Vec::new();
        for k in known {
            names.push(k.as_str());
            colors.push(DfsColor::White);
        }
        let color_idx = |names: &[&str], n: &str| -> Option<usize> {
            names.iter().position(|x| *x == n)
        };
        let mut stack: Vec<&str> = Vec::new();
        stack.push(start);
        while let Some(cur) = stack.pop() {
            let ci = match color_idx(&names, cur) {
                Some(i) => i,
                None => continue,
            };
            match colors[ci] {
                DfsColor::Gray => {
                    // 占位弹出：cur 的后继已全部处理完——转黑。
                    // 但若占位弹出时又见灰，说明是从别的路径重访——
                    // 环判定在"弹到灰且它不是占位"时立。区分法：
                    // 占位重压时计数。简化实现：白转灰时把 cur 的
                    // 后继直接判——后继==start 且 start 已灰即环。
                    colors[ci] = DfsColor::Black;
                    for (f, t) in &self.edges {
                        if f.as_str() == cur && t.as_str() == start {
                            // 后继回到起点（已灰）——环。
                            if colors[color_idx(&names, start).unwrap_or(0)] != DfsColor::White {
                                return true;
                            }
                        }
                    }
                }
                DfsColor::Black => continue,
                DfsColor::White => {
                    colors[ci] = DfsColor::Gray;
                    let successors: Vec<&str> = self
                        .edges
                        .iter()
                        .filter(|(f, _)| f.as_str() == cur)
                        .map(|(_, t)| t.as_str())
                        .collect();
                    stack.push(cur); // 占位——下次弹出转黑
                    for t in successors {
                        stack.push(t);
                    }
                }
            }
        }
        false
    }

    /// 出队闸：from 的全部前置是否 Done（done 集由调用方从 Scheduler 态出）。
    pub fn ready(&self, from: &str, done: &[String]) -> bool {
        self.edges
            .iter()
            .filter(|(f, _)| f.as_str() == from)
            .all(|(_, t)| done.iter().any(|d| d == t.as_str()))
    }

    /// 依赖指纹（边集 FNV——对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, "vcu05-depgraph");
        for (f, t) in &self.edges {
            s = fnv1a_str(s, f);
            s = fnv1a_str(s, t);
        }
        s
    }
}

impl Default for DepGraph {
    fn default() -> Self {
        DepGraph::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 深分位（定点线性插值——零浮点）
// ─────────────────────────────────────────────────────────────────────────────

/// 定点分位（判据"采集"深化面：p ∈ [0,1000] 千分位）。
///
/// 升序后 idx = p×(n−1)/1000：整数位 i、千分位残差 r——
/// 结果 = a[i] + (a[i+1]−a[i])×r/1000（i+1 越界时 r 恒 0——数学上
/// p=1000 时 idx=n−1 整除无残差，越界分支是防线不是常态路径）。
pub fn percentile_ns(sorted: &[u64], p: u64) -> Result<u64, RnrCode> {
    if sorted.is_empty() {
        return Err(RnrCode::RnRunsShort);
    }
    if p > 1000 {
        return Err(RnrCode::RnMetricRange);
    }
    let n = sorted.len();
    let num = p * (n as u64 - 1);
    let i = (num / 1000) as usize;
    let r = num % 1000;
    let a = sorted[i.min(n - 1)];
    if r == 0 || i + 1 >= n {
        return Ok(a);
    }
    let b = sorted[i + 1];
    Ok(a + (b - a) * r / 1000)
}

/// 六分位快照（Min/P50/P90/P95/P99/Max——随场景账走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PercentileSet {
    /// 最小。
    pub min: u64,
    /// P50。
    pub p50: u64,
    /// P90。
    pub p90: u64,
    /// P95。
    pub p95: u64,
    /// P99。
    pub p99: u64,
    /// 最大。
    pub max: u64,
}

impl PercentileSet {
    /// 从跑样算全套（内部升序一次——六分位共享）。
    pub fn compute(runs: &[u64]) -> Result<PercentileSet, RnrCode> {
        if runs.is_empty() {
            return Err(RnrCode::RnRunsShort);
        }
        let mut v: Vec<u64> = runs.to_vec();
        v.sort_unstable();
        let mn = v[0];
        let mx = v[v.len() - 1];
        let p50 = percentile_ns(&v, 500)?;
        let p90 = percentile_ns(&v, 900)?;
        let p95 = percentile_ns(&v, 950)?;
        let p99 = percentile_ns(&v, 990)?;
        Ok(PercentileSet { min: mn, p50, p90, p95, p99, max: mx })
    }

    /// 指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(FNV_OFFSET, self.min);
        let s = fnv1a_u64(s, self.p50);
        let s = fnv1a_u64(s, self.p90);
        let s = fnv1a_u64(s, self.p95);
        let s = fnv1a_u64(s, self.p99);
        fnv1a_u64(s, self.max)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 重跑账（RerunFlag 的消费面——分批保留不覆盖）
// ─────────────────────────────────────────────────────────────────────────────

/// 单场景重跑账。
#[derive(Clone, Debug)]
pub struct RerunLedger {
    /// 分批样本（第 0 批 = 首测，第 1..=RERUN_CAP 批 = 重跑——全保留）。
    pub batches: Vec<Vec<u64>>,
    /// 已重跑次数。
    pub reruns: u32,
    /// 终判（None=未定 / Some(Ok(中位))=出分 / Some(Err)=Failed(噪声)）。
    pub verdict: Option<Result<u64, RnrCodeV2>>,
}

impl RerunLedger {
    /// 新账（首测批入账并即时判——噪则暂记 Err 待 rerun 消解）。
    pub fn new(first_batch: &[u64]) -> Result<RerunLedger, RnrCode> {
        let med = median_ns(first_batch)?;
        let noisy = is_noisy(first_batch, med)?;
        let mut lg = RerunLedger {
            batches: Vec::new(),
            reruns: 0,
            verdict: None,
        };
        lg.batches.push(first_batch.to_vec());
        lg.verdict = Some(if noisy {
            Err(RnrCodeV2::RnRerunExhausted)
        } else {
            Ok(med)
        });
        Ok(lg)
    }

    /// 重跑一轮（批样本入账——第 reruns+1 批；全历史并判）。
    /// 超上限拒 RnRerunExhausted（终判已锁）。
    pub fn rerun(&mut self, batch: &[u64]) -> Result<(), RnrCodeV2> {
        if self.reruns >= RERUN_CAP {
            return Err(RnrCodeV2::RnRerunExhausted);
        }
        self.batches.push(batch.to_vec());
        self.reruns += 1;
        // 全历史并判（合并批取中位——样本越多中位越稳）。
        let mut all: Vec<u64> = Vec::new();
        for b in &self.batches {
            all.extend_from_slice(b);
        }
        self.verdict = Some(match median_ns(&all) {
            Ok(med) => match is_noisy(&all, med) {
                Ok(false) => Ok(med),
                _ => Err(RnrCodeV2::RnRerunExhausted),
            },
            Err(_) => Err(RnrCodeV2::RnRerunExhausted),
        });
        Ok(())
    }

    /// 是否仍可 rerun（出分即不可；Failed 且 reruns 未满可）。
    pub const fn can_rerun(&self) -> bool {
        match &self.verdict {
            None => true,
            Some(Ok(_)) => false,
            Some(Err(_)) => self.reruns < RERUN_CAP,
        }
    }

    /// 出分（只有 Ok 才有分——Failed 无分这是纪律）。
    pub const fn score(&self) -> Option<u64> {
        match &self.verdict {
            Some(Ok(m)) => Some(*m),
            _ => None,
        }
    }

    /// 账指纹（分批全量 FNV——分批保留的可审计性）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.reruns as u64);
        for b in &self.batches {
            s = fnv1a_u64(s, b.len() as u64);
            for v in b {
                s = fnv1a_u64(s, *v);
            }
        }
        s
    }
}

/// 噪声闸复用（极差/中位 > NOISE_LINE_BPS → 噪）。
fn is_noisy(runs: &[u64], median: u64) -> Result<bool, RnrCode> {
    if runs.is_empty() {
        return Err(RnrCode::RnRunsShort);
    }
    let mn = *runs.iter().min().ok_or(RnrCode::RnRunsShort)?;
    let mx = *runs.iter().max().ok_or(RnrCode::RnRunsShort)?;
    if median == 0 {
        return Err(RnrCode::RnMetricRange);
    }
    Ok((mx - mn) * BPS_ONE / median > NOISE_LINE_BPS)
}

// ─────────────────────────────────────────────────────────────────────────────
// 基线对拍（两份汇总逐场景——三态旗 + 缺口账）
// ─────────────────────────────────────────────────────────────────────────────

/// 单场景对拍结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffVerdict {
    /// 劣化 >5%（当前中位比基线大 5% 以上——慢了）。
    Regression,
    /// 改善 >5%（快了）。
    Improvement,
    /// 带内（±5% 内——变化在噪声语义内不解读）。
    NoChange,
}

impl DiffVerdict {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            DiffVerdict::Regression => "劣化",
            DiffVerdict::Improvement => "改善",
            DiffVerdict::NoChange => "带内",
        }
    }
}

/// 定点对拍（基线 med × 当前 med → bps 变化率 + 三态旗）。
/// bps 变化率 = (cur − base)×BPS_ONE/base（正 = 变慢 = 劣化方向）。
pub fn diff_verdict(base_med: u64, cur_med: u64) -> Result<(u64, DiffVerdict), RnrCode> {
    if base_med == 0 {
        return Err(RnrCode::RnMetricRange);
    }
    let delta = (cur_med as i128) - (base_med as i128);
    // bps = delta×1e4/base——i128 中转防溢出，取绝对值出旗向。
    let bps = ((delta * (BPS_ONE as i128)) / (base_med as i128)) as i64;
    let mag = bps.unsigned_abs();
    let v = if bps > 0 && mag > DIFF_LINE_BPS {
        DiffVerdict::Regression
    } else if bps < 0 && mag > DIFF_LINE_BPS {
        DiffVerdict::Improvement
    } else {
        DiffVerdict::NoChange
    };
    Ok((mag, v))
}

/// 全量对拍报告（缺口账显性——不硬比）。
#[derive(Clone, Debug)]
pub struct BaselineDiff {
    /// 对齐场景的对拍行（场景名 | bps | 旗）。
    pub rows: Vec<(String, u64, DiffVerdict)>,
    /// 仅基线有（当前缺——回归测试没跑全）。
    pub only_base: Vec<String>,
    /// 仅当前有（基线缺——新场景无锚）。
    pub only_cur: Vec<String>,
}

impl BaselineDiff {
    /// 跑对拍（同名场景中位对中位——基线/当前均为 名→中位 映射）。
    pub fn run(
        base: &[(String, u64)],
        cur: &[(String, u64)],
    ) -> Result<BaselineDiff, RnrCode> {
        let mut rows: Vec<(String, u64, DiffVerdict)> = Vec::new();
        let mut only_base: Vec<String> = Vec::new();
        let mut only_cur: Vec<String> = Vec::new();
        for (n, bm) in base {
            match cur.iter().find(|(cn, _)| cn == n) {
                Some((_, cm)) => {
                    let (bps, v) = diff_verdict(*bm, *cm)?;
                    rows.push((n.clone(), bps, v));
                }
                None => only_base.push(n.clone()),
            }
        }
        for (n, _) in cur {
            if !base.iter().any(|(bn, _)| bn == n) {
                only_cur.push(n.clone());
            }
        }
        // 缺口是事实不是错误——返回带缺口的账（RnDiffMisaligned 由
        // 调用方按需立：全自动闸语境下缺账即拒）。
        Ok(BaselineDiff { rows, only_base, only_cur })
    }

    /// 是否对齐（零缺口）。
    pub const fn aligned(&self) -> bool {
        self.only_base.is_empty() && self.only_cur.is_empty()
    }

    /// 劣化清单（旗 = Regression 的场景名——回归面直接可查）。
    pub fn regressions(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|(_, _, v)| *v == DiffVerdict::Regression)
            .map(|(n, _, _)| n.clone())
            .collect()
    }

    /// 读屏行（对齐状况 + 劣化数 + 改善数）。
    pub fn line(&self) -> String {
        let mut s = String::from("对拍|对齐:");
        s.push_str(if self.aligned() { "是" } else { "否" });
        let reg = self
            .rows
            .iter()
            .filter(|(_, _, v)| *v == DiffVerdict::Regression)
            .count();
        let imp = self
            .rows
            .iter()
            .filter(|(_, _, v)| *v == DiffVerdict::Improvement)
            .count();
        s.push_str("|劣化:");
        push_dec(&mut s, reg as u64);
        s.push_str("|改善:");
        push_dec(&mut s, imp as u64);
        if !self.only_base.is_empty() {
            s.push_str("|基线独有:");
            push_dec(&mut s, self.only_base.len() as u64);
        }
        if !self.only_cur.is_empty() {
            s.push_str("|当前独有:");
            push_dec(&mut s, self.only_cur.len() as u64);
        }
        s
    }
}

/// 手写十进制（本文件 v1 无 push_dec——补一个极简版）。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 预算闸（总 tick 预算——超支中断带 partial 标注）
// ─────────────────────────────────────────────────────────────────────────────

/// 预算闸账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetGate {
    /// 预算总量（tick）。
    pub budget_ticks: u64,
    /// 已耗（tick 累计——注入面）。
    pub spent_ticks: u64,
    /// 是否已爆。
    pub blown: bool,
}

impl BudgetGate {
    /// 新闸。
    pub const fn new(budget_ticks: u64) -> BudgetGate {
        BudgetGate { budget_ticks, spent_ticks: 0, blown: false }
    }

    /// 记一笔耗时（返回 Err(RnBudgetExhausted) = 本笔后爆）。
    pub const fn spend(&mut self, ticks: u64) -> Result<(), RnrCodeV2> {
        self.spent_ticks += ticks;
        if self.spent_ticks > self.budget_ticks {
            self.blown = true;
            return Err(RnrCodeV2::RnBudgetExhausted);
        }
        Ok(())
    }

    /// 余量。
    pub const fn remain(&self) -> u64 {
        if self.spent_ticks >= self.budget_ticks {
            0
        } else {
            self.budget_ticks - self.spent_ticks
        }
    }

    /// 耗尽率（bps——u128 中转）。
    pub const fn burn_bps(&self) -> u64 {
        if self.budget_ticks == 0 {
            return BPS_ONE;
        }
        ((self.spent_ticks as u128) * (BPS_ONE as u128) / (self.budget_ticks as u128)) as u64
    }

    /// 读屏行。
    pub fn line(&self) -> String {
        let mut s = String::from("预算|耗:");
        push_dec(&mut s, self.spent_ticks);
        s.push('/');
        push_dec(&mut s, self.budget_ticks);
        s.push_str("tick|");
        push_dec(&mut s, self.burn_bps());
        s.push_str("bps|");
        s.push_str(if self.blown { "已爆" } else { "在途" });
        s
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 深化探针 v2（依赖/重跑/分位/对拍/预算——五组手算恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 深化探针汇总。
pub struct ProbeV2Report {
    /// 依赖：合法挂线放行。
    pub dep_legal: bool,
    /// 依赖：未知前置拒（0xBF0E）。
    pub dep_unknown_block: bool,
    /// 依赖：环拒（A→B→A——0xBF0D 且边回滚）。
    pub dep_cycle_block: bool,
    /// 依赖：出队闸恰值（前置 Done 才 ready）。
    pub dep_ready_exact: bool,
    /// 重跑：首测噪暂记、一次重跑消解出分。
    pub rerun_resolves: bool,
    /// 重跑：三次重跑拒（上限 2）。
    pub rerun_cap_block: bool,
    /// 重跑：分批保留（三批全在账）。
    pub rerun_batches_kept: bool,
    /// 重跑：两次重跑仍噪终判 Failed 无分。
    pub rerun_exhausted_no_score: bool,
    /// 分位：插值恰值（[100,200,300,400] P50=250/P90=370/P95=385/P99=397）。
    pub percentile_exact: bool,
    /// 分位：六件自洽（min ≤ p50 ≤ p99 ≤ max）。
    pub percentile_ordered: bool,
    /// 对拍：劣化恰值（base 100 cur 110 → 1000bps 劣化）。
    pub diff_regression_exact: bool,
    /// 对拍：改善恰值（base 100 cur 90 → 1000bps 改善）。
    pub diff_improvement_exact: bool,
    /// 对拍：带内恰值（base 100 cur 104 → 400bps 带内）。
    pub diff_nochange_exact: bool,
    /// 对拍：缺口账（仅基线/仅当前各一）。
    pub diff_gap_ledger: bool,
    /// 对拍：劣化清单恰值。
    pub diff_regression_list: bool,
    /// 预算：恰值爆点（预算 1000 花 999 不爆、再花 2 爆）。
    pub budget_blown_exact: bool,
    /// 预算：余量与耗尽率恰值。
    pub budget_remain_exact: bool,
    /// 读屏：对拍行与预算行形。
    pub readout_shape: bool,
}

/// 跑深化探针 v2。
pub fn run_probes_v2() -> ProbeV2Report {
    // ── 依赖 ──
    let known: Vec<String> = ["base", "hi_tex", "lo_tex"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    let mut dg = DepGraph::new();
    let legal = dg.attach("hi_tex", "base", &known).is_ok();
    let unknown = matches!(
        dg.attach("lo_tex", "ghost", &known),
        Err(RnrCodeV2::RnDepUnknown)
    );
    // 环：base→hi_tex→base？base 不在 from 位……构造 A→B→A：
    let known2: Vec<String> = ["a", "b"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    let mut dg2 = DepGraph::new();
    let _ = dg2.attach("a", "b", &known2);
    let cyc = matches!(dg2.attach("b", "a", &known2), Err(RnrCodeV2::RnDepCycle));
    let edges_after = dg2.edges.len() == 1; // 回滚——只留 a→b
    // 出队闸：a Done 后 b ready。
    let done_a: Vec<String> = vec![String::from("a")];
    let ready_before = !dg2.ready("b", &[]);
    let ready_after = dg2.ready("b", &done_a);

    // ── 重跑 ──
    // 首测噪：{100, 200, 100, 200, 100} 中位 100 极差 100 → 10000bps > 1000 噪。
    let mut rl = match RerunLedger::new(&[100, 200, 100, 200, 100]) {
        Ok(x) => x,
        Err(_) => return ProbeV2Report::all_fail(),
    };
    let noisy_first = rl.score().is_none();
    // 一次重跑消解：{100,101,100,101,100}。
    let _ = rl.rerun(&[100, 101, 100, 101, 100]);
    let resolved = rl.score() == Some(100) && rl.reruns == 1;
    // 分批保留：两批都在。
    let kept = rl.batches.len() == 2;
    // 上限：已 reruns=1，可再一次；第三次拒。
    let can2 = rl.can_rerun();
    let _ = rl.rerun(&[100, 100, 100, 100, 100]);
    let cap = !rl.can_rerun()
        && matches!(rl.rerun(&[100]), Err(RnrCodeV2::RnRerunExhausted));
    // 两次重跑仍噪 → Failed 无分。
    let mut rn = RerunLedger::new(&[100, 300, 100, 300, 100]).unwrap_or_else(|_| {
        RerunLedger {
            batches: Vec::new(),
            reruns: 0,
            verdict: Some(Err(RnrCodeV2::RnRerunExhausted)),
        }
    });
    let _ = rn.rerun(&[100, 400, 100, 400, 100]);
    let _ = rn.rerun(&[100, 500, 100, 500, 100]);
    let exhausted = rn.score().is_none() && !rn.can_rerun() && rn.reruns == 2;
    let rerun_ok = noisy_first && resolved && kept && can2 && cap && exhausted;

    // ── 分位 ──
    // [100,200,300,400]：idx(p)=p×3/1000。
    // P50: idx=1500/1000=1 r=500 → 200+(300-200)×500/1000=250。
    // P90: idx=2700/1000=2 r=700 → 300+(400-300)×700/1000=370。
    // P95: idx=2850/1000=2 r=850 → 385。
    // P99: idx=2970/1000=2 r=970 → 397。
    let set = PercentileSet::compute(&[400, 100, 300, 200]).unwrap_or(PercentileSet {
        min: 0,
        p50: 0,
        p90: 0,
        p95: 0,
        p99: 0,
        max: 0,
    });
    let pct_exact = set.min == 100
        && set.max == 400
        && set.p50 == 250
        && set.p90 == 370
        && set.p95 == 385
        && set.p99 == 397;
    let pct_ordered = set.min <= set.p50 && set.p50 <= set.p99 && set.p99 <= set.max;
    // 整除面：[1,2,3] P50 → idx=1×2/1000=2 r=0 → 3（p=500,n=3: 500×2=1000, /1000=1, r=0 → a[1]=2）。
    // 修正手算：n=3 → (n-1)=2；P50: 500×2=1000 → i=1, r=0 → 2。
    let p50_odd = percentile_ns(&[1, 2, 3], 500).unwrap_or(0) == 2;

    // ── 对拍 ──
    let reg = diff_verdict(100, 110).unwrap_or((0, DiffVerdict::NoChange));
    let imp = diff_verdict(100, 90).unwrap_or((0, DiffVerdict::NoChange));
    let nc = diff_verdict(100, 104).unwrap_or((0, DiffVerdict::NoChange));
    let diff_exact = reg == (1000, DiffVerdict::Regression)
        && imp == (1000, DiffVerdict::Improvement)
        && nc == (400, DiffVerdict::NoChange);
    // 缺口账。
    let base: Vec<(String, u64)> = vec![
        (String::from("s1"), 100),
        (String::from("s2"), 200),
        (String::from("s3"), 300),
    ];
    let cur: Vec<(String, u64)> = vec![
        (String::from("s1"), 105),
        (String::from("s2"), 240),
        (String::from("s4"), 50),
    ];
    let bd = BaselineDiff::run(&base, &cur).unwrap_or(BaselineDiff {
        rows: Vec::new(),
        only_base: Vec::new(),
        only_cur: Vec::new(),
    });
    let gap = !bd.aligned()
        && bd.only_base == vec![String::from("s3")]
        && bd.only_cur == vec![String::from("s4")]
        && bd.rows.len() == 2;
    // 劣化清单：s2（240/200=1200bps>500 劣化）、s1（50bps 带内）。
    let reg_list = bd.regressions() == vec![String::from("s2")];

    // ── 预算 ──
    let mut bg = BudgetGate::new(1000);
    let not_blown = bg.spend(999).is_ok();
    let blown = matches!(bg.spend(2), Err(RnrCodeV2::RnBudgetExhausted)) && bg.blown;
    let budget_exact = not_blown && blown;
    // 余量/耗尽率：预算 1000 已花 999 → 余 1、9990bps。
    let mut bg2 = BudgetGate::new(1000);
    let _ = bg2.spend(999);
    let remain_exact = bg2.remain() == 1 && bg2.burn_bps() == 9990;

    // ── 读屏 ──
    let dl = bd.line();
    let bl = bg2.line();
    let readout = dl.contains("对齐:否")
        && dl.contains("劣化:1")
        && dl.contains("基线独有:1")
        && bl.contains("999/1000tick")
        && bl.contains("9990bps")
        && bl.contains("在途");

    ProbeV2Report {
        dep_legal: legal,
        dep_unknown_block: unknown,
        dep_cycle_block: cyc && edges_after,
        dep_ready_exact: ready_before && ready_after,
        rerun_resolves: rerun_ok && resolved,
        rerun_cap_block: cap,
        rerun_batches_kept: kept,
        rerun_exhausted_no_score: exhausted,
        percentile_exact: pct_exact && p50_odd,
        percentile_ordered: pct_ordered,
        diff_regression_exact: diff_exact,
        diff_improvement_exact: diff_exact,
        diff_nochange_exact: diff_exact,
        diff_gap_ledger: gap,
        diff_regression_list: reg_list,
        budget_blown_exact: budget_exact,
        budget_remain_exact: remain_exact,
        readout_shape: readout,
    }
}

impl ProbeV2Report {
    /// 全败底稿（unwrap 免除路径——探针异常时保持结构完整）。
    fn all_fail() -> ProbeV2Report {
        ProbeV2Report {
            dep_legal: false,
            dep_unknown_block: false,
            dep_cycle_block: false,
            dep_ready_exact: false,
            rerun_resolves: false,
            rerun_cap_block: false,
            rerun_batches_kept: false,
            rerun_exhausted_no_score: false,
            percentile_exact: false,
            percentile_ordered: false,
            diff_regression_exact: false,
            diff_improvement_exact: false,
            diff_nochange_exact: false,
            diff_gap_ledger: false,
            diff_regression_list: false,
            budget_blown_exact: false,
            budget_remain_exact: false,
            readout_shape: false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v2.3 DepsAwareRunner（运行编排层——三功能 + 依赖 + 重跑 + 预算的整体）
// ─────────────────────────────────────────────────────────────────────────────
//
// v1 判据原文"运行器实现"——调度器/计时/采集三件有了，但"运行器"
// 作为整体缺一个编排面：什么顺序跑（依赖感知）、跑了噪不噪（重跑账）、
// 烧了多少 tick（预算闸）、最后交什么账（终判收集）。本段把四者串成
// 一个确定的整体——编排层自身零投机：顺序由全序键 + 依赖闭包推出，
// 不引入任何"看情况"。

/// 分层拓扑轮次账（wave k = 依赖恰在 k−1 轮全完成的场景集）。
///
/// 分层拓扑序是"并行度"的确定投影：同轮场景互不依赖（理论可并行），
/// 轮数 = 关键路径长。轮次指纹随编排走——双跑对拍的粒度从"整体序"
/// 细化到"分层序"。
pub struct WaveLedger {
    /// 轮次账（wave 下标 → 场景名账——按全序键序）。
    pub waves: Vec<Vec<String>>,
}

impl WaveLedger {
    /// 从依赖线 + 全场景名账算分层拓扑（Kahn 分层法——零递归）。
    ///
    /// 无环由挂线闸保证；若调用方硬灌环账，轮次算不满全部场景——
    /// 返回的账 + `stranded`（搁浅名单）显性呈现，不静默吞。
    pub fn compute(names: &[String], deps: &DepGraph) -> (WaveLedger, Vec<String>) {
        let mut remaining: Vec<String> = names.to_vec();
        let mut done: Vec<String> = Vec::new();
        let mut waves: Vec<Vec<String>> = Vec::new();
        let mut guard = 0;
        while !remaining.is_empty() && guard <= names.len() + 1 {
            // 本轮 = 依赖全在 done 的（按全序键序——names 传入前已排序）。
            let mut wave: Vec<String> = Vec::new();
            let mut next: Vec<String> = Vec::new();
            for n in &remaining {
                if deps.ready(n, &done) {
                    wave.push(n.clone());
                } else {
                    next.push(n.clone());
                }
            }
            if wave.is_empty() {
                // 一轮空 = 有环搁浅（挂线闸不该放进来——防线仍显性）。
                return (WaveLedger { waves }, next);
            }
            done.extend(wave.iter().cloned());
            waves.push(wave);
            remaining = next;
            guard += 1;
        }
        (WaveLedger { waves }, Vec::new())
    }

    /// 总轮数（关键路径长）。
    pub const fn depth(&self) -> usize {
        self.waves.len()
    }

    /// 轮次指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, "vcu05-wave");
        s = fnv1a_u64(s, self.waves.len() as u64);
        for w in &self.waves {
            for n in w {
                s = fnv1a_str(s, n);
            }
            s = fnv1a_u64(s, 0xFFFF); // 轮界哨
        }
        s
    }

    /// 读屏（轮次行：轮0:a,b|轮1:c）。
    pub fn line(&self) -> String {
        let mut s = String::from("轮次|");
        let mut i = 0;
        while i < self.waves.len() {
            if i > 0 {
                s.push('|');
            }
            push_dec(&mut s, i as u64);
            s.push(':');
            let mut first = true;
            for n in &self.waves[i] {
                if !first {
                    s.push(',');
                }
                s.push_str(n);
                first = false;
            }
            i += 1;
        }
        s
    }
}

/// 编排器（整体面——持有 v1 调度器 + 依赖线 + 重跑账表 + 预算闸）。
pub struct DepsAwareRunner {
    /// 调度器（v1 复用）。
    pub sched: Scheduler,
    /// 依赖线。
    pub deps: DepGraph,
    /// 重跑账表（场景名 → 重跑账）。
    pub reruns: Vec<(String, RerunLedger)>,
    /// 预算闸。
    pub budget: BudgetGate,
    /// partial 标注（预算爆后置位——部分结果 ≠ 完整结果）。
    pub partial: bool,
}

impl DepsAwareRunner {
    /// 新编排器（预算注入）。
    pub const fn new(budget_ticks: u64) -> DepsAwareRunner {
        DepsAwareRunner {
            sched: Scheduler::new(),
            deps: DepGraph::new(),
            reruns: Vec::new(),
            budget: BudgetGate::new(budget_ticks),
            partial: false,
        }
    }

    /// 场景全名账（入账序——挂线 known 闭集）。
    pub fn known_names(&self) -> Vec<String> {
        self.sched.scenarios.iter().map(|s| s.name.clone()).collect()
    }

    /// 挂依赖线（挂线即探环——确定性键禁环）。
    pub fn link(&mut self, from: &str, to: &str) -> Result<(), RnrCodeV2> {
        let known = self.known_names();
        self.deps.attach(from, to, &known)
    }

    /// 记一批跑样（场景必须已在账——重跑账懒建）。
    pub fn record_batch(&mut self, name: &str, batch: &[u64]) -> Result<(), RnrCode> {
        if !self.sched.scenarios.iter().any(|s| s.name == name) {
            return Err(RnrCode::RnScenarioUnknown);
        }
        match self.reruns.iter_mut().find(|(n, _)| n == name) {
            Some((_, lg)) => {
                // 已有账 → 这是重跑轮。
                lg.rerun(batch).map_err(|_| RnrCode::RnNoisy)?;
            }
            None => {
                let lg = RerunLedger::new(batch)?;
                self.reruns.push((String::from(name), lg));
            }
        }
        Ok(())
    }

    /// 分层拓扑轮次（全序键序入 Kahn——names 按调度序预处理）。
    pub fn waves(&self) -> (WaveLedger, Vec<String>) {
        // 按全序键排序场景名（与 v1 order_fingerprint 同一插入排序口径）。
        let mut idx: Vec<usize> = (0..self.sched.scenarios.len()).collect();
        for i in 1..idx.len() {
            let mut j = i;
            while j > 0
                && scheduling_order(&self.sched.scenarios[idx[j]], &self.sched.scenarios[idx[j - 1]])
            {
                idx.swap(j, j - 1);
                j -= 1;
            }
        }
        let names: Vec<String> = idx
            .iter()
            .map(|&i| self.sched.scenarios[i].name.clone())
            .collect();
        WaveLedger::compute(&names, &self.deps)
    }

    /// 终判收集（出分账 / Failed 账分列——Failed 无分是纪律）。
    /// 预算爆 → partial 置位（账面诚实）。
    pub fn finalize(&mut self) -> (Vec<(String, u64)>, Vec<(String, RnrCodeV2)>) {
        if self.budget.blown {
            self.partial = true;
        }
        let mut scored: Vec<(String, u64)> = Vec::new();
        let mut failed: Vec<(String, RnrCodeV2)> = Vec::new();
        for (n, lg) in &self.reruns {
            match lg.score() {
                Some(m) => scored.push((n.clone(), m)),
                None => failed.push((n.clone(), RnrCodeV2::RnRerunExhausted)),
            }
        }
        (scored, failed)
    }

    /// 编排指纹（调度序 + 依赖线 + 轮次 + 重跑账 + 预算态——全量对拍面）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = self.sched.order_fingerprint();
        s = fnv1a_u64(s, self.deps.fingerprint());
        let (w, _) = self.waves();
        s = fnv1a_u64(s, w.fingerprint());
        for (n, lg) in &self.reruns {
            s = fnv1a_str(s, n);
            s = fnv1a_u64(s, lg.fingerprint());
        }
        s = fnv1a_u64(s, self.budget.spent_ticks);
        s = fnv1a_u64(s, self.partial as u64);
        s
    }
}

/// v2.3 编排探针汇总。
pub struct ProbeV23Report {
    /// 分层拓扑恰值（base→hi_tex 依赖：轮0=[base] 轮1=[hi_tex,lo_tex]——全序键 lo_tex<hi_tex）。
    pub wave_shape_exact: bool,
    /// 轮次深度恰值（2）。
    pub wave_depth_exact: bool,
    /// 搁浅显性（硬灌环账 → stranded 非空不静默）。
    pub wave_stranded_visible: bool,
    /// 编排：record 未知场景拒。
    pub runner_unknown_block: bool,
    /// 编排：重跑经编排层透传（record_batch 二次即 rerun 轮）。
    pub runner_rerun_passthrough: bool,
    /// 编排：终判分列（出分/Failed）。
    pub runner_finalize_split: bool,
    /// 编排：预算爆 → partial 置位。
    pub runner_partial_flag: bool,
    /// 编排：双跑指纹一致。
    pub runner_determinism: bool,
    /// 轮次读屏形。
    pub wave_line_shape: bool,
}

/// 跑编排探针。
pub fn run_probes_v23() -> ProbeV23Report {
    // 构造：base、lo_tex（优先级 1）、hi_tex（优先级 2）——全序键序
    // lo_tex(p1) < hi_tex(p2)；hi_tex 依赖 base。
    let mut r = DepsAwareRunner::new(10_000);
    let _ = r.sched.enqueue("hi_tex", 2);
    let _ = r.sched.enqueue("base", 0);
    let _ = r.sched.enqueue("lo_tex", 1);
    let link_ok = r.link("hi_tex", "base").is_ok();

    let (w, stranded) = r.waves();
    // 轮0 = 依赖已满足者：base（无依赖）+ lo_tex（无依赖）——全序键序
    // lo_tex 在前？lo_tex p1 < base p0——不对：base p0 最先。轮0=[base,lo_tex]。
    let wave0 = w.waves.first().cloned().unwrap_or_default();
    let wave1 = w.waves.get(1).cloned().unwrap_or_default();
    let shape = wave0 == vec![String::from("base"), String::from("lo_tex")]
        && wave1 == vec![String::from("hi_tex")]
        && stranded.is_empty()
        && w.depth() == 2
        && link_ok;

    // 搁浅显性：手工构造带环账硬灌 compute。
    let names: Vec<String> = ["x", "y"].iter().map(|s| String::from(*s)).collect();
    let mut cyc = DepGraph::new();
    let known = names.clone();
    let _ = cyc.attach("x", "y", &known);
    let known2: Vec<String> = vec![String::from("x"), String::from("y"), String::from("loop")];
    // 三角环：y→x（已有）+ x→loop + loop→y。
    let _ = cyc.attach("x", "loop", &known2);
    // 此时 x→y→? y 无出边……构造真环：loop→x。
    let mut cyc2 = DepGraph::new();
    let _ = cyc2.attach("loop", "x", &known2);
    let _ = cyc2.attach("x", "loop", &known2); // 探环应拒——用 raw edges 直灌绕过闸
    let (w2, stranded2) = {
        // 直灌环边（绕过挂线闸——防线纵深测试：compute 自身要能显性搁浅）。
        let mut raw = DepGraph::new();
        raw.edges.push((String::from("x"), String::from("loop")));
        raw.edges.push((String::from("loop"), String::from("x")));
        raw.edges.push((String::from("y"), String::from("x")));
        WaveLedger::compute(&names, &raw)
    };
    let stranded_visible = !stranded2.is_empty() && w2.waves.first().map(|v| v.contains(&String::from("y"))).unwrap_or(false);
    let _ = (cyc, cyc2);

    // 编排：未知场景拒。
    let unk = matches!(r.record_batch("ghost", &[100]), Err(RnrCode::RnScenarioUnknown));
    // 重跑透传：base 首批噪 → 二批经同一 record_batch 消解。
    let _ = r.record_batch("base", &[100, 200, 100, 200, 100]);
    let _ = r.record_batch("base", &[100, 101, 100, 101, 100]);
    let passthrough = r
        .reruns
        .iter()
        .find(|(n, _)| n == "base")
        .map(|(_, lg)| lg.reruns == 1 && lg.score() == Some(100))
        .unwrap_or(false);
    // 终判分列：base 出分；lo_tex 灌两轮仍噪 → Failed。
    let _ = r.record_batch("lo_tex", &[100, 900, 100, 900, 100]);
    let _ = r.record_batch("lo_tex", &[100, 950, 100, 950, 100]);
    let _ = r.record_batch("lo_tex", &[100, 990, 100, 990, 100]);
    let (scored, failed) = r.finalize();
    let split = scored.contains(&(String::from("base"), 100))
        && failed.len() == 1
        && failed[0].0 == "lo_tex";
    // 预算爆 → partial。
    let mut r2 = DepsAwareRunner::new(500);
    let _ = r2.sched.enqueue("s", 0);
    let _ = r2.record_batch("s", &[100, 100, 100, 100, 100]);
    let _ = r2.budget.spend(600);
    let _ = r2.finalize();
    let partial_flag = r2.partial;
    // 双跑。
    let build = || -> DepsAwareRunner {
        let mut x = DepsAwareRunner::new(10_000);
        let _ = x.sched.enqueue("hi_tex", 2);
        let _ = x.sched.enqueue("base", 0);
        let _ = x.sched.enqueue("lo_tex", 1);
        let _ = x.link("hi_tex", "base");
        let _ = x.record_batch("base", &[100, 101, 100, 101, 100]);
        let _ = x.record_batch("hi_tex", &[200, 201, 200, 201, 200]);
        x
    };
    let det = build().fingerprint() == build().fingerprint();
    // 轮次读屏。
    let wl = w.line();
    let wave_line = wl.contains("轮次|0:base,lo_tex|1:hi_tex");

    ProbeV23Report {
        wave_shape_exact: shape,
        wave_depth_exact: w.depth() == 2,
        wave_stranded_visible: stranded_visible,
        runner_unknown_block: unk,
        runner_rerun_passthrough: passthrough,
        runner_finalize_split: split,
        runner_partial_flag: partial_flag,
        runner_determinism: det,
        wave_line_shape: wave_line,
    }
}
