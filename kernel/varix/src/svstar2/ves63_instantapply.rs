//! VE-F3663 · 个性化即时生效链（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3663`
//!
//! **职责定位（锚点原文）**：个性化即时生效——
//! - **生效链**：改值 → 广播 → 应用面更新（生效链可查）；
//! - **延迟红线**：生效延迟可测（超时立案）；
//! - **部分生效可见**：多应用面部分生效必须显性（静默半生效 = 谜团）。
//!
//! **判据（锚点原文）**：生效链可查、延迟红线、部分生效可见、
//! 回退可查、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 生效延迟超时→立案 | [`L63Code::APPLY_TIMEOUT`] |
//! | 静默半生效→P1（红线实测） | [`L63Code::PARTIAL_SILENT`] |
//! | 链断→回退可见 | [`L63Code::CHAIN_BROKEN`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3663 诊断码段（独占 0x6Fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct L63Code(pub u16);

impl L63Code {
    /// 生效延迟超时（立案）。
    pub const APPLY_TIMEOUT: L63Code = L63Code(0x6F01);
    /// 静默半生效（P1 红线实测）。
    pub const PARTIAL_SILENT: L63Code = L63Code(0x6F02);
    /// 生效链断（回退可见）。
    pub const CHAIN_BROKEN: L63Code = L63Code(0x6F03);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            L63Code::APPLY_TIMEOUT => "生效延迟超时（立案）。建议：排查应用面响应。",
            L63Code::PARTIAL_SILENT => "静默半生效（P1 红线实测）。建议：部分生效必须显性。",
            L63Code::CHAIN_BROKEN => "生效链断。建议：回退并显性可见。",
            _ => "未知诊断码。",
        }
    }
}

// ── 生效链三段 ──────────────────────────────────────────────────────

/// 链段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 改值。
    Change,
    /// 广播。
    Broadcast,
    /// 应用面更新。
    Apply,
}

impl Stage {
    pub const CHAIN: [Stage; 3] = [Stage::Change, Stage::Broadcast, Stage::Apply];
}

/// 应用面。
#[derive(Clone, Copy)]
pub struct Surface {
    pub id: u32,
    pub updated: bool,
}

// ── 生效链执行器 ────────────────────────────────────────────────────

/// 个性化即时生效链。
pub struct InstantApply {
    stages: Vec<Stage>,
    surfaces: Vec<Surface>,
    /// 延迟预算（毫秒）。
    budget_ms: u32,
    timeouts: u32,
    partial_hits: u32,
    rolled_back: bool,
    last_code: L63Code,
    has_error: bool,
}

impl InstantApply {
    /// 新建；`budget_ms` 为生效延迟红线。
    pub fn new(budget_ms: u32) -> Self {
        Self {
            stages: Vec::new(),
            surfaces: Vec::new(),
            budget_ms,
            timeouts: 0,
            partial_hits: 0,
            rolled_back: false,
            last_code: L63Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> L63Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: L63Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 走一链段。
    pub fn stage(&mut self, s: Stage) -> usize {
        self.stages.push(s);
        self.stages.len()
    }

    /// 链完整（三段全走）。
    pub fn chain_complete(&self) -> bool {
        Stage::CHAIN.iter().all(|s| self.stages.contains(s))
    }

    /// 登记应用面。
    pub fn add_surface(&mut self, id: u32) -> usize {
        self.surfaces.push(Surface { id, updated: false });
        self.surfaces.len()
    }

    /// 应用一面上的更新。
    pub fn update_surface(&mut self, id: u32) -> Result<bool, L63Code> {
        for s in self.surfaces.iter_mut() {
            if s.id == id {
                s.updated = true;
                self.last_code = L63Code(0);
                self.has_error = false;
                return Ok(true);
            }
        }
        self.fail(L63Code::CHAIN_BROKEN);
        Err(L63Code::CHAIN_BROKEN)
    }

    /// 延迟红线：超预算即立案。
    pub fn measure_latency(&mut self, elapsed_ms: u32) -> Result<u32, L63Code> {
        if elapsed_ms > self.budget_ms {
            self.timeouts += 1;
            self.fail(L63Code::APPLY_TIMEOUT);
            return Err(L63Code::APPLY_TIMEOUT);
        }
        self.last_code = L63Code(0);
        self.has_error = false;
        Ok(elapsed_ms)
    }

    /// 部分生效显性审计：有未更新面且未申报即红线。
    pub fn audit_partial(&mut self, declared_partial: bool) -> Result<bool, L63Code> {
        let pending = self.surfaces.iter().filter(|s| !s.updated).count();
        if pending > 0 && !declared_partial {
            self.partial_hits += 1;
            self.fail(L63Code::PARTIAL_SILENT);
            return Err(L63Code::PARTIAL_SILENT);
        }
        self.last_code = L63Code(0);
        self.has_error = false;
        Ok(pending == 0)
    }

    /// 链断回退（可见）。
    pub fn rollback(&mut self) -> Result<bool, L63Code> {
        if self.chain_complete() {
            self.fail(L63Code::CHAIN_BROKEN);
            return Err(L63Code::CHAIN_BROKEN);
        }
        self.rolled_back = true;
        self.last_code = L63Code(0);
        self.has_error = false;
        Ok(true)
    }

    pub fn timeouts(&self) -> u32 {
        self.timeouts
    }

    pub fn partial_hits(&self) -> u32 {
        self.partial_hits
    }

    pub fn rolled_back(&self) -> bool {
        self.rolled_back
    }

    pub fn surface_count(&self) -> usize {
        self.surfaces.len()
    }

    /// 全面已更新数。
    pub fn updated_count(&self) -> usize {
        self.surfaces.iter().filter(|s| s.updated).count()
    }

    /// 即时生效总评：链齐 + 无超时 + 无静默半生效。
    pub fn instant_green(&self) -> bool {
        self.chain_complete() && self.timeouts == 0 && self.partial_hits == 0
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves63_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves63_instantapply");
    {
        let a = InstantApply::new(100);
        chk(&mut set, "C3663-01", a.surface_count() == 0 && a.timeouts() == 0, "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.stage(Stage::Change);
        a.stage(Stage::Broadcast);
        a.stage(Stage::Apply);
        chk(&mut set, "C3663-02", a.chain_complete(), "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.stage(Stage::Change);
        chk(&mut set, "C3663-03", !a.chain_complete(), "err");
    }
    {
        let mut a = InstantApply::new(100);
        let n = a.add_surface(1);
        chk(&mut set, "C3663-04", n == 1, "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.add_surface(1);
        let r = a.update_surface(1);
        chk(&mut set, "C3663-05", r == Ok(true) && a.updated_count() == 1, "err");
    }
    {
        let mut a = InstantApply::new(100);
        let r = a.update_surface(42);
        chk(&mut set, "C3663-06", r.err() == Some(L63Code::CHAIN_BROKEN), "err");
    }
    {
        let mut a = InstantApply::new(100);
        let r = a.measure_latency(50);
        chk(&mut set, "C3663-07", r == Ok(50), "err");
    }
    {
        let mut a = InstantApply::new(100);
        let r = a.measure_latency(250);
        chk(&mut set, "C3663-08", r.err() == Some(L63Code::APPLY_TIMEOUT) && a.timeouts() == 1, "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.add_surface(1);
        a.add_surface(2);
        a.update_surface(1);
        let r = a.audit_partial(false);
        chk(&mut set, "C3663-09", r.err() == Some(L63Code::PARTIAL_SILENT), "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.add_surface(1);
        a.add_surface(2);
        a.update_surface(1);
        let r = a.audit_partial(true);
        chk(&mut set, "C3663-10", r == Ok(false), "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.stage(Stage::Change);
        let r = a.rollback();
        chk(&mut set, "C3663-11", r == Ok(true) && a.rolled_back(), "err");
    }
    {
        let mut a = InstantApply::new(100);
        a.stage(Stage::Change);
        a.stage(Stage::Broadcast);
        a.stage(Stage::Apply);
        a.add_surface(1);
        a.update_surface(1);
        chk(&mut set, "C3663-12", a.instant_green(), "err");
    }
    set
}
