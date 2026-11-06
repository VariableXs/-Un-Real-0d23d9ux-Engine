//! UNX-F50150 发现埋点（发现/解析/聚合三点）（0x43ee0 段）
//!
//! 锚点：UNX-F50150-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 发现链只有三个可观测点：`Discover`（发包/收包）、`Parse`（解析应答）、
//! `Aggregate`（并入候选表）。覆盖率要能当判据用，前提是**失败与跳过也留痕**：
//! 解析失败时聚合根本没跑，如果聚合不发 span，覆盖率立刻掉到 66%，运维就分不清
//! 「这里没埋点」和「这里按设计跳过了」——前者是缺陷，后者是事实。
//!
//! 因此本模块把 `Outcome::Skipped` 提升为一等公民：跳过必须由 [`TraceRecorder::skip`]
//! 显式发一条零时长 span。覆盖率统计只数「阶段是否出现」，不看结果是否成功。
//!
//! 三个点是**一条线性流水线**，不是可以乱序的散点：
//! * 同一 run 内阶段序号必须严格递增，重复即 `DuplicateStage`，回退即 `OutOfOrder`；
//! * 两个 run 不允许交错（`run < last_run` 即 `OutOfOrder`）——交错意味着调用方
//!   在并发跑两条发现流程却共用了一个记录器，span 归属就不可信了；
//! * `begin` 与 `end` 必须配对，未开就关是 `NoOpen`，开两个是 `Nested`。
//!
//! 记录器定容（[`SPAN_CAP`]）。满时拒绝而不是覆盖：埋点丢一批旧 span 会让
//! 「某次发现没有日志」变成一个无法察觉的洞。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 9`，逐项见 [`TraceError`]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43ee0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ee0;

/// span 缓冲上限。64 轮 × 3 点 = 192，留出约 1.3 倍余量。
pub const SPAN_CAP: usize = 256;

pub const E_EMPTY_RUN: u16 = 1;
pub const E_NO_OPEN: u16 = 2;
pub const E_NESTED: u16 = 3;
pub const E_DUPLICATE_STAGE: u16 = 4;
pub const E_OUT_OF_ORDER: u16 = 5;
pub const E_BAD_CLOCK: u16 = 6;
pub const E_BUFFER_FULL: u16 = 7;
pub const E_SEALED: u16 = 8;
pub const E_MISSING_STAGE: u16 = 9;

/// 埋点域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    EmptyRun,
    NoOpen,
    Nested,
    DuplicateStage,
    OutOfOrder,
    BadClock,
    BufferFull,
    Sealed,
    MissingStage,
}

impl TraceError {
    pub fn offset(self) -> u16 {
        match self {
            TraceError::EmptyRun => E_EMPTY_RUN,
            TraceError::NoOpen => E_NO_OPEN,
            TraceError::Nested => E_NESTED,
            TraceError::DuplicateStage => E_DUPLICATE_STAGE,
            TraceError::OutOfOrder => E_OUT_OF_ORDER,
            TraceError::BadClock => E_BAD_CLOCK,
            TraceError::BufferFull => E_BUFFER_FULL,
            TraceError::Sealed => E_SEALED,
            TraceError::MissingStage => E_MISSING_STAGE,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TraceError::EmptyRun => "run id zero",
            TraceError::NoOpen => "span ended without begin",
            TraceError::Nested => "span begun while another is open",
            TraceError::DuplicateStage => "stage emitted twice in one run",
            TraceError::OutOfOrder => "stage or run went backwards",
            TraceError::BadClock => "span ended before it began",
            TraceError::BufferFull => "span buffer full",
            TraceError::Sealed => "recorder sealed",
            TraceError::MissingStage => "run does not cover all three stages",
        }
    }
}

/// 闭集枚举（顺序即偏移序）。
pub const ALL_ERRORS: [TraceError; 9] = [
    TraceError::EmptyRun,
    TraceError::NoOpen,
    TraceError::Nested,
    TraceError::DuplicateStage,
    TraceError::OutOfOrder,
    TraceError::BadClock,
    TraceError::BufferFull,
    TraceError::Sealed,
    TraceError::MissingStage,
];

/// 三个埋点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Discover,
    Parse,
    Aggregate,
}

impl Stage {
    pub fn rank(self) -> u8 {
        match self {
            Stage::Discover => 0,
            Stage::Parse => 1,
            Stage::Aggregate => 2,
        }
    }

    /// 覆盖率位图掩码。
    pub fn mask(self) -> u8 {
        1u8 << self.rank()
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Discover => "discover",
            Stage::Parse => "parse",
            Stage::Aggregate => "aggregate",
        }
    }
}

/// span 结果三态。`Skipped` 是覆盖率不塌的关键，必须显式产生。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Failed,
    Skipped,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Failed => "failed",
            Outcome::Skipped => "skipped",
        }
    }
}

/// 一条已完成的 span。`note` 承载上游诊断码（如发现错误码），便于跨模块串联。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub run: u32,
    pub stage: Stage,
    pub begin: u32,
    pub end: u32,
    pub outcome: Outcome,
    pub note: u16,
}

impl Span {
    /// 时长（`end >= begin` 由 `end`/`skip` 保证，这里仍用饱和减法兜底）。
    pub fn duration(self) -> u32 {
        self.end.saturating_sub(self.begin)
    }
}

/// 未闭合的 span。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenSpan {
    pub run: u32,
    pub stage: Stage,
    pub begin: u32,
}

/// 记录器：定容 span 缓冲 + 线性流水线顺序约束。
#[derive(Clone, Debug)]
pub struct TraceRecorder {
    spans: Vec<Span>,
    open: Option<OpenSpan>,
    last: Option<(u32, Stage)>,
    sealed: bool,
    rejects: u32,
}

impl TraceRecorder {
    pub fn new() -> TraceRecorder {
        TraceRecorder {
            spans: Vec::with_capacity(SPAN_CAP),
            open: None,
            last: None,
            sealed: false,
            rejects: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 被拒写入次数（显式记账，便于发现「埋点一直在失败」）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn at(&self, idx: usize) -> Option<Span> {
        self.spans.get(idx).copied()
    }

    pub fn open_span(&self) -> Option<OpenSpan> {
        self.open
    }

    /// 封存：之后的任何写入都 `Sealed`。封存是快照取值前的必要动作。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    fn reject(&mut self, e: TraceError) -> TraceError {
        self.rejects = self.rejects.saturating_add(1);
        e
    }

    /// 线性流水线顺序校验：同 run 严格递增，run 之间不允许回退/交错。
    fn order_ok(&self, run: u32, stage: Stage) -> Result<(), TraceError> {
        match self.last {
            None => Ok(()),
            Some((lr, ls)) => {
                if run < lr {
                    return Err(TraceError::OutOfOrder);
                }
                if run == lr {
                    if stage == ls {
                        return Err(TraceError::DuplicateStage);
                    }
                    if stage.rank() < ls.rank() {
                        return Err(TraceError::OutOfOrder);
                    }
                }
                Ok(())
            }
        }
    }

    /// 开一个 span。
    pub fn begin(&mut self, run: u32, stage: Stage, at: u32) -> Result<(), TraceError> {
        if self.sealed {
            return Err(self.reject(TraceError::Sealed));
        }
        if run == 0 {
            return Err(self.reject(TraceError::EmptyRun));
        }
        if self.open.is_some() {
            return Err(self.reject(TraceError::Nested));
        }
        if let Err(e) = self.order_ok(run, stage) {
            return Err(self.reject(e));
        }
        if self.spans.len() >= SPAN_CAP {
            return Err(self.reject(TraceError::BufferFull));
        }
        self.open = Some(OpenSpan {
            run,
            stage,
            begin: at,
        });
        Ok(())
    }

    /// 关闭当前 span。时钟倒退时**不消费** open，允许调用方改对时间重试。
    pub fn end(&mut self, at: u32, outcome: Outcome, note: u16) -> Result<Span, TraceError> {
        let o = match self.open {
            Some(v) => v,
            None => return Err(self.reject(TraceError::NoOpen)),
        };
        if at < o.begin {
            return Err(self.reject(TraceError::BadClock));
        }
        self.open = None;
        let sp = Span {
            run: o.run,
            stage: o.stage,
            begin: o.begin,
            end: at,
            outcome,
            note,
        };
        self.spans.push(sp);
        self.last = Some((o.run, o.stage));
        Ok(sp)
    }

    /// 发一条零时长 span，语义是「这一步按设计没跑」。跳过也必须留痕。
    pub fn skip(&mut self, run: u32, stage: Stage, at: u32, note: u16) -> Result<Span, TraceError> {
        if self.sealed {
            return Err(self.reject(TraceError::Sealed));
        }
        if run == 0 {
            return Err(self.reject(TraceError::EmptyRun));
        }
        if self.open.is_some() {
            return Err(self.reject(TraceError::Nested));
        }
        if let Err(e) = self.order_ok(run, stage) {
            return Err(self.reject(e));
        }
        if self.spans.len() >= SPAN_CAP {
            return Err(self.reject(TraceError::BufferFull));
        }
        let sp = Span {
            run,
            stage,
            begin: at,
            end: at,
            outcome: Outcome::Skipped,
            note,
        };
        self.spans.push(sp);
        self.last = Some((run, stage));
        Ok(sp)
    }

    /// 某个 run 的阶段位图（0b111 表示三点齐全）。
    pub fn stage_bits(&self, run: u32) -> u8 {
        let mut bits = 0u8;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(sp) = self.spans.get(i) {
                if sp.run == run {
                    bits |= sp.stage.mask();
                }
            }
            i += 1;
        }
        bits
    }

    /// 校验单个 run 覆盖三点；缺一即 `MissingStage`。
    pub fn verify_run(&self, run: u32) -> Result<(), TraceError> {
        if run == 0 {
            return Err(TraceError::EmptyRun);
        }
        if self.stage_bits(run) != 0b111 {
            return Err(TraceError::MissingStage);
        }
        Ok(())
    }

    /// 顺序违规计数（正常路径应恒为 0）：同 run 阶段序号非严格递增，或 run 回退。
    pub fn order_violations(&self) -> u32 {
        let mut bad = 0u32;
        let mut prev: Option<(u32, Stage)> = None;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(sp) = self.spans.get(i) {
                if let Some((pr, ps)) = prev {
                    if sp.run < pr {
                        bad += 1;
                    } else if sp.run == pr && sp.stage.rank() <= ps.rank() {
                        bad += 1;
                    }
                }
                prev = Some((sp.run, sp.stage));
            }
            i += 1;
        }
        bad
    }

    /// 负时长计数（`end` 已拦截，这里作为回归哨兵）。
    pub fn negative_durations(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(sp) = self.spans.get(i) {
                if sp.end < sp.begin {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖率汇总。`coverage_permille` 以「三点齐全的 run / 出现过的 run」计。
    pub fn summary(&self) -> TraceSummary {
        let mut out = TraceSummary {
            runs: 0,
            complete: 0,
            incomplete: 0,
            discover: 0,
            parse: 0,
            aggregate: 0,
            failed: 0,
            skipped: 0,
            coverage_permille: 0,
        };
        let mut cur = 0u32;
        let mut bits = 0u8;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(sp) = self.spans.get(i) {
                if sp.run != cur {
                    if cur != 0 {
                        out.runs += 1;
                        if bits == 0b111 {
                            out.complete += 1;
                        }
                    }
                    cur = sp.run;
                    bits = 0;
                }
                bits |= sp.stage.mask();
                match sp.stage {
                    Stage::Discover => out.discover += 1,
                    Stage::Parse => out.parse += 1,
                    Stage::Aggregate => out.aggregate += 1,
                }
                match sp.outcome {
                    Outcome::Ok => {}
                    Outcome::Failed => out.failed += 1,
                    Outcome::Skipped => out.skipped += 1,
                }
            }
            i += 1;
        }
        if cur != 0 {
            out.runs += 1;
            if bits == 0b111 {
                out.complete += 1;
            }
        }
        out.incomplete = out.runs.saturating_sub(out.complete);
        out.coverage_permille = if out.runs == 0 {
            0
        } else {
            out.complete.saturating_mul(1000) / out.runs
        };
        out
    }
}

/// 覆盖率汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceSummary {
    pub runs: u32,
    pub complete: u32,
    pub incomplete: u32,
    pub discover: u32,
    pub parse: u32,
    pub aggregate: u32,
    pub failed: u32,
    pub skipped: u32,
    pub coverage_permille: u32,
}

// ---------------------------------------------------------------------------
// 覆盖率审计：J1 的度量本体
// ---------------------------------------------------------------------------

/// 审计回报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceAudit {
    pub cycles: u32,
    pub runs: u32,
    pub complete: u32,
    pub coverage_permille: u32,
    pub spans: usize,
    pub failed: u32,
    pub skipped: u32,
    pub order_violations: u32,
    pub missing: u32,
    pub negative_durations: u32,
    pub rejects: u32,
}

/// 跑 `cycles` 轮完整发现流程，其中刻意混入两种故障：
/// * 发现阶段失败（每 11 轮一次）→ 解析与聚合跳过；
/// * 解析阶段失败（每 7 轮一次）→ 聚合跳过。
///
/// 两种故障都必须在**三点全覆盖**的前提下体现，否则覆盖率本身就是假指标。
pub fn trace_coverage_audit(cycles: u32) -> Result<TraceAudit, TraceError> {
    let mut r = TraceRecorder::new();
    let mut c = 0u32;
    while c < cycles {
        let run = c.saturating_add(1);
        let t = c.saturating_mul(100);
        let disc_fail = c % 11 == 5;
        r.begin(run, Stage::Discover, t)?;
        r.end(
            t + 1,
            if disc_fail {
                Outcome::Failed
            } else {
                Outcome::Ok
            },
            0,
        )?;
        if disc_fail {
            r.skip(run, Stage::Parse, t + 2, 0)?;
            r.skip(run, Stage::Aggregate, t + 3, 0)?;
        } else {
            let parse_fail = c % 7 == 3;
            r.begin(run, Stage::Parse, t + 2)?;
            r.end(
                t + 3,
                if parse_fail {
                    Outcome::Failed
                } else {
                    Outcome::Ok
                },
                0,
            )?;
            if parse_fail {
                r.skip(run, Stage::Aggregate, t + 4, 0)?;
            } else {
                r.begin(run, Stage::Aggregate, t + 4)?;
                r.end(t + 5, Outcome::Ok, 0)?;
            }
        }
        c = c.saturating_add(1);
    }
    let sum = r.summary();
    let mut missing = 0u32;
    let mut i = 0u32;
    while i < cycles {
        let run = i + 1;
        if r.verify_run(run).is_err() {
            missing += 1;
        }
        i += 1;
    }
    Ok(TraceAudit {
        cycles,
        runs: sum.runs,
        complete: sum.complete,
        coverage_permille: sum.coverage_permille,
        spans: r.len(),
        failed: sum.failed,
        skipped: sum.skipped,
        order_violations: r.order_violations(),
        missing,
        negative_durations: r.negative_durations(),
        rejects: r.rejects(),
    })
}

// ---------------------------------------------------------------------------
// 注入核：9 类坏样本
// ---------------------------------------------------------------------------

/// 注入核：9 类坏样本 → 9 个具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    if TraceRecorder::new().begin(0, Stage::Discover, 0).err() == Some(TraceError::EmptyRun) {
        caught += 1;
    }
    let mut a = TraceRecorder::new();
    if a.end(1, Outcome::Ok, 0).err() == Some(TraceError::NoOpen) {
        caught += 1;
    }
    // 开两个
    let mut b = TraceRecorder::new();
    let _ = b.begin(1, Stage::Discover, 0);
    if b.begin(1, Stage::Parse, 1).err() == Some(TraceError::Nested) {
        caught += 1;
    }
    // 重复阶段
    let mut c = TraceRecorder::new();
    let _ = c.begin(1, Stage::Discover, 0);
    let _ = c.end(1, Outcome::Ok, 0);
    if c.begin(1, Stage::Discover, 2).err() == Some(TraceError::DuplicateStage) {
        caught += 1;
    }
    // 回退：先 Parse 再 Discover
    let mut d = TraceRecorder::new();
    let _ = d.skip(1, Stage::Parse, 0, 0);
    if d.begin(1, Stage::Discover, 1).err() == Some(TraceError::OutOfOrder) {
        caught += 1;
    }
    // 时钟倒退
    let mut e = TraceRecorder::new();
    let _ = e.begin(1, Stage::Discover, 10);
    if e.end(5, Outcome::Ok, 0).err() == Some(TraceError::BadClock) {
        caught += 1;
    }
    // 缓冲满
    let mut f = TraceRecorder::new();
    let mut i = 0u32;
    while i < SPAN_CAP as u32 {
        let run = i + 1;
        let _ = f.skip(run, Stage::Aggregate, i, 0);
        i += 1;
    }
    if f.begin(SPAN_CAP as u32 + 1, Stage::Discover, 0).err() == Some(TraceError::BufferFull) {
        caught += 1;
    }
    // 封存后写入
    let mut g = TraceRecorder::new();
    g.seal();
    if g.begin(1, Stage::Discover, 0).err() == Some(TraceError::Sealed) {
        caught += 1;
    }
    // 缺阶段
    let mut h = TraceRecorder::new();
    let _ = h.skip(3, Stage::Discover, 0, 0);
    if h.verify_run(3).err() == Some(TraceError::MissingStage) {
        caught += 1;
    }

    (caught.min(total), total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50150");

    // J1-1 埋点 100% 覆盖（判据原文）
    let audit = trace_coverage_audit(64);
    let ok = match audit {
        Ok(a) => {
            a.cycles == 64
                && a.coverage_permille == 1000
                && a.complete == a.runs
                && a.missing == 0
                && a.order_violations == 0
                && a.negative_durations == 0
                && a.failed > 0
                && a.skipped > 0
        }
        Err(_) => false,
    };
    let det = match audit {
        Ok(a) => format!(
            "cycles={} cov={}‰ spans={} failed={} skipped={} miss={}",
            a.cycles, a.coverage_permille, a.spans, a.failed, a.skipped, a.missing
        ),
        Err(e) => format!("audit err {}", e.code()),
    };
    s.add_dyn("50150-J1-百覆盖", ok, &det);

    // J1-2 故障也留三点：发现失败时解析/聚合必须是 Skipped 而非缺席
    let mut r = TraceRecorder::new();
    let _ = r.begin(1, Stage::Discover, 0);
    let _ = r.end(1, Outcome::Failed, 7);
    let _ = r.skip(1, Stage::Parse, 2, 7);
    let _ = r.skip(1, Stage::Aggregate, 3, 7);
    let sum = r.summary();
    let ok = sum.coverage_permille == 1000
        && sum.complete == 1
        && sum.failed == 1
        && sum.skipped == 2
        && sum.incomplete == 0;
    s.add("50150-J1-跳过留痕", ok, if ok { "" } else { "跳过被吞" });

    // J1-3 顺序：三点必须递增，回退即拒
    let mut ord = TraceRecorder::new();
    let _ = ord.skip(1, Stage::Discover, 0, 0);
    let _ = ord.skip(1, Stage::Aggregate, 1, 0);
    let bad = ord.begin(1, Stage::Parse, 2).err();
    let ok = bad == Some(TraceError::OutOfOrder);
    s.add("50150-J1-阶段递增", ok, if ok { "" } else { "顺序回退被接受" });

    // J1-4 run 不允许交错
    let mut inter = TraceRecorder::new();
    let _ = inter.skip(2, Stage::Discover, 0, 0);
    let ok = inter.skip(1, Stage::Discover, 1, 0).err() == Some(TraceError::OutOfOrder)
        && inter.len() == 1;
    s.add("50150-J1-run不回退", ok, if ok { "" } else { "run 交错被接受" });

    // J1-5 重复阶段必拒且不污染计数
    let mut dup = TraceRecorder::new();
    let _ = dup.skip(1, Stage::Discover, 0, 0);
    let ok = dup.skip(1, Stage::Discover, 1, 0).err() == Some(TraceError::DuplicateStage)
        && dup.len() == 1
        && dup.summary().discover == 1;
    s.add("50150-J1-重复阶段", ok, if ok { "" } else { "重复阶段入账" });

    // J1-6 未开先关：NoOpen，且 rejects 显式累计
    let mut no = TraceRecorder::new();
    let ok = no.end(5, Outcome::Ok, 0).err() == Some(TraceError::NoOpen)
        && no.len() == 0
        && no.rejects() == 1;
    s.add("50150-J1-未开先关", ok, if ok { "" } else { "空关闭未记账" });

    // J1-7 时钟倒退：拒绝并保留 open，改对后可正常收尾
    let mut clk = TraceRecorder::new();
    let _ = clk.begin(1, Stage::Discover, 10);
    let bad = clk.end(9, Outcome::Ok, 0);
    let good = clk.end(11, Outcome::Ok, 0);
    let ok = bad.err() == Some(TraceError::BadClock)
        && good.is_ok()
        && clk.len() == 1
        && clk.open_span().is_none();
    s.add("50150-J1-时钟倒退", ok, if ok { "" } else { "负时长 span 入账" });

    // J1-8 封存后只读：任何写都被拒，已有 span 不变
    let mut se = TraceRecorder::new();
    let _ = se.skip(1, Stage::Discover, 0, 0);
    se.seal();
    let before = se.len();
    let ok = se.begin(1, Stage::Parse, 1).err() == Some(TraceError::Sealed)
        && se.skip(2, Stage::Discover, 2, 0).err() == Some(TraceError::Sealed)
        && se.len() == before
        && se.verify_run(1).err() == Some(TraceError::MissingStage);
    s.add("50150-J1-封存只读", ok, if ok { "" } else { "封存后仍可写" });

    // J1-9 缓冲满：拒绝且已写 span 一条不丢
    let mut bf = TraceRecorder::new();
    let mut i = 0u32;
    while i < SPAN_CAP as u32 {
        let _ = bf.skip(i + 1, Stage::Discover, i, 0);
        i += 1;
    }
    let ok = bf.len() == SPAN_CAP
        && bf.begin(SPAN_CAP as u32 + 1, Stage::Discover, 0).err() == Some(TraceError::BufferFull)
        && bf.len() == SPAN_CAP;
    s.add("50150-J1-满则不丢", ok, if ok { "" } else { "满后覆盖旧 span" });

    // J1-10 汇总自洽：三点计数之和 == span 总数；complete+incomplete == runs
    let mut co = TraceRecorder::new();
    let _ = co.skip(1, Stage::Discover, 0, 0);
    let _ = co.skip(1, Stage::Parse, 1, 0);
    let _ = co.skip(1, Stage::Aggregate, 2, 0);
    let _ = co.skip(2, Stage::Discover, 3, 0);
    let sum = co.summary();
    let ok = sum.discover + sum.parse + sum.aggregate == co.len() as u32
        && sum.complete + sum.incomplete == sum.runs
        && sum.runs == 2
        && sum.complete == 1
        && sum.incomplete == 1;
    s.add("50150-J1-汇总自洽", ok, if ok { "" } else { "汇总与 span 不符" });

    // J1-11 审计确定性：同参数两次跑逐项一致
    let a1 = trace_coverage_audit(32);
    let a2 = trace_coverage_audit(32);
    let ok = a1 == a2;
    s.add("50150-J1-审计确定", ok, if ok { "" } else { "审计不确定" });

    // J1-12 注入核全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50150-J1-注入必抓", ok, &det);

    // J1-13 错误码闭集连续
    let mut seen = [false; 10];
    let mut ok = true;
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        let off = ALL_ERRORS[i].offset();
        if off == 0 || off as usize >= seen.len() || seen[off as usize] {
            ok = false;
        } else {
            seen[off as usize] = true;
        }
        if ALL_ERRORS[i].code() != SEG_BASE + off as u32 {
            ok = false;
        }
        i += 1;
    }
    s.add("50150-J1-码值闭集", ok, if ok { "" } else { "码值重复或断裂" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50150_checks_green() {
        let set = run_unx50150_checks();
        assert!(set.all_passed(), "UNX-F50150 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50150_skipped_span_keeps_coverage() {
        let mut r = TraceRecorder::new();
        assert!(r.begin(1, Stage::Discover, 0).is_ok());
        assert!(r.end(1, Outcome::Failed, 0).is_ok());
        assert!(r.skip(1, Stage::Parse, 1, 0).is_ok());
        assert!(r.skip(1, Stage::Aggregate, 2, 0).is_ok());
        assert_eq!(r.summary().coverage_permille, 1000);
    }

    #[test]
    fn unx50150_order_rejected() {
        let mut r = TraceRecorder::new();
        assert!(r.skip(1, Stage::Aggregate, 0, 0).is_ok());
        assert_eq!(r.begin(1, Stage::Parse, 1).err(), Some(TraceError::OutOfOrder));
    }
}
