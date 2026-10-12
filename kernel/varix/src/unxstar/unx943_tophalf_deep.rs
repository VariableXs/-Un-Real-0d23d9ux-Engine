//! unx943_tophalf_deep —— UNX-F0943 top half 纪律深化版（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B08）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单）· 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 前驱契约：UNX-F0941（两级分界）、UNX-F0942（分发入口 <300ns）；与并行版 unx943_tophalf 并存，深化版为准。
//!
//! 深化要点（相对并行版的功能密度与深度扩充）：
//!   一、预算六档细化：Roomy/Borderline/Within/Watch/Over/Critical，含 Watch 预警带（80% 余量阈值）。
//!   二、多档预算剖面：QEMU/Prod/Strict 三档预算线 + 档位切换账（预算紧缩时逐档降级显性化）。
//!   三、最小化清单八类白名单 + 每类步数成本表 + 白名单命中计数（合规操作逐类统计）。
//!   四、禁令闭集八类 + 义务闭集五类 + 违禁注入变异器（八类各注入 → 10/10 拦截）。
//!   五、步数账双账本（top half 入账 / bottom half 出账）+ 守恒核 + 步数密度对拍（步/µs）。
//!   六、栈水位账：帧声明式登记 + Σ帧红线 4KiB + 余量账 + 最坏帧溯源。
//!   七、1024 桶直方图 p50/p95/p99 分位 + 溢出桶 + 样本充足门槛 + 分位单调性自检。
//!   八、超预算清单产出器：逐 handler 判档 → 违约入清单 → 10/10 次产出核。
//!   九、纪律文档三节冻结（义务/禁令/预算）+ FNV 指纹 + 篡改检测 + 段落回滚建议。
//!   十、契约白名单（允许向量集）+ 越界向量拦截 + 全审计闭集一次性核。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，确定性（逻辑 tick）。

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

// ---------------------------------------------------------------------------
// 一、模块常量（全部写死并注释来源，判据预算断言硬线）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const TD_VERSION: &str = "unx943_tophalf_deep-v1";

/// 诊断段基址（0x4c60 段 · 本深化版独占，与并行版 0x4c50 段不同段互不重叠）。
pub const TD_SEG_BASE: u16 = 0x4c60;

/// QEMU 档 top half 预算硬线（判据：p99 ≤ 5µs = 5000 纳秒）。
pub const TD_BUDGET_NS_QEMU: u64 = 5_000;

/// 生产档预算线（更严，≤2µs；真机无 TCG 模拟开销）。
pub const TD_BUDGET_NS_PROD: u64 = 2_000;

/// 严格档预算线（最严，≤1µs；关键中断路径专用）。
pub const TD_BUDGET_NS_STRICT: u64 = 1_000;

/// 预警余量阈值（80% 预算即进 Watch 预警带，早于硬违约）。
pub const TD_WATCH_RATIO_NUM: u64 = 80;

/// 预警余量阈值分母（百分比）。
pub const TD_WATCH_RATIO_DEN: u64 = 100;

/// 直方图桶数（1024 桶 + 1 溢出桶）。
pub const TD_HIST_BUCKETS: usize = 1024;

/// 桶宽（纳秒/桶）。
pub const TD_BUCKET_WIDTH_NS: u64 = 8;

/// 直方图覆盖上限（桶数 × 桶宽 = 8192ns，超此入溢出桶）。
pub const TD_HIST_MAX_NS: u64 = TD_BUCKET_WIDTH_NS * TD_HIST_BUCKETS as u64;

/// 统计置信最低样本量（<100 不足以判 p99，显性拒绝）。
pub const TD_MIN_SAMPLES: u64 = 100;

/// p95 分位目标。
pub const TD_P95_NUM: u64 = 95;

/// p99 分位目标（判据核心）。
pub const TD_P99_NUM: u64 = 99;

/// IRQ 栈红线（4KiB = 4096 字节，Σ帧必须 ≤ 红线）。
pub const TD_IRQ_STACK_REDLINE_B: u32 = 4_096;

/// 超预算清单产出目标次数（判据 10/10）。
pub const TD_OVERLIST_TARGET: u64 = 10;

/// 违例注入拦截目标（判据 10/10）。
pub const TD_INTERCEPT_TARGET: u64 = 10;

/// 单原子操作步数成本经验折算（纳秒/步；取指+译码+寄存器访问保守单步）。
pub const TD_STEP_NS: u64 = 12;

/// 最大白名单类数（安全上限，防止契约膨胀）。
pub const TD_MAX_ALLOWED_KINDS: usize = 8;

/// FNV-1a 64 位偏移基准。
pub const TD_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位质数。
pub const TD_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 契约白名单最大向量数（256 向量全空间）。
pub const TD_MAX_WHITELIST_VECTORS: usize = 256;

// ---------------------------------------------------------------------------
// 二、预算剖面（三档预算线 + 档位切换）
// ---------------------------------------------------------------------------

/// 预算档位剖面（三档闭集，判据预算断言的核心口径切换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdProfile {
    /// QEMU 档（≤5µs，判据默认；TCG 有模拟开销故最宽）。
    Qemu,
    /// 生产档（≤2µs，真机无模拟开销）。
    Prod,
    /// 严格档（≤1µs，关键中断路径专用）。
    Strict,
}

impl TdProfile {
    /// 全量闭集。
    pub const ALL: [TdProfile; 3] = [TdProfile::Qemu, TdProfile::Prod, TdProfile::Strict];

    /// 该档预算线（纳秒）。
    pub const fn budget_ns(self) -> u64 {
        match self {
            TdProfile::Qemu => TD_BUDGET_NS_QEMU,
            TdProfile::Prod => TD_BUDGET_NS_PROD,
            TdProfile::Strict => TD_BUDGET_NS_STRICT,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdProfile::Qemu => "QEMU档",
            TdProfile::Prod => "生产档",
            TdProfile::Strict => "严格档",
        }
    }

    /// 是否比另一档更严（预算线更小）。
    pub const fn stricter_than(self, other: TdProfile) -> bool {
        self.budget_ns() < other.budget_ns()
    }
}

// ---------------------------------------------------------------------------
// 三、预算分档（六档细化，含 Watch 预警带）
// ---------------------------------------------------------------------------

/// 预算分档（六档闭集，判据预算断言核心）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdGrade {
    /// 充裕：≤ 预算 20%。
    Roomy,
    /// 临界：预算 20%~60%。
    Borderline,
    /// 预算内：预算 60%~80%。
    Within,
    /// 预警：预算 80%~100%（未违约但进预警带）。
    Watch,
    /// 超支：预算 100%~150%（硬违约）。
    Over,
    /// 严重超支：> 预算 150%（硬违约）。
    Critical,
}

impl TdGrade {
    /// 全量闭集。
    pub const ALL: [TdGrade; 6] = [
        TdGrade::Roomy,
        TdGrade::Borderline,
        TdGrade::Within,
        TdGrade::Watch,
        TdGrade::Over,
        TdGrade::Critical,
    ];

    /// 由纳秒成本对预算判档（纯整数比较，无浮点；含 80% 预警带）。
    pub const fn of_ns(ns: u64, budget: u64) -> TdGrade {
        let fifth = budget / 5;
        let three_fifths = budget.saturating_mul(3) / 5;
        let watch_line = budget.saturating_mul(TD_WATCH_RATIO_NUM) / TD_WATCH_RATIO_DEN;
        let one_half_extra = budget + budget / 2;
        if ns <= fifth {
            TdGrade::Roomy
        } else if ns <= three_fifths {
            TdGrade::Borderline
        } else if ns <= watch_line {
            TdGrade::Within
        } else if ns <= budget {
            TdGrade::Watch
        } else if ns <= one_half_extra {
            TdGrade::Over
        } else {
            TdGrade::Critical
        }
    }

    /// 是否违约（超支或严重超支）。
    pub const fn violates(self) -> bool {
        matches!(self, TdGrade::Over | TdGrade::Critical)
    }

    /// 是否需预警（Watch 及以上）。
    pub const fn needs_watch(self) -> bool {
        matches!(self, TdGrade::Watch | TdGrade::Over | TdGrade::Critical)
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdGrade::Roomy => "充裕",
            TdGrade::Borderline => "临界",
            TdGrade::Within => "预算内",
            TdGrade::Watch => "预警",
            TdGrade::Over => "超支",
            TdGrade::Critical => "严重超支",
        }
    }

    /// 档位序数（越大越严重，用于取最坏档）。
    pub const fn severity(self) -> u8 {
        match self {
            TdGrade::Roomy => 0,
            TdGrade::Borderline => 1,
            TdGrade::Within => 2,
            TdGrade::Watch => 3,
            TdGrade::Over => 4,
            TdGrade::Critical => 5,
        }
    }
}


// ---------------------------------------------------------------------------
// 四、最小化清单·白名单操作（八类闭集 + 每类步数成本表）
// ---------------------------------------------------------------------------

/// top half 最小化清单·白名单操作（闭集：top half 只许做这八类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdAllowed {
    /// 应答中断控制器（EOI/掩码位清除）。
    AckController,
    /// 摘取硬件快照（寄存器一次性读入栈上结构）。
    TakeSnapshot,
    /// 置 pending 标志 / 挂 bottom half。
    RaisePending,
    /// 轻量计数统计（步数入账，无分配无锁）。
    LightStat,
    /// 读时间戳计数器（rdtsc，无副作用）。
    ReadTsc,
    /// 置中断屏蔽位（短临界区，持锁 ≤ 红线步数）。
    MaskIrq,
    /// 写就绪标志（memory-order 释放，唤醒 bottom half）。
    SetReadyFlag,
    /// 尾调用返回（中断返回前收尾，无重活）。
    TailReturn,
}

impl TdAllowed {
    /// 全量闭集。
    pub const ALL: [TdAllowed; 8] = [
        TdAllowed::AckController,
        TdAllowed::TakeSnapshot,
        TdAllowed::RaisePending,
        TdAllowed::LightStat,
        TdAllowed::ReadTsc,
        TdAllowed::MaskIrq,
        TdAllowed::SetReadyFlag,
        TdAllowed::TailReturn,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdAllowed::AckController => "应答控制器",
            TdAllowed::TakeSnapshot => "摘取快照",
            TdAllowed::RaisePending => "置pending",
            TdAllowed::LightStat => "轻量统计",
            TdAllowed::ReadTsc => "读时间戳",
            TdAllowed::MaskIrq => "置屏蔽位",
            TdAllowed::SetReadyFlag => "写就绪标志",
            TdAllowed::TailReturn => "尾调用返回",
        }
    }

    /// 该操作的固定步数成本（原子操作数；判据步数账入账依据）。
    pub const fn steps(self) -> u32 {
        match self {
            TdAllowed::AckController => 2,
            TdAllowed::TakeSnapshot => 4,
            TdAllowed::RaisePending => 3,
            TdAllowed::LightStat => 1,
            TdAllowed::ReadTsc => 1,
            TdAllowed::MaskIrq => 2,
            TdAllowed::SetReadyFlag => 2,
            TdAllowed::TailReturn => 1,
        }
    }

    /// 该操作折算纳秒成本（步数 × TD_STEP_NS）。
    pub const fn ns_cost(self) -> u64 {
        self.steps() as u64 * TD_STEP_NS
    }

    /// 是否允许在严格档执行（TailReturn/AckController 等最轻量者可在严格档）。
    pub const fn strict_ok(self) -> bool {
        matches!(
            self,
            TdAllowed::AckController | TdAllowed::RaisePending | TdAllowed::TailReturn
        )
    }
}

// ---------------------------------------------------------------------------
// 五、禁令闭集（八类禁做 + 可操作建议）
// ---------------------------------------------------------------------------

/// top half 禁令闭集（八类禁做，命中即违约拦截）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdForbid {
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

impl TdForbid {
    /// 全量闭集（八类）。
    pub const ALL: [TdForbid; 8] = [
        TdForbid::ForbidAlloc,
        TdForbid::ForbidSleep,
        TdForbid::ForbidBlockingLock,
        TdForbid::ForbidHeavyMath,
        TdForbid::ForbidBigScan,
        TdForbid::ForbidIo,
        TdForbid::ForbidLogging,
        TdForbid::ForbidUnbounded,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdForbid::ForbidAlloc => "禁动态分配",
            TdForbid::ForbidSleep => "禁睡眠让出",
            TdForbid::ForbidBlockingLock => "禁阻塞锁",
            TdForbid::ForbidHeavyMath => "禁复杂运算",
            TdForbid::ForbidBigScan => "禁遍历大表",
            TdForbid::ForbidIo => "禁磁盘IO",
            TdForbid::ForbidLogging => "禁详细日志",
            TdForbid::ForbidUnbounded => "禁无界循环",
        }
    }

    /// 违例后的可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TdForbid::ForbidAlloc => "改用栈上固定缓冲或预分配池，把分配下沉到 bottom half",
            TdForbid::ForbidSleep => "取消让出，用 pending 挂起重活交 bottom half",
            TdForbid::ForbidBlockingLock => "改用自旋锁且持锁步数 ≤ 红线，或下沉取锁",
            TdForbid::ForbidHeavyMath => "预计算或查表，复杂运算移出中断上下文",
            TdForbid::ForbidBigScan => "限制遍历规模或改用增量结构，避免 O(n) 全扫",
            TdForbid::ForbidIo => "IO 一律交 bottom half/workqueue，中断内不触盘",
            TdForbid::ForbidLogging => "中断内只做轻计数，格式化日志下沉到消费侧",
            TdForbid::ForbidUnbounded => "加显式上界与步数红线，杜绝中断内死循环",
        }
    }
}

// ---------------------------------------------------------------------------
// 六、义务闭集（五类必做，缺一即违约）
// ---------------------------------------------------------------------------

/// top half 义务闭集（五类必做，缺一即违约）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdOblige {
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

impl TdOblige {
    /// 全量闭集（五类）。
    pub const ALL: [TdOblige; 5] = [
        TdOblige::MustAck,
        TdOblige::MustSnap,
        TdOblige::MustPending,
        TdOblige::MustStepLedger,
        TdOblige::MustConserve,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TdOblige::MustAck => "必应答",
            TdOblige::MustSnap => "必摘快照",
            TdOblige::MustPending => "必挂pending",
            TdOblige::MustStepLedger => "必记步数",
            TdOblige::MustConserve => "必守恒",
        }
    }
}

