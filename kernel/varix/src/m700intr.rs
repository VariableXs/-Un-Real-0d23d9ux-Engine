//! m700intr — VARIX-M700 AI-04 中断与时钟域 (F076~F100)
//!
//! 中断向量大典/上下半场律/中断亲和图/时钟源仲裁庭/定时器轮谱/
//! 中断风暴阀/中断延迟仪/MSI/MSI-X 谱/中断嵌套律/定时器漂移考古/
//! 高精度定时器/中断记账簿/断言回归走廊/向量泄露纠察/中断风暴回放/
//! 时钟节拍裁缝/睡眠时钟协议/IPI 礼仪/中断负载画像/异常入口谱/
//! 定时器批量到期/中断源自描述/中断屏蔽恢复律/时钟精度档案/中断域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点/PPM）/ 纯逻辑。
//! 类型与常量统一加 `Intr` 前缀，避免与 crate 内 cpu/interrupt 既有符号撞名。

use crate::checks::CheckSet;

// ===========================================================================
// F076 — 中断向量大典：向量 32~255 可申领，owner 登记，先到先得
// ===========================================================================

/// 向量 0 号作空闲哨兵（向量 0 属异常入口，永不用于 IRQ）。
pub const INTR_VECTOR_FREE: u8 = 0;
pub const INTR_IRQ_VECTOR_BASE: u8 = 32;

#[derive(Clone, Copy, Debug)]
pub struct IntrVectorTable {
    owners: [u8; 256],
}

impl IntrVectorTable {
    pub const fn new() -> IntrVectorTable {
        IntrVectorTable { owners: [INTR_VECTOR_FREE; 256] }
    }

    /// 申领向量；异常区间/重复申领/零 owner 拒绝。
    pub fn claim(&mut self, vec: u8, owner: u8) -> bool {
        if vec < INTR_IRQ_VECTOR_BASE || owner == INTR_VECTOR_FREE {
            return false;
        }
        if self.owners[vec as usize] != INTR_VECTOR_FREE {
            return false;
        }
        self.owners[vec as usize] = owner;
        true
    }

    /// 释放向量；未申领/异常区间拒绝。
    pub fn release(&mut self, vec: u8) -> bool {
        if vec < INTR_IRQ_VECTOR_BASE || self.owners[vec as usize] == INTR_VECTOR_FREE {
            return false;
        }
        self.owners[vec as usize] = INTR_VECTOR_FREE;
        true
    }

    pub fn owner_of(&self, vec: u8) -> u8 {
        self.owners[vec as usize]
    }
}

// ===========================================================================
// F077 — 上下半场律：上半场超预算必须 defer 给下半场
// ===========================================================================

pub const INTR_TOP_BUDGET_CYCLES: u32 = 1000;

#[derive(Clone, Copy, Debug, Default)]
pub struct IntrDeferBook {
    pub handled_inline: u32,
    pub deferred: u32,
}

/// 上半场执行：预算内就地完成，超预算转下半场并记账。
pub fn run_top_half(cycles: u32, book: &mut IntrDeferBook) -> bool {
    if cycles <= INTR_TOP_BUDGET_CYCLES {
        book.handled_inline += 1;
        true
    } else {
        book.deferred += 1;
        false
    }
}

// ===========================================================================
// F078 — 中断亲和图：IRQ → CPU 绑定，重复绑定同一 CPU 拒绝
// ===========================================================================

pub const INTR_IRQ_LINES: usize = 16;
pub const INTR_CPU_COUNT: u8 = 8;
pub const INTR_AFFINITY_NONE: u8 = 0xFF;

#[derive(Clone, Copy, Debug)]
pub struct IntrAffinity {
    cpu_of: [u8; INTR_IRQ_LINES],
}

impl IntrAffinity {
    pub const fn new() -> IntrAffinity {
        IntrAffinity { cpu_of: [INTR_AFFINITY_NONE; INTR_IRQ_LINES] }
    }

    /// 绑定 IRQ 到 CPU；重复绑定/越界拒绝。
    pub fn set_affinity(&mut self, irq: usize, cpu: u8) -> bool {
        if irq >= INTR_IRQ_LINES || cpu >= INTR_CPU_COUNT {
            return false;
        }
        if self.cpu_of[irq] == cpu {
            return false;
        }
        self.cpu_of[irq] = cpu;
        true
    }

    pub fn cpu_for(&self, irq: usize) -> u8 {
        if irq < INTR_IRQ_LINES {
            self.cpu_of[irq]
        } else {
            INTR_AFFINITY_NONE
        }
    }
}

// ===========================================================================
// F079 — 时钟源仲裁庭：rating 高者胜，同分先到先得
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntrClockSource {
    pub id: u8,
    pub rating: u32,
}

pub fn clocksource_better(a: &IntrClockSource, b: &IntrClockSource) -> bool {
    a.rating > b.rating
}

/// 从候选中选出 rating 最高的时钟源；同分取先出现者；空表 None。
pub fn best_clocksource(srcs: &[IntrClockSource]) -> Option<u8> {
    if srcs.is_empty() {
        return None;
    }
    let mut best = 0usize;
    let mut i = 1usize;
    while i < srcs.len() {
        if clocksource_better(&srcs[i], &srcs[best]) {
            best = i;
        }
        i += 1;
    }
    Some(srcs[best].id)
}

// ===========================================================================
// F080 — 定时器轮谱：三级轮 0/6/12 位，槽位取模 64
// ===========================================================================

pub const INTR_WHEEL_SLOTS: u64 = 64;

pub fn wheel_level(delta_ticks: u64) -> u8 {
    if delta_ticks < INTR_WHEEL_SLOTS {
        0
    } else if delta_ticks < INTR_WHEEL_SLOTS * INTR_WHEEL_SLOTS {
        6
    } else {
        12
    }
}

pub fn wheel_slot(expires: u64, level_shift: u8) -> usize {
    ((expires >> level_shift) & (INTR_WHEEL_SLOTS - 1)) as usize
}

// ===========================================================================
// F081 — 中断风暴阀：速率三档裁决 + 线性限速
// ===========================================================================

pub const INTR_STORM_PASS_RATE: u32 = 10_000;
pub const INTR_STORM_MASK_RATE: u32 = 50_000;
pub const INTR_STORM_MAX_DELAY_US: u32 = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrStormAction {
    Pass,
    Throttle,
    Mask,
}

pub fn storm_action(rate_per_sec: u32) -> IntrStormAction {
    if rate_per_sec <= INTR_STORM_PASS_RATE {
        IntrStormAction::Pass
    } else if rate_per_sec <= INTR_STORM_MASK_RATE {
        IntrStormAction::Throttle
    } else {
        IntrStormAction::Mask
    }
}

pub fn storm_delay_us(rate_per_sec: u32) -> u32 {
    if rate_per_sec <= INTR_STORM_PASS_RATE {
        0
    } else if rate_per_sec >= INTR_STORM_MASK_RATE {
        INTR_STORM_MAX_DELAY_US
    } else {
        (rate_per_sec - INTR_STORM_PASS_RATE) * INTR_STORM_MAX_DELAY_US
            / (INTR_STORM_MASK_RATE - INTR_STORM_PASS_RATE)
    }
}

// ===========================================================================
// F082 — 中断延迟仪：计数/累计/峰值三件套，样本封顶
// ===========================================================================

pub const INTR_LATENCY_SAMPLE_CAP: u64 = 10_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct IntrLatencyMeter {
    pub count: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
}

impl IntrLatencyMeter {
    pub fn record(&mut self, ns: u64) -> bool {
        if self.count >= INTR_LATENCY_SAMPLE_CAP {
            return false;
        }
        self.count += 1;
        self.sum_ns = self.sum_ns.wrapping_add(ns);
        if ns > self.max_ns {
            self.max_ns = ns;
        }
        true
    }

    pub fn avg_ns(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.sum_ns / self.count
        }
    }
}

// ===========================================================================
// F083 — MSI/MSI-X 谱：MSI 只许 2 的幂且 ≤ 32，MSI-X 上限 2048
// ===========================================================================

pub const INTR_MSIX_MAX_VECTORS: u32 = 2048;

pub fn msi_count_ok(n: u32) -> bool {
    n == 1 || n == 2 || n == 4 || n == 8 || n == 16 || n == 32
}

pub fn msix_count_ok(n: u32) -> bool {
    n >= 1 && n <= INTR_MSIX_MAX_VECTORS
}

// ===========================================================================
// F084 — 中断嵌套律：硬中断不许嵌套，软中断深度 < 4
// ===========================================================================

pub const INTR_SOFTIRQ_MAX_DEPTH: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrNestKind {
    HardIrq,
    SoftIrq,
}

pub fn nest_allowed(current_depth: u32, kind: IntrNestKind) -> bool {
    match kind {
        IntrNestKind::HardIrq => current_depth == 0,
        IntrNestKind::SoftIrq => current_depth < INTR_SOFTIRQ_MAX_DEPTH,
    }
}

// ===========================================================================
// F085 — 定时器漂移考古：漂移 permille 与容差判定
// ===========================================================================

pub const INTR_DRIFT_TOL_PERMILLE: u64 = 50;

pub fn drift_permille(expected: u64, actual: u64) -> u64 {
    if expected == 0 {
        return 0;
    }
    let diff = if actual > expected {
        actual - expected
    } else {
        expected - actual
    };
    diff * 1000 / expected
}

pub fn drift_ok(expected: u64, actual: u64, tol_permille: u64) -> bool {
    drift_permille(expected, actual) <= tol_permille
}

// ===========================================================================
// F086 — 高精度定时器：到期不落在节拍边界 → 需要 hrtimer
// ===========================================================================

pub fn needs_hrtimer(expires_ns: u64, tick_ns: u64) -> bool {
    tick_ns != 0 && expires_ns % tick_ns != 0
}

/// 到期时刻到下一个节拍边界的松弛量（对齐时为 0）。
pub fn slack_ns(expires_ns: u64, tick_ns: u64) -> u64 {
    if tick_ns == 0 {
        return 0;
    }
    let r = expires_ns % tick_ns;
    if r == 0 {
        0
    } else {
        tick_ns - r
    }
}

// ===========================================================================
// F087 — 中断记账簿：16 条 IRQ 线计数，找出最忙 IRQ
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct IntrBook {
    counts: [u64; INTR_IRQ_LINES],
}

impl IntrBook {
    pub const fn new() -> IntrBook {
        IntrBook { counts: [0; INTR_IRQ_LINES] }
    }

    pub fn bump(&mut self, irq: usize) -> bool {
        if irq >= INTR_IRQ_LINES {
            return false;
        }
        self.counts[irq] += 1;
        true
    }

    pub fn total(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < INTR_IRQ_LINES {
            sum += self.counts[i];
            i += 1;
        }
        sum
    }

    /// 计数最高的 IRQ 线；全零返回 None（同分取最低线号）。
    pub fn top_irq(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut i = 0usize;
        while i < INTR_IRQ_LINES {
            if self.counts[i] > 0 {
                match best {
                    Some(b) if self.counts[b] >= self.counts[i] => {}
                    _ => best = Some(i),
                }
            }
            i += 1;
        }
        best
    }
}

// ===========================================================================
// F088 — 断言回归走廊：期望 IRQ 计数谱与实际谱逐项比对
// ===========================================================================

pub fn corridor_ok(expected: &[u32], actual: &[u32]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    let mut i = 0usize;
    while i < expected.len() {
        if expected[i] != actual[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F089 — 向量泄露纠察：teardown 后 claimed ≠ released 即泄露
// ===========================================================================

pub fn leaked_vectors(claimed: u32, released: u32) -> u32 {
    claimed.saturating_sub(released)
}

pub fn teardown_clean(claimed: u32, released: u32) -> bool {
    claimed == released
}

/// 释放多于申领也是账目事故。
pub fn over_release(claimed: u32, released: u32) -> bool {
    released > claimed
}

// ===========================================================================
// F090 — 中断风暴回放：8 槽速率环形样本，回放找峰值
// ===========================================================================

pub const INTR_STORM_RING: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct IntrStormReplay {
    samples: [u32; INTR_STORM_RING],
    head: usize,
    pub pushed: u64,
}

impl IntrStormReplay {
    pub const fn new() -> IntrStormReplay {
        IntrStormReplay { samples: [0; INTR_STORM_RING], head: 0, pushed: 0 }
    }

    pub fn push(&mut self, rate: u32) {
        self.samples[self.head] = rate;
        self.head = (self.head + 1) % INTR_STORM_RING;
        self.pushed += 1;
    }

    pub fn dropped(&self) -> u64 {
        self.pushed.saturating_sub(INTR_STORM_RING as u64)
    }

    pub fn sample(&self, i: usize) -> u32 {
        self.samples[i % INTR_STORM_RING]
    }

    /// 环内当前保留样本的峰值。
    pub fn peak(&self) -> u32 {
        let mut best = self.samples[0];
        let mut i = 1usize;
        while i < INTR_STORM_RING {
            if self.samples[i] > best {
                best = self.samples[i];
            }
            i += 1;
        }
        best
    }
}

// ===========================================================================
// F091 — 时钟节拍裁缝：负载越高节拍越密
// ===========================================================================

pub fn tick_hz(load_permille: u32) -> u32 {
    if load_permille >= 800 {
        1000
    } else if load_permille >= 300 {
        250
    } else {
        100
    }
}

// ===========================================================================
// F092 — 睡眠时钟协议：深睡必须先布好唤醒源
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrSleepDepth {
    Light,
    Deep,
}

pub fn sleep_ok(depth: IntrSleepDepth, wake_armed: bool) -> bool {
    match depth {
        IntrSleepDepth::Light => true,
        IntrSleepDepth::Deep => wake_armed,
    }
}

// ===========================================================================
// F093 — IPI 礼仪：广播默认排除自己，IPI 必须挂处理函数
// ===========================================================================

pub fn ipi_target_count(total_cpus: u8, include_self: bool) -> u8 {
    if total_cpus == 0 {
        0
    } else if include_self {
        total_cpus
    } else {
        total_cpus - 1
    }
}

pub fn ipi_needs_handler(vector: u8, has_handler: bool) -> bool {
    vector >= INTR_IRQ_VECTOR_BASE && has_handler
}

// ===========================================================================
// F094 — 中断负载画像：IRQ tick 占比 permille 与过载判定
// ===========================================================================

pub const INTR_OVERLOAD_PERMILLE: u32 = 400;

pub fn irq_load_permille(irq_ticks: u64, total_ticks: u64) -> u32 {
    if total_ticks == 0 {
        0
    } else {
        (irq_ticks * 1000 / total_ticks) as u32
    }
}

pub fn irq_overloaded(load_permille: u32) -> bool {
    load_permille >= INTR_OVERLOAD_PERMILLE
}

// ===========================================================================
// F095 — 异常入口谱：向量 0/8/13/14 的异常分类
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrException {
    DivideError,
    DoubleFault,
    GeneralProtection,
    PageFault,
}

pub fn exception_from_vector(v: u8) -> Option<IntrException> {
    match v {
        0 => Some(IntrException::DivideError),
        8 => Some(IntrException::DoubleFault),
        13 => Some(IntrException::GeneralProtection),
        14 => Some(IntrException::PageFault),
        _ => None,
    }
}

pub fn exception_fatal(e: IntrException) -> bool {
    matches!(e, IntrException::DoubleFault)
}

// ===========================================================================
// F096 — 定时器批量到期：收集 expires ≤ now 的定时器并原地压缩
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntrTimer {
    pub expires: u64,
}

/// 原地移除到期定时器（保序压缩），返回到期数量。
pub fn collect_due(timers: &mut [IntrTimer], now: u64) -> usize {
    let before = timers.len();
    let mut w = 0usize;
    for r in 0..timers.len() {
        if timers[r].expires > now {
            timers.swap(w, r);
            w += 1;
        }
    }
    before - w
}

// ===========================================================================
// F097 — 中断源自描述：设备描述先于 request_irq，共享线需双边声明
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntrSourceDesc {
    pub device_id: u8,
    pub line: u8,
    pub shared: bool,
}

pub fn desc_ok(d: IntrSourceDesc) -> bool {
    d.device_id != 0 && d.line < INTR_IRQ_LINES as u8
}

/// 同一条线上两设备：任一方未声明共享即冲突。
pub fn share_conflict(a: IntrSourceDesc, b: IntrSourceDesc) -> bool {
    a.line == b.line && !(a.shared && b.shared)
}

// ===========================================================================
// F098 — 中断屏蔽恢复律：保存-屏蔽-恢复栈，LIFO 精确还原
// ===========================================================================

pub const INTR_MASK_STACK_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct IntrMaskStack {
    flags: [bool; INTR_MASK_STACK_DEPTH],
    depth: usize,
}

impl IntrMaskStack {
    pub const fn new() -> IntrMaskStack {
        IntrMaskStack { flags: [false; INTR_MASK_STACK_DEPTH], depth: 0 }
    }

    /// 进入屏蔽区，保存先前中断使能位；栈满拒绝。
    pub fn push_mask(&mut self, prev_enabled: bool) -> bool {
        if self.depth >= INTR_MASK_STACK_DEPTH {
            return false;
        }
        self.flags[self.depth] = prev_enabled;
        self.depth += 1;
        true
    }

    /// 退出屏蔽区，弹出并还原先前使能位；空栈 None。
    pub fn pop_restore(&mut self) -> Option<bool> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        Some(self.flags[self.depth])
    }

    pub fn len(&self) -> usize {
        self.depth
    }
}

// ===========================================================================
// F099 — 时钟精度档案：PPM 误差与容差判定
// ===========================================================================

pub fn clock_error_ppm(expected: u64, actual: u64) -> i64 {
    if expected == 0 {
        return 0;
    }
    (actual as i64 - expected as i64) * 1_000_000 / (expected as i64)
}

pub fn clock_within_ppm(expected: u64, actual: u64, tol_ppm: i64) -> bool {
    let e = clock_error_ppm(expected, actual);
    let mag = if e < 0 { -e } else { e };
    mag <= tol_ppm
}

// ===========================================================================
// F100 — 中断域年报：年报章节完备性
// ===========================================================================

pub const INTR_REPORT_SECTIONS: [&str; 5] =
    ["vectors", "storms", "timers", "accounting", "precision"];

pub fn intr_report_complete(sections_filled: u32) -> bool {
    sections_filled >= INTR_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700intr_checks() -> CheckSet {
    let mut set = CheckSet::new("m700intr");

    // F076 中断向量大典
    let mut vt = IntrVectorTable::new();
    let c1 = vt.claim(40, 3);
    let cdup = vt.claim(40, 4);
    let cexc = vt.claim(14, 1);
    set.add(
        "F076 vector claim",
        c1 && !cdup && !cexc && vt.owner_of(40) == 3,
        "dedup + exception range guarded",
    );
    let rel1 = vt.release(40);
    let rel2 = vt.release(40);
    set.add("F076 vector release", rel1 && !rel2 && vt.owner_of(40) == 0, "freed to sentinel");
    let rec = vt.claim(40, 4);
    set.add("F076 vector reclaim", rec && vt.owner_of(40) == 4, "reusable after release");

    // F077 上下半场律
    let mut dh = IntrDeferBook::default();
    let t1 = run_top_half(500, &mut dh);
    let inline_after_fast = dh.handled_inline;
    let t2 = run_top_half(1500, &mut dh);
    set.add(
        "F077 top half inline",
        t1 && inline_after_fast == 1 && !t2 && dh.deferred == 1,
        "budget split recorded",
    );
    let t3 = run_top_half(INTR_TOP_BUDGET_CYCLES, &mut dh);
    set.add(
        "F077 top half boundary",
        t3 && dh.handled_inline == 2,
        "exact budget counts inline",
    );

    // F078 中断亲和图
    let mut aff = IntrAffinity::new();
    let a1 = aff.set_affinity(0, 2);
    let adup = aff.set_affinity(0, 2);
    let abad_cpu = aff.set_affinity(0, 9);
    let abad_irq = aff.set_affinity(16, 1);
    set.add(
        "F078 affinity set",
        a1 && !adup && !abad_cpu && !abad_irq && aff.cpu_for(0) == 2,
        "dedup + range",
    );
    let a2 = aff.set_affinity(0, 5);
    set.add(
        "F078 affinity migrate",
        a2 && aff.cpu_for(0) == 5 && aff.cpu_for(15) == INTR_AFFINITY_NONE,
        "rebind and unassigned line",
    );

    // F079 时钟源仲裁庭
    let srcs = [
        IntrClockSource { id: 1, rating: 300 },
        IntrClockSource { id: 2, rating: 250 },
        IntrClockSource { id: 3, rating: 100 },
    ];
    set.add(
        "F079 arbitration best",
        best_clocksource(&srcs) == Some(1) && clocksource_better(&srcs[0], &srcs[1]),
        "highest rating wins",
    );
    let tie = [IntrClockSource { id: 7, rating: 200 }, IntrClockSource { id: 8, rating: 200 }];
    set.add(
        "F079 arbitration tie",
        best_clocksource(&tie) == Some(7) && best_clocksource(&[]) == None,
        "first wins, empty none",
    );

    // F080 定时器轮谱
    set.add(
        "F080 wheel levels",
        wheel_level(10) == 0 && wheel_level(100) == 6 && wheel_level(100_000) == 12,
        "delta picks level",
    );
    set.add(
        "F080 wheel slots",
        wheel_slot(10, 0) == 10 && wheel_slot(100, 6) == 1 && wheel_slot(100_000, 12) == 24,
        "shift then mod 64",
    );

    // F081 中断风暴阀
    set.add(
        "F081 storm tiers",
        storm_action(5_000) == IntrStormAction::Pass
            && storm_action(30_000) == IntrStormAction::Throttle
            && storm_action(60_000) == IntrStormAction::Mask,
        "three actions",
    );
    set.add(
        "F081 storm delay",
        storm_delay_us(INTR_STORM_PASS_RATE) == 0
            && storm_delay_us(30_000) == 1000
            && storm_delay_us(INTR_STORM_MASK_RATE) == INTR_STORM_MAX_DELAY_US
            && storm_delay_us(500_000) == INTR_STORM_MAX_DELAY_US,
        "linear ramp clamped",
    );

    // F082 中断延迟仪
    let mut lat = IntrLatencyMeter::default();
    let l1 = lat.record(100);
    let l2 = lat.record(200);
    let l3 = lat.record(300);
    set.add(
        "F082 latency stats",
        l1 && l2 && l3 && lat.avg_ns() == 200 && lat.max_ns == 300,
        "mean and peak",
    );
    let mut full_lat = IntrLatencyMeter { count: INTR_LATENCY_SAMPLE_CAP, sum_ns: 0, max_ns: 0 };
    set.add("F082 latency cap", !full_lat.record(1), "sample cap enforced");

    // F083 MSI/MSI-X 谱
    set.add(
        "F083 msi powers",
        msi_count_ok(1) && msi_count_ok(8) && msi_count_ok(32) && !msi_count_ok(3)
            && !msi_count_ok(64),
        "power-of-two up to 32",
    );
    set.add(
        "F083 msix range",
        msix_count_ok(1) && msix_count_ok(INTR_MSIX_MAX_VECTORS)
            && !msix_count_ok(0)
            && !msix_count_ok(INTR_MSIX_MAX_VECTORS + 1),
        "1..=2048",
    );

    // F084 中断嵌套律
    set.add(
        "F084 hardirq no nest",
        nest_allowed(0, IntrNestKind::HardIrq) && !nest_allowed(1, IntrNestKind::HardIrq),
        "depth must be zero",
    );
    set.add(
        "F084 softirq nest",
        nest_allowed(3, IntrNestKind::SoftIrq) && !nest_allowed(4, IntrNestKind::SoftIrq),
        "depth < 4",
    );

    // F085 定时器漂移考古
    set.add(
        "F085 drift math",
        drift_permille(1000, 1020) == 20 && drift_permille(1000, 1100) == 100
            && drift_permille(0, 5) == 0,
        "permille of expected",
    );
    set.add(
        "F085 drift tolerance",
        drift_ok(1000, 1020, INTR_DRIFT_TOL_PERMILLE)
            && !drift_ok(1000, 1100, INTR_DRIFT_TOL_PERMILLE),
        "50 permille tolerance",
    );

    // F086 高精度定时器
    set.add(
        "F086 hrtimer needed",
        needs_hrtimer(1_500_000, 1_000_000) && slack_ns(1_500_000, 1_000_000) == 500_000,
        "mid-tick expiry",
    );
    set.add(
        "F086 hrtimer aligned",
        !needs_hrtimer(2_000_000, 1_000_000) && slack_ns(2_000_000, 1_000_000) == 0
            && !needs_hrtimer(1, 0) && slack_ns(1, 0) == 0,
        "aligned or no tick",
    );

    // F087 中断记账簿
    let mut bk = IntrBook::new();
    let b1 = bk.bump(3);
    let b2 = bk.bump(3);
    let b3 = bk.bump(5);
    let boob = bk.bump(16);
    set.add(
        "F087 book bump",
        b1 && b2 && b3 && !boob && bk.total() == 3,
        "counts and range",
    );
    set.add(
        "F087 book top",
        bk.top_irq() == Some(3) && IntrBook::new().top_irq() == None,
        "busiest line",
    );

    // F088 断言回归走廊
    set.add(
        "F088 corridor match",
        corridor_ok(&[1, 2, 3], &[1, 2, 3]),
        "spectra equal",
    );
    set.add(
        "F088 corridor drift",
        !corridor_ok(&[1, 2, 3], &[1, 2, 4]) && !corridor_ok(&[1, 2], &[1, 2, 3]),
        "drift and length mismatch",
    );

    // F089 向量泄露纠察
    set.add(
        "F089 leak clean",
        leaked_vectors(10, 10) == 0 && teardown_clean(10, 10),
        "balanced teardown",
    );
    set.add(
        "F089 leak found",
        leaked_vectors(10, 8) == 2 && !teardown_clean(10, 8)
            && over_release(3, 5) && !over_release(5, 3),
        "leak and over-release",
    );

    // F090 中断风暴回放
    let mut rp = IntrStormReplay::new();
    rp.push(100);
    rp.push(9000);
    rp.push(60_000);
    rp.push(20_000);
    let mut i = 0u64;
    while i < 6 {
        rp.push(1 + i as u32);
        i += 1;
    }
    set.add(
        "F090 replay wrap",
        rp.pushed == 10 && rp.dropped() == 2,
        "ring holds 8, rest dropped",
    );
    let peak = rp.peak();
    set.add(
        "F090 replay peak",
        peak == 60_000 && rp.sample(2) == 60_000,
        "surviving sample peak",
    );

    // F091 时钟节拍裁缝
    set.add(
        "F091 tick tiers",
        tick_hz(900) == 1000 && tick_hz(800) == 1000 && tick_hz(799) == 250
            && tick_hz(300) == 250 && tick_hz(299) == 100,
        "boundary exact",
    );

    // F092 睡眠时钟协议
    set.add(
        "F092 deep sleep gate",
        !sleep_ok(IntrSleepDepth::Deep, false) && sleep_ok(IntrSleepDepth::Deep, true),
        "wake source required",
    );
    set.add(
        "F092 light sleep free",
        sleep_ok(IntrSleepDepth::Light, false),
        "light needs nothing",
    );

    // F093 IPI 礼仪
    set.add(
        "F093 ipi counts",
        ipi_target_count(4, false) == 3 && ipi_target_count(4, true) == 4
            && ipi_target_count(1, false) == 0 && ipi_target_count(0, true) == 0,
        "self exclusion math",
    );
    set.add(
        "F093 ipi handler rule",
        ipi_needs_handler(40, true) && !ipi_needs_handler(40, false)
            && !ipi_needs_handler(14, true),
        "vector range + handler",
    );

    // F094 中断负载画像
    set.add(
        "F094 load permille",
        irq_load_permille(400, 1000) == 400 && irq_load_permille(399, 1000) == 399
            && irq_load_permille(1, 0) == 0,
        "ratio incl. zero total",
    );
    set.add(
        "F094 overload line",
        irq_overloaded(400) && !irq_overloaded(399),
        "400 permille threshold",
    );

    // F095 异常入口谱
    set.add(
        "F095 exception map",
        exception_from_vector(14) == Some(IntrException::PageFault)
            && exception_from_vector(0) == Some(IntrException::DivideError)
            && exception_from_vector(7) == None,
        "vectors 0/8/13/14 only",
    );
    set.add(
        "F095 exception fatality",
        exception_fatal(IntrException::DoubleFault)
            && !exception_fatal(IntrException::PageFault)
            && !exception_fatal(IntrException::GeneralProtection),
        "double fault alone fatal",
    );

    // F096 定时器批量到期
    let mut timers = [
        IntrTimer { expires: 5 },
        IntrTimer { expires: 10 },
        IntrTimer { expires: 15 },
    ];
    let n1 = collect_due(&mut timers, 10);
    set.add(
        "F096 batch partial",
        n1 == 2 && timers[0] == IntrTimer { expires: 15 },
        "two expired, one compacted",
    );
    let mut all_due = [IntrTimer { expires: 1 }, IntrTimer { expires: 2 }];
    let n2 = collect_due(&mut all_due, 10);
    let mut none_due = [IntrTimer { expires: 15 }];
    let n3 = collect_due(&mut none_due, 10);
    set.add(
        "F096 batch edges",
        n2 == 2 && n3 == 0 && none_due[0] == IntrTimer { expires: 15 },
        "all or nothing",
    );

    // F097 中断源自描述
    let good = IntrSourceDesc { device_id: 9, line: 5, shared: false };
    let bad_dev = IntrSourceDesc { device_id: 0, line: 5, shared: false };
    let bad_line = IntrSourceDesc { device_id: 9, line: 16, shared: false };
    set.add(
        "F097 desc gate",
        desc_ok(good) && !desc_ok(bad_dev) && !desc_ok(bad_line),
        "device + line range",
    );
    let sharer_a = IntrSourceDesc { device_id: 1, line: 5, shared: false };
    let sharer_b = IntrSourceDesc { device_id: 2, line: 5, shared: true };
    let sharer_c = IntrSourceDesc { device_id: 3, line: 5, shared: true };
    let other_line = IntrSourceDesc { device_id: 4, line: 6, shared: false };
    set.add(
        "F097 share rules",
        share_conflict(sharer_a, sharer_b) && !share_conflict(sharer_b, sharer_c)
            && !share_conflict(sharer_a, other_line),
        "both must declare shared",
    );

    // F098 中断屏蔽恢复律
    let mut ms = IntrMaskStack::new();
    let p1 = ms.push_mask(true);
    let p2 = ms.push_mask(false);
    let p5 = {
        let mut full = IntrMaskStack::new();
        let mut pushed = 0usize;
        while full.push_mask(true) {
            pushed += 1;
        }
        pushed
    };
    set.add(
        "F098 mask push",
        p1 && p2 && p5 == INTR_MASK_STACK_DEPTH,
        "stack cap 4",
    );
    let r1 = ms.pop_restore();
    let r2 = ms.pop_restore();
    let r3 = ms.pop_restore();
    set.add(
        "F098 mask restore",
        r1 == Some(false) && r2 == Some(true) && r3 == None && ms.len() == 0,
        "LIFO exact restore",
    );

    // F099 时钟精度档案
    set.add(
        "F099 ppm math",
        clock_error_ppm(1_000_000, 1_000_100) == 100
            && clock_error_ppm(1_000_000, 999_800) == -200
            && clock_error_ppm(0, 5) == 0,
        "signed ppm",
    );
    set.add(
        "F099 ppm tolerance",
        clock_within_ppm(1_000_000, 1_000_100, 250)
            && clock_within_ppm(1_000_000, 999_800, 250)
            && !clock_within_ppm(1_000_000, 999_800, 150),
        "magnitude against tol",
    );

    // F100 中断域年报
    set.add(
        "F100 intr report",
        INTR_REPORT_SECTIONS.len() == 5 && intr_report_complete(5) && !intr_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f076_vector_registry() {
        let mut t = IntrVectorTable::new();
        assert!(t.claim(32, 1));
        assert!(t.claim(255, 2));
        assert!(!t.claim(32, 9));
        assert!(!t.claim(31, 9));
        assert!(!t.claim(40, 0));
        assert_eq!(t.owner_of(32), 1);
        assert!(t.release(32));
        assert!(!t.release(32));
        assert_eq!(t.owner_of(32), 0);
    }

    #[test]
    fn f080_wheel_slots() {
        assert_eq!(wheel_level(0), 0);
        assert_eq!(wheel_level(63), 0);
        assert_eq!(wheel_level(64), 6);
        assert_eq!(wheel_level(4095), 6);
        assert_eq!(wheel_level(4096), 12);
        assert_eq!(wheel_slot(4096, 12), 1);
        assert_eq!(wheel_slot(63, 0), 63);
    }

    #[test]
    fn f085_drift_math() {
        assert_eq!(drift_permille(1000, 1000), 0);
        assert_eq!(drift_permille(1000, 1010), 10);
        assert_eq!(drift_permille(1000, 990), 10);
        assert!(drift_ok(1000, 1049, 50));
        assert!(!drift_ok(1000, 1051, 50));
    }

    #[test]
    fn f096_batch_expiry() {
        let mut buf = [
            IntrTimer { expires: 1 },
            IntrTimer { expires: 5 },
            IntrTimer { expires: 9 },
            IntrTimer { expires: 12 },
        ];
        // 协议：幸存者压缩到头部 + 返回到期数，调用方按返回值收缩
        let mut live: &mut [IntrTimer] = &mut buf;
        assert_eq!(collect_due(live, 9), 3);
        assert_eq!(live[0], IntrTimer { expires: 12 });
        live = &mut live[3..];
        assert_eq!(collect_due(live, 100), 1);
        live = &mut live[1..];
        assert_eq!(collect_due(live, 100), 0);
    }

    #[test]
    fn f098_mask_stack_lifo() {
        let mut s = IntrMaskStack::new();
        assert!(s.push_mask(true));
        assert!(s.push_mask(false));
        assert!(s.push_mask(true));
        assert_eq!(s.len(), 3);
        assert_eq!(s.pop_restore(), Some(true));
        assert_eq!(s.pop_restore(), Some(false));
        assert_eq!(s.pop_restore(), Some(true));
        assert_eq!(s.pop_restore(), None);
    }

    #[test]
    fn m700intr_selfcheck_all_pass() {
        let set = run_m700intr_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
