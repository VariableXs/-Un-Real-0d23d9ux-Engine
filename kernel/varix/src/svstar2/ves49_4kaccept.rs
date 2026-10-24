//! VE-F3649 · 4K 精度创作验收（VE-S 域 · 批次 R03 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3649`
//!
//! **职责定位（锚点原文）**：视觉创作 4K 精度验收（Variable 硬指标执法条）——
//! - **4K 门**：产物导出时 4K 检查（边缘锐度/缩放完好/纹理清晰）（4K 门红线：
//!   不过 4K 门的资产降级标记（标记显性，不静默上架——静默上架 = 精度欺诈，
//!   欺诈红线 P0）；
//! - **源分辨率**：源不足提示（源红线：低源放大 ≠ 4K → 伪 4K 检测
//!   = 上采样痕迹扫描）；
//! - **验收流程**：人工目检 + 自动检测双验（复述双验证）。
//!
//! **判据（锚点原文六条）**：4K 门、欺诈 P0、伪 4K 检测、双验证、
//! 总则执法、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 静默上架→P0（欺诈红线实测——审计扫描） | [`Q49Code::SILENT_LISTING`] |
//! | 伪 4K→检出标记（红线实测） | [`Q49Code::FAKE_4K`] |
//! | 门缺项→补（复述） | [`Q49Code::CHECK_INCOMPLETE`] |
//! | 降级未显性→立案（复述） | [`Q49Code::DEGRADE_SILENT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3649 诊断码段（独占 0x61xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Q49Code(pub u16);

impl Q49Code {
    /// 静默上架（欺诈红线 P0）。
    pub const SILENT_LISTING: Q49Code = Q49Code(0x6101);
    /// 伪 4K（上采样痕迹）。
    pub const FAKE_4K: Q49Code = Q49Code(0x6102);
    /// 4K 门缺项。
    pub const CHECK_INCOMPLETE: Q49Code = Q49Code(0x6103);
    /// 降级未显性（立案）。
    pub const DEGRADE_SILENT: Q49Code = Q49Code(0x6104);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Q49Code::SILENT_LISTING => "静默上架（欺诈红线 P0）。建议：降级标记显性后再上架。",
            Q49Code::FAKE_4K => "伪 4K 检出（上采样痕迹）。建议：标注真实分辨率。",
            Q49Code::CHECK_INCOMPLETE => "4K 门缺项。建议：补边缘锐度/缩放完好/纹理清晰。",
            Q49Code::DEGRADE_SILENT => "降级未显性。建议：立案并补降级标记。",
            _ => "未知诊断码。",
        }
    }
}

// ── 4K 检查三项 ─────────────────────────────────────────────────────

/// 检查项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// 边缘锐度。
    Sharpness,
    /// 缩放完好。
    ScaleIntact,
    /// 纹理清晰。
    TextureClear,
}

impl Check {
    pub const ALL: [Check; 3] = [Check::Sharpness, Check::ScaleIntact, Check::TextureClear];
}

/// 单项结果。
#[derive(Clone, Copy)]
pub struct CheckResult {
    pub check: Check,
    pub passed: bool,
}

// ── 伪 4K 检测 ──────────────────────────────────────────────────────

/// 资产描述。
#[derive(Clone, Copy)]
pub struct Asset {
    pub width: u32,
    pub height: u32,
    /// 源分辨率宽（上采样前）。
    pub source_width: u32,
    /// 上采样痕迹分（0..=100，越高越可疑）。
    pub upscale_score: u8,
}

impl Asset {
    /// 是否真 4K 宽。
    pub fn is_4k_width(&self) -> bool {
        self.width >= 3840
    }

    /// 伪 4K：标称 4K 但源不足或痕迹分高。
    pub fn fake_4k(&self) -> bool {
        self.is_4k_width() && (self.source_width < 3840 || self.upscale_score > 30)
    }
}

// ── 验收条目 ────────────────────────────────────────────────────────

/// 验收状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// 过 4K 门。
    Pass,
    /// 降级（须显性标记）。
    Degraded,
    /// 拒收。
    Reject,
}

/// 验收记录。
#[derive(Clone, Copy)]
pub struct Acceptance {
    pub asset_id: u32,
    pub grade: Grade,
    /// 降级标记是否显性（上架前置）。
    pub degrade_marked: bool,
    pub fake: bool,
}

// ── 验收器 ──────────────────────────────────────────────────────────

/// 4K 精度创作验收器。
pub struct AcceptanceGate {
    results: Vec<CheckResult>,
    records: Vec<Acceptance>,
    /// 人工目检位（双验）。
    manual_ok: bool,
    /// 自动检测位（双验）。
    auto_ok: bool,
    silent_listings: u32,
    last_code: Q49Code,
    has_error: bool,
}

impl AcceptanceGate {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            records: Vec::new(),
            manual_ok: false,
            auto_ok: false,
            silent_listings: 0,
            last_code: Q49Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> Q49Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: Q49Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 记录单项检查。
    pub fn record_check(&mut self, check: Check, passed: bool) -> usize {
        self.results.push(CheckResult { check, passed });
        self.results.len()
    }

    /// 三项是否齐。
    pub fn checks_complete(&self) -> bool {
        Check::ALL
            .iter()
            .all(|c| self.results.iter().any(|r| r.check == *c))
    }

    /// 全项通过。
    pub fn all_passed(&self) -> bool {
        self.checks_complete() && self.results.iter().all(|r| r.passed)
    }

    /// 伪 4K 检测。
    pub fn detect_fake(&mut self, asset: &Asset) -> Result<bool, Q49Code> {
        if asset.fake_4k() {
            self.fail(Q49Code::FAKE_4K);
            return Err(Q49Code::FAKE_4K);
        }
        self.last_code = Q49Code(0);
        self.has_error = false;
        Ok(false)
    }

    /// 双验登记（人工 + 自动）。
    pub fn dual_verify(&mut self, manual: bool, auto: bool) -> Result<bool, Q49Code> {
        self.manual_ok = manual;
        self.auto_ok = auto;
        if manual && auto {
            self.last_code = Q49Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(Q49Code::CHECK_INCOMPLETE);
            Err(Q49Code::CHECK_INCOMPLETE)
        }
    }

    /// 验收主入口：门缺项 / 伪 4K / 降级标记分别执法。
    pub fn accept(&mut self, asset_id: u32, asset: &Asset, degrade_marked: bool) -> Result<Grade, Q49Code> {
        if !self.checks_complete() {
            self.fail(Q49Code::CHECK_INCOMPLETE);
            return Err(Q49Code::CHECK_INCOMPLETE);
        }
        let fake = asset.fake_4k();
        let grade = if fake {
            Grade::Reject
        } else if self.all_passed() {
            Grade::Pass
        } else {
            Grade::Degraded
        };
        if grade == Grade::Degraded && !degrade_marked {
            self.silent_listings += 1;
            self.records.push(Acceptance { asset_id, grade, degrade_marked, fake });
            self.fail(Q49Code::DEGRADE_SILENT);
            return Err(Q49Code::DEGRADE_SILENT);
        }
        self.records.push(Acceptance { asset_id, grade, degrade_marked, fake });
        self.last_code = Q49Code(0);
        self.has_error = false;
        Ok(grade)
    }

    /// 上架审计：任一降级未标记即静默上架（P0）。
    pub fn audit_listing(&mut self) -> Result<u32, Q49Code> {
        let bad = self
            .records
            .iter()
            .filter(|r| r.grade == Grade::Degraded && !r.degrade_marked)
            .count();
        if bad > 0 {
            self.silent_listings += bad as u32;
            self.fail(Q49Code::SILENT_LISTING);
            return Err(Q49Code::SILENT_LISTING);
        }
        self.last_code = Q49Code(0);
        self.has_error = false;
        Ok(self.records.len() as u32)
    }

    pub fn silent_listings(&self) -> u32 {
        self.silent_listings
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 通过数。
    pub fn pass_count(&self) -> usize {
        self.records.iter().filter(|r| r.grade == Grade::Pass).count()
    }

    /// 降级数。
    pub fn degraded_count(&self) -> usize {
        self.records.iter().filter(|r| r.grade == Grade::Degraded).count()
    }

    /// 双验齐备。
    pub fn dual_green(&self) -> bool {
        self.manual_ok && self.auto_ok
    }

    /// 总则执法总评：无静默上架记录。
    pub fn enforcement_green(&self) -> bool {
        self.silent_listings == 0
    }

    /// 检查项数。
    pub fn check_count(&self) -> usize {
        self.results.len()
    }

    /// 重置检查轮（保留验收记录）。
    pub fn reset_checks(&mut self) {
        self.results.clear();
        self.last_code = Q49Code(0);
        self.has_error = false;
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves49_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves49_4kaccept");
    let asset_4k = Asset { width: 3840, height: 2160, source_width: 3840, upscale_score: 5 };
    let asset_fake = Asset { width: 3840, height: 2160, source_width: 1920, upscale_score: 60 };
    {
        let g = AcceptanceGate::new();
        chk(&mut set, "C3649-01", g.record_count() == 0 && g.check_count() == 0, "err");
    }
    {
        let mut g = AcceptanceGate::new();
        let r = g.detect_fake(&asset_4k);
        chk(&mut set, "C3649-02", r == Ok(false), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        let r = g.detect_fake(&asset_fake);
        chk(&mut set, "C3649-03", r.err() == Some(Q49Code::FAKE_4K), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        g.record_check(Check::Sharpness, true);
        g.record_check(Check::ScaleIntact, true);
        g.record_check(Check::TextureClear, true);
        chk(&mut set, "C3649-04", g.checks_complete() && g.all_passed(), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        g.record_check(Check::Sharpness, true);
        let r = g.accept(1, &asset_4k, true);
        chk(&mut set, "C3649-05", r.err() == Some(Q49Code::CHECK_INCOMPLETE), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        for c in Check::ALL.iter() {
            g.record_check(*c, true);
        }
        let r = g.accept(1, &asset_4k, false);
        chk(&mut set, "C3649-06", r == Ok(Grade::Pass), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        g.record_check(Check::Sharpness, false);
        g.record_check(Check::ScaleIntact, true);
        g.record_check(Check::TextureClear, true);
        let r = g.accept(1, &asset_4k, false);
        chk(&mut set, "C3649-07", r.err() == Some(Q49Code::DEGRADE_SILENT), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        g.record_check(Check::Sharpness, false);
        g.record_check(Check::ScaleIntact, true);
        g.record_check(Check::TextureClear, true);
        let r = g.accept(1, &asset_4k, true);
        chk(&mut set, "C3649-08", r == Ok(Grade::Degraded) && g.degraded_count() == 1, "err");
    }
    {
        let mut g = AcceptanceGate::new();
        for c in Check::ALL.iter() {
            g.record_check(*c, true);
        }
        let r = g.accept(2, &asset_fake, true);
        chk(&mut set, "C3649-09", r == Ok(Grade::Reject), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        let r = g.dual_verify(true, true);
        chk(&mut set, "C3649-10", r == Ok(true) && g.dual_green(), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        let r = g.dual_verify(true, false);
        chk(&mut set, "C3649-11", r.err() == Some(Q49Code::CHECK_INCOMPLETE), "err");
    }
    {
        let mut g = AcceptanceGate::new();
        g.record_check(Check::Sharpness, false);
        g.record_check(Check::ScaleIntact, true);
        g.record_check(Check::TextureClear, true);
        g.accept(1, &asset_4k, true).ok();
        let r = g.audit_listing();
        chk(&mut set, "C3649-12", r == Ok(1) && g.enforcement_green(), "err");
    }
    set
}
