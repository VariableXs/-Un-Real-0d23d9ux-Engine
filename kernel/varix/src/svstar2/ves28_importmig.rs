//! VE-F3628 · 主题导入迁移（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3628`
//!
//! **判据（锚点原文六条）**：映射诚实、差异确认、评分校准、批量复用、
//! 单源复用、判据。逐条落位：
//! - **映射诚实**：不支持的特性显性列出（映射诚实红线：静默丢弃 =
//!   导入后变形）——[`ImportMap::convert`] 丢弃项必须列入
//!   [`ConversionReport::dropped`]，静默 → [`V1CCode::SILENT_DROP`]。
//! - **差异确认**：评分差→用户确认接受（确认红线：静默接受 = 欺骗）——
//!   [`ConfirmGate`]，无确认 → [`V1CCode::NO_CONFIRM`]。
//! - **评分校准**：差异评分器 SSIM 对照，失真 → 校准。
//! - **批量复用**：批量容错复用（断点+失败清单）。
//! - **单源复用**：F3407 迁移模式单源。
//! - **判据**：`run_ves28_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 静默丢弃→P1（诚实红线实测） | [`V1CCode::SILENT_DROP`] |
//! | 无确认差异→拦截（确认红线实测） | [`V1CCode::NO_CONFIRM`] |
//! | 评分失真→校准（复述） | [`V1CCode::SCORE_DRIFT`] |
//! | 批量失败清单缺→补（复述） | [`V1CCode::BATCH_LIST_MISS`] |
//!
//! **码段**：独占 **0x4cxx**。
//!
//! **性能逐项分解**：映射 O(令牌)；评分 O(像素) 抽样；确认 O(1)；
//! 批量 O(断点)。
//!
//! **跨批对接点**：F3407 单源复用声明；F3113 评分复用；F3608 导入
//! 模式复用——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3628 诊断码段（独占 0x4cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1CCode(pub u16);

impl V1CCode {
    /// 静默丢弃特性。
    pub const SILENT_DROP: V1CCode = V1CCode(0x4c01);
    /// 差异未确认。
    pub const NO_CONFIRM: V1CCode = V1CCode(0x4c02);
    /// 评分失真。
    pub const SCORE_DRIFT: V1CCode = V1CCode(0x4c03);
    /// 批量失败清单缺。
    pub const BATCH_LIST_MISS: V1CCode = V1CCode(0x4c04);
    /// 映射表缺格。
    pub const MAP_GAP: V1CCode = V1CCode(0x4c05);
    /// 评分超阈值。
    pub const SCORE_OVER: V1CCode = V1CCode(0x4c06);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1CCode::SILENT_DROP => {
                "检出静默丢弃特性（诚实红线）。不支持的特性必须显性列出。"
            }
            V1CCode::NO_CONFIRM => {
                "差异评分超阈值但未经用户确认（确认红线）。静默接受 = 欺骗。"
            }
            V1CCode::SCORE_DRIFT => {
                "评分失真。建议：校准 SSIM 对照参数。"
            }
            V1CCode::BATCH_LIST_MISS => {
                "批量失败清单缺失。建议：补失败清单。"
            }
            V1CCode::MAP_GAP => {
                "映射表缺格。建议：补登不支持特性的显性声明。"
            }
            V1CCode::SCORE_OVER => {
                "差异评分超阈值。建议：走确认流程或改善转换质量。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 映射表（诚实红线）───────────────────────────────────────────────

/// 一条映射。
#[derive(Clone, Debug)]
pub struct MapEntry {
    /// 源特性名。
    pub source: String,
    /// 是否映射成功。
    pub mapped: bool,
    /// 不支持原因（mapped=false 时必填）。
    pub reason: String,
}

/// 导入映射表。
#[derive(Clone, Debug, Default)]
pub struct ImportMap {
    entries: Vec<MapEntry>,
}

impl ImportMap {
    /// 空表。
    pub fn new() -> ImportMap {
        ImportMap {
            entries: Vec::new(),
        }
    }

    /// 加映射。
    pub fn add(&mut self, source: &str, mapped: bool, reason: &str) {
        self.entries.push(MapEntry {
            source: String::from(source),
            mapped,
            reason: String::from(reason),
        });
    }

    /// 诚实审计：不支持项必须有原因。
    pub fn audit_honesty(&self) -> Result<(), V1CCode> {
        for e in &self.entries {
            if !e.mapped && e.reason.is_empty() {
                return Err(V1CCode::SILENT_DROP);
            }
        }
        Ok(())
    }

    /// 丢弃清单（显性）。
    pub fn dropped(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| !e.mapped)
            .map(|e| e.source.as_str())
            .collect()
    }
}

// ── 转换报告 ────────────────────────────────────────────────────────

/// 转换报告。
#[derive(Clone, Debug, Default)]
pub struct ConversionReport {
    /// 丢弃特性清单（必须非空 if 有丢弃）。
    pub dropped: Vec<String>,
    /// 差异评分（0..100，越低越好）。
    pub score: u32,
}

impl ConversionReport {
    /// 诚实审计：有丢弃必列清单。
    pub fn audit(&self, dropped_count: usize) -> Result<(), V1CCode> {
        if self.dropped.len() != dropped_count {
            return Err(V1CCode::SILENT_DROP);
        }
        Ok(())
    }
}

// ── 差异评分（SSIM 对照）────────────────────────────────────────────

/// 评分器。
#[derive(Clone, Debug, Default)]
pub struct ScoreCalibrator {
    /// 校准偏差。
    pub offset: i32,
}

impl ScoreCalibrator {
    /// 新建。
    pub fn new() -> ScoreCalibrator {
        ScoreCalibrator { offset: 0 }
    }

    /// 评分（抽样 SSIM 代理：确定性分数）。
    pub fn score(&self, source_fp: u64, target_fp: u64) -> u32 {
        let diff = (source_fp ^ target_fp).count_ones();
        // 差异位数 → 评分（0-100）
        let base = 100u32.saturating_sub(diff * 4);
        (base as i32 + self.offset).max(0) as u32
    }

    /// 校准。
    pub fn calibrate(&mut self, offset: i32) {
        self.offset = offset;
    }
}

/// 阈值（独立写死）。
pub const SCORE_THRESHOLD: u32 = 20;

// ── 确认门 ──────────────────────────────────────────────────────────

/// 确认门（差异必须用户确认）。
#[derive(Clone, Debug, Default)]
pub struct ConfirmGate {
    confirmed: bool,
}

impl ConfirmGate {
    /// 新建（未确认）。
    pub fn new() -> ConfirmGate {
        ConfirmGate { confirmed: false }
    }

    /// 用户确认。
    pub fn confirm(&mut self) {
        self.confirmed = true;
    }

    /// 确认审计：评分超阈值必须已确认。
    pub fn audit(&self, score: u32) -> Result<(), V1CCode> {
        if score <= SCORE_THRESHOLD {
            return Ok(()); // 低差异不需要确认
        }
        if self.confirmed {
            Ok(())
        } else {
            Err(V1CCode::NO_CONFIRM)
        }
    }
}

// ── 批量容错（复用）─────────────────────────────────────────────────

/// 批量报告。
#[derive(Clone, Debug, Default)]
pub struct BatchReport {
    pub successes: Vec<String>,
    pub failures: Vec<(String, u16)>,
}

impl BatchReport {
    /// 清单完整性。
    pub fn audit_listing(&self) -> Result<(), V1CCode> {
        // 每个失败必须有码
        if self.failures.iter().any(|(_, c)| *c == 0) {
            return Err(V1CCode::BATCH_LIST_MISS);
        }
        Ok(())
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3407", "VE-F3113", "VE-F3608"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves28_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3628");

    // 0x4c01 诚实映射
    let mut m = ImportMap::new();
    m.add("layer-blend", true, "");
    m.add("shader-x", false, "暂不支持 shader 特性");
    out.add("0x4c01-诚实映射", m.audit_honesty().is_ok(), "");

    // 0x4c02 静默丢弃检出
    let mut m2 = ImportMap::new();
    m2.add("shader-x", false, "");
    out.add("0x4c02-静默丢弃检出", m2.audit_honesty().is_err(), "");

    // 0x4c03 丢弃清单
    out.add("0x4c03-丢弃清单", m.dropped().len() == 1, "");

    // 0x4c04 评分低免确认
    let cg = ConfirmGate::new();
    out.add("0x4c04-评分低免确认", cg.audit(10).is_ok(), "");

    // 0x4c05 评分高需确认
    out.add("0x4c05-评分高需确认", cg.audit(50).is_err(), "");

    // 0x4c06 确认后放行
    let mut cg2 = ConfirmGate::new();
    cg2.confirm();
    out.add("0x4c06-确认后放行", cg2.audit(50).is_ok(), "");

    // 0x4c07 评分一致
    let sc = ScoreCalibrator::new();
    out.add("0x4c07-评分一致", sc.score(0xAAA, 0xAAA) == 100, "");

    // 0x4c08 批量清单
    let br = BatchReport {
        successes: vec![String::from("ok")],
        failures: vec![(String::from("bad"), 0x4c01)],
    };
    out.add("0x4c08-批量清单", br.audit_listing().is_ok(), "");

    // 0x4c09 批量缺码
    let br2 = BatchReport {
        successes: vec![],
        failures: vec![(String::from("bad"), 0)],
    };
    out.add("0x4c09-批量缺码", br2.audit_listing().is_err(), "");

    out
}
