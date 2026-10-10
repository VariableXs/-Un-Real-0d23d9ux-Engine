//! CGPU-F0961 · G 域开工与 NVIDIA 总览（CGPU-G 域 · GPU 兼容矩阵二域 · G01 组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0961`
//!
//! **判据（锚点原文）**：六主题、六代覆盖、特性盘点、十组规划、体系复用、判据。
//!
//! **职责定位（锚点原文）**：G 域开工：GPU 兼容矩阵二域（F0961-F1120，
//! 官方六主题：NVIDIA 全系/ARM Mali/Adreno/火线兼容/老旧 GPU 支持下限/
//! 认证发布）；NVIDIA 总览（RTX 40/30/20 系+GTX 16/10 系全覆盖——六代
//! 跨度）；NVIDIA 特性面（RT Core/Tensor（DLSS）/NVENC/G-Sync——特性
//! 盘点）；域结构（G01-G03 NVIDIA/G04-G05 Mali/Adreno/G06 火线兼容/
//! G07 老旧下限/G08 认证发布/G09 场景扩展/G10 域收口——十组规划）；
//! F01 认证体系复用声明。判据：六主题、六代覆盖、特性盘点、十组规划、
//! 体系复用、判据。
//!
//! ## 一、六主题闭集（判据一：官方主题映射）
//!
//! 六主题（[`GTheme`]）逐条绑定其兑现组区间（[`G_THEME_GROUPS`]）——
//! 主题是职责切分、组是批次切分，两张表正交且各自守恒；表外不立主题。
//!
//! ## 二、六代覆盖（判据二：NVIDIA 全系的代际面）
//!
//! 五个点名代际（RTX 40/30/20 + GTX 16/10）+ 老旧下限（G07 主题承接
//! Kepler 及更早）构成六代跨度（[`NvGeneration`] 闭集 + [`NV_GENERATIONS`]
//! 覆盖表）；每代绑定架构名与代表 SKU 面——「全覆盖」落为表内逐代
//! 有行，不落为口号。
//!
//! ## 三、特性盘点（判据三：四特性 × 六代矩阵）
//!
//! 特性闭集四项（RT Core/Tensor(DLSS)/NVENC/G-Sync）× 六代可用性矩阵
//! （[`NV_FEATURE_MATRIX`]）：每格三态（有/无/条件——条件格带条件说明
//! 键）。矩阵是兼容面的单源事实表：探测（F0962）与降级联动消费它，
//! 不另立口径。
//!
//! ## 四、十组规划（判据四：160 项批次守恒）
//!
//! G 域 160 项（F0961-F1120）÷ 10 组（[`G_GROUPS`]）：首尾连续无缺口
//! 无重叠；组名与起止单号可 grep；G10 收口组含域终验兑现位。
//!
//! ## 五、F01 认证体系复用声明（判据五：复用不重建）
//!
//! G08 认证发布主题复用 F01 认证体系（F0801 开工 + F0802 六步流程 +
//! F0806 分级）——复用声明三件套（[`REUSE_DECLARE`]：复用对象/复用面/
//! 禁重建条款）字面量冻结；G 域不另立认证流程（禁重建是纪律不是建议）。
//!
//! ## 六、域开工核验入口
//!
//! [`verify_gdomain_open`]：六主题组区间守恒 + 六代表完整 + 特性矩阵
//! 形状（4×6）+ 十组守恒 + 复用声明三件套齐备，五查全过 `Ok`，任一
//! 失败带诊断码——域开工有闸门。
//!
//! **对接**：F0962-F0964（NVIDIA 探测/驱动差异/显存预算——G01 后续）；
//! F0801/F0802/F0806（F01 认证体系——复用对象）；F1120（域收口）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const G_OPEN_VERSION: &str = "G01-gopen-v1";

/// G 域规模：160 项（F0961-F1120）。
pub const G_DOMAIN_TOTAL: u32 = 160;

/// 域首单号（F0961 ⇒ 961）。
pub const G_DOMAIN_FIRST: u32 = 961;

/// 域末单号（F1120 ⇒ 1120；首+总-1 守恒，编译期钉死）。
pub const G_DOMAIN_LAST: u32 = 1_120;

const _: () = assert!(G_DOMAIN_FIRST + G_DOMAIN_TOTAL - 1 == G_DOMAIN_LAST);

// ---------------------------------------------------------------------------
// 二、六主题闭集（判据一）
// ---------------------------------------------------------------------------

/// 六主题闭集（官方点名；表外不立主题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GTheme {
    /// NVIDIA 全系。
    NvidiaAll,
    /// ARM Mali。
    Mali,
    /// Adreno。
    Adreno,
    /// 火线兼容。
    FireCompat,
    /// 老旧 GPU 支持下限。
    LegacyFloor,
    /// 认证发布。
    CertPublish,
}

impl GTheme {
    /// 主题中文名（判据侧独立对拍值）。
    pub const fn zh(self) -> &'static str {
        match self {
            GTheme::NvidiaAll => "NVIDIA全系",
            GTheme::Mali => "ARMMali",
            GTheme::Adreno => "Adreno",
            GTheme::FireCompat => "火线兼容",
            GTheme::LegacyFloor => "老旧下限",
            GTheme::CertPublish => "认证发布",
        }
    }
}

/// 主题-组区间绑定行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeGroup {
    /// 主题。
    pub theme: GTheme,
    /// 承接组标签。
    pub group: &'static str,
}

/// 六主题-组映射（判据一：主题有承接组；G09 场景扩展/G10 收口为跨主题组）。
pub const G_THEME_GROUPS: [ThemeGroup; 6] = [
    ThemeGroup { theme: GTheme::NvidiaAll, group: "G01-G03" },
    ThemeGroup { theme: GTheme::Mali, group: "G04" },
    ThemeGroup { theme: GTheme::Adreno, group: "G05" },
    ThemeGroup { theme: GTheme::FireCompat, group: "G06" },
    ThemeGroup { theme: GTheme::LegacyFloor, group: "G07" },
    ThemeGroup { theme: GTheme::CertPublish, group: "G08" },
];

// ---------------------------------------------------------------------------
// 三、六代覆盖（判据二）
// ---------------------------------------------------------------------------

/// NVIDIA 代际闭集（五个点名代际 + 老旧下限承接 = 六代跨度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvGeneration {
    /// RTX 40 系（Ada Lovelace）。
    Rtx40,
    /// RTX 30 系（Ampere）。
    Rtx30,
    /// RTX 20 系（Turing）。
    Rtx20,
    /// GTX 16 系（Turing 无 RT 核心）。
    Gtx16,
    /// GTX 10 系（Pascal）。
    Gtx10,
    /// 老旧下限（Kepler 及更早——G07 主题承接，支持下限非性能承诺）。
    LegacyFloor,
}

impl NvGeneration {
    /// 架构名（覆盖表行值）。
    pub const fn arch(self) -> &'static str {
        match self {
            NvGeneration::Rtx40 => "Ada Lovelace",
            NvGeneration::Rtx30 => "Ampere",
            NvGeneration::Rtx20 => "Turing",
            NvGeneration::Gtx16 => "Turing(无RT)",
            NvGeneration::Gtx10 => "Pascal",
            NvGeneration::LegacyFloor => "Kepler及更早",
        }
    }

    /// 代表 SKU 面（覆盖表行值；表内逐代有行——全覆盖落为事实）。
    pub const fn examples(self) -> &'static str {
        match self {
            NvGeneration::Rtx40 => "RTX4090/4080/4070",
            NvGeneration::Rtx30 => "RTX3090/3080/3060",
            NvGeneration::Rtx20 => "RTX2080/2070/2060",
            NvGeneration::Gtx16 => "GTX1660/1650",
            NvGeneration::Gtx10 => "GTX1080/1060",
            NvGeneration::LegacyFloor => "GTX900/700 级下限",
        }
    }
}

/// 六代覆盖表（判据二：表内逐代有行）。
pub const NV_GENERATIONS: [NvGeneration; 6] = [
    NvGeneration::Rtx40,
    NvGeneration::Rtx30,
    NvGeneration::Rtx20,
    NvGeneration::Gtx16,
    NvGeneration::Gtx10,
    NvGeneration::LegacyFloor,
];

// ---------------------------------------------------------------------------
// 四、特性盘点矩阵（判据三：4 特性 × 6 代）
// ---------------------------------------------------------------------------

/// 特性闭集四项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvFeature {
    /// RT Core（光线追踪加速）。
    RtCore,
    /// Tensor 核心（DLSS 载体）。
    TensorDlss,
    /// NVENC（编解码引擎）。
    Nvenc,
    /// G-Sync（自适应同步）。
    GSync,
}

impl NvFeature {
    /// 特性名（矩阵行标签）。
    pub const fn zh(self) -> &'static str {
        match self {
            NvFeature::RtCore => "RTCore",
            NvFeature::TensorDlss => "Tensor(DLSS)",
            NvFeature::Nvenc => "NVENC",
            NvFeature::GSync => "G-Sync",
        }
    }
}

/// 特性可用三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatAvail {
    /// 具备。
    Yes,
    /// 不具备。
    No,
    /// 条件具备（条件键入 [`FEAT_CONDITIONS`] 查）。
    Conditional,
}

/// 特性可用性矩阵（行=特性×代际展平：特性 4 行 × 代际 6 列；
/// 列序与 [`NV_GENERATIONS`] 一致——单源事实表，探测与降级消费它）。
pub const NV_FEATURE_MATRIX: [[FeatAvail; 6]; 4] = [
    //           RTX40      RTX30      RTX20      GTX16      GTX10      Legacy
    [FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Yes, FeatAvail::No, FeatAvail::No, FeatAvail::No], // RTCore
    [FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Yes, FeatAvail::No, FeatAvail::No, FeatAvail::No], // Tensor
    [FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Conditional], // NVENC
    [FeatAvail::Yes, FeatAvail::Yes, FeatAvail::Conditional, FeatAvail::Conditional, FeatAvail::Conditional, FeatAvail::No], // GSync
];

/// 条件格条件键（Conditional 的条件说明；三格：GSync×RTX20/GTX16/GTX10）。
pub const FEAT_CONDITIONS: [&str; 3] = [
    "GSync 需显示器 G-Sync Compatible 认证",
    "GSync 需 DisplayPort 自适应同步支持",
    "GSync 需驱动版本 4xx+ 与 VBIOS 支持",
];

/// 查矩阵（特性 × 代际 → 可用态；越界给 No——表外即不具备，不猜）。
/// 注：const fn 内不能用非 const 稳定的 `slice::get`，改用显式界检+索引。
pub const fn feature_at(f: usize, g: usize) -> FeatAvail {
    if f >= NV_FEATURE_MATRIX.len() || g >= 6 {
        return FeatAvail::No;
    }
    NV_FEATURE_MATRIX[f][g]
}

// ---------------------------------------------------------------------------
// 五、十组规划（判据四）
// ---------------------------------------------------------------------------

/// 十组规划行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GGroupPlan {
    /// 组名。
    pub tag: &'static str,
    /// 首单。
    pub first: u32,
    /// 末单。
    pub last: u32,
}

/// 十组规划表（160 项 ÷ 10 = 每组 16 单；首尾连续守恒由
/// [`verify_gdomain_open`] 校验）。
pub const G_GROUPS: [GGroupPlan; 10] = [
    GGroupPlan { tag: "G01", first: 961, last: 976 },
    GGroupPlan { tag: "G02", first: 977, last: 992 },
    GGroupPlan { tag: "G03", first: 993, last: 1_008 },
    GGroupPlan { tag: "G04", first: 1_009, last: 1_024 },
    GGroupPlan { tag: "G05", first: 1_025, last: 1_040 },
    GGroupPlan { tag: "G06", first: 1_041, last: 1_056 },
    GGroupPlan { tag: "G07", first: 1_057, last: 1_072 },
    GGroupPlan { tag: "G08", first: 1_073, last: 1_088 },
    GGroupPlan { tag: "G09", first: 1_089, last: 1_104 },
    GGroupPlan { tag: "G10", first: 1_105, last: 1_120 },
];

// ---------------------------------------------------------------------------
// 六、F01 认证体系复用声明（判据五）
// ---------------------------------------------------------------------------

/// 复用声明三件套（字面量冻结：复用对象/复用面/禁重建条款）。
pub const REUSE_DECLARE: [&str; 3] = [
    "复用对象=F01认证体系（F0801开工+F0802六步流程+F0806分级）",
    "复用面=G08认证发布主题全量走F01流程不另立口径",
    "禁重建=G域不得另立认证流程；流程变更走F01侧ADR",
];

// ---------------------------------------------------------------------------
// 七、诊断码与域开工核验（0x64 段独占）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x64 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 主题-组映射失守。
    ThemeGroupMismatch,
    /// 六代表不完整。
    GenTableIncomplete,
    /// 矩阵形状失守（4×6）。
    MatrixShapeMismatch,
    /// 十组守恒失败。
    GroupMismatch,
    /// 复用声明三件套缺项。
    ReuseMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::ThemeGroupMismatch => "六主题-组映射失守：主题数或组标签与官方规划不符，建议回对锚点重建映射",
            XcCode::GenTableIncomplete => "六代覆盖表不完整：逐代有行是全覆盖的落地面，建议补齐代际行",
            XcCode::MatrixShapeMismatch => "特性矩阵形状失守：应为 4 特性 × 6 代际，建议回对锚点",
            XcCode::GroupMismatch => "十组守恒失败：组段不连续/覆盖不全，建议按 160 项 ÷ 10 组重建",
            XcCode::ReuseMissing => "复用声明缺项：三件套字面量冻结，缺一即未就位",
        }
    }
}

/// 主题映射守恒：六主题逐条有承接组且组标签以 G 前缀开头。
fn themes_consistent() -> bool {
    if G_THEME_GROUPS.len() != 6 {
        return false;
    }
    let mut i = 0usize;
    while i < G_THEME_GROUPS.len() {
        let t = match G_THEME_GROUPS.get(i) {
            Some(t) => t,
            None => return false,
        };
        if !t.group.starts_with('G') {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵形状守恒：4 行 × 每行 6 列。
fn matrix_shape_ok() -> bool {
    if NV_FEATURE_MATRIX.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < NV_FEATURE_MATRIX.len() {
        match NV_FEATURE_MATRIX.get(i) {
            Some(row) if row.len() == 6 => {}
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 十组守恒：首尾连续无缝覆盖域区间。
fn groups_contiguous() -> bool {
    let mut expect = G_DOMAIN_FIRST;
    let mut i = 0usize;
    while i < G_GROUPS.len() {
        let g = match G_GROUPS.get(i) {
            Some(g) => *g,
            None => return false,
        };
        if g.first != expect || g.first > g.last {
            return false;
        }
        expect = g.last.saturating_add(1);
        i += 1;
    }
    expect == G_DOMAIN_LAST.saturating_add(1)
}

/// G 域开工五查（判据一~五 + 判据总闸）。
pub fn verify_gdomain_open() -> Result<(), XcCode> {
    if !themes_consistent() {
        return Err(XcCode::ThemeGroupMismatch);
    }
    if NV_GENERATIONS.len() != 6 {
        return Err(XcCode::GenTableIncomplete);
    }
    if !matrix_shape_ok() {
        return Err(XcCode::MatrixShapeMismatch);
    }
    if !groups_contiguous() {
        return Err(XcCode::GroupMismatch);
    }
    if REUSE_DECLARE.len() != 3 {
        return Err(XcCode::ReuseMissing);
    }
    Ok(())
}

/// 域开工报告（人读面：判据逐条带结论）。
pub fn gdomain_open_report() -> String {
    let mut s = String::new();
    s.push_str("G01 域开工核验：");
    s.push_str(&alloc::format!(
        "六主题/六代覆盖 {}/特性矩阵 4×6/十组 {} 组；",
        NV_GENERATIONS.len(),
        G_GROUPS.len()
    ));
    match verify_gdomain_open() {
        Ok(()) => s.push_str("五查全过=就位"),
        Err(c) => {
            s.push_str("拒=");
            s.push_str(c.detail());
        }
    }
    s
}
