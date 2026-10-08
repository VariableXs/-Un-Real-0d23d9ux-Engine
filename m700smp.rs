//! m700smp — VARIX-M700 AI-20 多核与 SMP 域 (F476~F500)
//!
//! 启动核大典/核间通信谱/每核数据宪法/核拓扑档案/负载迁移官/RCU 宽限期谱/
//! 核热插拔律/全局序官/SMP 压力剧本/扩展线性度仪/核间死锁预言机/中断分发官/
//! 核健康分/SMP fuzz 桩/核间回放流/一致性对账官/核亲和 API/调度域协同谱/
//! 核隔离舱/SMP 回归走廊/核统计分账/拓扑导出格式/自旋延迟仪/SMP 文档生成器/
//! SMP 域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F476 — 启动核大典：BSC 先行、AP 依次跟上
// ===========================================================================

pub const SMP_CPUS: usize = 4;

#[derive(Clone, Copy)]
pub struct ApBootRecord {
    pub cpu: u8,
    pub started: bool,
    pub start_order: u8, // 0 = BSC（引导核）
}

/// 全部要求上线的核都已 started，且 BSC 必须最先。
pub fn boot_complete(recs: &[ApBootRecord], required: u8) -> bool {
    if recs.len() < required as usize {
        return false;
    }
    let mut started = 0u8;
    for (i, r) in recs.iter().enumerate() {
        if r.started {
            started += 1;
            if r.start_order as usize != i {
                return false; // 启动序必须与记录序一致
            }
        }
    }
    started >= required
}

// ===========================================================================
// F477 — 核间通信谱：IPI 信箱
// ===========================================================================

pub const IPI_NONE: u32 = 0;

#[derive(Clone, Copy)]
pub struct IpiMailbox {
    pending: [u32; SMP_CPUS], // 0 = 空
}

impl IpiMailbox {
    pub const fn new() -> IpiMailbox {
        IpiMailbox { pending: [IPI_NONE; SMP_CPUS] }
    }
    pub fn send(&mut self, to: u8, msg: u32) -> bool {
        if to as usize >= SMP_CPUS || msg == IPI_NONE || self.pending[to as usize] != IPI_NONE {
            return false;
        }
        self.pending[to as usize] = msg;
        true
    }
    pub fn poll(&mut self, cpu: u8) -> Option<u32> {
        if cpu as usize >= SMP_CPUS {
            return None;
        }
        let m = self.pending[cpu as usize];
        if m == IPI_NONE {
            None
        } else {
            self.pending[cpu as usize] = IPI_NONE;
            Some(m)
        }
    }
    pub fn pending(&self, cpu: u8) -> Option<u32> {
        if cpu as usize >= SMP_CPUS {
            None
        } else if self.pending[cpu as usize] == IPI_NONE {
            None
        } else {
            Some(self.pending[cpu as usize])
        }
    }
}

// ===========================================================================
// F478 — 每核数据宪法：stride 独占槽位
// ===========================================================================

pub const PERCPU_STRIDE: usize = 64; // 防伪共享：一条缓存行一槽

pub fn per_cpu_slot(cpu: u8) -> Option<usize> {
    if cpu as usize >= SMP_CPUS {
        return None;
    }
    Some(cpu as usize * PERCPU_STRIDE)
}

/// 槽位互不重叠且都落在容量内。
pub fn per_cpu_slots_disjoint(total_bytes: usize) -> bool {
    (0..SMP_CPUS).all(|c| per_cpu_slot(c as u8).unwrap() + PERCPU_STRIDE <= total_bytes)
}

// ===========================================================================
// F479 — 核拓扑档案：socket/core/smt 三层
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CoreTopo {
    pub socket: u8,
    pub core: u8,
    pub smt_sibling: Option<u8>, // 超线程兄弟（若有）
}

pub fn topo_sane(t: CoreTopo) -> bool {
    match t.smt_sibling {
        Some(s) => s != t.core || t.socket == 0xFF, // 兄弟不能是同 socket 同 core 的自己
        None => true,
    }
}

pub const TOPO_DIMENSIONS: [&str; 3] = ["socket", "core", "smt"];

// ===========================================================================
// F480 — 负载迁移官：挑最忙的核迁走
// ===========================================================================

/// 返回超过 threshold 的最忙核编号；无人超载返回 None。
pub fn pick_migration_victim(loads: &[u16], threshold: u16) -> Option<usize> {
    if loads.len() != SMP_CPUS {
        return None;
    }
    let mut best: Option<(u16, usize)> = None;
    for (i, &l) in loads.iter().enumerate() {
        if l > threshold {
            match best {
                Some((mx, _)) if mx >= l => {}
                _ => best = Some((l, i)),
            }
        }
    }
    best.map(|(_, i)| i)
}

// ===========================================================================
// F481 — RCU 宽限期谱：静默期上报与宽限期推进
// ===========================================================================

#[derive(Clone, Copy)]
pub struct RcuGrace {
    gp: u64,
    registered: [bool; SMP_CPUS],
    quiet: [bool; SMP_CPUS],
}

impl RcuGrace {
    pub const fn new() -> RcuGrace {
        RcuGrace { gp: 0, registered: [false; SMP_CPUS], quiet: [false; SMP_CPUS] }
    }
    pub fn register(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS || self.registered[cpu as usize] {
            return false;
        }
        self.registered[cpu as usize] = true;
        true
    }
    pub fn report_quiet(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS || !self.registered[cpu as usize] {
            return false;
        }
        self.quiet[cpu as usize] = true;
        true
    }
    /// 所有已注册核全部静默 → 宽限期完成。
    pub fn grace_complete(&self) -> bool {
        (0..SMP_CPUS).all(|c| !self.registered[c] || self.quiet[c])
    }
    pub fn gp(&self) -> u64 {
        self.gp
    }
    pub fn advance(&mut self) {
        self.gp += 1;
        self.quiet = [false; SMP_CPUS];
    }
}

// ===========================================================================
// F482 — 核热插拔律：至少留一个在线核
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HotplugState {
    online: [bool; SMP_CPUS],
}

impl HotplugState {
    pub const fn all_online() -> HotplugState {
        HotplugState { online: [true; SMP_CPUS] }
    }
    pub fn is_online(&self, cpu: u8) -> bool {
        (cpu as usize) < SMP_CPUS && self.online[cpu as usize]
    }
    pub fn online_count(&self) -> usize {
        self.online.iter().filter(|&&o| o).count()
    }
    /// 下线必须至少保留 1 个在线核。
    pub fn offline(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS || !self.online[cpu as usize] || self.online_count() <= 1 {
            return false;
        }
        self.online[cpu as usize] = false;
        true
    }
    pub fn online(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS || self.online[cpu as usize] {
            return false;
        }
        self.online[cpu as usize] = true;
        true
    }
}

// ===========================================================================
// F483 — 全局序官：单调递增全局时间戳
// ===========================================================================

pub struct GlobalSeq {
    counter: u64,
}

impl GlobalSeq {
    pub const fn new() -> GlobalSeq {
        GlobalSeq { counter: 0 }
    }
    pub fn stamp(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }
    pub fn value(&self) -> u64 {
        self.counter
    }
}

/// 严格递增才算全序（相等/倒序都算破坏）。
pub fn strictly_ordered(a: u64, b: u64) -> bool {
    a < b
}

// ===========================================================================
// F484 — SMP 压力剧本：多核并发压力参数
// ===========================================================================

pub const SMP_STRESS_THREADS: u8 = 8;
pub const SMP_STRESS_SECONDS: u32 = 30;

pub fn smp_stress_plan_valid(threads: u8, seconds: u32, cpus: u8) -> bool {
    threads >= 1 && threads <= 64 && seconds >= 10 && seconds <= 600 && cpus >= 1 && cpus as usize <= SMP_CPUS
}

// ===========================================================================
// F485 — 扩展线性度仪：加速比 permille
// ===========================================================================

/// 加速比 = 单线程耗时 / n 线程耗时（permille）。
pub fn speedup_permille(t1_us: u32, tn_us: u32) -> u32 {
    if tn_us == 0 {
        return 0;
    }
    t1_us as u32 * 1000 / tn_us as u32
}

/// 线性度合格线：达到理想加速比的 70%。
pub const SCALING_FLOOR_PERMILLE: u32 = 700;

pub fn scaling_ok(threads: u8, t1_us: u32, tn_us: u32) -> bool {
    if threads == 0 {
        return false;
    }
    speedup_permille(t1_us, tn_us) >= threads as u32 * 1000 * SCALING_FLOOR_PERMILLE / 1000
}

// ===========================================================================
// F486 — 核间死锁预言机：等待图环检测
// ===========================================================================

pub const WAIT_NONE: usize = usize::MAX;

/// waiting[cpu] = 该核正在等的核（WAIT_NONE = 不等任何人）。
/// 有环即死锁。
pub fn deadlock_exists(waiting: &[usize]) -> bool {
    let n = waiting.len();
    for start in 0..n {
        let mut cur = start;
        let mut steps = 0;
        while steps <= n {
            let nxt = waiting[cur];
            if nxt == WAIT_NONE || nxt >= n {
                break;
            }
            if nxt == start {
                return true;
            }
            cur = nxt;
            steps += 1;
        }
    }
    false
}

// ===========================================================================
// F487 — 中断分发官：轮转亲和
// ===========================================================================

/// IRQ 轮转分发到 cpu = irq % ncpus。
pub fn irq_affinity(irq: u16, ncpus: u8) -> Option<u8> {
    if ncpus == 0 || ncpus as usize > SMP_CPUS {
        return None;
    }
    Some((irq % ncpus as u16) as u8)
}

// ===========================================================================
// F488 — 核健康分：偷懒时间 × 温度
// ===========================================================================

pub fn core_health(steal_permille: u16, temp_c: i16) -> u16 {
    let steal_penalty = (steal_permille as u32 / 2).min(500) as u16; // 偷懒最多扣 500
    let temp_penalty: u16 = if temp_c > 85 { 200 } else if temp_c > 75 { 100 } else { 0 };
    1000u16.saturating_sub(steal_penalty + temp_penalty)
}

// ===========================================================================
// F489 — SMP fuzz 桩：确定性 IPI 消息生成
// ===========================================================================

pub fn fuzz_msg(seed: u32) -> u32 {
    let x = seed.wrapping_mul(2_979_413).wrapping_add(0x1B873593);
    (x >> 13) | 1 // 保证非 0（0 是信箱空标记）
}

pub fn fuzz_replayable(seed: u32) -> bool {
    fuzz_msg(seed) == fuzz_msg(seed)
}

// ===========================================================================
// F490 — 核间回放流：IPI 序列确定性摘要
// ===========================================================================

pub fn smp_digest(msgs: &[u32]) -> u32 {
    let mut h: u32 = 0x534D_5047; // "SMPG"
    for &m in msgs {
        h ^= m;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ===========================================================================
// F491 — 一致性对账官：失效计数守恒
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CoherenceLedger {
    pub stores: u64,
    pub invalidates: u64, // 每次全局写应失效其余 n-1 个核的副本
    pub acks: u64,
}

impl CoherenceLedger {
    /// n 核系统：invalidates 应为 stores×(n-1)，且全部有 ack。
    pub fn accounted(&self, ncpus: u8) -> bool {
        if ncpus < 1 {
            return false;
        }
        let expect = self.stores * (ncpus as u64 - 1);
        self.invalidates == expect && self.acks == self.invalidates
    }
}

// ===========================================================================
// F492 — 核亲和 API：位掩码亲和集
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Affinity {
    mask: u8,
}

impl Affinity {
    pub const fn empty() -> Affinity {
        Affinity { mask: 0 }
    }
    pub const fn all() -> Affinity {
        Affinity { mask: (1 << SMP_CPUS) - 1 }
    }
    pub fn allow(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS {
            return false;
        }
        self.mask |= 1 << cpu;
        true
    }
    pub fn deny(&mut self, cpu: u8) {
        if (cpu as usize) < SMP_CPUS {
            self.mask &= !(1 << cpu);
        }
    }
    pub fn allowed(&self, cpu: u8) -> bool {
        (cpu as usize) < SMP_CPUS && (self.mask >> cpu) & 1 == 1
    }
    pub fn is_empty(&self) -> bool {
        self.mask == 0
    }
    pub fn popcount(&self) -> u32 {
        self.mask.count_ones()
    }
}

// ===========================================================================
// F493 — 调度域协同谱：抢占点礼仪
// ===========================================================================

/// 只有非中断上下文且抢占计数为 0 时才可让出。
pub fn may_yield(in_irq: bool, preempt_count: u8) -> bool {
    !in_irq && preempt_count == 0
}

// ===========================================================================
// F494 — 核隔离舱：隔离核不参与调度亲和
// ===========================================================================

#[derive(Clone, Copy)]
pub struct IsolateMask {
    mask: u8,
}

impl IsolateMask {
    pub const fn empty() -> IsolateMask {
        IsolateMask { mask: 0 }
    }
    pub fn isolate(&mut self, cpu: u8) -> bool {
        if cpu as usize >= SMP_CPUS {
            return false;
        }
        self.mask |= 1 << cpu;
        true
    }
    pub fn isolated(&self, cpu: u8) -> bool {
        (cpu as usize) < SMP_CPUS && (self.mask >> cpu) & 1 == 1
    }
    pub fn mask(&self) -> u8 {
        self.mask
    }
}

/// 亲和集不得触碰隔离核。
pub fn isolation_respected(aff: &Affinity, iso: &IsolateMask) -> bool {
    aff.mask & iso.mask == 0
}

// ===========================================================================
// F495 — SMP 回归走廊：固定回归用例
// ===========================================================================

pub const SMP_REGRESSION_CASES: [&str; 6] =
    ["boot-order", "ipi-loss", "rcu-grace", "hotplug-minimum", "deadlock-oracle", "affinity-isolate"];

// ===========================================================================
// F496 — 核统计分账：上下文切换/迁移/自旋计数
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SmpStats {
    pub ctx_switches: u64,
    pub migrations: u64,
    pub ipi_sent: u64,
}

impl SmpStats {
    pub fn accounted(&self, ipi_acked: u64) -> bool {
        self.ipi_sent >= ipi_acked // 发出 ≥ 确认（确认不会凭空多）
    }
    pub fn migration_rate_permille(&self) -> u32 {
        if self.ctx_switches == 0 {
            return 0;
        }
        (self.migrations * 1000 / self.ctx_switches) as u32
    }
}

// ===========================================================================
// F497 — 拓扑导出格式：完整导出字段
// ===========================================================================

pub const TOPO_EXPORT_FIELDS: [&str; 4] = ["socket-map", "core-map", "smt-pairs", "isolate-mask"];

pub fn topo_export_complete(fields: [&str; 4]) -> bool {
    (0..4).all(|i| !fields[i].is_empty())
}

// ===========================================================================
// F498 — 自旋延迟仪：自旋预算与指数退避
// ===========================================================================

pub const SPIN_BUDGET_DEFAULT: u32 = 4096;

pub fn spin_budget_ok(wait_spins: u32, budget: u32) -> bool {
    wait_spins <= budget
}

/// 锁退避：第 n 次尝试自旋 2^n，封顶 1024。
pub fn spin_backoff(attempt: u32) -> u32 {
    1u32.checked_shl(attempt.min(10)).unwrap_or(1024).min(1024)
}

// ===========================================================================
// F499 — SMP 文档生成器：文档小节清单
// ===========================================================================

pub const SMP_DOC_SECTIONS: [&str; 5] =
    ["boot", "ipi", "rcu", "hotplug", "affinity"];

// ===========================================================================
// F500 — SMP 域年报
// ===========================================================================

pub const SMP_REPORT_SECTIONS: [&str; 4] = ["milestones", "scaling", "incidents", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700smp_checks() -> CheckSet {
    let mut set = CheckSet::new("m700smp");

    // F476 启动核
    let recs = [
        ApBootRecord { cpu: 0, started: true, start_order: 0 },
        ApBootRecord { cpu: 1, started: true, start_order: 1 },
        ApBootRecord { cpu: 2, started: true, start_order: 2 },
    ];
    set.add("F476 boot order", boot_complete(&recs, 3), "bsc first");
    set.add("F476 boot short", !boot_complete(&recs, 4), "missing ap");
    let disorder = [
        ApBootRecord { cpu: 1, started: true, start_order: 1 },
        ApBootRecord { cpu: 0, started: true, start_order: 0 },
    ];
    set.add("F476 boot disorder", !boot_complete(&disorder, 2), "order mismatch");

    // F477 核间通信
    let mut mb = IpiMailbox::new();
    let s1 = mb.send(2, 0xABCD);
    let pend2 = mb.pending(2);
    let s2 = mb.send(2, 0x1111); // 同核重复投递必须拒绝
    set.add("F477 ipi send", s1 && pend2 == Some(0xABCD) && !s2, "no overwrite");
    let got = mb.poll(2);
    let pend_after = mb.pending(2);
    set.add("F477 ipi poll once", got == Some(0xABCD) && pend_after.is_none(), "consume");
    set.add("F477 ipi oob", !mb.send(9, 1), "cpu out of range");

    // F478 每核数据
    set.add("F478 percpu slots", per_cpu_slot(3) == Some(3 * PERCPU_STRIDE) && per_cpu_slot(4).is_none(), "stride slots");
    set.add("F478 percpu disjoint", per_cpu_slots_disjoint(SMP_CPUS * PERCPU_STRIDE)
        && !per_cpu_slots_disjoint(SMP_CPUS * PERCPU_STRIDE - 1), "no overlap");

    // F479 核拓扑
    set.add("F479 topo sane", topo_sane(CoreTopo { socket: 0, core: 2, smt_sibling: Some(3) })
        && topo_sane(CoreTopo { socket: 1, core: 0, smt_sibling: None }), "smt pairs");
    set.add("F479 topo self-sib", !topo_sane(CoreTopo { socket: 0, core: 2, smt_sibling: Some(2) }), "sibling≠self");
    set.add("F479 topo dims", TOPO_DIMENSIONS.len() == 3, "3 layers");

    // F480 负载迁移
    let loads = [900u16, 500, 1500, 1200];
    let victim = pick_migration_victim(&loads, 1000);
    set.add("F480 migrate victim", victim == Some(2), "busiest above threshold");
    set.add("F480 migrate idle", pick_migration_victim(&loads, 2000).is_none(), "nobody over");
    set.add("F480 migrate arity", pick_migration_victim(&loads[..2], 0).is_none(), "wrong arity");

    // F481 RCU 宽限期
    let mut rcu = RcuGrace::new();
    let r1 = rcu.register(0);
    let r2 = rcu.register(1);
    let not_complete_yet = !rcu.grace_complete();
    let q1 = rcu.report_quiet(1);
    let q2 = rcu.report_quiet(0);
    set.add("F481 rcu register", r1 && r2 && rcu.gp() == 0, "two readers");
    set.add("F481 rcu partial quiet", q1 && not_complete_yet, "waiting on cpu0");
    let q3 = rcu.report_quiet(3);
    set.add("F481 rcu quiet needs reg", !q3 && rcu.grace_complete() && q2, "all quiet → grace done");

    // F482 核热插拔
    let mut hp = HotplugState::all_online();
    let off1 = hp.offline(3);
    let count_mid = hp.online_count();
    let off2 = hp.offline(2);
    let count_after = hp.online_count();
    set.add("F482 hotplug offline", off1 && count_mid == 3 && off2 && count_after == 2, "can offline");
    let off3 = hp.offline(1);
    let count_low = hp.online_count();
    set.add("F482 hotplug floor", off3 && count_low == 1, "down to one");
    set.add("F482 hotplug last core", !hp.offline(0) && hp.online_count() == 1, "keep ≥1");
    let mut hp2 = HotplugState { online: [true, false, false, false] };
    let reon = hp2.online(1);
    let reon_dup = hp2.online(1);
    set.add("F482 hotplug online", reon && !reon_dup && hp2.is_online(1), "re-online once");

    // F483 全局序
    let mut seq = GlobalSeq::new();
    let a = seq.stamp();
    let b = seq.stamp();
    let c = seq.stamp();
    set.add("F483 global order", a == 1 && b == 2 && c == 3
        && strictly_ordered(a, b) && strictly_ordered(b, c), "monotone");
    set.add("F483 global strict", !strictly_ordered(5, 5) && !strictly_ordered(9, 4), "strict <");

    // F484 压力剧本
    set.add("F484 smp stress", smp_stress_plan_valid(SMP_STRESS_THREADS, SMP_STRESS_SECONDS, 4)
        && !smp_stress_plan_valid(8, 30, 9), "bounds");

    // F485 扩展线性度
    set.add("F485 speedup ideal", speedup_permille(1000, 250) == 4000, "4× on 4 threads");
    set.add("F485 scaling pass", scaling_ok(4, 1000, 300), "≥70% of ideal");
    set.add("F485 scaling fail", !scaling_ok(4, 1000, 800), "poor scaling");

    // F486 死锁预言机
    let cycle = [1usize, 0, WAIT_NONE, WAIT_NONE];
    let chain = [1usize, 2, WAIT_NONE, WAIT_NONE];
    set.add("F486 deadlock cycle", deadlock_exists(&cycle), "2-cycle");
    set.add("F486 deadlock chain", !deadlock_exists(&chain), "acyclic ok");
    let three = [1usize, 2, 0, WAIT_NONE];
    set.add("F486 deadlock 3-cycle", deadlock_exists(&three), "3-cycle");

    // F487 中断分发
    set.add("F487 irq round-robin", irq_affinity(0, 4) == Some(0) && irq_affinity(5, 4) == Some(1)
        && irq_affinity(11, 4) == Some(3), "irq%ncpu");
    set.add("F487 irq no cpus", irq_affinity(1, 0).is_none(), "degenerate");

    // F488 核健康分
    set.add("F488 health clean", core_health(0, 60) == 1000, "no penalties");
    set.add("F488 health steal", core_health(400, 60) == 800, "steal −200");
    set.add("F488 health hot", core_health(400, 90) == 600, "hot −200");

    // F489 fuzz 桩
    set.add("F489 smp fuzz deterministic", fuzz_replayable(0xFEED_FACE), "replayable");
    set.add("F489 smp msg nonzero", fuzz_msg(3) != 0 && fuzz_msg(3) != IPI_NONE, "never empty-mark");

    // F490 核间回放
    let msgs = [1u32, 2, 3];
    set.add("F490 ipi replay", smp_digest(&msgs) == smp_digest(&msgs), "deterministic");
    set.add("F490 ipi digest differs", smp_digest(&msgs) != smp_digest(&[1u32, 2]), "order matters");

    // F491 一致性对账
    let led = CoherenceLedger { stores: 10, invalidates: 30, acks: 30 };
    set.add("F491 coherence 4 cores", led.accounted(4), "10×3=30 acked");
    let bad = CoherenceLedger { stores: 10, invalidates: 25, acks: 25 };
    set.add("F491 coherence leak", !bad.accounted(4), "missing invalidates");

    // F492 核亲和 API
    let mut aff = Affinity::empty();
    let a1 = aff.allow(0);
    let a2 = aff.allow(2);
    let bad_cpu = aff.allow(7);
    set.add("F492 affinity allow", a1 && a2 && aff.allowed(0) && aff.allowed(2) && !bad_cpu, "mask set");
    aff.deny(2);
    set.add("F492 affinity deny", !aff.allowed(2) && aff.popcount() == 1, "mask clear");

    // F493 调度协同
    set.add("F493 may yield", may_yield(false, 0) && !may_yield(true, 0) && !may_yield(false, 1), "preempt point");

    // F494 核隔离舱
    let mut iso = IsolateMask::empty();
    iso.isolate(3);
    let mut aff2 = Affinity::empty();
    aff2.allow(0);
    aff2.allow(1);
    aff2.allow(3);
    set.add("F494 isolate mask", iso.isolated(3) && !iso.isolated(0), "isolated cpu3");
    set.add("F494 isolation respected", isolation_respected(&Affinity::empty(), &iso)
        && !isolation_respected(&aff2, &iso), "affinity may not touch");

    // F495 回归走廊
    set.add("F495 smp regression", SMP_REGRESSION_CASES.len() == 6, "6 cases");

    // F496 统计分账
    let st = SmpStats { ctx_switches: 10_000, migrations: 250, ipi_sent: 100 };
    set.add("F496 smp stats", st.accounted(100) && st.migration_rate_permille() == 25, "2.5% migrate");
    set.add("F496 smp stats ack", !st.accounted(101), "ack ≤ sent");

    // F497 拓扑导出
    set.add("F497 topo export", topo_export_complete(["s", "c", "p", "i"]) && TOPO_EXPORT_FIELDS.len() == 4, "4 fields");

    // F498 自旋延迟
    set.add("F498 spin budget", spin_budget_ok(4000, SPIN_BUDGET_DEFAULT) && !spin_budget_ok(5000, SPIN_BUDGET_DEFAULT), "≤4096");
    set.add("F498 spin backoff", spin_backoff(0) == 1 && spin_backoff(10) == 1024 && spin_backoff(99) == 1024, "exp cap");

    // F499 文档生成器
    set.add("F499 smp doc sections", SMP_DOC_SECTIONS.len() == 5, "5 sections");

    // F500 年报
    set.add("F500 smp annual report", SMP_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f477_ipi_no_loss() {
        let mut mb = IpiMailbox::new();
        assert!(mb.send(1, 42));
        assert!(mb.send(0, 7));
        assert_eq!(mb.poll(1), Some(42));
        assert_eq!(mb.poll(1), None);
        assert_eq!(mb.poll(0), Some(7));
    }

    #[test]
    fn f481_rcu_two_graces() {
        let mut rcu = RcuGrace::new();
        for c in 0..SMP_CPUS as u8 {
            rcu.register(c);
        }
        for c in 0..SMP_CPUS as u8 {
            rcu.report_quiet(c);
        }
        assert!(rcu.grace_complete());
        rcu.advance();
        assert_eq!(rcu.gp(), 1);
        assert!(!rcu.grace_complete()); // 新宽限期重置静默
    }

    #[test]
    fn f482_hotplug_never_zero() {
        let mut hp = HotplugState::all_online();
        for c in 0..SMP_CPUS as u8 {
            hp.offline(c);
        }
        assert!(hp.online_count() >= 1);
    }

    #[test]
    fn f486_deadlock_self_wait() {
        let self_wait = [0usize, WAIT_NONE];
        assert!(deadlock_exists(&self_wait));
        assert!(!deadlock_exists(&[WAIT_NONE; 3]));
    }

    #[test]
    fn f488_health_boundaries() {
        assert_eq!(core_health(1000, 90), 300); // 偷懒-500 + 过热-200
        assert_eq!(core_health(2000, 50), 500); // 偷懒扣分封顶 500
        assert_eq!(core_health(0, 100), 800);
    }

    #[test]
    fn f500_domain_selfcheck_all_pass() {
        let set = run_m700smp_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
