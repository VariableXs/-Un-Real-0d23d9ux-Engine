//! VE-F3685 · 质量认证体系（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3685`
//!
//! **职责定位（锚点原文）**：质量认证体系——
//! - **认证分级**：bronze / silver / gold 三级（标准公开——复述透明）；
//! - **认证流程**：自动扫描 + 人工复核双段（复用双验证）；
//! - **认证权威**：认证 ≠ 担保（免责边界声明 + 用户知情断言）；
//! - **认证撤销**：后续劣化 → 撤认证（时效复述）。
//!
//! **判据（锚点原文）**：三级标准、双段流程、边界声明、撤销协议、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 边界不清→P1（红线实测——知情文案断言） | [`C85Code::BOUNDARY_BLUR`] |
//! | 标准暗改→ADR（复述） | [`C85Code::STD_TAMPER`] |
//! | 双段缺→补（复用红线） | [`C85Code::STAGE_GAP`] |
//! | 撤销滞后→时效（复述） | [`C85Code::REVOKE_DELAY`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3685 诊断码段（独占 0x85xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C85Code(pub u16);

impl C85Code {
    /// 边界不清（P1 红线实测）。
    pub const BOUNDARY_BLUR: C85Code = C85Code(0x8501);
    /// 标准暗改。
    pub const STD_TAMPER: C85Code = C85Code(0x8502);
    /// 双段缺。
    pub const STAGE_GAP: C85Code = C85Code(0x8503);
    /// 撤销滞后。
    pub const REVOKE_DELAY: C85Code = C85Code(0x8504);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C85Code::BOUNDARY_BLUR => "认证边界不清（P1 红线实测）。建议：补知情文案。",
            C85Code::STD_TAMPER => "认证标准暗改。建议：ADR 留痕。",
            C85Code::STAGE_GAP => "双段流程缺段。建议：补扫描或复核。",
            C85Code::REVOKE_DELAY => "撤销滞后。建议：按时效撤证。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三级认证 ────────────────────────────────────────────────────────

/// 认证等级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CertLevel {
    /// 铜。
    Bronze = 0,
    /// 银。
    Silver = 1,
    /// 金。
    Gold = 2,
}

impl CertLevel {
    pub const ALL: [CertLevel; 3] = [CertLevel::Bronze, CertLevel::Silver, CertLevel::Gold];
}

/// 认证标准（公开）。
pub struct CertStandard {
    /// (等级, 最低分)。
    thresholds: Vec<(CertLevel, u16)>,
    tamper_hits: u32,
    last_code: C85Code,
    has_error: bool,
}

impl CertStandard {
    /// 默认标准：铜 60 / 银 80 / 金 95。
    pub fn standard() -> Self {
        Self {
            thresholds: vec![
                (CertLevel::Bronze, 60),
                (CertLevel::Silver, 80),
                (CertLevel::Gold, 95),
            ],
            tamper_hits: 0,
            last_code: C85Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C85Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 查阈值。
    pub fn threshold(&self, level: CertLevel) -> Option<u16> {
        self.thresholds
            .iter()
            .find(|(l, _)| *l == level)
            .map(|(_, t)| *t)
    }

    /// 评级：按分数给最高可达级。
    pub fn grade(&self, score: u16) -> Option<CertLevel> {
        CertLevel::ALL
            .iter()
            .rev()
            .find(|l| self.threshold(**l).map(|t| score >= t).unwrap_or(false))
            .copied()
    }

    /// 暗改检测（ADR）。
    pub fn note_tamper(&mut self) {
        self.tamper_hits += 1;
        self.last_code = C85Code::STD_TAMPER;
        self.has_error = true;
    }

    pub fn tamper_hits(&self) -> u32 {
        self.tamper_hits
    }
}

// ── 双段流程 ────────────────────────────────────────────────────────

/// 流程段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 自动扫描。
    AutoScan,
    /// 人工复核。
    ManualReview,
}

/// 认证流程（双段复用双验证）。
pub struct CertPipeline {
    scanned: bool,
    reviewed: bool,
    gap_hits: u32,
    last_code: C85Code,
    has_error: bool,
}

impl CertPipeline {
    pub fn new() -> Self {
        Self {
            scanned: false,
            reviewed: false,
            gap_hits: 0,
            last_code: C85Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C85Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 完成一段。
    pub fn complete_stage(&mut self, stage: Stage) {
        match stage {
            Stage::AutoScan => self.scanned = true,
            Stage::ManualReview => self.reviewed = true,
        }
    }

    /// 双段齐备断言。
    pub fn assert_both(&mut self) -> Result<bool, C85Code> {
        if self.scanned && self.reviewed {
            self.last_code = C85Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.gap_hits += 1;
            self.last_code = C85Code::STAGE_GAP;
            self.has_error = true;
            Err(C85Code::STAGE_GAP)
        }
    }

    pub fn gap_hits(&self) -> u32 {
        self.gap_hits
    }
}

// ── 边界声明 ────────────────────────────────────────────────────────

/// 免责边界（认证 ≠ 担保）。
pub struct BoundaryDecl {
    /// 是否已声明。
    declared: bool,
    /// 用户是否知情。
    user_informed: bool,
    blur_hits: u32,
    last_code: C85Code,
    has_error: bool,
}

impl BoundaryDecl {
    pub fn new() -> Self {
        Self {
            declared: false,
            user_informed: false,
            blur_hits: 0,
            last_code: C85Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C85Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn declare(&mut self) {
        self.declared = true;
    }

    pub fn inform(&mut self) {
        self.user_informed = true;
    }

    /// 知情断言：声明 + 知情双齐。
    pub fn assert_clear(&mut self) -> Result<bool, C85Code> {
        if self.declared && self.user_informed {
            self.last_code = C85Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.blur_hits += 1;
            self.last_code = C85Code::BOUNDARY_BLUR;
            self.has_error = true;
            Err(C85Code::BOUNDARY_BLUR)
        }
    }

    pub fn blur_hits(&self) -> u32 {
        self.blur_hits
    }
}

// ── 撤销协议 ────────────────────────────────────────────────────────

/// 认证记录。
#[derive(Clone, Copy)]
pub struct CertRecord {
    pub id: u16,
    pub level: CertLevel,
    pub score: u16,
    /// 是否已撤。
    pub revoked: bool,
}

/// 撤销协议（劣化 → 撤证）。
pub struct RevokeProtocol {
    records: Vec<CertRecord>,
    delay_hits: u32,
    last_code: C85Code,
    has_error: bool,
}

impl RevokeProtocol {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            delay_hits: 0,
            last_code: C85Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C85Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn issue(&mut self, r: CertRecord) -> usize {
        self.records.push(r);
        self.records.len()
    }

    /// 劣化撤证：分数跌破阈值必须撤。
    pub fn revoke_if_degraded(&mut self, id: u16, new_score: u16, threshold: u16) -> Result<bool, C85Code> {
        match self.records.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.score = new_score;
                if new_score < threshold {
                    if !r.revoked {
                        r.revoked = true;
                        self.delay_hits += 1;
                        self.last_code = C85Code::REVOKE_DELAY;
                        self.has_error = true;
                        return Err(C85Code::REVOKE_DELAY);
                    }
                }
                self.last_code = C85Code(0);
                self.has_error = false;
                Ok(true)
            }
            None => {
                self.last_code = C85Code::STAGE_GAP;
                self.has_error = true;
                Err(C85Code::STAGE_GAP)
            }
        }
    }

    pub fn revoked_count(&self) -> usize {
        self.records.iter().filter(|r| r.revoked).count()
    }

    pub fn delay_hits(&self) -> u32 {
        self.delay_hits
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves85_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves85_qualcert");
    {
        let s = CertStandard::standard();
        chk(&mut set, "C3685-01", s.threshold(CertLevel::Gold) == Some(95), "err");
    }
    {
        let s = CertStandard::standard();
        chk(&mut set, "C3685-02", s.grade(90) == Some(CertLevel::Silver), "err");
    }
    {
        let s = CertStandard::standard();
        chk(&mut set, "C3685-03", s.grade(100) == Some(CertLevel::Gold), "err");
    }
    {
        let mut s = CertStandard::standard();
        s.note_tamper();
        chk(&mut set, "C3685-04", s.tamper_hits() == 1 && s.has_error(), "err");
    }
    {
        let mut p = CertPipeline::new();
        p.complete_stage(Stage::AutoScan);
        p.complete_stage(Stage::ManualReview);
        chk(&mut set, "C3685-05", p.assert_both() == Ok(true), "err");
    }
    {
        let mut p = CertPipeline::new();
        p.complete_stage(Stage::AutoScan);
        let res = p.assert_both();
        chk(&mut set, "C3685-06", res.err() == Some(C85Code::STAGE_GAP), "err");
    }
    {
        let mut b = BoundaryDecl::new();
        b.declare();
        b.inform();
        chk(&mut set, "C3685-07", b.assert_clear() == Ok(true), "err");
    }
    {
        let mut b = BoundaryDecl::new();
        b.declare();
        let res = b.assert_clear();
        chk(&mut set, "C3685-08", res.err() == Some(C85Code::BOUNDARY_BLUR), "err");
    }
    {
        let mut r = RevokeProtocol::new();
        r.issue(CertRecord { id: 1, level: CertLevel::Silver, score: 85, revoked: false });
        let res = r.revoke_if_degraded(1, 50, 60);
        chk(&mut set, "C3685-09", res.err() == Some(C85Code::REVOKE_DELAY), "err");
    }
    {
        let mut r = RevokeProtocol::new();
        r.issue(CertRecord { id: 1, level: CertLevel::Silver, score: 85, revoked: false });
        let res = r.revoke_if_degraded(1, 90, 60);
        chk(&mut set, "C3685-10", res == Ok(true) && r.revoked_count() == 0, "err");
    }
    {
        let mut r = RevokeProtocol::new();
        let res = r.revoke_if_degraded(9, 10, 60);
        chk(&mut set, "C3685-11", res.err() == Some(C85Code::STAGE_GAP), "err");
    }
    {
        let s = CertStandard::standard();
        chk(&mut set, "C3685-12", s.grade(30).is_none(), "err");
    }
    set
}
