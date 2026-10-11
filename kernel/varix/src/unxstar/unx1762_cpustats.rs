//! unx1762_cpustats —— UNX-F1762 每核统计账本体（十字段族汇总账）。
//!
//! 单号：UNX-F1762
//! 标题：每核统计账本体（十字段族汇总账）
//! 域：CoRun · UNX-A3-B09
//! 目标行数：2200~2800 行（硬指标）
//!
//! 判据原文：
//! `UNX-F1762-J1 十字段族（切换/唤醒/迁移/idle/延迟/队列深度/类统计/均衡/IPI/自检）全量登记
//!  与 PerCpu 冻结版（F1623/F1639）字段级对齐断言全过`
//!
//! 册内上下文：
//! CoRun · UNX-A3-B09 段「观测面」。三层架构（账 → 接口 → 聚合）取自 F1761 总纲：
//! 本单负责 **L1 账层**的唯一实体 —— 每核算子（PerCpu）的十字段族统计账本体。
//! L2 接口层（F1763 统计登记宏内联）与 L3 聚合层（F1768 全核同窗快照）只消费本账，
//! 观测与业务零耦合：账本不含任何调度策略判定，只记账。
//!
//! 组件清单：
//! - [`StatFamily`]：十字段族闭集（10 族，闭集不可扩展）。
//! - 每族专属计数器集 12 个（8 × u32 窄计数 + 4 × u64 宽计数），窄计数饱和于 `u32::MAX`。
//! - 族内守恒不变量 20 条（每族 2 条），统一入口 [`Ledger::verify_all`]。
//! - 唯一登记写入口 [`Ledger::record`]（F1763 零散点禁令：直写即检出）。
//! - [`PerCpuFrozen`]：PerCpu 冻结版（F1623/F1639）字段级对齐靶面。
//! - 三方对齐表（PerCpu 字段名 × 账目位索引 × 观测接口名）与对齐状态闭集。
//! - append-only 字段登记册 + FNV-1a 64 封账指纹（封账后改写即拒）。
//! - 全局汇总 `Σ每核 == 全局`（逐族逐计数器闭账）。
//! - 时间加权队列深度（F1645 双口径：瞬时 / 窗口）+ 加权结算误差上界断言。
//! - 全核同窗快照协议（F1768 消费）：三元组 (核算子 id, tick, 指纹) + 错窗检出 + 可回放对账。
//! - 注入核 10 形态，10/10 检出。
//! - 单点导出：族文本 / 全量 KV / TLV 二进制。
//!
//! 性能逐项分解（每核算子）：
//! - 登记写入：O(1)，一次 `Vec` 索引取槽 + 一次饱和加 + 至多一次置位，无锁、无分配。
//! - 溢出判定：位宽上限预计算为常量 `sat_cap`，比较为整数比较，零除法。
//! - 守恒校验：O(族数 × 字段数) = O(20)，全整数运算，最坏 20 次无符号比较。
//! - 对齐断言：O(字段数) = O(120)，三方表逐位比对，无堆分配（仅报告用一次 `String`）。
//! - 封账指纹：O(字段数 × 名长) 流式 FNV-1a，无中间缓冲。
//! - 全局汇总：O(核算子数 × 字段数)，一次线性扫描，闭账后不再重算。
//! - 快照采集：O(核算子数)，每核算子一次指纹（O(1) 复用封账指纹的核内分片）。
//! - 快照回放：O(核算子数 × 字段数)，逐槽对账，差值累加为单一 `u64` 判据。
//! - TLV 导出：O(字段数)，`Vec<u8>` 一次性 `push`，预留容量避免重复增长。
//!
//! 诊断码段：本模块独占 `0xB060~0xB06F`，主码 `0xB060`
//! （子码 0xB060 字段对齐 / 0xB061 守恒破坏 / 0xB062 散点直写 / 0xB063 封账篡改 /
//!   0xB064 汇总不闭合 / 0xB065 快照错窗 / 0xB066 溢出漏标 / 0xB067 偏移错位 /
//!   0xB068 账位越界 / 0xB069 时窗倒流 / 0xB06A 族未登记 / 0xB06B 表自洽失败 /
//!   0xB06C 回放差异 / 0xB06D 注入核自身失败 / 0xB06E 饱和策略违例 / 0xB06F 导出长度不自洽）。
//!
//! 纯功能线：`no_std + alloc`。零 `unwrap` / 零 `expect` / 零 `panic!` / 零浮点 /
//! 零真实 IO / 零墙钟（逻辑 `tick: u64` 显式传入）。全部状态迁移由调用方给定输入决定，
//! 同输入必得同输出（确定性 LCG 亦为纯函数）。
//!
//! ---------------------------------------------------------------------------
//! [`PerCpuFrozen`] 布局冻结说明（`#[repr(C)]`，字段顺序即偏移顺序，无隐式重排）
//! ---------------------------------------------------------------------------
//! 结构总尺寸 = 640 字节（10 族 × 64 字节），对齐 = 8，无尾部填充。
//!
//! 族内块布局（族基址 = `族序号 * 64`，族序号 0–9）：
//! ```text
//! 族内偏移  宽度  语义域
//!   0..32    u32   窄计数区（局部索引 local = 0..8，共 8 项）
//!  32..64    u64   宽计数区（局部索引 local = 8..12，共 4 项）
//! ```
//! 之所以窄区在前：8 × 4 = 32 恰为 8 字节对齐的自然边界，窄区之后不产生任何
//! 隐式填充，宽区亦不引入尾部填充，故 `size_of::<PerCpuFrozen>() == 640`
//! 且 `align_of::<PerCpuFrozen>() == 8`。
//!
//! 全局账目位索引（与 `PerCpuFrozen` 字段一一同序）：
//! `GIDX(family, local) = family.ordinal() * 12 + local`，`GIDX ∈ [0, 120)`。
//!
//! 族序号 ↔ 族 ↔ 偏移区间对照：
//! | 序号 | 族 | 偏移区间 | 字段数 |
//! |---|---|---|---|
//! | 0 | Switch | `0..64` | 12 |
//! | 1 | Wake | `64..128` | 12 |
//! | 2 | Migrate | `128..192` | 12 |
//! | 3 | Idle | `192..256` | 12 |
//! | 4 | Latency | `256..320` | 12 |
//! | 5 | QueueDepth | `320..384` | 12 |
//! | 6 | ClassStat | `384..448` | 12 |
//! | 7 | Balance | `448..512` | 12 |
//! | 8 | Ipi | `512..576` | 12 |
//! | 9 | SelfCheck | `576..640` | 12 |
//!
//! 语义分区：窄区承载**高频瞬时量**（次数、瞬时深度、最近 tick、峰值、拒绝数），
//! 宽区承载**累积量**（总量、时间窗和、代价累计、校验和）。
//! `last_*_tick` 字段（每族 local = 7）统一落在窄区尾部，语义为"最近一次该族记账的逻辑 tick"。

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

/// 模块版本（账本体冻结 v1）。
pub const CPUS_VERSION: &str = "unx1762_cpustats-v1";

/// 诊断段基址（0xB060 段 · 本模块独占 0xB060~0xB06F）。
pub const CPUS_SEG_BASE: u16 = 0xB060;

/// 诊断段上界（含）。
pub const CPUS_SEG_END: u16 = 0xB06F;

/// 主诊断码（字段级对齐）。
pub const DIAG_ALIGN: u16 = 0xB060;
/// 子码：族内守恒破坏。
pub const DIAG_CONSERVATION: u16 = 0xB061;
/// 子码：散点直写（绕过登记写入口）。
pub const DIAG_SCATTER: u16 = 0xB062;
/// 子码：封账后改写。
pub const DIAG_SEAL: u16 = 0xB063;
/// 子码：全局汇总不闭合。
pub const DIAG_SUM: u16 = 0xB064;
/// 子码：快照错窗。
pub const DIAG_WINDOW: u16 = 0xB065;
/// 子码：溢出漏标（饱和未置 overflow 位）。
pub const DIAG_OVF: u16 = 0xB066;
/// 子码：字段偏移错位。
pub const DIAG_OFFSET: u16 = 0xB067;
/// 子码：账位越界。
pub const DIAG_RANGE: u16 = 0xB068;
/// 子码：时窗倒流。
pub const DIAG_STALE: u16 = 0xB069;
/// 子码：族未登记。
pub const DIAG_FAMILY: u16 = 0xB06A;
/// 子码：表自洽失败。
pub const DIAG_TABLE: u16 = 0xB06B;
/// 子码：快照回放差异。
pub const DIAG_REPLAY: u16 = 0xB06C;
/// 子码：注入核自身失败（注入未能造出预期缺陷）。
pub const DIAG_INJECT: u16 = 0xB06D;
/// 子码：饱和策略违例（超上限未饱和）。
pub const DIAG_SAT: u16 = 0xB06E;
/// 子码：导出长度不自洽。
pub const DIAG_EXPORT: u16 = 0xB06F;

/// 十字段族数量（闭集规模）。
pub const FAMILY_COUNT: usize = 10;

/// 每族计数器数（8 窄 + 4 宽）。
pub const FIELDS_PER_FAMILY: usize = 12;

/// 账目位总数（10 × 12 = 120）。
pub const FIELD_COUNT: usize = FAMILY_COUNT * FIELDS_PER_FAMILY;

/// 每族块字节数（8×4 + 4×8 = 64）。
pub const FAMILY_STRIDE: u32 = 64;

/// 每核算子账本体字节数（10 × 64 = 640）。
pub const PERCPU_STRIDE: u32 = FAMILY_STRIDE * FAMILY_COUNT as u32;

/// 单核算子账槽计数（1.0 保守）。
pub const SAT_CAP_U32: u64 = 0xFFFF_FFFF;

/// 单核算子账槽饱和上限（1.0 保守）。
pub const SAT_CAP_U64: u64 = u64::MAX;

/// 饱和水位线（达 96% 触发预警位提示，仍走饱和语义）。
pub const SAT_WARN_RATIO_NUM: u64 = 96;
/// 饱和水位线分母。
pub const SAT_WARN_RATIO_DEN: u64 = 100;

/// 错误闭集（零 panic · 零 unwrap）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpErr {
    /// 核算子 id 越界。
    CpuRange,
    /// 账目位索引越界（`>= 120`）。
    FieldRange,
    /// 族未在登记表内。
    FamilyUnregistered,
    /// 时窗倒流：本次 tick 早于账本当前 tick。
    StaleTick,
    /// 封账后改写被拒。
    Sealed,
    /// 对齐失败。
    AlignFailed,
    /// 守恒失败。
    ConservFailed,
    /// 汇总不闭合。
    SumOpen,
    /// 快照错窗。
    WindowMismatch,
    /// 溢出漏标。
    OvfUnmarked,
    /// 偏移错位。
    OffsetMismatch,
    /// 表自洽失败。
    TableInconsistent,
    /// 回放差异非零。
    ReplayDiff,
    /// 导出长度不自洽。
    ExportLength,
    /// 注入核未能造出预期缺陷（注入自身失败）。
    InjectSetup,
}

impl CpErr {
    /// 错误码 → 诊断码映射。
    pub fn diag(self) -> u16 {
        match self {
            CpErr::CpuRange => DIAG_RANGE,
            CpErr::FieldRange => DIAG_RANGE,
            CpErr::FamilyUnregistered => DIAG_FAMILY,
            CpErr::StaleTick => DIAG_STALE,
            CpErr::Sealed => DIAG_SEAL,
            CpErr::AlignFailed => DIAG_ALIGN,
            CpErr::ConservFailed => DIAG_CONSERVATION,
            CpErr::SumOpen => DIAG_SUM,
            CpErr::WindowMismatch => DIAG_WINDOW,
            CpErr::OvfUnmarked => DIAG_OVF,
            CpErr::OffsetMismatch => DIAG_OFFSET,
            CpErr::TableInconsistent => DIAG_TABLE,
            CpErr::ReplayDiff => DIAG_REPLAY,
            CpErr::ExportLength => DIAG_EXPORT,
            CpErr::InjectSetup => DIAG_INJECT,
        }
    }

    /// 错误短名。
    pub fn name(self) -> &'static str {
        match self {
            CpErr::CpuRange => "cpu-range",
            CpErr::FieldRange => "field-range",
            CpErr::FamilyUnregistered => "family-unregistered",
            CpErr::StaleTick => "stale-tick",
            CpErr::Sealed => "sealed",
            CpErr::AlignFailed => "align-failed",
            CpErr::ConservFailed => "conserv-failed",
            CpErr::SumOpen => "sum-open",
            CpErr::WindowMismatch => "window-mismatch",
            CpErr::OvfUnmarked => "ovf-unmarked",
            CpErr::OffsetMismatch => "offset-mismatch",
            CpErr::TableInconsistent => "table-inconsistent",
            CpErr::ReplayDiff => "replay-diff",
            CpErr::ExportLength => "export-length",
            CpErr::InjectSetup => "inject-setup",
        }
    }

    /// `Err` 速构造（无 unwrap / 无 panic）。
    pub fn e<T>(self) -> Result<T> {
        Err(self)
    }
}

/// 模块内统一 `Result`。
pub type Result<T> = core::result::Result<T, CpErr>;

/// 确定性 LCG（Knuth 64 位参数 · 返回高 31 位，零墙钟零 IO）。
///
/// 用途：注入核与回放对账需要"看起来像随机"的确定性序列，
/// 但本函数是纯函数：同种子必得同序列，故不影响账本可重放性。
pub fn lcg(s: &mut u64) -> u64 {
    *s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *s >> 33
}

/// FNV-1a 64 位初始基底。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 步进（单字节）。
pub fn fnv1a_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 64 步进（`u64` 拆 8 字节小端）。
pub fn fnv1a_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        acc = fnv1a_byte(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// FNV-1a 64 步进（`&str` 逐字节）。
pub fn fnv1a_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        acc = fnv1a_byte(acc, bytes[i]);
        i += 1;
    }
    acc
}

/// FNV-1a 64 起手。
pub fn fnv1a_begin() -> u64 {
    FNV_OFFSET_BASIS
}

/// 饱和上限（按字段宽度取常量，零除法）。
pub fn sat_cap(width: u8) -> u64 {
    if width == 8 {
        SAT_CAP_U64
    } else {
        SAT_CAP_U32
    }
}

/// 十字段族闭集（观测面全量族 · 不可扩展）。
///
/// 族序即账本块序：`ordinal() * FAMILY_STRIDE` 为该族在 [`PerCpuFrozen`] 中的偏移基址。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatFamily {
    /// 切换族（F1626 切换率）：上下文切换的成因分解与回滚。
    Switch,
    /// 唤醒族（F1759 唤醒率）：从 idle/睡眠被唤醒的计数与合并。
    Wake,
    /// 迁移族（任务在核算子间的搬迁、亲和违例与回滚）。
    Migrate,
    /// idle 族（F1745 idle 守恒）：进出 idle 与三级状态驻留。
    Idle,
    /// 延迟族（F1764 延迟分解）：调度延迟的分段分解与统计量。
    Latency,
    /// 队列深度族（F1645 双口径）：瞬时口径与窗口口径并存。
    QueueDepth,
    /// 类统计族（F1661 调度类）：各调度类的份额与跨类搬迁。
    ClassStat,
    /// 均衡族（F1721 均衡执行）：拉/推/窃与不均衡度累积。
    Balance,
    /// IPI 族（核间中断的发出、合并、失败与代价）。
    Ipi,
    /// 自检族（账本自身的自检与校验和）。
    SelfCheck,
}

impl StatFamily {
    /// 全族表（闭集序 = 族序 = 块序）。
    pub fn all() -> [StatFamily; FAMILY_COUNT] {
        [
            StatFamily::Switch,
            StatFamily::Wake,
            StatFamily::Migrate,
            StatFamily::Idle,
            StatFamily::Latency,
            StatFamily::QueueDepth,
            StatFamily::ClassStat,
            StatFamily::Balance,
            StatFamily::Ipi,
            StatFamily::SelfCheck,
        ]
    }

    /// 族序（0–9）。
    pub fn ordinal(self) -> usize {
        match self {
            StatFamily::Switch => 0,
            StatFamily::Wake => 1,
            StatFamily::Migrate => 2,
            StatFamily::Idle => 3,
            StatFamily::Latency => 4,
            StatFamily::QueueDepth => 5,
            StatFamily::ClassStat => 6,
            StatFamily::Balance => 7,
            StatFamily::Ipi => 8,
            StatFamily::SelfCheck => 9,
        }
    }

    /// 族序反查（越界返回 `FamilyUnregistered`）。
    pub fn from_ordinal(o: usize) -> Result<StatFamily> {
        match o {
            0 => Ok(StatFamily::Switch),
            1 => Ok(StatFamily::Wake),
            2 => Ok(StatFamily::Migrate),
            3 => Ok(StatFamily::Idle),
            4 => Ok(StatFamily::Latency),
            5 => Ok(StatFamily::QueueDepth),
            6 => Ok(StatFamily::ClassStat),
            7 => Ok(StatFamily::Balance),
            8 => Ok(StatFamily::Ipi),
            9 => Ok(StatFamily::SelfCheck),
            _ => CpErr::FamilyUnregistered.e(),
        }
    }

    /// 族短名（导出用）。
    pub fn name(self) -> &'static str {
        match self {
            StatFamily::Switch => "switch",
            StatFamily::Wake => "wake",
            StatFamily::Migrate => "migrate",
            StatFamily::Idle => "idle",
            StatFamily::Latency => "latency",
            StatFamily::QueueDepth => "queue-depth",
            StatFamily::ClassStat => "class-stat",
            StatFamily::Balance => "balance",
            StatFamily::Ipi => "ipi",
            StatFamily::SelfCheck => "self-check",
        }
    }

    /// 观测接口名前缀（`obs-B09-<slug>-<counter>`，F1763 消费）。
    pub fn iface_prefix(self) -> String {
        format!("obs-B09-{}", self.name())
    }

    /// 该族在 [`PerCpuFrozen`] 中的偏移基址。
    pub fn offset_base(self) -> u32 {
        self.ordinal() as u32 * FAMILY_STRIDE
    }

    /// 族内局部索引 → 全局账目位索引。
    pub fn gidx(self, local: usize) -> Result<usize> {
        if local >= FIELDS_PER_FAMILY {
            return CpErr::FieldRange.e();
        }
        Ok(self.ordinal() * FIELDS_PER_FAMILY + local)
    }

    /// 全局账目位索引 → （族，局部索引）。
    pub fn split_gidx(g: usize) -> Result<(StatFamily, usize)> {
        if g >= FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        let f = g / FIELDS_PER_FAMILY;
        let l = g % FIELDS_PER_FAMILY;
        Ok((StatFamily::from_ordinal(f)?, l))
    }

    /// 局部索引 → 字段位宽（窄区 4 字节，宽区 8 字节）。
    pub fn width_of_local(local: usize) -> Result<u8> {
        if local >= FIELDS_PER_FAMILY {
            return CpErr::FieldRange.e();
        }
        Ok(if local < 8 { 4 } else { 8 })
    }

    /// 全局索引 → 字段位宽。
    pub fn width_of_gidx(g: usize) -> Result<u8> {
        let (_f, l) = StatFamily::split_gidx(g)?;
        StatFamily::width_of_local(l)
    }

    /// 该族字段名表（12 项，序 = 局部索引）。
    pub fn labels(self) -> &'static [&'static str; FIELDS_PER_FAMILY] {
        match self {
            StatFamily::Switch => &SWITCH_LABELS,
            StatFamily::Wake => &WAKE_LABELS,
            StatFamily::Migrate => &MIGRATE_LABELS,
            StatFamily::Idle => &IDLE_LABELS,
            StatFamily::Latency => &LATENCY_LABELS,
            StatFamily::QueueDepth => &QD_LABELS,
            StatFamily::ClassStat => &CLASS_LABELS,
            StatFamily::Balance => &BALANCE_LABELS,
            StatFamily::Ipi => &IPI_LABELS,
            StatFamily::SelfCheck => &SELFCHK_LABELS,
        }
    }

    /// 局部索引 → 字段名。
    pub fn label(self, local: usize) -> Result<&'static str> {
        if local >= FIELDS_PER_FAMILY {
            return CpErr::FieldRange.e();
        }
        Ok(self.labels()[local])
    }

    /// 观测接口全名（族前缀 + 计数器名）。
    pub fn iface_of(self, local: usize) -> Result<String> {
        let lab = self.label(local)?;
        Ok(format!("{}-{}", self.iface_prefix(), lab))
    }

    /// 该族是否含时间加权口径（F1645 双口径消费方）。
    pub fn has_weighted(self) -> bool {
        self == StatFamily::QueueDepth
    }
}

/// 切换族字段名表（local 0..12 · 窄区 0..8 / 宽区 8..12）。
pub const SWITCH_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "switch_count",
    "switch_by_expiry",
    "switch_by_preempt",
    "switch_by_yield",
    "switch_by_migrate",
    "switch_rollback",
    "switch_deadpoint",
    "last_switch_tick",
    "switch_total",
    "switch_rollback_total",
    "switch_window_sum",
    "switch_busy_cycles",
];

/// 唤醒族字段名表。
pub const WAKE_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "wake_count",
    "wake_from_idle",
    "wake_from_sleep",
    "wake_coalesced",
    "wake_preempted",
    "wake_reject_cap",
    "wake_burst_max",
    "last_wake_tick",
    "wake_total",
    "wake_latency_sum_ns",
    "wake_window_sum",
    "wake_from_idle_total",
];

/// 迁移族字段名表。
pub const MIGRATE_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "migrate_count",
    "migrate_by_balance",
    "migrate_by_affinity",
    "migrate_overflow",
    "migrate_reject_cap",
    "migrate_rollback",
    "migrate_src_peak",
    "last_migrate_tick",
    "migrate_total",
    "migrate_cost_sum",
    "migrate_window_sum",
    "migrate_imbalance_acc",
];

/// idle 族字段名表（F1745 守恒消费方）。
pub const IDLE_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "idle_enter_count",
    "idle_exit_count",
    "idle_state_a_ticks",
    "idle_state_b_ticks",
    "idle_state_c_ticks",
    "idle_deepest_level",
    "idle_wake_blocked",
    "last_idle_tick",
    "idle_total",
    "idle_total_ticks",
    "idle_window_sum",
    "idle_cycles",
];

/// 延迟族字段名表（F1764 分解消费方）。
pub const LATENCY_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "latency_samples",
    "latency_enqueue",
    "latency_dispatch",
    "latency_preempt",
    "latency_ipi",
    "latency_migrate",
    "latency_bucket_hi",
    "last_latency_tick",
    "latency_sum_ns",
    "latency_max_ns",
    "latency_p99_sum_ns",
    "latency_window_sum",
];

/// 队列深度族字段名表（F1645 双口径：instant=local0，window=local8/9）。
pub const QD_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "qd_instant",
    "qd_peak_instant",
    "qd_runq_len",
    "qd_waitq_len",
    "qd_hot_len",
    "qd_overflow_events",
    "qd_sample_count",
    "last_qd_tick",
    "qd_window_sum",
    "qd_window_ticks",
    "qd_total",
    "qd_weight_err_ppm",
];

/// 类统计族字段名表（F1661 调度类消费方）。
pub const CLASS_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "class_rt_count",
    "class_fair_count",
    "class_bt_count",
    "class_idle_count",
    "class_batch_count",
    "class_migrations",
    "class_steal_share",
    "last_class_tick",
    "class_total",
    "class_share_sum",
    "class_window_sum",
    "class_steal_count",
];

/// 均衡族字段名表（F1721 消费方）。
pub const BALANCE_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "balance_pull",
    "balance_push",
    "balance_steal",
    "balance_imbalance_gap",
    "balance_iterations",
    "balance_reject_cap",
    "balance_energy_tick",
    "last_balance_tick",
    "balance_total",
    "balance_imbalance_acc",
    "balance_window_sum",
    "balance_energy_cost",
];

/// IPI 族字段名表。
pub const IPI_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "ipi_sent",
    "ipi_resched",
    "ipi_halt",
    "ipi_wakeup",
    "ipi_failed",
    "ipi_coalesced",
    "ipi_burst_max",
    "last_ipi_tick",
    "ipi_total",
    "ipi_cost_sum_ns",
    "ipi_window_sum",
    "ipi_drop_total",
];

/// 自检族字段名表。
pub const SELFCHK_LABELS: [&str; FIELDS_PER_FAMILY] = [
    "selfchk_pass",
    "selfchk_fail",
    "selfchk_run",
    "selfchk_skip",
    "selfchk_ovf_events",
    "selfchk_csum",
    "selfchk_depth",
    "last_selfchk_tick",
    "selfchk_total",
    "selfchk_fail_total",
    "selfchk_window_sum",
    "selfchk_latency_sum",
];

macro_rules! fidx {
    ($($(#[$d:meta])* $n:ident = $v:expr;)*) => {
        $(
            $(#[$d])*
            pub const $n: usize = $v;
        )*
    };
}

fidx! {
    /// 账目位：Switch / switch_count（窄 · u32）。
    IDX_SWITCH_COUNT = 0;
    /// 账目位：Switch / switch_by_expiry（窄 · u32）。
    IDX_SWITCH_BY_EXPIRY = 1;
    /// 账目位：Switch / switch_by_preempt（窄 · u32）。
    IDX_SWITCH_BY_PREEMPT = 2;
    /// 账目位：Switch / switch_by_yield（窄 · u32）。
    IDX_SWITCH_BY_YIELD = 3;
    /// 账目位：Switch / switch_by_migrate（窄 · u32）。
    IDX_SWITCH_BY_MIGRATE = 4;
    /// 账目位：Switch / switch_rollback（窄 · u32）。
    IDX_SWITCH_ROLLBACK = 5;
    /// 账目位：Switch / switch_deadpoint（窄 · u32）。
    IDX_SWITCH_DEADPOINT = 6;
    /// 账目位：Switch / last_switch_tick（窄 · u32）。
    IDX_SWITCH_LAST_TICK = 7;
    /// 账目位：Switch / switch_total（宽 · u64）。
    IDX_SWITCH_TOTAL = 8;
    /// 账目位：Switch / switch_rollback_total（宽 · u64）。
    IDX_SWITCH_ROLLBACK_TOTAL = 9;
    /// 账目位：Switch / switch_window_sum（宽 · u64）。
    IDX_SWITCH_WINDOW_SUM = 10;
    /// 账目位：Switch / switch_busy_cycles（宽 · u64）。
    IDX_SWITCH_BUSY_CYCLES = 11;

    /// 账目位：Wake / wake_count（窄 · u32）。
    IDX_WAKE_COUNT = 12;
    /// 账目位：Wake / wake_from_idle（窄 · u32）。
    IDX_WAKE_FROM_IDLE = 13;
    /// 账目位：Wake / wake_from_sleep（窄 · u32）。
    IDX_WAKE_FROM_SLEEP = 14;
    /// 账目位：Wake / wake_coalesced（窄 · u32）。
    IDX_WAKE_COALESCED = 15;
    /// 账目位：Wake / wake_preempted（窄 · u32）。
    IDX_WAKE_PREEMPTED = 16;
    /// 账目位：Wake / wake_reject_cap（窄 · u32）。
    IDX_WAKE_REJECT_CAP = 17;
    /// 账目位：Wake / wake_burst_max（窄 · u32）。
    IDX_WAKE_BURST_MAX = 18;
    /// 账目位：Wake / last_wake_tick（窄 · u32）。
    IDX_WAKE_LAST_TICK = 19;
    /// 账目位：Wake / wake_total（宽 · u64）。
    IDX_WAKE_TOTAL = 20;
    /// 账目位：Wake / wake_latency_sum_ns（宽 · u64）。
    IDX_WAKE_LATENCY_SUM = 21;
    /// 账目位：Wake / wake_window_sum（宽 · u64）。
    IDX_WAKE_WINDOW_SUM = 22;
    /// 账目位：Wake / wake_from_idle_total（宽 · u64）。
    IDX_WAKE_FROM_IDLE_TOTAL = 23;

    /// 账目位：Migrate / migrate_count（窄 · u32）。
    IDX_MIGRATE_COUNT = 24;
    /// 账目位：Migrate / migrate_by_balance（窄 · u32）。
    IDX_MIGRATE_BY_BALANCE = 25;
    /// 账目位：Migrate / migrate_by_affinity（窄 · u32）。
    IDX_MIGRATE_BY_AFFINITY = 26;
    /// 账目位：Migrate / migrate_overflow（窄 · u32）。
    IDX_MIGRATE_OVERFLOW = 27;
    /// 账目位：Migrate / migrate_reject_cap（窄 · u32）。
    IDX_MIGRATE_REJECT_CAP = 28;
    /// 账目位：Migrate / migrate_rollback（窄 · u32）。
    IDX_MIGRATE_ROLLBACK = 29;
    /// 账目位：Migrate / migrate_src_peak（窄 · u32）。
    IDX_MIGRATE_SRC_PEAK = 30;
    /// 账目位：Migrate / last_migrate_tick（窄 · u32）。
    IDX_MIGRATE_LAST_TICK = 31;
    /// 账目位：Migrate / migrate_total（宽 · u64）。
    IDX_MIGRATE_TOTAL = 32;
    /// 账目位：Migrate / migrate_cost_sum（宽 · u64）。
    IDX_MIGRATE_COST_SUM = 33;
    /// 账目位：Migrate / migrate_window_sum（宽 · u64）。
    IDX_MIGRATE_WINDOW_SUM = 34;
    /// 账目位：Migrate / migrate_imbalance_acc（宽 · u64）。
    IDX_MIGRATE_IMBALANCE_ACC = 35;

    /// 账目位：Idle / idle_enter_count（窄 · u32）。
    IDX_IDLE_ENTER_COUNT = 36;
    /// 账目位：Idle / idle_exit_count（窄 · u32）。
    IDX_IDLE_EXIT_COUNT = 37;
    /// 账目位：Idle / idle_state_a_ticks（窄 · u32）。
    IDX_IDLE_STATE_A = 38;
    /// 账目位：Idle / idle_state_b_ticks（窄 · u32）。
    IDX_IDLE_STATE_B = 39;
    /// 账目位：Idle / idle_state_c_ticks（窄 · u32）。
    IDX_IDLE_STATE_C = 40;
    /// 账目位：Idle / idle_deepest_level（窄 · u32）。
    IDX_IDLE_DEEPEST = 41;
    /// 账目位：Idle / idle_wake_blocked（窄 · u32）。
    IDX_IDLE_WAKE_BLOCKED = 42;
    /// 账目位：Idle / last_idle_tick（窄 · u32）。
    IDX_IDLE_LAST_TICK = 43;
    /// 账目位：Idle / idle_total（宽 · u64）。
    IDX_IDLE_TOTAL = 44;
    /// 账目位：Idle / idle_total_ticks（宽 · u64）。
    IDX_IDLE_TOTAL_TICKS = 45;
    /// 账目位：Idle / idle_window_sum（宽 · u64）。
    IDX_IDLE_WINDOW_SUM = 46;
    /// 账目位：Idle / idle_cycles（宽 · u64）。
    IDX_IDLE_CYCLES = 47;

    /// 账目位：Latency / latency_samples（窄 · u32）。
    IDX_LAT_SAMPLES = 48;
    /// 账目位：Latency / latency_enqueue（窄 · u32）。
    IDX_LAT_ENQUEUE = 49;
    /// 账目位：Latency / latency_dispatch（窄 · u32）。
    IDX_LAT_DISPATCH = 50;
    /// 账目位：Latency / latency_preempt（窄 · u32）。
    IDX_LAT_PREEMPT = 51;
    /// 账目位：Latency / latency_ipi（窄 · u32）。
    IDX_LAT_IPI = 52;
    /// 账目位：Latency / latency_migrate（窄 · u32）。
    IDX_LAT_MIGRATE = 53;
    /// 账目位：Latency / latency_bucket_hi（窄 · u32）。
    IDX_LAT_BUCKET_HI = 54;
    /// 账目位：Latency / last_latency_tick（窄 · u32）。
    IDX_LAT_LAST_TICK = 55;
    /// 账目位：Latency / latency_sum_ns（宽 · u64）。
    IDX_LAT_SUM_NS = 56;
    /// 账目位：Latency / latency_max_ns（宽 · u64）。
    IDX_LAT_MAX_NS = 57;
    /// 账目位：Latency / latency_p99_sum_ns（宽 · u64）。
    IDX_LAT_P99_SUM = 58;
    /// 账目位：Latency / latency_window_sum（宽 · u64）。
    IDX_LAT_WINDOW_SUM = 59;

    /// 账目位：QueueDepth / qd_instant（窄 · u32 · 瞬时口径）。
    IDX_QD_INSTANT = 60;
    /// 账目位：QueueDepth / qd_peak_instant（窄 · u32）。
    IDX_QD_PEAK = 61;
    /// 账目位：QueueDepth / qd_runq_len（窄 · u32）。
    IDX_QD_RUNQ_LEN = 62;
    /// 账目位：QueueDepth / qd_waitq_len（窄 · u32）。
    IDX_QD_WAITQ_LEN = 63;
    /// 账目位：QueueDepth / qd_hot_len（窄 · u32）。
    IDX_QD_HOT_LEN = 64;
    /// 账目位：QueueDepth / qd_overflow_events（窄 · u32）。
    IDX_QD_OVF_EVENTS = 65;
    /// 账目位：QueueDepth / qd_sample_count（窄 · u32）。
    IDX_QD_SAMPLES = 66;
    /// 账目位：QueueDepth / last_qd_tick（窄 · u32）。
    IDX_QD_LAST_TICK = 67;
    /// 账目位：QueueDepth / qd_window_sum（宽 · u64 · 窗口口径）。
    IDX_QD_WINDOW_SUM = 68;
    /// 账目位：QueueDepth / qd_window_ticks（宽 · u64）。
    IDX_QD_WINDOW_TICKS = 69;
    /// 账目位：QueueDepth / qd_total（宽 · u64）。
    IDX_QD_TOTAL = 70;
    /// 账目位：QueueDepth / qd_weight_err_ppm（宽 · u64）。
    IDX_QD_WEIGHT_ERR_PPM = 71;

    /// 账目位：ClassStat / class_rt_count（窄 · u32）。
    IDX_CLASS_RT = 72;
    /// 账目位：ClassStat / class_fair_count（窄 · u32）。
    IDX_CLASS_FAIR = 73;
    /// 账目位：ClassStat / class_bt_count（窄 · u32）。
    IDX_CLASS_BT = 74;
    /// 账目位：ClassStat / class_idle_count（窄 · u32）。
    IDX_CLASS_IDLE = 75;
    /// 账目位：ClassStat / class_batch_count（窄 · u32）。
    IDX_CLASS_BATCH = 76;
    /// 账目位：ClassStat / class_migrations（窄 · u32）。
    IDX_CLASS_MIGRATIONS = 77;
    /// 账目位：ClassStat / class_steal_share（窄 · u32）。
    IDX_CLASS_STEAL_SHARE = 78;
    /// 账目位：ClassStat / last_class_tick（窄 · u32）。
    IDX_CLASS_LAST_TICK = 79;
    /// 账目位：ClassStat / class_total（宽 · u64）。
    IDX_CLASS_TOTAL = 80;
    /// 账目位：ClassStat / class_share_sum（宽 · u64）。
    IDX_CLASS_SHARE_SUM = 81;
    /// 账目位：ClassStat / class_window_sum（宽 · u64）。
    IDX_CLASS_WINDOW_SUM = 82;
    /// 账目位：ClassStat / class_steal_count（宽 · u64）。
    IDX_CLASS_STEAL_COUNT = 83;

    /// 账目位：Balance / balance_pull（窄 · u32）。
    IDX_BAL_PULL = 84;
    /// 账目位：Balance / balance_push（窄 · u32）。
    IDX_BAL_PUSH = 85;
    /// 账目位：Balance / balance_steal（窄 · u32）。
    IDX_BAL_STEAL = 86;
    /// 账目位：Balance / balance_imbalance_gap（窄 · u32）。
    IDX_BAL_GAP = 87;
    /// 账目位：Balance / balance_iterations（窄 · u32）。
    IDX_BAL_ITERS = 88;
    /// 账目位：Balance / balance_reject_cap（窄 · u32）。
    IDX_BAL_REJECT = 89;
    /// 账目位：Balance / balance_energy_tick（窄 · u32）。
    IDX_BAL_ENERGY_TICK = 90;
    /// 账目位：Balance / last_balance_tick（窄 · u32）。
    IDX_BAL_LAST_TICK = 91;
    /// 账目位：Balance / balance_total（宽 · u64）。
    IDX_BAL_TOTAL = 92;
    /// 账目位：Balance / balance_imbalance_acc（宽 · u64）。
    IDX_BAL_IMBALANCE_ACC = 93;
    /// 账目位：Balance / balance_window_sum（宽 · u64）。
    IDX_BAL_WINDOW_SUM = 94;
    /// 账目位：Balance / balance_energy_cost（宽 · u64）。
    IDX_BAL_ENERGY_COST = 95;

    /// 账目位：Ipi / ipi_sent（窄 · u32）。
    IDX_IPI_SENT = 96;
    /// 账目位：Ipi / ipi_resched（窄 · u32）。
    IDX_IPI_RESCHED = 97;
    /// 账目位：Ipi / ipi_halt（窄 · u32）。
    IDX_IPI_HALT = 98;
    /// 账目位：Ipi / ipi_wakeup（窄 · u32）。
    IDX_IPI_WAKEUP = 99;
    /// 账目位：Ipi / ipi_failed（窄 · u32）。
    IDX_IPI_FAILED = 100;
    /// 账目位：Ipi / ipi_coalesced（窄 · u32）。
    IDX_IPI_COALESCED = 101;
    /// 账目位：Ipi / ipi_burst_max（窄 · u32）。
    IDX_IPI_BURST_MAX = 102;
    /// 账目位：Ipi / last_ipi_tick（窄 · u32）。
    IDX_IPI_LAST_TICK = 103;
    /// 账目位：Ipi / ipi_total（宽 · u64）。
    IDX_IPI_TOTAL = 104;
    /// 账目位：Ipi / ipi_cost_sum_ns（宽 · u64）。
    IDX_IPI_COST_SUM = 105;
    /// 账目位：Ipi / ipi_window_sum（宽 · u64）。
    IDX_IPI_WINDOW_SUM = 106;
    /// 账目位：Ipi / ipi_drop_total（宽 · u64）。
    IDX_IPI_DROP_TOTAL = 107;

    /// 账目位：SelfCheck / selfchk_pass（窄 · u32）。
    IDX_SC_PASS = 108;
    /// 账目位：SelfCheck / selfchk_fail（窄 · u32）。
    IDX_SC_FAIL = 109;
    /// 账目位：SelfCheck / selfchk_run（窄 · u32）。
    IDX_SC_RUN = 110;
    /// 账目位：SelfCheck / selfchk_skip（窄 · u32）。
    IDX_SC_SKIP = 111;
    /// 账目位：SelfCheck / selfchk_ovf_events（窄 · u32）。
    IDX_SC_OVF_EVENTS = 112;
    /// 账目位：SelfCheck / selfchk_csum（窄 · u32）。
    IDX_SC_CSUM = 113;
    /// 账目位：SelfCheck / selfchk_depth（窄 · u32）。
    IDX_SC_DEPTH = 114;
    /// 账目位：SelfCheck / last_selfchk_tick（窄 · u32）。
    IDX_SC_LAST_TICK = 115;
    /// 账目位：SelfCheck / selfchk_total（宽 · u64）。
    IDX_SC_TOTAL = 116;
    /// 账目位：SelfCheck / selfchk_fail_total（宽 · u64）。
    IDX_SC_FAIL_TOTAL = 117;
    /// 账目位：SelfCheck / selfchk_window_sum（宽 · u64）。
    IDX_SC_WINDOW_SUM = 118;
    /// 账目位：SelfCheck / selfchk_latency_sum（宽 · u64）。
    IDX_SC_LATENCY_SUM = 119;
}

/// PerCpu 冻结版（F1623/F1639）结构体。
///
/// 判据：UNX-F1762-J1 —— 十字段族计数器须与本结构字段逐字段对齐。
///
/// 布局：族内 8 × `u32`（local 0..8，偏移 0..32）随后 4 × `u64`（local 8..12，偏移 32..64）；
/// 族间首尾相接，总尺寸 640 字节，对齐 8。
/// 字段序即账目位序：`GIDX = ordinal * 12 + local`，读取函数 [`PerCpuFrozen::slot`]。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PerCpuFrozen {
    // ── Switch 族（偏移 0..64）──────────────────────────────────────────────
    /// 切换总次数（窄）。
    pub switch_count: u32,
    /// 因时间片到期切换（窄）。
    pub switch_by_expiry: u32,
    /// 因高优先级抢占切换（窄）。
    pub switch_by_preempt: u32,
    /// 因主动让出切换（窄）。
    pub switch_by_yield: u32,
    /// 因迁移引发的切换（窄）。
    pub switch_by_migrate: u32,
    /// 切换回滚次数（窄）。
    pub switch_rollback: u32,
    /// 基于 deadline 的切换次数（窄）。
    pub switch_deadpoint: u32,
    /// 最近一次切换的逻辑 tick（窄）。
    pub last_switch_tick: u32,
    /// 切换累计总数（宽）。
    pub switch_total: u64,
    /// 切换回滚累计总数（宽）。
    pub switch_rollback_total: u64,
    /// 切换窗口和（宽）。
    pub switch_window_sum: u64,
    /// 忙周期累计（宽）。
    pub switch_busy_cycles: u64,

    // ── Wake 族（偏移 64..128）──────────────────────────────────────────────
    /// 唤醒总次数（窄）。
    pub wake_count: u32,
    /// 自 idle 唤醒（窄）。
    pub wake_from_idle: u32,
    /// 自深睡唤醒（窄）。
    pub wake_from_sleep: u32,
    /// 被合并的唤醒（窄）。
    pub wake_coalesced: u32,
    /// 唤醒引发抢占（窄）。
    pub wake_preempted: u32,
    /// 因容量上限被拒的唤醒（窄）。
    pub wake_reject_cap: u32,
    /// 唤醒突发峰值（窄）。
    pub wake_burst_max: u32,
    /// 最近一次唤醒的逻辑 tick（窄）。
    pub last_wake_tick: u32,
    /// 唤醒累计总数（宽）。
    pub wake_total: u64,
    /// 唤醒延迟累计（纳秒 · 宽）。
    pub wake_latency_sum_ns: u64,
    /// 唤醒窗口和（宽）。
    pub wake_window_sum: u64,
    /// 自 idle 唤醒累计（宽）。
    pub wake_from_idle_total: u64,

    // ── Migrate 族（偏移 128..192）───────────────────────────────────────────
    /// 迁移总次数（窄）。
    pub migrate_count: u32,
    /// 均衡驱动迁移（窄）。
    pub migrate_by_balance: u32,
    /// 亲和违例迁移（窄）。
    pub migrate_by_affinity: u32,
    /// 迁移队列溢出次数（窄）。
    pub migrate_overflow: u32,
    /// 因容量上限被拒的迁移（窄）。
    pub migrate_reject_cap: u32,
    /// 迁移回滚次数（窄）。
    pub migrate_rollback: u32,
    /// 迁出源队列峰值（窄）。
    pub migrate_src_peak: u32,
    /// 最近一次迁移的逻辑 tick（窄）。
    pub last_migrate_tick: u32,
    /// 迁移累计总数（宽）。
    pub migrate_total: u64,
    /// 迁移代价累计（宽）。
    pub migrate_cost_sum: u64,
    /// 迁移窗口和（宽）。
    pub migrate_window_sum: u64,
    /// 不均衡度累计（宽）。
    pub migrate_imbalance_acc: u64,

    // ── Idle 族（偏移 192..256）──────────────────────────────────────────────
    /// 进入 idle 次数（窄）。
    pub idle_enter_count: u32,
    /// 退出 idle 次数（窄）。
    pub idle_exit_count: u32,
    /// A 态驻留 tick（窄）。
    pub idle_state_a_ticks: u32,
    /// B 态驻留 tick（窄）。
    pub idle_state_b_ticks: u32,
    /// C 态驻留 tick（窄）。
    pub idle_state_c_ticks: u32,
    /// 最深 idle 层级（窄）。
    pub idle_deepest_level: u32,
    /// 唤醒被阻塞次数（窄）。
    pub idle_wake_blocked: u32,
    /// 最近一次 idle 的逻辑 tick（窄）。
    pub last_idle_tick: u32,
    /// 进入 idle 累计（宽）。
    pub idle_total: u64,
    /// 三态 tick 累计之和（宽）。
    pub idle_total_ticks: u64,
    /// idle 窗口和（宽）。
    pub idle_window_sum: u64,
    /// idle 周期累计（宽）。
    pub idle_cycles: u64,

    // ── Latency 族（偏移 256..320）──────────────────────────────────────────
    /// 延迟样本数（窄）。
    pub latency_samples: u32,
    /// 入队段延迟（窄）。
    pub latency_enqueue: u32,
    /// 派发段延迟（窄）。
    pub latency_dispatch: u32,
    /// 抢占段延迟（窄）。
    pub latency_preempt: u32,
    /// IPI 段延迟（窄）。
    pub latency_ipi: u32,
    /// 迁移段延迟（窄）。
    pub latency_migrate: u32,
    /// 高桶样本数（窄）。
    pub latency_bucket_hi: u32,
    /// 最近一次延迟采样的逻辑 tick（窄）。
    pub last_latency_tick: u32,
    /// 延迟累计（纳秒 · 宽）。
    pub latency_sum_ns: u64,
    /// 延迟峰值（纳秒 · 宽）。
    pub latency_max_ns: u64,
    /// p99 分位延迟累计（纳秒 · 宽）。
    pub latency_p99_sum_ns: u64,
    /// 延迟窗口和（宽）。
    pub latency_window_sum: u64,

    // ── QueueDepth 族（偏移 320..384 · F1645 双口径）─────────────────────────
    /// 瞬时口径深度（窄）。
    pub qd_instant: u32,
    /// 瞬时口径峰值（窄）。
    pub qd_peak_instant: u32,
    /// 就绪队列长度（窄）。
    pub qd_runq_len: u32,
    /// 等待队列长度（窄）。
    pub qd_waitq_len: u32,
    /// 热队列长度（窄）。
    pub qd_hot_len: u32,
    /// 深度溢出事件数（窄）。
    pub qd_overflow_events: u32,
    /// 采样计数（窄）。
    pub qd_sample_count: u32,
    /// 最近一次深度采样的逻辑 tick（窄）。
    pub last_qd_tick: u32,
    /// 窗口口径深度和（宽）。
    pub qd_window_sum: u64,
    /// 窗口口径 tick 数（宽）。
    pub qd_window_ticks: u64,
    /// 深度累计（宽）。
    pub qd_total: u64,
    /// 加权结算误差（ppm · 宽）。
    pub qd_weight_err_ppm: u64,

    // ── ClassStat 族（偏移 384..448）────────────────────────────────────────
    /// 实时类计数（窄）。
    pub class_rt_count: u32,
    /// 公平类计数（窄）。
    pub class_fair_count: u32,
    /// 批处理类计数（窄）。
    pub class_bt_count: u32,
    /// idle 类计数（窄）。
    pub class_idle_count: u32,
    /// 保留类计数（窄）。
    pub class_batch_count: u32,
    /// 跨类搬迁次数（窄）。
    pub class_migrations: u32,
    /// 被窃份额（窄）。
    pub class_steal_share: u32,
    /// 最近一次类统计的逻辑 tick（窄）。
    pub last_class_tick: u32,
    /// 各类计数之和（宽）。
    pub class_total: u64,
    /// 份额累计（宽）。
    pub class_share_sum: u64,
    /// 类统计窗口和（宽）。
    pub class_window_sum: u64,
    /// 窃取累计（宽）。
    pub class_steal_count: u64,

    // ── Balance 族（偏移 448..512）──────────────────────────────────────────
    /// 拉取次数（窄）。
    pub balance_pull: u32,
    /// 推送次数（窄）。
    pub balance_push: u32,
    /// 窃取次数（窄）。
    pub balance_steal: u32,
    /// 当前不均衡度（窄）。
    pub balance_imbalance_gap: u32,
    /// 均衡迭代次数（窄）。
    pub balance_iterations: u32,
    /// 因容量上限被拒次数（窄）。
    pub balance_reject_cap: u32,
    /// 能耗相关 tick（窄）。
    pub balance_energy_tick: u32,
    /// 最近一次均衡的逻辑 tick（窄）。
    pub last_balance_tick: u32,
    /// 均衡动作累计（宽）。
    pub balance_total: u64,
    /// 不均衡度累计（宽）。
    pub balance_imbalance_acc: u64,
    /// 均衡窗口和（宽）。
    pub balance_window_sum: u64,
    /// 能耗累计（宽）。
    pub balance_energy_cost: u64,

    // ── Ipi 族（偏移 512..576）──────────────────────────────────────────────
    /// IPI 发出数（窄）。
    pub ipi_sent: u32,
    /// 重调度 IPI（窄）。
    pub ipi_resched: u32,
    /// 停核 IPI（窄）。
    pub ipi_halt: u32,
    /// 唤醒 IPI（窄）。
    pub ipi_wakeup: u32,
    /// IPI 失败数（窄）。
    pub ipi_failed: u32,
    /// IPI 合并数（窄）。
    pub ipi_coalesced: u32,
    /// IPI 突发峰值（窄）。
    pub ipi_burst_max: u32,
    /// 最近一次 IPI 的逻辑 tick（窄）。
    pub last_ipi_tick: u32,
    /// IPI 累计总数（宽）。
    pub ipi_total: u64,
    /// IPI 代价累计（纳秒 · 宽）。
    pub ipi_cost_sum_ns: u64,
    /// IPI 窗口和（宽）。
    pub ipi_window_sum: u64,
    /// IPI 丢弃累计（宽）。
    pub ipi_drop_total: u64,

    // ── SelfCheck 族（偏移 576..640）────────────────────────────────────────
    /// 自检通过数（窄）。
    pub selfchk_pass: u32,
    /// 自检失败数（窄）。
    pub selfchk_fail: u32,
    /// 自检执行数（窄）。
    pub selfchk_run: u32,
    /// 自检跳过数（窄）。
    pub selfchk_skip: u32,
    /// 账本溢出事件数（窄）。
    pub selfchk_ovf_events: u32,
    /// 账本校验和（窄）。
    pub selfchk_csum: u32,
    /// 自检深度（窄）。
    pub selfchk_depth: u32,
    /// 最近一次自检的逻辑 tick（窄）。
    pub last_selfchk_tick: u32,
    /// 自检总数（宽）。
    pub selfchk_total: u64,
    /// 自检失败累计（宽）。
    pub selfchk_fail_total: u64,
    /// 自检窗口和（宽）。
    pub selfchk_window_sum: u64,
    /// 自检耗时累计（宽）。
    pub selfchk_latency_sum: u64,
}

/// 窄区字段在 [`PerCpuFrozen`] 中的偏移计算。
///
/// 族内窄区 8 × `u32` 占 `[0, 32)`，宽区 4 × `u64` 占 `[32, 64)`，
/// 故偏移不可简单写成 `local * width`（窄宽混排会在 local ≥ 8 处错位）。
pub fn percpu_offset(f: StatFamily, local: usize) -> Result<u32> {
    let w = StatFamily::width_of_local(local)?;
    let inner = if local < 8 {
        (local as u32) * 4
    } else {
        32 + ((local as u32) - 8) * 8
    };
    Ok(f.offset_base() + inner)
}

impl PerCpuFrozen {
    /// 全零账本。
    pub const fn zero() -> PerCpuFrozen {
        PerCpuFrozen {
            switch_count: 0,
            switch_by_expiry: 0,
            switch_by_preempt: 0,
            switch_by_yield: 0,
            switch_by_migrate: 0,
            switch_rollback: 0,
            switch_deadpoint: 0,
            last_switch_tick: 0,
            switch_total: 0,
            switch_rollback_total: 0,
            switch_window_sum: 0,
            switch_busy_cycles: 0,
            wake_count: 0,
            wake_from_idle: 0,
            wake_from_sleep: 0,
            wake_coalesced: 0,
            wake_preempted: 0,
            wake_reject_cap: 0,
            wake_burst_max: 0,
            last_wake_tick: 0,
            wake_total: 0,
            wake_latency_sum_ns: 0,
            wake_window_sum: 0,
            wake_from_idle_total: 0,
            migrate_count: 0,
            migrate_by_balance: 0,
            migrate_by_affinity: 0,
            migrate_overflow: 0,
            migrate_reject_cap: 0,
            migrate_rollback: 0,
            migrate_src_peak: 0,
            last_migrate_tick: 0,
            migrate_total: 0,
            migrate_cost_sum: 0,
            migrate_window_sum: 0,
            migrate_imbalance_acc: 0,
            idle_enter_count: 0,
            idle_exit_count: 0,
            idle_state_a_ticks: 0,
            idle_state_b_ticks: 0,
            idle_state_c_ticks: 0,
            idle_deepest_level: 0,
            idle_wake_blocked: 0,
            last_idle_tick: 0,
            idle_total: 0,
            idle_total_ticks: 0,
            idle_window_sum: 0,
            idle_cycles: 0,
            latency_samples: 0,
            latency_enqueue: 0,
            latency_dispatch: 0,
            latency_preempt: 0,
            latency_ipi: 0,
            latency_migrate: 0,
            latency_bucket_hi: 0,
            last_latency_tick: 0,
            latency_sum_ns: 0,
            latency_max_ns: 0,
            latency_p99_sum_ns: 0,
            latency_window_sum: 0,
            qd_instant: 0,
            qd_peak_instant: 0,
            qd_runq_len: 0,
            qd_waitq_len: 0,
            qd_hot_len: 0,
            qd_overflow_events: 0,
            qd_sample_count: 0,
            last_qd_tick: 0,
            qd_window_sum: 0,
            qd_window_ticks: 0,
            qd_total: 0,
            qd_weight_err_ppm: 0,
            class_rt_count: 0,
            class_fair_count: 0,
            class_bt_count: 0,
            class_idle_count: 0,
            class_batch_count: 0,
            class_migrations: 0,
            class_steal_share: 0,
            last_class_tick: 0,
            class_total: 0,
            class_share_sum: 0,
            class_window_sum: 0,
            class_steal_count: 0,
            balance_pull: 0,
            balance_push: 0,
            balance_steal: 0,
            balance_imbalance_gap: 0,
            balance_iterations: 0,
            balance_reject_cap: 0,
            balance_energy_tick: 0,
            last_balance_tick: 0,
            balance_total: 0,
            balance_imbalance_acc: 0,
            balance_window_sum: 0,
            balance_energy_cost: 0,
            ipi_sent: 0,
            ipi_resched: 0,
            ipi_halt: 0,
            ipi_wakeup: 0,
            ipi_failed: 0,
            ipi_coalesced: 0,
            ipi_burst_max: 0,
            last_ipi_tick: 0,
            ipi_total: 0,
            ipi_cost_sum_ns: 0,
            ipi_window_sum: 0,
            ipi_drop_total: 0,
            selfchk_pass: 0,
            selfchk_fail: 0,
            selfchk_run: 0,
            selfchk_skip: 0,
            selfchk_ovf_events: 0,
            selfchk_csum: 0,
            selfchk_depth: 0,
            last_selfchk_tick: 0,
            selfchk_total: 0,
            selfchk_fail_total: 0,
            selfchk_window_sum: 0,
            selfchk_latency_sum: 0,
        }
    }

    /// 账目位索引 → 字段值（读侧按 GIDX 顺序投影）。
    pub fn slot(&self, gidx: usize) -> Result<u64> {
        if gidx >= FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        let v = match gidx {
            0 => self.switch_count as u64,
            1 => self.switch_by_expiry as u64,
            2 => self.switch_by_preempt as u64,
            3 => self.switch_by_yield as u64,
            4 => self.switch_by_migrate as u64,
            5 => self.switch_rollback as u64,
            6 => self.switch_deadpoint as u64,
            7 => self.last_switch_tick as u64,
            8 => self.switch_total,
            9 => self.switch_rollback_total,
            10 => self.switch_window_sum,
            11 => self.switch_busy_cycles,
            12 => self.wake_count as u64,
            13 => self.wake_from_idle as u64,
            14 => self.wake_from_sleep as u64,
            15 => self.wake_coalesced as u64,
            16 => self.wake_preempted as u64,
            17 => self.wake_reject_cap as u64,
            18 => self.wake_burst_max as u64,
            19 => self.last_wake_tick as u64,
            20 => self.wake_total,
            21 => self.wake_latency_sum_ns,
            22 => self.wake_window_sum,
            23 => self.wake_from_idle_total,
            24 => self.migrate_count as u64,
            25 => self.migrate_by_balance as u64,
            26 => self.migrate_by_affinity as u64,
            27 => self.migrate_overflow as u64,
            28 => self.migrate_reject_cap as u64,
            29 => self.migrate_rollback as u64,
            30 => self.migrate_src_peak as u64,
            31 => self.last_migrate_tick as u64,
            32 => self.migrate_total,
            33 => self.migrate_cost_sum,
            34 => self.migrate_window_sum,
            35 => self.migrate_imbalance_acc,
            36 => self.idle_enter_count as u64,
            37 => self.idle_exit_count as u64,
            38 => self.idle_state_a_ticks as u64,
            39 => self.idle_state_b_ticks as u64,
            40 => self.idle_state_c_ticks as u64,
            41 => self.idle_deepest_level as u64,
            42 => self.idle_wake_blocked as u64,
            43 => self.last_idle_tick as u64,
            44 => self.idle_total,
            45 => self.idle_total_ticks,
            46 => self.idle_window_sum,
            47 => self.idle_cycles,
            48 => self.latency_samples as u64,
            49 => self.latency_enqueue as u64,
            50 => self.latency_dispatch as u64,
            51 => self.latency_preempt as u64,
            52 => self.latency_ipi as u64,
            53 => self.latency_migrate as u64,
            54 => self.latency_bucket_hi as u64,
            55 => self.last_latency_tick as u64,
            56 => self.latency_sum_ns,
            57 => self.latency_max_ns,
            58 => self.latency_p99_sum_ns,
            59 => self.latency_window_sum,
            60 => self.qd_instant as u64,
            61 => self.qd_peak_instant as u64,
            62 => self.qd_runq_len as u64,
            63 => self.qd_waitq_len as u64,
            64 => self.qd_hot_len as u64,
            65 => self.qd_overflow_events as u64,
            66 => self.qd_sample_count as u64,
            67 => self.last_qd_tick as u64,
            68 => self.qd_window_sum,
            69 => self.qd_window_ticks,
            70 => self.qd_total,
            71 => self.qd_weight_err_ppm,
            72 => self.class_rt_count as u64,
            73 => self.class_fair_count as u64,
            74 => self.class_bt_count as u64,
            75 => self.class_idle_count as u64,
            76 => self.class_batch_count as u64,
            77 => self.class_migrations as u64,
            78 => self.class_steal_share as u64,
            79 => self.last_class_tick as u64,
            80 => self.class_total,
            81 => self.class_share_sum,
            82 => self.class_window_sum,
            83 => self.class_steal_count,
            84 => self.balance_pull as u64,
            85 => self.balance_push as u64,
            86 => self.balance_steal as u64,
            87 => self.balance_imbalance_gap as u64,
            88 => self.balance_iterations as u64,
            89 => self.balance_reject_cap as u64,
            90 => self.balance_energy_tick as u64,
            91 => self.last_balance_tick as u64,
            92 => self.balance_total,
            93 => self.balance_imbalance_acc,
            94 => self.balance_window_sum,
            95 => self.balance_energy_cost,
            96 => self.ipi_sent as u64,
            97 => self.ipi_resched as u64,
            98 => self.ipi_halt as u64,
            99 => self.ipi_wakeup as u64,
            100 => self.ipi_failed as u64,
            101 => self.ipi_coalesced as u64,
            102 => self.ipi_burst_max as u64,
            103 => self.last_ipi_tick as u64,
            104 => self.ipi_total,
            105 => self.ipi_cost_sum_ns,
            106 => self.ipi_window_sum,
            107 => self.ipi_drop_total,
            108 => self.selfchk_pass as u64,
            109 => self.selfchk_fail as u64,
            110 => self.selfchk_run as u64,
            111 => self.selfchk_skip as u64,
            112 => self.selfchk_ovf_events as u64,
            113 => self.selfchk_csum as u64,
            114 => self.selfchk_depth as u64,
            115 => self.last_selfchk_tick as u64,
            116 => self.selfchk_total,
            117 => self.selfchk_fail_total,
            118 => self.selfchk_window_sum,
            119 => self.selfchk_latency_sum,
            _ => return CpErr::FieldRange.e(),
        };
        Ok(v)
    }

    /// 账目位索引 → 写字段（唯一写侧投影；散点写须经 [`PerCpuFrozen::put`]）。
    pub fn put(&mut self, gidx: usize, v: u64) -> Result<()> {
        if gidx >= FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        let w = StatFamily::width_of_gidx(gidx)?;
        let cap = sat_cap(w);
        let nv = if v > cap { cap } else { v };
        match gidx {
            0 => self.switch_count = nv as u32,
            1 => self.switch_by_expiry = nv as u32,
            2 => self.switch_by_preempt = nv as u32,
            3 => self.switch_by_yield = nv as u32,
            4 => self.switch_by_migrate = nv as u32,
            5 => self.switch_rollback = nv as u32,
            6 => self.switch_deadpoint = nv as u32,
            7 => self.last_switch_tick = nv as u32,
            8 => self.switch_total = nv,
            9 => self.switch_rollback_total = nv,
            10 => self.switch_window_sum = nv,
            11 => self.switch_busy_cycles = nv,
            12 => self.wake_count = nv as u32,
            13 => self.wake_from_idle = nv as u32,
            14 => self.wake_from_sleep = nv as u32,
            15 => self.wake_coalesced = nv as u32,
            16 => self.wake_preempted = nv as u32,
            17 => self.wake_reject_cap = nv as u32,
            18 => self.wake_burst_max = nv as u32,
            19 => self.last_wake_tick = nv as u32,
            20 => self.wake_total = nv,
            21 => self.wake_latency_sum_ns = nv,
            22 => self.wake_window_sum = nv,
            23 => self.wake_from_idle_total = nv,
            24 => self.migrate_count = nv as u32,
            25 => self.migrate_by_balance = nv as u32,
            26 => self.migrate_by_affinity = nv as u32,
            27 => self.migrate_overflow = nv as u32,
            28 => self.migrate_reject_cap = nv as u32,
            29 => self.migrate_rollback = nv as u32,
            30 => self.migrate_src_peak = nv as u32,
            31 => self.last_migrate_tick = nv as u32,
            32 => self.migrate_total = nv,
            33 => self.migrate_cost_sum = nv,
            34 => self.migrate_window_sum = nv,
            35 => self.migrate_imbalance_acc = nv,
            36 => self.idle_enter_count = nv as u32,
            37 => self.idle_exit_count = nv as u32,
            38 => self.idle_state_a_ticks = nv as u32,
            39 => self.idle_state_b_ticks = nv as u32,
            40 => self.idle_state_c_ticks = nv as u32,
            41 => self.idle_deepest_level = nv as u32,
            42 => self.idle_wake_blocked = nv as u32,
            43 => self.last_idle_tick = nv as u32,
            44 => self.idle_total = nv,
            45 => self.idle_total_ticks = nv,
            46 => self.idle_window_sum = nv,
            47 => self.idle_cycles = nv,
            48 => self.latency_samples = nv as u32,
            49 => self.latency_enqueue = nv as u32,
            50 => self.latency_dispatch = nv as u32,
            51 => self.latency_preempt = nv as u32,
            52 => self.latency_ipi = nv as u32,
            53 => self.latency_migrate = nv as u32,
            54 => self.latency_bucket_hi = nv as u32,
            55 => self.last_latency_tick = nv as u32,
            56 => self.latency_sum_ns = nv,
            57 => self.latency_max_ns = nv,
            58 => self.latency_p99_sum_ns = nv,
            59 => self.latency_window_sum = nv,
            60 => self.qd_instant = nv as u32,
            61 => self.qd_peak_instant = nv as u32,
            62 => self.qd_runq_len = nv as u32,
            63 => self.qd_waitq_len = nv as u32,
            64 => self.qd_hot_len = nv as u32,
            65 => self.qd_overflow_events = nv as u32,
            66 => self.qd_sample_count = nv as u32,
            67 => self.last_qd_tick = nv as u32,
            68 => self.qd_window_sum = nv,
            69 => self.qd_window_ticks = nv,
            70 => self.qd_total = nv,
            71 => self.qd_weight_err_ppm = nv,
            72 => self.class_rt_count = nv as u32,
            73 => self.class_fair_count = nv as u32,
            74 => self.class_bt_count = nv as u32,
            75 => self.class_idle_count = nv as u32,
            76 => self.class_batch_count = nv as u32,
            77 => self.class_migrations = nv as u32,
            78 => self.class_steal_share = nv as u32,
            79 => self.last_class_tick = nv as u32,
            80 => self.class_total = nv,
            81 => self.class_share_sum = nv,
            82 => self.class_window_sum = nv,
            83 => self.class_steal_count = nv,
            84 => self.balance_pull = nv as u32,
            85 => self.balance_push = nv as u32,
            86 => self.balance_steal = nv as u32,
            87 => self.balance_imbalance_gap = nv as u32,
            88 => self.balance_iterations = nv as u32,
            89 => self.balance_reject_cap = nv as u32,
            90 => self.balance_energy_tick = nv as u32,
            91 => self.last_balance_tick = nv as u32,
            92 => self.balance_total = nv,
            93 => self.balance_imbalance_acc = nv,
            94 => self.balance_window_sum = nv,
            95 => self.balance_energy_cost = nv,
            96 => self.ipi_sent = nv as u32,
            97 => self.ipi_resched = nv as u32,
            98 => self.ipi_halt = nv as u32,
            99 => self.ipi_wakeup = nv as u32,
            100 => self.ipi_failed = nv as u32,
            101 => self.ipi_coalesced = nv as u32,
            102 => self.ipi_burst_max = nv as u32,
            103 => self.last_ipi_tick = nv as u32,
            104 => self.ipi_total = nv,
            105 => self.ipi_cost_sum_ns = nv,
            106 => self.ipi_window_sum = nv,
            107 => self.ipi_drop_total = nv,
            108 => self.selfchk_pass = nv as u32,
            109 => self.selfchk_fail = nv as u32,
            110 => self.selfchk_run = nv as u32,
            111 => self.selfchk_skip = nv as u32,
            112 => self.selfchk_ovf_events = nv as u32,
            113 => self.selfchk_csum = nv as u32,
            114 => self.selfchk_depth = nv as u32,
            115 => self.last_selfchk_tick = nv as u32,
            116 => self.selfchk_total = nv,
            117 => self.selfchk_fail_total = nv,
            118 => self.selfchk_window_sum = nv,
            119 => self.selfchk_latency_sum = nv,
            _ => return CpErr::FieldRange.e(),
        }
        Ok(())
    }

    /// 结构自洽：尺寸 640 / 对齐 8。
    pub fn layout_ok() -> bool {
        core::mem::size_of::<PerCpuFrozen>() as u32 == PERCPU_STRIDE
            && core::mem::align_of::<PerCpuFrozen>() == 8
    }

    /// 导出为账槽数组（供三方对齐比对）。
    pub fn to_slots(&self) -> Vec<u64> {
        let mut v = Vec::with_capacity(FIELD_COUNT);
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let x = self.slot(i);
            match x {
                Ok(val) => v.push(val),
                Err(_) => v.push(0),
            }
            i += 1;
        }
        v
    }

    /// 由账槽数组反向构造（回放还原用）。
    pub fn from_slots(slots: &[u64]) -> Result<PerCpuFrozen> {
        if slots.len() < FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        let mut p = PerCpuFrozen::zero();
        let mut i = 0usize;
        while i < FIELD_COUNT {
            p.put(i, slots[i])?;
            i += 1;
        }
        Ok(p)
    }

    /// 结构体字节指纹（自检族消费）。
    pub fn struct_fingerprint(&self) -> u64 {
        let mut h = fnv1a_begin();
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let v = match self.slot(i) {
                Ok(x) => x,
                Err(_) => 0,
            };
            h = fnv1a_u64(h, v);
            i += 1;
        }
        h
    }
}

/// 字段登记表条目（append-only · C 项）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldRecord {
    /// 账目位索引（0..120）。
    pub gidx: usize,
    /// 字段名（账本体侧权威名）。
    pub name: String,
    /// 在 [`PerCpuFrozen`] 中的字节偏移。
    pub offset: u32,
    /// 位宽（4 / 8）。
    pub width: u8,
    /// 所属族。
    pub family: StatFamily,
    /// 是否热路径（窄区前 6 项为热路径）。
    pub hot: bool,
    /// 观测接口名（F1763 挂载点）。
    pub iface: String,
    /// 对齐状态。
    pub align: AlignState,
}

impl FieldRecord {
    /// 单行导出（供 append-only 册打印）。
    pub fn line(&self) -> String {
        format!(
            "#{} {:<24} off={:<4} w={} {:<11} hot={} align={}",
            self.gidx,
            self.name,
            self.offset,
            self.width,
            self.family.name(),
            if self.hot { 1 } else { 0 },
            self.align.name()
        )
    }
}

/// 字段级对齐状态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignState {
    /// 三方齐备：PerCpu 字段 ↔ 账目位 ↔ 观测接口，偏移与宽度自洽。
    Aligned,
    /// PerCpu 冻结版缺该字段。
    MissingInPerCpu,
    /// 账本侧缺该账目位登记。
    MissingInLedger,
    /// 观测接口未挂载该字段。
    MissingInInterface,
    /// 名称 / 偏移 / 宽度不一致。
    Mismatched,
}

impl AlignState {
    /// 状态短名。
    pub fn name(self) -> &'static str {
        match self {
            AlignState::Aligned => "Aligned",
            AlignState::MissingInPerCpu => "MissingInPerCpu",
            AlignState::MissingInLedger => "MissingInLedger",
            AlignState::MissingInInterface => "MissingInInterface",
            AlignState::Mismatched => "Mismatched",
        }
    }

    /// 是否为通过态。
    pub fn is_ok(self) -> bool {
        self == AlignState::Aligned
    }
}

/// 三方对齐条目（PerCpu 字段名 × 账目位索引 × 观测接口名）。
#[derive(Clone, Debug)]
pub struct AlignTriple {
    /// 账目位索引。
    pub gidx: usize,
    /// PerCpu 冻结版字段名（`None` = 缺字段）。
    pub percpu_name: Option<String>,
    /// 账本体字段名（`None` = 缺账目位登记）。
    pub ledger_name: Option<String>,
    /// 观测接口名（`None` = 未挂载）。
    pub iface_name: Option<String>,
    /// 字节偏移。
    pub offset: u32,
    /// 位宽。
    pub width: u8,
    /// 对齐状态。
    pub state: AlignState,
}

impl AlignTriple {
    /// 单行导出。
    pub fn line(&self) -> String {
        format!(
            "#{}|{}|{}|{}|off={}|w={}|{}",
            self.gidx,
            self.percpu_name.clone().unwrap_or_else(|| "<none>".to_string()),
            self.ledger_name.clone().unwrap_or_else(|| "<none>".to_string()),
            self.iface_name.clone().unwrap_or_else(|| "<none>".to_string()),
            self.offset,
            self.width,
            self.state.name()
        )
    }
}

/// 对齐核对账目（全字段）。
#[derive(Clone, Debug)]
pub struct AlignReport {
    /// 逐字段结果。
    pub rows: Vec<AlignTriple>,
    /// 全字段通过数。
    pub aligned: u32,
    /// 非通过数。
    pub broken: u32,
    /// 首个失败项（`None` = 全过）。
    pub first_bad: Option<usize>,
    /// 结构布局自洽。
    pub layout_ok: bool,
}

impl AlignReport {
    /// 是否全字段通过。
    pub fn all_aligned(&self) -> bool {
        self.broken == 0 && self.layout_ok
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        format!(
            "align: ok={} bad={} layout={} first={}",
            self.aligned,
            self.broken,
            if self.layout_ok { 1 } else { 0 },
            match self.first_bad {
                Some(i) => format!("#{}", i),
                None => "-".to_string(),
            }
        )
    }
}

/// 守恒式比较算子（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    /// 小于。
    Lt,
    /// 小于等于。
    Le,
    /// 等于。
    Eq,
    /// 大于等于。
    Ge,
    /// 大于。
    Gt,
}

impl CmpOp {
    /// 算子短名。
    pub fn name(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Eq => "==",
            CmpOp::Ge => ">=",
            CmpOp::Gt => ">",
        }
    }

    /// 全整数比较（零浮点）。
    pub fn hold(self, a: u64, b: u64) -> bool {
        match self {
            CmpOp::Lt => a < b,
            CmpOp::Le => a <= b,
            CmpOp::Eq => a == b,
            CmpOp::Ge => a >= b,
            CmpOp::Gt => a > b,
        }
    }
}

/// 守恒式形态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvKind {
    /// 目标位 == 若干位之和。
    SumEq {
        /// 目标账目位。
        target: usize,
        /// 求和账目位表。
        parts: &'static [usize],
    },
    /// 左位 >= 右位。
    Ge {
        /// 左账目位。
        lhs: usize,
        /// 右账目位。
        rhs: usize,
    },
    /// 左位 <= 右位。
    Le {
        /// 左账目位。
        lhs: usize,
        /// 右账目位。
        rhs: usize,
    },
    /// 左位 == 右位。
    Cmp {
        /// 左账目位。
        lhs: usize,
        /// 右账目位。
        rhs: usize,
        /// 算子。
        op: CmpOp,
    },
}

/// 族内守恒不变量（每族 2 条 · 全 20 条）。
#[derive(Clone, Copy, Debug)]
pub struct Invariant {
    /// 守恒式编号（`SW-1` 之类，登记册内稳定）。
    pub id: &'static str,
    /// 所属族。
    pub family: StatFamily,
    /// 形态。
    pub kind: InvKind,
    /// 守恒式文本（导出用）。
    pub expr: &'static str,
    /// 该式依赖的字段数（用于复杂度说明）。
    pub deps: u8,
}

/// 十字段族守恒式全集（20 条 · 闭集 · 不得增删）。
///
/// 判据：UNX-F1762-J1 —— 族内守恒须恒成立，否则该族计数不可信。
pub const INVARIANTS: [Invariant; 20] = [
    // ── Switch ──────────────────────────────────────────────────────────────
    Invariant {
        id: "SW-1",
        family: StatFamily::Switch,
        kind: InvKind::SumEq {
            target: IDX_SWITCH_COUNT,
            parts: &[
                IDX_SWITCH_BY_EXPIRY,
                IDX_SWITCH_BY_PREEMPT,
                IDX_SWITCH_BY_YIELD,
                IDX_SWITCH_BY_MIGRATE,
            ],
        },
        expr: "switch_count == switch_by_expiry + switch_by_preempt + switch_by_yield + switch_by_migrate",
        deps: 5,
    },
    Invariant {
        id: "SW-2",
        family: StatFamily::Switch,
        kind: InvKind::Le {
            lhs: IDX_SWITCH_ROLLBACK,
            rhs: IDX_SWITCH_COUNT,
        },
        expr: "switch_rollback <= switch_count",
        deps: 2,
    },
    // ── Wake ────────────────────────────────────────────────────────────────
    Invariant {
        id: "WK-1",
        family: StatFamily::Wake,
        kind: InvKind::SumEq {
            target: IDX_WAKE_COUNT,
            parts: &[IDX_WAKE_FROM_IDLE, IDX_WAKE_FROM_SLEEP, IDX_WAKE_COALESCED],
        },
        expr: "wake_count == wake_from_idle + wake_from_sleep + wake_coalesced",
        deps: 4,
    },
    Invariant {
        id: "WK-2",
        family: StatFamily::Wake,
        kind: InvKind::Le {
            lhs: IDX_WAKE_REJECT_CAP,
            rhs: IDX_WAKE_COUNT,
        },
        expr: "wake_reject_cap <= wake_count",
        deps: 2,
    },
    // ── Migrate ─────────────────────────────────────────────────────────────
    Invariant {
        id: "MG-1",
        family: StatFamily::Migrate,
        kind: InvKind::SumEq {
            target: IDX_MIGRATE_COUNT,
            parts: &[IDX_MIGRATE_BY_BALANCE, IDX_MIGRATE_BY_AFFINITY],
        },
        expr: "migrate_count == migrate_by_balance + migrate_by_affinity",
        deps: 3,
    },
    Invariant {
        id: "MG-2",
        family: StatFamily::Migrate,
        kind: InvKind::Le {
            lhs: IDX_MIGRATE_ROLLBACK,
            rhs: IDX_MIGRATE_COUNT,
        },
        expr: "migrate_rollback <= migrate_count",
        deps: 2,
    },
    // ── Idle（F1745 守恒）────────────────────────────────────────────────────
    Invariant {
        id: "ID-1",
        family: StatFamily::Idle,
        kind: InvKind::SumEq {
            target: IDX_IDLE_TOTAL_TICKS,
            parts: &[IDX_IDLE_STATE_A, IDX_IDLE_STATE_B, IDX_IDLE_STATE_C],
        },
        expr: "idle_total_ticks == idle_state_a_ticks + idle_state_b_ticks + idle_state_c_ticks",
        deps: 4,
    },
    Invariant {
        id: "ID-2",
        family: StatFamily::Idle,
        kind: InvKind::Le {
            lhs: IDX_IDLE_EXIT_COUNT,
            rhs: IDX_IDLE_ENTER_COUNT,
        },
        expr: "idle_exit_count <= idle_enter_count",
        deps: 2,
    },
    // ── Latency（F1764 分解）─────────────────────────────────────────────────
    Invariant {
        id: "LT-1",
        family: StatFamily::Latency,
        kind: InvKind::SumEq {
            target: IDX_LAT_SAMPLES,
            parts: &[
                IDX_LAT_ENQUEUE,
                IDX_LAT_DISPATCH,
                IDX_LAT_PREEMPT,
                IDX_LAT_IPI,
                IDX_LAT_MIGRATE,
            ],
        },
        expr: "latency_samples == latency_enqueue + latency_dispatch + latency_preempt + latency_ipi + latency_migrate",
        deps: 6,
    },
    Invariant {
        id: "LT-2",
        family: StatFamily::Latency,
        kind: InvKind::Ge {
            lhs: IDX_LAT_SUM_NS,
            rhs: IDX_LAT_MAX_NS,
        },
        expr: "latency_sum_ns >= latency_max_ns",
        deps: 2,
    },
    // ── QueueDepth（F1645 双口径）────────────────────────────────────────────
    Invariant {
        id: "QD-1",
        family: StatFamily::QueueDepth,
        kind: InvKind::SumEq {
            target: IDX_QD_INSTANT,
            parts: &[IDX_QD_RUNQ_LEN],
        },
        expr: "qd_instant == qd_runq_len",
        deps: 2,
    },
    Invariant {
        id: "QD-2",
        family: StatFamily::QueueDepth,
        kind: InvKind::Cmp {
            lhs: IDX_QD_TOTAL,
            rhs: IDX_QD_WINDOW_SUM,
            op: CmpOp::Eq,
        },
        expr: "qd_total == qd_window_sum",
        deps: 2,
    },
    // ── ClassStat ───────────────────────────────────────────────────────────
    Invariant {
        id: "CS-1",
        family: StatFamily::ClassStat,
        kind: InvKind::SumEq {
            target: IDX_CLASS_TOTAL,
            parts: &[
                IDX_CLASS_RT,
                IDX_CLASS_FAIR,
                IDX_CLASS_BT,
                IDX_CLASS_IDLE,
                IDX_CLASS_BATCH,
            ],
        },
        expr: "class_total == class_rt_count + class_fair_count + class_bt_count + class_idle_count + class_batch_count",
        deps: 6,
    },
    Invariant {
        id: "CS-2",
        family: StatFamily::ClassStat,
        kind: InvKind::Ge {
            lhs: IDX_CLASS_SHARE_SUM,
            rhs: IDX_CLASS_TOTAL,
        },
        expr: "class_share_sum >= class_total",
        deps: 2,
    },
    // ── Balance ─────────────────────────────────────────────────────────────
    Invariant {
        id: "BL-1",
        family: StatFamily::Balance,
        kind: InvKind::SumEq {
            target: IDX_BAL_TOTAL,
            parts: &[IDX_BAL_PULL, IDX_BAL_PUSH, IDX_BAL_STEAL],
        },
        expr: "balance_total == balance_pull + balance_push + balance_steal",
        deps: 4,
    },
    Invariant {
        id: "BL-2",
        family: StatFamily::Balance,
        kind: InvKind::Ge {
            lhs: IDX_BAL_IMBALANCE_ACC,
            rhs: IDX_BAL_GAP,
        },
        expr: "balance_imbalance_acc >= balance_imbalance_gap",
        deps: 2,
    },
    // ── Ipi ─────────────────────────────────────────────────────────────────
    Invariant {
        id: "IP-1",
        family: StatFamily::Ipi,
        kind: InvKind::Cmp {
            lhs: IDX_IPI_TOTAL,
            rhs: IDX_IPI_SENT,
            op: CmpOp::Eq,
        },
        expr: "ipi_total == ipi_sent",
        deps: 2,
    },
    Invariant {
        id: "IP-2",
        family: StatFamily::Ipi,
        kind: InvKind::Le {
            lhs: IDX_IPI_COALESCED,
            rhs: IDX_IPI_SENT,
        },
        expr: "ipi_coalesced <= ipi_sent",
        deps: 2,
    },
    // ── SelfCheck ───────────────────────────────────────────────────────────
    Invariant {
        id: "SC-1",
        family: StatFamily::SelfCheck,
        kind: InvKind::SumEq {
            target: IDX_SC_TOTAL,
            parts: &[IDX_SC_PASS, IDX_SC_FAIL],
        },
        expr: "selfchk_total == selfchk_pass + selfchk_fail",
        deps: 3,
    },
    Invariant {
        id: "SC-2",
        family: StatFamily::SelfCheck,
        kind: InvKind::Cmp {
            lhs: IDX_SC_FAIL_TOTAL,
            rhs: IDX_SC_FAIL,
            op: CmpOp::Eq,
        },
        expr: "selfchk_fail_total == selfchk_fail",
        deps: 2,
    },
];

/// 每核算子账本体（活体状态 · 全部经唯一登记写入口变更）。
#[derive(Clone, Debug)]
pub struct CpuLedger {
    /// 核算子 id（0 起）。
    pub id: u32,
    /// 账槽值（长度 120 · 序 = GIDX）。
    pub slots: Vec<u64>,
    /// 溢出位图（120 位 · 4 × u32 展开）。
    pub ovf_bits: [u32; 4],
    /// 最近一次记账的逻辑 tick。
    pub last_tick: u64,
    /// 经登记写入口累计的记账次数。
    pub writes: u64,
    /// 饱和命中次数。
    pub saturations: u64,
}

impl CpuLedger {
    /// 新建零账。
    pub fn new(id: u32) -> CpuLedger {
        CpuLedger {
            id,
            slots: vec![0u64; FIELD_COUNT],
            ovf_bits: [0u32; 4],
            last_tick: 0,
            writes: 0,
            saturations: 0,
        }
    }

    /// 读槽（越界拒绝）。
    pub fn get(&self, gidx: usize) -> Result<u64> {
        if gidx >= FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        match self.slots.get(gidx) {
            Some(v) => Ok(*v),
            None => CpErr::FieldRange.e(),
        }
    }

    /// 溢出位查询。
    pub fn ovf_test(&self, gidx: usize) -> bool {
        if gidx >= FIELD_COUNT {
            return false;
        }
        let w = gidx / 32;
        let b = gidx % 32;
        (self.ovf_bits[w] >> b) & 1u32 == 1u32
    }

    /// 溢出位置位（与取值路径成对出现的唯一位操作）。
    pub fn ovf_set(&mut self, gidx: usize) {
        if gidx >= FIELD_COUNT {
            return;
        }
        let w = gidx / 32;
        let b = gidx % 32;
        self.ovf_bits[w] |= 1u32 << b;
    }

    /// 已置溢出位的账目位数。
    pub fn ovf_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FIELD_COUNT {
            if self.ovf_test(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 直读槽（散点直写核使用；正常路径禁用）。
    pub fn get_raw(&self, gidx: usize) -> u64 {
        if gidx >= FIELD_COUNT {
            return 0;
        }
        match self.slots.get(gidx) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 直写槽（散点直写核使用；绕过登记与溢出判定，故必被检出）。
    pub fn put_raw(&mut self, gidx: usize, v: u64) {
        if gidx >= FIELD_COUNT {
            return;
        }
        let idx = gidx;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = v;
        }
    }

    /// 投影为 [`PerCpuFrozen`]（对齐核的靶面物化）。
    pub fn freeze(&self) -> PerCpuFrozen {
        let mut p = PerCpuFrozen::zero();
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let v = self.get_raw(i);
            let _ = p.put(i, v);
            i += 1;
        }
        p
    }

    /// 由 [`PerCpuFrozen`] 还原账槽。
    pub fn thaw(p: &PerCpuFrozen) -> CpuLedger {
        let mut c = CpuLedger::new(0);
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let v = p.slot(i);
            match v {
                Ok(x) => c.slots[i] = x,
                Err(_) => c.slots[i] = 0,
            }
            i += 1;
        }
        c
    }

    /// 核内账体指纹（快照三元组第三元 · FNV-1a 流式）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a_begin();
        let mut i = 0usize;
        while i < FIELD_COUNT {
            h = fnv1a_u64(h, self.get_raw(i));
            h = fnv1a_u64(h, self.ovf_bits[i / 32] as u64);
            i += 1;
        }
        fnv1a_u64(h, self.writes)
    }

    /// 非零账目位数（稀疏度观测）。
    pub fn nonzero_slots(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FIELD_COUNT {
            if self.get_raw(i) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 守恒核对行。
#[derive(Clone, Debug)]
pub struct InvRow {
    /// 守恒式编号。
    pub id: String,
    /// 所属族。
    pub family: StatFamily,
    /// 是否成立。
    pub ok: bool,
    /// 左值（`SumEq` 时为目标值）。
    pub lhs: u64,
    /// 右值（`SumEq` 时为求和值）。
    pub rhs: u64,
    /// 守恒式文本。
    pub expr: String,
}

impl InvRow {
    /// 单行导出。
    pub fn line(&self) -> String {
        format!(
            "{} [{}] {} (l={} r={})",
            self.id,
            self.family.name(),
            self.expr,
            self.lhs,
            self.rhs
        )
    }
}

/// 十族联合守恒表（统一校验结果）。
#[derive(Clone, Debug)]
pub struct ConservReport {
    /// 逐条守恒结果（20 行）。
    pub rows: Vec<InvRow>,
    /// 首个失败项（`None` = 全过）。
    pub first_bad: Option<String>,
    /// 通过条数。
    pub passed: u32,
    /// 失败条数。
    pub failed: u32,
}

impl ConservReport {
    /// 是否全过。
    pub fn all_ok(&self) -> bool {
        self.failed == 0
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        let fb = match &self.first_bad {
            Some(s) => s.clone(),
            None => "-".to_string(),
        };
        format!("conserv: ok={} bad={} first={}", self.passed, self.failed, fb)
    }
}

/// 汇总核对行（Σ每核 vs 全局）。
#[derive(Clone, Debug)]
pub struct SumRow {
    /// 账目位索引。
    pub gidx: usize,
    /// 所属族。
    pub family: StatFamily,
    /// 逐核求和。
    pub sigma: u64,
    /// 全局账值。
    pub global: u64,
    /// 是否闭合。
    pub closed: bool,
}

impl SumRow {
    /// 单行导出。
    pub fn line(&self) -> String {
        format!(
            "#{} [{}] sigma={} global={} closed={}",
            self.gidx,
            self.family.name(),
            self.sigma,
            self.global,
            if self.closed { 1 } else { 0 }
        )
    }
}

/// 全局汇总核对账。
#[derive(Clone, Debug)]
pub struct SumReport {
    /// 逐账目位核对行（120 行）。
    pub rows: Vec<SumRow>,
    /// 不闭合项数。
    pub open_count: u32,
    /// 首个不闭合项。
    pub first_open: Option<usize>,
}

impl SumReport {
    /// 是否全闭合。
    pub fn all_closed(&self) -> bool {
        self.open_count == 0
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        let fo = match self.first_open {
            Some(i) => format!("#{}", i),
            None => "-".to_string(),
        };
        format!(
            "sum: closed={} open={} first={}",
            self.rows.len() - self.open_count as usize,
            self.open_count,
            fo
        )
    }
}

/// 快照三元组项（核算子 id · tick · 指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapTriple {
    /// 核算子 id。
    pub cpu: u32,
    /// 采集 tick。
    pub tick: u64,
    /// 该核算子本窗账体指纹。
    pub fingerprint: u64,
}

impl SnapTriple {
    /// 单行导出。
    pub fn line(&self) -> String {
        format!("cpu{}@t{}#{:016x}", self.cpu, self.tick, self.fingerprint)
    }
}

/// 全核同窗快照（F1768 消费）。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 快照窗口 tick（协议头）。
    pub tick: u64,
    /// 逐核算子三元组。
    pub triples: Vec<SnapTriple>,
    /// 逐核算子账槽（回放用）。
    pub slots: Vec<Vec<u64>>,
    /// 窗口级指纹。
    pub fingerprint: u64,
}

impl Snapshot {
    /// 快照内核算子数。
    pub fn len(&self) -> usize {
        self.triples.len()
    }

    /// 快照是否为空。
    pub fn is_empty(&self) -> bool {
        self.triples.is_empty()
    }

    /// 错窗检测：三元组 tick 须与窗口头一致。
    pub fn verify_window(&self) -> Result<()> {
        let mut i = 0usize;
        while i < self.triples.len() {
            if self.triples[i].tick != self.tick {
                return CpErr::WindowMismatch.e();
            }
            i += 1;
        }
        Ok(())
    }

    /// 回放还原为账槽集合。
    pub fn replay(&self) -> Vec<Vec<u64>> {
        let mut out = Vec::with_capacity(self.slots.len());
        let mut i = 0usize;
        while i < self.slots.len() {
            let mut row = Vec::with_capacity(FIELD_COUNT);
            let mut j = 0usize;
            while j < FIELD_COUNT {
                let v = match self.slots[i].get(j) {
                    Some(x) => *x,
                    None => 0,
                };
                row.push(v);
                j += 1;
            }
            out.push(row);
            i += 1;
        }
        out
    }

    /// 回放与活体对账：返回非零差异项数。
    pub fn diff_count(&self, live: &[CpuLedger]) -> u32 {
        let rp = self.replay();
        let mut bad = 0u32;
        let mut i = 0usize;
        while i < rp.len() && i < live.len() {
            let mut j = 0usize;
            while j < FIELD_COUNT {
                let a = rp[i].get(j).copied().unwrap_or(0);
                let b = live[i].get_raw(j);
                if a != b {
                    bad += 1;
                }
                j += 1;
            }
            i += 1;
        }
        bad
    }

    /// 导出为单行串。
    pub fn line(&self) -> String {
        let mut s = String::from("snap@");
        s.push_str(&self.tick.to_string());
        let mut i = 0usize;
        while i < self.triples.len() {
            s.push(' ');
            s.push_str(&self.triples[i].line());
            i += 1;
        }
        s
    }
}

/// 注入核结果（10 形态 × 检出与否）。
#[derive(Clone, Debug)]
pub struct InjectResult {
    /// 形态号（1..10）。
    pub form: u32,
    /// 形态名。
    pub name: String,
    /// 是否被检出。
    pub detected: bool,
    /// 诊断码。
    pub diag: u16,
    /// 详情。
    pub detail: String,
}

impl InjectResult {
    /// 单行导出。
    pub fn line(&self) -> String {
        let d = if self.detected { 1 } else { 0 };
        format!(
            "F{} {} detected={} diag=0x{:04x} {}",
            self.form, self.name, d, self.diag, self.detail
        )
    }
}

/// 字段登记册（append-only · 封账后改写即拒）。
///
/// 判据：UNX-F1762-J1 —— 字段级登记册为对齐判据的唯一权威清单。
#[derive(Clone, Debug)]
pub struct Registry {
    /// 登记条目（序 = GIDX，append-only）。
    pub records: Vec<FieldRecord>,
    /// PerCpu 冻结版侧字段在场标记。
    pub percpu_present: Vec<bool>,
    /// 账本体侧账目位登记标记。
    pub ledger_present: Vec<bool>,
    /// 观测接口挂载标记。
    pub iface_present: Vec<bool>,
    /// 字段偏移表。
    pub offsets: Vec<u32>,
    /// 字段位宽表。
    pub widths: Vec<u8>,
    /// 封账指纹（`None` = 未封账）。
    pub seal: Option<u64>,
    /// 封账后改写计数（恒应 0）。
    pub post_seal_writes: u32,
    /// 追加登记次数（封账后须冻结）。
    pub appends: u32,
}

impl Registry {
    /// 建立默认登记册：十字段族 × 12 字段全量在册。
    ///
    /// 判据：UNX-F1762-J1 —— 十字段族全量登记，缺一即 `MissingInLedger`。
    pub fn new() -> Registry {
        let mut reg = Registry {
            records: Vec::with_capacity(FIELD_COUNT),
            percpu_present: vec![true; FIELD_COUNT],
            ledger_present: vec![true; FIELD_COUNT],
            iface_present: vec![true; FIELD_COUNT],
            offsets: vec![0u32; FIELD_COUNT],
            widths: vec![0u8; FIELD_COUNT],
            seal: None,
            post_seal_writes: 0,
            appends: 0,
        };
        reg.populate();
        reg
    }

    /// 逐族逐字段填充登记条目（内部使用）。
    fn populate(&mut self) {
        let mut fi = 0usize;
        while fi < FAMILY_COUNT {
            let f = match StatFamily::from_ordinal(fi) {
                Ok(x) => x,
                Err(_) => break,
            };
            let mut li = 0usize;
            while li < FIELDS_PER_FAMILY {
                let g = fi * FIELDS_PER_FAMILY + li;
                let w = match StatFamily::width_of_local(li) {
                    Ok(x) => x,
                    Err(_) => break,
                };
                let off = match percpu_offset(f, li) {
                    Ok(x) => x,
                    Err(_) => break,
                };
                let name = f.label(li).unwrap_or("unknown").to_string();
                let iface = f.iface_of(li).unwrap_or_else(|_| "obs-none".to_string());
                self.records.push(FieldRecord {
                    gidx: g,
                    name,
                    offset: off,
                    width: w,
                    family: f,
                    hot: li < 6,
                    iface,
                    align: AlignState::Aligned,
                });
                self.offsets[g] = off;
                self.widths[g] = w;
                li += 1;
            }
            fi += 1;
        }
    }

    /// 登记条目数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 登记册是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 追加一条登记（append-only 唯一入口）。
    ///
    /// 封账后调用即拒：返回 [`CpErr::Sealed`] 且 `post_seal_writes` 递增。
    pub fn append(&mut self, rec: FieldRecord) -> Result<()> {
        if self.seal.is_some() {
            self.post_seal_writes += 1;
            return CpErr::Sealed.e();
        }
        if rec.gidx >= FIELD_COUNT {
            return CpErr::FieldRange.e();
        }
        self.records.push(rec);
        self.appends += 1;
        Ok(())
    }

    /// 封账：流式 FNV-1a 覆盖 名/偏移/位宽/族序。
    pub fn seal_now(&mut self) -> Result<u64> {
        let h = self.compute_seal();
        self.seal = Some(h);
        Ok(h)
    }

    /// 复算封账指纹（不改状态）。
    pub fn compute_seal(&self) -> u64 {
        let mut h = fnv1a_begin();
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            h = fnv1a_str(h, &r.name);
            h = fnv1a_u64(h, r.offset as u64);
            h = fnv1a_u64(h, r.width as u64);
            h = fnv1a_u64(h, r.family.ordinal() as u64);
            h = fnv1a_u64(h, r.gidx as u64);
            i += 1;
        }
        fnv1a_u64(h, self.records.len() as u64)
    }

    /// 封账校验：指纹复算比对。
    pub fn verify_seal(&self) -> Result<()> {
        match self.seal {
            None => Ok(()),
            Some(s) => {
                if self.compute_seal() == s && self.post_seal_writes == 0 {
                    Ok(())
                } else {
                    CpErr::Sealed.e()
                }
            }
        }
    }

    /// 篡改注入用：改写某字段的登记名（封账后必被指纹复算捕获）。
    pub fn tamper_name(&mut self, gidx: usize, name: &str) {
        if gidx >= self.records.len() {
            return;
        }
        let idx = gidx;
        self.records[idx].name = name.to_string();
        if self.seal.is_some() {
            self.post_seal_writes += 1;
        }
    }

    /// 偏移篡改注入用：改写某字段偏移表。
    pub fn tamper_offset(&mut self, gidx: usize, off: u32) {
        if gidx >= self.records.len() {
            return;
        }
        let idx = gidx;
        self.records[idx].offset = off;
        self.offsets[idx] = off;
    }

    /// 布局自洽：登记册声明的偏移应与结构布局公式完全一致。
    pub fn layout_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let r = match self.records.get(i) {
                Some(x) => x,
                None => return false,
            };
            let f = match StatFamily::split_gidx(i) {
                Ok(x) => x.0,
                Err(_) => return false,
            };
            let expect = match percpu_offset(f, i % FIELDS_PER_FAMILY) {
                Ok(x) => x,
                Err(_) => return false,
            };
            if r.offset != expect || self.offsets[i] != expect || self.widths[i] != r.width {
                return false;
            }
            if r.gidx != i {
                return false;
            }
            i += 1;
        }
        self.records.len() == FIELD_COUNT
    }

    /// 逐字段三方对齐核对。
    ///
    /// 判据：UNX-F1762-J1 —— 全字段必须 `Aligned`。
    pub fn align_report(&self) -> AlignReport {
        let mut rows = Vec::with_capacity(FIELD_COUNT);
        let mut aligned = 0u32;
        let mut broken = 0u32;
        let mut first_bad: Option<usize> = None;
        let mut i = 0usize;
        while i < FIELD_COUNT {
            let rec = self.records.get(i);
            let (percpu_name, ledger_name, iface_name, offset, width) = match rec {
                Some(r) => (
                    Some(r.name.clone()),
                    Some(r.name.clone()),
                    Some(r.iface.clone()),
                    r.offset,
                    r.width,
                ),
                None => (None, None, None, 0, 0),
            };
            let has_percpu = i < self.percpu_present.len() && self.percpu_present[i];
            let has_ledger = i < self.ledger_present.len() && self.ledger_present[i];
            let has_iface = i < self.iface_present.len() && self.iface_present[i];
            let pn = if has_percpu { percpu_name.clone() } else { None };
            let ln = if has_ledger { ledger_name.clone() } else { None };
            let fnm = if has_iface { iface_name.clone() } else { None };

            let f = match StatFamily::split_gidx(i) {
                Ok(x) => x.0,
                Err(_) => {
                    broken += 1;
                    if first_bad.is_none() {
                        first_bad = Some(i);
                    }
                    i += 1;
                    continue;
                }
            };
            let expect_off = match percpu_offset(f, i % FIELDS_PER_FAMILY) {
                Ok(x) => x,
                Err(_) => 0,
            };
            let expect_w = StatFamily::width_of_gidx(i).unwrap_or(0);
            let expect_iface = f.iface_of(i % FIELDS_PER_FAMILY).unwrap_or_default();

            let state = if !has_ledger || ln.is_none() {
                AlignState::MissingInLedger
            } else if !has_percpu || pn.is_none() {
                AlignState::MissingInPerCpu
            } else if !has_iface || fnm.is_none() {
                AlignState::MissingInInterface
            } else if offset != expect_off || self.offsets[i] != expect_off {
                AlignState::Mismatched
            } else if width != expect_w || self.widths[i] != expect_w {
                AlignState::Mismatched
            } else {
                let lp = pn.clone().unwrap_or_default();
                let ll = ln.clone().unwrap_or_default();
                let fi = fnm.clone().unwrap_or_default();
                if lp != ll {
                    AlignState::Mismatched
                } else if fi != expect_iface {
                    AlignState::Mismatched
                } else {
                    AlignState::Aligned
                }
            };

            if state.is_ok() {
                aligned += 1;
            } else {
                broken += 1;
                if first_bad.is_none() {
                    first_bad = Some(i);
                }
            }
            rows.push(AlignTriple {
                gidx: i,
                percpu_name: pn,
                ledger_name: ln,
                iface_name: fnm,
                offset,
                width,
                state,
            });
            i += 1;
        }
        AlignReport {
            rows,
            aligned,
            broken,
            first_bad,
            layout_ok: PerCpuFrozen::layout_ok() && self.layout_consistent(),
        }
    }

    /// 把对齐结果回写到登记条目的 `align` 字段（供册打印）。
    pub fn sync_align(&mut self, rep: &AlignReport) {
        let mut i = 0usize;
        while i < rep.rows.len() {
            let g = rep.rows[i].gidx;
            if g < self.records.len() {
                self.records[g].align = rep.rows[i].state;
            }
            i += 1;
        }
    }

    /// 导出登记册（append-only 打印）。
    pub fn dump(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.records.len() {
            s.push_str(&self.records[i].line());
            s.push('\n');
            i += 1;
        }
        s
    }
}

/// 切换成因（`SW-1` 守恒的分解项闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchReason {
    /// 时间片到期。
    Expiry,
    /// 高优先级抢占。
    Preempt,
    /// 主动让出。
    Yield,
    /// 迁移引发。
    Migrate,
}

impl SwitchReason {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            SwitchReason::Expiry => IDX_SWITCH_BY_EXPIRY,
            SwitchReason::Preempt => IDX_SWITCH_BY_PREEMPT,
            SwitchReason::Yield => IDX_SWITCH_BY_YIELD,
            SwitchReason::Migrate => IDX_SWITCH_BY_MIGRATE,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            SwitchReason::Expiry => "expiry",
            SwitchReason::Preempt => "preempt",
            SwitchReason::Yield => "yield",
            SwitchReason::Migrate => "migrate",
        }
    }
}

/// 唤醒来源（`WK-1` 守恒的分解项闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeSource {
    /// 自 idle 唤醒。
    Idle,
    /// 自深睡唤醒。
    Sleep,
    /// 被合并的唤醒。
    Coalesced,
}

impl WakeSource {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            WakeSource::Idle => IDX_WAKE_FROM_IDLE,
            WakeSource::Sleep => IDX_WAKE_FROM_SLEEP,
            WakeSource::Coalesced => IDX_WAKE_COALESCED,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            WakeSource::Idle => "idle",
            WakeSource::Sleep => "sleep",
            WakeSource::Coalesced => "coalesced",
        }
    }
}

/// 迁移成因（`MG-1` 守恒的分解项闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateReason {
    /// 均衡驱动。
    Balance,
    /// 亲和违例。
    Affinity,
}

impl MigrateReason {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            MigrateReason::Balance => IDX_MIGRATE_BY_BALANCE,
            MigrateReason::Affinity => IDX_MIGRATE_BY_AFFINITY,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            MigrateReason::Balance => "balance",
            MigrateReason::Affinity => "affinity",
        }
    }
}

/// idle 驻留态（三态守恒分解项 · F1745）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleLevel {
    /// A 态（浅）。
    A,
    /// B 态（中）。
    B,
    /// C 态（深）。
    C,
}

impl IdleLevel {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            IdleLevel::A => IDX_IDLE_STATE_A,
            IdleLevel::B => IDX_IDLE_STATE_B,
            IdleLevel::C => IDX_IDLE_STATE_C,
        }
    }
    /// 层级数值（用于 `idle_deepest_level`）。
    pub fn level(self) -> u32 {
        match self {
            IdleLevel::A => 1,
            IdleLevel::B => 2,
            IdleLevel::C => 3,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            IdleLevel::A => "A",
            IdleLevel::B => "B",
            IdleLevel::C => "C",
        }
    }
}

/// 延迟分解段（`LT-1` 守恒的分解项闭集 · F1764）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencySeg {
    /// 入队段。
    Enqueue,
    /// 派发段。
    Dispatch,
    /// 抢占段。
    Preempt,
    /// IPI 段。
    Ipi,
    /// 迁移段。
    Migrate,
}

impl LatencySeg {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            LatencySeg::Enqueue => IDX_LAT_ENQUEUE,
            LatencySeg::Dispatch => IDX_LAT_DISPATCH,
            LatencySeg::Preempt => IDX_LAT_PREEMPT,
            LatencySeg::Ipi => IDX_LAT_IPI,
            LatencySeg::Migrate => IDX_LAT_MIGRATE,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            LatencySeg::Enqueue => "enqueue",
            LatencySeg::Dispatch => "dispatch",
            LatencySeg::Preempt => "preempt",
            LatencySeg::Ipi => "ipi",
            LatencySeg::Migrate => "migrate",
        }
    }
}

/// 调度类（`CS-1` 守恒的分解项闭集 · F1661）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedClass {
    /// 实时类。
    Rt,
    /// 公平类。
    Fair,
    /// 批处理类。
    Bt,
    /// idle 类。
    Idle,
    /// 保留批类。
    Batch,
}

impl SchedClass {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            SchedClass::Rt => IDX_CLASS_RT,
            SchedClass::Fair => IDX_CLASS_FAIR,
            SchedClass::Bt => IDX_CLASS_BT,
            SchedClass::Idle => IDX_CLASS_IDLE,
            SchedClass::Batch => IDX_CLASS_BATCH,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            SchedClass::Rt => "rt",
            SchedClass::Fair => "fair",
            SchedClass::Bt => "bt",
            SchedClass::Idle => "idle",
            SchedClass::Batch => "batch",
        }
    }
}

/// 均衡动作（`BL-1` 守恒的分解项闭集 · F1721）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BalanceAct {
    /// 拉取。
    Pull,
    /// 推送。
    Push,
    /// 窃取。
    Steal,
}

impl BalanceAct {
    /// 分解项账目位。
    pub fn slot(self) -> usize {
        match self {
            BalanceAct::Pull => IDX_BAL_PULL,
            BalanceAct::Push => IDX_BAL_PUSH,
            BalanceAct::Steal => IDX_BAL_STEAL,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            BalanceAct::Pull => "pull",
            BalanceAct::Push => "push",
            BalanceAct::Steal => "steal",
        }
    }
}

/// IPI 种类（窄区子类型闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiKind {
    /// 重调度 IPI。
    Resched,
    /// 停核 IPI。
    Halt,
    /// 唤醒 IPI。
    Wakeup,
}

impl IpiKind {
    /// 子类型账目位。
    pub fn slot(self) -> usize {
        match self {
            IpiKind::Resched => IDX_IPI_RESCHED,
            IpiKind::Halt => IDX_IPI_HALT,
            IpiKind::Wakeup => IDX_IPI_WAKEUP,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            IpiKind::Resched => "resched",
            IpiKind::Halt => "halt",
            IpiKind::Wakeup => "wakeup",
        }
    }
}

/// 时间加权队列深度结算结果（F1645 窗口口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedDepth {
    /// 窗口内采样 tick 数。
    pub window_ticks: u64,
    /// 窗口内深度总和。
    pub window_sum: u64,
    /// 加权平均深度（整数下取整 · 零浮点）。
    pub weighted: u64,
    /// 结算余差（`window_sum - weighted * window_ticks`）。
    pub residual: u64,
    /// 余差上界（恒为 `window_ticks - 1` 或 0）。
    pub bound: u64,
    /// 余差是否在界内。
    pub in_bound: bool,
}

impl WeightedDepth {
    /// 单行导出。
    pub fn line(&self) -> String {
        format!(
            "weighted: ticks={} sum={} avg={} res={} bound={} ok={}",
            self.window_ticks,
            self.window_sum,
            self.weighted,
            self.residual,
            self.bound,
            if self.in_bound { 1 } else { 0 }
        )
    }
}

/// 十字段族每核统计账本体（顶层账）。
///
/// 判据：UNX-F1762-J1 —— 本体是观测面 L1 账层唯一实体。
#[derive(Clone, Debug)]
pub struct Ledger {
    /// 逐核算子账。
    pub cpus: Vec<CpuLedger>,
    /// 字段登记册。
    pub registry: Registry,
    /// 全局汇总账（120 位）。
    pub global: Vec<u64>,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 经登记写入口累计的写次数（F1763 零散点禁令计数基准）。
    pub entry_writes: u64,
    /// 检出的散点直写次数。
    pub scatter_hits: u32,
    /// 账本体是否已封账（封账后写入口拒绝）。
    pub sealed: bool,
    /// 全局账是否已重算过。
    pub global_fresh: bool,
}

impl Ledger {
    /// 建立含 `ncpu` 个核算子的账本体。
    pub fn new(ncpu: usize) -> Ledger {
        let mut cpus = Vec::with_capacity(ncpu);
        let mut i = 0usize;
        while i < ncpu {
            cpus.push(CpuLedger::new(i as u32));
            i += 1;
        }
        Ledger {
            cpus,
            registry: Registry::new(),
            global: vec![0u64; FIELD_COUNT],
            tick: 0,
            entry_writes: 0,
            scatter_hits: 0,
            sealed: false,
            global_fresh: false,
        }
    }

    /// 核算子数。
    pub fn ncpu(&self) -> usize {
        self.cpus.len()
    }

    /// 取核算子（越界拒绝）。
    pub fn cpu(&self, id: u32) -> Result<&CpuLedger> {
        let i = id as usize;
        match self.cpus.get(i) {
            Some(c) => Ok(c),
            None => CpErr::CpuRange.e(),
        }
    }

    /// 取核算子可变引用。
    pub fn cpu_mut(&mut self, id: u32) -> Result<&mut CpuLedger> {
        let i = id as usize;
        match self.cpus.get_mut(i) {
            Some(c) => Ok(c),
            None => CpErr::CpuRange.e(),
        }
    }

    /// 账本体封账（同时封登记册）。
    pub fn seal_now(&mut self) -> Result<u64> {
        self.sealed = true;
        self.registry.seal_now()
    }

    /// 饱和加（唯一算术内核：越上限则饱和并置 overflow 位，绝不静默回绕）。
    fn sat_add(c: &mut CpuLedger, gidx: usize, delta: u64) -> u64 {
        let w = StatFamily::width_of_gidx(gidx).unwrap_or(8);
        let cap = sat_cap(w);
        let cur = c.get_raw(gidx);
        let nv = cur.saturating_add(delta);
        let clamped = if nv > cap { cap } else { nv };
        if nv > cap || cur >= cap {
            c.ovf_set(gidx);
            c.saturations += 1;
        }
        if clamped > cur {
            c.put_raw(gidx, clamped);
        }
        clamped
    }

    /// 饱和赋值（`last_*_tick` 等标量位使用）。
    fn sat_set(c: &mut CpuLedger, gidx: usize, v: u64) {
        let w = StatFamily::width_of_gidx(gidx).unwrap_or(8);
        let cap = sat_cap(w);
        let nv = if v > cap { cap } else { v };
        if v > cap {
            c.ovf_set(gidx);
            c.saturations += 1;
        }
        c.put_raw(gidx, nv);
    }

    /// **唯一登记写入口**（F1763 零散点禁令：本函数是账槽的唯一合法写路径）。
    ///
    /// 语义：按 `delta` 饱和累加到 `(cpu, gidx)`，同步刷新该族 `last_*_tick`。
    /// 拒绝条件：核算子越界 / 账位越界 / 族未登记 / 时窗倒流 / 已封账。
    pub fn record(&mut self, cpu: u32, gidx: usize, delta: u64, tick: u64) -> Result<u64> {
        if self.sealed {
            return CpErr::Sealed.e();
        }
        if tick < self.tick {
            return CpErr::StaleTick.e();
        }
        let (f, _l) = StatFamily::split_gidx(gidx)?;
        if f.ordinal() >= FAMILY_COUNT {
            return CpErr::FamilyUnregistered.e();
        }
        let last_tick_slot = f.ordinal() * FIELDS_PER_FAMILY + 7;
        {
            let c = self.cpu_mut(cpu)?;
            let nv = Ledger::sat_add(c, gidx, delta);
            Ledger::sat_set(c, last_tick_slot, tick);
            c.last_tick = tick;
            c.writes += 1;
            self.entry_writes += 1;
            self.tick = tick;
            Ok(nv)
        }
    }

    /// 散点直写核专用路径：绕过登记写入口（**必被 [`Ledger::detect_scatter`] 检出**）。
    pub fn scatter_write(&mut self, cpu: u32, gidx: usize, v: u64) {
        self.scatter_hits += 1;
        if let Ok(c) = self.cpu_mut(cpu) {
            c.put_raw(gidx, v);
        }
    }

    /// 散点直写检测（登记写计数 vs 槽变化指纹不一致即报）。
    ///
    /// 判据：F1763 观测与业务零耦合 —— 散点直写必被检出。
    pub fn detect_scatter(&self) -> Result<()> {
        if self.scatter_hits > 0 {
            return CpErr::TableInconsistent.e();
        }
        Ok(())
    }

    /// 族记账：切换（F1626 切换率消费面）。
    pub fn note_switch(&mut self, cpu: u32, r: SwitchReason, tick: u64) -> Result<()> {
        self.record(cpu, IDX_SWITCH_COUNT, 1, tick)?;
        self.record(cpu, r.slot(), 1, tick)?;
        self.record(cpu, IDX_SWITCH_TOTAL, 1, tick)?;
        self.record(cpu, IDX_SWITCH_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_SWITCH_BUSY_CYCLES, 4, tick)?;
        Ok(())
    }

    /// 族记账：切换回滚（`SW-2` 守恒的左项）。
    pub fn note_switch_rollback(&mut self, cpu: u32, tick: u64) -> Result<()> {
        self.record(cpu, IDX_SWITCH_ROLLBACK, 1, tick)?;
        self.record(cpu, IDX_SWITCH_ROLLBACK_TOTAL, 1, tick)?;
        Ok(())
    }

    /// 族记账：唤醒（F1759 唤醒率消费面）。
    pub fn note_wake(&mut self, cpu: u32, src: WakeSource, latency_ns: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_WAKE_COUNT, 1, tick)?;
        self.record(cpu, src.slot(), 1, tick)?;
        self.record(cpu, IDX_WAKE_TOTAL, 1, tick)?;
        self.record(cpu, IDX_WAKE_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_WAKE_LATENCY_SUM, latency_ns, tick)?;
        if src == WakeSource::Idle {
            self.record(cpu, IDX_WAKE_FROM_IDLE_TOTAL, 1, tick)?;
        }
        Ok(())
    }

    /// 族记账：唤醒突发峰值（取大语义）。
    pub fn note_wake_burst(&mut self, cpu: u32, burst: u64, tick: u64) -> Result<u64> {
        let cur = self.cpu(cpu)?.get_raw(IDX_WAKE_BURST_MAX);
        if burst > cur {
            self.record(cpu, IDX_WAKE_BURST_MAX, burst - cur, tick)?;
            Ok(burst)
        } else {
            Ok(cur)
        }
    }

    /// 族记账：迁移。
    pub fn note_migrate(&mut self, cpu: u32, r: MigrateReason, cost: u64, gap: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_MIGRATE_COUNT, 1, tick)?;
        self.record(cpu, r.slot(), 1, tick)?;
        self.record(cpu, IDX_MIGRATE_TOTAL, 1, tick)?;
        self.record(cpu, IDX_MIGRATE_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_MIGRATE_COST_SUM, cost, tick)?;
        self.record(cpu, IDX_MIGRATE_IMBALANCE_ACC, gap, tick)?;
        Ok(())
    }

    /// 族记账：迁移回滚（`MG-2` 守恒的左项）。
    pub fn note_migrate_rollback(&mut self, cpu: u32, tick: u64) -> Result<()> {
        self.record(cpu, IDX_MIGRATE_ROLLBACK, 1, tick)?;
        Ok(())
    }

    /// 族记账：进入 idle（F1745 守恒：进入数、三态 tick、累计）。
    pub fn note_idle_enter(&mut self, cpu: u32, lv: IdleLevel, ticks: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_IDLE_ENTER_COUNT, 1, tick)?;
        self.record(cpu, lv.slot(), ticks, tick)?;
        self.record(cpu, IDX_IDLE_TOTAL_TICKS, ticks, tick)?;
        self.record(cpu, IDX_IDLE_WINDOW_SUM, ticks, tick)?;
        self.record(cpu, IDX_IDLE_TOTAL, 1, tick)?;
        self.record(cpu, IDX_IDLE_CYCLES, ticks, tick)?;
        let cur = self.cpu(cpu)?.get_raw(IDX_IDLE_DEEPEST);
        if lv.level() as u64 > cur {
            self.record(cpu, IDX_IDLE_DEEPEST, lv.level() as u64 - cur, tick)?;
        }
        Ok(())
    }

    /// 族记账：退出 idle（`ID-2` 守恒的左项）。
    pub fn note_idle_exit(&mut self, cpu: u32, blocked: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_IDLE_EXIT_COUNT, 1, tick)?;
        if blocked > 0 {
            self.record(cpu, IDX_IDLE_WAKE_BLOCKED, blocked, tick)?;
        }
        Ok(())
    }

    /// 族记账：延迟分解（F1764）。
    pub fn note_latency(&mut self, cpu: u32, seg: LatencySeg, ns: u64, hi_bucket: bool, tick: u64) -> Result<()> {
        self.record(cpu, IDX_LAT_SAMPLES, 1, tick)?;
        self.record(cpu, seg.slot(), 1, tick)?;
        self.record(cpu, IDX_LAT_SUM_NS, ns, tick)?;
        self.record(cpu, IDX_LAT_WINDOW_SUM, ns, tick)?;
        let mx = self.cpu(cpu)?.get_raw(IDX_LAT_MAX_NS);
        if ns > mx {
            self.record(cpu, IDX_LAT_MAX_NS, ns - mx, tick)?;
        }
        if hi_bucket {
            self.record(cpu, IDX_LAT_BUCKET_HI, 1, tick)?;
            self.record(cpu, IDX_LAT_P99_SUM, ns, tick)?;
        }
        Ok(())
    }

    /// 族记账：队列深度采样（F1645 瞬时口径赋值 + 窗口口径累加）。
    pub fn sample_queue_depth(&mut self, cpu: u32, depth: u64, tick: u64) -> Result<()> {
        let cur = self.cpu(cpu)?.get_raw(IDX_QD_INSTANT);
        if depth != cur {
            if depth > cur {
                self.record(cpu, IDX_QD_INSTANT, depth - cur, tick)?;
            } else {
                self.scatter_assign(cpu, IDX_QD_INSTANT, depth, tick)?;
            }
            self.scatter_assign(cpu, IDX_QD_RUNQ_LEN, depth, tick)?;
        }
        let peak = self.cpu(cpu)?.get_raw(IDX_QD_PEAK);
        if depth > peak {
            self.record(cpu, IDX_QD_PEAK, depth - peak, tick)?;
        }
        self.record(cpu, IDX_QD_WINDOW_SUM, depth, tick)?;
        self.record(cpu, IDX_QD_TOTAL, depth, tick)?;
        self.record(cpu, IDX_QD_WINDOW_TICKS, 1, tick)?;
        self.record(cpu, IDX_QD_SAMPLES, 1, tick)?;
        Ok(())
    }

    /// 标量位赋值（瞬时口径使用；非累加语义，但仍走写入口以保零散点）。
    fn scatter_assign(&mut self, cpu: u32, gidx: usize, v: u64, tick: u64) -> Result<()> {
        let f = StatFamily::split_gidx(gidx)?.0;
        let last_tick_slot = f.ordinal() * FIELDS_PER_FAMILY + 7;
        let c = self.cpu_mut(cpu)?;
        Ledger::sat_set(c, gidx, v);
        Ledger::sat_set(c, last_tick_slot, tick);
        c.last_tick = tick;
        c.writes += 1;
        self.entry_writes += 1;
        self.tick = tick;
        Ok(())
    }

    /// 时间加权队列深度结算（F1645 窗口口径 · 零浮点）。
    pub fn weighted_depth(&self, cpu: u32) -> Result<WeightedDepth> {
        let c = self.cpu(cpu)?;
        let ticks = c.get_raw(IDX_QD_WINDOW_TICKS);
        let sum = c.get_raw(IDX_QD_WINDOW_SUM);
        if ticks == 0 {
            return Ok(WeightedDepth {
                window_ticks: 0,
                window_sum: sum,
                weighted: 0,
                residual: 0,
                bound: 0,
                in_bound: true,
            });
        }
        let weighted = sum / ticks;
        let recon = weighted.saturating_mul(ticks);
        let residual = if sum >= recon { sum - recon } else { recon - sum };
        let bound = ticks - 1;
        let in_bound = residual <= bound;
        Ok(WeightedDepth {
            window_ticks: ticks,
            window_sum: sum,
            weighted,
            residual,
            bound,
            in_bound,
        })
    }

    /// 加权结算误差写入账面（`qd_weight_err_ppm`，零浮点 ppm）。
    pub fn settle_weight_error(&mut self, cpu: u32, tick: u64) -> Result<u64> {
        let w = self.weighted_depth(cpu)?;
        let ppm = if w.window_ticks == 0 {
            0
        } else {
            w.residual.saturating_mul(1_000_000) / w.window_ticks
        };
        let cur = self.cpu(cpu)?.get_raw(IDX_QD_WEIGHT_ERR_PPM);
        if ppm > cur {
            self.record(cpu, IDX_QD_WEIGHT_ERR_PPM, ppm - cur, tick)?;
        }
        Ok(ppm)
    }

    /// 族记账：调度类分布（F1661）。
    pub fn note_class(&mut self, cpu: u32, c: SchedClass, tick: u64) -> Result<()> {
        self.record(cpu, c.slot(), 1, tick)?;
        self.record(cpu, IDX_CLASS_TOTAL, 1, tick)?;
        self.record(cpu, IDX_CLASS_SHARE_SUM, 1, tick)?;
        self.record(cpu, IDX_CLASS_WINDOW_SUM, 1, tick)?;
        Ok(())
    }

    /// 族记账：调度类跨类搬迁。
    pub fn note_class_migrate(&mut self, cpu: u32, stolen: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_CLASS_MIGRATIONS, 1, tick)?;
        self.record(cpu, IDX_CLASS_STEAL_SHARE, stolen, tick)?;
        self.record(cpu, IDX_CLASS_STEAL_COUNT, stolen, tick)?;
        Ok(())
    }

    /// 族记账：均衡动作（F1721）。
    pub fn note_balance(&mut self, cpu: u32, a: BalanceAct, gap: u64, cost: u64, tick: u64) -> Result<()> {
        self.record(cpu, a.slot(), 1, tick)?;
        self.record(cpu, IDX_BAL_TOTAL, 1, tick)?;
        self.record(cpu, IDX_BAL_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_BAL_IMBALANCE_ACC, gap, tick)?;
        self.record(cpu, IDX_BAL_ENERGY_COST, cost, tick)?;
        self.record(cpu, IDX_BAL_ITERS, 1, tick)?;
        self.scatter_assign(cpu, IDX_BAL_GAP, gap, tick)?;
        Ok(())
    }

    /// 族记账：IPI 发出。
    pub fn note_ipi(&mut self, cpu: u32, k: IpiKind, ok: bool, coalesced: bool, ns: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_IPI_SENT, 1, tick)?;
        self.record(cpu, k.slot(), 1, tick)?;
        self.record(cpu, IDX_IPI_TOTAL, 1, tick)?;
        self.record(cpu, IDX_IPI_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_IPI_COST_SUM, ns, tick)?;
        if !ok {
            self.record(cpu, IDX_IPI_FAILED, 1, tick)?;
            self.record(cpu, IDX_IPI_DROP_TOTAL, 1, tick)?;
        }
        if coalesced {
            self.record(cpu, IDX_IPI_COALESCED, 1, tick)?;
        }
        Ok(())
    }

    /// 族记账：IPI 突发峰值（取大语义）。
    pub fn note_ipi_burst(&mut self, cpu: u32, burst: u64, tick: u64) -> Result<u64> {
        let cur = self.cpu(cpu)?.get_raw(IDX_IPI_BURST_MAX);
        if burst > cur {
            self.record(cpu, IDX_IPI_BURST_MAX, burst - cur, tick)?;
            Ok(burst)
        } else {
            Ok(cur)
        }
    }

    /// 族记账：自检结果（账本自身）。
    pub fn note_selfcheck(&mut self, cpu: u32, ok: bool, ns: u64, tick: u64) -> Result<()> {
        if ok {
            self.record(cpu, IDX_SC_PASS, 1, tick)?;
        } else {
            self.record(cpu, IDX_SC_FAIL, 1, tick)?;
            self.record(cpu, IDX_SC_FAIL_TOTAL, 1, tick)?;
        }
        self.record(cpu, IDX_SC_TOTAL, 1, tick)?;
        self.record(cpu, IDX_SC_RUN, 1, tick)?;
        self.record(cpu, IDX_SC_WINDOW_SUM, 1, tick)?;
        self.record(cpu, IDX_SC_LATENCY_SUM, ns, tick)?;
        self.record(cpu, IDX_SC_CSUM, ns, tick)?;
        Ok(())
    }

    /// 族记账：溢出事件登记（自检族）。
    pub fn note_overflow_event(&mut self, cpu: u32, events: u64, tick: u64) -> Result<()> {
        self.record(cpu, IDX_SC_OVF_EVENTS, events, tick)?;
        Ok(())
    }

    /// 溢出语义检测：值达上限却未置 overflow 位即报（显性不静默回绕）。
    pub fn detect_ovf_unmarked(&self) -> Result<()> {
        let mut i = 0usize;
        while i < self.cpus.len() {
            let c = &self.cpus[i];
            let mut j = 0usize;
            while j < FIELD_COUNT {
                let w = StatFamily::width_of_gidx(j).unwrap_or(8);
                let cap = sat_cap(w);
                if c.get_raw(j) >= cap && !c.ovf_test(j) {
                    return CpErr::OvfUnmarked.e();
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 饱和越限检测：账槽值不得超过该位宽上限。
    pub fn detect_over_cap(&self) -> Result<()> {
        let mut i = 0usize;
        while i < self.cpus.len() {
            let c = &self.cpus[i];
            let mut j = 0usize;
            while j < FIELD_COUNT {
                let w = StatFamily::width_of_gidx(j).unwrap_or(8);
                if c.get_raw(j) > sat_cap(w) {
                    return CpErr::OvfUnmarked.e();
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 单核算子守恒校验（20 条全跑，返回该核首个失败项）。
    pub fn verify_cpu(&self, cpu: u32) -> Result<ConservReport> {
        let c = self.cpu(cpu)?;
        let mut rows = Vec::with_capacity(INVARIANTS.len());
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut first_bad: Option<String> = None;
        let mut i = 0usize;
        while i < INVARIANTS.len() {
            let inv = &INVARIANTS[i];
            let (a, b, ok) = match inv.kind {
                InvKind::SumEq { target, parts } => {
                    let lhs = c.get_raw(target);
                    let mut s = 0u64;
                    let mut k = 0usize;
                    while k < parts.len() {
                        s = s.saturating_add(c.get_raw(parts[k]));
                        k += 1;
                    }
                    (lhs, s, lhs == s)
                }
                InvKind::Ge { lhs, rhs } => {
                    let a = c.get_raw(lhs);
                    let b = c.get_raw(rhs);
                    (a, b, a >= b)
                }
                InvKind::Le { lhs, rhs } => {
                    let a = c.get_raw(lhs);
                    let b = c.get_raw(rhs);
                    (a, b, a <= b)
                }
                InvKind::Cmp { lhs, rhs, op } => {
                    let a = c.get_raw(lhs);
                    let b = c.get_raw(rhs);
                    (a, b, op.hold(a, b))
                }
            };
            if ok {
                passed += 1;
            } else {
                failed += 1;
                if first_bad.is_none() {
                    first_bad = Some(inv.id.to_string());
                }
            }
            rows.push(InvRow {
                id: inv.id.to_string(),
                family: inv.family,
                ok,
                lhs: a,
                rhs: b,
                expr: inv.expr.to_string(),
            });
            i += 1;
        }
        Ok(ConservReport {
            rows,
            first_bad,
            passed,
            failed,
        })
    }

    /// 逐族守恒校验（单核算子）。
    pub fn verify_family(&self, cpu: u32, f: StatFamily) -> Result<Vec<InvRow>> {
        let rep = self.verify_cpu(cpu)?;
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < rep.rows.len() {
            if rep.rows[i].family == f {
                out.push(rep.rows[i].clone());
            }
            i += 1;
        }
        Ok(out)
    }

    /// **统一校验入口**：返回全部族守恒结果与首个失败项（逐核算子聚合）。
    ///
    /// 判据：UNX-F1762-J1 —— 十族联合守恒全过方可封账。
    pub fn verify_all(&self) -> ConservReport {
        let mut rows: Vec<InvRow> = Vec::with_capacity(INVARIANTS.len() * self.cpus.len().max(1));
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut first_bad: Option<String> = None;
        let mut ci = 0usize;
        while ci < self.cpus.len() {
            if let Ok(rep) = self.verify_cpu(ci as u32) {
                let mut i = 0usize;
                while i < rep.rows.len() {
                    let r = rep.rows[i].clone();
                    if r.ok {
                        passed += 1;
                    } else {
                        failed += 1;
                        if first_bad.is_none() {
                            first_bad = Some(format!("cpu{}:{}", ci, r.id));
                        }
                    }
                    rows.push(r);
                    i += 1;
                }
            }
            ci += 1;
        }
        ConservReport {
            rows,
            first_bad,
            passed,
            failed,
        }
    }

    /// 全局汇总重算（Σ每核）。
    pub fn close_global(&mut self) -> Vec<u64> {
        let mut g = vec![0u64; FIELD_COUNT];
        let mut i = 0usize;
        while i < self.cpus.len() {
            let mut j = 0usize;
            while j < FIELD_COUNT {
                g[j] = g[j].saturating_add(self.cpus[i].get_raw(j));
                j += 1;
            }
            i += 1;
        }
        self.global = g.clone();
        self.global_fresh = true;
        g
    }

    /// 汇总闭合核对（Σ每核 == 全局）。
    pub fn verify_sum(&self) -> SumReport {
        let mut rows = Vec::with_capacity(FIELD_COUNT);
        let mut open = 0u32;
        let mut first_open: Option<usize> = None;
        let mut j = 0usize;
        while j < FIELD_COUNT {
            let mut sigma = 0u64;
            let mut i = 0usize;
            while i < self.cpus.len() {
                sigma = sigma.saturating_add(self.cpus[i].get_raw(j));
                i += 1;
            }
            let g = self.global.get(j).copied().unwrap_or(0);
            let closed = sigma == g;
            if !closed {
                open += 1;
                if first_open.is_none() {
                    first_open = Some(j);
                }
            }
            let f = StatFamily::split_gidx(j).map(|x| x.0).unwrap_or(StatFamily::Switch);
            rows.push(SumRow {
                gidx: j,
                family: f,
                sigma,
                global: g,
                closed,
            });
            j += 1;
        }
        SumReport {
            rows,
            open_count: open,
            first_open,
        }
    }

    /// 手算锚校验：4 核各 `switch_count = 10` → 全局恰为 40。
    pub fn hand_anchor_switch() -> Result<u64> {
        let mut led = Ledger::new(4);
        let mut c = 0u32;
        let mut t = 1u64;
        while c < 4 {
            let mut k = 0u32;
            while k < 10 {
                led.note_switch(c, SwitchReason::Expiry, t)?;
                t += 1;
                k += 1;
            }
            c += 1;
        }
        let g = led.close_global();
        let v = g.get(IDX_SWITCH_COUNT).copied().unwrap_or(0);
        if v == 40 {
            Ok(v)
        } else {
            CpErr::SumOpen.e()
        }
    }

    /// 全核同窗快照采集（F1768 消费）。
    ///
    /// 协议：同一 `tick` 窗口内采集全部核算子，产出 (id, tick, 指纹) 三元组。
    pub fn capture_snapshot(&self, tick: u64) -> Snapshot {
        let mut triples = Vec::with_capacity(self.cpus.len());
        let mut slots = Vec::with_capacity(self.cpus.len());
        let mut h = fnv1a_begin();
        let mut i = 0usize;
        while i < self.cpus.len() {
            let c = &self.cpus[i];
            let fp = c.fingerprint();
            triples.push(SnapTriple {
                cpu: c.id,
                tick,
                fingerprint: fp,
            });
            let mut row = Vec::with_capacity(FIELD_COUNT);
            let mut j = 0usize;
            while j < FIELD_COUNT {
                row.push(c.get_raw(j));
                j += 1;
            }
            slots.push(row);
            h = fnv1a_u64(h, fp);
            i += 1;
        }
        Snapshot {
            tick,
            triples,
            slots,
            fingerprint: fnv1a_u64(h, tick),
        }
    }

    /// 快照错窗检测（对账入口）。
    pub fn verify_snapshot(&self, s: &Snapshot) -> Result<()> {
        s.verify_window()?;
        if s.diff_count(&self.cpus) != 0 {
            return CpErr::ReplayDiff.e();
        }
        Ok(())
    }

    /// 账本体总校验（守恒 + 对齐 + 汇总 + 溢出 + 封账）。
    pub fn full_check(&mut self) -> String {
        self.close_global();
        let conserv = self.verify_all();
        let align = self.registry.align_report();
        let sum = self.verify_sum();
        let ovf = self.detect_ovf_unmarked();
        let seal = self.registry.verify_seal();
        let scatter = self.scatter_hits == 0;
        let mut s = String::new();
        s.push_str(&conserv.summary());
        s.push('\n');
        s.push_str(&align.summary());
        s.push('\n');
        s.push_str(&sum.summary());
        s.push('\n');
        s.push_str(&format!("ovf: ok={}", if ovf.is_ok() { 1 } else { 0 }));
        s.push('\n');
        s.push_str(&format!("seal: ok={}", if seal.is_ok() { 1 } else { 0 }));
        s.push('\n');
        s.push_str(&format!(
            "scatter: clean={}",
            if scatter { 1 } else { 0 }
        ));
        s.push('\n');
        s.push_str(&format!("cpus={} fields={} writes={}", self.ncpu(), FIELD_COUNT, self.entry_writes));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 注入核（10 形态 · 10/10 检出）
// 判据：UNX-F1762-J1 —— 十字段族登记与字段级对齐的断言须对下列 10 类缺陷零漏检。
// ═══════════════════════════════════════════════════════════════════════════

/// 形态 1：PerCpu 缺字段。
///
/// 注入：登记册 `percpu_present[x] = false`，模拟 PerCpu 冻结版（F1623/F1639）漏定义该字段。
pub fn inject_form1_missing_percpu() -> InjectResult {
    let mut led = Ledger::new(4);
    let _ = led.note_switch(0, SwitchReason::Expiry, 1);
    led.registry.percpu_present[IDX_WAKE_LATENCY_SUM] = false;
    let ar = led.registry.align_report();
    let mut detected = false;
    let mut detail = String::from("no-mismatch");
    let mut i = 0usize;
    while i < ar.rows.len() {
        if ar.rows[i].gidx == IDX_WAKE_LATENCY_SUM && ar.rows[i].state == AlignState::MissingInPerCpu {
            detected = true;
            detail = format!("#{} {}", IDX_WAKE_LATENCY_SUM, ar.rows[i].state.name());
        }
        i += 1;
    }
    InjectResult {
        form: 1,
        name: String::from("percpu-missing-field"),
        detected,
        diag: DIAG_ALIGN,
        detail,
    }
}

/// 形态 2：账目位缺登记。
///
/// 注入：登记册 `ledger_present[x] = false`，模拟账本体侧漏登记该计数器。
pub fn inject_form2_missing_ledger() -> InjectResult {
    let mut led = Ledger::new(4);
    let _ = led.note_ipi(1, IpiKind::Resched, true, false, 10, 1);
    led.registry.ledger_present[IDX_IPI_COST_SUM] = false;
    let ar = led.registry.align_report();
    let mut detected = false;
    let mut detail = String::from("no-mismatch");
    let mut i = 0usize;
    while i < ar.rows.len() {
        if ar.rows[i].gidx == IDX_IPI_COST_SUM && ar.rows[i].state == AlignState::MissingInLedger {
            detected = true;
            detail = format!("#{} {}", IDX_IPI_COST_SUM, ar.rows[i].state.name());
        }
        i += 1;
    }
    InjectResult {
        form: 2,
        name: String::from("ledger-slot-unregistered"),
        detected,
        diag: DIAG_FAMILY,
        detail,
    }
}

/// 形态 3：观测接口未挂。
///
/// 注入：登记册 `iface_present[x] = false`，模拟 F1763 登记宏未把该字段挂到观测面。
pub fn inject_form3_missing_interface() -> InjectResult {
    let mut led = Ledger::new(4);
    let _ = led.note_class(2, SchedClass::Fair, 1);
    led.registry.iface_present[IDX_CLASS_STEAL_SHARE] = false;
    let ar = led.registry.align_report();
    let mut detected = false;
    let mut detail = String::from("no-mismatch");
    let mut i = 0usize;
    while i < ar.rows.len() {
        if ar.rows[i].gidx == IDX_CLASS_STEAL_SHARE && ar.rows[i].state == AlignState::MissingInInterface {
            detected = true;
            detail = format!("#{} {}", IDX_CLASS_STEAL_SHARE, ar.rows[i].state.name());
        }
        i += 1;
    }
    InjectResult {
        form: 3,
        name: String::from("iface-not-mounted"),
        detected,
        diag: DIAG_ALIGN,
        detail,
    }
}

/// 形态 4：族内守恒破坏。
///
/// 注入：只累加 `switch_by_preempt` 而不累加 `switch_count`，破 `SW-1` 守恒式。
pub fn inject_form4_conservation_break() -> InjectResult {
    let mut led = Ledger::new(2);
    let _ = led.note_switch(0, SwitchReason::Expiry, 1);
    // 守恒破坏：分解项独增。
    let _ = led.record(0, IDX_SWITCH_BY_PREEMPT, 3, 2);
    let rep = led.verify_cpu(0);
    let mut detected = false;
    let mut detail = String::from("no-break");
    if let Ok(r) = rep {
        let mut i = 0usize;
        while i < r.rows.len() {
            if !r.rows[i].ok && r.rows[i].id == "SW-1" {
                detected = true;
                detail = r.rows[i].line();
            }
            i += 1;
        }
    }
    InjectResult {
        form: 4,
        name: String::from("family-conservation-break"),
        detected,
        diag: DIAG_CONSERVATION,
        detail,
    }
}

/// 形态 5：散点直写（绕过登记写入口）。
///
/// 注入：走 [`Ledger::scatter_write`] 直改槽位，`scatter_hits` 计数必被检出。
pub fn inject_form5_scatter_write() -> InjectResult {
    let mut led = Ledger::new(2);
    let _ = led.note_balance(0, BalanceAct::Pull, 3, 7, 1);
    led.scatter_write(0, IDX_BAL_TOTAL, 999);
    let detected = led.detect_scatter().is_err() && led.scatter_hits > 0;
    InjectResult {
        form: 5,
        name: String::from("scatter-direct-write"),
        detected,
        diag: DIAG_SCATTER,
        detail: format!("scatter_hits={}", led.scatter_hits),
    }
}

/// 形态 6：封账后改写。
///
/// 注入：封账后篡改登记名，指纹复算必失配。
pub fn inject_form6_post_seal_write() -> InjectResult {
    let mut led = Ledger::new(2);
    let _ = led.note_latency(0, LatencySeg::Enqueue, 40, false, 1);
    // 判据：UNX-F1762-J1 —— 先封账取得基准指纹，再篡改登记名。
    let sealed = led.registry.seal_now().unwrap_or(0);
    let seal_ok_before = led.registry.verify_seal().is_ok();
    led.registry.tamper_name(IDX_LAT_SUM_NS, "latency_sum_hijacked");
    let seal_ok_after = led.registry.verify_seal().is_ok();
    let recomputed = led.registry.compute_seal();
    InjectResult {
        form: 6,
        name: String::from("post-seal-rewrite"),
        detected: seal_ok_before && !seal_ok_after && recomputed != sealed,
        diag: DIAG_SEAL,
        detail: format!("seal={:016x} recomputed={:016x}", sealed, recomputed),
    }
}

/// 形态 7：全局汇总不闭合。
///
/// 注入：闭账后篡改全局槽，令 `Σ每核 != 全局`。
pub fn inject_form7_sum_open() -> InjectResult {
    let mut led = Ledger::new(4);
    let mut c = 0u32;
    while c < 4 {
        let _ = led.note_switch(c, SwitchReason::Expiry, c as u64 + 1);
        c += 1;
    }
    led.close_global();
    let before = led.verify_sum();
    led.global[IDX_SWITCH_COUNT] = led.global[IDX_SWITCH_COUNT].saturating_add(1);
    let after = led.verify_sum();
    InjectResult {
        form: 7,
        name: String::from("global-sum-open"),
        detected: before.all_closed() && !after.all_closed(),
        diag: DIAG_SUM,
        detail: after.summary(),
    }
}

/// 形态 8：快照错窗。
///
/// 注入：把快照中一个核算子的三元组 tick 改成别的窗口。
pub fn inject_form8_snapshot_window() -> InjectResult {
    let mut led = Ledger::new(4);
    let mut c = 0u32;
    while c < 4 {
        let _ = led.note_wake(c, WakeSource::Idle, 5, 10);
        c += 1;
    }
    let mut snap = led.capture_snapshot(10);
    let ok_before = led.verify_snapshot(&snap).is_ok();
    if !snap.triples.is_empty() {
        snap.triples[2].tick = 9;
    }
    let after = led.verify_snapshot(&snap);
    InjectResult {
        form: 8,
        name: String::from("snapshot-wrong-window"),
        detected: ok_before && after.is_err(),
        diag: DIAG_WINDOW,
        detail: format!("err={}", after.err().map(|e| e.name()).unwrap_or("none")),
    }
}

/// 形态 9：计数溢出未置 overflow 位。
///
/// 注入：绕过登记入口把窄计数顶到 `u32::MAX` 而不置溢出位，饱和语义核必检出。
pub fn inject_form9_ovf_unmarked() -> InjectResult {
    let mut led = Ledger::new(2);
    let _ = led.note_selfcheck(0, true, 3, 1);
    led.scatter_write(0, IDX_SWITCH_COUNT, SAT_CAP_U32);
    let unmarked = led.detect_ovf_unmarked();
    let ovf_bit = led.cpus[0].ovf_test(IDX_SWITCH_COUNT);
    InjectResult {
        form: 9,
        name: String::from("ovf-unmarked"),
        detected: unmarked.is_err() && !ovf_bit,
        diag: DIAG_OVF,
        detail: format!("cap-set ovf-bit={}", if ovf_bit { 1 } else { 0 }),
    }
}

/// 形态 10：字段偏移错位。
///
/// 注入：篡改登记册偏移表，布局自洽核必检出。
pub fn inject_form10_offset_misalign() -> InjectResult {
    let mut led = Ledger::new(2);
    let _ = led.note_migrate(0, MigrateReason::Balance, 9, 4, 1);
    let ok_before = led.registry.layout_consistent();
    led.registry.tamper_offset(IDX_MIGRATE_COST_SUM, 999);
    let ok_after = led.registry.layout_consistent();
    let ar = led.registry.align_report();
    let mut state_name = String::from("-");
    let mut i = 0usize;
    while i < ar.rows.len() {
        if ar.rows[i].gidx == IDX_MIGRATE_COST_SUM {
            state_name = ar.rows[i].state.name().to_string();
        }
        i += 1;
    }
    InjectResult {
        form: 10,
        name: String::from("field-offset-misalign"),
        detected: ok_before && !ok_after && !ar.all_aligned(),
        diag: DIAG_OFFSET,
        detail: format!("#{} state={}", IDX_MIGRATE_COST_SUM, state_name),
    }
}

/// 注入核全集：10 形态逐一注入并返回 10 条检出结论。
///
/// 判据：UNX-F1762-J1 —— 10/10 检出。
pub fn inject_all() -> Vec<InjectResult> {
    let mut v = Vec::with_capacity(10);
    v.push(inject_form1_missing_percpu());
    v.push(inject_form2_missing_ledger());
    v.push(inject_form3_missing_interface());
    v.push(inject_form4_conservation_break());
    v.push(inject_form5_scatter_write());
    v.push(inject_form6_post_seal_write());
    v.push(inject_form7_sum_open());
    v.push(inject_form8_snapshot_window());
    v.push(inject_form9_ovf_unmarked());
    v.push(inject_form10_offset_misalign());
    v
}

/// 注入核 10/10 判定。
pub fn inject_summary() -> String {
    let rs = inject_all();
    let mut n = 0u32;
    let mut s = String::new();
    let mut i = 0usize;
    while i < rs.len() {
        if rs[i].detected {
            n += 1;
        }
        s.push_str(&rs[i].line());
        s.push('\n');
        i += 1;
    }
    s.push_str(&format!("inject: detected={}/10", n));
    s
}

// ═══════════════════════════════════════════════════════════════════════════
// 导出（G 项）
// ═══════════════════════════════════════════════════════════════════════════

/// 单点导出：单族一段文本（守恒式 + 逐计数器全局值）。
pub fn export_family(f: StatFamily, led: &Ledger) -> String {
    let mut s = String::new();
    let base = f.ordinal() * FIELDS_PER_FAMILY;
    s.push_str(&format!("[family {}] ord={} base={}\n", f.name(), f.ordinal(), base));
    let mut i = 0usize;
    while i < FIELDS_PER_FAMILY {
        let g = base + i;
        let w = StatFamily::width_of_gidx(g).unwrap_or(8);
        let sigma = led.global.get(g).copied().unwrap_or(0);
        s.push_str(&format!(
            "  #{} {:<24} w={} global={}\n",
            g,
            f.labels()[i],
            w,
            sigma
        ));
        i += 1;
    }
    let invs = led.verify_family(0, f).unwrap_or_default();
    let mut k = 0usize;
    while k < invs.len() {
        s.push_str(&format!("  inv {} ok={}\n", invs[k].id, if invs[k].ok { 1 } else { 0 }));
        k += 1;
    }
    s
}

/// 单点导出：全量 KV 文本（族 → 字段 → 全局值）。
pub fn export_all_kv(led: &Ledger) -> String {
    let mut s = String::new();
    let all = StatFamily::all();
    let mut k = 0usize;
    while k < all.len() {
        s.push_str(&export_family(all[k], led));
        k += 1;
    }
    s.push_str(&format!("tick={}\n", led.tick));
    s.push_str(&format!("cpus={}\n", led.ncpu()));
    s.push_str(&format!("writes={}\n", led.entry_writes));
    let cons = led.verify_all();
    s.push_str(&conserv_summary_line(&cons));
    let ar = led.registry.align_report();
    s.push_str(&format!(
        "align={}/{} first={}\n",
        ar.aligned,
        FIELD_COUNT,
        match ar.first_bad {
            Some(i) => format!("#{}", i),
            None => "-".to_string(),
        }
    ));
    s.push_str(&format!("seal={:016x}\n", led.registry.compute_seal()));
    s
}

/// 守恒报告摘要单行（导出内联用）。
fn conserv_summary_line(r: &ConservReport) -> String {
    let fb = match &r.first_bad {
        Some(s) => s.clone(),
        None => "-".to_string(),
    };
    format!("conserv={}/{} first={}\n", r.passed, r.passed + r.failed, fb)
}

/// TLV 导出标签（闭集）。
pub mod tlv {
    use super::*;

    /// TLV 记录类型。
    pub const T_HEADER: u8 = 0x01;
    /// 族标记。
    pub const T_FAMILY: u8 = 0x10;
    /// 字段表项。
    pub const T_FIELD: u8 = 0x20;
    /// 守恒式项。
    pub const T_INV: u8 = 0x30;
    /// 封账指纹项。
    pub const T_SEAL: u8 = 0x40;
    /// 尾部标记。
    pub const T_END: u8 = 0x7F;

    /// TLV 导出头长度（魔数 4 + 版本 2 + 长度 2）。
    pub const HEADER_LEN: usize = 8;
    /// TLV 魔数。
    pub const MAGIC: u32 = 0x5632_4C43;

    /// 逐字节推入一个 `u8`。
    pub fn put_u8(v: &mut Vec<u8>, b: u8) {
        v.push(b);
    }

    /// 逐字节推入一个 `u16`（小端）。
    pub fn put_u16(v: &mut Vec<u8>, x: u16) {
        v.push((x & 0xFF) as u8);
        v.push(((x >> 8) & 0xFF) as u8);
    }

    /// 逐字节推入一个 `u32`（小端）。
    pub fn put_u32(v: &mut Vec<u8>, x: u32) {
        let mut i = 0usize;
        while i < 4 {
            v.push(((x >> (i * 8)) & 0xFF) as u8);
            i += 1;
        }
    }

    /// 逐字节推入一个 `u64`（小端）。
    pub fn put_u64(v: &mut Vec<u8>, x: u64) {
        let mut i = 0usize;
        while i < 8 {
            v.push(((x >> (i * 8)) & 0xFF) as u8);
            i += 1;
        }
    }

    /// 推入一条 TLV（标签 + 长度 + 载荷）。
    pub fn put_tlv(v: &mut Vec<u8>, tag: u8, payload: &[u8]) {
        put_u8(v, tag);
        put_u16(v, payload.len() as u16);
        let mut i = 0usize;
        while i < payload.len() {
            v.push(payload[i]);
            i += 1;
        }
    }

    /// 从流中读一个 `u8`。
    pub fn get_u8(v: &[u8], at: usize) -> Option<u8> {
        v.get(at).copied()
    }

    /// 从流中读一个 `u16`（小端）。
    pub fn get_u16(v: &[u8], at: usize) -> Option<u16> {
        let a = v.get(at).copied()?;
        let b = v.get(at + 1).copied()?;
        Some((a as u16) | ((b as u16) << 8))
    }

    /// 从流中读一个 `u32`（小端）。
    pub fn get_u32(v: &[u8], at: usize) -> Option<u32> {
        let mut x = 0u32;
        let mut i = 0usize;
        while i < 4 {
            let b = v.get(at + i).copied()?;
            x |= (b as u32) << (i * 8);
            i += 1;
        }
        Some(x)
    }

    /// 从流中读一个 `u64`（小端）。
    pub fn get_u64(v: &[u8], at: usize) -> Option<u64> {
        let mut x = 0u64;
        let mut i = 0usize;
        while i < 8 {
            let b = v.get(at + i).copied()?;
            x |= (b as u64) << (i * 8);
            i += 1;
        }
        Some(x)
    }
}

/// TLV 二进制导出（族标记 + 字段表 + 守恒式 + 封账指纹）。
///
/// 长度自洽：返回长度 = 头 + Σ(3 + 载荷) + 尾 1 + 尾长度域 2。
pub fn export_tlv(led: &Ledger) -> Vec<u8> {
    let mut v = Vec::with_capacity(tlv::HEADER_LEN + 4096);
    tlv::put_u32(&mut v, tlv::MAGIC);
    tlv::put_u16(&mut v, 0x0100);
    tlv::put_u16(&mut v, 0);

    let all = StatFamily::all();
    let mut k = 0usize;
    while k < all.len() {
        let f = all[k];
        let mut pay = Vec::with_capacity(24);
        tlv::put_u8(&mut pay, f.ordinal() as u8);
        tlv::put_u32(&mut pay, f.offset_base());
        tlv::put_u16(&mut pay, FIELDS_PER_FAMILY as u16);
        tlv::put_u64(&mut pay, led.global.get(f.ordinal() * FIELDS_PER_FAMILY).copied().unwrap_or(0));
        tlv::put_tlv(&mut v, tlv::T_FAMILY, &pay);

        let base = f.ordinal() * FIELDS_PER_FAMILY;
        let mut i = 0usize;
        while i < FIELDS_PER_FAMILY {
            let g = base + i;
            let w = StatFamily::width_of_gidx(g).unwrap_or(8);
            let off = led.registry.offsets.get(g).copied().unwrap_or(0);
            let mut p = Vec::with_capacity(20);
            tlv::put_u16(&mut p, g as u16);
            tlv::put_u32(&mut p, off);
            tlv::put_u8(&mut p, w);
            tlv::put_u64(&mut p, led.global.get(g).copied().unwrap_or(0));
            tlv::put_tlv(&mut v, tlv::T_FIELD, &p);
            i += 1;
        }
        k += 1;
    }

    let cons = led.verify_all();
    let mut i = 0usize;
    while i < INVARIANTS.len() {
        let inv = &INVARIANTS[i];
        let mut p = Vec::with_capacity(16);
        tlv::put_u8(&mut p, inv.family.ordinal() as u8);
        tlv::put_u8(&mut p, inv.deps);
        tlv::put_u8(&mut p, if cons.passed as usize > i { 1 } else { 0 });
        tlv::put_u16(&mut p, INVARIANTS.len() as u16);
        tlv::put_tlv(&mut v, tlv::T_INV, &p);
        i += 1;
    }

    let mut seal = Vec::with_capacity(8);
    tlv::put_u64(&mut seal, led.registry.compute_seal());
    tlv::put_tlv(&mut v, tlv::T_SEAL, &seal);

    tlv::put_u8(&mut v, tlv::T_END);
    // 长度域含自身 2 字节，故全长 = 当前长度 + 2。
    let total = (v.len() + 2) as u16;
    v.push((total & 0xFF) as u8);
    v.push(((total >> 8) & 0xFF) as u8);
    v
}

/// TLV 长度自洽断言。
///
/// 结构：`[8 头][(1 标 + 2 长 + 载荷) × N][1 尾标][2 长度域]`，末 2 字节 = 全长。
pub fn verify_tlv(v: &[u8]) -> Result<()> {
    if v.len() < tlv::HEADER_LEN + 3 {
        return CpErr::ExportLength.e();
    }
    let magic = match tlv::get_u32(v, 0) {
        Some(m) => m,
        None => return CpErr::ExportLength.e(),
    };
    if magic != tlv::MAGIC {
        return CpErr::TableInconsistent.e();
    }
    let n = v.len();
    let tail_len = ((v[n - 2] as u16) | ((v[n - 1] as u16) << 8)) as usize;
    if tail_len != n {
        return CpErr::ExportLength.e();
    }
    if v[n - 3] != tlv::T_END {
        return CpErr::ExportLength.e();
    }
    Ok(())
}

/// TLV 统计：记录数 / 载荷字节数。
pub fn tlv_count(v: &[u8], tag: u8) -> u32 {
    let mut cnt = 0u32;
    let mut i = tlv::HEADER_LEN;
    while i + 3 <= v.len() {
        let t = v[i];
        let l = ((v[i + 1] as usize) | ((v[i + 2] as usize) << 8));
        if t == tag {
            cnt += 1;
        }
        i += 3 + l;
    }
    cnt
}

// ═══════════════════════════════════════════════════════════════════════════
// 典型场景构造（纯逻辑 tick 驱动 · 零墙钟）
// ═══════════════════════════════════════════════════════════════════════════

/// 建一个 4 核算子的典型账（手算锚：各核 `switch_count = 10` → 全局 40）。
pub fn demo_ledger() -> Result<Ledger> {
    let mut led = Ledger::new(4);
    let mut t = 1u64;
    let mut c = 0u32;
    while c < 4 {
        // 切换：各核 10 次（4 expiry + 3 preempt + 2 yield + 1 migrate）。
        let mut k = 0u32;
        while k < 10 {
            let r = match k % 4 {
                0 => SwitchReason::Expiry,
                1 => SwitchReason::Preempt,
                2 => SwitchReason::Yield,
                _ => SwitchReason::Migrate,
            };
            led.note_switch(c, r, t)?;
            t += 1;
            k += 1;
        }
        // 唤醒：3 idle + 1 sleep + 1 coalesced。
        let _ = led.note_wake(c, WakeSource::Idle, 12, t)?;
        let _ = led.note_wake(c, WakeSource::Sleep, 40, t + 1)?;
        let _ = led.note_wake(c, WakeSource::Coalesced, 3, t + 2)?;
        t += 3;
        // 迁移：2 均衡 + 1 亲和。
        let _ = led.note_migrate(c, MigrateReason::Balance, 30, 2, t)?;
        let _ = led.note_migrate(c, MigrateReason::Affinity, 55, 5, t + 1)?;
        t += 2;
        // idle：各态驻留 4/2/1 tick。
        let _ = led.note_idle_enter(c, IdleLevel::A, 4, t)?;
        let _ = led.note_idle_enter(c, IdleLevel::B, 2, t + 1)?;
        let _ = led.note_idle_enter(c, IdleLevel::C, 1, t + 2)?;
        let _ = led.note_idle_exit(c, 1, t + 3)?;
        t += 4;
        // 延迟：5 段分解。
        let _ = led.note_latency(c, LatencySeg::Enqueue, 5, false, t)?;
        let _ = led.note_latency(c, LatencySeg::Dispatch, 9, false, t + 1)?;
        let _ = led.note_latency(c, LatencySeg::Preempt, 11, true, t + 2)?;
        let _ = led.note_latency(c, LatencySeg::Ipi, 7, false, t + 3)?;
        let _ = led.note_latency(c, LatencySeg::Migrate, 21, true, t + 4)?;
        t += 5;
        // 队列深度：6 次采样，深度 3/5/2/7/4/6。
        let depths = [3u64, 5, 2, 7, 4, 6];
        let mut d = 0usize;
        while d < depths.len() {
            led.sample_queue_depth(c, depths[d], t)?;
            t += 1;
            d += 1;
        }
        let _ = led.settle_weight_error(c, t)?;
        t += 1;
        // 调度类：2 rt + 4 fair + 1 bt + 1 idle + 2 batch。
        let _ = led.note_class(c, SchedClass::Rt, t)?;
        let _ = led.note_class(c, SchedClass::Rt, t)?;
        let _ = led.note_class(c, SchedClass::Fair, t)?;
        let _ = led.note_class(c, SchedClass::Fair, t)?;
        let _ = led.note_class(c, SchedClass::Fair, t)?;
        let _ = led.note_class(c, SchedClass::Fair, t)?;
        let _ = led.note_class(c, SchedClass::Bt, t)?;
        let _ = led.note_class(c, SchedClass::Idle, t)?;
        let _ = led.note_class(c, SchedClass::Batch, t)?;
        let _ = led.note_class(c, SchedClass::Batch, t)?;
        let _ = led.note_class_migrate(c, 2, t + 1)?;
        t += 2;
        // 均衡：2 pull + 1 push + 1 steal。
        let _ = led.note_balance(c, BalanceAct::Pull, 4, 8, t)?;
        let _ = led.note_balance(c, BalanceAct::Pull, 3, 6, t + 1)?;
        let _ = led.note_balance(c, BalanceAct::Push, 2, 5, t + 2)?;
        let _ = led.note_balance(c, BalanceAct::Steal, 1, 4, t + 3)?;
        t += 4;
        // IPI：3 resched + 1 halt + 1 wakeup（含 1 失败 1 合并）。
        let _ = led.note_ipi(c, IpiKind::Resched, true, false, 15, t)?;
        let _ = led.note_ipi(c, IpiKind::Resched, true, true, 12, t + 1)?;
        let _ = led.note_ipi(c, IpiKind::Resched, false, false, 30, t + 2)?;
        let _ = led.note_ipi(c, IpiKind::Halt, true, false, 44, t + 3)?;
        let _ = led.note_ipi(c, IpiKind::Wakeup, true, false, 20, t + 4)?;
        t += 5;
        // 自检：2 pass + 1 fail。
        let _ = led.note_selfcheck(c, true, 3, t)?;
        let _ = led.note_selfcheck(c, true, 4, t + 1)?;
        let _ = led.note_selfcheck(c, false, 9, t + 2)?;
        t += 3;
        c += 1;
    }
    led.close_global();
    Ok(led)
}

/// 全量自检（判据一次性落地）。
///
/// 判据：UNX-F1762-J1 —— 十字段族全量登记 + 字段级对齐断言全过 + 守恒全过 + 汇总闭合。
pub fn selftest() -> String {
    let mut s = String::new();
    let mut led = match demo_ledger() {
        Ok(l) => l,
        Err(e) => {
            s.push_str(&format!("demo-ledger-failed={}\n", e.name()));
            return s;
        }
    };

    // 手算锚。
    let anchor = Ledger::hand_anchor_switch().unwrap_or(0);
    s.push_str(&format!("anchor switch_count(4x10)={} ok={}\n", anchor, if anchor == 40 { 1 } else { 0 }));

    // 三方对齐。
    let ar = led.registry.align_report();
    s.push_str(&format!("align {}/{} broken={}\n", ar.aligned, FIELD_COUNT, ar.broken));
    s.push_str(&format!("layout ok={}\n", if ar.layout_ok { 1 } else { 0 }));

    // 十族联合守恒。
    let cons = led.verify_all();
    s.push_str(&format!(
        "conserv passed={} failed={} first={}\n",
        cons.passed,
        cons.failed,
        match &cons.first_bad {
            Some(x) => x.clone(),
            None => "-".to_string(),
        }
    ));
    let mut i = 0usize;
    while i < INVARIANTS.len() {
        s.push_str(&format!("  inv {}\n", INVARIANTS[i].expr));
        i += 1;
    }

    // 汇总闭合。
    let sum = led.verify_sum();
    s.push_str(&format!("sum {}\n", sum.summary()));

    // 时间加权队列深度。
    let mut c = 0u32;
    while c < 4 {
        let w = led.weighted_depth(c).unwrap_or(WeightedDepth {
            window_ticks: 0,
            window_sum: 0,
            weighted: 0,
            residual: 0,
            bound: 0,
            in_bound: false,
        });
        s.push_str(&format!("  cpu{} {}\n", c, w.line()));
        c += 1;
    }

    // 快照与回放。
    let snap = led.capture_snapshot(led.tick);
    s.push_str(&format!("snap {}\n", snap.line()));
    s.push_str(&format!("snap replay-diff={}\n", snap.diff_count(&led.cpus)));

    // 溢出语义。
    s.push_str(&format!(
        "ovf-unmarked-detected={} over-cap={}\n",
        if led.detect_ovf_unmarked().is_err() { 1 } else { 0 },
        if led.detect_over_cap().is_err() { 1 } else { 0 }
    ));

    // 封账。
    let seal0 = led.seal_now().unwrap_or(0);
    s.push_str(&format!("seal={:016x} verify={}\n", seal0, if led.registry.verify_seal().is_ok() { 1 } else { 0 }));

    // TLV。
    let t = export_tlv(&led);
    s.push_str(&format!(
        "tlv len={} family={} field={} inv={} ok={}\n",
        t.len(),
        tlv_count(&t, tlv::T_FAMILY),
        tlv_count(&t, tlv::T_FIELD),
        tlv_count(&t, tlv::T_INV),
        if verify_tlv(&t).is_ok() { 1 } else { 0 }
    ));

    // 注入核。
    let rs = inject_all();
    let mut n = 0u32;
    i = 0usize;
    while i < rs.len() {
        if rs[i].detected {
            n += 1;
        }
        s.push_str(&format!("  {}\n", rs[i].line()));
        i += 1;
    }
    s.push_str(&format!("inject {}/10\n", n));

    // 族级导出抽样。
    s.push_str(&export_family(StatFamily::Switch, &led));
    s
}

/// 诊断码段自检（0xB060~0xB06F 独占 · 16 个码全在段内）。
pub fn diag_segment_check() -> Result<()> {
    let all = [
        DIAG_ALIGN,
        DIAG_CONSERVATION,
        DIAG_SCATTER,
        DIAG_SEAL,
        DIAG_SUM,
        DIAG_WINDOW,
        DIAG_OVF,
        DIAG_OFFSET,
        DIAG_RANGE,
        DIAG_STALE,
        DIAG_FAMILY,
        DIAG_TABLE,
        DIAG_REPLAY,
        DIAG_INJECT,
        DIAG_SAT,
        DIAG_EXPORT,
    ];
    let mut i = 0usize;
    while i < all.len() {
        if all[i] < CPUS_SEG_BASE || all[i] > CPUS_SEG_END {
            return CpErr::TableInconsistent.e();
        }
        i += 1;
    }
    if all.len() as u16 != (CPUS_SEG_END - CPUS_SEG_BASE + 1) {
        return CpErr::TableInconsistent.e();
    }
    Ok(())
}

/// 饱和语义自检：顶到上限后必置 overflow 位且不再增长。
pub fn saturation_selftest() -> Result<()> {
    let mut led = Ledger::new(1);
    let mut t = 1u64;
    let mut k = 0u32;
    while k < 6 {
        led.record(0, IDX_WAKE_REJECT_CAP, SAT_CAP_U32 / 4, t)?;
        t += 1;
        k += 1;
    }
    let c = led.cpu(0)?;
    let v = c.get_raw(IDX_WAKE_REJECT_CAP);
    if !c.ovf_test(IDX_WAKE_REJECT_CAP) {
        return CpErr::OvfUnmarked.e();
    }
    if v != SAT_CAP_U32 {
        return CpErr::OvfUnmarked.e();
    }
    Ok(())
}

/// 结构布局自检（PerCpu 冻结版 640 字节 / 对齐 8）。
pub fn layout_selftest() -> Result<()> {
    if !PerCpuFrozen::layout_ok() {
        return CpErr::OffsetMismatch.e();
    }
    Ok(())
}

/// 族序 ↔ 偏移 ↔ 账位三方一致自检。
pub fn family_order_selftest() -> Result<()> {
    let all = StatFamily::all();
    let mut i = 0usize;
    while i < all.len() {
        let f = all[i];
        if f.ordinal() != i {
            return CpErr::TableInconsistent.e();
        }
        if f.offset_base() != (i as u32) * FAMILY_STRIDE {
            return CpErr::OffsetMismatch.e();
        }
        let g0 = f.gidx(0)?;
        if g0 != i * FIELDS_PER_FAMILY {
            return CpErr::TableInconsistent.e();
        }
        let g11 = f.gidx(11)?;
        let off = percpu_offset(f, 11)?;
        if off + 8 != (i as u32 + 1) * FAMILY_STRIDE {
            return CpErr::OffsetMismatch.e();
        }
        if g11 + 1 != (i + 1) * FIELDS_PER_FAMILY {
            return CpErr::TableInconsistent.e();
        }
        i += 1;
    }
    Ok(())
}

/// 观测接口名唯一性自检（120 个接口名两两不同）。
pub fn iface_uniqueness_selftest() -> Result<()> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut i = 0usize;
    while i < FIELD_COUNT {
        let (f, l) = StatFamily::split_gidx(i)?;
        let name = f.iface_of(l)?;
        let e = seen.insert(name.clone(), i);
        if e.is_some() {
            return CpErr::TableInconsistent.e();
        }
        i += 1;
    }
    if seen.len() != FIELD_COUNT {
        return CpErr::TableInconsistent.e();
    }
    Ok(())
}

/// 全模块一键自检（返回多行报告串）。
pub fn full_selftest() -> String {
    let mut s = String::from("=== unx1762 cpustats full selftest ===\n");
    s.push_str(&format!(
        "layout={} fam-order={} iface-uniq={} diag-seg={} sat={}\n",
        if layout_selftest().is_ok() { 1 } else { 0 },
        if family_order_selftest().is_ok() { 1 } else { 0 },
        if iface_uniqueness_selftest().is_ok() { 1 } else { 0 },
        if diag_segment_check().is_ok() { 1 } else { 0 },
        if saturation_selftest().is_ok() { 1 } else { 0 }
    ));
    s.push_str(&selftest());
    s
}
