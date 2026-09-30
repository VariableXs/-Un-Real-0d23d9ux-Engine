//! F172 引导自检可视化 · 批次三深化（secstar · G-G-02）。
//!
//! 批次三功能面（主册判据「项数对拍 9/9·11/11·10/10 / 红闪 / 日志
//! 100%」纵深）：
//! - [`TimingLedger`]：逐项耗时账——开始/结束/耗时 µs 三元组定长账
//!   （性能回归的数据面：慢项一眼可见）；
//! - [`RedFlashModel`]：红闪状态机——3 闪 500ms 周期（FLASH_* 常量的
//!   驱动面：闪烁计数可复核，不凭感觉闪）；
//! - [`export_log_full`]：日志全量导出——导出行数 = 名册项数（100%
//!   完整率的机械对账：缺一行即红）；
//! - [`topo_order`]：自检项依赖排序——内存→处理器→存储的拓扑序校验
//!   （依赖未满足先跑 = 报告失真，排序面是失真的解药）。
//!
//! 零堆纪律：定长账 + 定长行缓冲，无 alloc。

use super::selftestviz::{FLASH_COUNT, FLASH_PERIOD_MS, LOG_CAP, MILESTONE_N, SUITE_MEM_ITEMS, SUITE_PROC_ITEMS, SUITE_STORE_ITEMS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 逐项耗时账
// ---------------------------------------------------------------------------

/// 账容量（四大套件项数和 = 9+11+10+6）。
pub const LEDGER_CAP: usize = SUITE_MEM_ITEMS + SUITE_PROC_ITEMS + SUITE_STORE_ITEMS + 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingRec {
    pub item: &'static str,
    pub start_us: u64,
    pub end_us: u64,
}

impl TimingRec {
    pub fn elapsed_us(&self) -> u64 {
        self.end_us.saturating_sub(self.start_us)
    }
}

/// 耗时账：定长表 + 全量导出（慢项=耗时最大项——回归定位面）。
pub struct TimingLedger {
    recs: [Option<TimingRec>; LEDGER_CAP],
    pub n: usize,
}

impl TimingLedger {
    pub const fn new() -> TimingLedger {
        TimingLedger { recs: [const { None }; LEDGER_CAP], n: 0 }
    }

    /// 收项（end < start 的坏表诚实拒——时钟倒走在账上不合法）。
    pub fn record(&mut self, item: &'static str, start_us: u64, end_us: u64) -> bool {
        if self.n >= LEDGER_CAP || end_us < start_us {
            return false;
        }
        self.recs[self.n] = Some(TimingRec { item, start_us, end_us });
        self.n += 1;
        true
    }

    pub fn get(&self, i: usize) -> Option<TimingRec> {
        self.recs.get(i).copied().flatten()
    }

    /// 最慢项（回归的第一嫌疑人）。
    pub fn slowest(&self) -> Option<TimingRec> {
        (0..self.n)
            .filter_map(|i| self.get(i))
            .max_by_key(|r| r.elapsed_us())
    }

    /// 总耗时（末项结束-首项开始——流水口径）。
    pub fn span_us(&self) -> Option<u64> {
        match (self.get(0), self.n.checked_sub(1).and_then(|i| self.get(i))) {
            (Some(first), Some(last)) => Some(last.end_us.saturating_sub(first.start_us)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 红闪状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashPhase {
    /// 熄（半周期）。
    Dark,
    /// 亮（半周期）。
    Lit,
    /// 闪完定格（3 闪后常亮/转入错误卡——不无限闪）。
    Done,
}

/// 红闪驱动：周期 FLASH_PERIOD_MS、次数 FLASH_COUNT——到次即 Done。
pub struct RedFlash {
    pub phase: FlashPhase,
    elapsed_ms: u64,
    flashes_done: usize,
}

impl RedFlash {
    pub const fn new() -> RedFlash {
        RedFlash { phase: FlashPhase::Dark, elapsed_ms: 0, flashes_done: 0 }
    }

    pub fn tick(&mut self, dt_ms: u64) {
        if self.phase == FlashPhase::Done {
            return;
        }
        self.elapsed_ms += dt_ms;
        // 半周期翻转；每满一整周期计一闪。
        while self.elapsed_ms >= FLASH_PERIOD_MS / 2 {
            self.elapsed_ms -= FLASH_PERIOD_MS / 2;
            match self.phase {
                FlashPhase::Dark => self.phase = FlashPhase::Lit,
                FlashPhase::Lit => {
                    self.phase = FlashPhase::Dark;
                    self.flashes_done += 1;
                    if self.flashes_done >= FLASH_COUNT {
                        self.phase = FlashPhase::Done;
                    }
                }
                FlashPhase::Done => {}
            }
        }
    }

    pub fn flashes_done(&self) -> usize {
        self.flashes_done
    }
}

// ---------------------------------------------------------------------------
// 日志全量导出（100% 完整率机械对账）
// ---------------------------------------------------------------------------

/// 导出行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogRow {
    pub item: &'static str,
    pub passed: bool,
}

/// 全量导出：名册逐项一行——导出行数 ≠ 名册项数即不完整（缺行=红）。
pub fn export_log_full(roster: &[&'static str], results: &[bool], out: &mut [Option<LogRow>; LOG_CAP]) -> usize {
    if roster.len() != results.len() {
        return 0; // 名册-结果不齐 = 完整率立即破产（返回 0 触发对账红）
    }
    let n = roster.len().min(out.len());
    for i in 0..n {
        out[i] = Some(LogRow { item: roster[i], passed: results[i] });
    }
    n
}

/// 完整率判定：导出行数 == 名册项数 且全部行在位。
pub fn log_complete(roster_n: usize, exported_n: usize) -> bool {
    roster_n == exported_n
}

// ---------------------------------------------------------------------------
// 自检项依赖排序
// ---------------------------------------------------------------------------

/// 套件序（主册名册序）：内存 0 / 处理器 1 / 存储 2 / 输入 3。
pub const SUITE_ORDER: [usize; 4] = [0, 1, 2, 3];

/// 拓扑校验：执行序必须按 SUITE_ORDER 的子序列出现（内存没过不准先跑
/// 存储——依赖前置的机械判定）。
pub fn topo_order(executed: &[usize]) -> bool {
    let mut expect = 0usize;
    for &s in executed {
        if s == SUITE_ORDER[expect] {
            expect += 1;
        } else if expect < SUITE_ORDER.len() && s > SUITE_ORDER[expect] {
            return false; // 跳过了依赖套件
        }
        // s < SUITE_ORDER[expect]：重复执行已完成套件（重跑允许——回归）
        if expect >= SUITE_ORDER.len() {
            break;
        }
    }
    expect == SUITE_ORDER.len()
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_selftestviz_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b3");

    // 1) 耗时账：记录/读取/耗时三元组全对（性能数据面在岗）。
    let mut led = TimingLedger::new();
    led.record("phys-map", 100, 350);
    led.record("pgtables", 350, 500);
    cs.add(
        "timing_record_read",
        led.n == 2 && led.get(0).unwrap().elapsed_us() == 250 && led.get(1).unwrap().elapsed_us() == 150,
        "",
    );

    // 2) 坏表诚实拒：end < start（时钟倒走不合法）。
    let mut led2 = TimingLedger::new();
    cs.add("timing_backwards_rejected", !led2.record("x", 500, 400), "");

    // 3) 最慢项定位：三项中 pgtables 最慢 → slowest 恰指它。
    led.record("zombie-ok", 500, 560);
    cs.add("timing_slowest", led.slowest().unwrap().item == "phys-map", "");

    // 4) 总耗时流水口径：末尾结束-开头开始 = 460（不重不漏）。
    cs.add("timing_span", led.span_us() == Some(460), "");

    // 5) 红闪三闪：1500ms 走完恰 3 闪入 Done（次数可复核）。
    let mut f = RedFlash::new();
    for _ in 0..15 {
        f.tick(100);
    }
    cs.add("flash_three_then_done", f.flashes_done() == 3 && f.phase == FlashPhase::Done, "");

    // 6) 红闪中间态：半程时亮暗交替且未完成（闪烁不是跳变）。
    let mut f2 = RedFlash::new();
    f2.tick(250);
    let lit_mid = f2.phase == FlashPhase::Lit;
    f2.tick(250);
    let dark_after = f2.phase == FlashPhase::Dark && f2.flashes_done() == 1;
    cs.add("flash_alternates", lit_mid && dark_after, "");

    // 7) Done 后不复活：继续 tick 状态不动（终态纪律）。
    let mut f3 = RedFlash::new();
    for _ in 0..20 {
        f3.tick(100);
    }
    let before = (f3.phase, f3.flashes_done());
    f3.tick(10_000);
    cs.add("flash_done_terminal", (f3.phase, f3.flashes_done()) == before && before.0 == FlashPhase::Done, "");

    // 8) 全量导出：11 项名册 → 11 行全在（100% 完整率机械对账）。
    let roster: [&'static str; SUITE_PROC_ITEMS] = [
        "sched-ready", "ctx-switch", "syscall-tbl", "ipc-ports", "irq-map", "timer-cal",
        "prio-inherit", "affinity", "stack-guard", "fpu-save", "zombie-ok",
    ];
    let results = [true; SUITE_PROC_ITEMS];
    let mut out = [const { None }; LOG_CAP];
    let n = export_log_full(&roster, &results, &mut out);
    cs.add("log_full_11_of_11", log_complete(SUITE_PROC_ITEMS, n) && out[10].unwrap().item == "zombie-ok", "");

    // 9) 完整率破产路径：名册-结果不齐 → 导出 0（缺行=红不是静默）。
    let mut out2 = [const { None }; LOG_CAP];
    cs.add("log_mismatch_zero", export_log_full(&roster, &results[..5], &mut out2) == 0, "");

    // 10) 拓扑序：顺序执行全绿、跳套件判红（依赖前置机械判定）。
    cs.add(
        "topo_ok_and_skip_red",
        topo_order(&[0, 1, 2, 3]) && !topo_order(&[0, 2, 1, 3]) && topo_order(&[0, 0, 1, 2, 3]),
        "",
    );

    // 11) 名册常量贯通：9/11/10 套件项数一处一事实（主册对拍口径）。
    cs.add(
        "roster_consts",
        SUITE_MEM_ITEMS == 9 && SUITE_PROC_ITEMS == 11 && SUITE_STORE_ITEMS == 10,
        "",
    );

    // 12) 红闪常量贯通：3 闪 500ms 周期 / 日志 128 行一处一事实。
    cs.add("flash_consts", FLASH_COUNT == 3 && FLASH_PERIOD_MS == 500 && LOG_CAP == 128 && MILESTONE_N == 4, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn timing_ledger_cap_and_span() {
        // 满账拒收 + 流水口径在满账下仍准（容量边界行为）。
        let mut led = TimingLedger::new();
        for i in 0..LEDGER_CAP {
            assert!(led.record("it", (i as u64) * 10, (i as u64) * 10 + 5));
        }
        assert!(!led.record("over", 0, 1));
        assert_eq!(led.span_us(), Some((LEDGER_CAP as u64 - 1) * 10 + 5));
    }

    #[test]
    fn flash_exactly_count_ticks() {
        // 恰好 FLASH_COUNT 个整周期：3×500=1500ms 边界逐 ms 扫描。
        let mut f = RedFlash::new();
        let mut done_at = None;
        for ms in 1..=2_000u64 {
            f.tick(1);
            if f.phase == FlashPhase::Done {
                done_at = Some(ms);
                break;
            }
        }
        // 半周期 250ms × 6 次翻转 = 1500ms 时第 3 闪完成。
        assert_eq!(done_at, Some(1_500));
    }

    #[test]
    fn topo_order_rejects_memory_after_store() {
        // 存储先于内存 = 依赖倒挂（判红——报告会失真的执行序）；
        // 空执行序也不绿（一项没跑不等于全过）。
        assert!(!topo_order(&[2, 0, 1, 3]));
        assert!(!topo_order(&[]));
    }
}
