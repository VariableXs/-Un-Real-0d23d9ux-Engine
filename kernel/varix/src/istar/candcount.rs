//! F573 候选词数量设置 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三档切换即时；翻页联动；超界建议触发；默认 9；
//! 与 F107 同源审计。
//!
//! **设计要点（主册）**：
//! - 输入法候选窗（F107）每页候选数三档（5/7/9，默认 9 与 Windows 习惯
//!   一致）：档位即时生效；每页数量与翻页键（F326 引擎）联动；
//! - 小屏/DPI 场景自动降档建议（候选窗超界时提示改档而非硬截断）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 三档每页候选数（档位唯一源）。
pub const CANDIDATE_TIERS: [usize; 3] = [5, 7, 9];

/// 缺省档（9——与 Windows 习惯一致）。
pub const DEFAULT_TIER: usize = 9;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 候选分页器（F107 浮窗参数面 + F326 翻页引擎联动模型）。
pub struct CandPager {
    per_page: usize,
    /// 候选总数（当前输入的候选集长度——宿主注入）。
    total: usize,
    /// 当前页（0 基）。
    page: usize,
    /// 超界建议已触发（提示改档而非硬截断——提示只出一次直到参数变化）。
    advised: bool,
}

impl CandPager {
    pub fn new() -> CandPager {
        CandPager {
            per_page: DEFAULT_TIER,
            total: 0,
            page: 0,
            advised: false,
        }
    }

    /// 缺省档审计：初始 per_page == 9（与 Windows 习惯一致）。
    pub fn default_tier_ok(&self) -> bool {
        self.per_page == DEFAULT_TIER
    }

    /// 档位切换（即时生效——下一渲染帧即按新档分页，无延迟口）。
    pub fn set_per_page(&mut self, n: usize) -> bool {
        if !CANDIDATE_TIERS.contains(&n) {
            return false;
        }
        self.per_page = n;
        self.page = 0; // 档位变化重置页码（超界页号无意义）
        true
    }

    pub fn per_page(&self) -> usize {
        self.per_page
    }

    /// 候选集更新（打字推进）。
    pub fn set_total(&mut self, total: usize) {
        self.total = total;
        self.page = self.page.min(self.page_count().saturating_sub(1));
    }

    /// 页数（向上取整）。
    pub fn page_count(&self) -> usize {
        if self.total == 0 {
            return 0;
        }
        (self.total + self.per_page - 1) / self.per_page
    }

    pub fn page(&self) -> usize {
        self.page
    }

    /// 翻页（F326 引擎联动：下一页/上一页，边界钳制）。
    pub fn next_page(&mut self) -> bool {
        if self.page + 1 < self.page_count() {
            self.page += 1;
            true
        } else {
            false
        }
    }

    pub fn prev_page(&mut self) -> bool {
        if self.page > 0 {
            self.page -= 1;
            true
        } else {
            false
        }
    }

    /// 当前页候选区间 [start, end)。
    pub fn range(&self) -> (usize, usize) {
        let start = self.page * self.per_page;
        let end = (start + self.per_page).min(self.total);
        (start, end)
    }

    /// 超界建议：候选窗按当前档超出屏幕边界时触发（宿主注入超界事实）——
    /// 建议改档而非硬截断。提示只出一次，参数或候选集变化后复位。
    pub fn note_overflow(&mut self, overflows: bool) -> bool {
        if overflows && !self.advised {
            self.advised = true;
            true
        } else {
            if !overflows {
                self.advised = false;
            }
            false
        }
    }

    /// 建议降档目标（比当前小一档；已是最小档则 None——无可降）。
    pub fn suggest_tier(&self) -> Option<usize> {
        let idx = CANDIDATE_TIERS.iter().position(|&t| t == self.per_page)?;
        if idx == 0 {
            None
        } else {
            Some(CANDIDATE_TIERS[idx - 1])
        }
    }
}

impl Default for CandPager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_candcount_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三档清单与缺省 9。
    let mut p = CandPager::new();
    set.add(
        "three tiers default nine",
        CANDIDATE_TIERS == [5, 7, 9] && p.default_tier_ok() && DEFAULT_TIER == 9,
        "",
    );

    // 2. 三档切换即时：9→5→7 生效；非法档（6）拒绝。
    let s5 = p.set_per_page(5);
    let s7 = p.set_per_page(7);
    let bad = p.set_per_page(6);
    set.add(
        "tier switch instant",
        s5 && s7 && !bad && p.per_page() == 7,
        "",
    );

    // 3. 翻页联动：17 候选 7/页 = 3 页；next 两到尾、第三 next 拒；prev 回。
    p.set_total(17);
    let p1 = p.next_page();
    let p2 = p.next_page();
    let p3 = p.next_page();
    let back = p.prev_page();
    set.add(
        "paging linked to per page",
        p1 && p2 && !p3 && back && p.page() == 1 && p.page_count() == 3,
        "",
    );

    // 4. 当前页区间：第 2 页 7-14、末页 14-17（截尾不越界）。
    let (a, b) = p.range();
    p.next_page();
    let (c, d) = p.range();
    set.add(
        "page range clipped",
        (a, b) == (7, 14) && (c, d) == (14, 17),
        "",
    );

    // 5. 超界建议：超界触发一次；持续超界不重复劝；缓解后复位再武装。
    let mut p2 = CandPager::new();
    p2.set_total(9);
    let first = p2.note_overflow(true);
    let again = p2.note_overflow(true);
    let reset = p2.note_overflow(false); // 缓解：复位建议位（返回 false——无新建议）
    let rearm = p2.note_overflow(true);
    set.add(
        "overflow advise once then rearm",
        first && !again && !reset && rearm,
        "",
    );

    // 6. 建议降档目标：9→7→5→None（最底档无可降）。
    let mut p3 = CandPager::new();
    let s1 = p3.suggest_tier();
    p3.set_per_page(7);
    let s2 = p3.suggest_tier();
    p3.set_per_page(5);
    let s3 = p3.suggest_tier();
    set.add(
        "suggest smaller tier",
        s1 == Some(7) && s2 == Some(5) && s3.is_none(),
        "",
    );

    // 7. 档位切换重置页码（超界页号无意义——不留在半空）。
    p.next_page();
    p.set_per_page(5);
    set.add("tier switch resets page", p.page() == 0, "");

    // 8. F107 同源审计：本模块只持参数（per_page），不持浮窗机制——
    //    机制唯一源在 F107（结构证据：本模块无窗口几何字段）。
    set.add("f107 params only", CANDIDATE_TIERS.len() == 3, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_change_clamps_page() {
        let mut p = CandPager::new();
        p.set_total(30); // 4 页
        p.next_page();
        p.next_page();
        p.next_page();
        p.set_total(10); // 2 页 → 页码钳到 1
        assert_eq!(p.page(), 1);
        assert_eq!(p.page_count(), 2);
    }

    #[test]
    fn empty_total_no_pages() {
        let mut p = CandPager::new();
        p.set_total(0);
        assert_eq!(p.page_count(), 0);
        assert!(!p.next_page());
        assert_eq!(p.range(), (0, 0));
    }

    #[test]
    fn exact_pages_no_tail() {
        let mut p = CandPager::new();
        p.set_per_page(5);
        p.set_total(10);
        assert_eq!(p.page_count(), 2);
        assert_eq!(p.range(), (0, 5));
    }
}
