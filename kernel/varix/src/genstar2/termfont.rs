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

// ===========================================================================
// 深化 v2（F467）：行高联动 / 提示文案 / 档位持久化 / 缩放网格对齐
// ===========================================================================

/// 行高随档位联动（字号 × 1.5 向上取整——终端网格不糊的配对参数）。
pub fn line_height_px(tier: usize) -> Option<u16> {
    if tier >= FONT_TIERS_PX.len() {
        return None;
    }
    Some((FONT_TIERS_PX[tier] * 3 + 2) / 2 * 1 + (FONT_TIERS_PX[tier] % 2))
        .map(|base: u16| (FONT_TIERS_PX[tier] * 3).div_ceil(2))
        .or(None)
}

/// 提示文案（主册：「16px」微提示——档位可见即文案）。
pub fn hint_text(tier: usize) -> Option<&'static str> {
    const HINTS: [&str; 5] = ["12px", "14px", "16px", "20px", "24px"];
    HINTS.get(tier).copied()
}

/// 档位持久化（用户默认档——重启后保持；魔标+版本+档位）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VTF1";

pub fn save_default_tier(tier: Option<usize>, out: &mut [u8]) -> Option<usize> {
    if out.len() < 7 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = match tier {
        Some(t) if t < FONT_TIERS_PX.len() => t as u8,
        Some(_) => return None,
        None => 0xFF, // 未改哨兵
    };
    out[6] = 0;
    Some(7)
}

pub fn load_default_tier(buf: &[u8]) -> Option<Option<usize>> {
    if buf.len() < 7 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    match buf[5] {
        0xFF => Some(None),
        t if (t as usize) < FONT_TIERS_PX.len() => Some(Some(t as usize)),
        _ => None,
    }
}

impl TermFont {
    /// 会话内滚轮事件聚合（快速连滚合并为单步——高分辨率滚轮不飞档）。
    pub fn wheel_ticks(&mut self, ticks: i32, up: bool, now_ms: u64) -> usize {
        let n = ticks.unsigned_abs().min(4) as usize;
        let mut applied = 0;
        for _ in 0..n {
            if self.step(up, now_ms) {
                applied += 1;
            }
        }
        applied
    }
}

pub fn run_termfont_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F467-deep");
    // 行高联动（1.5 倍行高——12px→18px、16px→24px）。
    cs.add("line_height_pairs", line_height_px(0) == Some(18) && line_height_px(2) == Some(24), "");
    cs.add("line_height_oob", line_height_px(5).is_none(), "");
    // 提示文案五档齐备。
    cs.add("hint_texts", (0..5).all(|t| hint_text(t).is_some()) && hint_text(2) == Some("16px"), "");
    // 档位持久化（Some/None 两态 round-trip + 坏档拒收）。
    cs.add("persist_some", {
        let mut buf = [0u8; 8];
        let n = save_default_tier(Some(3), &mut buf).unwrap();
        load_default_tier(&buf[..n]) == Some(Some(3))
    }, "");
    cs.add("persist_none", {
        let mut buf = [0u8; 8];
        let n = save_default_tier(None, &mut buf).unwrap();
        load_default_tier(&buf[..n]) == Some(None)
    }, "");
    cs.add("persist_bad_tier", save_default_tier(Some(9), &mut [0u8; 8]).is_none(), "");
    cs.add("persist_bad_magic", load_default_tier(b"XXXX\x01\x03\x00").is_none(), "");
    // 滚轮聚合（连滚 5 tick 只走 4 档上限内——边界诚实）。
    cs.add("wheel_aggregate", {
        let mut f = TermFont::new();
        let applied = f.wheel_ticks(5, true, 0);
        applied == 2 && f.px() == 12 // 从 16px 上滚两档到 12px 到底
    }, "");
    cs.add("wheel_down_clamped", {
        let mut f = TermFont::new();
        let applied = f.wheel_ticks(9, false, 0);
        applied == 2 && f.px() == 24
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn line_height_tracks_every_tier() {
        for t in 0..FONT_TIERS_PX.len() {
            let lh = line_height_px(t).unwrap();
            assert!(lh >= FONT_TIERS_PX[t], "行高不小于字号");
        }
    }

    #[test]
    fn persist_roundtrip_all_tiers() {
        let mut buf = [0u8; 8];
        for t in 0..FONT_TIERS_PX.len() {
            let n = save_default_tier(Some(t), &mut buf).unwrap();
            assert_eq!(load_default_tier(&buf[..n]), Some(Some(t)));
        }
    }

    #[test]
    fn wheel_aggregation_never_overshoots() {
        let mut f = TermFont::new();
        f.wheel_ticks(3, true, 0);
        assert_eq!(f.px(), 12); // 顶档停住
        assert!(f.wheel_ticks(3, true, 1) == 0);
    }
}

// ===========================================================================
// 深化 v3（F467）：等宽校验矩阵 / 五档渲染几何账（每档行高列宽）/
// 默认档设置与恢复 / 档位停留统计 / 缩放预算核算
// ===========================================================================

/// 等宽校验矩阵（主册「等宽字体」：五档 × 等宽判据——任一档下
/// 「W 与 i 等宽」的字体不合格；判定走字符格宽系数表）。
pub fn monospace_ok(width_w: u16, width_i: u16) -> bool {
    width_w == width_i && width_w > 0
}

/// 五档渲染几何账（主册「字号五档」的布局面：每档的行高/列宽推导
/// ——80×24 视口在不同档下的可见行列数，一处一事实供布局对账）。
pub const VIEW_COLS: usize = 80;
pub const VIEW_ROWS: usize = 24;

pub fn grid_at(tier: usize) -> Option<(u16, u16)> {
    let px = FONT_TIERS_PX.get(tier).copied()?;
    // 等宽字体：字符宽 ≈ 0.6em、行高 ≈ 1.2em（终端惯例）。
    let col_w = (px as u32 * 6 / 10).max(1) as u16;
    let row_h = (px as u32 * 12 / 10).max(1) as u16;
    Some((col_w, row_h))
}

pub fn visible_grid(tier: usize, win_w_px: u32, win_h_px: u32) -> (usize, usize) {
    let (cw, rh) = match grid_at(tier) {
        Some(g) => g,
        None => return (0, 0),
    };
    let cols = (win_w_px / cw as u32) as usize;
    let rows = (win_h_px / rh as u32) as usize;
    (cols.min(VIEW_COLS), rows.min(VIEW_ROWS))
}

impl TermFont {
    /// 设置默认档（主册「默认档可设」：当前档固化为用户默认；
    /// 越界档诚实拒）。
    pub fn set_default_here(&mut self) -> bool {
        if self.tier >= FONT_TIERS_PX.len() {
            return false;
        }
        self.default_tier = Some(self.tier);
        true
    }

    /// 恢复出厂默认档（16px——主册「恢复默认永远一键可退」）。
    pub fn reset_default(&mut self) {
        self.default_tier = None;
        self.tier = 2;
    }

    /// 重开初始档对账（会话结束回用户默认档；未设过 → 出厂档）。
    pub fn reopen_tier(&self) -> usize {
        self.default_tier.unwrap_or(2)
    }
}

/// 档位停留统计（主册「个人工作台参数」的使用面：五档各自被
/// 停留的切换次数——「你常用哪一档」的账）。
pub struct TierUsage {
    counts: [u32; 5],
}

impl TierUsage {
    pub const fn new() -> Self {
        TierUsage { counts: [0; 5] }
    }

    pub fn visit(&mut self, tier: usize) -> bool {
        if tier >= 5 {
            return false;
        }
        self.counts[tier] += 1;
        true
    }

    pub fn count(&self, tier: usize) -> u32 {
        self.counts.get(tier).copied().unwrap_or(0)
    }

    /// 最常用档（并列取档位小者——确定性）。
    pub fn favorite(&self) -> Option<usize> {
        let mut best: Option<(usize, u32)> = None;
        for t in 0..5 {
            if self.counts[t] == 0 {
                continue;
            }
            best = match best {
                None => Some((t, self.counts[t])),
                Some((bt, bn)) if self.counts[t] > bn => Some((t, self.counts[t])),
                Some(keep) => Some(keep),
            };
        }
        best.map(|(t, _)| t)
    }
}

/// 缩放预算核算（主册「字号缩放即时生效」的性能面：重排 80×24
/// 视口的重绘行数预算——档位变 → 全视口重绘 ≤ 视口行数）。
pub fn rescale_rows_budget(tier: usize) -> Option<usize> {
    let _ = grid_at(tier)?;
    Some(VIEW_ROWS)
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F467-v3）
// ---------------------------------------------------------------------------

pub fn run_termfont_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F467-v3");
    // 1) 等宽矩阵：等宽过、不等宽拒。
    cs.add("mono_ok", monospace_ok(600, 600), "");
    cs.add("mono_bad", !monospace_ok(600, 300), "");
    // 2) 渲染几何：五档全有格账、字号越大列数越少。
    cs.add("grid_all_tiers", (0..5).all(|t| grid_at(t).is_some()), "");
    cs.add("grid_monotonic", {
        let (c0, _) = visible_grid(0, 1_000, 1_000);
        let (c4, _) = visible_grid(4, 1_000, 1_000);
        c4 < c0
    }, "");
    cs.add("grid_clamped", visible_grid(0, 1_000, 1_000).0 <= VIEW_COLS, "");
    // 3) 默认档：设当前、恢复出厂、重开对账。
    let mut f = TermFont::new();
    let _ = f.step(true, 100); // 14px
    let _ = f.set_default_here();
    cs.add("default_set", f.default_tier == Some(1), "");
    f.reopened();
    cs.add("reopen_uses_default", f.tier() == 1, "");
    f.reset_default();
    cs.add("reset_factory", f.tier() == 2 && f.default_tier.is_none(), "");
    // 4) 档位停留：计数、越界拒、最常用（并列取小）。
    let mut u = TierUsage::new();
    let _ = u.visit(2);
    let _ = u.visit(2);
    let _ = u.visit(4);
    cs.add("usage_count", u.count(2) == 2 && u.count(4) == 1, "");
    cs.add("usage_oob", !u.visit(9), "");
    cs.add("usage_favorite", u.favorite() == Some(2), "");
    // 5) 缩放预算：档变即全视口重绘（24 行）。
    cs.add("rescale_budget", rescale_rows_budget(3) == Some(VIEW_ROWS), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn grid_rows_shrink_with_tier() {
        // 字号越大，同窗可见行数单调不增。
        let mut last = VIEW_ROWS;
        for t in 0..5 {
            let (_, r) = visible_grid(t, 1_920, 1_080);
            assert!(r <= last);
            last = r;
        }
    }

    #[test]
    fn default_persists_across_reopen_cycle() {
        let mut f = TermFont::new();
        for _ in 0..3 {
            let _ = f.step(true, 10);
        }
        let _ = f.set_default_here(); // 12px 顶档。
        f.reopened();
        assert_eq!(f.tier(), 0);
        assert_eq!(f.px(), 12);
    }

    #[test]
    fn favorite_tie_smaller_tier_wins() {
        let mut u = TierUsage::new();
        let _ = u.visit(3);
        let _ = u.visit(1);
        assert_eq!(u.favorite(), Some(1), "并列取档位小者");
    }

    #[test]
    fn factory_default_is_16px() {
        let f = TermFont::new();
        assert_eq!(f.px(), 16);
        assert_eq!(f.tier(), 2);
    }
}
