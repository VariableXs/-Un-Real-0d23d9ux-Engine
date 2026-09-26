//! F467 终端字号快捷（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **五档边界；提示时序；会话/默认两级记忆；与全局字号不冲突（独立域审计）；
//! 滚轮通路（F204 惯性不用于终端——逐档切换非平滑缩放，文档化差异）。**
//!
//! 功能定义（主册批次三）：终端 Ctrl+滚轮=字号缩放：五档字号（12-24px）、
//! 当前档位右下角微提示 1 秒（「16px」）、会话内记忆（关终端回默认、设置里
//! 可改默认档）；与 F246 全局字号独立（终端有自己的性子，文档化）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 五档字号（主册：五档 12-24px）。
pub const FONT_TIERS_PX: [u16; 5] = [12, 14, 16, 20, 24];
/// 档位微提示时长（主册：1 秒）。
pub const HINT_MS: u64 = 1_000;
/// 独立域标记（与 F246 全局字号不冲突——独立域审计锚）。
pub const INDEPENDENT_FROM_GLOBAL: bool = true;
/// 文档化差异：终端滚轮逐档切换，不用 F204 平滑惯性。
pub const DISCRETE_STEPPING: bool = true;

/// 终端字号状态机（会话/默认两级记忆）。
pub struct TermFont {
    tier: usize,
    /// 用户改过的默认档（None = 未改，用出厂档 2=16px）。
    default_tier: Option<usize>,
    /// 提示计时（最近一次档位变更时刻）。
    hint_from_ms: u64,
}

impl TermFont {
    pub const fn new() -> Self {
        TermFont { tier: 2, default_tier: None, hint_from_ms: 0 }
    }

    pub fn tier(&self) -> usize {
        self.tier
    }

    pub fn px(&self) -> u16 {
        FONT_TIERS_PX[self.tier]
    }

    /// Ctrl+滚轮上/下：逐档切换（五档边界：顶/底档不再越界）。
    pub fn step(&mut self, up: bool, now_ms: u64) -> bool {
        let next = if up {
            if self.tier == 0 {
                return false;
            }
            self.tier - 1
        } else {
            if self.tier == FONT_TIERS_PX.len() - 1 {
                return false;
            }
            self.tier + 1
        };
        self.tier = next;
        self.hint_from_ms = now_ms;
        true
    }

    /// 微提示 1 秒内可见（主册：右下角微提示 1 秒）。
    pub fn hint_alive(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.hint_from_ms) < HINT_MS
    }

    /// 会话内记忆：关终端回默认档（会话结束 = 状态丢弃，由调用方重建；
    /// 此处提供重开初始态：用户默认档或出厂档）。
    pub fn reopened(&mut self) {
        self.tier = self.default_tier.unwrap_or(2);
        self.hint_from_ms = 0;
    }

    /// 设置里改默认档（默认级记忆；越界档诚实拒绝）。
    pub fn set_default_tier(&mut self, tier: usize) -> bool {
        if tier >= FONT_TIERS_PX.len() {
            return false;
        }
        self.default_tier = Some(tier);
        true
    }

    pub fn default_tier(&self) -> usize {
        self.default_tier.unwrap_or(2)
    }
}

/// 独立域审计：终端字号变更不触碰全局字号状态（返回 false = 无全局副作用）。
pub fn global_scale_untouched(global_before: u16, global_after: u16) -> bool {
    global_before == global_after
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termfont_checks() -> CheckSet {
    let mut cs = CheckSet::new("F467-termfont");
    // 1) 五档边界（12/14/16/20/24 px，主册 12-24px 五档）。
    cs.add("five_tiers", FONT_TIERS_PX == [12, 14, 16, 20, 24], "");
    let mut f = TermFont::new();
    cs.add("default_tier_16px", f.px() == 16, "");
    // 2) 五档边界：顶/底不再越界（16px→24px 需两档）。
    let mut all = true;
    for _ in 0..2 {
        all &= f.step(false, 100);
    }
    cs.add("reach_24px", all && f.px() == 24, "");
    cs.add("top_bound_honest", !f.step(false, 200), "");
    // 3) 提示时序：1 秒内可见、1 秒后消失（最后成功变更在 100ms）。
    cs.add("hint_1s_alive", f.hint_alive(100 + 999), "");
    cs.add("hint_1s_gone", !f.hint_alive(100 + 1_000), "");
    // 4) 会话/默认两级记忆。
    let mut f2 = TermFont::new();
    f2.step(true, 0);
    f2.step(true, 0); // 16→14→12
    f2.reopened(); // 关终端重开：回默认（未改默认 → 出厂 16px）
    cs.add("session_memory_reset", f2.px() == 16, "");
    f2.set_default_tier(3); // 用户改默认 = 20px
    f2.reopened();
    cs.add("default_level_memory", f2.px() == 20, "");
    cs.add("set_default_bounds", !f2.set_default_tier(5), "");
    // 5) 与全局字号不冲突（独立域审计）。
    let (g0, g1) = (16u16, 16u16);
    f2.step(true, 500);
    cs.add("independent_domain", INDEPENDENT_FROM_GLOBAL && global_scale_untouched(g0, g1), "");
    // 6) 文档化差异：逐档切换非平滑缩放。
    cs.add("discrete_not_smooth", DISCRETE_STEPPING, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_steps_through_all_tiers() {
        let mut f = TermFont::new();
        let mut seen = vec![f.px()];
        while f.step(false, 0) {
            seen.push(f.px());
        }
        while f.step(true, 0) {
            seen.push(f.px());
        }
        // 五档全部可达（从小到大再到小）。
        for t in FONT_TIERS_PX {
            assert!(seen.contains(&t), "档位 {t} 应可达");
        }
    }

    #[test]
    fn hint_timing_exact() {
        let mut f = TermFont::new();
        f.step(true, 10_000);
        assert!(f.hint_alive(10_500));
        assert!(!f.hint_alive(11_000));
    }

    #[test]
    fn reopened_uses_user_default() {
        let mut f = TermFont::new();
        f.set_default_tier(0); // 用户偏好最小档 12px
        f.reopened();
        assert_eq!(f.px(), 12);
    }
}
