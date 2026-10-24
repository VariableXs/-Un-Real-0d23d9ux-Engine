//! VE-F3648 · 视觉资产导出（VE-S 域 · 批次 R03 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3648`
//!
//! **职责定位（锚点原文）**：创作→可用资产——
//! - **导出流水**：格式选择（复用 F3282）→ 优化（复用 F3287）→ 打包（复用 Q04）
//!   → 验证（全链复用红线：导出流水私造 = 四处优化逻辑 → 私造红线）；
//! - **多目标导出**：一次创作多目标（壁纸/商店预览/市场缩略）（模板化导出）；
//! - **导出预算**：复用 F3610 导出 5s。
//!
//! **判据（锚点原文六条）**：全链复用、多目标模板、导出预算、
//! 私造红线、验证复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 私造流水→P1（复用红线实测） | [`E48Code::PRIVATE_PIPELINE`] |
//! | 目标缺→模板补（复述） | [`E48Code::TARGET_GAP`] |
//! | 导出超时→分块显性（复述） | [`E48Code::BUDGET_EXCEEDED`] |
//! | 产物失真→验证拦截（复用） | [`E48Code::OUTPUT_DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3648 诊断码段（独占 0x60xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct E48Code(pub u16);

impl E48Code {
    /// 私造导出流水（复用红线 P1）。
    pub const PRIVATE_PIPELINE: E48Code = E48Code(0x6001);
    /// 多目标模板缺档。
    pub const TARGET_GAP: E48Code = E48Code(0x6002);
    /// 导出超预算（5s 线）。
    pub const BUDGET_EXCEEDED: E48Code = E48Code(0x6003);
    /// 产物失真（验证拦截）。
    pub const OUTPUT_DRIFT: E48Code = E48Code(0x6004);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            E48Code::PRIVATE_PIPELINE => "导出流水私造（复用红线 P1）。建议：走 F3282/F3287/Q04 单源链。",
            E48Code::TARGET_GAP => "多目标模板缺档。建议：补齐壁纸/商店预览/市场缩略。",
            E48Code::BUDGET_EXCEEDED => "导出超 5s 预算。建议：分块显性导出。",
            E48Code::OUTPUT_DRIFT => "产物失真。建议：验证段拦截重导。",
            _ => "未知诊断码。",
        }
    }
}

// ── 流水段（全链复用声明） ──────────────────────────────────────────

/// 流水段（四段单源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 格式选择（F3282 单源）。
    Format,
    /// 优化（F3287 单源）。
    Optimize,
    /// 打包（Q04 单源）。
    Pack,
    /// 验证（复用验证段）。
    Verify,
}

impl Stage {
    pub const CHAIN: [Stage; 4] = [
        Stage::Format,
        Stage::Optimize,
        Stage::Pack,
        Stage::Verify,
    ];

    /// 单源 id（私造审计用）。
    pub fn source_id(self) -> u32 {
        match self {
            Stage::Format => 3282,
            Stage::Optimize => 3287,
            Stage::Pack => 4004,
            Stage::Verify => 3288,
        }
    }
}

// ── 多目标模板 ──────────────────────────────────────────────────────

/// 导出目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// 壁纸包。
    Wallpaper,
    /// 商店预览。
    StorePreview,
    /// 市场缩略。
    MarketThumb,
}

impl Target {
    pub const ALL: [Target; 3] = [
        Target::Wallpaper,
        Target::StorePreview,
        Target::MarketThumb,
    ];
}

/// 目标模板。
#[derive(Clone, Copy)]
pub struct TargetTemplate {
    pub target: Target,
    /// 最长边像素。
    pub max_edge: u32,
    /// 是否已配。
    pub configured: bool,
}

// ── 导出记录 ────────────────────────────────────────────────────────

/// 单次导出。
#[derive(Clone, Copy)]
pub struct ExportRun {
    pub target: Target,
    pub elapsed_ms: u32,
    /// 产物指纹（验证对拍）。
    pub fingerprint: u32,
    /// 预期指纹。
    pub expect_fp: u32,
}

impl ExportRun {
    /// 预算窗（F3610：导出 5s）。
    pub const BUDGET_MS: u32 = 5_000;

    pub fn in_budget(&self) -> bool {
        self.elapsed_ms <= Self::BUDGET_MS
    }

    pub fn verified(&self) -> bool {
        self.fingerprint == self.expect_fp
    }
}

// ── 导出器 ──────────────────────────────────────────────────────────

/// 视觉资产导出器。
pub struct AssetExporter {
    templates: Vec<TargetTemplate>,
    runs: Vec<ExportRun>,
    /// 流水是否走单源链（false = 私造）。
    single_source: bool,
    private_hits: u32,
    last_code: E48Code,
    has_error: bool,
}

impl AssetExporter {
    /// 新建；`single_source` 必须为 true（全链复用红线）。
    pub fn new(single_source: bool) -> Self {
        Self {
            templates: Vec::new(),
            runs: Vec::new(),
            single_source,
            private_hits: 0,
            last_code: E48Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> E48Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: E48Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 配目标模板。
    pub fn configure(&mut self, target: Target, max_edge: u32) -> Result<usize, E48Code> {
        if !self.single_source {
            self.private_hits += 1;
            self.fail(E48Code::PRIVATE_PIPELINE);
            return Err(E48Code::PRIVATE_PIPELINE);
        }
        if let Some(t) = self.templates.iter_mut().find(|t| t.target == target) {
            t.max_edge = max_edge;
            t.configured = true;
        } else {
            self.templates.push(TargetTemplate { target, max_edge, configured: true });
        }
        self.last_code = E48Code(0);
        self.has_error = false;
        Ok(self.templates.len())
    }

    /// 多目标齐配审计。
    pub fn targets_complete(&self) -> bool {
        Target::ALL
            .iter()
            .all(|t| self.templates.iter().any(|x| x.target == *t && x.configured))
    }

    /// 缺档目标。
    pub fn missing_targets(&self) -> Vec<Target> {
        Target::ALL
            .iter()
            .copied()
            .filter(|t| !self.templates.iter().any(|x| x.target == *t && x.configured))
            .collect()
    }

    /// 走一遍全链（四段必须全部为单源段）。
    pub fn run_chain(&mut self, target: Target, elapsed_ms: u32, fingerprint: u32, expect_fp: u32) -> Result<ExportRun, E48Code> {
        if !self.single_source {
            self.private_hits += 1;
            self.fail(E48Code::PRIVATE_PIPELINE);
            return Err(E48Code::PRIVATE_PIPELINE);
        }
        if Stage::CHAIN.iter().any(|s| s.source_id() == 0) {
            self.fail(E48Code::PRIVATE_PIPELINE);
            return Err(E48Code::PRIVATE_PIPELINE);
        }
        if !self.templates.iter().any(|t| t.target == target && t.configured) {
            self.fail(E48Code::TARGET_GAP);
            return Err(E48Code::TARGET_GAP);
        }
        let run = ExportRun { target, elapsed_ms, fingerprint, expect_fp };
        if !run.in_budget() {
            self.runs.push(run);
            self.fail(E48Code::BUDGET_EXCEEDED);
            return Err(E48Code::BUDGET_EXCEEDED);
        }
        if !run.verified() {
            self.runs.push(run);
            self.fail(E48Code::OUTPUT_DRIFT);
            return Err(E48Code::OUTPUT_DRIFT);
        }
        self.runs.push(run);
        self.last_code = E48Code(0);
        self.has_error = false;
        Ok(run)
    }

    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub fn private_hits(&self) -> u32 {
        self.private_hits
    }

    /// 预算内成功数。
    pub fn in_budget_count(&self) -> usize {
        self.runs.iter().filter(|r| r.in_budget()).count()
    }

    /// 验证通过数。
    pub fn verified_count(&self) -> usize {
        self.runs.iter().filter(|r| r.verified()).count()
    }

    /// 全链复用总评：单源 + 四段 id 非零。
    pub fn chain_reuse_green(&self) -> bool {
        self.single_source && Stage::CHAIN.iter().all(|s| s.source_id() != 0)
    }

    /// 五秒预算常量出口。
    pub fn budget_ms(&self) -> u32 {
        ExportRun::BUDGET_MS
    }

    /// 模板数。
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// 分块显性导出（超时处置路径）：记录一条预算外但显性标注的运行。
    pub fn export_chunked(&mut self, target: Target, elapsed_ms: u32) -> Result<u32, E48Code> {
        if !self.single_source {
            self.fail(E48Code::PRIVATE_PIPELINE);
            return Err(E48Code::PRIVATE_PIPELINE);
        }
        let run = ExportRun { target, elapsed_ms, fingerprint: 0, expect_fp: 1 };
        self.runs.push(run);
        self.fail(E48Code::BUDGET_EXCEEDED);
        Ok(elapsed_ms)
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves48_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves48_assetexport");
    {
        let e = AssetExporter::new(true);
        chk(&mut set, "C3648-01", e.run_count() == 0 && e.budget_ms() == 5_000, "err");
    }
    {
        let mut e = AssetExporter::new(true);
        let r = e.configure(Target::Wallpaper, 3840);
        chk(&mut set, "C3648-02", r == Ok(1), "err");
    }
    {
        let mut e = AssetExporter::new(false);
        let r = e.configure(Target::Wallpaper, 3840);
        chk(&mut set, "C3648-03", r.err() == Some(E48Code::PRIVATE_PIPELINE) && e.private_hits() == 1, "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        e.configure(Target::StorePreview, 1024);
        e.configure(Target::MarketThumb, 512);
        chk(&mut set, "C3648-04", e.targets_complete(), "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        chk(&mut set, "C3648-05", e.missing_targets().len() == 2, "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        let r = e.run_chain(Target::Wallpaper, 1_000, 0xAA, 0xAA);
        chk(&mut set, "C3648-06", r.is_ok() && e.verified_count() == 1, "err");
    }
    {
        let mut e = AssetExporter::new(true);
        let r = e.run_chain(Target::Wallpaper, 1_000, 0xAA, 0xAA);
        chk(&mut set, "C3648-07", r.err() == Some(E48Code::TARGET_GAP), "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        let r = e.run_chain(Target::Wallpaper, 9_000, 0xAA, 0xAA);
        chk(&mut set, "C3648-08", r.err() == Some(E48Code::BUDGET_EXCEEDED), "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        let r = e.run_chain(Target::Wallpaper, 1_000, 0xAA, 0xBB);
        chk(&mut set, "C3648-09", r.err() == Some(E48Code::OUTPUT_DRIFT), "err");
    }
    {
        let e = AssetExporter::new(true);
        chk(&mut set, "C3648-10", e.chain_reuse_green(), "err");
    }
    {
        let e = AssetExporter::new(false);
        chk(&mut set, "C3648-11", !e.chain_reuse_green(), "err");
    }
    {
        let mut e = AssetExporter::new(true);
        e.configure(Target::Wallpaper, 3840);
        let r = e.export_chunked(Target::Wallpaper, 8_000);
        chk(&mut set, "C3648-12", r == Ok(8_000) && e.run_count() == 1, "err");
    }
    set
}
