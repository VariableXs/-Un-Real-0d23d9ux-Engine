//! VE-F3687 · 评分评价体系（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3687`
//!
//! **职责定位（锚点原文）**：评分评价体系——
//! - **多维评分**：质量/易用/美观/兼容四维（单一分数 = 信息丢失——多维红线）；
//! - **评价真实性**：刷分识别（同源批量/时间聚集——复用反刷制度）；
//! - **评价展示**：评分分布（只给均分 = 隐藏两极——分布红线）；
//! - **评价治理**：恶意评价治理（复用仲裁）。
//!
//! **判据（锚点原文）**：多维断言、刷分检测 P0、分布断言、多维不倒退、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 刷分容忍→P0（复述红线实测） | [`C87Code::RIGGED_SCORE`] |
//! | 均值隐藏→P1（分布红线实测） | [`C87Code::MEAN_ONLY`] |
//! | 多维倒退→修正（红线实测） | [`C87Code::DIM_REGRESS`] |
//! | 误删评价→治理审计（复用） | [`C87Code::REVIEW_DROP`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3687 诊断码段（独占 0x87xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C87Code(pub u16);

impl C87Code {
    /// 刷分（P0 红线实测）。
    pub const RIGGED_SCORE: C87Code = C87Code(0x8701);
    /// 只给均值（P1 分布红线）。
    pub const MEAN_ONLY: C87Code = C87Code(0x8702);
    /// 多维倒退。
    pub const DIM_REGRESS: C87Code = C87Code(0x8703);
    /// 评价误删。
    pub const REVIEW_DROP: C87Code = C87Code(0x8704);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C87Code::RIGGED_SCORE => "刷分检出（P0 红线实测）。建议：清理并反刷。",
            C87Code::MEAN_ONLY => "只展示均值（P1 分布红线）。建议：补分布。",
            C87Code::DIM_REGRESS => "多维评分倒退。建议：恢复四维。",
            C87Code::REVIEW_DROP => "评价误删。建议：治理审计复原。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四维模型 ────────────────────────────────────────────────────────

/// 评分维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 质量。
    Quality,
    /// 易用。
    Usability,
    /// 美观。
    Aesthetics,
    /// 兼容。
    Compat,
}

impl Dim {
    pub const ALL: [Dim; 4] =
        [Dim::Quality, Dim::Usability, Dim::Aesthetics, Dim::Compat];
}

/// 多维评分。
pub struct MultiDimScore {
    scores: Vec<(Dim, u8)>,
    dim_regress: u32,
    last_code: C87Code,
    has_error: bool,
}

impl MultiDimScore {
    pub fn new() -> Self {
        Self {
            scores: Vec::new(),
            dim_regress: 0,
            last_code: C87Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C87Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn set(&mut self, dim: Dim, score: u8) {
        if let Some(e) = self.scores.iter_mut().find(|(d, _)| *d == dim) {
            e.1 = score;
        } else {
            self.scores.push((dim, score));
        }
    }

    /// 四维齐备。
    pub fn complete(&self) -> bool {
        Dim::ALL.iter().all(|d| self.scores.iter().any(|(x, _)| x == d))
    }

    /// 多维断言：不得退回单维。
    pub fn assert_multi(&mut self) -> Result<bool, C87Code> {
        if self.scores.len() >= 4 {
            self.last_code = C87Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.dim_regress += 1;
            self.last_code = C87Code::DIM_REGRESS;
            self.has_error = true;
            Err(C87Code::DIM_REGRESS)
        }
    }

    /// 各维均值。
    pub fn dim_avg(&self, dim: Dim) -> Option<u8> {
        self.scores
            .iter()
            .find(|(d, _)| *d == dim)
            .map(|(_, s)| *s)
    }

    pub fn dim_regress(&self) -> u32 {
        self.dim_regress
    }

    pub fn score_count(&self) -> usize {
        self.scores.len()
    }
}

// ── 刷分检测 ────────────────────────────────────────────────────────

/// 评价样本。
#[derive(Clone, Copy)]
pub struct ReviewSample {
    pub id: u32,
    /// 评分（1-5）。
    pub stars: u8,
    /// 来源指纹。
    pub source: u16,
    /// 时间窗（抽象 tick）。
    pub tick: u32,
}

/// 刷分检测器（反刷复用）。
pub struct RigDetector {
    samples: Vec<ReviewSample>,
    rigged_hits: u32,
    last_code: C87Code,
    has_error: bool,
}

impl RigDetector {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            rigged_hits: 0,
            last_code: C87Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C87Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn ingest(&mut self, s: ReviewSample) {
        self.samples.push(s);
    }

    /// 刷分审计：同源 ≥5 条同窗即刷分。
    pub fn audit(&mut self) -> Result<bool, C87Code> {
        let mut ok = true;
        for s in &self.samples {
            let same = self
                .samples
                .iter()
                .filter(|x| x.source == s.source && x.tick / 10 == s.tick / 10)
                .count();
            if same >= 5 {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C87Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.rigged_hits += 1;
            self.last_code = C87Code::RIGGED_SCORE;
            self.has_error = true;
            Err(C87Code::RIGGED_SCORE)
        }
    }

    pub fn rigged_hits(&self) -> u32 {
        self.rigged_hits
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

// ── 分布断言 ────────────────────────────────────────────────────────

/// 评分分布（1-5 星计数）。
pub struct ScoreDist {
    buckets: [u32; 5],
    mean_only_hits: u32,
    last_code: C87Code,
    has_error: bool,
}

impl ScoreDist {
    pub fn new() -> Self {
        Self {
            buckets: [0; 5],
            mean_only_hits: 0,
            last_code: C87Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C87Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, stars: u8) {
        if (1..=5).contains(&stars) {
            self.buckets[(stars - 1) as usize] += 1;
        }
    }

    /// 均值。
    pub fn mean(&self) -> f32 {
        let total: u32 = self.buckets.iter().sum();
        if total == 0 {
            return 0.0;
        }
        let weighted: u32 = self
            .buckets
            .iter()
            .enumerate()
            .map(|(i, c)| (i as u32 + 1) * c)
            .sum();
        weighted as f32 / total as f32
    }

    /// 分布断言：必须有分布（不只均值）。
    pub fn assert_distribution(&mut self) -> Result<bool, C87Code> {
        let nonzero = self.buckets.iter().filter(|c| **c > 0).count();
        if nonzero >= 2 {
            self.last_code = C87Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.mean_only_hits += 1;
            self.last_code = C87Code::MEAN_ONLY;
            self.has_error = true;
            Err(C87Code::MEAN_ONLY)
        }
    }

    /// 两极检测：1 星和 5 星都有。
    pub fn polarized(&self) -> bool {
        self.buckets[0] > 0 && self.buckets[4] > 0
    }

    pub fn mean_only_hits(&self) -> u32 {
        self.mean_only_hits
    }
}

// ── 评价治理 ────────────────────────────────────────────────────────

/// 治理动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovAction {
    /// 保留。
    Keep,
    /// 删除（恶意）。
    Remove,
    /// 复原（误删）。
    Restore,
}

/// 评价治理（误删审计复用）。
pub struct ReviewGovern {
    removed: Vec<u32>,
    drop_hits: u32,
    last_code: C87Code,
    has_error: bool,
}

impl ReviewGovern {
    pub fn new() -> Self {
        Self {
            removed: Vec::new(),
            drop_hits: 0,
            last_code: C87Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C87Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 治理动作。
    pub fn act(&mut self, id: u32, action: GovAction) -> Result<bool, C87Code> {
        match action {
            GovAction::Keep => Ok(true),
            GovAction::Remove => {
                self.removed.push(id);
                Ok(true)
            }
            GovAction::Restore => {
                if let Some(pos) = self.removed.iter().position(|x| *x == id) {
                    self.removed.remove(pos);
                    self.last_code = C87Code(0);
                    self.has_error = false;
                    Ok(true)
                } else {
                    self.drop_hits += 1;
                    self.last_code = C87Code::REVIEW_DROP;
                    self.has_error = true;
                    Err(C87Code::REVIEW_DROP)
                }
            }
        }
    }

    pub fn removed_count(&self) -> usize {
        self.removed.len()
    }

    pub fn drop_hits(&self) -> u32 {
        self.drop_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves87_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves87_ratings");
    {
        let mut m = MultiDimScore::new();
        m.set(Dim::Quality, 9);
        m.set(Dim::Usability, 8);
        m.set(Dim::Aesthetics, 7);
        m.set(Dim::Compat, 9);
        chk(&mut set, "C3687-01", m.complete() && m.assert_multi() == Ok(true), "err");
    }
    {
        let mut m = MultiDimScore::new();
        m.set(Dim::Quality, 9);
        let res = m.assert_multi();
        chk(&mut set, "C3687-02", res.err() == Some(C87Code::DIM_REGRESS), "err");
    }
    {
        let mut m = MultiDimScore::new();
        m.set(Dim::Quality, 8);
        chk(&mut set, "C3687-03", m.dim_avg(Dim::Quality) == Some(8), "err");
    }
    {
        let mut r = RigDetector::new();
        r.ingest(ReviewSample { id: 1, stars: 5, source: 1, tick: 10 });
        r.ingest(ReviewSample { id: 2, stars: 5, source: 2, tick: 11 });
        chk(&mut set, "C3687-04", r.audit() == Ok(true), "err");
    }
    {
        let mut r = RigDetector::new();
        for i in 0..5u32 {
            r.ingest(ReviewSample { id: i, stars: 5, source: 9, tick: 10 + i });
        }
        let res = r.audit();
        chk(&mut set, "C3687-05", res.err() == Some(C87Code::RIGGED_SCORE), "err");
    }
    {
        let mut d = ScoreDist::new();
        d.add(5);
        d.add(5);
        d.add(4);
        chk(&mut set, "C3687-06", d.assert_distribution() == Ok(true), "err");
    }
    {
        let mut d = ScoreDist::new();
        d.add(5);
        let res = d.assert_distribution();
        chk(&mut set, "C3687-07", res.err() == Some(C87Code::MEAN_ONLY), "err");
    }
    {
        let mut d = ScoreDist::new();
        d.add(1);
        d.add(5);
        chk(&mut set, "C3687-08", d.polarized(), "err");
    }
    {
        let mut d = ScoreDist::new();
        d.add(4);
        d.add(4);
        d.add(5);
        let mean = d.mean();
        chk(&mut set, "C3687-09", mean > 4.0 && mean < 4.5, "err");
    }
    {
        let mut g = ReviewGovern::new();
        g.act(1, GovAction::Remove);
        chk(&mut set, "C3687-10", g.removed_count() == 1, "err");
    }
    {
        let mut g = ReviewGovern::new();
        g.act(1, GovAction::Remove);
        let res = g.act(1, GovAction::Restore);
        chk(&mut set, "C3687-11", res == Ok(true) && g.removed_count() == 0, "err");
    }
    {
        let mut g = ReviewGovern::new();
        let res = g.act(9, GovAction::Restore);
        chk(&mut set, "C3687-12", res.err() == Some(C87Code::REVIEW_DROP), "err");
    }
    set
}
