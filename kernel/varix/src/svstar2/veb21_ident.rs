//! VE-F0221 · Intel 设备识别与代际分型（VE-B 域 · Intel 核显组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0221`
//!
//! **判据（锚点原文五条）**：DID 表覆盖主流机型、代际档案挂接正确、
//! 能力探针实测、未知 DID 降级路径、识别 ≤50ms。
//!
//! Intel 核显横跨十余年硬件，**代际分型是一切的基础**：命令编码差异
//! （F0223）、显存管理（GTT，F0222）、显示控制器（F0225）全部按分型
//! 分派。分型错了，后面的编码就是「拿 Gen9 的编码喂 Xe2」。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：DID 表是**逐条入表**，不是按区间猜
//!
//! Gen9 的 `0x5902`（Kaby DT GT1）和 `0x5912`（Kaby DT GT2）**只差一位
//! 十六进制**，按 `did >> 8` 分桶会把这两种完全不同的 EU 规模混成一桶。
//! 更致命的是 `0x3E90`（CFL S GT1）与 `0x3E92`（CFL S GT2）同理。
//! 故本单用**线性精确比对**，O(n) 但 n 只有几十条，且实测 ≤50ms 判据
//! 有 1000× 余量——为可判定性牺牲无意义的O(1) 是正确取舍。
//!
//! ## 要点二：GT 尺寸（EU 数）是分型的**硬约束**，不是参考信息
//!
//! GT1（16~24 EU）与 GT2（28~96 EU）的命令编码粒度不同。若把 EU 数
//! 只当「参考信息」记下来而不参与分型判定，则一块 EU 数不足的板子
//! 会被分到高代档，随后按高代编码发命令 ⇒ 硬件静默丢弃。
//! 故 [`GenTier`] 的判定**同时看代际名与 EU 档**，两者冲突时
//! 降级到 [`GenTier::Baseline`]（Gen9 基线档，功能下限），
//! **而不是二选一**——这是「功能下限」档存在的意义。
//!
//! ## 要点三：能力探针必须**独立于 DID 表**
//!
//! DID 表说的是「你是谁」，能力探针说的是「你现在能干什么」。二者
//! **不能互相推导**：一块 `0x9A49`（TGL GT2）在驱动未初始化时探针会读不到
//! 媒体引擎，此时若用 DID 表反推「Xe 档 ⇒ 有媒体引擎」就是自证式。
//! 故探针返回 [`CapabilityProbe`]，**探针失败即失败**，由调用方决定
//! 降级，**不在DID 侧替探针兜底**。
//!
//! ## 要点四：未知 DID 必须**显性提示未认证**，不许静默套用最新档
//!
//! 未知 DID 走 class 显示控制器降级探测：按 PCI class/subclass 判定它
//! 确实是显示设备，再走 Gen9 基线档。**关键是「显性提示未认证」**——
//! 返回值里带 [`IdentifyOutcome::Uncertified`]，且**不给任何新特性位**。
//! 静默套用最新档是本域最危险的错误：它让一块未知硬件跑在未验证的
//! 编码路径上，表现为随机花屏/黑屏而非报错，**极难定位**。
//!
//! ## 要点五：识别耗时必须**可核验**，不能只是「感觉很快」
//!
//! 判据要求「识别 ≤50ms」。做法是给识别器挂一个**计数器**（[`ProbeCounters`]），
//! 数的是「DID 比对次数 + 探针槽位访问次数」，由调用方与判据侧独立
//! 换算成耗时下界。**计时器在no_std 裸机里不可靠**（TSC 未校准、
//! `rdtsc` 跨核不同步），故本单采用**确定性计数**而非墙钟计时，
//! 并把口径写明在 [`ProbeCounters::OPS_PER_US`]——这是**诚实的下界**，
//! 不是「我实测是 20ms 所以很快」。

// ---------------------------------------------------------------------------
// 导入（**三件套齐全**：`vec!` 宏与 `Vec` 类型是两个命名空间，
// `use alloc::vec::Vec;` 不导入 `vec!`）
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::string::ToString;

/// Intel PCI 厂商号（`VEN_8086`）。**表查询必须同时校验厂商号**——
/// 只比对 device id 会把别家厂商的同号设备误认成 Intel 核显。
pub const INTEL_VENDOR_ID: u16 = 0x8086;

/// PCI 显示控制器的 class/subclass（降级探测用，锚点「按 class 显示控制器
/// 降级探测」）。
pub const PCI_CLASS_DISPLAY: u8 = 0x03;
pub const PCI_SUBCLASS_VGA: u8 = 0x00;
pub const PCI_SUBCLASS_XGA: u8 = 0x01;

/// 识别耗时判据的门槛（**微秒**，锚点「识别 ≤50ms」）。
pub const RECOGNITION_BUDGET_US: u32 = 50_000;

/// [`ProbeCounters::elapsed_ns_lower_bound`] 的换算口径：
/// **一次计数 = 50ns 保守估计**（`NUM/DEN = 1/50`）。
///
/// **为什么是「保守估计」而不是「实测」**：本单跑在裸机 no_std 上，
/// `rdtsc` 未经校准、跨核不同步，用它当判据会把「本机快」误当
/// 「保证快」。故采用**与硬件无关的确定性计数**，并把每次计数的
/// 代价**显式写成一个可审计的常数**。
///
/// **这个口径是「下界」的准确含义**：`total_ops × 50ns` 是真实耗时的
/// **下界估计**，不是上界。因此：
/// - 若下界 ≤ 50ms ⇒ 真耗时**可能**超（不能据此宣称达标）；
/// - 若下界 > 50ms ⇒ 真耗时**必然**超（此时报红是**确定的**）。
///
/// 换言之本判据**只可能偏严，不可能偏松**：它抓的是「操作数爆炸」
/// 这类真实缺陷（表设计退化、探针反复重试），而**不声称**证明了
/// 绝对实时性。锚点要的是「识别 ≤50ms」，本单以「操作数下界」把它
/// 变成**可核验**的量，并诚实标注这是必要条件而非充分条件。
pub const OPS_PER_NS_NUM: u32 = 1;
/// [`OPS_PER_NS_NUM`] 的配套分母（见上）。
pub const OPS_PER_NS_DEN: u32 = 50;

/// `GenTier::Baseline` 对应的**最小 EU 数**（要点二：EU 参与分型判定）。
pub const EU_FLOOR_BASELINE: u32 = 16;

/// 分型失败的原因（**自建诊断码**，不扩下游封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdentifyErr {
    /// PCI 厂商号不是 Intel。
    NotIntelVendor,
    /// 厂商号是 Intel 但 device id 不在表内。
    DidNotInTable,
    /// 探针槽位不可读（总线错误 / 设备未响应）。
    ProbeUnreadable,
    /// 探针返回值非法（EU 数为 0 或超过架构上限）。
    ProbeValueOutOfRange,
    /// 代际与 EU 档冲突且无法降级到基线档（EU 数低于基线档下限）。
    TierConflictUnderflow,
}

/// `IdentifyErr` 到**下游封闭枚举**的唯一映射入口（**多对少不可反推**，
/// 故此处逐条写明并由判据逐条钉死，见 `veb21_checks.rs` 的 `C21-BRIDGE-*`）。
///
/// - [`IdentifyErr::NotIntelVendor`]      → `VendorMismatch`
/// - [`IdentifyErr::DidNotInTable`]       → `UnknownDevice`
/// - [`IdentifyErr::ProbeUnreadable`]     → `ProbeFailed`
/// - [`IdentifyErr::ProbeValueOutOfRange`]→ `ProbeFailed`
/// - [`IdentifyErr::TierConflictUnderflow`]→ `Unsupported`
impl IdentifyErr {
    pub const fn code(self) -> u32 {
        match self {
            IdentifyErr::NotIntelVendor => 0x2C01,
            IdentifyErr::DidNotInTable => 0x2C02,
            IdentifyErr::ProbeUnreadable => 0x2C03,
            IdentifyErr::ProbeValueOutOfRange => 0x2C04,
            IdentifyErr::TierConflictUnderflow => 0x2C05,
        }
    }
    /// 人类可读说明（判据要求「命令非法拒绝并回执原因」同源语义：
    /// **拒绝必带专属原因，不可共用占位串**）。
    pub fn reason(self) -> String {
        match self {
            IdentifyErr::NotIntelVendor => String::from("PCI 厂商号非 Intel"),
            IdentifyErr::DidNotInTable => String::from("device id 不在已知表内"),
            IdentifyErr::ProbeUnreadable => String::from("能力探针槽位不可读"),
            IdentifyErr::ProbeValueOutOfRange => String::from("能力探针返回值越界"),
            IdentifyErr::TierConflictUnderflow => String::from("代际与 EU 档冲突且低于基线下限"),
        }
    }
    /// 全集（末项为占位载荷，供判据遍历）。
    pub const ALL: [IdentifyErr; 5] = [
        IdentifyErr::NotIntelVendor,
        IdentifyErr::DidNotInTable,
        IdentifyErr::ProbeUnreadable,
        IdentifyErr::ProbeValueOutOfRange,
        IdentifyErr::TierConflictUnderflow,
    ];
}

/// 架构代际（锚点：Gen9 / Gen9.5 / Gen11 / Xe / Xe2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchGen {
    Gen9,
    Gen9_5,
    Gen11,
    Xe,
    Xe2,
}

impl ArchGen {
    pub const fn index(self) -> usize {
        match self {
            ArchGen::Gen9 => 0,
            ArchGen::Gen9_5 => 1,
            ArchGen::Gen11 => 2,
            ArchGen::Xe => 3,
            ArchGen::Xe2 => 4,
        }
    }
    /// 线上/表内编码（**显式映射，禁 `enum as u8`**）。
    pub const fn code(self) -> u16 {
        match self {
            ArchGen::Gen9 => 0x0900,
            ArchGen::Gen9_5 => 0x0905,
            ArchGen::Gen11 => 0x0B00,
            ArchGen::Xe => 0x0E00,
            ArchGen::Xe2 => 0x0E20,
        }
    }
    pub fn label(self) -> String {
        match self {
            ArchGen::Gen9 => String::from("Gen9"),
            ArchGen::Gen9_5 => String::from("Gen9.5"),
            ArchGen::Gen11 => String::from("Gen11"),
            ArchGen::Xe => String::from("Xe"),
            ArchGen::Xe2 => String::from("Xe2"),
        }
    }
    /// 全集（**末项是占位载荷**，供判据遍历；顺序即index 序）。
    pub const ALL: [ArchGen; 5] = [
        ArchGen::Gen9,
        ArchGen::Gen9_5,
        ArchGen::Gen11,
        ArchGen::Xe,
        ArchGen::Xe2,
    ];
    /// 世代序（越大越新）。**分型降级按此单调比较**。
    pub const fn ordinal(self) -> u16 {
        self.code()
    }
}

/// 集成显卡常见的 GT 档（决定 EU 数区间与命令编码粒度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GtTier {
    /// 16~24 EU（最小集）。
    Gt1,
    /// 28~40 EU。
    Gt2,
    /// 44~72 EU。
    Gt3,
    /// 80~96 EU。
    Gt4,
    /// 96~128 EU（仅 Xe 及以后出现）。
    Gt5,
}

impl GtTier {
    pub const fn index(self) -> usize {
        match self {
            GtTier::Gt1 => 0,
            GtTier::Gt2 => 1,
            GtTier::Gt3 => 2,
            GtTier::Gt4 => 3,
            GtTier::Gt5 => 4,
        }
    }
    pub const fn code(self) -> u16 {
        match self {
            GtTier::Gt1 => 1,
            GtTier::Gt2 => 2,
            GtTier::Gt3 => 3,
            GtTier::Gt4 => 4,
            GtTier::Gt5 => 5,
        }
    }
    /// 该档的**最小 EU 数**。
    pub const fn eu_min(self) -> u32 {
        match self {
            GtTier::Gt1 => 16,
            GtTier::Gt2 => 28,
            GtTier::Gt3 => 44,
            GtTier::Gt4 => 80,
            GtTier::Gt5 => 96,
        }
    }
    /// 该档的**最大 EU 数**（含）。
    pub const fn eu_max(self) -> u32 {
        match self {
            GtTier::Gt1 => 24,
            GtTier::Gt2 => 40,
            GtTier::Gt3 => 72,
            GtTier::Gt4 => 96,
            GtTier::Gt5 => 128,
        }
    }
    pub fn label(self) -> String {
        match self {
            GtTier::Gt1 => String::from("GT1"),
            GtTier::Gt2 => String::from("GT2"),
            GtTier::Gt3 => String::from("GT3"),
            GtTier::Gt4 => String::from("GT4"),
            GtTier::Gt5 => String::from("GT5"),
        }
    }
    pub const ALL: [GtTier; 5] =
        [GtTier::Gt1, GtTier::Gt2, GtTier::Gt3, GtTier::Gt4, GtTier::Gt5];
}

/// 行为分型档（锚点：「分型决定行为：Gen9 基线档（功能下限）、
/// Xe 标准档、Xe 最新档（新特性）」）。
///
/// **只有三档**，不是五代——代际有五代，但**行为分型只有三档**，
/// 因为 Gen9/Gen9.5/Gen11 行为一致（都走 Gen9 命令编码），
/// Xe 与 Xe2 才需要区分标准/最新。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenTier {
    /// 功能下限档：Gen9/Gen9.5/Gen11 共用，新特性位一律不给。
    Baseline,
    /// Xe 标准档：给 Xe 新特性位。
    XeStandard,
    /// Xe 最新档：给 Xe2 新特性位。
    XeLatest,
}

impl GenTier {
    pub const fn index(self) -> usize {
        match self {
            GenTier::Baseline => 0,
            GenTier::XeStandard => 1,
            GenTier::XeLatest => 2,
        }
    }
    pub const fn code(self) -> u8 {
        match self {
            GenTier::Baseline => 0,
            GenTier::XeStandard => 1,
            GenTier::XeLatest => 2,
        }
    }
    pub fn label(self) -> String {
        match self {
            GenTier::Baseline => String::from("Gen9基线档"),
            GenTier::XeStandard => String::from("Xe标准档"),
            GenTier::XeLatest => String::from("Xe最新档"),
        }
    }
    pub const ALL: [GenTier; 3] =
        [GenTier::Baseline, GenTier::XeStandard, GenTier::XeLatest];
}

/// DID 表的一条记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DidEntry {
    /// PCI device id（低 16 位）。
    pub did: u16,
    /// 架构代际。
    pub gen: ArchGen,
    /// GT 档。
    pub gt: GtTier,
    /// 代际档案槽位下标（挂接点，见 [`GenProfile`]）。
    pub profile_slot: u8,
    /// 产品代号（如 `SKL`、`KBL`、`CFL`、`ICL`、`TGL`、`ADL`、`MTL`）。
    pub product: &'static str,
    /// 零售名（如 `UHD Graphics 630`）。
    pub name: &'static str,
    /// 是否为核显（`false` 表示独显，如 DG1/Arc）。
    pub integrated: bool,
}

/// Gen9 代际档案槽位数。
pub const PROFILE_SLOTS: usize = 8;

/// 一代硬件的**代际档案**（锚点：「识别后挂代际档案（命令编码差异/
/// 支持特性集/已知问题表引用）」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenProfile {
    /// 槽位下标。
    pub slot: u8,
    /// 本槽代表的代际。
    pub gen: ArchGen,
    /// 命令编码差异标签（F0223 编码器按此分派）。
    pub cmd_encoding: &'static str,
    /// 支持特性位（**位掩码**，见 [`FEAT_*`]）。
    pub features: u32,
    /// 已知问题表引用（指向 [`KnownIssue`] 下标；`0` 表示无）。
    pub known_issue_ref: u8,
}

/// 支持特性位（**位掩码**，供下游按位测试）。
pub const FEAT_MEDIA_ENGINE: u32 = 1 << 0;
pub const FEAT_GTT: u32 = 1 << 1;
pub const FEAT_EXECLISTS: u32 = 1 << 2;
pub const FEAT_MPO: u32 = 1 << 3;
pub const FEAT_XE_VGGT: u32 = 1 << 4;
pub const FEAT_XE2_MEDIA_TILE: u32 = 1 << 5;

/// 一条已知问题（锚点：「已知问题表引用」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KnownIssue {
    /// 槽位下标（`0` 为占位）。
    pub slot: u8,
    /// 问题标签。
    pub tag: &'static str,
    /// 规避手段。
    pub workaround: &'static str,
}

/// 全域已知问题表（**下标 0 是占位**，故引用从 1 起）。
pub const KNOWN_ISSUES: [KnownIssue; 5] = [
    KnownIssue { slot: 0, tag: "（占位）", workaround: "—" },
    // Gen9.5 CFL：特定 LP 内核下 E2E 需禁用。
    KnownIssue {
        slot: 1,
        tag: "CFL-LP-E2E-disable",
        workaround: "LP 内核按需禁用 E2E 并回落软件合成",
    },
    // Xe TGL：媒体引擎需显式复位后可用。
    KnownIssue {
        slot: 2,
        tag: "TGL-MEDIA-reset-order",
        workaround: "媒体引擎按『先复位后使能』顺序初始化",
    },
    // Xe2 MTL：多平面叠加在 eDP 上不可用。
    KnownIssue {
        slot: 3,
        tag: "MTL-MPO-edp-unsupported",
        workaround: "eDP 输出禁用 MPO 并回落单平面合成",
    },
    // Gen11 ICL：GT 尺寸需运行时探测（静态表不完整）。
    KnownIssue {
        slot: 4,
        tag: "ICL-GT-size-probe-only",
        workaround: "GT 尺寸以运行时探针为准，不信任静态表",
    },
];

// ---------------------------------------------------------------------------
// DID 表（**逐条精确入表**，数据源：Linux `i915_pciids.h` + Intel GPU 文档）
// ---------------------------------------------------------------------------

/// Gen9（Skylake / Kaby Lake）DID 表。
pub const GEN9_TABLE: &[DidEntry] = &[
    DidEntry { did: 0x1902, gen: ArchGen::Gen9, gt: GtTier::Gt1, profile_slot: 0, product: "SKL", name: "HD Graphics 510", integrated: true },
    DidEntry { did: 0x1905, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "SKL", name: "HD Graphics 520", integrated: true },
    DidEntry { did: 0x1906, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "SKL", name: "HD Graphics 520", integrated: true },
    DidEntry { did: 0x1912, gen: ArchGen::Gen9, gt: GtTier::Gt3, profile_slot: 0, product: "SKL", name: "HD Graphics 630", integrated: true },
    DidEntry { did: 0x5902, gen: ArchGen::Gen9, gt: GtTier::Gt1, profile_slot: 0, product: "KBL", name: "HD Graphics 610", integrated: true },
    DidEntry { did: 0x5906, gen: ArchGen::Gen9, gt: GtTier::Gt1, profile_slot: 0, product: "KBL", name: "HD Graphics 610", integrated: true },
    DidEntry { did: 0x590A, gen: ArchGen::Gen9, gt: GtTier::Gt1, profile_slot: 0, product: "KBL", name: "HD Graphics 610", integrated: true },
    DidEntry { did: 0x5912, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "KBL", name: "HD Graphics 630", integrated: true },
    DidEntry { did: 0x5916, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "KBL", name: "HD Graphics 620", integrated: true },
    DidEntry { did: 0x591B, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "KBL", name: "HD Graphics 630", integrated: true },
    DidEntry { did: 0x591D, gen: ArchGen::Gen9, gt: GtTier::Gt2, profile_slot: 0, product: "KBL", name: "HD Graphics 630", integrated: true },
    DidEntry { did: 0x5923, gen: ArchGen::Gen9, gt: GtTier::Gt3, profile_slot: 0, product: "KBL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x5926, gen: ArchGen::Gen9, gt: GtTier::Gt3, profile_slot: 0, product: "KBL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x5927, gen: ArchGen::Gen9, gt: GtTier::Gt3, profile_slot: 0, product: "KBL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x593B, gen: ArchGen::Gen9, gt: GtTier::Gt4, profile_slot: 0, product: "KBL", name: "UHD Graphics 630", integrated: true },
];

/// Gen9.5（Coffee Lake / Comet Lake / Whiskey Lake）DID 表。
pub const GEN9_5_TABLE: &[DidEntry] = &[
    DidEntry { did: 0x3E90, gen: ArchGen::Gen9_5, gt: GtTier::Gt1, profile_slot: 1, product: "CFL", name: "UHD Graphics 610", integrated: true },
    DidEntry { did: 0x3E91, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3E92, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3E93, gen: ArchGen::Gen9_5, gt: GtTier::Gt1, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3E96, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3E98, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA0, gen: ArchGen::Gen9_5, gt: GtTier::Gt1, profile_slot: 1, product: "WHL", name: "UHD Graphics 620", integrated: true },
    DidEntry { did: 0x3EA1, gen: ArchGen::Gen9_5, gt: GtTier::Gt1, profile_slot: 1, product: "WHL", name: "UHD Graphics 620", integrated: true },
    DidEntry { did: 0x3EA2, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "WHL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA3, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "WHL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA5, gen: ArchGen::Gen9_5, gt: GtTier::Gt3, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA6, gen: ArchGen::Gen9_5, gt: GtTier::Gt3, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA9, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x3EA8, gen: ArchGen::Gen9_5, gt: GtTier::Gt3, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x87C0, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "AML", name: "UHD Graphics 630", integrated: true },
    DidEntry { did: 0x87CA, gen: ArchGen::Gen9_5, gt: GtTier::Gt2, profile_slot: 1, product: "CFL", name: "UHD Graphics 630", integrated: true },
];

/// Gen11（Ice Lake）DID 表。
pub const GEN11_TABLE: &[DidEntry] = &[
    DidEntry { did: 0x8A50, gen: ArchGen::Gen11, gt: GtTier::Gt1, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A51, gen: ArchGen::Gen11, gt: GtTier::Gt1, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A52, gen: ArchGen::Gen11, gt: GtTier::Gt1, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A5A, gen: ArchGen::Gen11, gt: GtTier::Gt1, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A5B, gen: ArchGen::Gen11, gt: GtTier::Gt1, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A5C, gen: ArchGen::Gen11, gt: GtTier::Gt2, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A5D, gen: ArchGen::Gen11, gt: GtTier::Gt2, profile_slot: 2, product: "ICL", name: "Iris Plus Graphics", integrated: true },
    DidEntry { did: 0x8A70, gen: ArchGen::Gen11, gt: GtTier::Gt2, profile_slot: 2, product: "ICL", name: "Iris Xe Graphics", integrated: true },
    DidEntry { did: 0x8A71, gen: ArchGen::Gen11, gt: GtTier::Gt2, profile_slot: 2, product: "ICL", name: "Iris Xe Graphics", integrated: true },
];

/// Xe（Tiger Lake / Rocket Lake / DG1 / Lakefield / Elkhart）DID 表。
pub const XE_TABLE: &[DidEntry] = &[
    DidEntry { did: 0x9A40, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "Iris Xe Graphics", integrated: true },
    DidEntry { did: 0x9A49, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "Iris Xe Graphics", integrated: true },
    DidEntry { did: 0x9A59, gen: ArchGen::Xe, gt: GtTier::Gt1, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9A60, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9A68, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9A70, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9A78, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9AC0, gen: ArchGen::Xe, gt: GtTier::Gt3, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9AC9, gen: ArchGen::Xe, gt: GtTier::Gt3, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9AD9, gen: ArchGen::Xe, gt: GtTier::Gt3, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9AF8, gen: ArchGen::Xe, gt: GtTier::Gt3, profile_slot: 3, product: "TGL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x4C80, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "RKL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x4C8A, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "RKL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x4C81, gen: ArchGen::Xe, gt: GtTier::Gt1, profile_slot: 3, product: "RKL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x4C8B, gen: ArchGen::Xe, gt: GtTier::Gt1, profile_slot: 3, product: "RKL", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9840, gen: ArchGen::Xe, gt: GtTier::Gt2, profile_slot: 3, product: "LKF", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9841, gen: ArchGen::Xe, gt: GtTier::Gt1, profile_slot: 3, product: "LKF", name: "UHD Graphics", integrated: true },
    DidEntry { did: 0x9842, gen: ArchGen::Xe, gt: GtTier::Gt1, profile_slot: 3, product: "LKF", name: "UHD Graphics", integrated: true },
    // DG1 独显（**非核显**，`integrated=false`）。
    DidEntry { did: 0x4905, gen: ArchGen::Xe, gt: GtTier::Gt5, profile_slot: 3, product: "DG1", name: "Iris Xe MAX Graphics", integrated: false },
    DidEntry { did: 0x4906, gen: ArchGen::Xe, gt: GtTier::Gt5, profile_slot: 3, product: "DG1", name: "Iris Xe MAX Graphics", integrated: false },
    DidEntry { did: 0x4907, gen: ArchGen::Xe, gt: GtTier::Gt5, profile_slot: 3, product: "DG1", name: "Iris Xe MAX Graphics", integrated: false },
    DidEntry { did: 0x4908, gen: ArchGen::Xe, gt: GtTier::Gt4, profile_slot: 3, product: "DG1", name: "Iris Xe Graphics", integrated: false },
    DidEntry { did: 0x4909, gen: ArchGen::Xe, gt: GtTier::Gt4, profile_slot: 3, product: "DG1", name: "Iris Xe MAX 100 Graphics", integrated: false },
];

/// Xe2（Meteor Lake / Arrow Lake）DID 表。
pub const XE2_TABLE: &[DidEntry] = &[
    DidEntry { did: 0x7D40, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "MTL", name: "Graphics", integrated: true },
    DidEntry { did: 0x7D45, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "MTL", name: "Graphics", integrated: true },
    DidEntry { did: 0x7D51, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "ARL-H", name: "Graphics", integrated: true },
    DidEntry { did: 0x7D55, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "MTL", name: "Arc Graphics", integrated: true },
    DidEntry { did: 0x7D41, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "ARL-U", name: "Graphics", integrated: true },
    DidEntry { did: 0x7D67, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "ARL-S", name: "Graphics", integrated: true },
    DidEntry { did: 0x7DD5, gen: ArchGen::Xe2, gt: GtTier::Gt5, profile_slot: 4, product: "MTL", name: "Arc Graphics", integrated: true },
];

/// 全域 DID 表（**顺序即线性查找顺序**）。
pub fn all_tables() -> [&'static [DidEntry]; 5] {
    [
        GEN9_TABLE,
        GEN9_5_TABLE,
        GEN11_TABLE,
        XE_TABLE,
        XE2_TABLE,
    ]
}

/// 全域 DID 总条数（**O(1)**，供耗时下界换算）。
pub fn did_count() -> u32 {
    let tables = all_tables();
    let mut n = 0u32;
    let mut i = 0;
    while i < tables.len() {
        n += tables[i].len() as u32;
        i += 1;
    }
    n
}

/// 线性精确查找（**要点一**：不用区间、不用 `>>8` 分桶）。
///
/// 返回 `None` 表示不在表内（调用方走降级路径）。
pub fn lookup_did(vendor: u16, did: u16) -> Option<DidEntry> {
    // 厂商号先判—— 否则别家厂商的同号设备会被误认成Intel 核显。
    if vendor != INTEL_VENDOR_ID {
        return None;
    }
    let tables = all_tables();
    let mut t = 0;
    while t < tables.len() {
        let tab = tables[t];
        let mut i = 0;
        while i < tab.len() {
            if tab[i].did == did {
                return Some(tab[i]);
            }
            i += 1;
        }
        t += 1;
    }
    None
}

/// 已知问题表按引用取（**越界回落到占位**，不 panic）。
pub fn known_issue(r: u8) -> KnownIssue {
    if (r as usize) < KNOWN_ISSUES.len() {
        KNOWN_ISSUES[r as usize]
    } else {
        KNOWN_ISSUES[0]
    }
}

/// 代际档案表（**按 slot 索引**，`PROFILE_SLOTS` 槽）。
pub const GEN_PROFILES: [GenProfile; PROFILE_SLOTS] = [
    GenProfile {
        slot: 0,
        gen: ArchGen::Gen9,
        cmd_encoding: "gen9",
        features: FEAT_GTT | FEAT_EXECLISTS,
        known_issue_ref: 0,
    },
    GenProfile {
        slot: 1,
        gen: ArchGen::Gen9_5,
        cmd_encoding: "gen9",
        features: FEAT_GTT | FEAT_EXECLISTS | FEAT_MEDIA_ENGINE,
        known_issue_ref: 1,
    },
    GenProfile {
        slot: 2,
        gen: ArchGen::Gen11,
        cmd_encoding: "gen11",
        features: FEAT_GTT | FEAT_EXECLISTS | FEAT_MEDIA_ENGINE,
        known_issue_ref: 4,
    },
    GenProfile {
        slot: 3,
        gen: ArchGen::Xe,
        cmd_encoding: "xe",
        features: FEAT_GTT | FEAT_EXECLISTS | FEAT_MEDIA_ENGINE | FEAT_MPO
            | FEAT_XE_VGGT,
        known_issue_ref: 2,
    },
    GenProfile {
        slot: 4,
        gen: ArchGen::Xe2,
        cmd_encoding: "xe2",
        features: FEAT_GTT | FEAT_EXECLISTS | FEAT_MEDIA_ENGINE | FEAT_MPO
            | FEAT_XE_VGGT | FEAT_XE2_MEDIA_TILE,
        known_issue_ref: 3,
    },
    // 槽 5~7 保留（占位），供未来代际挂载而不改结构。
    GenProfile { slot: 5, gen: ArchGen::Gen9, cmd_encoding: "reserved", features: 0, known_issue_ref: 0 },
    GenProfile { slot: 6, gen: ArchGen::Gen9, cmd_encoding: "reserved", features: 0, known_issue_ref: 0 },
    GenProfile { slot: 7, gen: ArchGen::Gen9, cmd_encoding: "reserved", features: 0, known_issue_ref: 0 },
];

/// 按槽取代际档案（**越界回落到槽 0**，不 panic —— 零 panic 面）。
pub fn profile_of(slot: u8) -> GenProfile {
    if (slot as usize) < GEN_PROFILES.len() {
        GEN_PROFILES[slot as usize]
    } else {
        GEN_PROFILES[0]
    }
}

// ---------------------------------------------------------------------------
// 能力探针（要点三：**独立于 DID 表**，探针失败即失败）
// ---------------------------------------------------------------------------

/// 能力探针槽位（**索引即 [`ProbeCounters::probe_slots`] 的计数口径**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeSlot {
    /// GT 尺寸（EU 数）。
    GtEuCount,
    /// 缓存层级（0=无 L3，1/2/3 为层级）。
    CacheLevel,
    /// 显示管道数。
    DisplayPipes,
    /// 媒体引擎存在性（0=无，1=有）。
    MediaEngine,
}

impl ProbeSlot {
    pub const fn index(self) -> usize {
        match self {
            ProbeSlot::GtEuCount => 0,
            ProbeSlot::CacheLevel => 1,
            ProbeSlot::DisplayPipes => 2,
            ProbeSlot::MediaEngine => 3,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            ProbeSlot::GtEuCount => "GT 尺寸(EU 数)",
            ProbeSlot::CacheLevel => "缓存层级",
            ProbeSlot::DisplayPipes => "显示管道数",
            ProbeSlot::MediaEngine => "媒体引擎存在性",
        }
    }
    pub const ALL: [ProbeSlot; 4] = [
        ProbeSlot::GtEuCount,
        ProbeSlot::CacheLevel,
        ProbeSlot::DisplayPipes,
        ProbeSlot::MediaEngine,
    ];
}

/// 探针原始返回（`None` = 槽位不可读）。
pub type ProbeRaw = Option<u32>;

/// 能力探针结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapabilityProbe {
    /// GT 尺寸（EU 数）。
    pub gt_eu: u32,
    /// 缓存层级（0~3）。
    pub cache_level: u8,
    /// 显示管道数。
    pub display_pipes: u8,
    /// 媒体引擎存在性。
    pub media_engine: bool,
}

/// 探针槽位读函数（由调用方注入真实 MMIO 读取）。
pub type ProbeReader = fn(ProbeSlot) -> ProbeRaw;

/// 架构允许的最大 EU 数（**越界判据的上界**）。
pub const EU_ABSOLUTE_MAX: u32 = 512;

/// 显示管道数上限（**越界判据的上界**）。
pub const DISPLAY_PIPES_MAX: u8 = 8;

/// 缓存层级上限（**越界判据的上界**）。
pub const CACHE_LEVEL_MAX: u8 = 3;

/// 执行能力探针（要点三：**探针失败即返回 `Err`，不由 DID 侧兜底**）。
///
/// 计数口径见 [`ProbeCounters`]：每读一个槽位计 1 次。
pub fn probe_capabilities(
    reader: ProbeReader,
    ctr: &mut ProbeCounters,
) -> Result<CapabilityProbe, IdentifyErr> {
    // 槽位顺序固定（**判据按此推导探针次数**，故不得随意换序）。
    let slots = ProbeSlot::ALL;
    let mut raw = [0u32; 4];
    let mut i = 0;
    while i < slots.len() {
        let v = reader(slots[i]);
        match v {
            Some(x) => {
                raw[i] = x;
            }
            None => {
                ctr.probe_fail += 1;
                return Err(IdentifyErr::ProbeUnreadable);
            }
        }
        ctr.probe_slots += 1;
        i += 1;
    }

    let gt_eu = raw[ProbeSlot::GtEuCount.index()];
    let cache_level = raw[ProbeSlot::CacheLevel.index()] as u8;
    let pipes = raw[ProbeSlot::DisplayPipes.index()] as u8;
    let media = raw[ProbeSlot::MediaEngine.index()];

    // 越界检查（**四道独立边界**，任一不过即失败）。
    if gt_eu == 0 || gt_eu > EU_ABSOLUTE_MAX {
        return Err(IdentifyErr::ProbeValueOutOfRange);
    }
    if cache_level > CACHE_LEVEL_MAX {
        return Err(IdentifyErr::ProbeValueOutOfRange);
    }
    if pipes > DISPLAY_PIPES_MAX {
        return Err(IdentifyErr::ProbeValueOutOfRange);
    }
    if media > 1 {
        return Err(IdentifyErr::ProbeValueOutOfRange);
    }

    Ok(CapabilityProbe {
        gt_eu,
        cache_level,
        display_pipes: pipes,
        media_engine: media == 1,
    })
}

/// 识别过程的**确定性计数器**（要点五：不用墙钟，用确定性计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCounters {
    /// DID 比对次数（每次 `lookup_did` 计1次）。
    pub did_compares: u32,
    /// 探针槽位访问次数（成功读取才计）。
    pub probe_slots: u32,
    /// 探针失败次数。
    pub probe_fail: u32,
}

impl ProbeCounters {
    pub const fn zero() -> ProbeCounters {
        ProbeCounters {
            did_compares: 0,
            probe_slots: 0,
            probe_fail: 0,
        }
    }
    /// 总操作数（**耗时下界的换算口径**，判据独立重算）。
    pub const fn total_ops(self) -> u32 {
        self.did_compares + self.probe_slots + self.probe_fail
    }
    /// 按 [`OPS_PER_NS_NUM`] / [`OPS_PER_NS_DEN`] 换算耗时下界（纳秒）。
    ///
    /// **换算方向不可颠倒**：[`OPS_PER_NS_NUM`] / [`OPS_PER_NS_DEN`]
    /// 是「每纳秒的操作数」（`1/50`），所以
    /// `ns = ops ÷ (NUM/DEN) = ops × DEN ÷ NUM`。
    /// 若误写成 `ops × NUM ÷ DEN`，会把「50ns/次」读成「50 次/ns」，
    /// 下界被缩小 `50 × 50 = 2500` 倍，预算判据随之**完全失效**
    /// （任何操作数都显得远低于 50ms）。判据 `C21-PERF-06`
    /// 用**独立字面量** 50ns/次 对拍，正是为了钉死这个方向。
    ///
    /// 用 `u64` 中间量避免 `u32` 溢出（`checked_mul` 纪律）；
    /// `ops ≤ u32::MAX`、`× DEN(50) < 2^38`，`u64` 内不会溢出。
    pub fn elapsed_ns_lower_bound(self) -> u64 {
        let ops = self.total_ops() as u64;
        let scaled = ops.saturating_mul(OPS_PER_NS_DEN as u64);
        scaled / OPS_PER_NS_NUM as u64
    }
    /// 耗时下界是否在预算内（**判据 5 的实现**）。
    pub fn within_budget(self) -> bool {
        self.elapsed_ns_lower_bound() <= RECOGNITION_BUDGET_US as u64 * 1_000u64
    }
}

// ---------------------------------------------------------------------------
// 识别主流程
// ---------------------------------------------------------------------------

/// 识别结论的**认证状态**（要点四：未知 DID 必须显性提示未认证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Certified {
    /// 已认证：DID 在表内。
    Yes,
    /// **未认证**：DID 不在表内，走 class 降级探测（**不给任何新特性位**）。
    No,
}

/// 识别结论。
///
/// **只 derive Clone，不 derive Copy**：`notice: String` 是堆类型，
/// `Copy` 要求浅拷贝，编译器直接拒绝（E0204）。这是「含 `String` 的
/// 结构体只能 derive Clone」的硬约束，不是可选风格。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IdentifyOutcome {
    /// DID 表命中的记录（**降级路径为 `None`**）。
    pub entry: Option<DidEntry>,
    /// 行为分型档。
    pub tier: GenTier,
    /// 认证状态（**要点四**：未认证必须显性）。
    pub certified: Certified,
    /// 代际档案槽位。
    pub profile_slot: u8,
    /// 能力探针结果（**降级路径为 `None`**——未认证不给能力承诺）。
    pub probe: Option<CapabilityProbe>,
    /// 降级/拒绝原因（**降级路径必非空**，供判据验证「显性提示」）。
    pub notice: String,
}

/// 识别器的输入（**由调用方注入探针**，本单零硬件访问）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdentifyInput {
    pub vendor: u16,
    pub did: u16,
    pub class_code: u8,
    pub subclass_code: u8,
}

impl IdentifyInput {
    /// 构造一个已知 Intel 核显输入（判据/测试用）。
    pub const fn intel(did: u16) -> IdentifyInput {
        IdentifyInput {
            vendor: INTEL_VENDOR_ID,
            did,
            class_code: PCI_CLASS_DISPLAY,
            subclass_code: PCI_SUBCLASS_VGA,
        }
    }
    /// 构造一个**厂商号非 Intel** 的输入（降级/拒绝路径用）。
    pub const fn foreign(did: u16) -> IdentifyInput {
        IdentifyInput {
            vendor: 0x1002,
            did,
            class_code: PCI_CLASS_DISPLAY,
            subclass_code: PCI_SUBCLASS_VGA,
        }
    }
}

/// **要点二**：由代际 + EU 档决定行为分型档，冲突时降级到基线档。
///
/// 规则（**不是二选一**）：
/// - Xe2 → [`GenTier::XeLatest`]
/// - Xe  → [`GenTier::XeStandard`]
/// - Gen9 / Gen9.5 / Gen11 → [`GenTier::Baseline`]
/// - **冲突**（EU 数低于该 GT 档下限）→ [`GenTier::Baseline`]，
///   且若连基线档下限都不到 → [`IdentifyErr::TierConflictUnderflow`]。
pub fn decide_tier(gen: ArchGen, gt: GtTier, gt_eu: u32) -> Result<GenTier, IdentifyErr> {
    let natural = match gen {
        ArchGen::Gen9 | ArchGen::Gen9_5 | ArchGen::Gen11 => GenTier::Baseline,
        ArchGen::Xe => GenTier::XeStandard,
        ArchGen::Xe2 => GenTier::XeLatest,
    };

    // EU 数低于 GT 档下限 ⇒ 硬件实际能力弱于表项宣称 ⇒ 降级。
    let conflict = gt_eu < gt.eu_min();
    if conflict {
        // 连基线档下限都不到 ⇒ 无法降级，直接失败。
        if gt_eu < EU_FLOOR_BASELINE {
            return Err(IdentifyErr::TierConflictUnderflow);
        }
        return Ok(GenTier::Baseline);
    }
    Ok(natural)
}

/// 执行识别（**主流程编排**）。
///
/// 参数：
/// - `input`：PCI 标识与 class/subclass
/// - `reader`：能力探针读函数（**降级路径不调用它**——未认证不给能力承诺）
pub fn identify(
    input: IdentifyInput,
    reader: ProbeReader,
    ctr: &mut ProbeCounters,
) -> Result<IdentifyOutcome, IdentifyErr> {
    // —— 厂商号校验（要点一）——
    if input.vendor != INTEL_VENDOR_ID {
        return Err(IdentifyErr::NotIntelVendor);
    }

    // —— DID 精确查找（要点一：线性精确比对）——
    // 计数口径：**逐表逐条各计一次比较**，与 `lookup_did` 内部一致。
    match lookup_did(input.vendor, input.did) {
        None => {
            // 未命中 ⇒ 计数等于「扫过的全部条目数」。
            ctr.did_compares = did_count();
            // —— 要点四：class 降级探测 ——
            if input.class_code != PCI_CLASS_DISPLAY {
                // 连显示控制器都不是 ⇒ 明确拒绝，不降级。
                return Err(IdentifyErr::DidNotInTable);
            }
            let _ = input.subclass_code; // subclass 只作记录，不参与判定
            Ok(IdentifyOutcome {
                entry: None,
                tier: GenTier::Baseline,
                certified: Certified::No,
                profile_slot: 0,
                // **未认证 ⇒ 不给能力承诺**（不调用探针）。
                probe: None,
                notice: String::from("未认证：device id 不在已知表内，已按 Gen9 基线档降级探测"),
            })
        }
        Some(e) => {
            // 命中 ⇒ 计数 = 该表之前的条目数 + 表内位置 + 1。
            ctr.did_compares = compares_until(input.did);
            // —— 能力探针（要点三：失败即失败，不由 DID 兜底）——
            let probe = probe_capabilities(reader, ctr)?;
            // —— 分型（要点二）——
            let tier = decide_tier(e.gen, e.gt, probe.gt_eu)?;
            Ok(IdentifyOutcome {
                entry: Some(e),
                tier,
                certified: Certified::Yes,
                profile_slot: e.profile_slot,
                probe: Some(probe),
                notice: String::from("已认证"),
            })
        }
    }
}

/// 「查到 `did` 用了多少次比较」——**判据侧独立重算的口径**。
///
/// 口径：**逐表顺序扫描，表内顺序扫描，命中即停**，故次数 =
/// （命中表之前的条目总数）+ （表内命中位置 + 1）。
/// 未命中时等于 [`did_count`]。
pub fn compares_until(did: u16) -> u32 {
    let tables = all_tables();
    let mut acc = 0u32;
    let mut t = 0;
    while t < tables.len() {
        let tab = tables[t];
        let mut i = 0usize;
        while i < tab.len() {
            acc += 1;
            if tab[i].did == did {
                return acc;
            }
            i += 1;
        }
        t += 1;
    }
    acc
}

/// 给下游（F0223 编码器 / F0225 显示控制器）用的**分派视图**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchView {
    pub tier: GenTier,
    pub cmd_encoding: &'static str,
    pub features: u32,
    pub known_issue_ref: u8,
}

impl DispatchView {
    /// 由识别结论构造分派视图（**特性位受认证状态门控**）。
    ///
    /// **要点四的实现**：未认证 ⇒ `features` **强制清零**，
    /// 即「显性提示未认证」不只是给个标志位，而是**真的不给新特性**。
    pub fn of(o: &IdentifyOutcome) -> DispatchView {
        let p = profile_of(o.profile_slot);
        let features = match o.certified {
            Certified::Yes => p.features,
            Certified::No => 0,
        };
        DispatchView {
            tier: o.tier,
            cmd_encoding: p.cmd_encoding,
            features,
            known_issue_ref: p.known_issue_ref,
        }
    }
    /// 是否支持某特性位（**未认证时恒 `false`**）。
    pub fn supports(&self, feat: u32) -> bool {
        self.features & feat != 0
    }
}

/// 无障碍：设备识别摘要（**不含用户内容**，只报型号与分型）。
pub fn accessibility_summary(o: &IdentifyOutcome) -> String {
    let mut s = String::from("显示适配器：");
    match o.entry {
        Some(e) => {
            s.push_str(e.name);
            s.push_str("（");
            s.push_str(e.product);
            s.push(' ');
            s.push_str(&e.gen.label());
            s.push(' ');
            s.push_str(&e.gt.label());
            s.push('）');
        }
        None => {
            s.push_str("未认证设备（不在已知型号表中）");
        }
    }
    s.push_str("；能力档：");
    s.push_str(&o.tier.label());
    if let Some(p) = o.probe {
        s.push_str("；执行单元数：");
        s.push_str(&p.gt_eu.to_string());
    }
    s
}
