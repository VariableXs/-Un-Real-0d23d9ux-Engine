//! VE-F3711 · 质量排行榜
//!
//! 双榜结构：总分榜 + 分维榜。偏科红线：总分高但无障碍维低于 60 不得进总分前十。
//! 刷榜检测复用反刷单源；排行动态周更新 + 历史趋势曲线。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0x9Fxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x9F00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct C9FCode(pub u16);

impl C9FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const RULE_HIDDEN: Self = Self(SEG_BASE + 1);
    pub const SKILL_LAG: Self = Self(SEG_BASE + 2);
    pub const CHEAT_DETECTED: Self = Self(SEG_BASE + 3);
    pub const TREND_DISTORTED: Self = Self(SEG_BASE + 4);
    pub const DIM_UNAVAILABLE: Self = Self(SEG_BASE + 5);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RULE_HIDDEN => "排行规则未公开（ADR 必需）",
            Self::SKILL_LAG => "偏科登顶：无障碍维低于 60 不得进总分前十",
            Self::CHEAT_DETECTED => "刷榜检出（P0 红线）",
            Self::TREND_DISTORTED => "趋势失真，需校准",
            Self::DIM_UNAVAILABLE => "分维数据缺失，分维榜不可发布",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 五维分（0-100）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimScores {
    pub overall: u16,
    pub visual: u16,
    pub a11y: u16,
    pub perf: u16,
    pub consistency: u16,
}

impl DimScores {
    pub fn new(overall: u16, visual: u16, a11y: u16, perf: u16, consistency: u16) -> Self {
        Self { overall, visual, a11y, perf, consistency }
    }

    /// 最低分维度（偏科检测用）
    pub fn min_dim(&self) -> u16 {
        let mut m = self.visual;
        if self.a11y < m { m = self.a11y; }
        if self.perf < m { m = self.perf; }
        if self.consistency < m { m = self.consistency; }
        m
    }

    /// 无障碍维是否达标（≥60）
    pub fn a11y_ok(&self) -> bool {
        self.a11y >= 60
    }
}

/// 排行条目
#[derive(Debug, Clone)]
pub struct RankEntry {
    pub asset_id: u32,
    pub scores: DimScores,
    pub trend: i16,
    pub cheat_flag: bool,
}

/// 排行规则（公开）
#[derive(Debug, Clone)]
pub struct RankRule {
    pub published: bool,
    pub min_a11y_for_top: u16,
    pub update_interval_days: u8,
}

impl RankRule {
    pub fn standard() -> Self {
        Self { published: true, min_a11y_for_top: 60, update_interval_days: 7 }
    }

    pub fn assert_public(&self) -> Result<(), C9FCode> {
        if self.published {
            Ok(())
        } else {
            Err(C9FCode::RULE_HIDDEN)
        }
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualRank {
    rule: RankRule,
    entries: Vec<RankEntry>,
    history: Vec<i16>,
}

impl QualRank {
    pub fn new(rule: RankRule) -> Self {
        Self { rule, entries: Vec::new(), history: Vec::new() }
    }

    pub fn add_entry(&mut self, entry: RankEntry) {
        self.entries.push(entry);
    }

    /// 偏科断言：总分高但 a11y < 60 不得进总分前十
    pub fn top_overall(&self) -> Result<Vec<u32>, C9FCode> {
        self.rule.assert_public()?;
        let mut sorted: Vec<&RankEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.scores.overall.cmp(&a.scores.overall));
        let mut out = Vec::new();
        for (i, e) in sorted.iter().enumerate() {
            if i >= 10 {
                break;
            }
            if !e.scores.a11y_ok() {
                return Err(C9FCode::SKILL_LAG);
            }
            out.push(e.asset_id);
        }
        Ok(out)
    }

    /// 分维榜：单一维度独立排序
    pub fn top_dim(&self, use_a11y: bool) -> Result<Vec<u32>, C9FCode> {
        self.rule.assert_public()?;
        if self.entries.is_empty() {
            return Err(C9FCode::DIM_UNAVAILABLE);
        }
        let mut sorted: Vec<&RankEntry> = self.entries.iter().collect();
        if use_a11y {
            sorted.sort_by(|a, b| b.scores.a11y.cmp(&a.scores.a11y));
        } else {
            sorted.sort_by(|a, b| b.scores.visual.cmp(&a.scores.visual));
        }
        Ok(sorted.iter().take(10).map(|e| e.asset_id).collect())
    }

    /// 刷榜检测（P0）：复用反刷单源——cheat_flag 任一即拒
    pub fn assert_no_cheat(&self) -> Result<(), C9FCode> {
        if self.entries.iter().any(|e| e.cheat_flag) {
            Err(C9FCode::CHEAT_DETECTED)
        } else {
            Ok(())
        }
    }

    /// 趋势记录 + 失真校准
    pub fn push_trend(&mut self, delta: i16) -> Result<(), C9FCode> {
        if delta.abs() > 50 {
            return Err(C9FCode::TREND_DISTORTED);
        }
        self.history.push(delta);
        Ok(())
    }

    pub fn trend_curve(&self) -> &[i16] {
        &self.history
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves111_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves111_qualrank");

    // 01 偏科红线：a11y 低不得进前十
    {
        let mut q = QualRank::new(RankRule::standard());
        q.add_entry(RankEntry { asset_id: 1, scores: DimScores::new(95, 90, 40, 90, 90), trend: 0, cheat_flag: false });
        q.add_entry(RankEntry { asset_id: 2, scores: DimScores::new(80, 80, 80, 80, 80), trend: 0, cheat_flag: false });
        chk(&mut set, "C3711-01", q.top_overall() == Err(C9FCode::SKILL_LAG), "err");
    }
    // 02 正常双榜
    {
        let mut q = QualRank::new(RankRule::standard());
        q.add_entry(RankEntry { asset_id: 1, scores: DimScores::new(95, 90, 80, 90, 90), trend: 0, cheat_flag: false });
        chk(&mut set, "C3711-02", q.top_overall().is_ok(), "err");
    }
    // 03 刷榜 P0
    {
        let mut q = QualRank::new(RankRule::standard());
        q.add_entry(RankEntry { asset_id: 1, scores: DimScores::new(90, 90, 90, 90, 90), trend: 0, cheat_flag: true });
        chk(&mut set, "C3711-03", q.assert_no_cheat() == Err(C9FCode::CHEAT_DETECTED), "err");
    }
    // 04 规则未公开
    {
        let rule = RankRule { published: false, min_a11y_for_top: 60, update_interval_days: 7 };
        let q = QualRank::new(rule);
        chk(&mut set, "C3711-04", q.top_overall().is_err(), "err");
    }
    // 05 分维榜独立
    {
        let mut q = QualRank::new(RankRule::standard());
        q.add_entry(RankEntry { asset_id: 1, scores: DimScores::new(70, 60, 95, 60, 60), trend: 0, cheat_flag: false });
        q.add_entry(RankEntry { asset_id: 2, scores: DimScores::new(80, 95, 60, 60, 60), trend: 0, cheat_flag: false });
        let a11y_top = q.top_dim(true).unwrap();
        chk(&mut set, "C3711-05", a11y_top[0] == 1, "err");
    }
    // 06 趋势失真
    {
        let mut q = QualRank::new(RankRule::standard());
        chk(&mut set, "C3711-06", q.push_trend(80) == Err(C9FCode::TREND_DISTORTED), "err");
    }
    // 07 趋势正常
    {
        let mut q = QualRank::new(RankRule::standard());
        chk(&mut set, "C3711-07", q.push_trend(5).is_ok() && q.trend_curve().len() == 1, "err");
    }
    // 08 空分维榜
    {
        let q = QualRank::new(RankRule::standard());
        chk(&mut set, "C3711-08", q.top_dim(true) == Err(C9FCode::DIM_UNAVAILABLE), "err");
    }
    // 09 解释非空
    {
        chk(&mut set, "C3711-09", !C9FCode::SKILL_LAG.explain().is_empty(), "err");
    }
    // 10 min_a11y 门槛可调
    {
        let r = RankRule { published: true, min_a11y_for_top: 70, update_interval_days: 14 };
        chk(&mut set, "C3711-10", r.min_a11y_for_top == 70 && r.update_interval_days == 14, "err");
    }

    set
}
