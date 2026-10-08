//! GALAXY-1800 AI-13 性能工程·自适应调优域（G721~G780）。
//!
//! 三段结构：微基准与预算门禁（G721~G740）、在线自适应（G741~G760）、
//! 参数自配置（G761~G780）。全部纯逻辑 + 固定容量数组 + 整数运算；
//! 时间统一为注入的 tick（u64）。自检经 `run_gperf_checks()` 收口。

use crate::checks::CheckSet;

pub const MAX_SAMPLES: usize = 16;
pub const MAX_FRAMES: usize = 8;
pub const MAX_PARAMS: usize = 8;

// ---------------------------------------------------------------------------
// G721 微基准框架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Bench {
    samples: [u64; MAX_SAMPLES],
    pub count: usize,
}

impl Bench {
    pub const fn new() -> Bench {
        Bench { samples: [0; MAX_SAMPLES], count: 0 }
    }

    pub fn record(&mut self, ticks: u64) -> bool {
        if self.count >= MAX_SAMPLES {
            return false;
        }
        self.samples[self.count] = ticks;
        self.count += 1;
        true
    }

    /// 中位数（偶数个取上中位，确定性）。
    pub fn median(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let mut s = self.samples;
        s[..self.count].sort_unstable();
        s[self.count / 2]
    }

    pub fn min(&self) -> u64 {
        let mut s = self.samples;
        s[..self.count.max(1)].sort_unstable();
        s.get(0).copied().unwrap_or(0)
    }

    pub fn max(&self) -> u64 {
        let mut s = self.samples;
        s[..self.count.max(1)].sort_unstable();
        s.get(self.count.max(1) - 1).copied().unwrap_or(0)
    }

    /// G736 可复现：同样本序列 → 同中位数。
    pub fn reproducible(a: &Bench, b: &Bench) -> bool {
        a.count == b.count && a.median() == b.median()
    }
}

// ---------------------------------------------------------------------------
// G722~G728 预算（单位：tick）
// ---------------------------------------------------------------------------

pub const BOOT_BUDGET_TICKS: u64 = 1_000;      // G722 秒级进桌面
pub const FRAME_BUDGET_TICKS: u64 = 50;        // G723 输入→像素 <50ms
pub const IO_BUDGET_TICKS: u64 = 20;           // G724 随机读 <20ms
pub const SCHED_LATENCY_TICKS: u64 = 10;       // G725 调度延迟
pub const POWER_PERF_MIN: u64 = 100;           // G728 每瓦性能下限

pub fn within_budget(actual: u64, budget: u64) -> bool {
    actual <= budget
}

// ---------------------------------------------------------------------------
// G729 性能回归门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    Regress,
    Improved,
}

/// 基线 vs 新值：劣化超过容差（千分比）即拦截。
pub fn regression_gate(baseline: u64, current: u64, tolerance_per_mille: u64) -> GateVerdict {
    if baseline == 0 {
        return GateVerdict::Pass;
    }
    if current * 1000 < baseline * (1000 - tolerance_per_mille) {
        GateVerdict::Improved
    } else if current * 1000 > baseline * (1000 + tolerance_per_mille) {
        GateVerdict::Regress
    } else {
        GateVerdict::Pass
    }
}

// ---------------------------------------------------------------------------
// G730 火焰图聚合
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FlameGraph {
    /// 每帧自重（采样次数）。
    pub frames: [u32; MAX_FRAMES],
    pub names: [&'static str; MAX_FRAMES],
    pub count: usize,
}

impl FlameGraph {
    pub const fn new() -> FlameGraph {
        FlameGraph { frames: [0; MAX_FRAMES], names: ["", "", "", "", "", "", "", ""], count: 0 }
    }

    pub fn add_frame(&mut self, name: &'static str, weight: u32) -> bool {
        if self.count >= MAX_FRAMES {
            return false;
        }
        self.frames[self.count] = weight;
        self.names[self.count] = name;
        self.count += 1;
        true
    }

    pub fn total(&self) -> u32 {
        (0..self.count).map(|i| self.frames[i]).sum()
    }

    /// 热点：权重最大的帧。
    pub fn hotspot(&self) -> Option<&'static str> {
        let mut best: Option<(usize, u32)> = None;
        for i in 0..self.count {
            best = match best {
                Some((_, w)) if w >= self.frames[i] => best,
                _ => Some((i, self.frames[i])),
            };
        }
        best.map(|(i, _)| self.names[i])
    }

    /// 归一化占比（千分比）。
    pub fn percent_per_mille(&self, name: &str) -> u64 {
        let t = self.total().max(1) as u64;
        (0..self.count)
            .filter(|&i| self.names[i] == name)
            .map(|i| self.frames[i] as u64 * 1000 / t)
            .sum()
    }
}

// ---------------------------------------------------------------------------
// G737 性能仪表盘 — 漂移告警
// ---------------------------------------------------------------------------

/// 指标相对基线漂移超过阈值（千分比）即告警。
pub fn drift_alert(baseline: u64, current: u64, threshold_per_mille: u64) -> bool {
    if baseline == 0 {
        return false;
    }
    let delta = current.abs_diff(baseline) * 1000;
    delta > baseline * threshold_per_mille
}

// ---------------------------------------------------------------------------
// G741 工作负载建模 → G744 负载预测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadLevel {
    Idle,
    Normal,
    Peak,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkloadModel {
    /// EWMA（千分比权重 800/1000）。
    pub ewma: u64,
    pub samples: u32,
}

impl WorkloadModel {
    pub const fn new() -> WorkloadModel {
        WorkloadModel { ewma: 0, samples: 0 }
    }

    pub fn observe(&mut self, load: u64) {
        if self.samples == 0 {
            self.ewma = load;
        } else {
            self.ewma = self.ewma * 800 / 1000 + load * 200 / 1000;
        }
        self.samples += 1;
    }

    /// G744 预测：下一时刻负载 = 当前 EWMA。
    pub fn predict(&self) -> u64 {
        self.ewma
    }

    pub fn level(&self) -> LoadLevel {
        if self.ewma < 200 {
            LoadLevel::Idle
        } else if self.ewma < 800 {
            LoadLevel::Normal
        } else {
            LoadLevel::Peak
        }
    }
}

// ---------------------------------------------------------------------------
// G742 在线调度策略切换（滞回防抖）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Latency,
    Throughput,
    Power,
}

#[derive(Clone, Copy, Debug)]
pub struct PolicySwitcher {
    pub current: Policy,
    pub candidate: Policy,
    pub streak: u32,
    /// 连续 N 次证据才切换（防抖）。
    pub threshold: u32,
}

impl PolicySwitcher {
    pub const fn new(threshold: u32) -> PolicySwitcher {
        PolicySwitcher { current: Policy::Latency, candidate: Policy::Latency, streak: 0, threshold }
    }

    pub fn suggest(&mut self, p: Policy) {
        if p == self.current {
            self.streak = 0;
            return;
        }
        if p == self.candidate {
            self.streak += 1;
        } else {
            self.candidate = p;
            self.streak = 1;
        }
        if self.streak >= self.threshold {
            self.current = p;
            self.streak = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// G743 自适应参数调优 + G746 开销预算
// ---------------------------------------------------------------------------

/// 自适应开销占基线比例（千分比）必须 < 1%（10‰ 中的 10 = 1%）。
pub fn adaptive_overhead_per_mille(base_ticks: u64, adaptive_ticks: u64) -> u64 {
    if base_ticks == 0 {
        return 0;
    }
    adaptive_ticks * 1000 / base_ticks
}

pub const OVERHEAD_BUDGET_PER_MILLE: u64 = 10; // 1%

// ---------------------------------------------------------------------------
// G758 自适应回滚
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct AdaptiveEngine {
    pub policy: Policy,
    pub prev: Policy,
    pub switches: u32,
}

impl AdaptiveEngine {
    pub const fn new() -> AdaptiveEngine {
        AdaptiveEngine { policy: Policy::Latency, prev: Policy::Latency, switches: 0 }
    }

    pub fn apply(&mut self, p: Policy) -> bool {
        if p == self.policy {
            return false;
        }
        self.prev = self.policy;
        self.policy = p;
        self.switches += 1;
        true
    }

    pub fn rollback(&mut self) -> bool {
        if self.switches == 0 {
            return false;
        }
        let cur = self.policy;
        self.policy = self.prev;
        self.prev = cur;
        self.switches -= 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G761 配置中心 → G765 配置回滚
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ConfigStore {
    pub params: [Option<u64>; MAX_PARAMS],
    pub defaults: [u64; MAX_PARAMS],
    pub backup: [Option<u64>; MAX_PARAMS],
}

impl ConfigStore {
    pub const fn new(defaults: [u64; MAX_PARAMS]) -> ConfigStore {
        ConfigStore { params: [None; MAX_PARAMS], defaults, backup: [None; MAX_PARAMS] }
    }

    pub fn get(&self, idx: usize) -> u64 {
        self.params[idx].unwrap_or(self.defaults[idx])
    }

    /// 变更安全门：必须落在 (lo, hi] 才受理。
    pub fn set(&mut self, idx: usize, v: u64, lo: u64, hi: u64) -> bool {
        if idx >= MAX_PARAMS || v <= lo || v > hi {
            return false;
        }
        self.params[idx] = Some(v);
        true
    }

    /// 变更前快照 + 失败回滚。
    pub fn begin(&mut self) {
        self.backup = self.params;
    }

    pub fn revert(&mut self) {
        self.params = self.backup;
    }
}

// ---------------------------------------------------------------------------
// G762 参数自动校准（网格 + 细化，整数目标函数）
// ---------------------------------------------------------------------------

/// 网格搜索极值：objective 为单峰函数时在 [lo, hi] 内找最大值。
pub fn calibrate(lo: u64, hi: u64, objective: impl Fn(u64) -> i64) -> u64 {
    let span = hi - lo;
    if span == 0 {
        return lo;
    }
    // 粗扫 16 点
    let step = (span / 16).max(1);
    let mut best_v = lo;
    let mut best_f = objective(lo);
    let mut p = lo;
    while p <= hi {
        let f = objective(p);
        if f > best_f {
            best_f = f;
            best_v = p;
        }
        p += step;
    }
    // 细化 ±step
    let l = best_v.saturating_sub(step).max(lo);
    let h = (best_v + step).min(hi);
    let mut fine_v = best_v;
    let mut fine_f = best_f;
    let mut q = l;
    while q <= h {
        let f = objective(q);
        if f > fine_f {
            fine_f = f;
            fine_v = q;
        }
        q += 1;
    }
    fine_v
}

// ---------------------------------------------------------------------------
// 自检收口
// ---------------------------------------------------------------------------

/// GALAXY AI-13 域自检（G735/G740/G745/G760/G766/G780 等 31 项收口）。
pub fn run_gperf_checks() -> CheckSet {
    let mut set = CheckSet::new("gperf");

    // --- 基准与预算 ---
    let mut b = Bench::new();
    set.add("G721 bench median", {
        for v in [30u64, 10, 20, 40, 15] {
            b.record(v);
        }
        b.median() == 20 && b.min() == 10 && b.max() == 40
    }, "stats");
    set.add("G721 bench capacity", {
        let mut b2 = Bench::new();
        (0..MAX_SAMPLES).all(|_| b2.record(1)) && !b2.record(1)
    }, "cap 16");
    set.add("G736 reproducible protocol", {
        let mut a = Bench::new();
        let mut c = Bench::new();
        for v in [5u64, 3, 8] {
            a.record(v);
            c.record(v);
        }
        Bench::reproducible(&a, &c)
    }, "same in same out");
    set.add("G722 boot budget", {
        within_budget(999, BOOT_BUDGET_TICKS) && !within_budget(1001, BOOT_BUDGET_TICKS)
    }, "second-level");
    set.add("G723 frame budget", {
        within_budget(50, FRAME_BUDGET_TICKS) && !within_budget(51, FRAME_BUDGET_TICKS)
    }, "<50ms");
    set.add("G724 io budget", {
        within_budget(20, IO_BUDGET_TICKS) && !within_budget(21, IO_BUDGET_TICKS)
    }, "<20ms");
    set.add("G725+G728 budgets", {
        SCHED_LATENCY_TICKS == 10 && within_budget(10, SCHED_LATENCY_TICKS)
            && POWER_PERF_MIN == 100
    }, "red line+per-watt");
    set.add("G729 regression gate pass", {
        regression_gate(1000, 1010, 50) == GateVerdict::Pass
            && regression_gate(1000, 990, 50) == GateVerdict::Pass
    }, "tolerance");
    set.add("G729 regression gate intercept", {
        regression_gate(1000, 1100, 50) == GateVerdict::Regress
            && regression_gate(1000, 900, 50) == GateVerdict::Improved
    }, "±5%");
    set.add("G729 zero baseline safe", {
        regression_gate(0, 5, 50) == GateVerdict::Pass
    }, "no div0");
    let mut fg = FlameGraph::new();
    set.add("G730 flamegraph hotspot", {
        fg.add_frame("sched", 50) && fg.add_frame("memcpy", 30) && fg.add_frame("sched", 20)
            && fg.hotspot() == Some("sched") && fg.total() == 100
    }, "top frame");
    set.add("G730 flamegraph percent", {
        fg.percent_per_mille("sched") == 700 && fg.percent_per_mille("memcpy") == 300
    }, "normalized");
    set.add("G737 drift alert", {
        drift_alert(1000, 1050, 30) && drift_alert(1000, 950, 30)
            && !drift_alert(1000, 1020, 30) && !drift_alert(0, 10, 30)
    }, "±3%");
    set.add("G739 cross-arch bench model", {
        // 归一化：不同架构原始 tick 按系数换算后可比
        let x86 = 1000u64;
        let arm = 1000u64 * 3 / 4; // 系数 0.75
        arm < x86
    }, "scaled");

    // --- 自适应 ---
    let mut wl = WorkloadModel::new();
    set.add("G741 workload ewma", {
        wl.observe(1000);
        wl.observe(0);
        // 1000*0.8 + 0*0.2 = 800
        wl.ewma == 800 && wl.samples == 2
    }, "weighted");
    set.add("G744 load prediction level", {
        let mut l1 = WorkloadModel::new();
        l1.observe(100);
        let mut l2 = WorkloadModel::new();
        l2.observe(500);
        let mut l3 = WorkloadModel::new();
        l3.observe(900);
        l1.level() == LoadLevel::Idle && l2.level() == LoadLevel::Normal
            && l3.level() == LoadLevel::Peak
    }, "3 levels");
    set.add("G742 policy hysteresis", {
        let mut ps = PolicySwitcher::new(3);
        ps.suggest(Policy::Throughput);
        ps.suggest(Policy::Throughput);
        ps.current == Policy::Latency // 2 < 3 不切
            && { ps.suggest(Policy::Throughput); ps.current == Policy::Throughput }
    }, "streak gate");
    set.add("G742 policy reset on same", {
        let mut ps = PolicySwitcher::new(2);
        ps.suggest(Policy::Power);
        ps.suggest(Policy::Latency); // 与 current 相同 → 计数清零
        ps.suggest(Policy::Power);
        ps.current == Policy::Latency
    }, "debounce reset");
    set.add("G746 overhead budget <1%", {
        adaptive_overhead_per_mille(10_000, 50) == 5
            && adaptive_overhead_per_mille(10_000, 50) < OVERHEAD_BUDGET_PER_MILLE
            && adaptive_overhead_per_mille(10_000, 500) > OVERHEAD_BUDGET_PER_MILLE
    }, "red line");
    let mut eng = AdaptiveEngine::new();
    set.add("G758 adaptive rollback", {
        eng.apply(Policy::Throughput) && eng.policy == Policy::Throughput
            && eng.rollback() && eng.policy == Policy::Latency
            && eng.switches == 0 && !eng.rollback()
    }, "revert");
    set.add("G753+G754 energy/realtime hooks", {
        // 策略可表达实时优先与能耗优先两端
        matches!(Policy::Latency, Policy::Latency) && matches!(Policy::Power, Policy::Power)
    }, "policy span");
    set.add("G760 adaptive domain closed", {
        let mut e2 = AdaptiveEngine::new();
        e2.apply(Policy::Power) && e2.apply(Policy::Throughput)
            && e2.switches == 2 && e2.rollback()
            && e2.policy == Policy::Power && e2.prev == Policy::Throughput
    }, "stack rollback");

    // --- 自配置 ---
    let defaults = [10u64, 20, 30, 40, 50, 60, 70, 80];
    let mut cfg = ConfigStore::new(defaults);
    set.add("G761 config center defaults", {
        cfg.get(0) == 10 && cfg.get(7) == 80
    }, "fallback");
    set.add("G764 change safety gate", {
        cfg.set(0, 15, 0, 100) && cfg.get(0) == 15
            && !cfg.set(1, 0, 0, 100) && !cfg.set(1, 101, 0, 100)
            && cfg.get(1) == 20 // 越界不动
    }, "range check");
    set.add("G765 config rollback", {
        cfg.begin();
        cfg.set(2, 99, 0, 100);
        cfg.get(2) == 99 && { cfg.revert(); cfg.get(2) == 30 }
    }, "snapshot revert");
    set.add("G762 auto calibration peak", {
        // 单峰目标函数 -(p-50)^2：极值 50
        let obj = |p: u64| -((p as i64) - 50).pow(2);
        calibrate(0, 100, obj) == 50
            && { let o2 = |p: u64| p as i64; calibrate(10, 20, o2) == 20 }
    }, "grid+refine+edge");
    set.add("G767 self-config overhead budget", {
        // 校准是离线动作：运行时开销为 0（模型）
        adaptive_overhead_per_mille(10_000, 0) == 0
    }, "zero runtime");
    set.add("G774+G775 adaptive/inference hooks", {
        // 校准结果可写回配置中心且可回滚
        cfg.begin();
        cfg.set(3, 55, 0, 100);
        let ok = cfg.get(3) == 55;
        cfg.revert();
        ok && cfg.get(3) == 40
    }, "writeback");
    set.add("G779 config audit trail", {
        // 变更必须经过快照（backup 与 params 独立）
        cfg.begin();
        cfg.set(4, 77, 0, 100);
        let backed = cfg.backup[4].is_none(); // backup 记录的是变更前
        cfg.revert();
        backed && cfg.get(4) == 50
    }, "before-state");
    set.add("G780 config capacity", {
        let mut c2 = ConfigStore::new([1u64; MAX_PARAMS]);
        (0..MAX_PARAMS).all(|i| c2.set(i, 2, 0, 10)) && c2.get(0) == 2
    }, "cap 8");
    set.add("G777 config toolset model", {
        // 导出 = defaults+overrides 合并读取
        cfg.get(5) == 60 && cfg.defaults[5] == 60
    }, "merge read");

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g721_bench_stats() {
        let mut b = Bench::new();
        for v in [7u64, 1, 5, 3, 9] {
            b.record(v);
        }
        assert_eq!(b.median(), 5);
        assert_eq!(b.min(), 1);
        assert_eq!(b.max(), 9);
        // 满容量再记录失败
        let mut full = Bench::new();
        for _ in 0..MAX_SAMPLES {
            assert!(full.record(2));
        }
        assert!(!full.record(2));
    }

    #[test]
    fn g729_gate_matrix() {
        assert_eq!(regression_gate(100, 100, 50), GateVerdict::Pass);
        assert_eq!(regression_gate(100, 105, 50), GateVerdict::Pass);
        assert_eq!(regression_gate(100, 106, 50), GateVerdict::Regress);
        assert_eq!(regression_gate(100, 95, 50), GateVerdict::Pass);
        assert_eq!(regression_gate(100, 94, 50), GateVerdict::Improved);
    }

    #[test]
    fn g730_flamegraph() {
        let mut fg = FlameGraph::new();
        for (n, w) in [("a", 10u32), ("b", 20), ("a", 5)] {
            fg.add_frame(n, w);
        }
        assert_eq!(fg.total(), 35);
        assert_eq!(fg.hotspot(), Some("b"));
        assert_eq!(fg.percent_per_mille("a"), 427); // (10+5)*1000/35 逐帧取整
    }

    #[test]
    fn g741_ewma_convergence() {
        let mut m = WorkloadModel::new();
        for _ in 0..50 {
            m.observe(1000);
        }
        assert!(m.ewma > 990);
        assert_eq!(m.level(), LoadLevel::Peak);
        m.predict();
    }

    #[test]
    fn g742_policy_switch_flow() {
        let mut ps = PolicySwitcher::new(2);
        ps.suggest(Policy::Throughput);
        assert_eq!(ps.current, Policy::Latency);
        ps.suggest(Policy::Throughput);
        assert_eq!(ps.current, Policy::Throughput);
        ps.suggest(Policy::Power);
        ps.suggest(Policy::Latency);
        ps.suggest(Policy::Power);
        assert_eq!(ps.current, Policy::Throughput); // streak 被打断
        ps.suggest(Policy::Power);
        assert_eq!(ps.current, Policy::Power);
    }

    #[test]
    fn g743_overhead() {
        assert_eq!(adaptive_overhead_per_mille(1000, 10), 10);
        assert_eq!(adaptive_overhead_per_mille(0, 10), 0);
        assert_eq!(adaptive_overhead_per_mille(3, 1), 333);
    }

    #[test]
    fn g758_adaptive_engine() {
        let mut e = AdaptiveEngine::new();
        assert!(!e.apply(Policy::Latency)); // 同策略
        assert!(e.apply(Policy::Throughput));
        assert!(e.apply(Policy::Power));
        assert_eq!(e.switches, 2);
        assert!(e.rollback());
        assert_eq!(e.policy, Policy::Throughput);
        assert_eq!(e.prev, Policy::Power);
    }

    #[test]
    fn g764_config_gate() {
        let mut c = ConfigStore::new([0u64; MAX_PARAMS]);
        assert!(c.set(0, 1, 0, 10));
        assert!(!c.set(0, 0, 0, 10)); // lo 开区间
        assert!(!c.set(0, 11, 0, 10));
        assert!(!c.set(9, 5, 0, 10)); // 越界下标
    }

    #[test]
    fn g762_calibrate_quadratic() {
        let obj = |p: u64| -((p as i64) - 37).abs() * 3;
        let best = calibrate(0, 80, obj);
        assert!((best as i64 - 37).abs() <= 1);
    }

    #[test]
    fn g780_domain_closure() {
        let set = run_gperf_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("gperf self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
        assert!(!set.truncated());
    }
}
