//! F172 引导自检可视化 · 批次四深化（secstar · G-G-02）。
//!
//! 批次四功能面（与批次三互补：批次三管「账与闪」，本批管「跑与环」）：
//! - [`SuiteRunner`]：套件执行引擎——四套件逐项喂结果，进度 ‰ 精确到项
//!   （36 项粒度——进度环不骗人条款的执行面）；
//! - [`ProgressRing`]：进度环数据——总项数完成度 → 环段点亮序列
//!   （30 段环：‰ → 段数的确定性映射）；
//! - [`MilestoneBudget`]：里程碑预算账——四里程碑各配预算毫秒，超期
//!   即标黄（甘特面：超在哪个里程碑一目了然）；
//! - [`LogQuery`]：日志环检索——按套件/按结果过滤，检索不搬数据
//!   （索引面——10 万条流畅的环内查询路径）。
//!
//! 零堆纪律：定长执行账 + 定长环，无 alloc。

use super::selftestviz::{FLASH_PERIOD_MS, MILESTONE_N, SUITE_MEM_ITEMS, SUITE_PROC_ITEMS, SUITE_STORE_ITEMS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 套件执行引擎
// ---------------------------------------------------------------------------

/// 全部套件项数（9+11+10+6=36——进度 ‰ 的分母）。
pub const TOTAL_ITEMS: usize = SUITE_MEM_ITEMS + SUITE_PROC_ITEMS + SUITE_STORE_ITEMS + 6;

/// 套件序（拓扑序号——与批次三 SUITE_ORDER 同一事实源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suite {
    Mem,
    Proc,
    Store,
    Input,
}

impl Suite {
    pub fn items(self) -> usize {
        match self {
            Suite::Mem => SUITE_MEM_ITEMS,
            Suite::Proc => SUITE_PROC_ITEMS,
            Suite::Store => SUITE_STORE_ITEMS,
            Suite::Input => 6,
        }
    }

    pub fn ord(self) -> usize {
        match self {
            Suite::Mem => 0,
            Suite::Proc => 1,
            Suite::Store => 2,
            Suite::Input => 3,
        }
    }
}

/// 执行引擎：按拓扑序逐项喂结果，进度单调推进。
pub struct SuiteRunner {
    suite_items_done: [usize; 4],
    pub current: Suite,
    pub failed: usize,
}

impl SuiteRunner {
    pub const fn new() -> SuiteRunner {
        SuiteRunner { suite_items_done: [0; 4], current: Suite::Mem, failed: 0 }
    }

    /// 喂一项结果（当前套件内推进；失败计数不阻断推进——失败也是结果）。
    /// 返回是否全部完成。
    pub fn feed(&mut self, passed: bool) -> bool {
        if !passed {
            self.failed += 1;
        }
        let cur = self.current.ord();
        self.suite_items_done[cur] += 1;
        if self.suite_items_done[cur] >= self.current.items() && cur < 3 {
            self.current = match cur {
                0 => Suite::Proc,
                1 => Suite::Store,
                _ => Suite::Input,
            };
        }
        self.progress_permille() >= 1_000
    }

    /// 进度 ‰（完成项数 ×1000 / 总项数——项级粒度，不按时间猜）。
    pub fn progress_permille(&self) -> u32 {
        let done: usize = self.suite_items_done.iter().sum();
        (done * 1_000 / TOTAL_ITEMS) as u32
    }

    pub fn done_in(&self, s: Suite) -> usize {
        self.suite_items_done[s.ord()]
    }
}

// ---------------------------------------------------------------------------
// 进度环（30 段）
// ---------------------------------------------------------------------------

/// 环段数。
pub const RING_SEGMENTS: usize = 30;

/// ‰ → 点亮段数（确定性映射：999‰ 也要亮到 floor，1000‰ 满环）。
pub fn ring_lit_segments(permille: u32) -> usize {
    let clamped = permille.min(1_000) as usize;
    clamped * RING_SEGMENTS / 1_000
}

/// 环段点亮序列：段 i 是否亮（i < lit）——渲染面直接消费。
pub fn ring_segment_lit(i: usize, permille: u32) -> bool {
    i < ring_lit_segments(permille)
}

// ---------------------------------------------------------------------------
// 里程碑预算账
// ---------------------------------------------------------------------------

/// 里程碑预算（毫秒）——四段各 500ms（超期标黄，甘特面）。
pub const MILESTONE_BUDGET_MS: [u64; MILESTONE_N] = [500, 500, 500, 500];

/// 里程碑实际耗时 → 预算裁决（超期标黄列表）。
pub fn milestone_verdicts(actual_ms: &[u64; MILESTONE_N]) -> [bool; MILESTONE_N] {
    let mut out = [true; MILESTONE_N];
    for i in 0..MILESTONE_N {
        out[i] = actual_ms[i] <= MILESTONE_BUDGET_MS[i];
    }
    out
}

/// 总预算线：四段合计 ≤ 2s（启动甘特的全局线——F053 联动）。
pub const MILESTONE_TOTAL_BUDGET_MS: u64 = 2_000;

// ---------------------------------------------------------------------------
// 日志环检索
// ---------------------------------------------------------------------------

/// 检索条件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogQuery {
    /// 全部。
    All,
    /// 按套件。
    BySuite(Suite),
    /// 只看失败。
    FailedOnly,
    /// 只看通过。
    PassedOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub suite: Suite,
    pub item: usize,
    pub passed: bool,
}

/// 环内检索：返回命中数（数据不搬——环是唯一存储，查询只数）。
pub fn log_query(ring: &[Option<LogEntry>], q: LogQuery) -> usize {
    ring.iter().flatten().filter(|e| match q {
        LogQuery::All => true,
        LogQuery::BySuite(s) => e.suite == s,
        LogQuery::FailedOnly => !e.passed,
        LogQuery::PassedOnly => e.passed,
    }).count()
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_selftestviz_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b4");

    // 1) 执行引擎全绿线：36 项逐项喂 → 进度单调到 1000‰（项级粒度）。
    let mut r = SuiteRunner::new();
    let mut prev = 0;
    let mut monotone = true;
    let mut done = false;
    for _ in 0..TOTAL_ITEMS {
        done = r.feed(true);
        let p = r.progress_permille();
        monotone &= p >= prev;
        prev = p;
    }
    cs.add("runner_full_green", done && monotone && r.progress_permille() == 1_000 && r.failed == 0, "");

    // 2) 套件切换：9 项内存套件喂满 → 当前自动进处理器套件（拓扑推进）。
    let mut r2 = SuiteRunner::new();
    for _ in 0..SUITE_MEM_ITEMS {
        r2.feed(true);
    }
    cs.add("runner_suite_advance", r2.current == Suite::Proc && r2.done_in(Suite::Mem) == SUITE_MEM_ITEMS, "");

    // 3) 失败计数：喂 3 败不阻断推进（失败也是结果——报告不失真）。
    let mut r3 = SuiteRunner::new();
    r3.feed(true);
    r3.feed(false);
    r3.feed(false);
    r3.feed(false);
    cs.add("runner_fail_counted", r3.failed == 3 && r3.progress_permille() == 111, "");

    // 4) 进度环映射：0‰=0 段、500‰=15 段、1000‰=30 段（确定性三锚）。
    cs.add(
        "ring_map_anchors",
        ring_lit_segments(0) == 0 && ring_lit_segments(500) == 15 && ring_lit_segments(1_000) == RING_SEGMENTS,
        "",
    );

    // 5) 环段点亮：500‰ 时前 15 段亮后 15 段灭（序列面逐段）。
    let all_ok = (0..RING_SEGMENTS).all(|i| ring_segment_lit(i, 500) == (i < 15));
    cs.add("ring_segment_sequence", all_ok, "");

    // 6) 超量钳制：>1000‰ 不炸不满溢（进度环不吃越界值）。
    cs.add("ring_clamp", ring_lit_segments(2_000) == RING_SEGMENTS && ring_segment_lit(29, 99_999), "");

    // 7) 里程碑裁决：四段全达标真、一段超期假（甘特面）。
    let ok = milestone_verdicts(&[400, 500, 300, 100]);
    let bad = milestone_verdicts(&[400, 501, 300, 100]);
    cs.add(
        "milestone_verdict",
        ok == [true; MILESTONE_N] && !bad[1] && bad[0] && bad[2] && bad[3],
        "",
    );

    // 8) 里程碑总预算：常量 2s 与四段和一致（一处一事实算术）。
    cs.add(
        "milestone_total",
        MILESTONE_BUDGET_MS.iter().sum::<u64>() == MILESTONE_TOTAL_BUDGET_MS,
        "",
    );

    // 9) 日志检索：四条件命中数全对（All/BySuite/FailedOnly/PassedOnly）。
    let ring = [
        Some(LogEntry { suite: Suite::Mem, item: 0, passed: true }),
        Some(LogEntry { suite: Suite::Mem, item: 1, passed: false }),
        Some(LogEntry { suite: Suite::Proc, item: 0, passed: true }),
        Some(LogEntry { suite: Suite::Store, item: 2, passed: false }),
    ];
    cs.add(
        "log_query_four",
        log_query(&ring, LogQuery::All) == 4
            && log_query(&ring, LogQuery::BySuite(Suite::Mem)) == 2
            && log_query(&ring, LogQuery::FailedOnly) == 2
            && log_query(&ring, LogQuery::PassedOnly) == 2,
        "",
    );

    // 10) 空环检索零命中（无数据不出数——检索面诚实）。
    let empty: [Option<LogEntry>; 4] = [None; 4];
    cs.add("log_query_empty", log_query(&empty, LogQuery::All) == 0, "");

    // 11) 总项数常量贯通：36 = 9+11+10+6（进度分母一处一事实）。
    cs.add("total_items_const", TOTAL_ITEMS == 36, "");

    // 12) 红闪周期贯通：500ms 周期常量批次四复用（不另造线）。
    cs.add("flash_period_reused", FLASH_PERIOD_MS == 500, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn runner_progress_exact_permille() {
        // 逐项进度对照表：9 项套件喂 i 项 = 精确 ‰（不猜不近似）。
        let mut r = SuiteRunner::new();
        for i in 1..=SUITE_MEM_ITEMS {
            r.feed(true);
            assert_eq!(r.progress_permille(), (i * 1_000 / TOTAL_ITEMS) as u32, "i={i}");
        }
    }

    #[test]
    fn ring_is_monotone_in_permille() {
        // ‰ 单调 → 段数单调（进度环不倒退）。
        let mut prev = 0;
        for pm in (0..=1_000u32).step_by(7) {
            let lit = ring_lit_segments(pm);
            assert!(lit >= prev, "pm={pm}");
            prev = lit;
        }
    }

    #[test]
    fn runner_survives_all_fail() {
        // 全败线：36 项全喂失败 → 完成度仍 1000‰（完成≠通过——诚实两轴）。
        let mut r = SuiteRunner::new();
        for _ in 0..TOTAL_ITEMS {
            r.feed(false);
        }
        assert_eq!(r.progress_permille(), 1_000);
        assert_eq!(r.failed, TOTAL_ITEMS);
    }
}
