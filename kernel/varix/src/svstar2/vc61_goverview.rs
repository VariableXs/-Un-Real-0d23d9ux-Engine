//! CGPU-F0961 · G 域开工与 NVIDIA 总览（CGPU-G 域 · GPU 兼容矩阵 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0961`
//!
//! **判据（锚点原文）**：六主题、六代覆盖、特性盘点、十组规划、体系复用、判据。
//!
//! **职责定位（锚点原文）**：GPU 兼容矩阵二域（F0961-F1120，官方六主题：
//! NVIDIA 全系 / ARM Mali / Adreno / 火线兼容 / 老旧 GPU 支持下限 / 认证发布）；
//! NVIDIA 总览（RTX 40/30/20 系 + GTX 16/10 系全覆盖——六代跨度）；
//! NVIDIA 特性面（RT Core / Tensor（DLSS）/ NVENC / G-Sync——特性盘点）；
//! 域结构（G01-G03 NVIDIA / G04-G05 Mali / Adreno / G06 火线兼容 / G07 老旧
//! 下限 / G08 认证发布 / G09 场景扩展 / G10 域收口——十组规划）；
//! F01 认证体系复用声明。
//!
//! # 一、兼容矩阵是**分组账**不是清单：六主题先把问题分完
//!
//! GPU 兼容不是一个「支持/不支持」能回答的问题：NVIDIA 桌面全系、ARM 移动
//! 两家（Mali/Adreno）、无 API 的火线兼容路径、老设备保底、以及最终对外
//! 承诺的认证发布——六类问题各有各的证据形态与判据口径。锚点六主题做成
//! 闭集枚举（[`GpuTheme`]），后续 160 个单号的产出都挂进主题位——主题位
//! 缺号或串号，域收口（G10）时对不齐账。
//!
//! # 二、六代覆盖是**矩阵**不是括号：代际×特性逐格登记
//!
//! 「RTX 40/30/20 + GTX 16/10 全覆盖」的诚实写法是逐格支持态：RT Core 在
//! GTX 10 系是**没有**（硬件缺席），DLSS 在 RTX 20 是 Partial（首代），
//! NVENC 全系有但代际不同。用三态（[`FeatSupport`]：Yes/Partial/No）而非
//! bool——「部分支持」单列是 F1185 纪律（不做不假装）在兼容矩阵的形态。
//! 六代跨度表（[`GENERATION_TABLE`]）+ 特性盘点表（[`FEATURE_TABLE`]）是
//! G01-G03 三组探测单的数据底座。
//!
//! # 三、十组规划是**区间账**：G01-G10 各管一段，区间互斥可机检
//!
//! 160 个单号（F0961-F1120）分十组（[`GROUP_PLAN`]），每组的单号区间
//! 显式登记且**互斥**——区间重叠会让两个组同时认领同一单，区间缝隙会让
//! 单号无组可归。判据侧逐对相邻组断言 `next.first > cur.last`，规划错误
//! 在开工日就红，而不是收口日。
//!
//! # 四、F01 认证体系复用：不另造认证协议，只做域内实例化
//!
//! 认证发布主题（G08）复用 F01 认证体系（五态门）——复用声明表
//! （[`CERTIFICATION_REUSE`]）逐条登记复用点、对端与实例化差异。
//! 「复用」若不锚定对端就是另起炉灶的遮羞布，判据对每行对端非空断言。
//!
//! ## 错误契约：独占 0x61 细分段（vc21 用 0x21，本域 G 开工占用 0x61）
//!
//! 零 panic 面：全部表驱动 + `Option`/`Result`，无 `unwrap`/`expect`。

use alloc::format;
use alloc::string::String;

// 错误契约：独占 0x61 细分段。
pub const E_G061_THEME_UNKNOWN: u16 = 0x6100;
pub const E_G061_GEN_UNKNOWN: u16 = 0x6101;
pub const E_G061_FEATURE_UNKNOWN: u16 = 0x6102;
pub const E_G061_GROUP_RANGE_INVALID: u16 = 0x6103;
pub const E_G061_REUSE_UNANCHORED: u16 = 0x6104;

// ===========================================================================
// 一、六主题闭集（判据一）
// ===========================================================================

/// GPU 兼容矩阵六主题（闭集；后续单号的产出都挂进主题位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuTheme {
    /// NVIDIA 全系。
    Nvidia,
    /// ARM Mali。
    Mali,
    /// ARM Adreno。
    Adreno,
    /// 火线兼容（无原生 API 路径）。
    Firebrand,
    /// 老旧 GPU 支持下限。
    LegacyFloor,
    /// 认证发布。
    CertifiedRelease,
}

impl GpuTheme {
    /// 全集（顺序即主题位口径）。
    pub const ALL: [GpuTheme; 6] = [
        GpuTheme::Nvidia,
        GpuTheme::Mali,
        GpuTheme::Adreno,
        GpuTheme::Firebrand,
        GpuTheme::LegacyFloor,
        GpuTheme::CertifiedRelease,
    ];

    /// 短码。
    pub const fn wire(self) -> &'static str {
        match self {
            GpuTheme::Nvidia => "nvidia",
            GpuTheme::Mali => "mali",
            GpuTheme::Adreno => "adreno",
            GpuTheme::Firebrand => "firebrand",
            GpuTheme::LegacyFloor => "legacy-floor",
            GpuTheme::CertifiedRelease => "certified-release",
        }
    }

    /// 由短码反查（未知短码 `None`）。
    pub fn from_wire(s: &str) -> Option<GpuTheme> {
        Some(match s {
            "nvidia" => GpuTheme::Nvidia,
            "mali" => GpuTheme::Mali,
            "adreno" => GpuTheme::Adreno,
            "firebrand" => GpuTheme::Firebrand,
            "legacy-floor" => GpuTheme::LegacyFloor,
            "certified-release" => GpuTheme::CertifiedRelease,
            _ => return None,
        })
    }
}

// ===========================================================================
// 二、六代覆盖（判据二：RTX 40/30/20 + GTX 16/10 + Pascal 前代 = 六代跨度）
// ===========================================================================

/// 架构代际（闭集；顺序从新到旧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenId {
    /// RTX 50（Blackwell）。
    Rtx50,
    /// RTX 40（Ada Lovelace）。
    Rtx40,
    /// RTX 30（Ampere）。
    Rtx30,
    /// RTX 20（Turing，RT 首代）。
    Rtx20,
    /// GTX 16（Turing，无 RT 核心）。
    Gtx16,
    /// GTX 10（Pascal）。
    Gtx10,
}

impl GenId {
    /// 全集（六代跨度）。
    pub const ALL: [GenId; 6] = [
        GenId::Rtx50,
        GenId::Rtx40,
        GenId::Rtx30,
        GenId::Rtx20,
        GenId::Gtx16,
        GenId::Gtx10,
    ];

    /// 短码。
    pub const fn wire(self) -> &'static str {
        match self {
            GenId::Rtx50 => "rtx50",
            GenId::Rtx40 => "rtx40",
            GenId::Rtx30 => "rtx30",
            GenId::Rtx20 => "rtx20",
            GenId::Gtx16 => "gtx16",
            GenId::Gtx10 => "gtx10",
        }
    }

    /// 架构名（人读）。
    pub const fn arch(self) -> &'static str {
        match self {
            GenId::Rtx50 => "Blackwell",
            GenId::Rtx40 => "Ada Lovelace",
            GenId::Rtx30 => "Ampere",
            GenId::Rtx20 => "Turing",
            GenId::Gtx16 => "Turing GTX",
            GenId::Gtx10 => "Pascal",
        }
    }

    /// 由短码反查。
    pub fn from_wire(s: &str) -> Option<GenId> {
        Some(match s {
            "rtx50" => GenId::Rtx50,
            "rtx40" => GenId::Rtx40,
            "rtx30" => GenId::Rtx30,
            "rtx20" => GenId::Rtx20,
            "gtx16" => GenId::Gtx16,
            "gtx10" => GenId::Gtx10,
            _ => return None,
        })
    }
}

// ===========================================================================
// 三、特性盘点（判据三：三态支持矩阵，不做不假装）
// ===========================================================================

/// 特性支持三态（F1185：部分支持单列，混进支持里就是撒谎）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatSupport {
    /// 支持。
    Yes,
    /// 部分支持（代际/规格边界已声明）。
    Partial,
    /// 硬件缺席。
    No,
}

/// 特性 id（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureId {
    /// RT Core（光追加速）。
    RtCore,
    /// Tensor 核心（DLSS 载体）。
    Tensor,
    /// NVENC（编码单元）。
    Nvenc,
    /// G-Sync Compatible。
    GSync,
}

impl FeatureId {
    /// 全集（锚点四特性）。
    pub const ALL: [FeatureId; 4] = [
        FeatureId::RtCore,
        FeatureId::Tensor,
        FeatureId::Nvenc,
        FeatureId::GSync,
    ];

    /// 短码。
    pub const fn wire(self) -> &'static str {
        match self {
            FeatureId::RtCore => "rt-core",
            FeatureId::Tensor => "tensor",
            FeatureId::Nvenc => "nvenc",
            FeatureId::GSync => "g-sync",
        }
    }
}

/// 一格特性支持（代×特性），带边界说明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeatureCell {
    /// 代。
    pub gen: GenId,
    /// 特性。
    pub feature: FeatureId,
    /// 支持态。
    pub support: FeatSupport,
    /// 边界说明（No/Partial 必非空；Yes 说明代际口径）。
    pub note: &'static str,
}

/// 特性盘点表（6 代 × 4 特性 = 24 格全登记——缺格=探测缺口）。
pub const FEATURE_TABLE: [FeatureCell; 24] = [
    FeatureCell { gen: GenId::Rtx50, feature: FeatureId::RtCore, support: FeatSupport::Yes, note: "第四代 RT 核心" },
    FeatureCell { gen: GenId::Rtx50, feature: FeatureId::Tensor, support: FeatSupport::Yes, note: "第五代 Tensor；DLSS 4" },
    FeatureCell { gen: GenId::Rtx50, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第九代 NVENC" },
    FeatureCell { gen: GenId::Rtx50, feature: FeatureId::GSync, support: FeatSupport::Yes, note: "原生 G-Sync + Compatible" },
    FeatureCell { gen: GenId::Rtx40, feature: FeatureId::RtCore, support: FeatSupport::Yes, note: "第三代 RT 核心" },
    FeatureCell { gen: GenId::Rtx40, feature: FeatureId::Tensor, support: FeatSupport::Yes, note: "第四代 Tensor；DLSS 3 帧生成" },
    FeatureCell { gen: GenId::Rtx40, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第八代 NVENC；AV1" },
    FeatureCell { gen: GenId::Rtx40, feature: FeatureId::GSync, support: FeatSupport::Yes, note: "原生 G-Sync" },
    FeatureCell { gen: GenId::Rtx30, feature: FeatureId::RtCore, support: FeatSupport::Yes, note: "第二代 RT 核心" },
    FeatureCell { gen: GenId::Rtx30, feature: FeatureId::Tensor, support: FeatSupport::Yes, note: "第三代 Tensor；DLSS 2" },
    FeatureCell { gen: GenId::Rtx30, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第七代 NVENC" },
    FeatureCell { gen: GenId::Rtx30, feature: FeatureId::GSync, support: FeatSupport::Yes, note: "原生 G-Sync" },
    FeatureCell { gen: GenId::Rtx20, feature: FeatureId::RtCore, support: FeatSupport::Partial, note: "第一代 RT 核心：规模小，重负载光追降级明显" },
    FeatureCell { gen: GenId::Rtx20, feature: FeatureId::Tensor, support: FeatSupport::Partial, note: "第二代 Tensor：DLSS 首代，质量有界" },
    FeatureCell { gen: GenId::Rtx20, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第六代 NVENC" },
    FeatureCell { gen: GenId::Rtx20, feature: FeatureId::GSync, support: FeatSupport::Yes, note: "原生 G-Sync" },
    FeatureCell { gen: GenId::Gtx16, feature: FeatureId::RtCore, support: FeatSupport::No, note: "硬件缺席：Turing GTX 无 RT 核心" },
    FeatureCell { gen: GenId::Gtx16, feature: FeatureId::Tensor, support: FeatSupport::No, note: "硬件缺席：无 Tensor 核心，DLSS 不可用" },
    FeatureCell { gen: GenId::Gtx16, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第六代 NVENC（与 RTX 20 同代）" },
    FeatureCell { gen: GenId::Gtx16, feature: FeatureId::GSync, support: FeatSupport::Partial, note: "仅 G-Sync Compatible（显示器端认证）" },
    FeatureCell { gen: GenId::Gtx10, feature: FeatureId::RtCore, support: FeatSupport::No, note: "硬件缺席：Pascal 无 RT 核心" },
    FeatureCell { gen: GenId::Gtx10, feature: FeatureId::Tensor, support: FeatSupport::No, note: "硬件缺席：无 Tensor 核心" },
    FeatureCell { gen: GenId::Gtx10, feature: FeatureId::Nvenc, support: FeatSupport::Yes, note: "第六代 NVENC（Pascal 末期型号）" },
    FeatureCell { gen: GenId::Gtx10, feature: FeatureId::GSync, support: FeatSupport::Partial, note: "仅 G-Sync Compatible" },
];

/// 查一格（越界/未知返回错误码——封闭查询）。
pub fn feature_cell(gen: GenId, feature: FeatureId) -> Result<FeatureCell, u16> {
    let mut k = 0usize;
    while k < FEATURE_TABLE.len() {
        let c = &FEATURE_TABLE[k];
        if c.gen == gen && c.feature == feature {
            return Ok(*c);
        }
        k += 1;
    }
    Err(E_G061_FEATURE_UNKNOWN)
}

// ===========================================================================
// 四、十组规划（判据四：G01-G10 区间互斥可机检）
// ===========================================================================

/// 一组规划行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupPlanRow {
    /// 组名（G01…G10）。
    pub group: &'static str,
    /// 单号区间（含端点）。
    pub first: u32,
    pub last: u32,
    /// 职责（一句话）。
    pub duty: &'static str,
    /// 挂靠主题位。
    pub theme: GpuTheme,
}

/// 十组规划表（G01-G10；区间互斥、连续覆盖 F0961-F1120）。
pub const GROUP_PLAN: [GroupPlanRow; 10] = [
    GroupPlanRow { group: "G01", first: 961, last: 970, duty: "NVIDIA 总览与能力探测", theme: GpuTheme::Nvidia },
    GroupPlanRow { group: "G02", first: 971, last: 990, duty: "NVIDIA 特性面与 DLSS/NVENC 深化", theme: GpuTheme::Nvidia },
    GroupPlanRow { group: "G03", first: 991, last: 1010, duty: "NVIDIA 火线兼容与驱动面", theme: GpuTheme::Nvidia },
    GroupPlanRow { group: "G04", first: 1011, last: 1030, duty: "ARM Mali 全系", theme: GpuTheme::Mali },
    GroupPlanRow { group: "G05", first: 1031, last: 1050, duty: "ARM Adreno 全系", theme: GpuTheme::Adreno },
    GroupPlanRow { group: "G06", first: 1051, last: 1070, duty: "火线兼容通用路径", theme: GpuTheme::Firebrand },
    GroupPlanRow { group: "G07", first: 1071, last: 1085, duty: "老旧 GPU 支持下限", theme: GpuTheme::LegacyFloor },
    GroupPlanRow { group: "G08", first: 1086, last: 1100, duty: "认证发布", theme: GpuTheme::CertifiedRelease },
    GroupPlanRow { group: "G09", first: 1101, last: 1110, duty: "场景扩展", theme: GpuTheme::Nvidia },
    GroupPlanRow { group: "G10", first: 1111, last: 1120, duty: "域收口", theme: GpuTheme::CertifiedRelease },
];

/// 规划区间审计：十组序连续、区间互斥（相邻组 next.first > cur.last）、
/// 首尾恰覆盖 F0961-F1120、职责与主题位非空。
pub fn group_plan_audit() -> Result<(), u16> {
    if GROUP_PLAN.len() != 10 {
        return Err(E_G061_GROUP_RANGE_INVALID);
    }
    let mut k = 0usize;
    while k < GROUP_PLAN.len() {
        let row = &GROUP_PLAN[k];
        if row.first > row.last || row.duty.is_empty() {
            return Err(E_G061_GROUP_RANGE_INVALID);
        }
        if GpuTheme::from_wire(row.theme.wire()).is_none() {
            return Err(E_G061_THEME_UNKNOWN);
        }
        if k + 1 < GROUP_PLAN.len() && GROUP_PLAN[k + 1].first <= row.last {
            return Err(E_G061_GROUP_RANGE_INVALID);
        }
        k += 1;
    }
    if GROUP_PLAN[0].first != 961 || GROUP_PLAN[9].last != 1120 {
        return Err(E_G061_GROUP_RANGE_INVALID);
    }
    Ok(())
}

// ===========================================================================
// 五、F01 认证体系复用声明（判据五）
// ===========================================================================

/// 一条复用声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseRow {
    /// 本域复用点。
    pub reuse_point: &'static str,
    /// 对端（F01 体系内的具体机制）。
    pub counterpart: &'static str,
    /// 实例化差异（复用不是照抄——差异要写清楚）。
    pub delta: &'static str,
}

/// F01 认证体系复用声明表（G08 认证发布的底座）。
pub const CERTIFICATION_REUSE: [ReuseRow; 3] = [
    ReuseRow {
        reuse_point: "GPU 认证五态门",
        counterpart: "F01 认证体系：五态门（未测/测试中/通过/有条件通过/失败）",
        delta: "GPU 侧的『测试』= 兼容矩阵逐格探测；五态语义与状态机原样复用",
    },
    ReuseRow {
        reuse_point: "认证证据归档",
        counterpart: "F01 认证体系：证据归档与复核留痕",
        delta: "证据单元从『用例通过记录』换为『SKU×驱动版本探测记录』",
    },
    ReuseRow {
        reuse_point: "认证失效与重认",
        counterpart: "F01 认证体系：条件通过带失效条款，失效即重认",
        delta: "GPU 侧失效触发源增加『驱动大版本更新』（驱动可改变能力面）",
    },
];

/// 复用声明审计：三行全在、对端与差异全非空（复用不锚定对端=另起炉灶）。
pub fn reuse_audit() -> Result<(), u16> {
    if CERTIFICATION_REUSE.len() != 3 {
        return Err(E_G061_REUSE_UNANCHORED);
    }
    let mut k = 0usize;
    while k < CERTIFICATION_REUSE.len() {
        let r = &CERTIFICATION_REUSE[k];
        if r.reuse_point.is_empty() || r.counterpart.is_empty() || r.delta.is_empty() {
            return Err(E_G061_REUSE_UNANCHORED);
        }
        if !r.counterpart.contains("F01") {
            return Err(E_G061_REUSE_UNANCHORED);
        }
        k += 1;
    }
    Ok(())
}

/// 域版本（G 域开工标识）。
pub const GDOMAIN_VERSION: &str = "GDOM-G01-v1";

/// 摘要行（面板/日志/读屏共用）。
pub fn screen_line() -> String {
    format!(
        "{} themes={} gens={} cells={} groups={}",
        GDOMAIN_VERSION,
        GpuTheme::ALL.len(),
        GenId::ALL.len(),
        FEATURE_TABLE.len(),
        GROUP_PLAN.len(),
    )
}
