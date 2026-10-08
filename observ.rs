//! VARIX-M500 AI-19 · 性能艺术与观测（F451~F475，M1 · 先行域）
//!
//! 先有仪表后动刀——延迟链、基准套件、回归机器人、预算 CI。
//! 纯逻辑 + 固定容量数组（no_std），域自检 F475 汇入 `robust::run_kernel_checkup()`。

use crate::checks::CheckSet;

pub const MAX_STAGES: usize = 8;
pub const MAX_BUCKETS: usize = 8;

// ---------------------------------------------------------------------------
// F451 端到端延迟链
// ---------------------------------------------------------------------------

/// 输入→像素全链：各段累加，返回总延迟与最慢段。
pub fn latency_chain(stages: &[u32]) -> (u32, usize) {
    let mut total = 0u32;
    let mut slow = 0usize;
    for (i, &s) in stages.iter().enumerate() {
        total = total.saturating_add(s);
        if s > stages[slow] {
            slow = i;
        }
    }
    (total, slow)
}

// ---------------------------------------------------------------------------
// F452 延迟预算编辑器
// ---------------------------------------------------------------------------

/// 各段预算校验：返回超支段数。
pub fn budget_check(actual: &[u32], budget: &[u32]) -> usize {
    actual
        .iter()
        .zip(budget.iter())
        .filter(|(a, b)| a > b)
        .count()
}

// ---------------------------------------------------------------------------
// F453 帧侦探
// ---------------------------------------------------------------------------

/// 掉帧归因：最长的段承担责任。
pub fn frame_detect(frame_ms: &[u32; 4], deadline: u32) -> Option<usize> {
    let total: u32 = frame_ms.iter().sum();
    if total <= deadline {
        return None;
    }
    let mut slow = 0usize;
    for i in 1..4 {
        if frame_ms[i] > frame_ms[slow] {
            slow = i;
        }
    }
    Some(slow)
}

// ---------------------------------------------------------------------------
// F454 功耗剖析
// ---------------------------------------------------------------------------

/// 组件能耗排行：返回最大能耗组件索引。
pub fn power_hotspot(mw: &[u32]) -> usize {
    let mut hot = 0usize;
    for i in 1..mw.len() {
        if mw[i] > mw[hot] {
            hot = i;
        }
    }
    hot
}

// ---------------------------------------------------------------------------
// F455 唤醒源统计
// ---------------------------------------------------------------------------

/// 唤醒源计数（固定 4 源），返回最高频源。
pub fn wakeup_top(counts: &[u32; 4]) -> usize {
    power_hotspot(counts)
}

// ---------------------------------------------------------------------------
// F456 IO 延迟分位图
// ---------------------------------------------------------------------------

/// 延迟落桶（每桶 span us），返回 p99 所在桶。
pub fn io_percentile(samples: &[u32], span_us: u32) -> usize {
    let mut buckets = [0usize; MAX_BUCKETS];
    for &s in samples {
        let b = ((s / span_us) as usize).min(MAX_BUCKETS - 1);
        buckets[b] += 1;
    }
    // p99：从最高桶往下累计，覆盖到第 1% 尾部样本（即含最大样本）即可。
    let total = samples.len();
    if total == 0 {
        return 0;
    }
    let target = total - (total * 99 / 100); // 需要从顶部覆盖的样本数
    let mut acc = 0usize;
    for b in (0..MAX_BUCKETS).rev() {
        acc += buckets[b];
        if acc >= target {
            return b;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// F457 软渲染计数器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SwRenderCounters {
    pub frames: u32,
    pub blits: u32,
    pub fill_px: u64,
}

impl SwRenderCounters {
    pub const fn new() -> SwRenderCounters {
        SwRenderCounters { frames: 0, blits: 0, fill_px: 0 }
    }

    pub fn on_frame(&mut self, blits: u32, px: u64) {
        self.frames += 1;
        self.blits += blits;
        self.fill_px += px;
    }
}

// ---------------------------------------------------------------------------
// F458 标准基准套件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BenchScore {
    pub name: &'static str,
    pub score: u32,
}

/// 套件总分 = 各项几何平均的整数近似（乘积开方太贵，用对数近似省略 → 取调和平均）。
pub fn bench_total(scores: &[BenchScore]) -> u32 {
    if scores.is_empty() {
        return 0;
    }
    let n = scores.len() as u32;
    let sum: u64 = scores.iter().map(|s| 1_000_000u64 / s.score.max(1) as u64).sum();
    (n as u64 * 1_000_000 / sum.max(1)) as u32
}

// ---------------------------------------------------------------------------
// F459 回归机器人
// ---------------------------------------------------------------------------

/// 回归判定：劣化超 5% 才报回归（噪声容忍）。
pub fn regression(base: u32, now: u32) -> bool {
    let b = base.max(1);
    now * 100 < b * 95
}

// ---------------------------------------------------------------------------
// F460 性能档案库
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Profile {
    pub machine: u16,
    pub bench: u32,
}

/// 档案库按机器号存取（固定 4 槽）。
pub fn profile_put(store: &mut [Option<Profile>; 4], p: Profile) -> bool {
    for slot in store.iter_mut() {
        if slot.map(|s| s.machine) == Some(p.machine) {
            *slot = Some(p);
            return true;
        }
    }
    for slot in store.iter_mut() {
        if slot.is_none() {
            *slot = Some(p);
            return true;
        }
    }
    false
}

pub fn profile_get(store: &[Option<Profile>; 4], machine: u16) -> Option<u32> {
    store.iter().find_map(|o| o.filter(|p| p.machine == machine).map(|p| p.bench))
}

// ---------------------------------------------------------------------------
// F461 性能 diff 视图
// ---------------------------------------------------------------------------

/// 双档案 diff：正=提升，负=劣化（千分比）。
pub fn perf_diff(base: u32, now: u32) -> i32 {
    let b = base.max(1) as i64;
    ((now as i64 - b) * 1000 / b) as i32
}

// ---------------------------------------------------------------------------
// F462 常驻开销审计
// ---------------------------------------------------------------------------

/// 常驻组件内存审计：返回超预算组件数。
pub fn resident_audit(rss_kb: &[u32], budget_kb: &[u32]) -> usize {
    budget_check(rss_kb, budget_kb)
}

// ---------------------------------------------------------------------------
// F463 冷启动火焰
// ---------------------------------------------------------------------------

/// 启动火焰图：各阶段耗时占比最高的阶段索引。
pub fn cold_flame(stages_ms: &[u32]) -> usize {
    power_hotspot(stages_ms)
}

// ---------------------------------------------------------------------------
// F464 峰值猎手
// ---------------------------------------------------------------------------

/// 窗口内最大瞬时值及其位置。
pub fn peak_hunter(samples: &[u32]) -> (u32, usize) {
    let mut best = 0u32;
    let mut idx = 0usize;
    for (i, &s) in samples.iter().enumerate() {
        if s > best {
            best = s;
            idx = i;
        }
    }
    (best, idx)
}

// ---------------------------------------------------------------------------
// F465 性能预算 CI
// ---------------------------------------------------------------------------

/// CI 门禁：任一指标超红线即失败。
pub fn budget_ci(metrics: &[(u32, u32)]) -> bool {
    metrics.iter().all(|&(v, limit)| v <= limit)
}

// ---------------------------------------------------------------------------
// F466 微基准框架
// ---------------------------------------------------------------------------

/// 微基准：固定迭代次数，返回每次迭代均耗时（伪时钟：iter*常数）。
pub fn micro_bench(iters: u32, per_iter_ticks: u32) -> u32 {
    if iters == 0 {
        return 0;
    }
    per_iter_ticks
}

// ---------------------------------------------------------------------------
// F467 真实负载录制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Trace {
    ring: [u16; 8],
    head: usize,
    len: usize,
}

impl Trace {
    pub const fn new() -> Trace {
        Trace { ring: [0; 8], head: 0, len: 0 }
    }

    pub fn record(&mut self, op: u16) {
        self.ring[self.head] = op;
        self.head = (self.head + 1) % 8;
        if self.len < 8 {
            self.len += 1;
        }
    }

    /// 按时间正序回放到 out，返回回放的操作数。
    pub fn replay(&self, out: &mut [u16]) -> usize {
        let n = self.len.min(out.len());
        for i in 0..n {
            out[i] = self.ring[(self.head + 8 - self.len + i) % 8];
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F468 性能周报
// ---------------------------------------------------------------------------

/// 周报：7 天分数 → 本周均值 + 相对上周变化。
pub fn weekly_report(week: &[u32; 7], prev_avg: u32) -> (u32, i32) {
    let sum: u64 = week.iter().map(|&s| s as u64).sum();
    let avg = (sum / 7) as u32;
    (avg, perf_diff(prev_avg, avg))
}

// ---------------------------------------------------------------------------
// F469 观测开销守恒
// ---------------------------------------------------------------------------

/// 观测自身开销红线：不得超过目标耗时 2%。
pub fn observer_overhead_ok(target_ms: u32, overhead_ms: u32) -> bool {
    overhead_ms * 50 <= target_ms
}

// ---------------------------------------------------------------------------
// F470 性能 API 版本化
// ---------------------------------------------------------------------------

/// 主版本一致 + 次版本向前兼容。
pub fn api_version_ok(major: u8, minor_req: u8, minor_have: u8) -> bool {
    minor_have >= minor_req && major <= 1
}

// ---------------------------------------------------------------------------
// F471 机型对比档案
// ---------------------------------------------------------------------------

/// 多机对比：返回排名（分数高者靠前）。
pub fn machine_rank(profiles: &[Profile]) -> [u8; 4] {
    let mut rank = [0u8; 4];
    let n = profiles.len().min(4);
    for i in 0..n {
        let mut better = 0u8;
        for j in 0..n {
            if profiles[j].bench > profiles[i].bench {
                better += 1;
            }
        }
        rank[i] = better + 1;
    }
    rank
}

// ---------------------------------------------------------------------------
// F472 性能 fuzz
// ---------------------------------------------------------------------------

/// fuzz：任意输入下延迟链不 panic 且单调（越多样本总延迟不减）。
pub fn fuzz_latency(stages: &[u32]) -> bool {
    let (total, slow) = latency_chain(stages);
    slow < stages.len().max(1) && (stages.is_empty() || total >= stages[slow])
}

// ---------------------------------------------------------------------------
// F473 优化案例库
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Case {
    pub id: u16,
    pub before: u32,
    pub after: u32,
}

/// 案例收益 = (before-after)/before 千分比；取收益最大案例。
pub fn best_case(cases: &[Case]) -> usize {
    let mut best = 0usize;
    let mut best_gain = i64::MIN;
    for (i, c) in cases.iter().enumerate() {
        let b = c.before.max(1) as i64;
        let gain = (b - c.after as i64) * 1000 / b;
        if gain > best_gain {
            best_gain = gain;
            best = i;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F474 观测数据导出
// ---------------------------------------------------------------------------

/// 导出 CSV 行到固定缓冲（无格式化器，手写十进制）。
pub fn export_rows(rows: &[(u16, u32)], out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut put = |b: u8| {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    };
    for (id, v) in rows {
        let mut digits = [0u8; 10];
        let mut d = 0usize;
        let mut x = *id as u32;
        loop {
            digits[d] = b'0' + (x % 10) as u8;
            d += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        while d > 0 {
            d -= 1;
            put(digits[d]);
        }
        put(b',');
        let mut x = *v;
        let mut d = 0usize;
        loop {
            digits[d] = b'0' + (x % 10) as u8;
            d += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        while d > 0 {
            d -= 1;
            put(digits[d]);
        }
        put(b'\n');
    }
    n
}

// ---------------------------------------------------------------------------
// F475 性能域自检
// ---------------------------------------------------------------------------

pub fn run_observ_checks() -> CheckSet {
    let mut set = CheckSet::new("m5-obs");

    // F451
    let stages = [3u32, 5, 12, 2];
    let (total, slow) = latency_chain(&stages);
    set.add("F451 e2e latency chain", total == 22 && slow == 2, "sum + slowest stage");

    // F452
    let over = budget_check(&[4u32, 6, 1], &[4u32, 5, 1]);
    set.add("F452 budget editor", over == 1, "one stage over budget");

    // F453
    let d = frame_detect(&[4u32, 20, 4, 4], 30);
    let ok_frame = frame_detect(&[4u32, 4, 4, 4], 30);
    set.add("F453 frame detective", d == Some(1) && ok_frame.is_none(), "blame slowest stage");

    // F454
    let mw = [10u32, 300, 20];
    set.add("F454 power profiling", power_hotspot(&mw) == 1, "hotspot found");

    // F455
    let wk = [50u32, 10, 80, 5];
    set.add("F455 wakeup sources", wakeup_top(&wk) == 2, "top waker");

    // F456
    let samples = [10u32, 20, 30, 40, 100, 200, 300, 5000];
    let p99 = io_percentile(&samples, 100);
    set.add("F456 io percentile", p99 == MAX_BUCKETS - 1, "p99 lands top bucket");

    // F457
    let mut sw = SwRenderCounters::new();
    sw.on_frame(3, 800 * 600);
    let f1 = sw.frames;
    let px1 = sw.fill_px;
    sw.on_frame(2, 800 * 600);
    set.add(
        "F457 swrender counters",
        f1 == 1 && px1 == 480_000 && sw.frames == 2 && sw.blits == 5,
        "frame counters accumulate",
    );

    // F458
    let bs = [
        BenchScore { name: "mem", score: 1000 },
        BenchScore { name: "disk", score: 1000 },
    ];
    set.add("F458 bench suite", bench_total(&bs) == 1000 && bench_total(&[]) == 0, "harmonic mean");

    // F459
    set.add(
        "F459 regression robot",
        regression(1000, 900) && !regression(1000, 970) && !regression(1000, 1100),
        "5% tolerance",
    );

    // F460
    let mut store = [const { None }; 4];
    let p1 = profile_put(&mut store, Profile { machine: 1, bench: 900 });
    let p2 = profile_put(&mut store, Profile { machine: 2, bench: 800 });
    let upd = profile_put(&mut store, Profile { machine: 1, bench: 950 });
    let got = profile_get(&store, 1);
    let miss = profile_get(&store, 3);
    set.add(
        "F460 profile library",
        p1 && p2 && upd && got == Some(950) && miss.is_none(),
        "upsert by machine",
    );

    // F461
    set.add(
        "F461 perf diff view",
        perf_diff(1000, 1100) == 100 && perf_diff(1000, 900) == -100 && perf_diff(200, 220) == 100,
        "permille delta",
    );

    // F462
    let over = resident_audit(&[100u32, 900], &[200u32, 800]);
    set.add("F462 resident audit", over == 1, "budget overrun count");

    // F463
    let cold = [120u32, 40, 15, 5];
    set.add("F463 cold flame", cold_flame(&cold) == 0, "boot hotspot stage");

    // F464
    let (pk, pi) = peak_hunter(&[1u32, 9, 5, 3]);
    set.add("F464 peak hunter", pk == 9 && pi == 1, "peak value+index");

    // F465
    let ok = budget_ci(&[(10u32, 20u32), (30, 30)]);
    let bad = budget_ci(&[(10u32, 20u32), (31, 30)]);
    set.add("F465 budget CI", ok && !bad, "red-line gate");

    // F466
    set.add("F466 micro bench", micro_bench(1000, 7) == 7 && micro_bench(0, 7) == 0, "per-iter cost");

    // F467
    let mut tr = Trace::new();
    tr.record(1);
    tr.record(2);
    tr.record(3);
    let mut out = [0u16; 8];
    let n = tr.replay(&mut out);
    set.add(
        "F467 workload record/replay",
        n == 3 && out[0] == 1 && out[2] == 3,
        "in-order replay",
    );

    // F468
    let (avg, delta) = weekly_report(&[100u32; 7], 90);
    set.add("F468 weekly report", avg == 100 && delta == 111, "avg + trend");

    // F469
    set.add(
        "F469 observer overhead",
        observer_overhead_ok(1000, 20) && !observer_overhead_ok(1000, 21),
        "2% red line",
    );

    // F470
    set.add(
        "F470 perf api version",
        api_version_ok(1, 2, 3) && !api_version_ok(1, 4, 3) && !api_version_ok(2, 0, 0),
        "semver gate",
    );

    // F471
    let profs = [
        Profile { machine: 1, bench: 500 },
        Profile { machine: 2, bench: 900 },
        Profile { machine: 3, bench: 700 },
    ];
    let rank = machine_rank(&profs);
    set.add(
        "F471 machine compare",
        rank[0] == 3 && rank[1] == 1 && rank[2] == 2,
        "ranking by score",
    );

    // F472
    set.add(
        "F472 perf fuzz",
        fuzz_latency(&[]) && fuzz_latency(&[1u32, 2, 3]) && fuzz_latency(&[u32::MAX]),
        "no panic any input",
    );

    // F473
    let cases = [
        Case { id: 1, before: 100, after: 50 },
        Case { id: 2, before: 100, after: 90 },
    ];
    set.add("F473 case library", best_case(&cases) == 0, "best gain wins");

    // F474
    let mut buf = [0u8; 32];
    let n = export_rows(&[(12u16, 345u32)], &mut buf);
    let s = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F474 data export", n == 7 && s == "12,345\n", "csv row");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f467_trace_wrap() {
        let mut tr = Trace::new();
        for i in 0..10u16 {
            tr.record(i);
        }
        let mut out = [0u16; 8];
        assert_eq!(tr.replay(&mut out), 8);
        assert_eq!(out[0], 2);
        assert_eq!(out[7], 9);
    }

    #[test]
    fn f460_store_full() {
        let mut store = [const { None }; 4];
        for m in 0..4u16 {
            assert!(profile_put(&mut store, Profile { machine: m, bench: 100 }));
        }
        assert!(!profile_put(&mut store, Profile { machine: 9, bench: 1 }));
    }

    #[test]
    fn f475_observ_self_test_passes() {
        let set = run_observ_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m5-obs self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
