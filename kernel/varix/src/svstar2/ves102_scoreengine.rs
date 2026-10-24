// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves102_scoreengine
// VE-F3702 · 主题质量评分引擎（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3702
// 判据原文：五维引擎、和谐公开、三单源、可解释全域、专家对拍、判据。
//
// 难点分析：
//   1. 评分可解释是全域红线——每维度子分必须带依据数据，黑箱分即 P1。
//   2. 色彩和谐度量算法必须公开——和谐度量本身是评分依据，暗箱和谐度=伪客观。
//   3. 专家对拍制度：算法评分与专家评审偏差超阈值即校准。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9600 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9600;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C96Code(pub u16);

impl C96Code {
    pub const SCORE_BLACKBOX: Self = Self(SEG_BASE);    // 评分黑箱 P1
    pub const HARMONY_UNPUBLIC: Self = Self(SEG_BASE + 1); // 和谐度量未公开
    pub const CALIBRATION_DUE: Self = Self(SEG_BASE + 2); // 对拍偏差超阈值
    pub const DIM_SCORE_MISSING: Self = Self(SEG_BASE + 3); // 维度子分缺失
    pub const ENGINE_READY: Self = Self(SEG_BASE + 4);  // 引擎就绪
    pub const EXPLAINABLE: Self = Self(SEG_BASE + 5);   // 可解释合规
    pub const HARMONY_PUBLIC: Self = Self(SEG_BASE + 6); // 和谐度量已公开

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::SCORE_BLACKBOX => "评分黑箱——可解释红线 P1",
            Self::HARMONY_UNPUBLIC => "和谐度量未公开——算法公开复述",
            Self::CALIBRATION_DUE => "专家对拍偏差超阈值——须校准",
            Self::DIM_SCORE_MISSING => "维度子分缺失——可解释要求未满足",
            Self::ENGINE_READY => "五维评分引擎就绪",
            Self::EXPLAINABLE => "可解释合规",
            Self::HARMONY_PUBLIC => "和谐度量公开合规",
            _ => "未知诊断",
        }
    }
}

// ── 色彩和谐度量 ──────────────────────────────────────────────────

/// 色彩对和谐度量——算法公开。
pub struct HarmonyMetric {
    /// 算法版本号（公开）。
    pub algo_version: u16,
    /// 是否公开。
    pub published: bool,
}

impl HarmonyMetric {
    /// v1：基于色相距离的简谐模型（公开公式）。
    /// harmony = 1 - |hue_a - hue_b| / 180（折叠到 0..1）
    pub fn v1() -> Self {
        Self { algo_version: 1, published: true }
    }

    /// 计算和谐度（0..10000 基点）。
    pub fn compute(&self, hue_a_deg: u16, hue_b_deg: u16) -> u16 {
        let a = (hue_a_deg % 360) as i32;
        let b = (hue_b_deg % 360) as i32;
        let mut diff = (a - b).abs();
        if diff > 180 {
            diff = 360 - diff;
        }
        let harmony_bp = 10000 - (diff as u32 * 10000 / 180);
        harmony_bp as u16
    }

    /// 公开检查。
    pub fn check_public(&self) -> Result<(), C96Code> {
        if self.published {
            Ok(())
        } else {
            Err(C96Code::HARMONY_UNPUBLIC)
        }
    }
}

// ── 可解释子分 ────────────────────────────────────────────────────

/// 维度子分+依据。
pub struct DimScore {
    /// 子分（0..10000）。
    pub score_bp: u16,
    /// 依据数据指纹。
    pub evidence_fp: u64,
    /// 是否有依据。
    pub has_evidence: bool,
}

impl DimScore {
    pub fn new(score_bp: u16, evidence_fp: u64) -> Self {
        Self { score_bp, evidence_fp, has_evidence: true }
    }

    /// 无依据子分（黑箱）。
    pub fn opaque(score_bp: u16) -> Self {
        Self { score_bp, evidence_fp: 0, has_evidence: false }
    }

    /// 可解释检查。
    pub fn check_explainable(&self) -> Result<(), C96Code> {
        if self.has_evidence {
            Ok(())
        } else {
            Err(C96Code::SCORE_BLACKBOX)
        }
    }
}

// ── 五维评分引擎 ──────────────────────────────────────────────────

/// 五维评分结果。
pub struct ScoreCard {
    /// 五维子分（一致性/无障碍/性能/兼容/美观）。
    scores: [Option<DimScore>; 5],
}

impl ScoreCard {
    pub fn new() -> Self {
        Self { scores: [const { None }; 5] }
    }

    /// 设置维度子分。
    pub fn set(&mut self, dim_idx: usize, s: DimScore) {
        if dim_idx < 5 {
            self.scores[dim_idx] = Some(s);
        }
    }

    /// 全维已评？
    pub fn all_scored(&self) -> bool {
        self.scores.iter().all(|x| x.is_some())
    }

    /// 全维可解释？
    pub fn all_explainable(&self) -> Result<(), C96Code> {
        for s in &self.scores {
            match s {
                Some(ds) => ds.check_explainable()?,
                None => return Err(C96Code::DIM_SCORE_MISSING),
            }
        }
        Ok(())
    }

    /// 总分 = 五维简单平均（基点）。
    pub fn total_bp(&self) -> u32 {
        let mut sum = 0u32;
        let mut n = 0u32;
        for s in &self.scores {
            if let Some(ds) = s {
                sum += ds.score_bp as u32;
                n += 1;
            }
        }
        if n == 0 {
            0
        } else {
            sum / n
        }
    }

    /// 缺分数。
    pub fn missing_count(&self) -> usize {
        self.scores.iter().filter(|x| x.is_none()).count()
    }
}

impl Default for ScoreCard {
    fn default() -> Self {
        Self::new()
    }
}

// ── 专家对拍 ──────────────────────────────────────────────────────

/// 对拍样本。
pub struct CalibrationSample {
    /// 算法分（基点）。
    pub algo_bp: u16,
    /// 专家分（基点）。
    pub expert_bp: u16,
}

impl CalibrationSample {
    pub fn new(algo_bp: u16, expert_bp: u16) -> Self {
        Self { algo_bp, expert_bp }
    }

    /// 偏差（绝对值基点）。
    pub fn deviation(&self) -> u16 {
        let d = self.algo_bp as i32 - self.expert_bp as i32;
        d.abs() as u16
    }
}

/// 专家对拍器。
pub struct ExpertCalibrator {
    /// 偏差阈值（基点）。
    pub threshold_bp: u16,
    /// 最近偏差。
    last_deviation: u16,
    /// 校准触发数。
    calibrations: u32,
}

impl ExpertCalibrator {
    pub fn standard() -> Self {
        Self { threshold_bp: 1000, last_deviation: 0, calibrations: 0 }
    }

    /// 对拍单样本。偏差超阈值→校准。
    pub fn calibrate(&mut self, s: &CalibrationSample) -> Result<(), C96Code> {
        self.last_deviation = s.deviation();
        if self.last_deviation > self.threshold_bp {
            self.calibrations = self.calibrations.saturating_add(1);
            return Err(C96Code::CALIBRATION_DUE);
        }
        Ok(())
    }

    /// 批量对拍：任一超阈值即校准。
    pub fn calibrate_batch(&mut self, samples: &[CalibrationSample]) -> Result<(), C96Code> {
        for s in samples {
            self.calibrate(s)?;
        }
        Ok(())
    }

    pub fn last_deviation(&self) -> u16 {
        self.last_deviation
    }

    pub fn calibrations(&self) -> u32 {
        self.calibrations
    }
}

// ── 评分引擎总装 ──────────────────────────────────────────────────

/// 评分引擎。
pub struct ScoreEngine {
    pub harmony: HarmonyMetric,
    pub card: ScoreCard,
    pub calibrator: ExpertCalibrator,
}

impl ScoreEngine {
    pub fn new() -> Self {
        Self {
            harmony: HarmonyMetric::v1(),
            card: ScoreCard::new(),
            calibrator: ExpertCalibrator::standard(),
        }
    }

    /// 引擎就绪 = 和谐公开 + 全维可解释 + 全维已评。
    pub fn ready(&self) -> Result<(), C96Code> {
        self.harmony.check_public()?;
        self.card.all_explainable()?;
        if !self.card.all_scored() {
            return Err(C96Code::DIM_SCORE_MISSING);
        }
        Ok(())
    }
}

impl Default for ScoreEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves102_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves102_scoreengine");
    {
        // 01 和谐度量公开
        let h = HarmonyMetric::v1();
        chk(&mut set, "C396-01", h.check_public().is_ok(), "和谐度量算法公开（v1）");
    }
    {
        // 02 和谐度计算——同色和谐最高
        let h = HarmonyMetric::v1();
        let same = h.compute(120, 120);
        let opposite = h.compute(0, 180);
        chk(
            &mut set,
            "C396-02",
            same == 10000 && opposite < 2000,
            "同色相=10000 基点和谐，互补色≈0",
        );
    }
    {
        // 03 和谐度对称
        let h = HarmonyMetric::v1();
        let ab = h.compute(30, 200);
        let ba = h.compute(200, 30);
        chk(&mut set, "C396-03", ab == ba, "和谐度对称：h(a,b)==h(b,a)");
    }
    {
        // 04 无依据子分即黑箱
        let s = DimScore::opaque(5000);
        chk(
            &mut set,
            "C396-04",
            s.check_explainable().err() == Some(C96Code::SCORE_BLACKBOX),
            "无依据子分=评分黑箱 P1（可解释红线）",
        );
    }
    {
        // 05 有依据合规
        let s = DimScore::new(5000, 0xDEAD);
        chk(&mut set, "C396-05", s.check_explainable().is_ok(), "带依据指纹可解释");
    }
    {
        // 06 全维可解释
        let mut card = ScoreCard::new();
        for i in 0..5 {
            card.set(i, DimScore::new(8000, i as u64 + 1));
        }
        chk(
            &mut set,
            "C396-06",
            card.all_scored() && card.all_explainable().is_ok(),
            "五维全评+全可解释",
        );
    }
    {
        // 07 缺维被拒
        let mut card = ScoreCard::new();
        card.set(0, DimScore::new(8000, 1));
        let r = card.all_explainable();
        chk(
            &mut set,
            "C396-07",
            r.err() == Some(C96Code::DIM_SCORE_MISSING) && card.missing_count() == 4,
            "缺 4 维子分被拒",
        );
    }
    {
        // 08 总分平均
        let mut card = ScoreCard::new();
        for i in 0..5 {
            card.set(i, DimScore::new(5000, 1));
        }
        chk(&mut set, "C396-08", card.total_bp() == 5000, "五维同分均值正确");
    }
    {
        // 09 对拍偏差在阈值内
        let mut cal = ExpertCalibrator::standard();
        let s = CalibrationSample::new(8000, 7500); // 偏差 500
        chk(&mut set, "C396-09", cal.calibrate(&s).is_ok(), "偏差 500 < 阈值 1000 通过");
    }
    {
        // 10 对拍偏差超阈值
        let mut cal = ExpertCalibrator::standard();
        let s = CalibrationSample::new(8000, 6000); // 偏差 2000
        let r = cal.calibrate(&s);
        chk(
            &mut set,
            "C396-10",
            r.err() == Some(C96Code::CALIBRATION_DUE) && cal.calibrations() == 1,
            "偏差 2000 > 阈值→校准触发",
        );
    }
    {
        // 11 引擎就绪
        let mut eng = ScoreEngine::new();
        for i in 0..5 {
            eng.card.set(i, DimScore::new(7000, i as u64 + 10));
        }
        chk(&mut set, "C396-11", eng.ready().is_ok(), "和谐公开+全维可解释→引擎就绪");
    }
    {
        // 12 批量对拍
        let mut cal = ExpertCalibrator::standard();
        let samples = alloc::vec![
            CalibrationSample::new(8000, 7800),
            CalibrationSample::new(8000, 6500), // 超阈值
        ];
        let r = cal.calibrate_batch(&samples);
        chk(
            &mut set,
            "C396-12",
            r.err() == Some(C96Code::CALIBRATION_DUE),
            "批量对拍任一超阈值即校准",
        );
    }
    set
}
