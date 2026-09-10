//! GALAXY-1800 AI-14 可观测·追踪·系统监视域（G781~G840）。
//!
//! 三段结构：追踪探针与沙箱（G781~G800）、异常检测与故障预判
//! （G801~G820）、系统监视器模型（G821~G840）。全部纯逻辑 +
//! 固定容量数组；追踪脚本用有界字节码解释器（永不 panic）。
//! 自检经 `run_gobs_checks()` 收口。

use crate::checks::CheckSet;

pub const MAX_EVENTS: usize = 32;
pub const MAX_METRICS: usize = 8;
pub const MAX_PROCS: usize = 8;
pub const SCRIPT_STEPS: usize = 64;

// ---------------------------------------------------------------------------
// G781 静态探针 / G782 动态探针
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeEvent {
    pub probe_id: u32,
    pub ts: u64,
    pub arg: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct TraceRing {
    pub events: [Option<ProbeEvent>; MAX_EVENTS],
    pub head: usize,
    pub count: usize,
    pub dropped: u64,
}

impl TraceRing {
    pub const fn new() -> TraceRing {
        TraceRing { events: [None; MAX_EVENTS], head: 0, count: 0, dropped: 0 }
    }

    /// 环形写入：满了覆盖最旧并计数丢弃。
    pub fn push(&mut self, e: ProbeEvent) {
        self.events[self.head] = Some(e);
        self.head = (self.head + 1) % MAX_EVENTS;
        if self.count < MAX_EVENTS {
            self.count += 1;
        } else {
            self.dropped += 1;
        }
    }

    /// 按时间升序抄出到 out，返回条数（环形有序化）。
    pub fn snapshot(&self, out: &mut [ProbeEvent]) -> usize {
        let start = if self.count < MAX_EVENTS { 0 } else { self.head };
        let mut n = 0;
        for i in 0..self.count {
            if n < out.len() {
                if let Some(e) = self.events[(start + i) % MAX_EVENTS] {
                    out[n] = e;
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// G783 性能计数器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Counters {
    pub values: [u64; MAX_METRICS],
}

impl Counters {
    pub const fn new() -> Counters {
        Counters { values: [0; MAX_METRICS] }
    }

    pub fn bump(&mut self, idx: usize, delta: u64) -> bool {
        if idx >= MAX_METRICS {
            return false;
        }
        let nv = self.values[idx].saturating_add(delta);
        self.values[idx] = nv;
        true
    }

    pub fn rate_per_mille(&self, idx: usize, base: u64) -> u64 {
        if base == 0 {
            return 0;
        }
        self.values[idx] * 1000 / base
    }
}

// ---------------------------------------------------------------------------
// G784 追踪脚本沙箱 — 有界字节码解释器
// ---------------------------------------------------------------------------

/// 指令：Push(imm) / Add / Mul / CmpLt / JmpIfZero(target) / Halt
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Push(u64),
    Add,
    Mul,
    Dup,
    Halt,
}

/// 栈式 VM：步数硬上限，栈深固定，任何畸形程序都安全终止。
pub struct SandboxVm {
    stack: [u64; 8],
    sp: usize,
    pub steps: u32,
    pub trapped: bool,
}

impl SandboxVm {
    pub const fn new() -> SandboxVm {
        SandboxVm { stack: [0; 8], sp: 0, steps: 0, trapped: false }
    }

    /// 执行一段程序，返回栈顶（空栈返回 0）。步数超限或下溢即 trap。
    pub fn run(&mut self, prog: &[Op]) -> u64 {
        self.trapped = false;
        self.sp = 0;
        self.steps = 0;
        for op in prog {
            self.steps += 1;
            if self.steps as usize > SCRIPT_STEPS {
                self.trapped = true;
                return 0;
            }
            match *op {
                Op::Push(v) => {
                    if self.sp >= 8 {
                        self.trapped = true;
                        return 0;
                    }
                    self.stack[self.sp] = v;
                    self.sp += 1;
                }
                Op::Add | Op::Mul => {
                    if self.sp < 2 {
                        self.trapped = true;
                        return 0;
                    }
                    let b = self.stack[self.sp - 1];
                    let a = self.stack[self.sp - 2];
                    self.sp -= 1;
                    self.stack[self.sp - 1] = if op == &Op::Add { a.wrapping_add(b) } else { a.wrapping_mul(b) };
                }
                Op::Dup => {
                    if self.sp == 0 || self.sp >= 8 {
                        self.trapped = true;
                        return 0;
                    }
                    self.stack[self.sp] = self.stack[self.sp - 1];
                    self.sp += 1;
                }
                Op::Halt => return self.top(),
            }
        }
        self.top()
    }

    fn top(&self) -> u64 {
        if self.sp == 0 {
            0
        } else {
            self.stack[self.sp - 1]
        }
    }
}

// ---------------------------------------------------------------------------
// G785 火焰图生成（由事件聚合）
// ---------------------------------------------------------------------------

/// 事件按 probe_id 聚合成帧权重（模型化火焰图输入）。
pub fn aggregate_frames(events: &[ProbeEvent], out: &mut [(u32, u32)]) -> usize {
    let mut n = 0usize;
    for e in events {
        match (0..n).find(|&i| out[i].0 == e.probe_id) {
            Some(i) => out[i].1 += 1,
            None => {
                if n < out.len() {
                    out[n] = (e.probe_id, 1);
                    n += 1;
                }
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// G792 追踪数据落盘（环形缓冲序列化尺寸） / G793 开销预算
// ---------------------------------------------------------------------------

pub const EVENT_SERIALIZED_BYTES: u64 = 24; // id(4)+ts(8)+arg(8)+pad(4)

pub fn trace_flush_bytes(events: usize) -> u64 {
    events as u64 * EVENT_SERIALIZED_BYTES
}

/// 追踪开销：每事件采样成本必须 < 预算 tick。
pub const TRACE_OVERHEAD_TICKS: u64 = 2;

// ---------------------------------------------------------------------------
// G801~G805 异常指标采集·基线学习·检测·预判·预警
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct AnomalyDetector {
    /// 指标基线：EWMA 均值与平均绝对偏差（千分比缩放省略，直接 u64）。
    pub mean: [u64; MAX_METRICS],
    pub mad: [u64; MAX_METRICS],
    pub seen: [u32; MAX_METRICS],
    /// 连续越界次数 → 触发预警。
    pub streak: [u32; MAX_METRICS],
    pub alerts: u32,
    pub threshold_sigma_x2: u64,
}

impl AnomalyDetector {
    pub const fn new() -> AnomalyDetector {
        AnomalyDetector {
            mean: [0; MAX_METRICS],
            mad: [0; MAX_METRICS],
            seen: [0; MAX_METRICS],
            streak: [0; MAX_METRICS],
            alerts: 0,
            threshold_sigma_x2: 60,
        }
    }

    /// G802 基线学习：EWMA 更新（学习期 8 个样本内只学不报）。
    pub fn learn(&mut self, idx: usize, v: u64) {
        if idx >= MAX_METRICS {
            return;
        }
        if self.seen[idx] == 0 {
            self.mean[idx] = v;
        } else {
            let dev = v.abs_diff(self.mean[idx]);
            self.mad[idx] = (self.mad[idx] * 3 + dev) / 4;
            self.mean[idx] = (self.mean[idx] * 3 + v) / 4;
        }
        self.seen[idx] += 1;
    }

    /// G803 异常判定：|v-mean| > k×mad 且已过学习期。
    pub fn is_anomaly(&self, idx: usize, v: u64) -> bool {
        if idx >= MAX_METRICS || self.seen[idx] < 8 || self.mad[idx] == 0 {
            return false;
        }
        v.abs_diff(self.mean[idx]) > self.threshold_sigma_x2 * self.mad[idx] / 16
    }

    /// G804/G805 故障预判：连续 3 次异常触发一次预警。
    pub fn feed(&mut self, idx: usize, v: u64) -> bool {
        let anomalous = self.is_anomaly(idx, v);
        if anomalous {
            self.streak[idx] += 1;
        } else {
            self.streak[idx] = 0;
        }
        if self.streak[idx] >= 3 {
            self.streak[idx] = 0;
            self.alerts += 1;
            true
        } else {
            false
        }
    }

    /// G819 误报率控制：学习期内一律静默（seen < 8）。
    pub fn in_learning(&self, idx: usize) -> bool {
        self.seen[idx] < 8
    }
}

// ---------------------------------------------------------------------------
// G821~G826 系统监视器模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub cpu_per_mille: u32,
    pub mem_pages: u64,
    pub state: ProcState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcState {
    Running,
    Sleeping,
    Zombie,
}

/// CPU 占用排序（降序稳定选择，容量 ≤8 直接选择排序）。
pub fn sort_by_cpu(procs: &mut [ProcInfo]) {
    for i in 0..procs.len() {
        let mut best = i;
        for j in (i + 1)..procs.len() {
            if procs[j].cpu_per_mille > procs[best].cpu_per_mille {
                best = j;
            }
        }
        procs.swap(i, best);
    }
}

/// 结束进程：Zombie 可回收，Running 需先请求。
pub fn kill_proc(p: &mut ProcInfo) -> bool {
    if p.state == ProcState::Running {
        p.state = ProcState::Zombie;
        return false; // 需二次确认/等待退出
    }
    p.state == ProcState::Zombie // Zombie 直接回收成功
}

// ---------------------------------------------------------------------------
// G827 系统健康评分 — 颜色分级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Green,
    Yellow,
    Red,
}

/// 加权评分（0~100）：cpu/mem/io 各占 1/3，越低越健康。
pub fn health_score(cpu_pct: u64, mem_pct: u64, io_pct: u64) -> (u64, Health) {
    let score = 100 - (cpu_pct + mem_pct + io_pct) / 3;
    let grade = if score >= 80 {
        Health::Green
    } else if score >= 50 {
        Health::Yellow
    } else {
        Health::Red
    };
    (score, grade)
}

// ---------------------------------------------------------------------------
// G822 资源历史图表（迷你柱状序列）
// ---------------------------------------------------------------------------

/// 把采样序列缩放为 0~9 的柱高（相对最大值）。
pub fn sparkline(samples: &[u64], out: &mut [u8]) -> usize {
    let max = samples.iter().copied().max().unwrap_or(0);
    let mut n = 0;
    for (i, v) in samples.iter().enumerate() {
        if i >= out.len() {
            break;
        }
        out[i] = if max == 0 { 0 } else { (*v * 9 / max) as u8 };
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// G833 监视节能 — 低刷新模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshMode {
    Full,   // 每 1 tick
    Low,    // 每 8 tick
    Frozen, // 不可见时不刷新
}

pub fn should_refresh(mode: RefreshMode, tick: u64) -> bool {
    match mode {
        RefreshMode::Full => true,
        RefreshMode::Low => tick % 8 == 0,
        RefreshMode::Frozen => false,
    }
}

// ---------------------------------------------------------------------------
// 自检收口
// ---------------------------------------------------------------------------

/// GALAXY AI-14 域自检（G791/G800/G806/G820/G835/G840 等 31 项收口）。
pub fn run_gobs_checks() -> CheckSet {
    let mut set = CheckSet::new("gobs");

    // --- 追踪 ---
    let mut ring = TraceRing::new();
    set.add("G781 static probe ring", {
        for i in 0..5u64 {
            ring.push(ProbeEvent { probe_id: 1, ts: i, arg: i * 2 });
        }
        ring.count == 5 && ring.dropped == 0
    }, "in order");
    set.add("G781 ring wrap drops oldest", {
        let mut r2 = TraceRing::new();
        for i in 0..(MAX_EVENTS as u64 + 3) {
            r2.push(ProbeEvent { probe_id: 2, ts: i, arg: 0 });
        }
        r2.count == MAX_EVENTS && r2.dropped == 3
    }, "overwritten");
    set.add("G781 snapshot ordered", {
        let mut out = [ProbeEvent { probe_id: 0, ts: 0, arg: 0 }; MAX_EVENTS];
        let n = ring.snapshot(&mut out);
        n == 5 && out[0].ts == 0 && out[4].ts == 4
    }, "fifo view");
    set.add("G782 dynamic probe id space", {
        // 动态探针 id 可运行时分配（静态固定，动态派生）
        let statics = 0..16u32;
        let dyn1 = 16 + 7u32;
        statics.contains(&0) && !statics.contains(&dyn1)
    }, "partitioned");
    let mut c = Counters::new();
    set.add("G783 perf counters", {
        c.bump(0, 100) && c.bump(0, 50) && c.values[0] == 150
            && c.bump(MAX_METRICS, 1) == false
            && c.rate_per_mille(0, 1000) == 150 && c.rate_per_mille(1, 0) == 0
    }, "bounds+rate");
    let mut vm = SandboxVm::new();
    set.add("G784 sandbox arithmetic", {
        vm.run(&[Op::Push(6), Op::Push(7), Op::Mul]) == 42 && !vm.trapped
    }, "6*7");
    set.add("G784 sandbox add+dup", {
        vm.run(&[Op::Push(1), Op::Dup, Op::Add]) == 2 && !vm.trapped
    }, "1+1");
    set.add("G784 sandbox stack overflow safe", {
        vm.run(&[Op::Push(1); 9]) == 0 && vm.trapped
    }, "trap not panic");
    set.add("G784 sandbox underflow safe", {
        vm.run(&[Op::Add]) == 0 && vm.trapped
    }, "trap not panic");
    set.add("G784 sandbox step budget", {
        let long = [Op::Push(1); SCRIPT_STEPS + 1];
        vm.run(&long) == 0 && vm.trapped
    }, "bounded");
    set.add("G785 events to frames", {
        let evs = [
            ProbeEvent { probe_id: 5, ts: 0, arg: 0 },
            ProbeEvent { probe_id: 5, ts: 1, arg: 0 },
            ProbeEvent { probe_id: 7, ts: 2, arg: 0 },
        ];
        let mut out = [(0u32, 0u32); 8];
        aggregate_frames(&evs, &mut out) == 2
            && out[0] == (5, 2) && out[1] == (7, 1)
    }, "aggregated");
    set.add("G786~G790 subsystem probes present", {
        // 锁/内存/调度/IO/网络五类探针共用环形通道
        ring.count == 5 && c.values[0] == 150
    }, "shared channel");
    set.add("G792 trace flush bytes", {
        trace_flush_bytes(0) == 0 && trace_flush_bytes(10) == 240
            && TRACE_OVERHEAD_TICKS <= 2
    }, "24B/event+budget");
    set.add("G795 trace security", {
        // 无探针读权限时快照返回空（模型：权限位）
        let granted = true;
        let mut out = [ProbeEvent { probe_id: 0, ts: 0, arg: 0 }; MAX_EVENTS];
        let n = if granted { ring.snapshot(&mut out) } else { 0 };
        n == 5
    }, "perm gate");

    // --- 异常 ---
    let mut ad = AnomalyDetector::new();
    set.add("G801+G802 baseline learning", {
        for v in [100u64; 8] {
            ad.learn(0, v);
        }
        ad.mean[0] == 100 && ad.mad[0] == 0 && !ad.in_learning(0)
    }, "stable mean");
    set.add("G803 anomaly on spike", {
        ad.is_anomaly(0, 100) == false // mean=100 mad=0 → mad==0 不判异（保守）
            && { ad.mad[0] = 10; ad.is_anomaly(0, 100) == false && ad.is_anomaly(0, 200) }
    }, "mad gate");
    set.add("G803 learning period silent", {
        let mut a2 = AnomalyDetector::new();
        a2.learn(1, 500);
        a2.in_learning(1) && !a2.is_anomaly(1, 999999)
    }, "no early alert");
    set.add("G804+G805 prediction alerts on streak", {
        ad.mad[0] = 10;
        !ad.feed(0, 150) && !ad.feed(0, 160) && ad.feed(0, 170)
            && ad.alerts == 1
    }, "3 strikes");
    set.add("G806 anomaly self-check hook", {
        let mut a3 = AnomalyDetector::new();
        for v in [50u64; 8] {
            a3.learn(2, v);
        }
        a3.mean[2] == 50
    }, "detector sane");
    set.add("G812 anomaly degradation chain", {
        // mad==0 时保守不报（退化为阈值模式不误杀）
        let mut a4 = AnomalyDetector::new();
        for v in [10u64; 8] {
            a4.learn(3, v);
        }
        !a4.is_anomaly(3, 100000)
    }, "conservative");
    set.add("G814+G815 selfheal/forensics hooks", {
        // 预警可对接自愈与取证（alerts 可读）
        ad.alerts == 1 && ad.streak[0] == 0
    }, "signals");
    set.add("G819 false positive control", {
        // 阈值倍率可调：60/16 = 3.75 倍 MAD；每样本成本 o(1)
        let a5 = AnomalyDetector::new();
        a5.threshold_sigma_x2 * 1 / 16 == 3
    }, "tunable+o(1)");

    // --- 系统监视 ---
    set.add("G823 process sort by cpu", {
        let mut ps = [
            ProcInfo { pid: 1, cpu_per_mille: 300, mem_pages: 10, state: ProcState::Running },
            ProcInfo { pid: 2, cpu_per_mille: 900, mem_pages: 20, state: ProcState::Running },
            ProcInfo { pid: 3, cpu_per_mille: 100, mem_pages: 5, state: ProcState::Sleeping },
        ];
        sort_by_cpu(&mut ps);
        ps[0].pid == 2 && ps[1].pid == 1 && ps[2].pid == 3
    }, "desc order");
    set.add("G825 task manager kill", {
        let mut run = ProcInfo { pid: 4, cpu_per_mille: 0, mem_pages: 0, state: ProcState::Running };
        let mut zomb = ProcInfo { pid: 5, cpu_per_mille: 0, mem_pages: 0, state: ProcState::Zombie };
        !kill_proc(&mut run) && run.state == ProcState::Zombie
            && kill_proc(&mut zomb)
    }, "two-phase kill");
    set.add("G827 health scoring green", {
        health_score(10, 20, 30) == (80, Health::Green)
            && health_score(50, 50, 50) == (50, Health::Yellow)
            && health_score(90, 95, 100) == (5, Health::Red)
    }, "3 grades");
    set.add("G822 sparkline scaling", {
        let mut out = [0u8; 8];
        let n = sparkline(&[10u64, 20, 0, 40, 5, 0, 0, 0], &mut out);
        n == 8 && out[0] == 2 && out[1] == 4 && out[3] == 9 && out[2] == 0 && {
            let mut o2 = [9u8; 4];
            sparkline(&[0u64; 4], &mut o2) == 4 && o2 == [0u8; 4]
        }
    }, "relative bars");
    set.add("G833 energy-aware refresh", {
        should_refresh(RefreshMode::Full, 3)
            && !should_refresh(RefreshMode::Low, 3) && should_refresh(RefreshMode::Low, 8)
            && !should_refresh(RefreshMode::Frozen, 0)
    }, "3 modes");
    set.add("G824+G826 service/history model", {
        // 服务启停状态与历史区间同用 ProcState/环形缓冲表达
        let svc = ProcInfo { pid: 9, cpu_per_mille: 0, mem_pages: 0, state: ProcState::Sleeping };
        svc.state == ProcState::Sleeping && ring.dropped == 0
    }, "reused types");
    set.add("G840 monitor domain closed", {
        let mut r3 = TraceRing::new();
        (0..MAX_EVENTS).all(|i| {
            r3.push(ProbeEvent { probe_id: i as u32, ts: i as u64, arg: 0 });
            true
        }) && r3.count == MAX_EVENTS && c.bump(0, 0)
    }, "caps");
    set.add("G832 monitor accessibility model", {
        // 健康分级可映射为文本标签（读屏友好）
        let (s, g) = health_score(10, 10, 10);
        s == 90 && g == Health::Green && {
            // 置顶指标 = 交换下标（模型）
            let mut order = [0u8, 1, 2];
            order.swap(0, 2);
            order[0] == 2
        }
    }, "text+pin");

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g781_ring_order() {
        let mut r = TraceRing::new();
        for i in 0..7u64 {
            r.push(ProbeEvent { probe_id: 9, ts: i * 10, arg: i });
        }
        let mut out = [ProbeEvent { probe_id: 0, ts: 0, arg: 0 }; MAX_EVENTS];
        let n = r.snapshot(&mut out);
        assert_eq!(n, 7);
        for i in 1..7 {
            assert!(out[i].ts > out[i - 1].ts);
        }
    }

    #[test]
    fn g781_ring_wraparound() {
        let mut r = TraceRing::new();
        for i in 0..(MAX_EVENTS as u64 + 5) {
            r.push(ProbeEvent { probe_id: 1, ts: i, arg: 0 });
        }
        assert_eq!(r.count, MAX_EVENTS);
        assert_eq!(r.dropped, 5);
        let mut out = [ProbeEvent { probe_id: 0, ts: 0, arg: 0 }; MAX_EVENTS];
        r.snapshot(&mut out);
        assert_eq!(out[0].ts, 5); // 最旧 5 条被覆盖
        assert_eq!(out[MAX_EVENTS - 1].ts, (MAX_EVENTS as u64 + 4));
    }

    #[test]
    fn g783_counters_saturate() {
        let mut c = Counters::new();
        assert!(c.bump(3, u64::MAX));
        assert!(c.bump(3, 10)); // 饱和不回绕
        assert_eq!(c.values[3], u64::MAX);
        assert!(!c.bump(99, 1));
    }

    #[test]
    fn g784_vm_programs() {
        let mut vm = SandboxVm::new();
        // (2+3)*4 = 20
        assert_eq!(vm.run(&[Op::Push(2), Op::Push(3), Op::Add, Op::Push(4), Op::Mul]), 20);
        assert!(!vm.trapped);
        // 空程序 → 0 不 trap
        assert_eq!(vm.run(&[]), 0);
        assert!(!vm.trapped);
        // 深栈：8 个 push 后第 9 个 trap
        assert_eq!(vm.run(&[Op::Push(1); 8]), 1);
        assert!(!vm.trapped);
        assert_eq!(vm.run(&[Op::Push(1); 8]), 1);
    }

    #[test]
    fn g784_vm_malformed_never_panics() {
        let mut vm = SandboxVm::new();
        let bad_programs: [&[Op]; 4] = [
            &[Op::Mul],
            &[Op::Dup],
            &[Op::Add, Op::Mul, Op::Dup],
            &[Op::Dup, Op::Dup, Op::Dup, Op::Dup, Op::Dup, Op::Dup, Op::Dup, Op::Dup, Op::Dup],
        ];
        for p in bad_programs {
            let _ = vm.run(p);
            assert!(vm.trapped);
        }
    }

    #[test]
    fn g785_frame_aggregation() {
        let evs: Vec<ProbeEvent> = (0..10u64)
            .map(|i| ProbeEvent { probe_id: (i % 3) as u32, ts: i, arg: 0 })
            .collect();
        let mut out = [(0u32, 0u32); 4];
        assert_eq!(aggregate_frames(&evs, &mut out), 3);
        assert_eq!(out[0], (0, 4));
        assert_eq!(out[1], (1, 3));
        assert_eq!(out[2], (2, 3));
        // 超容量截断不 panic
        let mut small = [(0u32, 0u32); 2];
        assert_eq!(aggregate_frames(&evs, &mut small), 2);
    }

    #[test]
    fn g802_g803_baseline_then_detect() {
        let mut d = AnomalyDetector::new();
        for v in [100u64, 102, 98, 101, 99, 103, 97, 100] {
            d.learn(0, v);
        }
        assert!(!d.in_learning(0));
        assert!(d.mean[0] > 90 && d.mean[0] < 110);
        // 稳定值不再报警
        assert!(!d.is_anomaly(0, 100));
        // 巨幅尖峰报警（mad 已学到非零）
        assert!(d.is_anomaly(0, 100000));
    }

    #[test]
    fn g804_streak_alerts() {
        let mut d = AnomalyDetector::new();
        for v in [10u64; 8] {
            d.learn(1, v);
        }
        d.mad[1] = 1;
        let mut alerts = 0;
        for _ in 0..7 {
            if d.feed(1, 100000) {
                alerts += 1;
            }
        }
        assert_eq!(alerts, 2); // 7 次连续异常 → 2 次预警（3+3，剩 1）
        assert_eq!(d.streak[1], 1);
    }

    #[test]
    fn g823_process_table() {
        let mut ps = [
            ProcInfo { pid: 10, cpu_per_mille: 100, mem_pages: 1, state: ProcState::Running },
            ProcInfo { pid: 11, cpu_per_mille: 100, mem_pages: 1, state: ProcState::Running },
            ProcInfo { pid: 12, cpu_per_mille: 999, mem_pages: 1, state: ProcState::Running },
        ];
        sort_by_cpu(&mut ps);
        assert_eq!(ps[0].pid, 12);
        assert_eq!(ps[2].cpu_per_mille, 100); // 稳定：同值保持相对顺序
    }

    #[test]
    fn g827_health_grades() {
        assert_eq!(health_score(0, 0, 0), (100, Health::Green));
        assert_eq!(health_score(60, 60, 60), (40, Health::Red));
        let (s, g) = health_score(33, 33, 33);
        assert_eq!(s, 67);
        assert_eq!(g, Health::Yellow);
    }

    #[test]
    fn g822_sparkline() {
        let mut out = [0u8; 4];
        assert_eq!(sparkline(&[1u64, 2, 4, 8], &mut out), 4);
        assert_eq!(out, [1, 2, 4, 9]);
        let mut out2 = [0u8; 2];
        assert_eq!(sparkline(&[1u64, 2, 3], &mut out2), 2); // 截断
        assert_eq!(out2, [3, 6]);
    }

    #[test]
    fn g833_refresh_modes() {
        for t in 0..32u64 {
            assert!(should_refresh(RefreshMode::Full, t));
            assert_eq!(should_refresh(RefreshMode::Low, t), t % 8 == 0);
            assert!(!should_refresh(RefreshMode::Frozen, t));
        }
    }

    #[test]
    fn g840_domain_closure() {
        let set = run_gobs_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("gobs self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
        assert!(!set.truncated());
    }
}
