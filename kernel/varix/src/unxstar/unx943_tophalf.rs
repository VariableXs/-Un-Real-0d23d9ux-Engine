//! unx943_tophalf —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单）· 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 前驱契约：UNX-F0941（两级分界）、UNX-F0942（分发入口统一路径 <300ns）
//! 组件：预算阈值族（QEMU 档硬线 ≤5µs，五档）+ 成本计量（纳秒 + 步数账）
//!      + 最小化清单（白名单闭集）+ 禁令闭集（八类）+ 义务闭集（五类）
//!      + 栈水位账（IRQ 栈 4KiB 红线）+ 直方图 p50/p99 分位（超预算清单自动产出）
//!      + 纪律文档冻结（三节 FNV 指纹，篡改即显性）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TH_VERSION: &str = "unx943_tophalf-v1";

/// 诊断段基址（0x4c50 段 · 本模块独占）。
pub const TH_SEG_BASE: u16 = 0x4c50;

/// QEMU 档 top half 预算硬线（判据：p99 ≤ 5µs = 5000 纳秒）。
pub const TH_BUDGET_NS_QEMU: u64 = 5_000;

/// 生产档更严预算（≤2µs，对比档；QEMU TCG 有模拟开销故放宽）。
pub const TH_BUDGET_NS_PROD: u64 = 2_000;

/// p99 分位桶数（1024 桶直方图）。
pub const TH_HIST_BUCKETS: usize = 1024;

/// 桶宽（纳秒/桶）。
pub const TH_BUCKET_WIDTH_NS: u64 = 8;

/// 直方图覆盖上限（桶数 × 桶宽 = 8192ns，超此落入溢出桶）。
pub const TH_HIST_MAX_NS: u64 = TH_BUCKET_WIDTH_NS * TH_HIST_BUCKETS as u64;

/// 统计置信最低样本量（样本 <100 不足以判 p99，显性拒绝）。
pub const TH_MIN_SAMPLES: u64 = 100;

/// 分位目标（99 分位）。
pub const TH_P99_NUM: u64 = 99;

/// IRQ 栈红线（4KiB = 4096 字节，Σ栈帧必须 ≤ 红线）。
pub const TH_IRQ_STACK_REDLINE_B: u32 = 4_096;

/// 超预算清单产出目标次数（判据 10/10）。
pub const TH_OVERLIST_TARGET: u64 = 10;

/// 违例注入拦截目标（10/10）。
pub const TH_INTERCEPT_TARGET: u64 = 10;

/// 单原子操作步数成本（经验步长，ns/步）。
pub const TH_STEP_NS: u64 = 12;

/// FNV-1a 64 位偏移基准。
pub const TH_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位质数。
pub const TH_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 预算分档（五档闭集，判据预算断言核心）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetGrade {
    /// 充裕：≤ 预算 20%（≤1µs）。
    Roomy,
    /// 临界：预算 20%~60%（1µs~3µs）。
    Borderline,
    /// 预算内：预算 60%~100%（3µs~5µs，达标不宽裕）。
    Within,
    /// 超支：预算 100%~150%（5µs~7.5µs，违约）。
    Over,
    /// 严重超支：> 预算 150%（>7.5µs，硬违约）。
    Critical,
}

impl BudgetGrade {
    /// 全量闭集。
    pub const ALL: [BudgetGrade; 5] = [
        BudgetGrade::Roomy,
        BudgetGrade::Borderline,
        BudgetGrade::Within,
        BudgetGrade::Over,
        BudgetGrade::Critical,
    ];

    /// 由纳秒成本对预算判档（纯整数比较，无浮点）。
    pub const fn of_ns(ns: u64, budget: u64) -> BudgetGrade {
        let fifth = budget / 5;
        let three_fifths = budget.saturating_mul(3) / 5;
        let one_half_extra = budget + budget / 2;
        if ns <= fifth {
            BudgetGrade::Roomy
        } else if ns <= three_fifths {
            BudgetGrade::Borderline
        } else if ns <= budget {
            BudgetGrade::Within
        } else if ns <= one_half_extra {
            BudgetGrade::Over
        } else {
            BudgetGrade::Critical
        }
    }

    /// 是否违约（超支或严重超支）。
    pub const fn violates(self) -> bool {
        matches!(self, BudgetGrade::Over | BudgetGrade::Critical)
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            BudgetGrade::Roomy => "充裕",
            BudgetGrade::Borderline => "临界",
            BudgetGrade::Within => "预算内",
            BudgetGrade::Over => "超支",
            BudgetGrade::Critical => "严重超支",
        }
    }
}

/// top half 最小化清单·白名单操作（闭集：top half 只许做这四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopAllowed {
    /// 应答中断控制器（EOI/掩码位清除）。
    AckController,
    /// 摘取硬件快照（寄存器一次性读入栈上结构）。
    TakeSnapshot,
    /// 置 pending 标志 / 挂 bottom half。
    RaisePending,
    /// 轻量计数统计（步数入账，无分配无锁）。
    LightStat,
}

impl TopAllowed {
    /// 全量闭集。
    pub const ALL: [TopAllowed; 4] = [
        TopAllowed::AckController,
        TopAllowed::TakeSnapshot,
        TopAllowed::RaisePending,
        TopAllowed::LightStat,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TopAllowed::AckController => "应答控制器",
            TopAllowed::TakeSnapshot => "摘取快照",
            TopAllowed::RaisePending => "置 pending",
            TopAllowed::LightStat => "轻量统计",
        }
    }

    /// 该操作的固定步数成本（原子操作数）。
    pub const fn steps(self) -> u32 {
        match self {
            TopAllowed::AckController => 2,
            TopAllowed::TakeSnapshot => 4,
            TopAllowed::RaisePending => 3,
            TopAllowed::LightStat => 1,
        }
    }
}


/// top half 禁令闭集（八类禁做）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopForbid {
    /// 禁动态分配（alloc/malloc）。
    ForbidAlloc,
    /// 禁睡眠/让出。
    ForbidSleep,
    /// 禁阻塞锁。
    ForbidBlockingLock,
    /// 禁复杂运算/浮点。
    ForbidHeavyMath,
    /// 禁遍历大表（O(n) 遍历容量 > 64 的表）。
    ForbidBigScan,
    /// 禁触盘/IO。
    ForbidIo,
    /// 禁打详细日志（格式化输出）。
    ForbidLogging,
    /// 禁无界循环。
    ForbidUnbounded,
}

impl TopForbid {
    /// 全量闭集（八类）。
    pub const ALL: [TopForbid; 8] = [
        TopForbid::ForbidAlloc,
        TopForbid::ForbidSleep,
        TopForbid::ForbidBlockingLock,
        TopForbid::ForbidHeavyMath,
        TopForbid::ForbidBigScan,
        TopForbid::ForbidIo,
        TopForbid::ForbidLogging,
        TopForbid::ForbidUnbounded,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TopForbid::ForbidAlloc => "禁动态分配",
            TopForbid::ForbidSleep => "禁睡眠让出",
            TopForbid::ForbidBlockingLock => "禁阻塞锁",
            TopForbid::ForbidHeavyMath => "禁复杂运算",
            TopForbid::ForbidBigScan => "禁遍历大表",
            TopForbid::ForbidIo => "禁磁盘IO",
            TopForbid::ForbidLogging => "禁详细日志",
            TopForbid::ForbidUnbounded => "禁无界循环",
        }
    }

    /// 违例后的可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TopForbid::ForbidAlloc => "改用栈上固定缓冲或预分配池，把分配下沉到 bottom half",
            TopForbid::ForbidSleep => "取消让出，用 pending 挂起重活交 bottom half",
            TopForbid::ForbidBlockingLock => "改用自旋锁且持锁步数 ≤ 红线，或下沉取锁",
            TopForbid::ForbidHeavyMath => "预计算或查表，复杂运算移出中断上下文",
            TopForbid::ForbidBigScan => "限制遍历规模或改用增量结构，避免 O(n) 全扫",
            TopForbid::ForbidIo => "IO 一律交 bottom half/workqueue，中断内不触盘",
            TopForbid::ForbidLogging => "中断内只做轻计数，格式化日志下沉到消费侧",
            TopForbid::ForbidUnbounded => "加显式上界与步数红线，杜绝中断内死循环",
        }
    }
}

/// top half 义务闭集（五类必做，缺一即违约）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopOblige {
    /// 义务：必须应答中断控制器。
    MustAck,
    /// 义务：必须摘取硬件快照。
    MustSnap,
    /// 义务：必须挂 pending / 交 bottom half。
    MustPending,
    /// 义务：必须记录步数入账。
    MustStepLedger,
    /// 义务：必须守恒（步数入账 ≡ 出账）。
    MustConserve,
}

impl TopOblige {
    /// 全量闭集（五类）。
    pub const ALL: [TopOblige; 5] = [
        TopOblige::MustAck,
        TopOblige::MustSnap,
        TopOblige::MustPending,
        TopOblige::MustStepLedger,
        TopOblige::MustConserve,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TopOblige::MustAck => "必应答",
            TopOblige::MustSnap => "必摘快照",
            TopOblige::MustPending => "必挂 pending",
            TopOblige::MustStepLedger => "必记步数",
            TopOblige::MustConserve => "必守恒",
        }
    }
}

/// 纪律文档三节（义务/禁令/预算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocSection {
    /// 义务节。
    Oblige,
    /// 禁令节。
    Forbid,
    /// 预算节。
    Budget,
}

impl DocSection {
    /// 全量闭集（三节）。
    pub const ALL: [DocSection; 3] = [DocSection::Oblige, DocSection::Forbid, DocSection::Budget];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            DocSection::Oblige => "义务节",
            DocSection::Forbid => "禁令节",
            DocSection::Budget => "预算节",
        }
    }
}

/// 诊断码（0x4c50 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThCode {
    /// 0x4c50 段位守卫失败。
    SegGuard,
    /// 0x4c51 单 handler 超 QEMU 预算（p99 > 5µs）。
    BudgetExceeded,
    /// 0x4c52 严重超预算（> 预算 150%）。
    BudgetCritical,
    /// 0x4c53 违禁操作注入被拦截（黑名单命中）。
    ForbiddenHit,
    /// 0x4c54 义务缺失（应答/快照/pending/步数任一缺）。
    ObligeMissing,
    /// 0x4c55 步数账不平（入账 ≢ 出账）。
    StepUnbalanced,
    /// 0x4c56 栈水位超红线（Σ帧 > 4KiB）。
    StackOverflow,
    /// 0x4c57 样本量不足（<100 不足以判 p99）。
    SampleThin,
    /// 0x4c58 直方图溢出桶非零且未说明。
    HistOverflow,
    /// 0x4c59 超预算清单空（有违约却无清单条目）。
    OverlistEmpty,
    /// 0x4c5a 超预算清单条数 ≢ 违约数。
    OverlistGap,
    /// 0x4c5b 纪律文档三节不全（义务/禁令/预算缺节）。
    DocIncomplete,
    /// 0x4c5c 文档指纹篡改（现值 ≢ 冻结值）。
    DocTampered,
    /// 0x4c5d 违例注入 ≠ 拦截（计数错位）。
    InterceptGap,
    /// 0x4c5e 分位计算越界（rank 超样本数）。
    PercentileOOR,
    /// 0x4c5f 分档错位（超预算却判预算内）。
    GradeMismatch,
}

impl ThCode {
    /// 全量闭集（十六码）。
    pub const ALL: [ThCode; 16] = [
        ThCode::SegGuard,
        ThCode::BudgetExceeded,
        ThCode::BudgetCritical,
        ThCode::ForbiddenHit,
        ThCode::ObligeMissing,
        ThCode::StepUnbalanced,
        ThCode::StackOverflow,
        ThCode::SampleThin,
        ThCode::HistOverflow,
        ThCode::OverlistEmpty,
        ThCode::OverlistGap,
        ThCode::DocIncomplete,
        ThCode::DocTampered,
        ThCode::InterceptGap,
        ThCode::PercentileOOR,
        ThCode::GradeMismatch,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TH_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ThCode::SegGuard => "段位守卫失败：诊断码须落在 0x4c50 段",
            ThCode::BudgetExceeded => "单 handler 超 QEMU 预算：p99 > 5µs",
            ThCode::BudgetCritical => "严重超预算：p99 > 预算 150%",
            ThCode::ForbiddenHit => "违禁操作注入被拦截：黑名单命中",
            ThCode::ObligeMissing => "义务缺失：应答/快照/pending/步数任一缺",
            ThCode::StepUnbalanced => "步数账不平：入账与出账不守恒",
            ThCode::StackOverflow => "栈水位超红线：Σ帧 > 4KiB",
            ThCode::SampleThin => "样本量不足：<100 不足以判 p99",
            ThCode::HistOverflow => "直方图溢出桶非零且未说明",
            ThCode::OverlistEmpty => "超预算清单空：有违约却无清单条目",
            ThCode::OverlistGap => "超预算清单条数不等于违约数",
            ThCode::DocIncomplete => "纪律文档三节不全：义务/禁令/预算缺节",
            ThCode::DocTampered => "文档指纹篡改：现值不等于冻结值",
            ThCode::InterceptGap => "违例注入不等于拦截：计数错位",
            ThCode::PercentileOOR => "分位计算越界：rank 超样本数",
            ThCode::GradeMismatch => "分档错位：超预算却判预算内",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ThCode::SegGuard => "检查诊断码枚举是否越出本模块 0x4c50 段",
            ThCode::BudgetExceeded => "裁剪 top half 到 ≤5µs：只保留应答/摘快照/置 pending",
            ThCode::BudgetCritical => "重写 handler：把重活全部下沉 bottom half",
            ThCode::ForbiddenHit => "移除违禁调用：分配/睡眠/锁/日志等八类禁做",
            ThCode::ObligeMissing => "补齐缺失义务：应答/快照/pending/步数四件必做",
            ThCode::StepUnbalanced => "核对步数账：每入账必须有对应出账",
            ThCode::StackOverflow => "缩小栈帧或改堆分配，Σ帧 ≤ 4KiB 红线",
            ThCode::SampleThin => "加大采样至 ≥100 次再判分位",
            ThCode::HistOverflow => "检查是否漏记超大样本或桶宽需调",
            ThCode::OverlistEmpty => "排查超预算清单产出逻辑是否漏触发",
            ThCode::OverlistGap => "核对清单条目与违约计数的一一对应",
            ThCode::DocIncomplete => "补齐纪律文档义务/禁令/预算三节",
            ThCode::DocTampered => "文档被改：回滚到冻结指纹对应版本",
            ThCode::InterceptGap => "核对注入数与拦截数是否严格相等",
            ThCode::PercentileOOR => "检查 rank 计算是否超出样本总数",
            ThCode::GradeMismatch => "检查分档阈值比较方向是否写反",
        }
    }
}


/// FNV-1a 64 位增量指纹（对字节切片，用于文档冻结与契约白名单）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = TH_FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(TH_FNV_PRIME);
        i += 1;
    }
    h
}

/// 步数账：top half 每次操作按固定步数入账，出账核守恒。
/// 判据 MustStepLedger / MustConserve：入账步数 ≡ 出账步数。
#[derive(Clone, Debug)]
pub struct StepLedger {
    /// 入账总步数。
    pub in_steps: u64,
    /// 出账总步数。
    pub out_steps: u64,
    /// 入账次数。
    pub in_ops: u64,
    /// 出账次数。
    pub out_ops: u64,
}

impl StepLedger {
    /// 空账。
    pub const fn new() -> Self {
        StepLedger { in_steps: 0, out_steps: 0, in_ops: 0, out_ops: 0 }
    }

    /// 按白名单操作入账（步数 = 操作.steps()）。
    pub fn charge(&mut self, op: TopAllowed) {
        self.in_steps += op.steps() as u64;
        self.in_ops += 1;
    }

    /// 出账（bottom half 消化同等步数）。
    pub fn discharge(&mut self, steps: u64) {
        self.out_steps += steps;
        self.out_ops += 1;
    }

    /// 守恒核：入账 ≡ 出账（步数与次数都要平）。
    pub fn balanced(&self) -> bool {
        self.in_steps == self.out_steps && self.in_ops == self.out_ops
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "步数账：入账 {} 步/{} 次，出账 {} 步/{} 次，守恒 {}",
            self.in_steps,
            self.in_ops,
            self.out_steps,
            self.out_ops,
            self.balanced()
        )
    }
}

impl Default for StepLedger {
    fn default() -> Self {
        StepLedger::new()
    }
}

/// 栈水位账：Σ栈帧必须 ≤ IRQ 栈 4KiB 红线（判据 StackOverflow）。
#[derive(Clone, Debug)]
pub struct StackLedger {
    /// 各帧字节数的上限累计（声明式，运行期以最坏帧估）。
    pub frames_b: Vec<u32>,
    /// 红线（默认 4KiB）。
    pub redline_b: u32,
}

impl StackLedger {
    /// 以红线建账。
    pub const fn new(redline_b: u32) -> Self {
        StackLedger { frames_b: Vec::new(), redline_b }
    }

    /// 登记一帧字节数。
    pub fn push_frame(&mut self, frame_b: u32) {
        self.frames_b.push(frame_b);
    }

    /// Σ 帧字节（u64 防溢出）。
    pub fn sum_b(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i = 0;
        while i < self.frames_b.len() {
            s += self.frames_b[i] as u64;
            i += 1;
        }
        s
    }

    /// 是否越红线。
    pub fn over(&self) -> bool {
        self.sum_b() > self.redline_b as u64
    }

    /// 剩余余量（字节，u64；越线为 0）。
    pub fn headroom_b(&self) -> u64 {
        if self.over() {
            0
        } else {
            self.redline_b as u64 - self.sum_b()
        }
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "栈水位：Σ {} B / 红线 {} B，越线 {}，余量 {} B",
            self.sum_b(),
            self.redline_b,
            self.over(),
            self.headroom_b()
        )
    }
}

impl Default for StackLedger {
    fn default() -> Self {
        StackLedger::new(TH_IRQ_STACK_REDLINE_B)
    }
}

/// 固定容量直方图（1024 桶 + 1 溢出桶），整数分位。
/// 用于 p50/p99 判定，判据 BudgetExceeded。
#[derive(Clone, Debug)]
pub struct Histogram {
    /// 桶计数（TH_HIST_BUCKETS + 1 溢出桶）。
    pub buckets: Vec<u64>,
    /// 总样本数。
    pub n: u64,
    /// 溢出桶计数（≥ TH_HIST_MAX_NS 的样本）。
    pub overflow: u64,
    /// 求和（用于均值，u64）。
    pub sum_ns: u64,
    /// 最大值。
    pub max_ns: u64,
}

impl Histogram {
    /// 空直方图（固定容量，运行期建）。
    pub fn new() -> Self {
        Histogram {
            buckets: alloc::vec![0u64; TH_HIST_BUCKETS + 1],
            n: 0,
            overflow: 0,
            sum_ns: 0,
            max_ns: 0,
        }
    }

    /// 记录一个纳秒样本。
    pub fn record(&mut self, ns: u64) {
        self.n += 1;
        self.sum_ns = self.sum_ns.saturating_add(ns);
        if ns > self.max_ns {
            self.max_ns = ns;
        }
        if ns >= TH_HIST_MAX_NS {
            self.overflow += 1;
            return;
        }
        let idx = (ns / TH_BUCKET_WIDTH_NS) as usize;
        // 下标先比长度（铁律 7）。
        if idx < TH_HIST_BUCKETS {
            self.buckets[idx] += 1;
        } else {
            self.overflow += 1;
        }
    }

    /// 桶上界（纳秒）：第 idx 桶代表 (idx*W, (idx+1)*W]。
    fn bucket_upper_ns(idx: usize) -> u64 {
        (idx as u64 + 1) * TH_BUCKET_WIDTH_NS
    }

    /// 第 q 分位（q ∈ (0,100)，百分位整数）。返回上界纳秒估计。
    /// rank = ⌈q/100 · n⌉，从低桶向上累加至达到 rank 的桶上界。
    pub fn percentile(&self, q: u64) -> Option<u64> {
        if self.n == 0 {
            return None;
        }
        // rank = ceil(q*n/100)，至少 1。
        let rank = {
            let prod = q.saturating_mul(self.n);
            let d = prod / 100;
            if prod % 100 == 0 {
                d
            } else {
                d + 1
            }
        };
        let rank = if rank == 0 { 1 } else { rank };
        if rank > self.n {
            return None;
        }
        let mut cum: u64 = 0;
        let mut idx = 0;
        while idx < TH_HIST_BUCKETS {
            cum += self.buckets[idx];
            if cum >= rank {
                return Some(Histogram::bucket_upper_ns(idx));
            }
            idx += 1;
        }
        // 落入溢出桶：返回覆盖上限。
        Some(TH_HIST_MAX_NS)
    }

    /// 均值（纳秒，整数向下取整）。
    pub fn mean_ns(&self) -> u64 {
        if self.n == 0 {
            0
        } else {
            self.sum_ns / self.n
        }
    }

    /// 是否样本充足（≥100）。
    pub fn sufficient(&self) -> bool {
        self.n >= TH_MIN_SAMPLES
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let p50 = self.percentile(50).unwrap_or(0);
        let p99 = self.percentile(TH_P99_NUM).unwrap_or(0);
        format!(
            "直方图：n={} 均值={}ns p50≤{}ns p99≤{}ns max={}ns 溢出={}",
            self.n, self.mean_ns(), p50, p99, self.max_ns, self.overflow
        )
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Histogram::new()
    }
}


/// 单次 handler 执行记录（一次 top half 的完整审计条目）。
#[derive(Clone, Debug)]
pub struct TopRun {
    /// 中断向量。
    pub vector: u16,
    /// 本次纳秒成本。
    pub ns: u64,
    /// 是否应答。
    pub acked: bool,
    /// 是否摘快照（快照字节数 > 0 记为已摘）。
    pub snap_len: u32,
    /// 是否挂 pending。
    pub pended: bool,
    /// 违禁命中集合（本次命中的黑名单项）。
    pub forbidden: Vec<TopForbid>,
    /// 判档结果。
    pub grade: BudgetGrade,
}

impl TopRun {
    /// 由要素构造并判档。
    pub fn new(vector: u16, ns: u64, acked: bool, snap_len: u32, pended: bool) -> Self {
        TopRun {
            vector,
            ns,
            acked,
            snap_len,
            pended,
            forbidden: Vec::new(),
            grade: BudgetGrade::of_ns(ns, TH_BUDGET_NS_QEMU),
        }
    }

    /// 义务完备性：应答 ∧ 快照 ∧ pending 齐备。
    pub fn oblige_ok(&self) -> bool {
        self.acked && self.snap_len > 0 && self.pended
    }

    /// 是否违约（超预算）。
    pub fn over_budget(&self) -> bool {
        self.grade.violates()
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "vec={} ns={} 档={} 应答={} 快照={}B pending={} 违禁={}项",
            self.vector,
            self.ns,
            self.grade.name(),
            self.acked,
            self.snap_len,
            self.pended,
            self.forbidden.len()
        )
    }
}

/// 纪律文档（义务/禁令/预算三节，冻结指纹）。
/// 判据 DocIncomplete / DocTampered。
#[derive(Clone, Debug)]
pub struct DisciplineDoc {
    /// 三节文案（顺序 = 义务/禁令/预算）。
    pub sections: Vec<String>,
    /// 冻结指纹（FNV-1a of 拼接文案）。
    pub frozen_fp: u64,
    /// 是否已冻结。
    pub frozen: bool,
}

impl DisciplineDoc {
    /// 由三节文案建文档并冻结。
    pub fn freeze(oblige: &str, forbid: &str, budget: &str) -> Self {
        let sections = alloc::vec![oblige.to_string(), forbid.to_string(), budget.to_string()];
        let joined = sections.join("|");
        let fp = fnv1a(joined.as_bytes());
        DisciplineDoc { sections, frozen_fp: fp, frozen: true }
    }

    /// 三节是否齐备（每节非空）。
    pub fn complete(&self) -> bool {
        self.sections.len() == 3 && self.sections.iter().all(|s| !s.is_empty())
    }

    /// 当前指纹。
    pub fn current_fp(&self) -> u64 {
        let joined = self.sections.join("|");
        fnv1a(joined.as_bytes())
    }

    /// 是否被篡改（冻结后现值 ≠ 冻结值）。
    pub fn tampered(&self) -> bool {
        self.frozen && self.current_fp() != self.frozen_fp
    }

    /// 段位守卫（三节闭集文案非空，节名可查）。
    pub fn well_formed(&self) -> bool {
        self.complete() && DocSection::ALL.iter().all(|s| !s.name().is_empty())
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "纪律文档：{} 节，冻结 {}，现指纹 {:016x}，冻结指纹 {:016x}，篡改 {}",
            self.sections.len(),
            self.frozen,
            self.current_fp(),
            self.frozen_fp,
            self.tampered()
        )
    }
}


/// 超预算清单条目（判据 OverlistEmpty / OverlistGap）。
#[derive(Clone, Debug)]
pub struct OverEntry {
    /// 向量。
    pub vector: u16,
    /// 观测到的 p99 上界。
    pub p99_ns: u64,
    /// 超预算的纳秒幅度。
    pub exceed_ns: u64,
    /// 判档。
    pub grade: BudgetGrade,
    /// 裁剪建议。
    pub advice: String,
}

/// top half 纪律引擎：全 handler 预算断言 + 违禁拦截 + 义务核 + 文档冻结。
/// 判据核心：p99 ≤5µs 全 handler、超预算清单 10/10、纪律文档三节冻结。
#[derive(Clone, Debug)]
pub struct TopHalfEngine {
    /// 每向量一个直方图（按向量索引，容量 = 256 向量表）。
    pub hist: Vec<Histogram>,
    /// 最近若干次执行记录（滚动审计窗，容量 256）。
    pub recent: Vec<TopRun>,
    /// 违禁注入计数。
    pub injected: u64,
    /// 违禁拦截计数。
    pub intercepted: u64,
    /// 步数账。
    pub steps: StepLedger,
    /// 栈水位账。
    pub stack: StackLedger,
    /// 纪律文档。
    pub doc: Option<DisciplineDoc>,
    /// 超预算清单。
    pub overlist: Vec<OverEntry>,
    /// 超预算清单产出次数（判据 10/10）。
    pub overlist_runs: u64,
    /// 预算线（默认 QEMU 档 5µs）。
    pub budget_ns: u64,
}

impl TopHalfEngine {
    /// 向量表容量。
    pub const VEC_CAP: usize = 256;

    /// 以默认 QEMU 预算建引擎。
    pub fn new() -> Self {
        TopHalfEngine::with_budget(TH_BUDGET_NS_QEMU)
    }

    /// 以指定预算建引擎。
    pub fn with_budget(budget_ns: u64) -> Self {
        let mut hist = Vec::new();
        let mut i = 0;
        while i < TopHalfEngine::VEC_CAP {
            hist.push(Histogram::new());
            i += 1;
        }
        TopHalfEngine {
            hist,
            recent: Vec::new(),
            injected: 0,
            intercepted: 0,
            steps: StepLedger::new(),
            stack: StackLedger::new(TH_IRQ_STACK_REDLINE_B),
            doc: None,
            overlist: Vec::new(),
            overlist_runs: 0,
            budget_ns,
        }
    }

    /// 向量下标（越界钳到 255）。
    fn vec_idx(vector: u16) -> usize {
        let v = vector as usize;
        if v < TopHalfEngine::VEC_CAP {
            v
        } else {
            TopHalfEngine::VEC_CAP - 1
        }
    }

    /// 挂载纪律文档（三节）。
    pub fn attach_doc(&mut self, doc: DisciplineDoc) {
        self.doc = Some(doc);
    }

    /// 记录一次 top half 执行（入直方图 + 滚动窗 + 步数账 + 义务核）。
    /// 返回本次判档。
    pub fn run(&mut self, run: TopRun) -> BudgetGrade {
        let idx = TopHalfEngine::vec_idx(run.vector);
        if idx < self.hist.len() {
            self.hist[idx].record(run.ns);
        }
        // 步数账：应答/快照/pending 各按白名单操作入账。
        if run.acked {
            self.steps.charge(TopAllowed::AckController);
        }
        if run.snap_len > 0 {
            self.steps.charge(TopAllowed::TakeSnapshot);
        }
        if run.pended {
            self.steps.charge(TopAllowed::RaisePending);
        }
        self.steps.charge(TopAllowed::LightStat);
        // 滚动窗追加（超 256 丢弃最旧）。
        if self.recent.len() >= TopHalfEngine::VEC_CAP {
            self.recent.remove(0);
        }
        self.recent.push(run.clone());
        run.grade
    }

    /// 违禁注入拦截：把一次「想做违禁操作」的意图交给拦截器。
    /// 命中黑名单即拦截并计数（判据 ForbiddenHit / InterceptGap）。
    pub fn inject_forbidden(&mut self, forbid: TopForbid) -> Result<(), (ThCode, String)> {
        self.injected += 1;
        // 黑名单闭集命中即拦截（所有 TopForbid 成员都是禁做）。
        let hit = TopForbid::ALL.iter().any(|f| core::mem::discriminant(f) == core::mem::discriminant(&forbid));
        if hit {
            self.intercepted += 1;
            return Err((ThCode::ForbiddenHit, forbid.advice().to_string()));
        }
        // 未命中（不该发生，黑名单是全闭集）——显性拒。
        Err((ThCode::ForbiddenHit, forbid.advice().to_string()))
    }

    /// 拦截审计：注入数 ≡ 拦截数（判据 InterceptGap，10/10）。
    pub fn intercept_ok(&self) -> bool {
        self.injected > 0 && self.injected == self.intercepted
    }

    /// 义务核：滚动窗内所有执行义务完备（应答/快照/pending）。
    pub fn oblige_ok(&self) -> bool {
        !self.recent.is_empty() && self.recent.iter().all(|r| r.oblige_ok())
    }

    /// 步数守恒核。
    pub fn steps_ok(&self) -> bool {
        self.steps.balanced()
    }

    /// 单向量 p99（上界纳秒），样本不足返回 None。
    pub fn vector_p99(&self, vector: u16) -> Option<u64> {
        let idx = TopHalfEngine::vec_idx(vector);
        if idx >= self.hist.len() {
            return None;
        }
        let h = &self.hist[idx];
        if !h.sufficient() {
            return None;
        }
        h.percentile(TH_P99_NUM)
    }

    /// 全局预算断言：全 handler p99 ≤ 预算（判据 BudgetExceeded）。
    /// 只对有足够样本的向量判定。
    pub fn budget_ok(&self) -> bool {
        let mut i = 0;
        while i < self.hist.len() {
            let h = &self.hist[i];
            if h.sufficient() {
                let p99 = h.percentile(TH_P99_NUM).unwrap_or(0);
                if p99 > self.budget_ns {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 是否有样本不足的向量（SampleThin 提示）。
    pub fn has_thin_sample(&self) -> bool {
        self.hist.iter().any(|h| h.n > 0 && !h.sufficient())
    }

    /// 违约向量数（有足够样本且 p99 超预算）。
    fn violating_vectors(&self) -> u64 {
        let mut viol = 0u64;
        let mut i = 0;
        while i < self.hist.len() {
            let h = &self.hist[i];
            if h.sufficient() {
                let p99 = h.percentile(TH_P99_NUM).unwrap_or(0);
                if p99 > self.budget_ns {
                    viol += 1;
                }
            }
            i += 1;
        }
        viol
    }

    /// 产出超预算清单（判据 OverlistEmpty / OverlistGap，10/10）。
    /// 遍历有足够样本且 p99 超预算的向量，逐条入清单；产出一次计数 +1。
    pub fn build_overlist(&mut self) -> u64 {
        self.overlist.clear();
        let mut i = 0;
        while i < self.hist.len() {
            let h = &self.hist[i];
            if h.sufficient() {
                let p99 = h.percentile(TH_P99_NUM).unwrap_or(0);
                if p99 > self.budget_ns {
                    let grade = BudgetGrade::of_ns(p99, self.budget_ns);
                    let exceed = p99.saturating_sub(self.budget_ns);
                    let advice = match grade {
                        BudgetGrade::Over => "裁剪 top half：只保留应答/摘快照/置 pending",
                        BudgetGrade::Critical => "重写 handler：把重活全部下沉 bottom half",
                        _ => "复核预算阈值",
                    };
                    self.overlist.push(OverEntry {
                        vector: i as u16,
                        p99_ns: p99,
                        exceed_ns: exceed,
                        grade,
                        advice: advice.to_string(),
                    });
                }
            }
            i += 1;
        }
        self.overlist_runs += 1;
        self.overlist.len() as u64
    }

    /// 超预算清单一致性：有违约 ⇔ 清单非空，且条数 ≡ 违约向量数。
    pub fn overlist_consistent(&self) -> bool {
        let viol = self.violating_vectors();
        let list_len = self.overlist.len() as u64;
        (viol == 0 && list_len == 0) || (viol > 0 && list_len == viol)
    }

    /// 栈水位核：不越红线。
    pub fn stack_ok(&self) -> bool {
        !self.stack.over()
    }

    /// 文档核：齐备 ∧ 未篡改（判据 DocIncomplete / DocTampered）。
    pub fn doc_ok(&self) -> bool {
        match &self.doc {
            Some(d) => d.complete() && !d.tampered(),
            None => false,
        }
    }


    /// 全审计闭集核：所有判据一次性核（判据核心入口）。
    /// 返回 (通过?, 失败诊断码列表)。
    pub fn audit(&self) -> (bool, Vec<ThCode>) {
        let mut fails: Vec<ThCode> = Vec::new();
        if !segment_guard() {
            fails.push(ThCode::SegGuard);
        }
        if !self.budget_ok() {
            // 区分超支与严重超支（取最坏档）。
            let mut worst = BudgetGrade::Within;
            let mut i = 0;
            while i < self.hist.len() {
                let h = &self.hist[i];
                if h.sufficient() {
                    let p99 = h.percentile(TH_P99_NUM).unwrap_or(0);
                    let g = BudgetGrade::of_ns(p99, self.budget_ns);
                    if g.violates() && (g as u8) > (worst as u8) {
                        worst = g;
                    }
                }
                i += 1;
            }
            if matches!(worst, BudgetGrade::Critical) {
                fails.push(ThCode::BudgetCritical);
            } else {
                fails.push(ThCode::BudgetExceeded);
            }
        }
        if !self.intercept_ok() && self.injected > 0 {
            fails.push(ThCode::InterceptGap);
        }
        if !self.oblige_ok() && !self.recent.is_empty() {
            fails.push(ThCode::ObligeMissing);
        }
        if !self.steps_ok() {
            fails.push(ThCode::StepUnbalanced);
        }
        if !self.stack_ok() {
            fails.push(ThCode::StackOverflow);
        }
        if self.has_thin_sample() {
            fails.push(ThCode::SampleThin);
        }
        if !self.doc_ok() {
            if let Some(d) = &self.doc {
                if d.tampered() {
                    fails.push(ThCode::DocTampered);
                } else {
                    fails.push(ThCode::DocIncomplete);
                }
            } else {
                fails.push(ThCode::DocIncomplete);
            }
        }
        (fails.is_empty(), fails)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let (ok, fails) = self.audit();
        let mut fail_names = String::new();
        let mut i = 0;
        while i < fails.len() {
            fail_names.push_str(fails[i].text());
            if i + 1 < fails.len() {
                fail_names.push_str("; ");
            }
            i += 1;
        }
        format!(
            "top half 纪律：预算线 {}ns，注入 {} 拦截 {}，{}，栈 {}，文档 {}，超预算清单 {} 条/产出 {} 次，审计 {}",
            self.budget_ns,
            self.injected,
            self.intercepted,
            self.steps.digest(),
            self.stack.digest(),
            self.doc.as_ref().map(|d| d.digest()).unwrap_or_else(|| "未挂载".to_string()),
            self.overlist.len(),
            self.overlist_runs,
            if ok { "全绿".to_string() } else { format!("失败: {}", fail_names) }
        )
    }
}

impl Default for TopHalfEngine {
    fn default() -> Self {
        TopHalfEngine::new()
    }
}

/// 段位守卫：全部诊断码落在 0x4c50 段内。
pub fn segment_guard() -> bool {
    ThCode::ALL
        .iter()
        .all(|c| c.val() >= TH_SEG_BASE && c.val() < TH_SEG_BASE + 0x100)
}

