//! UNX-F50139（合并入收官）B27 批集成自检探针（0x43d80 段）
//!
//! 锚点：UNX-F50139-J1 探针全绿；判据锚定 §AI-63 §7。
//!
//! 批收官前的最后一道闸：按固定顺序把四段自检（向量 → grep → 印 → 汇总）
//! 跑一遍，每段给出「通过/不通过 + 一个可复算的度量值」，四段全绿才允许
//! 产出报告。探针的价值在于**顺序**与**预算**都是显式的：跳段、重段、预算
//! 用尽、失败后继续跑，都会拿到专属诊断码，而不是悄悄给一个「看起来全绿」
//! 的汇总。
//!
//! 为什么失败即熔断：后面的段依赖前面的产物（grep 记录要挂到向量账上、
//! 印章要对账本复核）。前段失败还继续跑，后面只会产出更多假绿——所以
//! `step(false)` 当场把状态钉死在 `Failed`，后续任何推进都 `E_STATE`。
//!
//! 数据结构（状态机 + 调度器 + 环形账）：
//! * `ProbeStage` / `ProbeState` —— 四段固定序 + 四态运行机；
//! * `ProbeRunner` —— 步进调度器（预算扣减、顺序推进、失败熔断）；
//! * `ProbeReport` —— 报告凭据（度量合计 + 尾部摘要 + 自校验）；
//! * `ProbeRing` —— 定容报告环（最近 N 次，旧报告显性记入 `evicted`）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43d80 段），九个码值全部被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43d80 独占 0x20）。
pub const SEG_BASE: u32 = 0x43d80;

pub const E_EMPTY: u16 = 1;
pub const E_STAGE: u16 = 2;
pub const E_ORDER: u16 = 3;
pub const E_BUDGET: u16 = 4;
pub const E_STATE: u16 = 5;
pub const E_DUPE: u16 = 6;
pub const E_RANGE: u16 = 7;
pub const E_RING: u16 = 8;
pub const E_DIGEST: u16 = 9;

/// 探针错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    Empty,
    Stage,
    Order,
    Budget,
    State,
    Dupe,
    Range,
    Ring,
    Digest,
}

impl ProbeError {
    /// 诊断码 = 段基址 + 序号。
    pub fn code(self) -> u32 {
        let n = match self {
            ProbeError::Empty => E_EMPTY,
            ProbeError::Stage => E_STAGE,
            ProbeError::Order => E_ORDER,
            ProbeError::Budget => E_BUDGET,
            ProbeError::State => E_STATE,
            ProbeError::Dupe => E_DUPE,
            ProbeError::Range => E_RANGE,
            ProbeError::Ring => E_RING,
            ProbeError::Digest => E_DIGEST,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            ProbeError::Empty => "probe plan is empty",
            ProbeError::Stage => "stage code outside 0..=3",
            ProbeError::Order => "stage plan is not the canonical order",
            ProbeError::Budget => "probe step budget exhausted",
            ProbeError::State => "runner state does not allow this action",
            ProbeError::Dupe => "stage repeated in plan",
            ProbeError::Range => "plan length or budget outside legal range",
            ProbeError::Ring => "report ring capacity invalid",
            ProbeError::Digest => "report digest does not match fields",
        }
    }
}

/// 四段自检。顺序即数组顺序，不可重排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeStage {
    Vector,
    Grep,
    Seal,
    Aggregate,
}

/// 段数（固定 4）。
pub const STAGE_COUNT: usize = 4;

/// 段预算上界（一次收口最多允许 4096 步，足够覆盖重试）。
pub const MAX_BUDGET: u32 = 4096;

/// 环形报告容量上界。
pub const MAX_RING: usize = 8;

/// 规范段序。
pub const STAGE_ORDER: [ProbeStage; STAGE_COUNT] = [
    ProbeStage::Vector,
    ProbeStage::Grep,
    ProbeStage::Seal,
    ProbeStage::Aggregate,
];

impl ProbeStage {
    /// 段码（0..3）。
    pub fn code(self) -> u8 {
        match self {
            ProbeStage::Vector => 0,
            ProbeStage::Grep => 1,
            ProbeStage::Seal => 2,
            ProbeStage::Aggregate => 3,
        }
    }

    /// 码值反解。
    pub fn from_code(c: u8) -> Result<ProbeStage, ProbeError> {
        match c {
            0 => Ok(ProbeStage::Vector),
            1 => Ok(ProbeStage::Grep),
            2 => Ok(ProbeStage::Seal),
            3 => Ok(ProbeStage::Aggregate),
            _ => Err(ProbeError::Stage),
        }
    }

    /// 段标签。
    pub fn label(self) -> &'static str {
        match self {
            ProbeStage::Vector => "vector",
            ProbeStage::Grep => "grep",
            ProbeStage::Seal => "seal",
            ProbeStage::Aggregate => "aggregate",
        }
    }
}

/// 运行机四态。Idle → Running →（Passed | Failed），单向不回退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    Idle,
    Running,
    Passed,
    Failed,
}

impl ProbeState {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            ProbeState::Idle => "idle",
            ProbeState::Running => "running",
            ProbeState::Passed => "passed",
            ProbeState::Failed => "failed",
        }
    }

    /// 是否已收束（可以出报告）。
    pub fn settled(self) -> bool {
        matches!(self, ProbeState::Passed | ProbeState::Failed)
    }
}

/// 单段结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageOutcome {
    pub stage: ProbeStage,
    pub ok: bool,
    pub metric: u32,
    pub code: u16,
}

impl StageOutcome {
    /// 一行渲染。
    pub fn render(&self) -> String {
        format!(
            "{} ok={} metric={} code={}",
            self.stage.label(),
            self.ok,
            self.metric,
            self.code
        )
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

/// 报告摘要：种子 ‖ 段数 ‖ 通过数 ‖ 度量合计 ‖ 尾部摘要。
pub fn report_digest(seed: u32, stages: u8, passed: u8, metric_sum: u32, tail: u64) -> u64 {
    let mut h = FNV_OFFSET;
    h = fnv_u32(h, seed);
    h = fnv_u8(h, stages);
    h = fnv_u8(h, passed);
    h = fnv_u32(h, metric_sum);
    h = fnv_u32(h, (tail & 0xffff_ffff) as u32);
    fnv_u32(h, (tail >> 32) as u32)
}

fn outcome_tail(outcomes: &[StageOutcome]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < outcomes.len() {
        if let Some(o) = outcomes.get(i) {
            h = fnv_u8(h, o.stage.code());
            h = fnv_u8(h, if o.ok { 1 } else { 0 });
            h = fnv_u32(h, o.metric);
        }
        i += 1;
    }
    h
}

/// 探针报告凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    pub seed: u32,
    pub stages: u8,
    pub passed: u8,
    pub metric_sum: u32,
    pub tail: u64,
    pub digest: u64,
    pub state: ProbeState,
}

impl ProbeReport {
    /// 全绿：四段都跑完且都通过。
    pub fn complete(&self) -> bool {
        self.state == ProbeState::Passed
            && self.stages as usize == STAGE_COUNT
            && self.passed as usize == STAGE_COUNT
    }

    /// 自校验：字段重算摘要必须与携带的摘要一致。
    pub fn verify(&self) -> Result<u64, ProbeError> {
        let d = report_digest(self.seed, self.stages, self.passed, self.metric_sum, self.tail);
        if d != self.digest {
            return Err(ProbeError::Digest);
        }
        Ok(d)
    }

    /// 一行渲染（诊断用）。
    pub fn render(&self) -> String {
        format!(
            "probe {} {}/{} metric={} digest={:016x}",
            self.state.label(),
            self.passed,
            self.stages,
            self.metric_sum,
            self.digest
        )
    }
}

/// 步进调度器：预算扣减 + 段序推进 + 失败熔断。
pub struct ProbeRunner {
    state: ProbeState,
    seed: u32,
    budget: u32,
    spent: u32,
    outcomes: Vec<StageOutcome>,
    runs: u16,
}

impl ProbeRunner {
    /// 建一个空转运行机。预算必须落在 1..=MAX_BUDGET。
    pub fn new(seed: u32, budget: u32) -> Result<ProbeRunner, ProbeError> {
        if budget == 0 || budget > MAX_BUDGET {
            return Err(ProbeError::Range);
        }
        Ok(ProbeRunner {
            state: ProbeState::Idle,
            seed,
            budget,
            spent: 0,
            outcomes: Vec::with_capacity(STAGE_COUNT),
            runs: 0,
        })
    }

    /// 当前状态。
    pub fn state(&self) -> ProbeState {
        self.state
    }

    /// 已用步数。
    pub fn spent(&self) -> u32 {
        self.spent
    }

    /// 预算余量。
    pub fn remaining(&self) -> u32 {
        self.budget.saturating_sub(self.spent)
    }

    /// 已跑段数。
    pub fn len(&self) -> usize {
        self.outcomes.len()
    }

    /// 是否尚未跑任何段。
    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// 启动轮次（记录重跑次数，幂等性判据用）。
    pub fn runs(&self) -> u16 {
        self.runs
    }

    /// 段结果只读视图。
    pub fn outcomes(&self) -> &[StageOutcome] {
        &self.outcomes
    }

    /// 取第 `idx` 段结果。
    pub fn outcome(&self, idx: usize) -> Option<&StageOutcome> {
        self.outcomes.get(idx)
    }

    /// 起跑：Idle → Running。只能起一次。
    pub fn begin(&mut self) -> Result<(), ProbeError> {
        if self.state != ProbeState::Idle {
            return Err(ProbeError::State);
        }
        self.state = ProbeState::Running;
        self.runs = self.runs.saturating_add(1);
        Ok(())
    }

    /// 推进一段。段序由 `outcomes.len()` 决定，调用方不能自选。
    ///
    /// 预算检查放在「还有没有段可跑」之后：四段跑满后剩余预算再多也不该
    /// 允许第五步，那是计划错误（`E_RANGE`）而不是预算错误。
    pub fn step(&mut self, metric: u32, ok: bool) -> Result<ProbeStage, ProbeError> {
        if self.state != ProbeState::Running {
            return Err(ProbeError::State);
        }
        if self.outcomes.len() >= STAGE_COUNT {
            return Err(ProbeError::Range);
        }
        if self.spent >= self.budget {
            return Err(ProbeError::Budget);
        }
        let stage = match STAGE_ORDER.get(self.outcomes.len()) {
            Some(s) => *s,
            None => return Err(ProbeError::Range),
        };
        self.outcomes.push(StageOutcome {
            stage,
            ok,
            metric,
            code: 0,
        });
        self.spent = self.spent.saturating_add(1);
        if !ok {
            self.state = ProbeState::Failed;
        } else if self.outcomes.len() == STAGE_COUNT {
            self.state = ProbeState::Passed;
        }
        Ok(stage)
    }

    /// 收束：产出报告。必须已经 Passed/Failed（Running 说明还没跑完）。
    pub fn finish(&mut self) -> Result<ProbeReport, ProbeError> {
        if !self.state.settled() {
            return Err(ProbeError::State);
        }
        Ok(self.report())
    }

    /// 由当前段结果构造报告（不改变状态）。
    pub fn report(&self) -> ProbeReport {
        let mut passed = 0u8;
        let mut metric_sum = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    passed = passed.saturating_add(1);
                }
                metric_sum = metric_sum.wrapping_add(o.metric);
            }
            i += 1;
        }
        let stages = self.outcomes.len() as u8;
        let tail = outcome_tail(&self.outcomes);
        ProbeReport {
            seed: self.seed,
            stages,
            passed,
            metric_sum,
            tail,
            digest: report_digest(self.seed, stages, passed, metric_sum, tail),
            state: self.state,
        }
    }
}

fn fold_u32(mut h: u64, v: u32) -> u64 {
    h = fnv_u32(h, v);
    h.wrapping_mul(FNV_PRIME)
}

fn count_word(line: &str, word: &str) -> u16 {
    let hay = line.as_bytes();
    let pat = word.as_bytes();
    if pat.is_empty() || pat.len() > hay.len() {
        return 0;
    }
    let mut n = 0u16;
    let mut i = 0usize;
    while i + pat.len() <= hay.len() {
        let mut k = 0usize;
        let mut hit = true;
        while k < pat.len() {
            match (hay.get(i + k), pat.get(k)) {
                (Some(a), Some(b)) => {
                    if *a != *b {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            k += 1;
        }
        if hit {
            n = n.saturating_add(1);
            i += pat.len();
        } else {
            i += 1;
        }
    }
    n
}

fn eval_vector(seed: u32) -> (bool, u32) {
    let mut s = if seed == 0 { 0x9e37_79b9 } else { seed };
    let mut i = 0usize;
    let mut chunks = 0u32;
    let mut ok = true;
    while i < 24 {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let bytes = 1 + s % 4096;
        let mp = 512u32;
        let c = (bytes + mp - 1) / mp;
        if c == 0 || c * mp < bytes || (c - 1) * mp >= bytes {
            ok = false;
        }
        chunks = chunks.saturating_add(c);
        i += 1;
    }
    (ok && chunks >= 24, chunks)
}

fn eval_grep(_seed: u32) -> (bool, u32) {
    let corpus = ["usb one", "x", "usb usb", "x", "y", "usb three"];
    let mut hits = 0u16;
    let mut first = 0u16;
    let mut last = 0u16;
    let mut i = 0usize;
    while i < corpus.len() {
        let c = count_word(corpus[i], "usb");
        if c > 0 {
            hits = hits.saturating_add(c);
            if first == 0 {
                first = 1 + i as u16;
            }
            last = 1 + i as u16;
        }
        i += 1;
    }
    (hits == 4 && first == 1 && last == 6, hits as u32)
}

fn eval_seal(seed: u32) -> (bool, u32) {
    let mut h = FNV_OFFSET;
    let mut i = 0u32;
    while i < 20 {
        h = fold_u32(h, seed.wrapping_add(i));
        i += 1;
    }
    // 敏感性：改第 10 环的值，根必须变——链式摘要若对单环不敏感，
    // 印章就证明不了「这 20 环原样未动」。
    let mut alt = FNV_OFFSET;
    let mut j = 0u32;
    while j < 20 {
        let v = if j == 10 {
            seed.wrapping_add(j) ^ 0x5a5a_5a5a
        } else {
            seed.wrapping_add(j)
        };
        alt = fold_u32(alt, v);
        j += 1;
    }
    (h != alt, (h & 0xffff) as u32)
}

fn eval_stage(stage: ProbeStage, seed: u32, prior: &[StageOutcome]) -> (bool, u32) {
    match stage {
        ProbeStage::Vector => eval_vector(seed),
        ProbeStage::Grep => eval_grep(seed),
        ProbeStage::Seal => eval_seal(seed),
        ProbeStage::Aggregate => {
            let mut sum = 0u32;
            let mut all_ok = prior.len() == 3;
            let mut i = 0usize;
            while i < prior.len() {
                if let Some(o) = prior.get(i) {
                    sum = sum.wrapping_add(o.metric);
                    if !o.ok {
                        all_ok = false;
                    }
                }
                i += 1;
            }
            (all_ok && sum > 0, sum)
        }
    }
}

/// 校验计划：必须恰好四段、严格规范序、无重段。
pub fn validate_plan(plan: &[u8]) -> Result<(), ProbeError> {
    if plan.is_empty() {
        return Err(ProbeError::Empty);
    }
    if plan.len() != STAGE_COUNT {
        return Err(ProbeError::Range);
    }
    let mut i = 0usize;
    while i < plan.len() {
        let c = match plan.get(i) {
            Some(v) => *v,
            None => return Err(ProbeError::Range),
        };
        if c as usize >= STAGE_COUNT {
            return Err(ProbeError::Stage);
        }
        if i > 0 {
            if let Some(p) = plan.get(i - 1) {
                if c == *p {
                    return Err(ProbeError::Dupe);
                }
                if c < *p {
                    return Err(ProbeError::Order);
                }
            }
        }
        if c as usize != i {
            return Err(ProbeError::Order);
        }
        i += 1;
    }
    Ok(())
}

/// 按计划跑一整轮探针，返回报告（失败即熔断，报告里带失败段位置）。
pub fn run_plan(plan: &[u8], seed: u32, budget: u32) -> Result<ProbeReport, ProbeError> {
    validate_plan(plan)?;
    let mut r = ProbeRunner::new(seed, budget)?;
    r.begin()?;
    let mut i = 0usize;
    while i < plan.len() {
        let code = match plan.get(i) {
            Some(v) => *v,
            None => break,
        };
        let stage = ProbeStage::from_code(code)?;
        let (ok, metric) = eval_stage(stage, seed, r.outcomes());
        r.step(metric, ok)?;
        if !ok {
            break;
        }
        i += 1;
    }
    r.finish()
}

/// 定容报告环：只留最近 `cap` 份，被挤掉的显性记入 `evicted`。
pub struct ProbeRing {
    slots: Vec<ProbeReport>,
    cap: usize,
    evicted: usize,
}

impl ProbeRing {
    /// 建环。容量 1..=MAX_RING。
    pub fn new(cap: usize) -> Result<ProbeRing, ProbeError> {
        if cap == 0 || cap > MAX_RING {
            return Err(ProbeError::Ring);
        }
        Ok(ProbeRing {
            slots: Vec::with_capacity(cap),
            cap,
            evicted: 0,
        })
    }

    /// 当前份数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 已驱逐份数。
    pub fn evicted(&self) -> usize {
        self.evicted
    }

    /// 最新一份。
    pub fn last(&self) -> Option<&ProbeReport> {
        self.slots.last()
    }

    /// 压入一份报告；满则先弹最旧的一份。
    pub fn push(&mut self, r: ProbeReport) {
        if self.slots.len() >= self.cap {
            self.slots.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.slots.push(r);
    }

    /// 环内摘要是否全同（同一实现、同一批输入必须给同一凭据）。
    pub fn consistent(&self) -> bool {
        let first = match self.slots.first() {
            Some(r) => r.digest,
            None => return false,
        };
        let mut i = 1usize;
        while i < self.slots.len() {
            if let Some(r) = self.slots.get(i) {
                if r.digest != first {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入核：十类坏探针，逐类断言专属码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 空计划
    if validate_plan(&[]).err() == Some(ProbeError::Empty) {
        caught += 1;
    }

    // 2 段码越界
    if validate_plan(&[0, 1, 9, 3]).err() == Some(ProbeError::Stage) {
        caught += 1;
    }

    // 3 乱序
    if validate_plan(&[1, 0, 2, 3]).err() == Some(ProbeError::Order) {
        caught += 1;
    }

    // 4 重段
    if validate_plan(&[0, 0, 2, 3]).err() == Some(ProbeError::Dupe) {
        caught += 1;
    }

    // 5 计划长度不合规
    if validate_plan(&[0, 1, 2]).err() == Some(ProbeError::Range) {
        caught += 1;
    }

    // 6 预算为 0
    if ProbeRunner::new(1, 0).err() == Some(ProbeError::Range) {
        caught += 1;
    }

    // 7 预算耗尽（预算 2，跑四段）
    if run_plan(&[0, 1, 2, 3], 7, 2).err() == Some(ProbeError::Budget) {
        caught += 1;
    }

    // 8 未起跑就推进
    let mut r8 = match ProbeRunner::new(1, 8) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if r8.step(1, true).err() == Some(ProbeError::State) {
        caught += 1;
    }

    // 9 报告被改
    let mut r9 = match ProbeRunner::new(1, 8) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if r9.begin().is_ok() {
        let _ = r9.step(10, true);
        let _ = r9.step(20, true);
        let _ = r9.step(30, true);
        let _ = r9.step(40, true);
        if let Ok(rep) = r9.finish() {
            let tampered = ProbeReport {
                metric_sum: rep.metric_sum.wrapping_add(1),
                ..rep
            };
            if tampered.verify().err() == Some(ProbeError::Digest) {
                caught += 1;
            }
        }
    }

    // 环容量非法
    if ProbeRing::new(0).err() == Some(ProbeError::Ring) {
        caught += 1;
    }

    (caught, total)
}

/// 判据集（12 条）。
pub fn run_unx50139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50139");

    // J1-1 段码闭集与规范序
    let mut ok = true;
    let mut c = 0u8;
    while c < 4 {
        match ProbeStage::from_code(c) {
            Ok(st) => {
                if st.code() != c {
                    ok = false;
                }
                if let Some(canon) = STAGE_ORDER.get(c as usize) {
                    if canon.code() != c {
                        ok = false;
                    }
                }
            }
            Err(_) => ok = false,
        }
        c += 1;
    }
    let ok = ok
        && ProbeStage::from_code(4).err() == Some(ProbeError::Stage)
        && STAGE_COUNT == 4
        && STAGE_ORDER[0] == ProbeStage::Vector
        && STAGE_ORDER[3] == ProbeStage::Aggregate;
    s.add("50139-J1-段表闭集", ok, if ok { "" } else { "段表漂移" });

    // J1-2 计划校验：空/长/乱序/重段/越界
    let ok = validate_plan(&[]).err() == Some(ProbeError::Empty)
        && validate_plan(&[0, 1, 2]).err() == Some(ProbeError::Range)
        && validate_plan(&[0, 1, 2, 3, 0]).err() == Some(ProbeError::Range)
        && validate_plan(&[1, 0, 2, 3]).err() == Some(ProbeError::Order)
        && validate_plan(&[0, 0, 2, 3]).err() == Some(ProbeError::Dupe)
        && validate_plan(&[0, 1, 9, 3]).err() == Some(ProbeError::Stage)
        && validate_plan(&[0, 1, 2, 3]).is_ok();
    s.add("50139-J1-计划校验", ok, if ok { "" } else { "计划校验漏项" });

    // J1-3 全绿：四段全跑、状态 Passed、报告 complete
    let mut det = String::new();
    let ok = match run_plan(&[0, 1, 2, 3], 0x50139, 16) {
        Ok(rep) => {
            det = rep.render();
            rep.complete()
                && rep.stages == 4
                && rep.passed == 4
                && rep.state == ProbeState::Passed
        }
        Err(_) => false,
    };
    s.add_dyn("50139-J1-探针全绿", ok, &det);

    // J1-4 确定性：同种子同计划两次同摘要
    let a = run_plan(&[0, 1, 2, 3], 0x777, 16);
    let b = run_plan(&[0, 1, 2, 3], 0x777, 16);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x == y && x.digest == y.digest,
        _ => false,
    };
    s.add("50139-J1-报告确定", ok, if ok { "" } else { "同种子报告漂移" });

    // J1-5 种子敏感：换种子必换摘要
    let c1 = run_plan(&[0, 1, 2, 3], 0x777, 16);
    let c2 = run_plan(&[0, 1, 2, 3], 0x778, 16);
    let ok = match (c1, c2) {
        (Ok(x), Ok(y)) => x.digest != y.digest,
        _ => false,
    };
    s.add("50139-J1-种子敏感", ok, if ok { "" } else { "异种子同摘要" });

    // J1-6 预算：刚好够、差一步
    let exact = run_plan(&[0, 1, 2, 3], 5, 4);
    let short = run_plan(&[0, 1, 2, 3], 5, 3);
    let ok =
        exact.is_ok() && short.err() == Some(ProbeError::Budget);
    let det = format!("exact={} short={:?}", exact.is_ok(), short.err().map(|x| x.code()));
    s.add_dyn("50139-J1-预算边界", ok, &det);

    // J1-7 失败熔断：失败段钉死状态，后续推进被拒
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut r) = ProbeRunner::new(9, 8) {
        let _ = r.begin();
        let _ = r.step(10, true);
        let _ = r.step(20, false);
        let blocked = r.step(30, true).err() == Some(ProbeError::State);
        let rep = r.finish();
        ok = blocked
            && r.state() == ProbeState::Failed
            && rep.map(|x| x.passed == 1 && x.stages == 2 && !x.complete()) == Ok(true);
        det = format!("state={} spent={}", r.state().label(), r.spent());
    }
    s.add_dyn("50139-J1-失败熔断", ok, &det);

    // J1-8 状态机：未起跑推进/重复起跑/未跑完收束
    let mut ok = false;
    if let Ok(mut r) = ProbeRunner::new(1, 8) {
        let before = r.step(1, true).err() == Some(ProbeError::State);
        let _ = r.begin();
        let twice = r.begin().err() == Some(ProbeError::State);
        let early = r.finish().err() == Some(ProbeError::State);
        ok = before && twice && early && r.runs() == 1 && r.state() == ProbeState::Running;
    }
    s.add("50139-J1-状态机", ok, if ok { "" } else { "状态机可跳步" });

    // J1-9 守恒：报告合计与逐段度量逐项相等
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut r) = ProbeRunner::new(0xabc, 16) {
        let _ = r.begin();
        let mut i = 0usize;
        while i < STAGE_COUNT {
            let code = i as u8;
            match ProbeStage::from_code(code) {
                Ok(st) => {
                    let (pass, metric) = eval_stage(st, 0xabc, r.outcomes());
                    let _ = r.step(metric, pass);
                }
                Err(_) => break,
            }
            i += 1;
        }
        let rep = r.report();
        let mut sum = 0u32;
        let mut n = 0usize;
        let mut pass_n = 0u8;
        while n < r.len() {
            if let Some(o) = r.outcome(n) {
                sum = sum.wrapping_add(o.metric);
                if o.ok {
                    pass_n = pass_n.saturating_add(1);
                }
            }
            n += 1;
        }
        ok = rep.metric_sum == sum && rep.stages as usize == r.len() && rep.passed == pass_n;
        det = format!("sum={} n={} passed={}", rep.metric_sum, rep.stages, rep.passed);
    }
    s.add_dyn("50139-J1-度量守恒", ok, &det);

    // J1-10 报告自校验：干净通过、改字段打回
    let mut ok = false;
    if let Ok(rep) = run_plan(&[0, 1, 2, 3], 3, 8) {
        let clean = rep.verify().is_ok();
        let tampered = ProbeReport {
            passed: rep.passed.saturating_sub(1),
            ..rep
        };
        ok = clean && tampered.verify().err() == Some(ProbeError::Digest);
    }
    s.add("50139-J1-报告自校验", ok, if ok { "" } else { "报告改动未被发现" });

    // J1-11 报告环：满则驱逐、同源同摘要
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut ring) = ProbeRing::new(4) {
        let mut i = 0usize;
        while i < 6 {
            if let Ok(rep) = run_plan(&[0, 1, 2, 3], 11, 8) {
                ring.push(rep);
            }
            i += 1;
        }
        let same = ring.consistent();
        if let Ok(rep) = run_plan(&[0, 1, 2, 3], 12, 8) {
            ring.push(rep);
        }
        ok = ring.len() == 4
            && ring.evicted() == 3
            && same
            && !ring.consistent()
            && ProbeRing::new(0).err() == Some(ProbeError::Ring)
            && ProbeRing::new(MAX_RING + 1).err() == Some(ProbeError::Ring);
        det = format!("len={} evicted={}", ring.len(), ring.evicted());
    }
    s.add_dyn("50139-J1-报告环", ok, &det);

    // J1-12 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50139-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50139_checks_green() {
        let set = run_unx50139_checks();
        assert!(set.all_passed(), "UNX-F50139 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50139_full_plan_is_green() {
        let rep = run_plan(&[0, 1, 2, 3], 0x50139, 16);
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert!(r.complete());
            assert!(r.verify().is_ok());
        }
    }

    #[test]
    fn unx50139_failure_is_sticky() {
        let mut r = match ProbeRunner::new(1, 8) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(r.begin().is_ok());
        assert!(r.step(1, true).is_ok());
        assert!(r.step(2, false).is_ok());
        assert_eq!(r.step(3, true).err(), Some(ProbeError::State));
        assert_eq!(r.state(), ProbeState::Failed);
    }
}
