//! VE-F3519 · 护眼健康提示 —— 诚实表述 + 休息优先 + 可关不骚扰 + 频控。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3519`
//!
//! # 职责（锚点原文拆解）
//!
//! - **诚实表述**（[`HealthNudge`]）：提示文本从**循证文案表**
//!   [`EVIDENCE_TEXTS`] 取，每条带证据等级（F3514 [`Evidence`] 同源）。
//!   提示只说有据的：「连续用眼 40 分钟，建议远眺休息」是行为
//!   建议（有据），「调暖色温保护视力」是疗效断言（无据，禁用）。
//! - **休息优先**：提示的第一优先级永远是「休息」——色温调节是
//!   舒适性功能不是健康功能，这个定位写在提示体系的第一条。
//! - **可关不骚扰**（[`NudgePolicy`]）：提示整体可关；开着也只按
//!   频控出现（同一提示在冷却窗 [`COOLDOWN_TICKS`] 内不重复）。
//!   频控是数据不是承诺：冷却窗内的请求直接吞掉并计数。
//! - **频控审计**（[`NudgeStats`]）：出示次数/吞掉次数/关闭状态
//!   可查——「不骚扰」要能被验证，不是自我声明。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 骚扰 | 频控（冷却窗吞掉） |
//! | 提示失效 | 修复（文案表失锚即报） |
//! | 表述失实 | 修正（证据等级复检） |
//!
//! # 复杂度
//!
//! 全部 O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5860 起）
// ---------------------------------------------------------------------------

/// 健康提示域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NudgeCode {
    /// 提示文案失锚（文案表里没有这条 id）。
    TextMissing = 5860,
    /// 冷却窗内重复出示（已被吞掉）。
    CooldownSwallowed = 5861,
    /// 提示体系关闭中仍请求出示（吞掉并计数）。
    DisabledRequested = 5862,
    /// 话术越界（无证据级断言）。
    Overclaim = 5863,
}

impl NudgeCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            NudgeCode::TextMissing => "提示文案失锚",
            NudgeCode::CooldownSwallowed => "冷却窗内重复出示已吞掉",
            NudgeCode::DisabledRequested => "关闭中仍请求出示",
            NudgeCode::Overclaim => "健康话术越界",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, NudgeCode::Overclaim | NudgeCode::TextMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、循证文案表
// ---------------------------------------------------------------------------

/// 冷却窗（逻辑分钟）：同一条提示 30 分钟内不重复。
pub const COOLDOWN_TICKS: u32 = 30;

/// 提示文案（循证白名单，与 F3514 证据等级同源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NudgeText {
    /// 文案 id。
    pub id: u16,
    /// 文本。
    pub text: &'static str,
    /// 证据等级。
    pub evidence: super::ver08n_docs::Evidence,
}

/// 内置文案表（休息优先：休息类永远排最前）。
pub const EVIDENCE_TEXTS: [NudgeText; 4] = [
    NudgeText {
        id: 1,
        text: "连续用眼约 40 分钟，建议远眺 20 秒休息",
        evidence: super::ver08n_docs::Evidence::Strong,
    },
    NudgeText {
        id: 2,
        text: "保持 50 厘米以上的观看距离",
        evidence: super::ver08n_docs::Evidence::Strong,
    },
    NudgeText {
        id: 3,
        text: "降低夜间色温可能改善主观舒适度，证据有限",
        evidence: super::ver08n_docs::Evidence::Limited,
    },
    NudgeText {
        id: 4,
        text: "色温调节不构成医疗建议",
        evidence: super::ver08n_docs::Evidence::None,
    },
];

/// 从文案表取一条。
pub fn nudge_text(id: u16) -> Result<&'static NudgeText, NudgeCode> {
    EVIDENCE_TEXTS
        .iter()
        .find(|t| t.id == id)
        .ok_or(NudgeCode::TextMissing)
}

/// 话术复检：无证据级文案不得出现在主动出示位（id 4 只用于设置页）。
pub fn overclaim_guard(t: &NudgeText) -> Result<(), NudgeCode> {
    use super::ver08n_docs::Evidence;
    if t.evidence == Evidence::None && t.id != 4 {
        return Err(NudgeCode::Overclaim);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、频控与策略
// ---------------------------------------------------------------------------

/// 出示决策。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NudgeDecision {
    /// 出示。
    Show,
    /// 吞掉（冷却/关闭）。
    Swallow(NudgeCode),
}

/// 频控统计（不骚扰的可验证形态）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct NudgeStats {
    /// 实际出示次数。
    pub shown: u32,
    /// 吞掉次数。
    pub swallowed: u32,
}

/// 提示器。
pub struct HealthNudge {
    /// 用户开关（默认开）。
    pub enabled: bool,
    last_shown_at: u32,
    last_text_id: u16,
    stats: NudgeStats,
    codes: Vec<NudgeCode>,
}

impl HealthNudge {
    /// 开启起步。
    pub fn new() -> Self {
        HealthNudge {
            enabled: true,
            last_shown_at: u32::MAX,
            last_text_id: 0,
            stats: NudgeStats::default(),
            codes: Vec::new(),
        }
    }

    /// 统计。
    pub fn stats(&self) -> NudgeStats {
        self.stats
    }

    /// 审计码。
    pub fn codes(&self) -> &[NudgeCode] {
        &self.codes
    }

    /// 请求出示一条提示。`now` 为逻辑分钟。
    pub fn request(&mut self, id: u16, now: u32) -> Result<NudgeDecision, NudgeCode> {
        let t = nudge_text(id)?;
        overclaim_guard(t)?;
        if !self.enabled {
            self.stats.swallowed += 1;
            self.codes.push(NudgeCode::DisabledRequested);
            return Ok(NudgeDecision::Swallow(NudgeCode::DisabledRequested));
        }
        // 冷却：同一条提示在冷却窗内不重复（不同提示不受同窗限制）。
        if self.last_text_id == id {
            let elapsed = now.wrapping_sub(self.last_shown_at);
            if elapsed < COOLDOWN_TICKS {
                self.stats.swallowed += 1;
                self.codes.push(NudgeCode::CooldownSwallowed);
                return Ok(NudgeDecision::Swallow(NudgeCode::CooldownSwallowed));
            }
        }
        self.last_shown_at = now;
        self.last_text_id = id;
        self.stats.shown += 1;
        Ok(NudgeDecision::Show)
    }

    /// 用户关闭提示（尊重用户，不再请求即吞）。
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// 用户重新开启。
    pub fn enable(&mut self) {
        self.enabled = true;
        // 重开即重置冷却（用户意图变化）。
        self.last_text_id = 0;
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_first_and_cooldown() {
        let mut n = HealthNudge::new();
        assert_eq!(n.request(1, 100).unwrap(), NudgeDecision::Show);
        // 10 分钟内同条 → 吞。
        assert_eq!(
            n.request(1, 110).unwrap(),
            NudgeDecision::Swallow(NudgeCode::CooldownSwallowed)
        );
        // 31 分钟后 → 可再示。
        assert_eq!(n.request(1, 131).unwrap(), NudgeDecision::Show);
        assert_eq!(n.stats().shown, 2);
        assert_eq!(n.stats().swallowed, 1);
    }

    #[test]
    fn disabled_never_shows() {
        let mut n = HealthNudge::new();
        n.disable();
        assert_eq!(
            n.request(2, 5).unwrap(),
            NudgeDecision::Swallow(NudgeCode::DisabledRequested)
        );
        assert_eq!(n.stats().shown, 0);
    }

    #[test]
    fn missing_text_blocked() {
        let mut n = HealthNudge::new();
        assert_eq!(n.request(999, 1), Err(NudgeCode::TextMissing));
    }
}
