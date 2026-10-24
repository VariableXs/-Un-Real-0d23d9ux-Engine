//! VE-F3666 · 主题智能推荐（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3666`
//!
//! **职责定位（锚点原文）**：主题智能推荐——
//! - **推荐输入**：本地画像（F3665 单源，复述本地红线）；
//! - **推荐与应用分离**（F3661 主权红线复述）：推荐 ≠ 应用，必须用户确认；
//! - **可解释**：每条推荐带理由（黑盒推荐 = 操纵嫌疑）；
//! - **可拒绝**：拒绝后不再推（拒绝权）。
//!
//! **判据（锚点原文）**：本地输入、推荐分离、可解释、拒绝权、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 推荐擅自应用→P1（分离红线实测） | [`T66Code::AUTO_APPLY`] |
//! | 理由缺失→拦截（黑盒红线） | [`T66Code::NO_REASON`] |
//! | 拒绝后仍推→P1（拒绝权红线） | [`T66Code::IGNORED_REFUSAL`] |
//! | 云端画像输入→P0（本地红线） | [`T66Code::CLOUD_INPUT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3666 诊断码段（独占 0x72xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T66Code(pub u16);

impl T66Code {
    /// 推荐被擅自应用（P1）。
    pub const AUTO_APPLY: T66Code = T66Code(0x7201);
    /// 理由缺失（黑盒红线）。
    pub const NO_REASON: T66Code = T66Code(0x7202);
    /// 拒绝后仍推（P1 拒绝权红线）。
    pub const IGNORED_REFUSAL: T66Code = T66Code(0x7203);
    /// 云端画像输入（P0 本地红线）。
    pub const CLOUD_INPUT: T66Code = T66Code(0x7204);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            T66Code::AUTO_APPLY => "推荐被擅自应用（P1 分离红线）。建议：等待用户确认。",
            T66Code::NO_REASON => "推荐理由缺失（黑盒红线）。建议：补可解释理由。",
            T66Code::IGNORED_REFUSAL => "拒绝后仍推（P1 拒绝权）。建议：尊重拒绝不再推。",
            T66Code::CLOUD_INPUT => "云端画像输入（P0 本地红线）。建议：只用本地画像。",
            _ => "未知诊断码。",
        }
    }
}

// ── 推荐条目 ────────────────────────────────────────────────────────

/// 推荐项。
#[derive(Clone, Copy)]
pub struct Recommendation {
    pub theme_id: u16,
    /// 理由长度（0 = 黑盒）。
    pub reason_len: u32,
    /// 匹配分（基点）。
    pub score_bps: u32,
}

impl Recommendation {
    /// 可解释断言。
    pub fn explainable(&self) -> bool {
        self.reason_len > 0
    }
}

// ── 推荐引擎 ────────────────────────────────────────────────────────

/// 主题智能推荐引擎。
pub struct ThemeRecommender {
    recs: Vec<Recommendation>,
    rejected: Vec<u16>,
    /// 画像输入源（true = 本地）。
    local_input: bool,
    auto_applied: u32,
    ignore_hits: u32,
    last_code: T66Code,
    has_error: bool,
}

impl ThemeRecommender {
    pub fn new(local_input: bool) -> Self {
        Self {
            recs: Vec::new(),
            rejected: Vec::new(),
            local_input,
            auto_applied: 0,
            ignore_hits: 0,
            last_code: T66Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> T66Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: T66Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 生成推荐：必须本地画像输入 + 理由非空 + 未拒绝。
    pub fn recommend(&mut self, theme_id: u16, reason_len: u32, score_bps: u32) -> Result<usize, T66Code> {
        if !self.local_input {
            self.fail(T66Code::CLOUD_INPUT);
            return Err(T66Code::CLOUD_INPUT);
        }
        if reason_len == 0 {
            self.fail(T66Code::NO_REASON);
            return Err(T66Code::NO_REASON);
        }
        if self.rejected.contains(&theme_id) {
            self.ignore_hits += 1;
            self.fail(T66Code::IGNORED_REFUSAL);
            return Err(T66Code::IGNORED_REFUSAL);
        }
        self.recs.push(Recommendation { theme_id, reason_len, score_bps });
        self.last_code = T66Code(0);
        self.has_error = false;
        Ok(self.recs.len())
    }

    /// 用户确认应用（推荐与应用分离）。
    pub fn user_apply(&mut self, theme_id: u16, confirmed: bool) -> Result<u16, T66Code> {
        if !confirmed {
            self.auto_applied += 1;
            self.fail(T66Code::AUTO_APPLY);
            return Err(T66Code::AUTO_APPLY);
        }
        if !self.recs.iter().any(|r| r.theme_id == theme_id) {
            self.fail(T66Code::NO_REASON);
            return Err(T66Code::NO_REASON);
        }
        self.last_code = T66Code(0);
        self.has_error = false;
        Ok(theme_id)
    }

    /// 拒绝推荐（拒绝权）。
    pub fn reject(&mut self, theme_id: u16) -> usize {
        if !self.rejected.contains(&theme_id) {
            self.rejected.push(theme_id);
        }
        self.rejected.len()
    }

    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }

    pub fn auto_applied(&self) -> u32 {
        self.auto_applied
    }

    pub fn ignore_hits(&self) -> u32 {
        self.ignore_hits
    }

    /// 全部可解释。
    pub fn all_explainable(&self) -> bool {
        self.recs.iter().all(|r| r.explainable())
    }

    /// 最高分推荐。
    pub fn best(&self) -> Option<u16> {
        self.recs.iter().max_by_key(|r| r.score_bps).map(|r| r.theme_id)
    }

    /// 推荐分离总评：无擅自应用 + 本地输入。
    pub fn separation_green(&self) -> bool {
        self.auto_applied == 0 && self.local_input
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves66_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves66_themerec");
    {
        let e = ThemeRecommender::new(true);
        chk(&mut set, "C3666-01", e.rec_count() == 0 && e.rejected_count() == 0, "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        let r = e.recommend(1, 20, 8_000);
        chk(&mut set, "C3666-02", r == Ok(1), "err");
    }
    {
        let mut e = ThemeRecommender::new(false);
        let r = e.recommend(1, 20, 8_000);
        chk(&mut set, "C3666-03", r.err() == Some(T66Code::CLOUD_INPUT), "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        let r = e.recommend(1, 0, 8_000);
        chk(&mut set, "C3666-04", r.err() == Some(T66Code::NO_REASON), "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        e.reject(2);
        let r = e.recommend(2, 10, 100);
        chk(&mut set, "C3666-05", r.err() == Some(T66Code::IGNORED_REFUSAL) && e.ignore_hits() == 1, "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        e.recommend(1, 10, 100);
        let r = e.user_apply(1, true);
        chk(&mut set, "C3666-06", r == Ok(1), "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        e.recommend(1, 10, 100);
        let r = e.user_apply(1, false);
        chk(&mut set, "C3666-07", r.err() == Some(T66Code::AUTO_APPLY) && e.auto_applied() == 1, "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        e.recommend(3, 15, 900);
        e.recommend(4, 12, 700);
        chk(&mut set, "C3666-08", e.all_explainable() && e.best() == Some(3), "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        let n = e.reject(5);
        chk(&mut set, "C3666-09", n == 1 && e.rejected_count() == 1, "err");
    }
    {
        let mut e = ThemeRecommender::new(true);
        e.recommend(1, 5, 100);
        chk(&mut set, "C3666-10", e.separation_green(), "err");
    }
    {
        let mut e = ThemeRecommender::new(false);
        let r = e.user_apply(9, true);
        chk(&mut set, "C3666-11", r.err() == Some(T66Code::NO_REASON), "err");
    }
    {
        let rec = Recommendation { theme_id: 1, reason_len: 30, score_bps: 100 };
        chk(&mut set, "C3666-12", rec.explainable(), "err");
    }
    set
}
