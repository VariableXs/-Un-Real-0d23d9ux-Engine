//! 深化层 · F573 候选词数量设置（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F573 节）：
//! ①「每页数量与翻页键（F326 引擎）联动」的 **翻页联动引擎**——总候选
//!   数 → 页数 → 末页余数的精确走查：逐页区间无缝衔接（前页 end ==
//!   后页 start，无重无漏）、末页越界一击拒、首页回卷一击拒；
//! ②「小屏/DPI 场景自动降档建议」的 **超界降档建议器**——可用高度/行
//!   高 → 容纳行数内取最大档且只往小降；只建议不硬截（当前档放得下
//!   不劝、最小档也放不下如实上抛 None）；
//! ③「档位即时生效」的 **选中项不丢账**——切档时全局选中序号向新档
//!   页码重投影，候选仍在集内（集缩水如实报丢）；
//! ④「默认 9」的 **审计锚**——三档清单升序 5/7/9、缺省档在档内、新分
//!   页器初始即 9。

use crate::checks::CheckSet;
use crate::istar::candcount::{CANDIDATE_TIERS, DEFAULT_TIER, CandPager};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 超界降档建议器
// ---------------------------------------------------------------------------

/// 降档建议器（只建议不硬截——候选窗超界时提示改档而非硬砍候选）。
pub struct TierAdvisor {
    /// 候选行高（px，宿主 DPI 管线注入）。
    pub line_h_px: u32,
}

impl TierAdvisor {
    pub fn new(line_h_px: u32) -> TierAdvisor {
        TierAdvisor { line_h_px }
    }

    /// 建议档位：可用高度能容纳的行数内取最大档，且必须比当前档小
    /// （降档方向）。None 的两种诚实情形：当前档已放得下（无需劝改）
    /// 或最小档也放不下（无档可降——超界如实上抛，不硬截不硬塞）。
    pub fn suggest(&self, avail_h_px: u32, current: usize) -> Option<usize> {
        if self.line_h_px == 0 {
            return None; // 行高非法不瞎建议。
        }
        let fits = (avail_h_px / self.line_h_px) as usize;
        let mut best: Option<usize> = None;
        for &t in CANDIDATE_TIERS.iter() {
            if t <= fits {
                best = Some(t);
            }
        }
        match best {
            Some(t) if t < current => Some(t),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 翻页联动引擎
// ---------------------------------------------------------------------------

/// 翻页全程走查：从首页一路翻到末页再补一记越界击。返回 (覆盖候选数,
/// 走查是否全过)。全过条件：逐页区间无缝衔接（前页 end == 后页 start
/// ——无重无漏）、越界 next 被拒、覆盖数恰为页数。
pub fn walk_audit(pager: &mut CandPager) -> (usize, bool) {
    while pager.prev_page() {}
    let pages = pager.page_count();
    let mut covered = 0usize;
    let mut visited = 0usize;
    for _ in 0..pages {
        let (a, b) = pager.range();
        if a != covered {
            return (visited, false); // 区间断层（有重或有漏）。
        }
        covered = b;
        visited += 1;
        if visited < pages && !pager.next_page() {
            return (visited, false);
        }
    }
    if pager.next_page() {
        return (visited, false); // 末页越界翻页未被拒。
    }
    (covered, visited == pages)
}

// ---------------------------------------------------------------------------
// 档位即时生效账（选中项不丢）
// ---------------------------------------------------------------------------

/// 切档重投影：全局选中序号在新档下的所在页（页码 = 序号 / 每页数）。
pub fn carry_selection(new_per_page: usize, sel_index: usize) -> usize {
    if new_per_page == 0 {
        0
    } else {
        sel_index / new_per_page
    }
}

/// 选中项仍在候选集内（切档不动候选集——丢选即红；空集如实报丢）。
pub fn selection_kept(sel_index: usize, total: usize) -> bool {
    total > 0 && sel_index < total
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f573_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 默认 9 审计锚：三档升序 5/7/9、缺省在档内、新分页器初始即 9。
    let tiers_ok = CANDIDATE_TIERS == [5, 7, 9]
        && CANDIDATE_TIERS.contains(&DEFAULT_TIER)
        && CandPager::new().per_page() == DEFAULT_TIER;
    cs.add("default nine audit anchor", tiers_ok, "");

    // 2) 翻页联动走查：22 候选 7/页 → 4 页（ceil），逐页无缝、末页越
    //    界拒、覆盖 22。
    let mut p = CandPager::new();
    let _ = p.set_per_page(7);
    p.set_total(22);
    let (covered, ok) = walk_audit(&mut p);
    cs.add(
        "paging walk seamless",
        ok && covered == 22 && p.page_count() == 4,
        "",
    );

    // 3) 末页余数精确：22 = 7+7+7 余 1（走查停在末页——区间 (21,22)）。
    let (start, end) = p.range();
    cs.add(
        "last page remainder exact",
        (start, end) == (21, 22) && end - start == 22 % 7,
        "",
    );

    // 4) 首页回卷拒：三记 prev 回 0（末页下标 3），第四记 prev 拒
    //    （翻页循环边界）。
    let b1 = p.prev_page();
    let b2 = p.prev_page();
    let b3 = p.prev_page();
    let at_home = p.page() == 0;
    let b4 = p.prev_page();
    cs.add("home boundary prev rejected", b1 && b2 && b3 && at_home && !b4, "");

    // 5) 降档建议器：行高 6、可用 45px → 容 7 行 → 9 档建议降 7；
    //    已是 7 档则不劝（当前档放得下）。
    let adv = TierAdvisor::new(6);
    let s9 = adv.suggest(45, 9);
    let s7 = adv.suggest(45, 7);
    cs.add("advisor suggests smaller tier", s9 == Some(7) && s7.is_none(), "");

    // 6) 建议器诚实两面：连最小档都放不下 → None（不硬截不硬塞）；
    //    恰容最小档 → 建议降 5。
    let none = adv.suggest(28, 9); // 28/6 = 4 行
    let five = adv.suggest(30, 9); // 30/6 = 5 行
    cs.add("advisor honest bounds", none.is_none() && five == Some(5), "");

    // 7) 档位即时生效 + 选中项不丢：9 档页 1 选中 12 → 切 5 档重投影
    //    页 2，落位后区间 (10,15) 恰含选中。
    let mut q = CandPager::new(); // 9/页
    q.set_total(20);
    let _ = q.next_page(); // 页 1：候选 9..18 含选中 12
    let carried = carry_selection(5, 12);
    let _ = q.set_per_page(5); // 基础件切档复位页 0（超界页号无意义）
    for _ in 0..carried {
        let _ = q.next_page(); // 深化账：按重投影页码落位
    }
    let (a, b) = q.range();
    cs.add(
        "selection survives tier switch",
        carried == 2 && selection_kept(12, 20) && (a, b) == (10, 15) && a <= 12 && 12 < b,
        "",
    );

    // 8) 选中项越界诚实：候选集缩水/空集后旧选中号如实报丢。
    cs.add(
        "shrink reports lost selection",
        !selection_kept(12, 10) && !selection_kept(5, 0),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisor_zero_line_height_refuses() {
        let adv = TierAdvisor::new(0);
        assert!(adv.suggest(100, 9).is_none());
    }

    #[test]
    fn walk_empty_total_clean() {
        let mut p = CandPager::new();
        p.set_total(0);
        let (covered, ok) = walk_audit(&mut p);
        assert_eq!(covered, 0);
        assert!(ok);
    }

    #[test]
    fn carry_first_page_selection() {
        assert_eq!(carry_selection(9, 3), 0);
        assert_eq!(carry_selection(5, 0), 0);
    }
}
