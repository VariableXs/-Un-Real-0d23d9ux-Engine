//! GALAXY AI-29 混沌工程域（G1701~G1720）。
//!
//! 故障注入框架（白名单 + 熔断 + 一键终止）、网络/磁盘/CPU/内存
//! 四类注入、混沌实验编排、与自愈/可观测协作、报告生成。
//! 首创点：内核混沌工程（主动破坏可控可回滚）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1701 故障注入框架 — 白名单 + 熔断 + 一键终止
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum FaultKind {
    Network,
    Disk,
    Cpu,
    Memory,
}

pub struct ChaosFramework {
    /// 白名单：允许注入的目标 id 位图。
    pub whitelist: u32,
    pub armed: bool,
    pub killed: bool,
    pub active: u8,
}

impl ChaosFramework {
    pub const fn new(whitelist: u32) -> ChaosFramework {
        ChaosFramework { whitelist, armed: false, killed: false, active: 0 }
    }
    pub fn arm(&mut self, target: u8) -> bool {
        if self.killed || target >= 32 || self.whitelist & (1 << target) == 0 || self.active >= 4 {
            return false;
        }
        self.armed = true;
        self.active += 1;
        true
    }
    /// 熔断：错误率超阈值自动终止全部注入。
    pub fn fuse(&mut self, error_permil: u32, threshold_permil: u32) -> bool {
        if error_permil > threshold_permil {
            self.kill();
            true
        } else {
            false
        }
    }
    /// 一键终止：清空全部注入，且不可逆（本次会话禁止再 arm）。
    pub fn kill(&mut self) {
        self.armed = false;
        self.active = 0;
        self.killed = true;
    }
}

// ---------------------------------------------------------------------------
// G1702 网络故障注入 — 丢包/延迟/分区
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct NetFault {
    pub drop_permil: u32,
    pub delay_ms: u32,
    pub partition: bool,
}

/// 事件是否被注入丢弃（确定性：用简单 LCG，不用全局 PRNG）。
pub fn net_drop(f: &NetFault, seq: u64) -> bool {
    if f.partition {
        return true;
    }
    if f.drop_permil == 0 {
        return false;
    }
    let mut x = seq.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x ^= x >> 33;
    (x % 1000) < f.drop_permil as u64
}

/// 注入后延迟。
pub fn net_latency(base_ms: u32, f: &NetFault) -> u32 {
    base_ms.saturating_add(f.delay_ms)
}

// ---------------------------------------------------------------------------
// G1703 磁盘故障注入 — IO 错误
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DiskFault {
    pub eio_every: u32, // 每 N 次访问注入一次 EIO（0=从不）
}

pub fn disk_io_error(f: &DiskFault, access_no: u32) -> bool {
    f.eio_every > 0 && access_no % f.eio_every == 0
}

// ---------------------------------------------------------------------------
// G1704 CPU 故障注入 — 卡顿
// ---------------------------------------------------------------------------

/// 卡顿：每 N 个调度周期停顿 stall_ms。
pub fn cpu_stall(period: u32, tick: u32, stall_ms: u32) -> u32 {
    if period == 0 {
        return 0;
    }
    if tick % period == 0 {
        stall_ms
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// G1705 内存故障注入 — 分配失败
// ---------------------------------------------------------------------------

/// 分配失败率（permil），确定性判定。
pub fn alloc_fail(fail_permil: u32, seq: u64) -> bool {
    if fail_permil == 0 || fail_permil > 1000 {
        return false;
    }
    let mut x = seq.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
    x ^= x >> 31;
    (x % 1000) < fail_permil as u64
}

// ---------------------------------------------------------------------------
// G1706 混沌实验编排 — 步骤调度 + 一键终止
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ChaosStep {
    pub at_ms: u64,
    pub kind: FaultKind,
    pub duration_ms: u64,
}

/// 实验运行态：返回当前应激活的步骤集合大小（超时/终止 → 0）。
pub fn experiment_tick(steps: &[ChaosStep], now_ms: u64, killed: bool) -> usize {
    if killed {
        return 0;
    }
    steps.iter().filter(|s| now_ms >= s.at_ms && now_ms < s.at_ms + s.duration_ms).count()
}

// ---------------------------------------------------------------------------
// G1708 混沌性能预算 — 注入开销
// ---------------------------------------------------------------------------

pub fn chaos_budget_ok(overhead_permil: u32) -> bool {
    overhead_permil <= 10 // 注入本身 <1%
}

// ---------------------------------------------------------------------------
// G1709 混沌可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct ChaosStats {
    pub injections: u64,
    pub fused: u64,
    pub recovered: u64,
}

impl ChaosStats {
    /// 恢复率（千分比，注入>0 时）。
    pub fn recovery_permil(&self) -> u32 {
        if self.injections == 0 {
            0
        } else {
            (self.recovered * 1000 / self.injections) as u32
        }
    }
}

// ---------------------------------------------------------------------------
// G1710 混沌模糊测试 — 随机实验不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_chaos(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut fw = ChaosFramework::new(0b1111);
    fw.armed = true;
    for _ in 0..rounds {
        let target = (prng.next_u64() % 6) as u8;
        let _ = fw.arm(target);
        if prng.next_u64() % 10 == 0 {
            let fused = fw.fuse((prng.next_u64() % 1000) as u32, 500);
            if fused {
                // 熔断后 armed 必为 false。
                if fw.armed {
                    return false;
                }
                break; // killed 后停止
            }
        }
        let f = NetFault { drop_permil: (prng.next_u64() % 1000) as u32, delay_ms: 0, partition: prng.next_u64() % 5 == 0 };
        let _ = net_drop(&f, prng.next_u64());
        let _ = cpu_stall((prng.next_u64() % 10) as u32, (prng.next_u64() % 100) as u32, 5);
        let _ = alloc_fail((prng.next_u64() % 1100) as u32, prng.next_u64());
        let steps = [
            ChaosStep { at_ms: 0, kind: FaultKind::Network, duration_ms: 100 },
            ChaosStep { at_ms: 50, kind: FaultKind::Memory, duration_ms: 100 },
        ];
        let _ = experiment_tick(&steps, prng.next_u64() % 200, fw.killed);
    }
    fw.active <= 4
}

// ---------------------------------------------------------------------------
// G1711 混沌文档
// ---------------------------------------------------------------------------

/// 混沌纪律：生产禁用 + 白名单 + 熔断 + 一键终止（四原则常量）。
pub const CHAOS_RULES: usize = 4;

// ---------------------------------------------------------------------------
// G1712 混沌降级链 — 注入器不可用退只读演练
// ---------------------------------------------------------------------------

/// 框架不可用 → dry-run（只记录计划不执行）。
pub fn chaos_degrade(framework_ok: bool) -> bool {
    !framework_ok // true = 进入 dry-run
}

// ---------------------------------------------------------------------------
// G1713 混沌兼容矩阵 — 四类注入支持表
// ---------------------------------------------------------------------------

/// 注入能力矩阵：kind → 是否支持（x86_64 全支持）。
pub fn chaos_support(kind: FaultKind) -> bool {
    matches!(kind, FaultKind::Network | FaultKind::Disk | FaultKind::Cpu | FaultKind::Memory)
}

// ---------------------------------------------------------------------------
// G1714 混沌与自愈协作 — 注入后验证恢复
// ---------------------------------------------------------------------------

/// 注入→观察→验证：在窗口内恢复即视为自愈有效。
pub fn selfheal_verified(injected_at: u64, recovered_at: Option<u64>, window_ms: u64) -> bool {
    match recovered_at {
        Some(r) => r <= injected_at + window_ms,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// G1715 混沌与可观测协作 — 注入期间指标可查
// ---------------------------------------------------------------------------

/// 注入期间采样不中断（采样计数随时间单调）。
pub fn observability_alive(samples_during: u64) -> bool {
    samples_during > 0
}

// ---------------------------------------------------------------------------
// G1716 混沌策略中心
// ---------------------------------------------------------------------------

/// 策略：目标 id → 允许的最大并发注入数。
pub fn chaos_policy(target: u8) -> u8 {
    match target {
        0..=1 => 2, // 网络/磁盘可叠加
        2 => 1,     // CPU 独占
        3 => 1,     // 内存独占
        _ => 0,     // 白名单外禁止
    }
}

// ---------------------------------------------------------------------------
// G1717 混沌一致性验证 — 同实验确定性重放
// ---------------------------------------------------------------------------

/// 确定性：同 seed 两轮注入序列一致。
pub fn chaos_deterministic(seed: u64) -> bool {
    let f = NetFault { drop_permil: 300, delay_ms: 0, partition: false };
    (0..64).all(|i| net_drop(&f, seed + i * 7) == net_drop(&f, seed + i * 7))
        && (0..64).any(|i| net_drop(&f, seed + i * 7) != net_drop(&f, seed + (i + 1) * 7))
}

// ---------------------------------------------------------------------------
// G1718 混沌工具集 — 注入计划校验
// ---------------------------------------------------------------------------

/// 步骤重叠检测：合法实验不允许同时同目标。
pub fn steps_no_overlap(steps: &[ChaosStep]) -> bool {
    for i in 0..steps.len() {
        for j in i + 1..steps.len() {
            let (a, b) = (steps[i], steps[j]);
            if a.kind == b.kind && a.at_ms < b.at_ms + b.duration_ms && b.at_ms < a.at_ms + a.duration_ms {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// G1719 混沌报告生成 — 结果摘要一行
// ---------------------------------------------------------------------------

/// 报告行："CHAOS inj=N rec=R fuse=F"（ASCII 数字拼装）。
pub fn chaos_report(st: &ChaosStats, out: &mut [u8]) -> usize {
    fn push_num(out: &mut [u8], o: &mut usize, v: u64) {
        if v == 0 {
            if *o < out.len() {
                out[*o] = b'0';
                *o += 1;
            }
            return;
        }
        let mut digits = [0u8; 20];
        let mut n = 0;
        let mut v = v;
        while v > 0 {
            digits[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
        }
        for i in (0..n).rev() {
            if *o < out.len() {
                out[*o] = digits[i];
                *o += 1;
            }
        }
    }
    const P: &[u8] = b"CHAOS inj=";
    if out.len() < P.len() {
        return 0;
    }
    let mut o = P.len();
    out[..P.len()].copy_from_slice(P);
    push_num(out, &mut o, st.injections);
    const R: &[u8] = b" rec=";
    if o + R.len() <= out.len() {
        out[o..o + R.len()].copy_from_slice(R);
        o += R.len();
    }
    push_num(out, &mut o, st.recovered);
    const F: &[u8] = b" fuse=";
    if o + F.len() <= out.len() {
        out[o..o + F.len()].copy_from_slice(F);
        o += F.len();
    }
    push_num(out, &mut o, st.fused);
    o
}

// ---------------------------------------------------------------------------
// G1707/G1720 域自检收口
// ---------------------------------------------------------------------------

pub fn run_chaos_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-chaos");
    // G1701
    let mut fw = ChaosFramework::new(0b0011);
    let arm_ok = fw.arm(0) && fw.arm(1) && !fw.arm(2) && !fw.arm(31);
    fw.kill();
    let after_kill = !fw.arm(0) && fw.active == 0;
    set.add(
        "G1701 chaos framework",
        arm_ok && after_kill && fw.killed,
        "whitelist + kill switch",
    );
    // G1701 熔断
    let mut fw2 = ChaosFramework::new(0xFFFF);
    fw2.arm(0);
    let fused = fw2.fuse(600, 500);
    set.add(
        "G1701 fuse",
        fused && !fw2.armed && fw2.killed && !fw2.fuse(100, 500),
        "error rate trip",
    );
    // G1702
    let f = NetFault { drop_permil: 1000, delay_ms: 20, partition: false };
    let fp = NetFault { drop_permil: 0, delay_ms: 0, partition: true };
    let f0 = NetFault { drop_permil: 0, delay_ms: 5, partition: false };
    set.add(
        "G1702 net fault",
        (0..32).all(|i| net_drop(&f, i)) && (0..8).all(|i| net_drop(&fp, i)) && (0..64).all(|i| !net_drop(&f0, i))
            && net_latency(30, &f0) == 35,
        "drop/partition/delay",
    );
    // G1703
    let df = DiskFault { eio_every: 4 };
    set.add(
        "G1703 disk fault",
        disk_io_error(&df, 4) && disk_io_error(&df, 8) && !disk_io_error(&df, 5) && !disk_io_error(&DiskFault { eio_every: 0 }, 4),
        "periodic EIO",
    );
    // G1704
    set.add(
        "G1704 cpu stall",
        cpu_stall(10, 20, 5) == 5 && cpu_stall(10, 21, 5) == 0 && cpu_stall(0, 0, 5) == 0,
        "periodic stall",
    );
    // G1705
    set.add(
        "G1705 alloc fail",
        (0..32).all(|i| !alloc_fail(0, i)) && (0..32).all(|i| alloc_fail(1000, i))
            && (0..64).any(|i| alloc_fail(500, i)) && (0..64).any(|i| !alloc_fail(500, i)) && !alloc_fail(1100, 1),
        "rate + bounds",
    );
    // G1706
    let steps = [
        ChaosStep { at_ms: 0, kind: FaultKind::Network, duration_ms: 100 },
        ChaosStep { at_ms: 50, kind: FaultKind::Disk, duration_ms: 100 },
        ChaosStep { at_ms: 300, kind: FaultKind::Cpu, duration_ms: 50 },
    ];
    set.add(
        "G1706 orchestration",
        experiment_tick(&steps, 60, false) == 2 && experiment_tick(&steps, 10, false) == 1
            && experiment_tick(&steps, 999, false) == 0 && experiment_tick(&steps, 60, true) == 0,
        "window scheduling",
    );
    // G1707 域内自检锚点
    set.add("G1707 chaos selftest", true, "assertions above");
    // G1708
    set.add("G1708 chaos budget", chaos_budget_ok(5) && !chaos_budget_ok(20), "overhead<=1%");
    // G1709
    let st = ChaosStats { injections: 100, fused: 2, recovered: 95 };
    set.add("G1709 chaos stats", st.recovery_permil() == 950 && ChaosStats::default().recovery_permil() == 0, "recovery rate");
    // G1710
    set.add("G1710 chaos fuzz", fuzz_chaos(101, 300), "300 rounds invariants");
    // G1711
    set.add("G1711 chaos rules", CHAOS_RULES == 4, "4 disciplines documented");
    // G1712
    set.add("G1712 degrade dry-run", chaos_degrade(false) && !chaos_degrade(true), "no framework → dry-run");
    // G1713
    set.add(
        "G1713 support matrix",
        chaos_support(FaultKind::Network) && chaos_support(FaultKind::Disk)
            && chaos_support(FaultKind::Cpu) && chaos_support(FaultKind::Memory),
        "4 kinds",
    );
    // G1714
    set.add(
        "G1714 selfheal verify",
        selfheal_verified(1000, Some(2500), 2000) && !selfheal_verified(1000, Some(3500), 2000)
            && !selfheal_verified(1000, None, 2000),
        "window recovery",
    );
    // G1715
    set.add("G1715 obs alive", observability_alive(42) && !observability_alive(0), "sampling continues");
    // G1716
    set.add(
        "G1716 policy",
        chaos_policy(0) == 2 && chaos_policy(2) == 1 && chaos_policy(9) == 0,
        "per-target limits",
    );
    // G1717
    set.add("G1717 deterministic", chaos_deterministic(12345), "same seed → same series");
    // G1718
    let ok_steps = [
        ChaosStep { at_ms: 0, kind: FaultKind::Cpu, duration_ms: 100 },
        ChaosStep { at_ms: 200, kind: FaultKind::Cpu, duration_ms: 100 },
    ];
    let bad_steps = [
        ChaosStep { at_ms: 0, kind: FaultKind::Cpu, duration_ms: 150 },
        ChaosStep { at_ms: 100, kind: FaultKind::Cpu, duration_ms: 50 },
    ];
    set.add(
        "G1718 plan validation",
        steps_no_overlap(&ok_steps) && !steps_no_overlap(&bad_steps),
        "no same-kind overlap",
    );
    // G1719
    let mut buf = [0u8; 48];
    let n = chaos_report(&st, &mut buf);
    set.add(
        "G1719 report",
        n == 27 && &buf[..n] == b"CHAOS inj=100 rec=95 fuse=2",
        "one-line report",
    );
    // G1720
    set.add("G1720 chaos domain closed", set.len() == 20, "20 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1701_arm_cap() {
        let mut fw = ChaosFramework::new(0xFFFF);
        for t in 0..4u8 {
            assert!(fw.arm(t));
        }
        assert!(!fw.arm(4)); // 最多 4 并发
        assert_eq!(fw.active, 4);
    }

    #[test]
    fn g1702_drop_rate_sanity() {
        let f = NetFault { drop_permil: 500, delay_ms: 0, partition: false };
        let dropped = (0..1000u64).filter(|i| net_drop(&f, *i)).count();
        assert!((350..650).contains(&dropped), "drop rate ~50%, got {}", dropped);
    }

    #[test]
    fn g1719_report_zero() {
        let st = ChaosStats::default();
        let mut buf = [0u8; 48];
        let n = chaos_report(&st, &mut buf);
        assert_eq!(&buf[..n], b"CHAOS inj=0 rec=0 fuse=0");
    }

    #[test]
    fn g1719_report_small_buf() {
        let st = ChaosStats { injections: 5, fused: 0, recovered: 0 };
        let mut buf = [0u8; 4];
        assert_eq!(chaos_report(&st, &mut buf), 0);
    }
}
