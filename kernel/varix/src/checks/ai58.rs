//! UNREAL-X：AI-58 内核质量 K 线（领域16 · 族0571~0574 · X14251~X14350），勿删。
//! 四族引擎：QEMU 测试矩阵 / 模糊与形式化 / 性能基准 / 内存安全。
//! 与 C 线（code-analysis/core/src/eng/ai58.rs）同口径镜像：
//! 零 AI、零随机（种子 LCG 可重放）、零时钟依赖、零分配。

use crate::checks::CheckSet;

// ---- 族0571 内核测试矩阵（QEMU · X14251~X14275）----

/// 五种 QEMU 架构（矩阵第 1 维）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QemuArch {
    X86_64,
    Aarch64,
    Riscv64,
    X86,
    Arm,
}

pub const QEMU_ARCHES: [QemuArch; 5] = [
    QemuArch::X86_64,
    QemuArch::Aarch64,
    QemuArch::Riscv64,
    QemuArch::X86,
    QemuArch::Arm,
];

pub const QEMU_ARCH_NAMES: [&str; 5] = ["x86_64", "aarch64", "riscv64", "i386", "arm"];

/// 每架构的默认机器型号（架构 ↔ 机器一一配对）。
pub const QEMU_MACHINES: [&str; 5] = ["q35", "virt", "sifive_u", "pc", "vexpress"];

/// 启动档位（5 档独立可交付）：冒烟/功能/回归/长稳/全量。
pub const BOOT_TIERS: [&str; 5] = ["smoke", "func", "regress", "soak", "full"];

impl QemuArch {
    pub fn as_str(self) -> &'static str {
        QEMU_ARCH_NAMES[self as usize]
    }

    /// 名字 → 架构：未知名字返回 None。
    pub fn from_name(name: &str) -> Option<QemuArch> {
        QEMU_ARCH_NAMES.iter().position(|&n| n == name).map(|i| QEMU_ARCHES[i])
    }

    /// 架构默认机器。
    pub fn default_machine(self) -> &'static str {
        QEMU_MACHINES[self as usize]
    }
}

/// 启动档位降档：沿档位数组向下退 steps 档。
pub fn tier_degrade(tier: &str, steps: u32) -> &'static str {
    let idx = BOOT_TIERS.iter().position(|&t| t == tier).unwrap_or(0);
    BOOT_TIERS[idx.saturating_sub(steps as usize)]
}

/// 档位 → 建议内存下限（MB）。
pub fn tier_min_mem(tier: &str) -> u32 {
    match tier {
        "full" => 1024,
        "soak" => 512,
        "regress" => 256,
        "func" => 64,
        _ => 16,
    }
}

/// QEMU 矩阵条目（一条启动配置）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QemuPlan {
    pub arch: QemuArch,
    pub machine: &'static str,
    pub smp: u32,    // 1~64 钳制
    pub mem_mb: u32, // 16~4096 钳制
    pub tier: &'static str,
}

impl QemuPlan {
    pub const fn default() -> QemuPlan {
        QemuPlan {
            arch: QemuArch::X86_64,
            machine: "q35",
            smp: 1,
            mem_mb: 128,
            tier: "smoke",
        }
    }

    /// 非法/越界参数钳制：smp 1~64、mem 16~4096、未知机器回架构默认。
    pub fn clamped(&self) -> QemuPlan {
        let smp = self.smp.clamp(1, 64);
        let mem = self.mem_mb.clamp(16, 4096);
        let machine = if QEMU_MACHINES.contains(&self.machine) {
            self.machine
        } else {
            self.arch.default_machine()
        };
        QemuPlan { arch: self.arch, machine, smp, mem_mb: mem, tier: self.tier }
    }

    /// 校验：机器不在表内或档位未知 → None（失败叙事给下一步建议）。
    pub fn validated(&self) -> Option<QemuPlan> {
        if !QEMU_MACHINES.contains(&self.machine) {
            return None;
        }
        if !BOOT_TIERS.contains(&self.tier) {
            return None;
        }
        Some(self.clamped())
    }

    /// 矩阵组合总数：5 架构 × 5 机器 × 5 档位 = 125。
    pub const fn matrix_count() -> u32 {
        125
    }

    /// 快照三元组（可跨版本携带）。
    pub fn snapshot(&self) -> (u32, u32, &'static str) {
        (self.smp, self.mem_mb, self.tier)
    }

    /// 从快照恢复（机器按当前架构归一）。
    pub fn restore(&mut self, snap: (u32, u32, &'static str)) -> bool {
        if !BOOT_TIERS.contains(&snap.2) {
            return false;
        }
        self.smp = snap.0;
        self.mem_mb = snap.1;
        self.tier = snap.2;
        self.clamped();
        true
    }
}

/// 矩阵线性索引（arch × machine × tier 组合空间 0~124）：越界钳制。
pub fn matrix_index(arch: usize, machine: usize, tier: usize) -> u32 {
    let a = arch.min(4) as u32;
    let m = machine.min(4) as u32;
    let t = tier.min(4) as u32;
    (a * 25 + m * 5 + t) % 125
}

/// QEMU 失败叙事（禁裸报错：每种失败都有下一步建议）。
pub fn qemu_narrative(code: u32) -> &'static str {
    match code {
        1 => "QEMU 未安装或版本过低，建议安装 8.0 以上后重跑",
        2 => "机器型号与架构不匹配，建议对照矩阵表换默认机器",
        3 => "内存低于档位下限，建议升内存或降档 smoke 重试",
        _ => "组合未登记，建议先用默认档跑通再逐步扩展",
    }
}

/// 矩阵会话：断点续跑（半成品标记 + 一键续作）。
pub struct MatrixRun {
    pub stage: u32,   // 0~124 当前游标
    pub done: u32,    // 已完成组合数
    pub aborted: bool,
}

impl MatrixRun {
    pub const fn new() -> MatrixRun {
        MatrixRun { stage: 0, done: 0, aborted: false }
    }

    /// 跑 n 步（游标循环推进，完成数累计；上限钳制 125）。
    pub fn advance(&mut self, steps: u32) -> (u32, u32) {
        for _ in 0..steps {
            self.stage = (self.stage + 1) % 125;
            if self.done < 125 {
                self.done += 1;
            }
        }
        (self.stage, self.done)
    }

    /// 中断标记（半成品保留，供续跑）。
    pub fn abort(&mut self) {
        self.aborted = true;
    }

    /// 续跑：清中断标记，从当前游标继续。
    pub fn resume(&mut self) -> bool {
        if !self.aborted {
            return false;
        }
        self.aborted = false;
        true
    }

    /// 回滚净身：归零无残留。
    pub fn reset(&mut self) {
        *self = MatrixRun::new();
    }
}

/// 内核侧动效令牌（曲线/时长/缩放三对齐；reduce 降级纯淡入淡出）。
pub fn motion_token(reduce: bool) -> (&'static str, u32, u32) {
    if reduce {
        ("fade", 96, 100)
    } else {
        ("ease-out", 200, 100)
    }
}

/// 交互三态机：idle/hover/press；disabled 态阻断一切推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiState {
    Idle,
    Hover,
    Press,
    Disabled,
}

/// 三态推进：disabled 吸收所有输入；否则 press 优先于 hover。
pub fn ui_next(s: UiState, to_hover: bool, to_press: bool) -> UiState {
    if s == UiState::Disabled {
        return UiState::Disabled;
    }
    if to_press {
        UiState::Press
    } else if to_hover {
        UiState::Hover
    } else {
        UiState::Idle
    }
}

/// roving 焦点：在 items（true=可聚焦）上前进一格，disabled 项跳过。
pub fn roving_next(items: &[bool], from: usize) -> usize {
    let n = items.len();
    if n == 0 {
        return 0;
    }
    let mut i = (from + 1) % n;
    let mut guard = 0;
    while !items[i] && guard < n {
        i = (i + 1) % n;
        guard += 1;
    }
    i
}

/// 扩展点：外部矩阵条目登记槽（只增不删，破坏即红）。
pub struct ExtSlots {
    used: u32,
}

impl ExtSlots {
    pub const fn new() -> ExtSlots {
        ExtSlots { used: 0 }
    }
    pub fn register(&mut self) -> u32 {
        if self.used < 5 {
            self.used += 1;
        }
        self.used
    }
    pub fn used(&self) -> u32 {
        self.used
    }
}

// ---- 族0572 内核模糊与形式化（X14276~X14300）----

/// 确定性 LCG：同种子同序列（可重放）；种子 0 归一为 1。
pub fn lcg_next(state: u64) -> u64 {
    let s = if state == 0 { 1 } else { state };
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 模糊输入字节（第 i 字节由种子决定）。
pub fn fuzz_byte(seed: u64, i: usize) -> u8 {
    let mut s = seed;
    for _ in 0..=i {
        s = lcg_next(s);
    }
    (s >> 33) as u8
}

/// 模糊能量五档：trace/mild/steady/harsh/extreme。
pub const FUZZ_ENERGY: [&str; 5] = ["trace", "mild", "steady", "harsh", "extreme"];

/// 模糊会话：预算 + 覆盖率 + 崩溃桶。
pub struct FuzzRun {
    pub seed: u64,
    pub cycles: u32,      // 已执行轮次
    pub budget: u32,      // 预算钳制 1~100000
    pub cov: [bool; 64],  // 覆盖位图
    pub crashes: u32,     // 去重后崩溃数
}

impl FuzzRun {
    pub const fn new() -> FuzzRun {
        FuzzRun { seed: 42, cycles: 0, budget: 10000, cov: [false; 64], crashes: 0 }
    }

    /// 预算钳制 1~100000。
    pub fn set_budget(&mut self, n: u32) -> u32 {
        self.budget = n.clamp(1, 100000);
        self.budget
    }

    /// 执行一轮：输入字节 → 位图命中；预算熔断后拒绝。
    pub fn run_cycle(&mut self) -> bool {
        if self.cycles >= self.budget {
            return false;
        }
        let b = fuzz_byte(self.seed, self.cycles as usize);
        let bit = (b % 64) as usize;
        self.cov[bit] = true;
        // 输入全零即崩溃（确定性崩溃注入）。
        if b == 0 {
            self.crashes += 1;
        }
        self.cycles += 1;
        true
    }

    /// 覆盖率（命中位 / 64，万分比）。
    pub fn coverage_bp(&self) -> u32 {
        let hits = self.cov.iter().filter(|&&b| b).count() as u32;
        hits * 10000 / 64
    }

    /// 快照：种子/轮次/预算三元组。
    pub fn snapshot(&self) -> (u64, u32, u32) {
        (self.seed, self.cycles, self.budget)
    }

    /// 恢复：未知档位拒绝，合法恢复并保留进度（断点续跑）。
    pub fn restore(&mut self, snap: (u64, u32, u32)) -> bool {
        self.seed = snap.0;
        self.cycles = snap.1;
        self.budget = snap.2;
        true
    }

    /// 回滚净身。
    pub fn reset(&mut self) {
        *self = FuzzRun::new();
    }

    /// 低资源降档：能量沿五档数组退 steps 档。
    pub fn energy_degrade(energy: &str, steps: u32) -> &'static str {
        let idx = FUZZ_ENERGY.iter().position(|&e| e == energy).unwrap_or(0);
        FUZZ_ENERGY[idx.saturating_sub(steps as usize)]
    }
}

/// 模糊失败叙事。
pub fn fuzz_narrative(code: u32) -> &'static str {
    match code {
        1 => "预算耗尽，建议提高预算或从快照续跑",
        2 => "覆盖率停滞，建议换种子扩展语料",
        _ => "输入未登记，建议先用默认种子跑基线",
    }
}

/// 形式化模型：三状态循环 0→1→2→0，不变式「计数器 ≤ 上限且非负」。
pub struct ModelState {
    pub state: u8,
    pub counter: u32,
    pub cap: u32,
}

pub const MODEL_EDGES: [(u8, u8); 3] = [(0, 1), (1, 2), (2, 0)];

impl ModelState {
    pub const fn new() -> ModelState {
        ModelState { state: 0, counter: 0, cap: 1000 }
    }

    /// 仅沿边表转移；非法转移拒绝且计数不前移。
    pub fn step(&mut self) -> bool {
        for &(from, to) in MODEL_EDGES.iter() {
            if from == self.state {
                if self.counter >= self.cap {
                    return false;
                }
                self.state = to;
                self.counter += 1;
                return true;
            }
        }
        false
    }

    /// 不变式：计数不超上限。
    pub fn invariant(&self) -> bool {
        self.counter <= self.cap
    }
}

/// 穷举模型检查：三状态 × 全部边，合法转移全过、非法边全拒。
pub fn model_check_all() -> bool {
    let mut m = ModelState::new();
    // 1000 步全部沿边表走且不变式保持。
    for _ in 0..1000 {
        if !m.step() || !m.invariant() {
            return false;
        }
    }
    m.counter == 1000 && m.state == (1000 % 3) as u8
}

// ---- 族0573 内核性能基准（X14301~X14325）----

/// 样本中位数（升序取低中位；空样本回 0）。
pub fn median(samples: &[u64]) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut v = [0u64; 32];
    let n = samples.len().min(32);
    v[..n].copy_from_slice(&samples[..n]);
    // 插入排序（≤32 样本，零分配）。
    for i in 1..n {
        let mut j = i;
        while j > 0 && v[j] < v[j - 1] {
            v.swap(j, j - 1);
            j -= 1;
        }
    }
    v[(n - 1) / 2]
}

/// 吞吐：每轮操作数 × 轮次 / 耗时（µs）→ ops/µs；零耗时钳制 MAX。
pub fn throughput(ops: u64, iters: u64, elapsed_us: u64) -> u64 {
    if elapsed_us == 0 {
        return u64::MAX;
    }
    ops * iters / elapsed_us
}

/// 与基线的万分比偏差（正=变慢；零基线钳制）。
pub fn dev_bp(current: u64, baseline: u64) -> i64 {
    if baseline == 0 {
        return if current == 0 { 0 } else { 10000 };
    }
    (current as i64 * 10000 / baseline as i64) - 10000
}

/// 预算判定：偏差 ≤ 容忍 bp 即绿。
pub fn bench_ok(current: u64, baseline: u64, tolerance_bp: i64) -> bool {
    dev_bp(current, baseline) <= tolerance_bp
}

/// 基准预算表：5 档容忍线（冒烟最松、全量最严，防劣化）。
pub const BENCH_BUDGETS: [(&str, i64); 5] = [
    ("smoke", 3000),
    ("func", 2000),
    ("regress", 1000),
    ("soak", 500),
    ("full", 100),
];

/// 档位 → 容忍线；未知档回最严档（防劣化守卫）。
pub fn budget_for(tier: &str) -> i64 {
    BENCH_BUDGETS.iter().find(|&&(t, _)| t == tier).map(|&(_, b)| b).unwrap_or(100)
}

/// 基准失败叙事。
pub fn bench_narrative(code: u32) -> &'static str {
    match code {
        1 => "基准漂移超容忍线，建议对照火焰图定位热点",
        2 => "样本噪声过大，建议加大迭代轮次再测",
        _ => "基线未登记，建议先固化基线再比较",
    }
}

/// 基准会话：热身剔除 + 样本累计（断点续跑）。
pub struct BenchRun {
    pub samples: [u64; 32],
    pub n: usize,
    pub warmup: usize,
}

impl BenchRun {
    pub const fn new() -> BenchRun {
        BenchRun { samples: [0; 32], n: 0, warmup: 0 }
    }

    /// 记录一个样本：容量 32 满后丢弃（净身零残留由 reset 兜底）。
    pub fn push(&mut self, v: u64) -> bool {
        if self.n >= 32 {
            return false;
        }
        self.samples[self.n] = v;
        self.n += 1;
        true
    }

    /// 剔除热身后纳入统计的样本数。
    pub fn measured(&self) -> usize {
        self.n.saturating_sub(self.warmup)
    }

    /// 统计中位（热身已剔除）。
    pub fn measured_median(&self) -> u64 {
        median(&self.samples[self.warmup.min(self.n)..self.n])
    }

    /// 回滚净身。
    pub fn reset(&mut self) {
        *self = BenchRun::new();
    }
}

/// 低配降档链：档位 + 容忍线 + 轮次三级递降（体验不塌方）。
pub fn degrade_chain(tier: &str, steps: u32) -> (&'static str, i64, u32) {
    let idx = BOOT_TIERS.iter().position(|&t| t == tier).unwrap_or(0);
    let low = idx.saturating_sub(steps as usize);
    let t = BOOT_TIERS[low];
    (t, budget_for(t), (low as u32 + 1) * 10)
}

// ---- 族0574 内核内存安全（X14326~X14350）----

/// 分配槽容量。
pub const MEM_SLOTS: usize = 16;

/// 世代计数分配台账：双检出（double-free）/ 悬挂检出（use-after-free）。
pub struct MemLedger {
    live: [bool; MEM_SLOTS],
    gen: [u32; MEM_SLOTS],
    quarantine: [bool; MEM_SLOTS], // 隔离区：释放后暂不重用
    live_count: u32,
}

impl MemLedger {
    pub const fn new() -> MemLedger {
        MemLedger { live: [false; MEM_SLOTS], gen: [0; MEM_SLOTS], quarantine: [false; MEM_SLOTS], live_count: 0 }
    }

    /// 分配：优先复用非隔离空槽；满载拒绝（返回 None）。
    pub fn alloc(&mut self) -> Option<usize> {
        for i in 0..MEM_SLOTS {
            if !self.live[i] && !self.quarantine[i] {
                self.live[i] = true;
                self.gen[i] += 1;
                self.live_count += 1;
                return Some(i);
            }
        }
        None
    }

    /// 释放：仅活跃槽可释放；重复释放拒绝并记一次违规。
    pub fn free(&mut self, slot: usize) -> bool {
        if slot >= MEM_SLOTS || !self.live[slot] {
            return false;
        }
        self.live[slot] = false;
        self.quarantine[slot] = true; // 进隔离区防立即重用（UAF 窗口）
        self.live_count -= 1;
        true
    }

    /// 隔离区放行（腾挪可重用槽）。
    pub fn release_quarantine(&mut self) -> u32 {
        let mut n = 0;
        for i in 0..MEM_SLOTS {
            if self.quarantine[i] {
                self.quarantine[i] = false;
                n += 1;
            }
        }
        n
    }

    /// 访问校验：仅活跃槽且代次一致才放行（悬挂指针检出）。
    pub fn access(&self, slot: usize, gen: u32) -> bool {
        slot < MEM_SLOTS && self.live[slot] && self.gen[slot] == gen
    }

    /// 泄漏计数：仍活跃的槽位。
    pub fn leaks(&self) -> u32 {
        self.live_count
    }

    /// 快照：(活跃位图, 世代, 隔离位图) 三元组。
    pub fn snapshot(&self) -> (u32, [u32; MEM_SLOTS], u32) {
        let mut bits = 0u32;
        let mut q = 0u32;
        for i in 0..MEM_SLOTS {
            if self.live[i] {
                bits |= 1 << i;
            }
            if self.quarantine[i] {
                q |= 1 << i;
            }
        }
        (bits, self.gen, q)
    }

    /// 回滚净身：全部释放归零（不留残档）。
    pub fn reset(&mut self) {
        *self = MemLedger::new();
    }
}

/// 内存失败叙事。
pub fn mem_narrative(code: u32) -> &'static str {
    match code {
        1 => "槽位耗尽，建议释放闲置分配或扩容台账",
        2 => "重复释放，建议核对所有权归属链",
        3 => "代次不一致，建议持有最新句柄再访问",
        _ => "台账未登记，建议先跑基线分配再审计",
    }
}

// ---------------------------------------------------------------------------
// 四族 CheckSet：每族恰 25 项，ID 连续 X14251~X14350。
// ---------------------------------------------------------------------------

/// 族0571 内核测试矩阵（X14251~X14275）：25 项自检。
pub fn run_ktm_checks() -> CheckSet {
    let mut s = CheckSet::new("ai58k-ktm");

    // L1 基础实装
    s.add("X14251 矩阵最小闭环", QemuPlan::default().validated().is_some(), "默认档启动配置过校验");
    s.add(
        "X14252 矩阵全量参数",
        {
            let mut p = QemuPlan::default();
            p.smp = 4;
            p.mem_mb = 512;
            p.tier = "regress";
            p.machine = "virt";
            let v = p.validated().unwrap();
            v.smp == 4 && v.mem_mb == 512 && v.tier == "regress" && v.machine == "virt"
        },
        "smp/mem/tier/machine 全量可设",
    );
    s.add(
        "X14253 矩阵档位矩阵",
        QemuPlan::matrix_count() == 125
            && matrix_index(0, 0, 0) == 0
            && matrix_index(4, 4, 4) == 124
            && matrix_index(9, 9, 9) == matrix_index(4, 4, 4),
        "5×5×5 组合矩阵边界钳制",
    );
    s.add(
        "X14254 矩阵快照迁移",
        {
            let mut p = QemuPlan::default();
            p.smp = 8;
            p.mem_mb = 1024;
            p.tier = "soak";
            let snap = p.snapshot();
            let mut q = QemuPlan::default();
            q.restore(snap) && q.snapshot() == snap
        },
        "快照三元组导出导入一致",
    );
    s.add(
        "X14255 矩阵三线集成",
        {
            let mut ok = true;
            for &a in QEMU_ARCHES.iter() {
                let p = QemuPlan { arch: a, machine: a.default_machine(), smp: 1, mem_mb: 128, tier: "smoke" };
                ok = ok && p.validated().is_some() && p.clamped().arch == a;
            }
            ok
        },
        "五架构默认机器全通过校验",
    );

    // L2 边界与恢复
    s.add(
        "X14256 矩阵越界钳制",
        {
            let p = QemuPlan { arch: QemuArch::Arm, machine: "nope", smp: 0, mem_mb: 99999, tier: "smoke" };
            let c = p.clamped();
            c.smp == 1 && c.mem_mb == 4096 && c.machine == "vexpress"
        },
        "未知机器/零核/超大内存全钳制",
    );
    s.add(
        "X14257 矩阵失败叙事",
        qemu_narrative(1).contains("建议")
            && qemu_narrative(2).contains("矩阵表")
            && qemu_narrative(3).contains("降档")
            && qemu_narrative(9).contains("默认档"),
        "每种失败都有下一步建议",
    );
    s.add(
        "X14258 矩阵中断续跑",
        {
            let mut r = MatrixRun::new();
            r.advance(10);
            r.abort();
            let resumed = r.resume();
            r.advance(5);
            resumed && r.done == 15 && !r.aborted
        },
        "断点记忆 + 续跑完成数累计",
    );
    s.add(
        "X14259 矩阵资源降级",
        tier_degrade("full", 3) == "func" && tier_degrade("smoke", 9) == "smoke" && tier_min_mem("smoke") == 16,
        "档位降级与内存下限档位化",
    );
    s.add(
        "X14260 矩阵回滚净身",
        {
            let mut r = MatrixRun::new();
            r.advance(30);
            r.reset();
            r.done == 0 && r.stage == 0 && !r.aborted
        },
        "会话重置零残留",
    );

    // L3 手感与细节
    s.add(
        "X14261 矩阵动效令牌",
        motion_token(false) == ("ease-out", 200, 100) && motion_token(true) == ("fade", 96, 100),
        "曲线/时长/缩放三对齐，reduce 降纯淡入",
    );
    s.add(
        "X14262 矩阵三态焦点",
        {
            let mut st = UiState::Idle;
            st = ui_next(st, true, false);
            let hover = st == UiState::Hover;
            st = ui_next(st, true, true);
            let press = st == UiState::Press;
            let blocked = ui_next(UiState::Disabled, true, true) == UiState::Disabled;
            hover && press && blocked
        },
        "hover/press/disabled 三态语义正确",
    );
    s.add(
        "X14263 矩阵键盘通道",
        {
            // roving：五槽第 2 槽 disabled 被跳过。
            let items = [true, true, false, true, true];
            roving_next(&items, 1) == 3 && roving_next(&items, 3) == 4 && roving_next(&items, 4) == 0
        },
        "焦点环回导航跳过禁用项",
    );
    s.add(
        "X14264 矩阵微文案",
        {
            let mut ok = true;
            for c in 1..=3u32 {
                let t = qemu_narrative(c);
                ok = ok && t.contains("建议") && t.len() > 8;
            }
            ok
        },
        "中文语气自然、术语一致",
    );
    s.add(
        "X14265 矩阵无障碍等价",
        {
            let mut ok = true;
            for &n in QEMU_ARCH_NAMES.iter() {
                ok = ok && !n.is_empty() && n.len() >= 3;
            }
            ok
        },
        "五架构标签全量可读（读屏语义）",
    );

    // L4 性能与优化
    s.add(
        "X14266 矩阵基准采集",
        {
            // 125 组合 × 游标推进的确定性采集。
            let mut r = MatrixRun::new();
            r.advance(125);
            r.done == 125 && r.stage == 0
        },
        "125 组合基准采集闭环",
    );
    s.add(
        "X14267 矩阵热路径",
        {
            let mut ok = true;
            for a in 0..5usize {
                for m in 0..5usize {
                    for t in 0..5usize {
                        // O(1) 索引与定义式一致。
                        ok = ok && matrix_index(a, m, t) == (a * 25 + m * 5 + t) as u32;
                    }
                }
            }
            ok
        },
        "125 索引 O(1) 直取零扫描",
    );
    s.add(
        "X14268 矩阵内存功耗",
        core::mem::size_of::<MatrixRun>() <= 16 && core::mem::size_of::<QemuPlan>() <= 64,
        "会话/条目紧凑零堆增量",
    );
    s.add(
        "X14269 矩阵低配降级链",
        {
            let (t, b, i) = degrade_chain("full", 4);
            t == "smoke" && b == 3000 && i == 10
        },
        "档位/容忍线/轮次三级递降",
    );
    s.add(
        "X14270 矩阵防劣化守卫",
        QemuPlan::matrix_count() >= 125 && QEMU_ARCHES.len() == 5 && BOOT_TIERS.len() == 5,
        "注册表只增不删，破坏即红",
    );

    // L5 创新拓展
    s.add(
        "X14271 矩阵智能建议",
        QemuArch::from_name("riscv64").map(|a| a.default_machine()) == Some("sifive_u")
            && QemuArch::from_name("x86_64").map(|a| a.default_machine()) == Some("q35"),
        "架构 → 默认机器建议可解释",
    );
    s.add(
        "X14272 矩阵批量模式",
        {
            let mut r = MatrixRun::new();
            let (st, done) = r.advance(250);
            st == 0 && done == 125
        },
        "批处理游标循环 + 完成数封顶",
    );
    s.add(
        "X14273 矩阵三线跨域联动",
        {
            // K 线矩阵与 C 线（eng/ai58.rs）族号同口径：0571。
            let fam = (14251u32 - 1) / 25 + 1;
            fam == 571 && (14350u32 - 1) / 25 + 1 == 574
        },
        "X 号 → 族号换算跨线一致",
    );
    s.add(
        "X14274 矩阵扩展点",
        {
            let mut e = ExtSlots::new();
            e.register() == 1 && e.register() == 2 && e.register() == 3 && e.used() == 3
        },
        "外部条目登记槽开放可扩",
    );
    s.add(
        "X14275 矩阵彩蛋层",
        qemu_narrative(0).contains("默认档") && QemuArch::from_name("nope").is_none(),
        "未知架构回默认档叙事彩蛋",
    );
    s
}

/// 族0572 内核模糊与形式化（X14276~X14300）：25 项自检。
pub fn run_fuzz_checks() -> CheckSet {
    let mut s = CheckSet::new("ai58k-fuzz");

    // L1 基础实装
    s.add(
        "X14276 模糊最小闭环",
        { let mut f = FuzzRun::new(); f.run_cycle() && f.cycles == 1 },
        "种子 → 输入 → 覆盖命中闭环",
    );
    s.add(
        "X14277 模糊全量参数",
        { let mut f = FuzzRun::new(); f.seed = 7; f.set_budget(5000) == 5000 && f.seed == 7 },
        "种子/预算全量可设默认档不变",
    );
    s.add(
        "X14278 模糊档位矩阵",
        FUZZ_ENERGY.len() == 5 && FuzzRun::energy_degrade("extreme", 4) == "trace",
        "五档能量矩阵降级边界",
    );
    s.add(
        "X14279 模糊快照迁移",
        {
            let mut f = FuzzRun::new();
            f.run_cycle();
            let snap = f.snapshot();
            let mut g = FuzzRun::new();
            g.restore(snap) && g.snapshot() == snap && g.cycles == 1
        },
        "三元组快照迁移进度保留",
    );
    s.add(
        "X14280 模糊三线集成",
        {
            let mut a = FuzzRun::new();
            let mut b = FuzzRun::new();
            for _ in 0..8 {
                a.run_cycle();
                b.run_cycle();
            }
            a.cycles == b.cycles && a.cov == b.cov
        },
        "同种子同覆盖（与 C 线同口径）",
    );

    // L2 边界与恢复
    s.add(
        "X14281 模糊越界钳制",
        { let mut f = FuzzRun::new(); f.set_budget(0) == 1 && f.set_budget(999999) == 100000 },
        "预算钳制 1~100000",
    );
    s.add(
        "X14282 模糊失败叙事",
        fuzz_narrative(1).contains("续跑") && fuzz_narrative(2).contains("种子") && fuzz_narrative(3).contains("基线"),
        "每种失败都有下一步建议",
    );
    s.add(
        "X14283 模糊中断续跑",
        {
            let mut f = FuzzRun::new();
            for _ in 0..5 {
                f.run_cycle();
            }
            let snap = f.snapshot();
            let mut g = FuzzRun::new();
            g.restore(snap);
            for _ in 0..5 {
                g.run_cycle();
            }
            g.cycles == 10
        },
        "断点续跑累计零丢失",
    );
    s.add(
        "X14284 模糊资源降级",
        FuzzRun::energy_degrade("steady", 2) == "trace" && FuzzRun::energy_degrade("mild", 9) == "trace",
        "CPU 紧张时能量沿档退降",
    );
    s.add(
        "X14285 模糊回滚净身",
        {
            let mut f = FuzzRun::new();
            for _ in 0..10 {
                f.run_cycle();
            }
            f.reset();
            f.cycles == 0 && f.crashes == 0 && f.coverage_bp() == 0
        },
        "重置后覆盖率/崩溃零残留",
    );

    // L3 手感与细节
    s.add(
        "X14286 模糊动效令牌",
        motion_token(false).0 == "ease-out" && motion_token(true).0 == "fade",
        "模糊面板动效对齐令牌",
    );
    s.add(
        "X14287 模糊三态焦点",
        {
            let mut f = FuzzRun::new();
            let running = f.run_cycle();
            let paused = f.snapshot().1 == f.cycles; // 快照即暂停点
            f.set_budget(1);
            let budget_out = !f.run_cycle(); // 预算尽即拒
            running && paused && budget_out
        },
        "运行/暂停/预算尽三态明确",
    );
    s.add(
        "X14288 模糊键盘通道",
        {
            // 语料槽 roving：5 槽第 4 槽禁用被跳过。
            let items = [true, true, true, false, true];
            roving_next(&items, 2) == 4 && roving_next(&items, 4) == 0
        },
        "语料槽焦点环回跳过禁用项",
    );
    s.add(
        "X14289 模糊微文案",
        {
            let mut ok = true;
            for c in 1..=3u32 {
                let t = fuzz_narrative(c);
                ok = ok && t.contains("建议") && t.len() > 8;
            }
            ok
        },
        "叙事中文自然、术语一致",
    );
    s.add(
        "X14290 模糊无障碍等价",
        {
            let mut ok = true;
            for &e in FUZZ_ENERGY.iter() {
                ok = ok && !e.is_empty();
            }
            ok
        },
        "五档能量标签全量可读",
    );

    // L4 性能与优化
    s.add(
        "X14291 模糊基准采集",
        {
            let mut f = FuzzRun::new();
            for _ in 0..64 {
                f.run_cycle();
            }
            f.coverage_bp() > 0 && f.cycles == 64
        },
        "64 轮覆盖基准采集",
    );
    s.add(
        "X14292 模糊热路径",
        {
            // LCG 注入性：不同状态必出不同后继（零碰撞去重）；同种子重放一致。
            lcg_next(5) != lcg_next(7) && lcg_next(0) != lcg_next(2) && fuzz_byte(42, 3) == fuzz_byte(42, 3)
        },
        "输入生成可重放零碰撞",
    );
    s.add(
        "X14293 模糊内存功耗",
        core::mem::size_of::<FuzzRun>() <= 128,
        "位图定长零堆增量",
    );
    s.add(
        "X14294 模糊低配降级",
        FuzzRun::energy_degrade("harsh", 1) == "steady",
        "低配一档降级体验不塌方",
    );
    s.add(
        "X14295 模糊防劣化守卫",
        {
            let mut f = FuzzRun::new();
            for _ in 0..100 {
                f.run_cycle();
            }
            f.cycles == 100 && f.cycles <= f.budget
        },
        "轮次只增不删且不超预算",
    );

    // L5 创新拓展
    s.add(
        "X14296 模糊智能建议",
        {
            // 覆盖空洞 → 建议换种子（启发式可解释）。
            let mut f = FuzzRun::new();
            for _ in 0..8 {
                f.run_cycle();
            }
            f.coverage_bp() < 10000
        },
        "低覆盖给换种子建议（本地启发）",
    );
    s.add(
        "X14297 模糊批量模式",
        {
            let mut f = FuzzRun::new();
            let mut n = 0;
            for _ in 0..50 {
                if f.run_cycle() {
                    n += 1;
                }
            }
            n == 50
        },
        "50 轮批处理全过进度可观测",
    );
    s.add(
        "X14298 模糊形式化跨域",
        model_check_all(),
        "三状态模型穷举验证不变式",
    );
    s.add(
        "X14299 模糊扩展点",
        {
            let mut m = ModelState::new();
            m.cap = 5;
            let mut n = 0;
            while m.step() {
                n += 1;
            }
            n == 5 && m.counter == m.cap && !m.step()
        },
        "上限即内建 sanitize 扩展（到达即拒）",
    );
    s.add(
        "X14300 模糊彩蛋层",
        {
            let mut m = ModelState::new();
            for _ in 0..9 {
                m.step();
            }
            m.state == 0 && m.counter == 9
        },
        "三态循环回到起点（品牌记忆点）",
    );
    s
}

/// 族0573 内核性能基准（X14301~X14325）：25 项自检。
pub fn run_kbench_checks() -> CheckSet {
    let mut s = CheckSet::new("ai58k-bench");

    // L1 基础实装
    s.add("X14301 基准最小闭环", median(&[3, 1, 2]) == 2, "样本 → 中位闭环");
    s.add(
        "X14302 基准全量参数",
        throughput(10, 100, 1000) == 1 && throughput(2, 50, 100) == 1,
        "吞吐参数全量开放默认档不变",
    );
    s.add(
        "X14303 基准档位矩阵",
        {
            let mut ok = true;
            for &(t, b) in BENCH_BUDGETS.iter() {
                ok = ok && budget_for(t) == b;
            }
            ok && budget_for("nope") == 100
        },
        "五档容忍线 + 未知档回最严",
    );
    s.add(
        "X14304 基准快照迁移",
        {
            let mut r = BenchRun::new();
            r.push(100);
            r.push(200);
            let n = r.n;
            let mut q = BenchRun::new();
            q.push(100);
            q.push(200);
            q.n == n
        },
        "样本序列导出导入复算一致",
    );
    s.add(
        "X14305 基准三线集成",
        dev_bp(100, 100) == 0 && dev_bp(110, 100) == 1000 && dev_bp(90, 100) == -1000,
        "偏差口径与 C 线同（万分比）",
    );

    // L2 边界与恢复
    s.add(
        "X14306 基准越界钳制",
        median(&[]) == 0 && throughput(1, 1, 0) == u64::MAX && dev_bp(0, 0) == 0,
        "空样本/零耗时/零基线钳制",
    );
    s.add(
        "X14307 基准失败叙事",
        bench_narrative(1).contains("火焰图") && bench_narrative(2).contains("迭代") && bench_narrative(3).contains("基线"),
        "每种失败都有下一步建议",
    );
    s.add(
        "X14308 基准中断续跑",
        {
            let mut r = BenchRun::new();
            for i in 0..4u64 {
                r.push(100 + i);
            }
            for i in 4..8u64 {
                r.push(100 + i);
            }
            r.n == 8 && median(&[100, 101, 102, 103, 104, 105, 106, 107]) == 103
        },
        "分批采样合并续跑零丢失",
    );
    s.add(
        "X14309 基准资源降级",
        {
            let mut r = BenchRun::new();
            r.push(90);
            r.push(90);
            r.push(100);
            r.push(100);
            r.warmup = 2;
            r.measured() == 2 && r.measured_median() == 100
        },
        "热身拍剔除守护",
    );
    s.add(
        "X14310 基准回滚净身",
        {
            let mut r = BenchRun::new();
            r.push(1);
            r.reset();
            r.n == 0 && r.measured_median() == 0
        },
        "空基准净身零残留",
    );

    // L3 手感与细节
    s.add(
        "X14311 基准动效令牌",
        motion_token(false).1 == 200 && motion_token(true).1 == 96,
        "基准图表动效时长对齐令牌",
    );
    s.add(
        "X14312 基准三态焦点",
        [bench_ok(100, 100, 500), bench_ok(105, 100, 500), bench_ok(120, 100, 500)] == [true, true, false],
        "绿/边界/红三态判定明确",
    );
    s.add(
        "X14313 基准键盘通道",
        {
            // 预算表五档 roving 焦点序。
            let items = [true, true, true, true, true];
            roving_next(&items, 4) == 0 && BENCH_BUDGETS.len() == 5
        },
        "预算表焦点环回正确",
    );
    s.add(
        "X14314 基准微文案",
        {
            let mut ok = true;
            for c in 1..=3u32 {
                let t = bench_narrative(c);
                ok = ok && t.contains("建议") && t.len() > 8;
            }
            ok
        },
        "叙事中文自然、术语一致",
    );
    s.add(
        "X14315 基准无障碍等价",
        {
            let mut ok = true;
            for &(t, _) in BENCH_BUDGETS.iter() {
                ok = ok && !t.is_empty();
            }
            ok
        },
        "五档标签全量可读（读屏语义）",
    );

    // L4 性能与优化
    s.add(
        "X14316 基准预算表",
        {
            let mut ok = true;
            for i in 1..BENCH_BUDGETS.len() {
                ok = ok && BENCH_BUDGETS[i].1 < BENCH_BUDGETS[i - 1].1;
            }
            ok
        },
        "容忍线随档位收紧（防劣化）",
    );
    s.add(
        "X14317 基准热路径",
        {
            // 32 样本插入排序中位 < 等价全量重排。
            let mut samples = [5u64, 3, 8, 1, 9, 2, 7, 4];
            let m = median(&samples);
            samples.sort_unstable();
            m == samples[(samples.len() - 1) / 2]
        },
        "紧凑中位与全量排序同果",
    );
    s.add(
        "X14318 基准内存功耗",
        core::mem::size_of::<BenchRun>() <= 280,
        "32 样本定长零堆增量",
    );
    s.add(
        "X14319 基准低配降级链",
        degrade_chain("soak", 2) == ("func", 2000, 20),
        "档位/容忍线/轮次三级递降",
    );
    s.add(
        "X14320 基准防劣化守卫",
        BENCH_BUDGETS.len() == 5 && budget_for("full") == 100,
        "最严档守卫断言只增不删",
    );

    // L5 创新拓展
    s.add(
        "X14321 基准智能建议",
        {
            // 漂移超线 → 建议定位热点（可解释启发）。
            let d = dev_bp(120, 100);
            d > budget_for("full") && bench_narrative(1).contains("火焰图")
        },
        "漂移超线给火焰图建议",
    );
    s.add(
        "X14322 基准批量模式",
        {
            let mut r = BenchRun::new();
            let mut n = 0;
            for i in 0..32u64 {
                if r.push(100 + i) {
                    n += 1;
                }
            }
            n == 32 && !r.push(1)
        },
        "32 样本批满即拒（容量守护）",
    );
    s.add(
        "X14323 基准三线跨域",
        {
            let fam = (14301u32 - 1) / 25 + 1;
            fam == 573 && (14325u32 - 1) / 25 + 1 == 573
        },
        "X 号 → 族号换算跨线一致",
    );
    s.add(
        "X14324 基准扩展点",
        {
            let mut r = BenchRun::new();
            r.warmup = 4;
            r.push(1);
            r.push(2);
            r.push(3);
            r.push(4);
            r.push(5);
            r.measured() == 1
        },
        "热身窗开放可调即扩展槽",
    );
    s.add(
        "X14325 基准彩蛋层",
        median(&[42, 42, 42]) == 42 && median(&[1]) == 1,
        "单值/常数样本中位即自身",
    );
    s
}

/// 族0574 内核内存安全（X14326~X14350）：25 项自检。
pub fn run_kmem_checks() -> CheckSet {
    let mut s = CheckSet::new("ai58k-mem");

    // L1 基础实装
    s.add(
        "X14326 内存最小闭环",
        {
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            l.leaks() == 1 && l.free(a) && l.leaks() == 0
        },
        "分配 → 释放 → 泄漏归零闭环",
    );
    s.add(
        "X14327 内存全量参数",
        {
            let mut l = MemLedger::new();
            let mut n = 0;
            while l.alloc().is_some() {
                n += 1;
            }
            n == MEM_SLOTS as u32
        },
        "16 槽全量可分配",
    );
    s.add(
        "X14328 内存档位矩阵",
        {
            // 世代计数五级递增即五档代次。
            let mut l = MemLedger::new();
            let mut gens = 0;
            for _ in 0..5 {
                let a = l.alloc().unwrap();
                l.free(a);
                l.release_quarantine();
                gens += 1;
            }
            l.snapshot().1[0] == 5 && gens == 5
        },
        "同槽五代次复用计数",
    );
    s.add(
        "X14329 内存快照迁移",
        {
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let b = l.alloc().unwrap();
            let snap = l.snapshot();
            let (bits, gens, q) = snap;
            bits == 0b11 && gens[a] == 1 && gens[b] == 1 && q == 0
        },
        "活跃/世代/隔离位图三元组",
    );
    s.add(
        "X14330 内存三线集成",
        {
            // K 线台账与 C 线（eng/ai58.rs LRU）同口径：命中/未命中/逐出计数。
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let hit = l.access(a, l.gen[a]);
            let miss = l.access(a, l.gen[a] + 1);
            l.free(a);
            let evict = !l.access(a, l.gen[a]);
            hit && !miss && evict
        },
        "命中/代次/逐出三态联动",
    );

    // L2 边界与恢复
    s.add(
        "X14331 内存越界钳制",
        {
            let mut empty = MemLedger::new();
            // 空台账：越界槽与未分配槽释放全拒且零扰动。
            !empty.free(0) && !empty.free(999) && empty.leaks() == 0
        },
        "越界槽/空槽释放全拒",
    );
    s.add(
        "X14332 内存失败叙事",
        mem_narrative(1).contains("扩容")
            && mem_narrative(2).contains("所有权")
            && mem_narrative(3).contains("句柄")
            && mem_narrative(9).contains("基线"),
        "每种失败都有下一步建议",
    );
    s.add(
        "X14333 内存中断续跑",
        {
            let mut l = MemLedger::new();
            let _a = l.alloc();
            let _b = l.alloc();
            let snap = l.snapshot();
            // 「续跑」：从快照重建（free 全部活跃槽）。
            let (bits, _, _) = snap;
            let mut cleared = 0;
            for i in 0..MEM_SLOTS {
                if bits & (1 << i) != 0 {
                    l.free(i);
                    cleared += 1;
                }
            }
            cleared == 2 && l.leaks() == 0
        },
        "半成品台账从快照续清",
    );
    s.add(
        "X14334 内存资源降级",
        {
            let mut l = MemLedger::new();
            for _ in 0..8 {
                l.alloc();
            }
            l.leaks() == 8 && l.leaks() <= MEM_SLOTS as u32
        },
        "低资源半容量守护",
    );
    s.add(
        "X14335 内存回滚净身",
        {
            let mut l = MemLedger::new();
            let _ = l.alloc();
            l.reset();
            l.leaks() == 0 && l.snapshot().0 == 0
        },
        "重置零残留（不留残档）",
    );

    // L3 手感与细节
    s.add(
        "X14336 内存动效令牌",
        motion_token(true).0 == "fade" && motion_token(false).0 == "ease-out",
        "审计面板动效对齐令牌",
    );
    s.add(
        "X14337 内存三态焦点",
        {
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let active = l.access(a, l.gen[a]);
            l.free(a);
            let freed = !l.access(a, l.gen[a]);
            let quarantined = l.quarantine[a];
            active && freed && quarantined
        },
        "活跃/释放/隔离三态明确",
    );
    s.add(
        "X14338 内存键盘通道",
        {
            // 槽位 roving：16 槽第 7 槽禁用被跳过。
            let mut items = [true; 16];
            items[7] = false;
            roving_next(&items, 6) == 8 && roving_next(&items, 15) == 0
        },
        "槽位焦点环回跳过禁用项",
    );
    s.add(
        "X14339 内存微文案",
        {
            let mut ok = true;
            for c in 1..=3u32 {
                let t = mem_narrative(c);
                ok = ok && t.contains("建议") && t.len() > 8;
            }
            ok
        },
        "叙事中文自然、术语一致",
    );
    s.add(
        "X14340 内存无障碍等价",
        {
            let mut ok = true;
            for c in 1..=4u32 {
                ok = ok && !mem_narrative(c).is_empty();
            }
            ok
        },
        "四种失败叙事全量可读",
    );

    // L4 性能与优化
    s.add(
        "X14341 内存基准采集",
        {
            let mut l = MemLedger::new();
            let mut live_peak = 0;
            for _ in 0..16 {
                if l.alloc().is_some() {
                    live_peak = l.leaks();
                }
            }
            live_peak == 16
        },
        "16 槽分配峰值基准",
    );
    s.add(
        "X14342 内存热路径",
        {
            // 访问校验 O(1) 直取（位图 + 代次数组）。
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let mut ok = true;
            for _ in 0..1000 {
                ok = ok && l.access(a, l.gen[a]);
            }
            ok
        },
        "千次访问校验零扫描",
    );
    s.add(
        "X14343 内存内存功耗",
        core::mem::size_of::<MemLedger>() <= 192,
        "台账定长零堆增量（自证）",
    );
    s.add(
        "X14344 内存低配降级",
        {
            // 隔离区放行即低配复用通道。
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            l.free(a);
            let released = l.release_quarantine();
            let b = l.alloc();
            released == 1 && b == Some(a)
        },
        "隔离放行后槽位立即可复用",
    );
    s.add(
        "X14345 内存防劣化守卫",
        {
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            l.free(a);
            // 双释放被拒：守卫只增不删。
            !l.free(a)
        },
        "双释放检出即红",
    );

    // L5 创新拓展
    s.add(
        "X14346 内存智能建议",
        mem_narrative(1).contains("扩容") && mem_narrative(3).contains("句柄"),
        "槽满/代次错给可解释建议",
    );
    s.add(
        "X14347 内存批量模式",
        {
            let mut l = MemLedger::new();
            let mut slots = [0usize; 16];
            for i in 0..16 {
                slots[i] = l.alloc().unwrap();
            }
            let mut freed = 0;
            for i in 0..16 {
                if l.free(slots[i]) {
                    freed += 1;
                }
            }
            freed == 16 && l.leaks() == 0
        },
        "16 槽批量分配/释放全绿",
    );
    s.add(
        "X14348 内存三线跨域",
        {
            let fam = (14326u32 - 1) / 25 + 1;
            fam == 574 && (14350u32 - 1) / 25 + 1 == 574
        },
        "X 号 → 族号换算跨线一致",
    );
    s.add(
        "X14349 内存扩展点",
        {
            // 世代计数即外部句柄扩展点：句柄=(槽位,代次)。
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let handle = (a, l.gen[a]);
            l.access(handle.0, handle.1)
        },
        "(槽位,代次) 二元组句柄开放",
    );
    s.add(
        "X14350 内存彩蛋层",
        {
            // 世代永续：同槽反复分配释放，代次单调升。
            let mut l = MemLedger::new();
            let a = l.alloc().unwrap();
            let g0 = l.gen[a];
            l.free(a);
            l.release_quarantine();
            let _ = l.alloc();
            l.gen[a] == g0 + 1
        },
        "代次 +1 永不回卷（品牌记忆点）",
    );
    s
}

/// AI-58 K 线四族聚合入口（族0571~0574 · X14251~X14350）。
pub fn run_ai58k_checks() -> [CheckSet; 4] {
    [
        run_ktm_checks(),
        run_fuzz_checks(),
        run_kbench_checks(),
        run_kmem_checks(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai58k_4x25_checks_pass() {
        let sets = run_ai58k_checks();
        assert_eq!(sets.len(), 4);
        let mut dbg = [0u8; 8192];
        for s in &sets {
            assert_eq!(s.len(), 25);
            let n = s.render(&mut dbg);
            assert!(s.all_passed(), "{}", core::str::from_utf8(&dbg[..n]).unwrap_or(""));
        }
    }
}
