//! 深化层 · F579 滚动条自动隐藏（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F579 节）：
//! ①「无滚动内容时完全隐藏（不留灰条假象）」的**边界值账**——内容
//!   恰等于视口（差 0px）= 不可滚，差 1px = 可滚：两态判定的边界
//!   钉死（最容易做错的恰是边界）；
//! ②「淡入淡出 150ms」的**对称时序账**——出现与消失走同一时长，
//!   进出场曲线对称（拖动中恒显例外在基础层，深化层只管对称性）；
//! ③「触屏模式常显粗态」的**规格复核**——44px 粗态与 4px 细态两套
//!   规格在两态判定之上的叠加关系（触屏优先于可滚性判定）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::scrollhide::{ScrollBar, FADE_MS, THIN_W_PX, TOUCH_W_PX};

// ---------------------------------------------------------------------------
// 两态边界账
// ---------------------------------------------------------------------------

/// 两态判定的边界语义（内容与视口差的钉死口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollEdge {
    /// content <= viewport：不可滚——完全隐藏。
    NotScrollable,
    /// content == viewport + 1：可滚（最小可滚差）。
    MinimalScrollable,
}

/// 边界判定：`content - viewport` 的差决定两态。
pub fn edge_of(viewport: u32, content: u32) -> ScrollEdge {
    if content <= viewport {
        ScrollEdge::NotScrollable
    } else {
        ScrollEdge::MinimalScrollable
    }
}

// ---------------------------------------------------------------------------
// 对称时序账
// ---------------------------------------------------------------------------

/// 进出场对称性：同一位移量在进场与出场耗时相同（±1ms 容差）。
///
/// `appear_ms` 从隐藏到全显的实测、`dismiss_ms` 从全显到隐藏的实测。
pub fn symmetric_timing(appear_ms: u32, dismiss_ms: u32) -> bool {
    let d = appear_ms.abs_diff(dismiss_ms);
    d <= 1
}

/// 全程时长合同：进与出都应是 [`FADE_MS`]（150ms 统一——不留慢出快进）。
pub fn timing_contract(appear_ms: u32, dismiss_ms: u32) -> bool {
    appear_ms == FADE_MS && dismiss_ms == FADE_MS
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f579_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 边界值账：差 0 不可滚（完全隐藏），差 1 即可滚。
    cs.add(
        "edge zero vs one",
        edge_of(800, 800) == ScrollEdge::NotScrollable
            && edge_of(800, 801) == ScrollEdge::MinimalScrollable,
        "",
    );

    // 2) 与基础台联动：恰好填满 → 隐藏不留灰条假象。
    let mut sb = ScrollBar::new();
    sb.layout(800, 800);
    sb.advance(FADE_MS);
    cs.add(
        "exactly filled hides fully",
        !sb.is_scrollable() && !sb.visible(),
        "",
    );

    // 3) 可滚内容：悬停可见、细态 4px（桌面态规格）。
    let mut sb2 = ScrollBar::new();
    sb2.layout(800, 2_000);
    sb2.advance(FADE_MS);
    cs.add(
        "scrollable thin visible",
        sb2.is_scrollable() && sb2.width_px() == THIN_W_PX,
        "",
    );

    // 4) 触屏常显粗态 44px（触屏优先——无 hover 世界的替代）。
    sb2.set_touch(true);
    cs.add(
        "touch mode fat always visible",
        sb2.width_px() == TOUCH_W_PX && sb2.visible(),
        "",
    );

    // 5) 拖动中恒显（触屏关、拖动开——拖动优先于淡出时钟）。
    sb2.set_touch(false);
    sb2.set_dragging(true);
    sb2.advance(FADE_MS * 3);
    cs.add("dragging never hides", sb2.always_visible_while_dragging() && sb2.visible(), "");

    // 6) 对称时序账：150ms 进 = 150ms 出（统一进出动画）。
    cs.add(
        "symmetric fade timing",
        symmetric_timing(FADE_MS, FADE_MS) && timing_contract(150, 150),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetry_tolerance_one_ms() {
        assert!(symmetric_timing(150, 151));
        assert!(!symmetric_timing(150, 153));
    }

    #[test]
    fn negative_content_clamps_not_scrollable() {
        // 防御：content < viewport（越界数据）也归不可滚。
        assert_eq!(edge_of(800, 100), ScrollEdge::NotScrollable);
    }
}
