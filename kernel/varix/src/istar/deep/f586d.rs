//! 深化层 · F586 文件夹大小列排序（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F586 节）：
//! ①「未算完的文件夹排位策略（估算值参与排序+『~』标注跟随）」的
//!   **标注跟随账**——标注层与排序账逐条对拍（标注不许留在已算完的
//!   条目上，也不许漏标仍在估算的）；
//! ②「算完后排序自动修正（一次轻微重排+提示）」的**重排提示账**——
//!   finalize 触发的重排提示只许一次；重复 finalize 同一条目不再
//!   变更也不再提示（提示不刷屏）；
//! ③「万目录性能」的**打点账**——排序耗时入环按峰值判预算（用户
//!   只记得最卡那次，均值会骗人）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::dirsize::{Entry, SizeSort, SortDir, SORT_BUDGET_MS};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 标注跟随账
// ---------------------------------------------------------------------------

/// 一条标注核对：(条目名, 排序账认为它是估算吗, 标注层认为它是估算吗)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationCheck {
    pub name_match: bool,
    pub ledger_approx: bool,
    pub shown_approx: bool,
}

impl AnnotationCheck {
    pub fn consistent(&self) -> bool {
        self.name_match && self.ledger_approx == self.shown_approx
    }
}

/// 标注跟随核对：标注层（渲染）与排序账（数据）逐条对拍。
pub fn annotations_follow(
    sort: &SizeSort,
    shown_approx: &[(&str, bool)],
) -> alloc::vec::Vec<AnnotationCheck> {
    sort.order()
        .iter()
        .map(|name| {
            let ledger_approx = sort
                .entries()
                .iter()
                .find(|e| e.name == *name)
                .map(|e| e.approximated())
                .unwrap_or(false);
            let shown = shown_approx
                .iter()
                .find(|(n, _)| *n == name.as_str())
                .map(|(_, a)| *a)
                .unwrap_or(false);
            AnnotationCheck {
                name_match: shown_approx.iter().any(|(n, _)| *n == name.as_str()),
                ledger_approx,
                shown_approx: shown,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 重排提示账
// ---------------------------------------------------------------------------

/// 重排提示状态机：finalize 有变更 → 提示一次；此后静默。
pub struct RearrangeNotice {
    fired: bool,
    pub message: &'static str,
}

impl RearrangeNotice {
    pub fn new() -> RearrangeNotice {
        RearrangeNotice { fired: false, message: "大小已算完，排序已修正" }
    }

    /// finalize 后调用：首次变更出提示，此后静默。
    pub fn on_rearranged(&mut self, changed: bool) -> bool {
        if changed && !self.fired {
            self.fired = true;
            return true;
        }
        false
    }

    pub fn fired(&self) -> bool {
        self.fired
    }
}

impl Default for RearrangeNotice {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 万目录打点账
// ---------------------------------------------------------------------------

/// 排序耗时打点（环 16，按峰值判预算）。
pub struct SortBench {
    samples: [u64; 16],
    head: usize,
    len: usize,
}

impl SortBench {
    pub fn new() -> SortBench {
        SortBench { samples: [0; 16], head: 0, len: 0 }
    }

    pub fn record(&mut self, ms: u64) {
        self.samples[self.head] = ms;
        self.head = (self.head + 1) % 16;
        if self.len < 16 {
            self.len += 1;
        }
    }

    /// 峰值（最差一次）。
    pub fn peak_ms(&self) -> u64 {
        (0..self.len).map(|i| self.samples[i]).max().unwrap_or(0)
    }

    /// 全样本在预算内（<200ms）。
    pub fn within_budget(&self) -> bool {
        self.peak_ms() < SORT_BUDGET_MS
    }
}

impl Default for SortBench {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f586_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 标注跟随：估算条目带 ~、算完条目不带（标注与账一致）。
    let mut s = SizeSort::new(alloc::vec![
        Entry { name: String::from("甲文件夹"), is_dir: true, exact: None, estimate: Some(3_000) },
        Entry { name: String::from("乙文件"), is_dir: false, exact: Some(5_000), estimate: None },
        Entry { name: String::from("丙文件夹"), is_dir: true, exact: None, estimate: Some(1_000) },
    ]);
    let _ = s.header_click(); // 降序
    let shown = [("乙文件", false), ("甲文件夹", true), ("丙文件夹", true)];
    let report = annotations_follow(&s, &shown);
    cs.add(
        "annotations follow ledger",
        report.len() == 3 && report.iter().all(|a| a.consistent()),
        "",
    );

    // 2) 标注错位立红：渲染把算完的条目标成估算 → 对账抓出。
    let wrong = [("乙文件", true), ("甲文件夹", true), ("丙文件夹", true)];
    let report2 = annotations_follow(&s, &wrong);
    cs.add(
        "misplaced annotation caught",
        report2.iter().any(|a| !a.consistent()),
        "",
    );

    // 3) 估算条目 finalize：变更出提示一次；重复 finalize 无变更无提示。
    let target = s
        .order()
        .iter()
        .find(|n| n.as_str() == "甲文件夹")
        .cloned()
        .unwrap_or_default();
    let mut notice = RearrangeNotice::new();
    let changed1 = s.finalize(&target, 9_000);
    let fired1 = notice.on_rearranged(changed1);
    let changed2 = s.finalize(&target, 9_000); // 已算完：不再变更
    let fired2 = notice.on_rearranged(changed2);
    cs.add(
        "rearrange notice once",
        changed1 && !changed2 && fired1 && !fired2,
        "",
    );

    // 4) 修正重排实效：切到降序（Asc→Desc）后算完的甲（9000）压过乙。
    let _ = s.header_click(); // Asc → Desc
    let order_after = s.order();
    let pos = |n: &str| order_after.iter().position(|x| x.as_str() == n).unwrap_or(usize::MAX);
    cs.add("finalize corrects order", pos("甲文件夹") < pos("乙文件"), "");

    // 5) 万目录打点：峰值口径的预算判定（最差一次也要 <200ms）。
    let mut bench = SortBench::new();
    for ms in [180u64, 190, 199] {
        bench.record(ms);
    }
    cs.add("peak within budget", bench.within_budget() && bench.peak_ms() == 199, "");

    // 6) 三态循环实判：None → Asc → Desc → None（既有语义不被深化破坏）。
    let mut s2 = SizeSort::new(alloc::vec![]);
    let d0 = s2.header_click();
    let d1 = s2.header_click();
    let d2 = s2.header_click();
    cs.add(
        "tri-state cycle real",
        d0 == SortDir::Asc && d1 == SortDir::Desc && d2 == SortDir::None,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_peak_zero_when_empty() {
        let b = SortBench::new();
        assert_eq!(b.peak_ms(), 0);
        assert!(b.within_budget());
    }

    #[test]
    fn approx_requires_estimate_without_exact() {
        let e = Entry { name: String::from("x"), is_dir: true, exact: None, estimate: Some(9) };
        assert!(e.approximated());
        let e2 = Entry { name: String::from("y"), is_dir: true, exact: Some(9), estimate: Some(8) };
        assert!(!e2.approximated());
    }
}
