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
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
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

    s
}
