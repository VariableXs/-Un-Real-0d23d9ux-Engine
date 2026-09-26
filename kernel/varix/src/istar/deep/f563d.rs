//! 深化层 · F563 任务视图搜索（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F563 节）：
//! ①「过滤是即时的（边打边筛）」的**打点账**——每次过滤的耗时入环形
//!   账，P95 估算可查（<100ms 不是声明，是账上可复核的分布）；
//! ②「跨桌标注与跳转」的**跳转语义**——命中窗口的所在桌标注 +
//!   跨桌跳转动作（跳过去必须把焦点一起带过去，两动作原子）；
//! ③「无结果时提示要不要开个新的」的**空态出路状态机**——空态提示
//!   一次一次给、接受建议的动作有明确落点（新窗 id），不许死路。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::tvsearch::{TvSearch, FILTER_BUDGET_MS};

// ---------------------------------------------------------------------------
// 过滤耗时打点账（P95 估算）
// ---------------------------------------------------------------------------

/// 耗时打点环（容量 32，覆盖一次会话的连续过滤）。
pub struct FilterBench {
    samples: [u64; 32],
    head: usize,
    len: usize,
}

impl FilterBench {
    pub fn new() -> FilterBench {
        FilterBench { samples: [0; 32], head: 0, len: 0 }
    }

    pub fn record(&mut self, elapsed_ms: u64) {
        self.samples[self.head] = elapsed_ms;
        self.head = (self.head + 1) % 32;
        if self.len < 32 {
            self.len += 1;
        }
    }

    /// 排序副本（小样本插入排序——32 元素栈上可承受）。
    fn sorted(&self) -> [u64; 32] {
        let mut out = [0u64; 32];
        out[..self.len].copy_from_slice(&self.samples[..self.len]);
        for i in 1..self.len {
            let key = out[i];
            let mut j = i;
            while j > 0 && out[j - 1] > key {
                out[j] = out[j - 1];
                j -= 1;
            }
            out[j] = key;
        }
        out
    }

    /// P95 估算（最近样本序位法：第 ceil(0.95*n) 个）。
    pub fn p95_ms(&self) -> u64 {
        if self.len == 0 {
            return 0;
        }
        let s = self.sorted();
        let idx = (self.len * 95 + 99) / 100; // ceil(0.95n)
        s[idx - 1]
    }

    /// 全样本都在预算内（<100ms 硬线）。
    pub fn all_within_budget(&self) -> bool {
        (0..self.len).all(|i| self.samples[i] < FILTER_BUDGET_MS)
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for FilterBench {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 跨桌跳转（标注 + 聚焦原子动作）
// ---------------------------------------------------------------------------

/// 跳转动作（跨桌结果的双动作原子：标注在哪桌 + 把焦点带过去）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskJump {
    pub win_id: u64,
    pub desk: usize,
    /// 跳转后是否伴随聚焦（语义合同：跳转必须带焦点，不许只翻页）。
    pub focus_follows: bool,
}

/// 从任务视图对某命中发起跳转。
pub fn desk_jump(search: &TvSearch, hit_index: usize) -> Option<DeskJump> {
    let id = search.focus_hit(hit_index)?;
    let desk = search.desk_of_hit(hit_index)?;
    Some(DeskJump { win_id: id, desk, focus_follows: true })
}

// ---------------------------------------------------------------------------
// 空态出路状态机
// ---------------------------------------------------------------------------

/// 空态出路状态机：无结果 → 提示一次；接受建议 → 产出新窗落点。
pub struct EmptyEscape {
    /// 当前空态是否已提示过（同一空态不重复打扰）。
    prompted: bool,
    /// 建议产出的新窗 id（接受建议的落点）。
    suggested_new: Option<u64>,
}

impl EmptyEscape {
    pub fn new() -> EmptyEscape {
        EmptyEscape { prompted: false, suggested_new: None }
    }

    /// 空态进入：给一次提示（重复进入同一空态不再刷屏）。
    pub fn on_empty(&mut self, guidance: Option<&'static str>) -> Option<&'static str> {
        if guidance.is_some() && !self.prompted {
            self.prompted = true;
            guidance
        } else {
            None
        }
    }

    /// 接受建议：产出新窗（落点 = 本桌新窗 id）。
    pub fn accept_new_window(&mut self, new_id: u64) -> u64 {
        self.suggested_new = Some(new_id);
        new_id
    }

    /// 状态复位（出空态即复位——下次空态重新享有一次提示）。
    pub fn reset(&mut self) {
        self.prompted = false;
    }

    pub fn suggested(&self) -> Option<u64> {
        self.suggested_new
    }
}

impl Default for EmptyEscape {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f563_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    let mut t = TvSearch::new();
    t.add_win(1, "报告 - 记事本", "notepad", 0);
    t.add_win(2, "报表 - 表格", "sheet", 1);
    t.add_win(3, "终端", "term", 2);

    // 1) 打点账：逐次过滤耗时入环，P95 与预算判定可查。
    let mut bench = FilterBench::new();
    for ms in [12u64, 40, 8, 61, 33] {
        let _ = t.filter("报", ms);
        bench.record(ms);
    }
    cs.add(
        "bench records and p95",
        bench.len() == 5 && bench.p95_ms() == 61 && bench.all_within_budget(),
        "",
    );

    // 2) 超预算一眼红：P95 出界的账骗不过。
    let mut bench2 = FilterBench::new();
    for ms in [12u64, 250] {
        bench2.record(ms);
    }
    cs.add("budget breach visible", !bench2.all_within_budget() && bench2.p95_ms() == 250, "");

    // 3) 跨桌跳转：命中跨桌窗口 → 标注所在桌 + 焦点随行（原子语义）。
    let _ = t.filter("报", 20);
    let jump = desk_jump(&t, 0);
    cs.add(
        "desk jump carries focus",
        jump == Some(DeskJump { win_id: 1, desk: 0, focus_follows: true }),
        "",
    );
    let jump2 = desk_jump(&t, 1);
    cs.add(
        "cross desk annotated",
        jump2.map(|j| j.desk == 1 && j.win_id == 2).unwrap_or(false),
        "",
    );

    // 4) 空态出路：无结果提示一次、重复空态不刷屏、接受建议有落点。
    let mut esc = EmptyEscape::new();
    let g1 = esc.on_empty(t.empty_guidance(0));
    let g2 = esc.on_empty(t.empty_guidance(0));
    let new_id = esc.accept_new_window(99);
    cs.add(
        "empty escape state machine",
        g1.is_some() && g2.is_none() && esc.suggested() == Some(99) && new_id == 99,
        "",
    );

    // 5) 出空态复位：下次空态重新享有提示。
    esc.reset();
    cs.add("escape resets on leave", esc.on_empty(t.empty_guidance(0)).is_some(), "");

    // 6) 非空态不给空态提示（提示只属于空态——语义不错位）。
    let _ = t.filter("报", 15);
    let mut esc2 = EmptyEscape::new();
    cs.add("non empty no guidance", t.empty_guidance(2).is_none() && esc2.on_empty(None).is_none(), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p95_catches_outliers() {
        let mut b = FilterBench::new();
        for _ in 0..18 {
            b.record(10);
        }
        b.record(250);
        b.record(250); // 20 样本：18×10 + 2×250 → P95 = 第 19 个 = 250
        assert_eq!(b.p95_ms(), 250);
        assert!(!b.all_within_budget()); // 出界样本必须被抓到
    }

    #[test]
    fn jump_on_empty_hits_none() {
        let mut t = TvSearch::new();
        t.add_win(1, "a", "b", 0);
        let _ = t.filter("zzz", 5);
        assert!(desk_jump(&t, 0).is_none());
    }
}
