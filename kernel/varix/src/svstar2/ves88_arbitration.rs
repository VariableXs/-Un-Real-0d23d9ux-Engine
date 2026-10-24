//! VE-F3688 · 举报与仲裁（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3688`
//!
//! **职责定位（锚点原文）**：举报与仲裁——
//! - **举报通道**：匿名选项（实名举报遭报复 = 寒蝉——匿名保护 + 可追责平衡）；
//! - **仲裁流程**：举报→初审→申诉→终裁四步（初审 48h——石沉大海 = 治理失效）；
//! - **仲裁独立**：利益回避申报（利害相关仲裁 = 不公）；
//! - **报复防护**：报复行为零容忍（报复 P0）。
//!
//! **判据（锚点原文）**：四步流程、时限断言、回避断言、报复 P0、匿名保护、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 报复检出→P0（红线最高级） | [`C88Code::RETALIATION`] |
//! | 超时未裁→P1（红线实测） | [`C88Code::TIMEOUT`] |
//! | 回避缺失→仲裁无效+重裁 | [`C88Code::CONFLICT_UNRECUSED`] |
//! | 匿名泄露→P0（红线最高级） | [`C88Code::ANON_LEAK`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3688 诊断码段（独占 0x88xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C88Code(pub u16);

impl C88Code {
    /// 报复检出（P0 红线最高级）。
    pub const RETALIATION: C88Code = C88Code(0x8801);
    /// 超时未裁（P1 红线实测）。
    pub const TIMEOUT: C88Code = C88Code(0x8802);
    /// 回避缺失（仲裁无效）。
    pub const CONFLICT_UNRECUSED: C88Code = C88Code(0x8803);
    /// 匿名泄露（P0 红线最高级）。
    pub const ANON_LEAK: C88Code = C88Code(0x8804);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C88Code::RETALIATION => "报复行为检出（P0 红线最高级）。建议：零容忍处置。",
            C88Code::TIMEOUT => "仲裁超时未裁（P1 红线实测）。建议：立案催办。",
            C88Code::CONFLICT_UNRECUSED => "利益冲突未回避。建议：仲裁无效并重裁。",
            C88Code::ANON_LEAK => "匿名举报泄露（P0 红线最高级）。建议：保护修复。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四步流程 ────────────────────────────────────────────────────────

/// 仲裁步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArbStep {
    /// 举报。
    Report,
    /// 初审。
    FirstReview,
    /// 申诉。
    Appeal,
    /// 终裁。
    FinalRuling,
}

impl ArbStep {
    pub const CHRONO: [ArbStep; 4] = [
        ArbStep::Report,
        ArbStep::FirstReview,
        ArbStep::Appeal,
        ArbStep::FinalRuling,
    ];
}

/// 仲裁单。
pub struct Arbitration {
    steps_done: Vec<ArbStep>,
    /// 初审时限（tick）。
    pub due_tick: u32,
    /// 当前 tick。
    pub now_tick: u32,
    /// 是否匿名举报。
    pub anonymous: bool,
    /// 匿名身份是否暴露。
    pub id_exposed: bool,
    timeout_hits: u32,
    last_code: C88Code,
    has_error: bool,
}

impl Arbitration {
    pub fn new(anonymous: bool, due_tick: u32) -> Self {
        Self {
            steps_done: Vec::new(),
            due_tick,
            now_tick: 0,
            anonymous,
            id_exposed: false,
            timeout_hits: 0,
            last_code: C88Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C88Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 完成一步。
    pub fn complete_step(&mut self, step: ArbStep) {
        if !self.steps_done.contains(&step) {
            self.steps_done.push(step);
        }
    }

    /// 四步齐备。
    pub fn complete(&self) -> bool {
        ArbStep::CHRONO.iter().all(|s| self.steps_done.contains(s))
    }

    /// 时限断言：初审未完成即超时。
    pub fn assert_timing(&mut self) -> Result<bool, C88Code> {
        let first_done = self.steps_done.contains(&ArbStep::FirstReview);
        if first_done || self.now_tick <= self.due_tick {
            self.last_code = C88Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.timeout_hits += 1;
            self.last_code = C88Code::TIMEOUT;
            self.has_error = true;
            Err(C88Code::TIMEOUT)
        }
    }

    /// 匿名保护断言。
    pub fn assert_anon(&mut self) -> Result<bool, C88Code> {
        if self.anonymous && self.id_exposed {
            self.last_code = C88Code::ANON_LEAK;
            self.has_error = true;
            Err(C88Code::ANON_LEAK)
        } else {
            self.last_code = C88Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    /// 暴露身份（违规路径）。
    pub fn expose_id(&mut self) {
        self.id_exposed = true;
    }

    pub fn step_count(&self) -> usize {
        self.steps_done.len()
    }

    pub fn timeout_hits(&self) -> u32 {
        self.timeout_hits
    }
}

// ── 回避申报 ────────────────────────────────────────────────────────

/// 仲裁员。
#[derive(Clone, Copy)]
pub struct Arbiter {
    pub id: u8,
    /// 是否利益相关。
    pub conflicted: bool,
    /// 是否已申报回避。
    pub recused: bool,
}

/// 回避申报器。
pub struct RecusalDesk {
    arbiters: Vec<Arbiter>,
    unrecused_hits: u32,
    last_code: C88Code,
    has_error: bool,
}

impl RecusalDesk {
    pub fn new() -> Self {
        Self {
            arbiters: Vec::new(),
            unrecused_hits: 0,
            last_code: C88Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C88Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn seat(&mut self, a: Arbiter) -> usize {
        self.arbiters.push(a);
        self.arbiters.len()
    }

    /// 回避断言：利益相关必须回避。
    pub fn assert_recused(&mut self) -> Result<bool, C88Code> {
        let mut ok = true;
        for a in &self.arbiters {
            if a.conflicted && !a.recused {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C88Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.unrecused_hits += 1;
            self.last_code = C88Code::CONFLICT_UNRECUSED;
            self.has_error = true;
            Err(C88Code::CONFLICT_UNRECUSED)
        }
    }

    pub fn unrecused_hits(&self) -> u32 {
        self.unrecused_hits
    }

    pub fn seat_count(&self) -> usize {
        self.arbiters.len()
    }
}

// ── 报复检测 ────────────────────────────────────────────────────────

/// 报复事件。
#[derive(Clone, Copy)]
pub struct RetaliationEvent {
    /// 是否针对举报人。
    pub targets_reporter: bool,
    /// 是否造成损害。
    pub caused_harm: bool,
}

/// 报复检测（零容忍 P0）。
pub struct RetaliationGuard {
    events: Vec<RetaliationEvent>,
    hits: u32,
    last_code: C88Code,
    has_error: bool,
}

impl RetaliationGuard {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            hits: 0,
            last_code: C88Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C88Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn ingest(&mut self, e: RetaliationEvent) {
        self.events.push(e);
    }

    /// 报复检测：针对举报人即 P0。
    pub fn audit(&mut self) -> Result<bool, C88Code> {
        let mut ok = true;
        for e in &self.events {
            if e.targets_reporter {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C88Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.hits += 1;
            self.last_code = C88Code::RETALIATION;
            self.has_error = true;
            Err(C88Code::RETALIATION)
        }
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves88_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves88_arbitration");
    {
        let a = Arbitration::new(false, 100);
        chk(&mut set, "C3688-01", !a.complete() && a.step_count() == 0, "err");
    }
    {
        let mut a = Arbitration::new(false, 100);
        for s in ArbStep::CHRONO {
            a.complete_step(s);
        }
        chk(&mut set, "C3688-02", a.complete(), "err");
    }
    {
        let mut a = Arbitration::new(false, 50);
        a.now_tick = 80;
        let res = a.assert_timing();
        chk(&mut set, "C3688-03", res.err() == Some(C88Code::TIMEOUT), "err");
    }
    {
        let mut a = Arbitration::new(false, 50);
        a.now_tick = 80;
        a.complete_step(ArbStep::FirstReview);
        chk(&mut set, "C3688-04", a.assert_timing() == Ok(true), "err");
    }
    {
        let mut a = Arbitration::new(true, 100);
        a.expose_id();
        let res = a.assert_anon();
        chk(&mut set, "C3688-05", res.err() == Some(C88Code::ANON_LEAK), "err");
    }
    {
        let mut a = Arbitration::new(true, 100);
        chk(&mut set, "C3688-06", a.assert_anon() == Ok(true), "err");
    }
    {
        let mut d = RecusalDesk::new();
        d.seat(Arbiter { id: 1, conflicted: false, recused: false });
        d.seat(Arbiter { id: 2, conflicted: true, recused: true });
        chk(&mut set, "C3688-07", d.assert_recused() == Ok(true), "err");
    }
    {
        let mut d = RecusalDesk::new();
        d.seat(Arbiter { id: 1, conflicted: true, recused: false });
        let res = d.assert_recused();
        chk(&mut set, "C3688-08", res.err() == Some(C88Code::CONFLICT_UNRECUSED), "err");
    }
    {
        let mut g = RetaliationGuard::new();
        g.ingest(RetaliationEvent { targets_reporter: false, caused_harm: true });
        chk(&mut set, "C3688-09", g.audit() == Ok(true), "err");
    }
    {
        let mut g = RetaliationGuard::new();
        g.ingest(RetaliationEvent { targets_reporter: true, caused_harm: true });
        let res = g.audit();
        chk(&mut set, "C3688-10", res.err() == Some(C88Code::RETALIATION), "err");
    }
    {
        let mut a = Arbitration::new(false, 100);
        a.complete_step(ArbStep::Report);
        a.complete_step(ArbStep::FirstReview);
        chk(&mut set, "C3688-11", a.step_count() == 2 && !a.complete(), "err");
    }
    {
        let mut g = RetaliationGuard::new();
        chk(&mut set, "C3688-12", g.audit() == Ok(true) && g.hits() == 0, "err");
    }
    set
}
