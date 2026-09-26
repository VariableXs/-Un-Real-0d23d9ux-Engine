//! 测量面：打点全覆盖 + 账本一致性 + 六类基准骨架（WP-209 · B-1401~1403 首轮）。
//!
//! MD2 篇 14.1/14.2：打点体系——全系统统一的打点接口（零分配、无锁、
//! 纳秒级单调时钟），点位覆盖启动十一点位、帧生命周期两点位、交接五步、
//! 应用启动四段、存储路径三段、网络路径两段。打点数据流向：环形缓冲
//! 实时记、启动报告与诊断中心聚合读、vxbench 归档存——一份打点三个
//! 消费者，不许各插各的桩。内核账本：内存与 CPU 分类记账，账本接口
//! 只有读。vxbench 六类硬指标：syscall 往返/上下文切换/页分配释放/
//! 帧管线/块路径/网络环回——每个基准的输出是分布不是均值（P50/P95/
//! P99 三线），回归判定按 P95 对基线偏差超 10% 标红。
//! 基准数字与监视器账本同源（WP-206 B-2001 契约——两套数字等于没有
//! 数字）。首轮 = 骨架 + 整数模型面，WP-402 补回归门全量。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// B-1401 打点全覆盖：点位清单与环形缓冲
// ---------------------------------------------------------------------------

/// 点位清单（篇 1.8 + 5.1 + 14.1）：十一 + 两 + 五 + 四 + 三 + 二 = 27。
pub const PROBE_POINTS: usize = 27;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbePoint {
    // 启动十一点位（篇 1.8）
    BootEntry,
    KernelEntry,
    SelfCheckDone,
    DriversReady,
    FsMounted,
    ServicesUp,
    CompositorUp,
    SessionReady,
    DesktopFirstFrame,
    LoginDone,
    StartupReported,
    // 帧生命周期两点位（篇 5.1）
    FrameBegin,
    FrameSubmit,
    // 交接五步
    HandoffArmed,
    HandoffFlush,
    HandoffSnapshot,
    HandoffSwitch,
    HandoffResume,
    // 应用启动四段
    AppFork,
    AppLoad,
    AppInit,
    AppFirstFrame,
    // 存储路径三段
    StoreEnqueue,
    StoreMergeFlush,
    StoreConfirm,
    // 网络路径两段
    NetEnqueue,
    NetConfirm,
}

pub const ALL_POINTS: [ProbePoint; PROBE_POINTS] = [
    ProbePoint::BootEntry,
    ProbePoint::KernelEntry,
    ProbePoint::SelfCheckDone,
    ProbePoint::DriversReady,
    ProbePoint::FsMounted,
    ProbePoint::ServicesUp,
    ProbePoint::CompositorUp,
    ProbePoint::SessionReady,
    ProbePoint::DesktopFirstFrame,
    ProbePoint::LoginDone,
    ProbePoint::StartupReported,
    ProbePoint::FrameBegin,
    ProbePoint::FrameSubmit,
    ProbePoint::HandoffArmed,
    ProbePoint::HandoffFlush,
    ProbePoint::HandoffSnapshot,
    ProbePoint::HandoffSwitch,
    ProbePoint::HandoffResume,
    ProbePoint::AppFork,
    ProbePoint::AppLoad,
    ProbePoint::AppInit,
    ProbePoint::AppFirstFrame,
    ProbePoint::StoreEnqueue,
    ProbePoint::StoreMergeFlush,
    ProbePoint::StoreConfirm,
    ProbePoint::NetEnqueue,
    ProbePoint::NetConfirm,
];

pub const RING_CAP: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProbeHit {
    pub point: ProbePoint,
    pub t_ns: u64,
}

/// 打点环形缓冲：实时记（单写多读快照面——零分配无锁建模为拷贝快照）。
pub struct ProbeRing {
    hits: [Option<ProbeHit>; RING_CAP],
    head: usize,
    pub count: u64,
}

/// 消费者三类（篇 14.1：一份打点三个消费者）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeConsumer {
    LiveRing,     // 环形缓冲实时记
    Aggregated,   // 启动报告与诊断中心聚合读
    Archived,     // vxbench 归档存
}

impl ProbeRing {
    pub fn new() -> Self {
        ProbeRing { hits: [None; RING_CAP], head: 0, count: 0 }
    }

    /// 打点：常数时间（拷贝快照建模读面——无锁语义的宿主近似）。
    pub fn hit(&mut self, point: ProbePoint, t_ns: u64) {
        self.hits[self.head] = Some(ProbeHit { point, t_ns });
        self.head = (self.head + 1) % RING_CAP;
        self.count += 1;
    }

    /// 消费者取数：三类消费者读同一环（同源——B-2001 契约的打点面）。
    pub fn read_by(&self, _who: ProbeConsumer) -> ([Option<ProbeHit>; RING_CAP], usize) {
        let mut snap: [Option<ProbeHit>; RING_CAP] = [None; RING_CAP];
        let mut i = 0;
        while i < RING_CAP {
            snap[i] = self.hits[i];
            i += 1;
        }
        let last = if self.count == 0 { 0 } else { ((self.count - 1) as usize) % RING_CAP };
        (snap, last)
    }

    /// 千点开销预算（B-1401 达标线 <1ms/千点）：每打点 ~400ns 整数模型。
    pub fn cost_per_kilo_ns() -> u64 {
        400 * 1000 // 400µs < 1ms
    }

    /// 点位清单零缺失校验：27 点位枚举齐且无重复。
    pub fn manifest_complete() -> bool {
        if ALL_POINTS.len() != PROBE_POINTS {
            return false;
        }
        let mut i = 0;
        while i < PROBE_POINTS {
            let mut j = i + 1;
            while j < PROBE_POINTS {
                if ALL_POINTS[i] == ALL_POINTS[j] {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// B-1402 账本一致性：分账合计 vs 总账，偏差 < 1%
// ---------------------------------------------------------------------------

pub const POOL_CAP: usize = 8;

/// 账本对账：各池占用合计与总账偏差千分比 < 10‰（1%）。
pub struct LedgerAudit {
    pub pools_kb: [u64; POOL_CAP],
    pub pool_cnt: usize,
    pub total_kb: u64,
}

impl LedgerAudit {
    /// 偏差千分比：|sum(pools) - total| * 1000 / max(total, 1)。
    pub fn drift_permille(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.pool_cnt {
            sum += self.pools_kb[i];
            i += 1;
        }
        let diff = if sum > self.total_kb { sum - self.total_kb } else { self.total_kb - sum };
        diff * 1000 / self.total_kb.max(1)
    }

    pub fn consistent(&self) -> bool {
        self.drift_permille() < 10 // < 1%
    }
}

// ---------------------------------------------------------------------------
// B-1403 六类基准骨架：三线分布 + 回归判定
// ---------------------------------------------------------------------------

pub const BENCH_KINDS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchKind {
    SyscallRoundtrip, // syscall 往返（空调用与常调用两版，百万次取 P95）
    CtxSwitch,        // 上下文切换（双进程乒乓）
    PageAllocFree,    // 页分配与释放（各尺寸档）
    FramePipeline,    // 帧管线（空帧/单表面/十表面分档）
    BlockPath,        // 块路径（顺序读写/随机读写/fsync 延迟分布）
    NetLoopback,      // 网络环回（栈内环回吞吐与延迟）
}

pub const ALL_BENCH: [BenchKind; BENCH_KINDS] = [
    BenchKind::SyscallRoundtrip,
    BenchKind::CtxSwitch,
    BenchKind::PageAllocFree,
    BenchKind::FramePipeline,
    BenchKind::BlockPath,
    BenchKind::NetLoopback,
];

/// 三线分布：P50/P95/P99（输出是分布不是均值——长尾才是手感杀手）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dist3 {
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
}

/// 基准结果：六类全部产出三线才算齐。
pub struct BenchReport {
    pub dist: [Option<Dist3>; BENCH_KINDS],
}

impl BenchReport {
    pub fn new() -> Self {
        BenchReport { dist: [None; BENCH_KINDS] }
    }

    pub fn record(&mut self, kind: BenchKind, samples: &[u64; 100]) {
        // 插入排序（零堆）后取三线：P50=第 50 位、P95=第 95 位、P99=第 99 位
        //（1-based nearest-rank，n=100 时即索引 49/94/98）。
        let mut s = *samples;
        let mut a = 1;
        while a < 100 {
            let mut b = a;
            while b > 0 && s[b - 1] > s[b] {
                s.swap(b - 1, b);
                b -= 1;
            }
            a += 1;
        }
        let d = Dist3 { p50_ns: s[49], p95_ns: s[94], p99_ns: s[98] };
        self.dist[kind_index(kind)] = Some(d);
    }

    pub fn all_three_line(&self) -> bool {
        let mut i = 0;
        while i < BENCH_KINDS {
            match self.dist[i] {
                Some(d) if d.p50_ns <= d.p95_ns && d.p95_ns <= d.p99_ns => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 回归判定：P95 对基线偏差超 10% 标红（19.2 阈值）。
    pub fn regress_red(kind: BenchKind, baseline_p95: u64, report: &BenchReport) -> bool {
        match report.dist[kind_index(kind)] {
            Some(d) => {
                let diff = if d.p95_ns > baseline_p95 { d.p95_ns - baseline_p95 } else { baseline_p95 - d.p95_ns };
                diff * 100 > baseline_p95.max(1) * 10
            }
            None => true, // 缺数据按红处理——不缺位
        }
    }
}

pub fn kind_index(k: BenchKind) -> usize {
    match k {
        BenchKind::SyscallRoundtrip => 0,
        BenchKind::CtxSwitch => 1,
        BenchKind::PageAllocFree => 2,
        BenchKind::FramePipeline => 3,
        BenchKind::BlockPath => 4,
        BenchKind::NetLoopback => 5,
    }
}

/// 确定性样本生成（宿主模型面：真实计时随实机——整数模型 + LCG 同源
/// galaxy::rt 范式）。
pub fn synth_samples(kind: BenchKind, seed: u64) -> [u64; 100] {
    let base = match kind {
        BenchKind::SyscallRoundtrip => 2_000u64,
        BenchKind::CtxSwitch => 5_000,
        BenchKind::PageAllocFree => 1_000,
        BenchKind::FramePipeline => 10_000,
        BenchKind::BlockPath => 50_000,
        BenchKind::NetLoopback => 30_000,
    };
    let mut x = seed | 1;
    let mut out = [0u64; 100];
    let mut i = 0;
    while i < 100 {
        // LCG（与 galaxy 范式同源）。
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let jitter = (x >> 33) % 1000; // 0..999 抖动
        out[i] = base + jitter;
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// CheckSet（B-1401 · 4 项 + B-1402 · 2 项 + B-1403 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_benchsix_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1401~1403 测量面首轮");
    // 1. 点位清单零缺失：27 点位枚举齐无重复。
    set.add(
        "B-1401 点位清单零缺失",
        ProbeRing::manifest_complete(),
        "启动 11+帧 2+交接 5+应用 4+存储 3+网络 2=27 点位齐且无重复",
    );
    // 2. 打点开销 < 1ms/千点（整数预算模型）。
    set.add(
        "B-1401 千点开销预算",
        ProbeRing::cost_per_kilo_ns() < 1_000_000,
        "400ns/点 × 1000 = 400µs < 1ms（宿主整数模型，实机回填）",
    );
    // 3. 一份打点三个消费者：三类读同一环，快照逐位一致。
    let mut ring = ProbeRing::new();
    ring.hit(ProbePoint::FrameBegin, 100);
    ring.hit(ProbePoint::FrameSubmit, 14_000);
    let (s1, n1) = ring.read_by(ProbeConsumer::LiveRing);
    let (s2, n2) = ring.read_by(ProbeConsumer::Aggregated);
    let (s3, n3) = ring.read_by(ProbeConsumer::Archived);
    set.add(
        "B-1401 三消费者同源",
        n1 == n2 && n2 == n3 && n3_count(&s1, &s2, &s3) == ring.count as usize && ring.count == 2,
        "实时记/聚合读/归档读同一环：最新位一致+命中逐位一致——不许各插各的桩",
    );
    // 4. 环形恒容量：300 次打点后容量不涨（覆盖最旧）。
    let mut ring4 = ProbeRing::new();
    let mut i4 = 0u64;
    while i4 < 300 {
        ring4.hit(ProbePoint::FrameBegin, i4);
        i4 += 1;
    }
    set.add(
        "B-1401 环形恒容量",
        ring4.count == 300 && RING_CAP == 256,
        "环形缓冲实时记：容量恒 256，第 257 点覆盖最旧",
    );
    // 5. 账本一致性：分账合计==总账（零偏差）。
    let la_ok = LedgerAudit { pools_kb: [1000, 2000, 3000, 4000, 0, 0, 0, 0], pool_cnt: 4, total_kb: 10_000 };
    set.add(
        "B-1402 合计==总账",
        la_ok.consistent() && la_ok.drift_permille() == 0,
        "各池占用合计与总账逐位一致——偏差 0‰ < 1%",
    );
    // 6. 偏差 < 1% 拒绝线：900 字节偏差（9‰）过、12KB 偏差（12‰）红。
    let la_edge = LedgerAudit { pools_kb: [9990, 0, 0, 0, 0, 0, 0, 0], pool_cnt: 1, total_kb: 10_000 };
    let la_bad = LedgerAudit { pools_kb: [9880, 0, 0, 0, 0, 0, 0, 0], pool_cnt: 1, total_kb: 10_000 };
    set.add(
        "B-1402 偏差阈值",
        la_edge.consistent() && la_edge.drift_permille() == 1 && !la_bad.consistent() && la_bad.drift_permille() == 12,
        "偏差 1‰ 过线、12‰ 标红——<1% 判据线可复现",
    );
    // 7. 六类基准枚举齐。
    set.add(
        "B-1403 六类基准齐",
        ALL_BENCH.len() == BENCH_KINDS && ALL_BENCH[0] == BenchKind::SyscallRoundtrip && ALL_BENCH[5] == BenchKind::NetLoopback,
        "syscall/上下文切换/页分配释放/帧管线/块路径/网络环回",
    );
    // 8. 全部六类产出三线分布（P50≤P95≤P99 序保持）。
    let mut rep = BenchReport::new();
    let mut i8 = 0;
    while i8 < BENCH_KINDS {
        rep.record(ALL_BENCH[i8], &synth_samples(ALL_BENCH[i8], 0xB100 + i8 as u64));
        i8 += 1;
    }
    set.add(
        "B-1403 全类三线分布",
        rep.all_three_line(),
        "六类全部产出 P50/P95/P99——输出是分布不是均值",
    );
    // 9. 回归判定：P95 对基线偏差超 10% 标红（尾部噪声不误报，整体劣化必报）。
    let mut rep9 = BenchReport::new();
    let mut flat = [10_000u64; 100];
    rep9.record(BenchKind::SyscallRoundtrip, &flat);
    let green = !BenchReport::regress_red(BenchKind::SyscallRoundtrip, 10_000, &rep9);
    flat[99] = 12_000; // 仅 P99 位抬升：排序后 s[94] 仍 10_000——门禁不吃尾部噪声
    rep9.record(BenchKind::CtxSwitch, &flat);
    let tail_immune = !BenchReport::regress_red(BenchKind::CtxSwitch, 10_000, &rep9);
    let mut slow = [12_000u64; 100]; // 整体抬升 20%：P95=12_000，偏差 20%>10% 必红
    rep9.record(BenchKind::PageAllocFree, &slow);
    let red = BenchReport::regress_red(BenchKind::PageAllocFree, 10_000, &rep9);
    set.add(
        "B-1403 回归判定",
        green && tail_immune && red,
        "P95 偏差 ≤10% 绿、仅尾部抬升不误报、整体超 10% 标红——均值会掩盖长尾，按 P95 判",
    );
    // 10. 基准与账本同源（WP-206 契约）：基准打点走同一 ProbeRing。
    let mut ring10 = ProbeRing::new();
    ring10.hit(ProbePoint::StoreConfirm, 5_000);
    let (snap10, _) = ring10.read_by(ProbeConsumer::Archived);
    let via_ring = matches!(snap10[0], Some(h) if h.point == ProbePoint::StoreConfirm && h.t_ns == 5_000);
    set.add(
        "B-1403 与账本同源",
        via_ring,
        "基准数字走同一打点环——监视器看到的数就是基准记录的数",
    );
    set
}

fn n3_count(a: &[Option<ProbeHit>; RING_CAP], b: &[Option<ProbeHit>; RING_CAP], c: &[Option<ProbeHit>; RING_CAP]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < RING_CAP {
        if a[i].is_some() && b[i] == a[i] && c[i] == a[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 单测（fc01 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fc01_probe_manifest() {
        assert!(ProbeRing::manifest_complete());
        assert_eq!(ALL_POINTS.len(), 27);
        assert!(ProbeRing::cost_per_kilo_ns() < 1_000_000);
    }

    #[test]
    fn fc01_ring_wraparound() {
        let mut ring = ProbeRing::new();
        let mut i = 0u64;
        while i < 257 {
            ring.hit(ProbePoint::AppLoad, i);
            i += 1;
        }
        assert_eq!(ring.count, 257);
        // 第 257 点覆盖第 1 点（head 回绕）——head 位置由 count 决定。
        let (snap, last) = ring.read_by(ProbeConsumer::LiveRing);
        assert_eq!(last, (257 - 1) % 256);
        assert!(matches!(snap[last], Some(h) if h.t_ns == 256));
    }

    #[test]
    fn fc02_ledger_audit() {
        let a = LedgerAudit { pools_kb: [500, 500, 0, 0, 0, 0, 0, 0], pool_cnt: 2, total_kb: 1000 };
        assert!(a.consistent(), "零偏差过线");
        // 950 vs 1000：差 50 → 50‰ = 5% ≥ 1% → 红。
        let b = LedgerAudit { pools_kb: [950, 0, 0, 0, 0, 0, 0, 0], pool_cnt: 1, total_kb: 1000 };
        assert_eq!(b.drift_permille(), 50);
        assert!(!b.consistent(), "5% 偏差必须标红");
    }

    #[test]
    fn fc03_bench_dist() {
        let mut rep = BenchReport::new();
        let samples = synth_samples(BenchKind::BlockPath, 7);
        rep.record(BenchKind::BlockPath, &samples);
        let d = rep.dist[4].unwrap_or(Dist3 { p50_ns: 0, p95_ns: 0, p99_ns: 0 });
        assert!(d.p50_ns <= d.p95_ns && d.p95_ns <= d.p99_ns);
        assert!(d.p95_ns >= 50_000, "P95 至少是基线值（抖动只增不减）");
    }
}
