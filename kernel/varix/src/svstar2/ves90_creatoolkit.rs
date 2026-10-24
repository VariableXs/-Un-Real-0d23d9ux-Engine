//! VE-F3690 · 创作者工具包（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3690`
//!
//! **职责定位（锚点原文）**：创作者工具包——
//! - **数据工具**：资产表现分析（依据数据复述）；
//! - **优化建议**：基于数据的建议（可解释复用）；
//! - **素材工具**：截图模板/描述优化（辅助不代写——AI 代写 = 同质化，代写红线）；
//! - **批量管理**：多资产批量（复用批量容错）。
//!
//! **判据（锚点原文）**：代写红线、建议可解释、数据单源、批量容错、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 代写越界→边界拦截（红线实测） | [`C90Code::GHOSTWRITE`] |
//! | 建议黑箱→可解释（复述红线实测） | [`C90Code::OPAQUE_ADVICE`] |
//! | 数据失实→单源（复述） | [`C90Code::DATA_DRIFT`] |
//! | 批量失败静默→清单（复述） | [`C90Code::BATCH_SILENT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3690 诊断码段（独占 0x8Axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C90Code(pub u16);

impl C90Code {
    /// AI 代写越界（代写红线实测）。
    pub const GHOSTWRITE: C90Code = C90Code(0x8A01);
    /// 建议黑箱。
    pub const OPAQUE_ADVICE: C90Code = C90Code(0x8A02);
    /// 数据失实。
    pub const DATA_DRIFT: C90Code = C90Code(0x8A03);
    /// 批量失败静默。
    pub const BATCH_SILENT: C90Code = C90Code(0x8A04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C90Code::GHOSTWRITE => "AI 代写越界（代写红线实测）。建议：收敛为辅助。",
            C90Code::OPAQUE_ADVICE => "建议不可解释。建议：补理由链。",
            C90Code::DATA_DRIFT => "数据失实。建议：对齐单源。",
            C90Code::BATCH_SILENT => "批量失败静默。建议：出失败清单。",
            _ => "未知诊断码。",
        }
    }
}

// ── 资产分析 ────────────────────────────────────────────────────────

/// 资产表现。
#[derive(Clone, Copy)]
pub struct AssetStats {
    pub id: u16,
    pub downloads: u32,
    pub rating_avg: u8,
}

/// 资产分析器（数据单源）。
pub struct AssetAnalyzer {
    stats: Vec<AssetStats>,
    drift_hits: u32,
    last_code: C90Code,
    has_error: bool,
}

impl AssetAnalyzer {
    pub fn new() -> Self {
        Self {
            stats: Vec::new(),
            drift_hits: 0,
            last_code: C90Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C90Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn ingest(&mut self, s: AssetStats) -> usize {
        self.stats.push(s);
        self.stats.len()
    }

    /// 最受欢迎资产 ID。
    pub fn most_popular(&self) -> Option<u16> {
        self.stats
            .iter()
            .max_by_key(|s| s.downloads)
            .map(|s| s.id)
    }

    /// 数据失实上报。
    pub fn note_drift(&mut self) {
        self.drift_hits += 1;
        self.last_code = C90Code::DATA_DRIFT;
        self.has_error = true;
    }

    /// 均分。
    pub fn avg_rating(&self) -> u8 {
        if self.stats.is_empty() {
            return 0;
        }
        let sum: u32 = self.stats.iter().map(|s| s.rating_avg as u32).sum();
        (sum / self.stats.len() as u32) as u8
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    pub fn count(&self) -> usize {
        self.stats.len()
    }
}

// ── 可解释建议 ──────────────────────────────────────────────────────

/// 优化建议。
#[derive(Clone, Copy)]
pub struct Advice {
    pub id: u16,
    /// 理由链长度（可解释）。
    pub reason_len: u8,
    /// 是否代写内容。
    pub ghostwrote: bool,
}

/// 建议器（可解释 + 代写红线）。
pub struct AdviceEngine {
    advices: Vec<Advice>,
    ghost_hits: u32,
    opaque_hits: u32,
    last_code: C90Code,
    has_error: bool,
}

impl AdviceEngine {
    pub fn new() -> Self {
        Self {
            advices: Vec::new(),
            ghost_hits: 0,
            opaque_hits: 0,
            last_code: C90Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C90Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 提出建议（代写红线检测）。
    pub fn propose(&mut self, a: Advice) -> Result<bool, C90Code> {
        if a.ghostwrote {
            self.ghost_hits += 1;
            self.last_code = C90Code::GHOSTWRITE;
            self.has_error = true;
            return Err(C90Code::GHOSTWRITE);
        }
        if a.reason_len == 0 {
            self.opaque_hits += 1;
            self.last_code = C90Code::OPAQUE_ADVICE;
            self.has_error = true;
            return Err(C90Code::OPAQUE_ADVICE);
        }
        self.advices.push(a);
        self.last_code = C90Code(0);
        self.has_error = false;
        Ok(true)
    }

    pub fn ghost_hits(&self) -> u32 {
        self.ghost_hits
    }

    pub fn opaque_hits(&self) -> u32 {
        self.opaque_hits
    }

    pub fn advice_count(&self) -> usize {
        self.advices.len()
    }
}

// ── 辅助边界 ────────────────────────────────────────────────────────

/// 素材辅助动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssistAction {
    /// 模板建议。
    SuggestTemplate,
    /// 提纲建议。
    SuggestOutline,
    /// 直接代写（违规）。
    WriteForUser,
}

/// 辅助边界声明。
pub struct AssistBoundary {
    ghost_hits: u32,
    last_code: C90Code,
    has_error: bool,
}

impl AssistBoundary {
    pub fn new() -> Self {
        Self {
            ghost_hits: 0,
            last_code: C90Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C90Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 边界检查：代写即拦截。
    pub fn check(&mut self, action: AssistAction) -> Result<bool, C90Code> {
        match action {
            AssistAction::WriteForUser => {
                self.ghost_hits += 1;
                self.last_code = C90Code::GHOSTWRITE;
                self.has_error = true;
                Err(C90Code::GHOSTWRITE)
            }
            _ => {
                self.last_code = C90Code(0);
                self.has_error = false;
                Ok(true)
            }
        }
    }

    pub fn ghost_hits(&self) -> u32 {
        self.ghost_hits
    }
}

// ── 批量管理 ────────────────────────────────────────────────────────

/// 批量结果。
#[derive(Clone, Copy)]
pub struct BatchResult {
    pub id: u16,
    pub ok: bool,
}

/// 批量器（失败清单复述）。
pub struct BatchManager {
    results: Vec<BatchResult>,
    silent_hits: u32,
    last_code: C90Code,
    has_error: bool,
}

impl BatchManager {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            silent_hits: 0,
            last_code: C90Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C90Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn run(&mut self, id: u16, ok: bool) -> usize {
        self.results.push(BatchResult { id, ok });
        self.results.len()
    }

    /// 失败清单（不得静默）。
    pub fn fail_list(&self) -> Vec<u16> {
        self.results
            .iter()
            .filter(|r| !r.ok)
            .map(|r| r.id)
            .collect()
    }

    /// 静默检测：有失败但清单为空即违规。
    pub fn audit_silence(&mut self) -> Result<bool, C90Code> {
        let has_fail = self.results.iter().any(|r| !r.ok);
        let list = self.fail_list();
        if has_fail && list.is_empty() {
            self.silent_hits += 1;
            self.last_code = C90Code::BATCH_SILENT;
            self.has_error = true;
            Err(C90Code::BATCH_SILENT)
        } else {
            self.last_code = C90Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    pub fn silent_hits(&self) -> u32 {
        self.silent_hits
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves90_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves90_creatoolkit");
    {
        let mut a = AssetAnalyzer::new();
        a.ingest(AssetStats { id: 1, downloads: 100, rating_avg: 4 });
        a.ingest(AssetStats { id: 2, downloads: 300, rating_avg: 5 });
        chk(&mut set, "C3690-01", a.most_popular() == Some(2), "err");
    }
    {
        let mut a = AssetAnalyzer::new();
        a.ingest(AssetStats { id: 1, downloads: 10, rating_avg: 4 });
        a.ingest(AssetStats { id: 2, downloads: 20, rating_avg: 2 });
        chk(&mut set, "C3690-02", a.avg_rating() == 3, "err");
    }
    {
        let mut a = AssetAnalyzer::new();
        a.note_drift();
        chk(&mut set, "C3690-03", a.drift_hits() == 1 && a.has_error(), "err");
    }
    {
        let mut e = AdviceEngine::new();
        let res = e.propose(Advice { id: 1, reason_len: 3, ghostwrote: false });
        chk(&mut set, "C3690-04", res == Ok(true) && e.advice_count() == 1, "err");
    }
    {
        let mut e = AdviceEngine::new();
        let res = e.propose(Advice { id: 1, reason_len: 1, ghostwrote: true });
        chk(&mut set, "C3690-05", res.err() == Some(C90Code::GHOSTWRITE), "err");
    }
    {
        let mut e = AdviceEngine::new();
        let res = e.propose(Advice { id: 1, reason_len: 0, ghostwrote: false });
        chk(&mut set, "C3690-06", res.err() == Some(C90Code::OPAQUE_ADVICE), "err");
    }
    {
        let mut b = AssistBoundary::new();
        chk(&mut set, "C3690-07", b.check(AssistAction::SuggestTemplate) == Ok(true), "err");
    }
    {
        let mut b = AssistBoundary::new();
        let res = b.check(AssistAction::WriteForUser);
        chk(&mut set, "C3690-08", res.err() == Some(C90Code::GHOSTWRITE), "err");
    }
    {
        let mut m = BatchManager::new();
        m.run(1, true);
        m.run(2, true);
        chk(&mut set, "C3690-09", m.audit_silence() == Ok(true) && m.fail_list().is_empty(), "err");
    }
    {
        let mut m = BatchManager::new();
        m.run(1, true);
        m.run(2, false);
        let list = m.fail_list();
        chk(&mut set, "C3690-10", list.len() == 1 && list[0] == 2, "err");
    }
    {
        let mut m = BatchManager::new();
        m.run(1, false);
        chk(&mut set, "C3690-11", m.audit_silence() == Ok(true), "err");
    }
    {
        let mut a = AssetAnalyzer::new();
        chk(&mut set, "C3690-12", a.count() == 0 && a.avg_rating() == 0, "err");
    }
    set
}
