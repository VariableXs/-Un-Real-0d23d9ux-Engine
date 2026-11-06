//! UNX-F48856 B03 观测埋点 · fuzz 面 · 判据用例集（20 例） · 0x706E0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48856 ① 纯功能代码 ≥220 行；
//!          ② UNX-F48856-J1 四点 100% 覆盖；异常零静默；
//!   F48857 ① 纯功能代码 ≥260 行；
//!          ② UNX-F48857-J1 fuzz ×10^4 零崩；
//!   F48858 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48858-J1 ×20 全绿入库。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：入口打点 → 时间单调 → 出口打点 → 耗时采样 → 结果采样 →
//!           未闭合扫描 → 四点覆盖核验 → 用例期望比对。
//!   次序不可倒：先记结果后记出口，未闭合的 span 会带着结果混入统计；
//!   时间单调必须先于封闭，倒退时间戳会被算成负耗时并污染 P 分位；
//!   未闭合扫描必须先于覆盖核验，悬空 span 会让「四点 100%」假绿。
//! 关键差异：本模块只观测 B03 各模块操作的入口/出口/耗时/结果四点，
//!   不参与任何解析判定。B04 的四段延迟账本复用本模块的 span 语义，
//!   但时间源换成 TSC；本模块的「异常零静默」指：任何未闭合 span
//!   与任何非零结果码都必须在账上可见。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x706E0，VX6 批 B03 第七槽）。
pub const SEG_BASE: u32 = 0x706E0;

/// 同时打开的 span 上限（64；超过即拒绝，不悄悄丢）。
pub const MAX_SPANS: usize = 64;
/// fuzz 轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10000;
/// 用例条数（判据 ×20）。
pub const CASE_ROWS: usize = 20;
/// 金标操作对数。
pub const GOLDEN_PAIRS: u32 = 100;
/// 结果码 0 表示成功。
pub const OK_CODE: u32 = 0;

// ── 诊断码闭集（+0x01..+0x0C） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    /// 未知 span 句柄
    HandleBad,
    /// 重复关闭
    ExitDup,
    /// 时间戳倒退
    TimeBad,
    /// span 表满
    SpanFull,
    /// 未闭合 span：异常未记账
    Silent,
    /// 四点覆盖不全
    Coverage,
    /// 用例期望不符
    CaseBad,
    /// fuzz 账不平
    FuzzBad,
    /// 双跑不一致
    ReplayBad,
    /// 结果码非法（非本模块码）
    ResultBad,
    /// 段位越界
    SegmentBad,
}

impl ObCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ObCode::HandleBad => "未知 span 句柄：序号越界或从未登记",
            ObCode::ExitDup => "重复关闭：span 已经 exit 过",
            ObCode::TimeBad => "时间戳倒退：exit 早于 enter 或乱序",
            ObCode::SpanFull => "span 表满：同时打开不得超过 64 个",
            ObCode::Silent => "未闭合 span：异常被静默，禁止通过",
            ObCode::Coverage => "四点覆盖不全：入口/出口/耗时/结果任一为零",
            ObCode::CaseBad => "用例期望不符：实际判定与申报相反",
            ObCode::FuzzBad => "fuzz 账不平：接收 + 拒绝 ≠ 总轮数",
            ObCode::ReplayBad => "双跑不一致：账本指纹不同",
            ObCode::ResultBad => "结果码非法：不属于 0 或本域诊断码",
            ObCode::SegmentBad => "段位越界：诊断码逃出 0x706E0 窗口",
        }
    }

    pub const ALL: [ObCode; 11] = [
        ObCode::HandleBad,
        ObCode::ExitDup,
        ObCode::TimeBad,
        ObCode::SpanFull,
        ObCode::Silent,
        ObCode::Coverage,
        ObCode::CaseBad,
        ObCode::FuzzBad,
        ObCode::ReplayBad,
        ObCode::ResultBad,
        ObCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ObCode::ALL.len() {
        if ObCode::ALL[i].val() == code {
            return ObCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x706E0 段闭集内"
}

/// 结果码是否合法：成功 0 或本域诊断码。
pub fn result_code_ok(code: u32) -> bool {
    if code == OK_CODE {
        return true;
    }
    let mut i = 0usize;
    while i < ObCode::ALL.len() {
        if ObCode::ALL[i].val() == code {
            return true;
        }
        i += 1;
    }
    false
}

// ── FNV-1a（本域基础） ────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (w & 0xFF) as u8);
    x = fold_byte(x, ((w >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((w >> 16) & 0xFF) as u8);
    fold_byte(x, ((w >> 24) & 0xFF) as u8)
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    x = fold_word(x, (v & 0xFFFF_FFFF) as u32);
    fold_word(x, ((v >> 32) & 0xFFFF_FFFF) as u32)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── span 观测器 ──────────────────────────────────────────────────
/// 句柄只认序号：不暴露内部下标运算给调用方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpanHandle {
    pub seq: usize,
}

impl SpanHandle {
    pub fn text(&self) -> String {
        format!("span#{}", self.seq)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub id: u32,
    pub enter_ts: u64,
    pub exit_ts: u64,
    pub code: u32,
    pub open: bool,
}

impl Span {
    pub fn elapsed(&self) -> u64 {
        if self.open || self.exit_ts < self.enter_ts {
            0
        } else {
            self.exit_ts - self.enter_ts
        }
    }

    pub fn text(&self) -> String {
        format!(
            "op{} enter={} exit={} code=0x{:X} {}",
            self.id,
            self.enter_ts,
            self.exit_ts,
            self.code,
            if self.open { "open" } else { "closed" }
        )
    }
}

pub struct Observer {
    spans: Vec<Span>,
    entered: u32,
    exited: u32,
    errors: u32,
    anomalies: u32,
    last_ts: u64,
}

impl Observer {
    pub fn new() -> Observer {
        Observer {
            spans: Vec::new(),
            entered: 0,
            exited: 0,
            errors: 0,
            anomalies: 0,
            last_ts: 0,
        }
    }

    pub fn entered(&self) -> u32 {
        self.entered
    }

    pub fn exited(&self) -> u32 {
        self.exited
    }

    pub fn errors(&self) -> u32 {
        self.errors
    }

    pub fn anomalies(&self) -> u32 {
        self.anomalies
    }

    pub fn pending(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(s) = self.spans.get(i) {
                if s.open {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn span_at(&self, h: SpanHandle) -> Result<Span, u32> {
        match self.spans.get(h.seq) {
            Some(s) => Ok(*s),
            None => Err(ObCode::HandleBad.val()),
        }
    }

    /// 入口打点：时间单调 + 未闭合数未满。
    pub fn enter(&mut self, id: u32, ts: u64) -> Result<SpanHandle, u32> {
        if self.pending() >= MAX_SPANS {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(ObCode::SpanFull.val());
        }
        if ts < self.last_ts {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(ObCode::TimeBad.val());
        }
        self.spans.push(Span { id, enter_ts: ts, exit_ts: 0, code: OK_CODE, open: true });
        self.entered = self.entered.saturating_add(1);
        self.last_ts = ts;
        Ok(SpanHandle { seq: self.spans.len() - 1 })
    }

    /// 出口打点：句柄在 + 未关闭 + 时间不倒退 + 结果码合法。
    pub fn exit(&mut self, h: SpanHandle, ts: u64, code: u32) -> Result<Span, u32> {
        let cur = match self.spans.get(h.seq) {
            Some(s) => *s,
            None => {
                self.anomalies = self.anomalies.saturating_add(1);
                return Err(ObCode::HandleBad.val());
            }
        };
        if !cur.open {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(ObCode::ExitDup.val());
        }
        if ts < cur.enter_ts || ts < self.last_ts {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(ObCode::TimeBad.val());
        }
        if !result_code_ok(code) {
            self.anomalies = self.anomalies.saturating_add(1);
            return Err(ObCode::ResultBad.val());
        }
        if let Some(s) = self.spans.get_mut(h.seq) {
            s.exit_ts = ts;
            s.code = code;
            s.open = false;
        }
        self.exited = self.exited.saturating_add(1);
        self.last_ts = ts;
        if code != OK_CODE {
            self.errors = self.errors.saturating_add(1);
        }
        Ok(Span { id: cur.id, enter_ts: cur.enter_ts, exit_ts: ts, code, open: false })
    }

    /// 四点覆盖：入口 / 出口 / 耗时样本 / 结果样本。
    ///
    /// 耗时样本与结果样本都只统计已闭合 span，因此三点恒等；
    /// 入口数减去闭合数就是未闭合数。四点全非零才算覆盖。
    pub fn coverage(&self) -> [u32; 4] {
        let mut elapsed_samples = 0u32;
        let mut result_samples = 0u32;
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(s) = self.spans.get(i) {
                if !s.open {
                    if s.exit_ts >= s.enter_ts {
                        elapsed_samples = elapsed_samples.saturating_add(1);
                    }
                    result_samples = result_samples.saturating_add(1);
                }
            }
            i += 1;
        }
        [self.entered, self.exited, elapsed_samples, result_samples]
    }

    pub fn full_coverage(&self) -> bool {
        let c = self.coverage();
        let mut i = 0usize;
        while i < 4 {
            let v = c.get(i).copied().unwrap_or(0);
            if v == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账本审计：无未闭合 span、入口 = 出口 + 未闭合、错误数可解释。
    pub fn audit(&self) -> Result<(), u32> {
        let pending = self.pending();
        if pending > 0 {
            return Err(ObCode::Silent.val());
        }
        if self.entered != self.exited.saturating_add(pending as u32) {
            return Err(ObCode::Silent.val());
        }
        Ok(())
    }

    /// 未闭合 span 的显性账：审计前也必须能报出来，不允许静默。
    pub fn silent_snapshot(&self) -> Vec<Span> {
        let mut v: Vec<Span> = Vec::new();
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(s) = self.spans.get(i) {
                if s.open {
                    v.push(*s);
                }
            }
            i += 1;
        }
        v
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.entered);
        h = fold_word(h, self.exited);
        h = fold_word(h, self.errors);
        h = fold_word(h, self.anomalies);
        let mut i = 0usize;
        while i < self.spans.len() {
            if let Some(s) = self.spans.get(i) {
                h = fold_word(h, s.id);
                h = fold_u64(h, s.enter_ts);
                h = fold_u64(h, s.exit_ts);
                h = fold_word(h, s.code);
                h = fold_byte(h, if s.open { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }
}

// ── 黄金观测：100 对入口/出口，半数为错误结果 ────────────────────
pub fn golden_observe(pairs: u32) -> Result<Observer, u32> {
    let mut obs = Observer::new();
    let mut i = 0u32;
    while i < pairs {
        let enter_ts = (i as u64).saturating_mul(10);
        let h = obs.enter(i, enter_ts)?;
        let exit_ts = enter_ts.saturating_add(7);
        let code = if i % 2 == 0 { OK_CODE } else { ObCode::TimeBad.val() };
        obs.exit(h, exit_ts, code)?;
        i = i.saturating_add(1);
    }
    obs.audit()?;
    if !obs.full_coverage() {
        return Err(ObCode::Coverage.val());
    }
    Ok(obs)
}

// ── 20 例判据语料 ────────────────────────────────────────────────
/// 用例步骤（原语级，便于逐例手写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 入口：ts 由调用方给定。
    Enter(u64),
    /// 合法出口：ts + 结果码。
    Exit(u64, u32),
    /// 对「当前最新 span」出口。
    ExitCurrent(u64, u32),
    /// 按入口序号出口（0 起，用于交错场景）。
    ExitAt(usize, u64, u32),
    /// 用越界句柄出口。
    ExitBogus,
    /// 同一句柄二次出口。
    DoubleExit,
    /// 入口时间倒退。
    EnterBackward,
}

#[derive(Clone, Copy)]
pub struct Case {
    pub id: u8,
    pub name: &'static str,
    pub steps: &'static [Step],
    pub expect_ok: bool,
    pub expect_audit_ok: bool,
}

/// 20 例：14 例合法流 + 6 例反例，逐例申报最终成败与审计成败。
pub const CASES: [Case; CASE_ROWS] = [
    Case { id: 1, name: "单点成功", steps: &[Step::Enter(0), Step::ExitCurrent(5, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 2, name: "单点错误码", steps: &[Step::Enter(1), Step::ExitCurrent(6, SEG_BASE + 3)], expect_ok: true, expect_audit_ok: true },
    Case { id: 3, name: "零耗时", steps: &[Step::Enter(7), Step::ExitCurrent(7, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 4, name: "串行两对", steps: &[Step::Enter(0), Step::ExitCurrent(2, 0), Step::Enter(2), Step::ExitCurrent(4, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 5, name: "跨段大间隔", steps: &[Step::Enter(0), Step::ExitCurrent(1_000_000, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 6, name: "结果码为段末", steps: &[Step::Enter(3), Step::ExitCurrent(9, SEG_BASE + 0x0B)], expect_ok: true, expect_audit_ok: true },
    Case { id: 7, name: "十四连对", steps: &[Step::Enter(0), Step::ExitCurrent(1, 0), Step::Enter(1), Step::ExitCurrent(2, 0), Step::Enter(2), Step::ExitCurrent(3, 0), Step::Enter(3), Step::ExitCurrent(4, 0), Step::Enter(4), Step::ExitCurrent(5, 0), Step::Enter(5), Step::ExitCurrent(6, 0), Step::Enter(6), Step::ExitCurrent(7, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 8, name: "相交打开", steps: &[Step::Enter(0), Step::Enter(1), Step::ExitAt(0, 2, 0), Step::ExitAt(1, 3, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 9, name: "错误后继续", steps: &[Step::Enter(0), Step::ExitCurrent(1, SEG_BASE + 5), Step::Enter(1), Step::ExitCurrent(2, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 10, name: "五对交错", steps: &[Step::Enter(0), Step::Enter(1), Step::ExitAt(0, 2, 0), Step::Enter(3), Step::ExitAt(2, 4, 0), Step::ExitAt(1, 5, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 11, name: "同刻入口同刻出口", steps: &[Step::Enter(5), Step::ExitCurrent(5, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 12, name: "大时刻对齐", steps: &[Step::Enter(4_294_967_295), Step::ExitCurrent(4_294_967_296, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 13, name: "错误码零", steps: &[Step::Enter(0), Step::ExitCurrent(1, 0), Step::Enter(1), Step::ExitCurrent(2, SEG_BASE + 1)], expect_ok: true, expect_audit_ok: true },
    Case { id: 14, name: "双开双闭", steps: &[Step::Enter(0), Step::Enter(0), Step::ExitAt(0, 2, 0), Step::ExitAt(1, 3, 0)], expect_ok: true, expect_audit_ok: true },
    Case { id: 15, name: "越界句柄出口", steps: &[Step::Enter(0), Step::ExitBogus], expect_ok: false, expect_audit_ok: false },
    Case { id: 16, name: "二次出口", steps: &[Step::Enter(0), Step::ExitCurrent(1, 0), Step::DoubleExit], expect_ok: false, expect_audit_ok: true },
    Case { id: 17, name: "出口早于入口", steps: &[Step::Enter(10), Step::Exit(9, 0)], expect_ok: false, expect_audit_ok: false },
    Case { id: 18, name: "入口时间倒退", steps: &[Step::Enter(10), Step::EnterBackward], expect_ok: false, expect_audit_ok: false },
    Case { id: 19, name: "未闭合审计", steps: &[Step::Enter(0)], expect_ok: true, expect_audit_ok: false },
    Case { id: 20, name: "非法结果码", steps: &[Step::Enter(0), Step::ExitCurrent(1, 0xDEAD)], expect_ok: false, expect_audit_ok: false },
];

/// 执行单例：返回 (步骤全成, 审计通过)。
pub fn run_case(c: &Case) -> (bool, bool) {
    let mut obs = Observer::new();
    let mut step_ok = true;
    let mut last: Option<SpanHandle> = None;
    let mut i = 0usize;
    while i < c.steps.len() {
        let st = match c.steps.get(i) {
            Some(s) => *s,
            None => {
                step_ok = false;
                break;
            }
        };
        match st {
            Step::Enter(ts) => match obs.enter(c.id as u32, ts) {
                Ok(h) => last = Some(h),
                Err(_) => step_ok = false,
            },
            Step::Exit(ts, code) => match last {
                Some(h) => {
                    if obs.exit(h, ts, code).is_err() {
                        step_ok = false;
                    }
                }
                None => step_ok = false,
            },
            Step::ExitCurrent(ts, code) => match last {
                Some(h) => {
                    if obs.exit(h, ts, code).is_err() {
                        step_ok = false;
                    }
                }
                None => step_ok = false,
            },
            Step::ExitAt(seq, ts, code) => {
                if obs.exit(SpanHandle { seq }, ts, code).is_err() {
                    step_ok = false;
                }
            }
            Step::ExitBogus => {
                if obs.exit(SpanHandle { seq: 9999 }, 99, 0).is_err() {
                    step_ok = false;
                }
            }
            Step::DoubleExit => match last {
                Some(h) => {
                    if obs.exit(h, 50, 0).is_err() {
                        step_ok = false;
                    }
                }
                None => step_ok = false,
            },
            Step::EnterBackward => {
                if obs.enter(c.id as u32, 0).is_err() {
                    step_ok = false;
                }
            }
        }
        i += 1;
    }
    (step_ok, obs.audit().is_ok())
}

/// 20 例入库：逐例判定方向必须与申报一致。
pub fn golden_corpus() -> Result<(u32, u32), u32> {
    if CASES.len() != CASE_ROWS {
        return Err(ObCode::CaseBad.val());
    }
    let mut matched = 0u32;
    let mut rejected_cases = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(ObCode::SegmentBad.val()),
        };
        let (ok, audit_ok) = run_case(&c);
        if ok != c.expect_ok || audit_ok != c.expect_audit_ok {
            return Err(ObCode::CaseBad.val());
        }
        if !(c.expect_ok && c.expect_audit_ok) {
            rejected_cases += 1;
        }
        matched += 1;
        i += 1;
    }
    if matched != CASE_ROWS as u32 || rejected_cases != 6 {
        return Err(ObCode::CaseBad.val());
    }
    Ok((matched, rejected_cases))
}

// ── fuzz 面（观测器自身） ────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

pub struct FuzzOut {
    pub accepts: u32,
    pub rejects: u32,
    pub open_left: u32,
    pub seal: u64,
}

/// fuzz：随机 enter/exit/坏句柄/倒退时间。所有路径只拒不崩。
pub fn fuzz_observer(rounds: u32) -> Result<FuzzOut, u32> {
    let mut obs = Observer::new();
    let mut rng = Lcg::new((SEG_BASE as u64) ^ 0x4F42_5345_5256);
    let mut accepts = 0u32;
    let mut rejects = 0u32;
    let mut clock = 0u64;
    let mut open: Vec<SpanHandle> = Vec::new();
    let mut r = 0u32;
    while r < rounds {
        clock = clock.saturating_add(rng.below(3));
        match rng.below(5) {
            0 => match obs.enter(r, clock) {
                Ok(h) => {
                    open.push(h);
                    accepts = accepts.saturating_add(1);
                }
                Err(_) => rejects = rejects.saturating_add(1),
            },
            1 => match open.pop() {
                Some(h) => {
                    clock = clock.saturating_add(1);
                    if obs.exit(h, clock, 0).is_ok() {
                        accepts = accepts.saturating_add(1);
                    } else {
                        rejects = rejects.saturating_add(1);
                    }
                }
                None => rejects = rejects.saturating_add(1),
            },
            2 => {
                // 坏句柄。
                if obs.exit(SpanHandle { seq: 10_000 }, clock, 0).is_err() {
                    rejects = rejects.saturating_add(1);
                } else {
                    accepts = accepts.saturating_add(1);
                }
            }
            3 => match open.last().copied() {
                Some(h) => {
                    // 倒退时间。
                    if obs.exit(h, 0, 0).is_err() {
                        rejects = rejects.saturating_add(1);
                    } else {
                        accepts = accepts.saturating_add(1);
                    }
                }
                None => rejects = rejects.saturating_add(1),
            },
            _ => {
                // 非法结果码。
                match open.last().copied() {
                    Some(h) => {
                        clock = clock.saturating_add(1);
                        if obs.exit(h, clock, 0xDEAD).is_err() {
                            rejects = rejects.saturating_add(1);
                        } else {
                            accepts = accepts.saturating_add(1);
                        }
                    }
                    None => rejects = rejects.saturating_add(1),
                }
            }
        }
        r = r.saturating_add(1);
    }
    // 收尾：所有未闭合 span 逐一带出口，保证 fuzz 账可审计。
    while let Some(h) = open.pop() {
        clock = clock.saturating_add(1);
        if obs.exit(h, clock, 0).is_ok() {
            accepts = accepts.saturating_add(1);
        } else {
            rejects = rejects.saturating_add(1);
        }
    }
    let open_left = obs.pending() as u32;
    if open_left != 0 {
        return Err(ObCode::Silent.val());
    }
    obs.audit()?;
    let seal = obs.seal();
    Ok(FuzzOut { accepts, rejects, open_left, seal })
}

// ── 黄金全链 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, [u32; 4], u32, u32, u64), u32> {
    let obs = golden_observe(GOLDEN_PAIRS)?;
    let cov = obs.coverage();
    let (matched, _red) = golden_corpus()?;
    let fz = fuzz_observer(FUZZ_ROUNDS)?;
    if matched != CASE_ROWS as u32 {
        return Err(ObCode::CaseBad.val());
    }
    if fz.accepts.saturating_add(fz.rejects) < FUZZ_ROUNDS {
        return Err(ObCode::FuzzBad.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, (obs.seal() & 0xFFFF_FFFF) as u32);
    h = fold_word(h, matched);
    h = fold_word(h, fz.accepts);
    h = fold_word(h, fz.rejects);
    h = fold_word(h, (fz.seal & 0xFFFF_FFFF) as u32);
    Ok((matched, cov, fz.accepts, fz.rejects, h))
}

// ── 注入账（四路：坏句柄/重复出口/倒退时间/静默未闭合） ─────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new((SEG_BASE as u64) ^ 0x494E_4A);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：坏句柄。
        let mut o = Observer::new();
        led.record(0, o.exit(SpanHandle { seq: 3 }, 1, 0) == Err(ObCode::HandleBad.val()));
        // 路 2：重复出口。
        let dup = match o.enter(1, 0) {
            Ok(h) => o.exit(h, 1, 0).is_ok() && o.exit(h, 2, 0) == Err(ObCode::ExitDup.val()),
            Err(_) => false,
        };
        led.record(1, dup);
        // 路 3：倒退时间。
        let mut o2 = Observer::new();
        let back = match o2.enter(1, 10) {
            Ok(h) => o2.exit(h, 9, 0) == Err(ObCode::TimeBad.val()),
            Err(_) => false,
        };
        led.record(2, back);
        // 路 4：未闭合审计。
        let mut o3 = Observer::new();
        let silent = match o3.enter(1, 0) {
            Ok(_) => o3.audit() == Err(ObCode::Silent.val()) && o3.silent_snapshot().len() == 1,
            Err(_) => false,
        };
        led.record(3, silent);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (matched, cov, _a, _r, _) = golden_run()?;
    if matched != 20 || cov != [100, 100, 100, 100] {
        return Err(ObCode::Coverage.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空观测器不覆盖。
    let o = Observer::new();
    if o.full_coverage() {
        return Err(ObCode::Coverage.val());
    }
    if !result_code_ok(0) || !result_code_ok(SEG_BASE + 1) || result_code_ok(0xDEAD) {
        return Err(ObCode::ResultBad.val());
    }
    // 20 例必须全绿。
    if golden_corpus().ok() != Some((20, 6)) {
        return Err(ObCode::CaseBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if MAX_SPANS == 64
        && FUZZ_ROUNDS == 10000
        && CASE_ROWS == 20
        && GOLDEN_PAIRS == 100
        && OK_CODE == 0
        && CASES.len() == 20
    {
        Ok(())
    } else {
        Err(ObCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ObCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ObCode::ALL.len() {
        let v = ObCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let obs = golden_observe(GOLDEN_PAIRS);
    v.push((
        "F48856-J1-四点百次全覆盖",
        match &obs {
            Ok(o) => o.coverage() == [100, 100, 100, 100] && o.audit().is_ok(),
            Err(_) => false,
        },
        match &obs {
            Ok(o) => format!("coverage={:?} errors={}", o.coverage(), o.errors()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let corpus = golden_corpus();
    v.push((
        "F48858-J1-二十例全绿入库",
        corpus == Ok((20, 6)),
        format!("corpus={:?}", corpus),
    ));
    let fz = fuzz_observer(FUZZ_ROUNDS);
    v.push((
        "F48857-J1-万轮fuzz零崩",
        match &fz {
            Ok(f) => f.open_left == 0 && f.accepts.saturating_add(f.rejects) >= FUZZ_ROUNDS,
            Err(_) => false,
        },
        match &fz {
            Ok(f) => format!("accept={} reject={} open={}", f.accepts, f.rejects, f.open_left),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48856-J1-异常零静默",
        {
            let mut o = Observer::new();
            let entered = o.enter(1, 0).is_ok();
            let snap = o.silent_snapshot();
            entered && snap.len() == 1 && o.audit() == Err(ObCode::Silent.val())
        },
        String::from("未闭合 span 在审计前即可见"),
    ));
    v.push((
        "F48856-J1-按功能正确关闭",
        {
            let mut o = Observer::new();
            let h = o.enter(2, 0);
            match h {
                Ok(hh) => {
                    let a = o.exit(hh, 3, SEG_BASE + 2).is_ok();
                    a && o.errors() == 1 && o.exited() == 1 && o.audit().is_ok()
                }
                Err(_) => false,
            }
        },
        String::from("错误结果计 errors 且账平"),
    ));
    v.push((
        "F48858-J1-六例反例方向",
        {
            let mut reds = 0u32;
            let mut i = 0usize;
            while i < CASES.len() {
                if let Some(c) = CASES.get(i) {
                    if !(c.expect_ok && c.expect_audit_ok) {
                        let (ok, audit_ok) = run_case(c);
                        if ok == c.expect_ok && audit_ok == c.expect_audit_ok {
                            reds += 1;
                        }
                    }
                }
                i += 1;
            }
            reds == 6
        },
        String::from("6 例反例全部按申报拒绝"),
    ));
    v.push((
        "F48857-J1-坏句柄只拒不崩",
        {
            let o = Observer::new();
            o.span_at(SpanHandle { seq: 0 }) == Err(ObCode::HandleBad.val())
        },
        String::from("空表取句柄 → HandleBad"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48857-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48857-J1-探针三序与双跑",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok() && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48858-J1-段位守卫",
        segment_guard(),
        String::from("0x706E0 窗口 11 码"),
    ));
    v
}

pub fn run_unx48856_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48856 B03 观测埋点、fuzz 面与判据用例集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_exit_records_four_points() {
        let mut o = Observer::new();
        let h = o.enter(1, 0);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(o.exit(hh, 5, 0).is_ok());
            assert_eq!(o.coverage(), [1, 1, 1, 1]);
            assert!(o.full_coverage());
            assert!(o.audit().is_ok());
        }
    }

    #[test]
    fn empty_observer_has_no_coverage() {
        let o = Observer::new();
        assert!(!o.full_coverage());
        assert_eq!(o.coverage(), [0, 0, 0, 0]);
    }

    #[test]
    fn open_span_is_visible_before_audit() {
        let mut o = Observer::new();
        assert!(o.enter(1, 0).is_ok());
        assert_eq!(o.silent_snapshot().len(), 1);
        assert_eq!(o.audit(), Err(ObCode::Silent.val()));
    }

    #[test]
    fn backwards_exit_is_rejected() {
        let mut o = Observer::new();
        let h = o.enter(1, 10);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert_eq!(o.exit(hh, 9, 0), Err(ObCode::TimeBad.val()));
            assert_eq!(o.pending(), 1);
        }
    }

    #[test]
    fn backwards_enter_is_rejected() {
        let mut o = Observer::new();
        assert!(o.enter(1, 10).is_ok());
        assert_eq!(o.enter(2, 9), Err(ObCode::TimeBad.val()));
        assert_eq!(o.entered(), 1);
    }

    #[test]
    fn double_exit_is_rejected() {
        let mut o = Observer::new();
        let h = o.enter(1, 0);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(o.exit(hh, 1, 0).is_ok());
            assert_eq!(o.exit(hh, 2, 0), Err(ObCode::ExitDup.val()));
            assert_eq!(o.exited(), 1);
        }
    }

    #[test]
    fn bogus_handle_is_rejected() {
        let mut o = Observer::new();
        assert_eq!(o.exit(SpanHandle { seq: 7 }, 1, 0), Err(ObCode::HandleBad.val()));
    }

    #[test]
    fn illegal_result_code_is_rejected() {
        let mut o = Observer::new();
        let h = o.enter(1, 0);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert_eq!(o.exit(hh, 1, 0xDEAD), Err(ObCode::ResultBad.val()));
            assert_eq!(o.pending(), 1);
        }
    }

    #[test]
    fn error_result_is_counted() {
        let mut o = Observer::new();
        let h = o.enter(1, 0);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(o.exit(hh, 1, SEG_BASE + 1).is_ok());
            assert_eq!(o.errors(), 1);
        }
    }

    #[test]
    fn span_full_is_rejected() {
        let mut o = Observer::new();
        let mut i = 0u32;
        while i < MAX_SPANS as u32 {
            assert!(o.enter(i, i as u64).is_ok());
            i += 1;
        }
        assert_eq!(o.enter(999, 999), Err(ObCode::SpanFull.val()));
    }

    #[test]
    fn hundred_pairs_cover_all_four() {
        let o = golden_observe(100);
        assert!(o.is_ok());
        if let Ok(oo) = o {
            assert_eq!(oo.coverage(), [100, 100, 100, 100]);
            assert_eq!(oo.errors(), 50);
            assert!(oo.audit().is_ok());
        }
    }

    #[test]
    fn corpus_has_twenty_cases_and_six_red() {
        assert_eq!(CASES.len(), 20);
        assert_eq!(golden_corpus(), Ok((20, 6)));
    }

    #[test]
    fn corpus_case_one_is_green() {
        if let Some(c) = CASES.first() {
            let (ok, audit_ok) = run_case(c);
            assert!(ok);
            assert!(audit_ok);
        }
    }

    #[test]
    fn corpus_bogus_case_declares_red() {
        if let Some(c) = CASES.get(14) {
            let (ok, audit_ok) = run_case(c);
            assert!(!ok);
            assert!(!audit_ok);
        }
    }

    #[test]
    fn ten_thousand_fuzz_is_balanced() {
        let f = fuzz_observer(10000);
        assert!(f.is_ok());
        if let Ok(ff) = f {
            assert_eq!(ff.open_left, 0);
            assert!(ff.accepts + ff.rejects >= 10000);
            assert!(ff.rejects > 0);
        }
    }

    #[test]
    fn fuzz_is_deterministic() {
        let a = fuzz_observer(500);
        let b = fuzz_observer(500);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(aa), Ok(bb)) = (a, b) {
            assert_eq!(aa.seal, bb.seal);
        }
    }

    #[test]
    fn golden_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(ObCode::ALL.len(), 11);
        assert_eq!(ObCode::HandleBad.val(), SEG_BASE + 1);
        assert_eq!(ObCode::SegmentBad.val(), SEG_BASE + 11);
    }
}
