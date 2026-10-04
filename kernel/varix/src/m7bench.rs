//! m7bench — VARIX-M700 AI-26 基准与度量域 (F626~F650)
//!
//! 基准宪法、微基准军火库、宏基准、统计裁决、基线漂移、归因、
//! 环境指纹、数据格式、可视化数据源、回归门、fuzz 联动、统计分账、
//! 事件流、健康分、自描述导出、对标协议、压力剧本、回归走廊、
//! 文档生成、预算官、噪声抑制、考古档案、金样本、自检入口、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F626 — 基准宪法：预热/重复/统计
// ===========================================================================

pub const BENCH_WARMUP_RUNS: u32 = 3;
pub const BENCH_MIN_RUNS: u32 = 20;

pub fn bench_protocol_valid(warmup: u32, runs: u32) -> bool {
    warmup >= BENCH_WARMUP_RUNS && runs >= BENCH_MIN_RUNS
}

// ===========================================================================
// F627 — 微基准军火库
// ===========================================================================

pub const MICRO_BENCHES: [&str; 6] =
    ["syscall-null", "page-fault", "spinlock", "irq-ack", "ipc-roundtrip", "ctx-switch"];

pub fn micro_bench_known(name: &str) -> bool {
    MICRO_BENCHES.iter().any(|b| *b == name)
}

// ===========================================================================
// F628 — 宏基准剧本
// ===========================================================================

pub const MACRO_SCENARIOS: [&str; 5] =
    ["compile", "file-copy-1g", "multithread-scale", "boot-time", "io-random-4k"];

pub fn macro_scenario_known(name: &str) -> bool {
    MACRO_SCENARIOS.iter().any(|s| *s == name)
}

// ===========================================================================
// F629 — 统计裁决官：裁剪离群 + 中位数
// ===========================================================================

pub fn trimmed_median(samples: &mut [u32], n: usize) -> u32 {
    let n = n.min(samples.len());
    if n == 0 {
        return 0;
    }
    samples[..n].sort_unstable();
    let trim = n / 10; // 裁掉 10%
    let lo = trim;
    let hi = n - trim;
    if lo >= hi {
        return samples[n / 2];
    }
    let m = (lo + hi) / 2;
    samples[m]
}

/// 变异系数 permille。
pub fn variance_permille(samples: &[u32], n: usize) -> u16 {
    let n = n.min(samples.len());
    if n < 2 {
        return 0;
    }
    let sum: u64 = samples[..n].iter().map(|&s| s as u64).sum();
    let mean = sum / n as u64;
    if mean == 0 {
        return 0;
    }
    let var: u64 = samples[..n].iter().map(|&s| {
        let d = (s as i64 - mean as i64) as i64;
        (d * d) as u64
    }).sum::<u64>() / n as u64;
    // sqrt via Newton（整数）
    let std = int_sqrt(var);
    ((std * 1000) / mean).min(1000) as u16
}

pub fn int_sqrt(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

// ===========================================================================
// F630 — 基线漂移哨兵
// ===========================================================================

pub const DRIFT_ALERT_PERMILLE: u16 = 100; // 10%

pub fn baseline_drift(baseline: u32, current: u32) -> bool {
    if baseline == 0 {
        return false;
    }
    let delta = baseline.abs_diff(current) as u64 * 1000;
    delta > baseline as u64 * DRIFT_ALERT_PERMILLE as u64
}

// ===========================================================================
// F631 — 基准归因官
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ChangeRecord {
    pub commit: u32,
    pub touched_perf_path: bool,
}

pub fn attribute_change(score_delta_permille: u16, changes: &[ChangeRecord], n: usize) -> Option<u32> {
    if score_delta_permille < DRIFT_ALERT_PERMILLE {
        return None;
    }
    let n = n.min(changes.len());
    changes[..n].iter().find(|c| c.touched_perf_path).map(|c| c.commit)
}

// ===========================================================================
// F632 — 环境指纹
// ===========================================================================

#[derive(Clone, Copy)]
pub struct EnvFingerprint {
    pub machine: u8,    // 0=qemu-q35 1=bare
    pub vcpus: u8,
    pub freq_mhz: u32,
    pub governor: u8,   // 0=perf 1=schedutil
}

pub fn fingerprint_stable(a: &EnvFingerprint, b: &EnvFingerprint) -> bool {
    a.machine == b.machine && a.vcpus == b.vcpus && a.freq_mhz == b.freq_mhz && a.governor == b.governor
}

// ===========================================================================
// F633 — 基准数据格式
// ===========================================================================

pub const BENCH_FORMAT_VER: u8 = 2;

#[derive(Clone, Copy)]
pub struct BenchRecord {
    pub name: &'static str,
    pub median_us: u32,
    pub cv_permille: u16,
    pub runs: u32,
}

pub fn bench_record_valid(r: &BenchRecord) -> bool {
    !r.name.is_empty() && r.runs >= BENCH_MIN_RUNS && r.cv_permille <= 1000
}

// ===========================================================================
// F634 — 基准可视化数据源
// ===========================================================================

/// 趋势序列归一化到 0~1000。
pub fn trend_normalized(values: &[u32], n: usize) -> [u16; 8] {
    let n = n.min(8).min(values.len());
    let mut out = [0u16; 8];
    if n == 0 {
        return out;
    }
    let max = values[..n].iter().copied().max().unwrap_or(1).max(1);
    for i in 0..n {
        out[i] = ((values[i] as u64 * 1000) / max as u64) as u16;
    }
    out
}

// ===========================================================================
// F635 — 基准回归门
// ===========================================================================

pub const REGRESSION_GATE_PERMILLE: u16 = 50; // 劣化 5% 拦下

pub fn regression_gate(baseline: u32, candidate: u32) -> bool {
    // true = pass
    if baseline == 0 {
        return true;
    }
    (candidate as u64 * 1000) <= baseline as u64 * (1000 + REGRESSION_GATE_PERMILLE as u64)
}

// ===========================================================================
// F636 — 基准 fuzz 联动
// ===========================================================================

/// fuzz 发现回归的入口后自动把对应微基准加入运行集。
pub fn fuzz_to_bench_link(syscall_no: u32) -> Option<&'static str> {
    match syscall_no & 0x7 {
        0..=1 => Some("syscall-null"),
        2..=3 => Some("ipc-roundtrip"),
        4 => Some("page-fault"),
        _ => Some("spinlock"),
    }
}

// ===========================================================================
// F637 — 基准统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DomainBench {
    pub domain: u8,
    pub best_us: u32,
}

pub fn bench_top(entries: &mut [DomainBench], n: usize) -> usize {
    let n = n.min(entries.len());
    for i in 1..n {
        let key = entries[i];
        let mut j = i;
        while j > 0 && entries[j - 1].best_us > key.best_us {
            entries[j] = entries[j - 1];
            j -= 1;
        }
        entries[j] = key;
    }
    n
}

// ===========================================================================
// F638 — 基准事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum BenchEvent {
    RunStarted,
    RunFinished,
    Regression,
    Drift,
}

pub fn bench_event_loggable(e: BenchEvent) -> bool {
    matches!(e, BenchEvent::RunStarted | BenchEvent::RunFinished | BenchEvent::Regression | BenchEvent::Drift)
}

// ===========================================================================
// F639 — 基准健康分
// ===========================================================================

pub struct BenchHealth {
    pub runs_total: u32,
    pub infra_errors: u32,
    pub cv_violations: u32,
}

impl BenchHealth {
    pub fn grade(&self) -> u8 {
        if self.infra_errors == 0 && self.cv_violations == 0 {
            0
        } else if self.infra_errors * 100 <= self.runs_total {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F640 — 基准自描述导出
// ===========================================================================

pub struct BenchExport {
    pub format_ver: u8,
    pub env: EnvFingerprint,
    pub record_count: u32,
}

pub fn bench_export_valid(e: &BenchExport) -> bool {
    e.format_ver == BENCH_FORMAT_VER && e.record_count > 0 && e.env.vcpus > 0
}

// ===========================================================================
// F641 — 对标协议：可比指标
// ===========================================================================

pub const COMPARABLE_METRICS: [&str; 5] =
    ["boot-ms", "syscall-ns", "ctx-switch-ns", "page-fault-ns", "ipc-rt-ns"];

pub fn metric_comparable(name: &str) -> bool {
    COMPARABLE_METRICS.iter().any(|m| *m == name)
}

// ===========================================================================
// F642 — 基准压力剧本
// ===========================================================================

pub const BENCH_SOAK_MINUTES: u32 = 60;

pub fn bench_soak_pass(minutes: u32, anomalies: u32) -> bool {
    minutes >= BENCH_SOAK_MINUTES && anomalies == 0
}

// ===========================================================================
// F643 — 基准回归走廊
// ===========================================================================

pub const BENCH_CORRIDOR_CASES: [&str; 5] =
    ["syscall-ns", "boot-ms", "ctx-switch-ns", "ipc-rt-ns", "page-fault-ns"];

pub fn bench_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F644 — 基准文档生成器
// ===========================================================================

pub const BENCH_DOC_SECTIONS: [&str; 5] = ["methodology", "environment", "results", "history", "how-to-run"];

pub fn bench_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << BENCH_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F645 — 基准预算官
// ===========================================================================

pub const BENCH_TIME_BUDGET_S: u32 = 600;

pub fn bench_budget_ok(planned_s: u32) -> bool {
    planned_s <= BENCH_TIME_BUDGET_S
}

// ===========================================================================
// F646 — 噪声抑制谱
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NoisePolicy {
    pub pin_cpu: bool,
    pub drop_caches: bool,
    pub disable_irq_balance: bool,
    pub warmup: u32,
}

pub fn noise_score(p: &NoisePolicy) -> u8 {
    let mut s = 0;
    if p.pin_cpu { s += 1; }
    if p.drop_caches { s += 1; }
    if p.disable_irq_balance { s += 1; }
    if p.warmup >= BENCH_WARMUP_RUNS { s += 1; }
    s
}

// ===========================================================================
// F647 — 基准考古档案
// ===========================================================================

pub const ARCHIVE_MAX_ENTRIES: usize = 16;

#[derive(Clone, Copy)]
pub struct BenchArchiveEntry {
    pub version: u32,
    pub median_us: u32,
}

/// 检索某版本的最好成绩。
pub fn archive_lookup(entries: &[BenchArchiveEntry], n: usize, version: u32) -> Option<u32> {
    let n = n.min(ARCHIVE_MAX_ENTRIES).min(entries.len());
    entries[..n].iter().find(|e| e.version == version).map(|e| e.median_us)
}

// ===========================================================================
// F648 — 基准金样本：标准环境标准成绩
// ===========================================================================

pub fn golden_bench_match(median_us: u32, golden_us: u32, tolerance_permille: u16) -> bool {
    if golden_us == 0 {
        return false;
    }
    median_us.abs_diff(golden_us) as u64 * 1000 <= golden_us as u64 * tolerance_permille as u64
}

// ===========================================================================
// F649 — 基准自检入口
// ===========================================================================

pub const BENCH_SELFTESTS: [&str; 4] = ["timer-calibration", "stat-lib", "format-roundtrip", "gate-thresholds"];

pub fn bench_selftest_known(name: &str) -> bool {
    BENCH_SELFTESTS.iter().any(|s| *s == name)
}

// ===========================================================================
// F650 — 基准域年报
// ===========================================================================

pub struct BenchYearbook {
    pub benches_run: u32,
    pub regressions_caught: u32,
    pub baselines_updated: u32,
}

impl BenchYearbook {
    pub fn catch_rate_permille(&self) -> u16 {
        if self.benches_run == 0 {
            return 0;
        }
        ((self.regressions_caught as u64 * 1000) / self.benches_run as u64).min(1000) as u16
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7bench_checks() -> CheckSet {
    let mut set = CheckSet::new("m7bench");

    // F626 宪法
    set.add(
        "F626 protocol",
        bench_protocol_valid(3, 20) && !bench_protocol_valid(2, 20) && !bench_protocol_valid(3, 19),
        "warmup+runs",
    );

    // F627 微基准
    set.add(
        "F627 micro",
        micro_bench_known("syscall-null") && !micro_bench_known("quantum"),
        "6 benches",
    );

    // F628 宏基准
    set.add(
        "F628 macro",
        macro_scenario_known("compile") && macro_scenario_known("boot-time"),
        "5 scenarios",
    );

    // F629 统计裁决
    let mut s1 = [50u32, 10, 40, 20, 30, 999]; // 999 是离群
    let med = trimmed_median(&mut s1, 6);
    set.add(
        "F629 stats",
        med == 40 && variance_permille(&[100, 100, 100], 3) == 0,
        "trim + cv",
    );

    // F630 漂移
    set.add(
        "F630 drift",
        baseline_drift(1000, 1200) && !baseline_drift(1000, 1050) && !baseline_drift(0, 9999),
        "10% alert",
    );

    // F631 归因
    let changes = [
        ChangeRecord { commit: 11, touched_perf_path: false },
        ChangeRecord { commit: 22, touched_perf_path: true },
    ];
    set.add(
        "F631 attribute",
        attribute_change(150, &changes, 2) == Some(22) && attribute_change(50, &changes, 2).is_none(),
        "delta + perf path",
    );

    // F632 指纹
    let f1 = EnvFingerprint { machine: 0, vcpus: 4, freq_mhz: 2500, governor: 0 };
    let f2 = f1;
    let f3 = EnvFingerprint { machine: 0, vcpus: 2, freq_mhz: 2500, governor: 0 };
    set.add(
        "F632 fingerprint",
        fingerprint_stable(&f1, &f2) && !fingerprint_stable(&f1, &f3),
        "env equality",
    );

    // F633 数据格式
    let rec = BenchRecord { name: "ipc-rt", median_us: 800, cv_permille: 30, runs: 20 };
    set.add(
        "F633 format",
        bench_record_valid(&rec) && !bench_record_valid(&BenchRecord { name: "", median_us: 1, cv_permille: 1, runs: 1 })
            && BENCH_FORMAT_VER == 2,
        "record schema",
    );

    // F634 可视化
    let tr = trend_normalized(&[10, 20, 40, 80], 4);
    set.add(
        "F634 trend",
        tr[0] == 125 && tr[3] == 1000 && trend_normalized(&[], 0) == [0; 8],
        "normalized",
    );

    // F635 回归门
    set.add(
        "F635 gate",
        regression_gate(1000, 1050) && !regression_gate(1000, 1051) && regression_gate(0, 99),
        "5% gate",
    );

    // F636 fuzz 联动
    set.add(
        "F636 fuzz link",
        fuzz_to_bench_link(0) == Some("syscall-null") && fuzz_to_bench_link(4) == Some("page-fault"),
        "entry->bench",
    );

    // F637 分账
    let mut db = [
        DomainBench { domain: 1, best_us: 500 },
        DomainBench { domain: 2, best_us: 100 },
        DomainBench { domain: 3, best_us: 300 },
    ];
    bench_top(&mut db, 3);
    set.add("F637 top", db[0].domain == 2 && db[2].domain == 1, "fastest first");

    // F638 事件流
    set.add(
        "F638 events",
        bench_event_loggable(BenchEvent::Regression) && bench_event_loggable(BenchEvent::Drift),
        "4 events",
    );

    // F639 健康分
    let h = BenchHealth { runs_total: 100, infra_errors: 0, cv_violations: 0 };
    let hb = BenchHealth { runs_total: 10, infra_errors: 5, cv_violations: 0 };
    set.add("F639 health", h.grade() == 0 && hb.grade() == 2, "infra errors");

    // F640 导出
    let ex = BenchExport { format_ver: 2, env: f1, record_count: 12 };
    set.add(
        "F640 export",
        bench_export_valid(&ex) && !bench_export_valid(&BenchExport { format_ver: 1, env: f1, record_count: 12 }),
        "ver + count",
    );

    // F641 对标
    set.add(
        "F641 comparable",
        metric_comparable("boot-ms") && !metric_comparable("vibes-perf"),
        "5 metrics",
    );

    // F642 压力
    set.add(
        "F642 soak",
        bench_soak_pass(60, 0) && !bench_soak_pass(59, 0) && !bench_soak_pass(60, 1),
        "60min clean",
    );

    // F643 走廊
    set.add(
        "F643 corridor",
        bench_corridor_pass(&[true; 5]) && !bench_corridor_pass(&[true, true, true, true, false]),
        "5 cases",
    );

    // F644 文档
    set.add(
        "F644 docs",
        bench_doc_complete(0b1_1111) && !bench_doc_complete(0b0_1111),
        "5 sections",
    );

    // F645 预算
    set.add(
        "F645 budget",
        bench_budget_ok(600) && !bench_budget_ok(601) && BENCH_TIME_BUDGET_S == 600,
        "10min cap",
    );

    // F646 噪声抑制
    let np = NoisePolicy { pin_cpu: true, drop_caches: true, disable_irq_balance: true, warmup: 3 };
    set.add("F646 noise", noise_score(&np) == 4, "full policy");

    // F647 考古
    let arch = [
        BenchArchiveEntry { version: 5, median_us: 900 },
        BenchArchiveEntry { version: 6, median_us: 800 },
    ];
    set.add(
        "F647 archive",
        archive_lookup(&arch, 2, 6) == Some(800) && archive_lookup(&arch, 2, 9).is_none(),
        "version lookup",
    );

    // F648 金样本
    set.add(
        "F648 golden bench",
        golden_bench_match(1010, 1000, 20) && !golden_bench_match(1100, 1000, 20),
        "tolerance",
    );

    // F649 自检
    set.add(
        "F649 selftest",
        bench_selftest_known("timer-calibration") && !bench_selftest_known("crystal-ball"),
        "4 selftests",
    );

    // F650 年报
    let yb = BenchYearbook { benches_run: 1000, regressions_caught: 5, baselines_updated: 3 };
    set.add("F650 yearbook", yb.catch_rate_permille() == 5, "catch rate");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f629_int_sqrt() {
        assert_eq!(int_sqrt(0), 0);
        assert_eq!(int_sqrt(1), 1);
        assert_eq!(int_sqrt(1_000_000), 1000);
    }

    #[test]
    fn f629_median_even() {
        let mut s = [10u32, 20, 30, 40];
        assert_eq!(trimmed_median(&mut s, 4), 30);
    }

    #[test]
    fn f634_single_value() {
        let t = trend_normalized(&[500], 1);
        assert_eq!(t[0], 1000);
    }

    #[test]
    fn f647_archive_cap() {
        let entries = [BenchArchiveEntry { version: 1, median_us: 1 }; ARCHIVE_MAX_ENTRIES + 4];
        // n 截断到 ARCHIVE_MAX_ENTRIES
        assert!(archive_lookup(&entries, entries.len(), 1).is_some());
    }

    #[test]
    fn f650_domain_selfcheck_all_pass() {
        let set = run_m7bench_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
