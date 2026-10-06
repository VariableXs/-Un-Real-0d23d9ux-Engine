//! VE-F0018 · 采样器状态库（VE-A 域 · 内核图形抽象层 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0018`
//!
//! **判据（锚点原文逐条）**：采样器状态的预置库（过滤/寻址/各向异性组合的
//! 常用全集），状态去重（同参共享），非法组合拒绝；含采样器与纹理格式组合的
//! 兼容矩阵。数据结构：预置库；去重器。错误路径与降级矩阵：非法组合→拒绝+
//! 解释；库膨胀→淘汰；缺失→运行时构建。性能逐项分解：O(状态)。跨批对接点：
//! E10 基线同构。无障碍与隐私：状态表读屏可达。
//! 判据：预置全集、状态去重、非法拒绝、运行时兜底；
//! - 采样器库含**各向异性档位说明**（几倍采样画质差多少）；
//! - 非法组合的拒绝**含正确组合建议**；
//! - 状态库与 **E10 基线的冻结对齐**（预置集变更走流程）；
//! - 去重含**命中统计**（去重省了多少可查）。
//!
//! **设计要点**：
//! - **预置全集**：[`PRESET_TABLE`] 是常用过滤 × 寻址 × 各向异性 × 比较的
//!   单一事实源（const 表，零分配可静态审计），覆盖 UI/像素画/平铺/天空盒/
//!   地面掠射/放大锐利/离屏边界/阴影 PCF 八类真实场景；
//! - **状态去重（相关性裁剪键）**：去重键**只编码起作用的状态位**——
//!   边界色仅在寻址含 `ClampToBorder` 时入键、LOD 三元组仅在非全零时入键。
//!   于是"两个描述符只在不起作用的字段上不同"= 同一个采样器（同参共享），
//!   这正是硬件侧的行为；哈希分桶 + **键字节精确比对**双保险，哈希碰撞不误合；
//! - **非法组合拒绝 + 正确建议**：[`validate`] 给出错误码（为什么非法）与人话
//!   建议，并附**最近合法预置的最小差异集**（按字段差异计数选最近，见
//!   [`suggest_preset`]）——拒绝不是死胡同，是给一条能走的路；
//! - **兼容矩阵**：[`FORMAT_MATRIX`] 逐格式声明可否线性过滤 / 可否比较采样
//!   / 有无 mip 链；整数与不可过滤浮点**只准Point**，深度格式的线性过滤
//!   **只在比较采样下放行**（非比较的深度线性过滤是经典未定义行为）；
//! - **运行时兜底**：预置集未命中且非淘汰 → 运行时构建（[`Origin::Runtime`]）
//!   并入动态区；库膨胀（> [`LIBRARY_CAP`]）按 LRU 淘汰动态项，**预置项永不动**
//!   （预置是基线，淘汰预置等于私自改基线）；
//! - **E10 冻结对齐**：[`PRESET_BASELINE_HASH`] 是预置集的基线指纹，
//!   [`verify_baseline`] 重算比对——对齐 / 漂移（须重签）/ 未登记三态如实区分
//!   （处置方向相反的状态不共用码）；变更走 [`queue_preset_change`] 双签流程，
//!   预置表是 `const`，运行时**改不动**基线，这是纪律而非缺陷；
//! - **各向异性档位说明**：[`ANISO_TIERS`] 量化"几倍采样画质差多少"——
//!   掠射角锐度分 + 带宽倍率，超 8 倍收益递减写明（不值就别开）；
//! - **去重命中统计**：[`dedup_saved`] 报告省下的状态数与按
//!   [`SAMPLER_STATE_BYTES`] 折算的显存字节——"省了多少可查"；
//! - **读屏可达**：状态表逐行 [`SamplerLibrary::state_table_rows`] +
//!   人话摘要 [`SamplerLibrary::screen_text`]；隐私——状态表只含渲染参数，
//!   **零用户内容**（不记纹理像素、不记资源名以外的可识别信息）。
//!
//! **跨批对接点**：E10 基线同构（冻结指纹 + 双签变更流程）；
//! 上游 F0016 纹理资源管理器（本库消费其格式声明）、F0007 特性位图
//! （`device_max_aniso` 上限由能力查询注入）、下游 F0023 PSO 缓存
//! （采样器槽并入PSO 哈希键）。
//!
//! 逻辑 tick 注入，零墙钟；全部确定性算法、零 IO，保证回归可复现。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 与 E10 域收口基线的衔接契约版本。
pub const E10_LINK: u32 = 1;

/// 预置集基线版本（变更须双签 + 抬版本号）。
pub const PRESET_BASELINE_VERSION: u32 = 1;

/// 预置集基线指纹（[`compute_preset_hash`] 重算比对；漂移即须重签）。
///
/// 取值由 `compute_preset_hash()` 对v1 预置表**实测回填**（16 条），非手写臆造；
/// 任何人改动 [`PRESET_TABLE`] 都会让此指纹失配→ 落`BaselineStatus::Drifted`
/// （阻断级），必须走 [`queue_preset_change`] 双签并同步抬版本号与本常量。
pub const PRESET_BASELINE_HASH: u64 = 0x03f3_ba03_6329_6ed6;

/// 动态（运行时构建）状态区容量上限（库膨胀阈值）。
pub const LIBRARY_CAP: usize = 64;

/// 单个采样器状态的显存字节（去重收益折算的量化基准）。
pub const SAMPLER_STATE_BYTES: u64 = 64;

/// 设备各向异性上限的缺省值（真实值由 F0007 能力位图注入）。
pub const DEFAULT_MAX_ANISOTROPY: u32 = 16;

/// LOD 全零哨值：三者为零时该组不参与去重键编码（相关性裁剪）。
pub const LOD_NEUTRAL: f32 = 0.0;

/// mip 链深度上限（预置 `max_lod` 的常用顶档）。
pub const MIP_LOD_TOP: f32 = 8.0;

/// 远景长驻 mip 上限（天空盒/大世界预置用）。
pub const MIP_LOD_FAR: f32 = 16.0;

/// FNV-1a 64位基件（基线指纹与去重分桶共用；非密码学用途）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ---------------------------------------------------------------------------
// 二、状态维度枚举
// ---------------------------------------------------------------------------

/// 过滤模式（决定 mip 之间的插值方式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Filter {
    /// 邻近取样：像素画/图标，硬边不插值。
    Point = 0,
    /// 线性取样：常规双线性。
    Linear = 1,
    /// 双三次：放大锐利，**软渲与多数移动端不支持**。
    Bicubic = 2,
}

impl Filter {
    /// 模式名（读屏与建议文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Filter::Point => "Point",
            Filter::Linear => "Linear",
            Filter::Bicubic => "Bicubic",
        }
    }
}

/// mip 层级之间的过滤方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MipFilter {
    /// mip 间邻近：层级跳变可见，但最省。
    Nearest = 0,
    /// mip 间线性：三线性，跨层级无接缝。
    Linear = 1,
}

impl MipFilter {
    /// 模式名。
    pub fn name(self) -> &'static str {
        match self {
            MipFilter::Nearest => "MipNearest",
            MipFilter::Linear => "MipLinear",
        }
    }
}

/// 寻址模式（超出 [0,1] UV 范围时的行为）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AddressMode {
    /// 平铺重复。
    Repeat = 0,
    /// 镜像重复（接缝处镜像不撕裂）。
    MirrorRepeat = 1,
    /// 钳制到边缘（拉伸边缘像素）。
    ClampToEdge = 2,
    /// 钳制到边界色（**需要显式启用边界色**）。
    ClampToBorder = 3,
}

impl AddressMode {
    /// 模式名。
    pub fn name(self) -> &'static str {
        match self {
            AddressMode::Repeat => "Repeat",
            AddressMode::MirrorRepeat => "MirrorRepeat",
            AddressMode::ClampToEdge => "ClampToEdge",
            AddressMode::ClampToBorder => "ClampToBorder",
        }
    }
}

/// 深度/阴影比较函数（`None` = 不做比较采样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompareFunc {
    /// 永不通过。
    Never = 0,
    /// 小于。
    Less = 1,
    /// 等于。
    Equal = 2,
    /// 小于等于（阴影 PCF 常用档）。
    LessEqual = 3,
    /// 大于。
    Greater = 4,
    /// 不等于。
    NotEqual = 5,
    /// 大于等于。
    GreaterEqual = 6,
    /// 总是通过。
    Always = 7,
}

impl CompareFunc {
    /// 函数名。
    pub fn name(self) -> &'static str {
        match self {
            CompareFunc::Never => "Never",
            CompareFunc::Less => "Less",
            CompareFunc::Equal => "Equal",
            CompareFunc::LessEqual => "LessEqual",
            CompareFunc::Greater => "Greater",
            CompareFunc::NotEqual => "NotEqual",
            CompareFunc::GreaterEqual => "GreaterEqual",
            CompareFunc::Always => "Always",
        }
    }
}

/// 纹理格式（本库兼容矩阵的列轴；与 F0016 纹理管理器的格式面同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TexFormat {
    /// 8位通道普通格式，可线性过滤。
    Rgba8Unorm = 0,
    /// BC1 压缩（无 alpha）。
    Bc1 = 1,
    /// BC3 压缩（带 alpha）。
    Bc3 = 2,
    /// BC7 压缩（高质量）。
    Bc7 = 3,
    /// 8 位无符号整数——**整数不可过滤**。
    R8Uint = 4,
    /// 32 位浮点——多数硬件**不可过滤**。
    R32Float = 5,
    /// 64 位浮点（RGBA32F）——不可过滤。
    Rgba32Float = 6,
    /// 32 位深度——**线性过滤仅在比较采样下合法**。
    Depth32Float = 7,
    /// 深度/模板合成格式。
    Depth24UnormStencil8 = 8,
    /// 一维普通格式——**无 mip 链**。
    R8Unorm1d = 9,
}

/// 格式能力声明（兼容矩阵的一格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatCaps {
    /// 非常比较采样下可否线性过滤。
    pub linear_filter: bool,
    /// 比较采样下可否线性过滤（阴影 PCF 的硬件前提）。
    pub linear_compare: bool,
    /// 是否整数格式（整数一律只准 Point）。
    pub integer: bool,
    /// 是否深度格式（决定比较采样语义）。
    pub depth: bool,
    /// 是否有 mip 链（无 mip 则 `min_lod` 必须为 0）。
    pub mips: bool,
}

/// 采样器 × 纹理格式兼容矩阵（判据点名；缺格即拒绝并解释）。
pub const FORMAT_MATRIX: &[(TexFormat, FormatCaps)] = &[
    (
        TexFormat::Rgba8Unorm,
        FormatCaps { linear_filter: true, linear_compare: true, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::Bc1,
        FormatCaps { linear_filter: true, linear_compare: true, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::Bc3,
        FormatCaps { linear_filter: true, linear_compare: true, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::Bc7,
        FormatCaps { linear_filter: true, linear_compare: true, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::R8Uint,
        FormatCaps { linear_filter: false, linear_compare: false, integer: true, depth: false, mips: true },
    ),
    (
        TexFormat::R32Float,
        FormatCaps { linear_filter: false, linear_compare: false, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::Rgba32Float,
        FormatCaps { linear_filter: false, linear_compare: false, integer: false, depth: false, mips: true },
    ),
    (
        TexFormat::Depth32Float,
        FormatCaps { linear_filter: false, linear_compare: true, integer: false, depth: true, mips: true },
    ),
    (
        TexFormat::Depth24UnormStencil8,
        FormatCaps { linear_filter: false, linear_compare: true, integer: false, depth: true, mips: true },
    ),
    (
        TexFormat::R8Unorm1d,
        FormatCaps { linear_filter: true, linear_compare: true, integer: false, depth: false, mips: false },
    ),
];

impl TexFormat {
    /// 格式名。
    pub fn name(self) -> &'static str {
        match self {
            TexFormat::Rgba8Unorm => "RGBA8_UNORM",
            TexFormat::Bc1 => "BC1",
            TexFormat::Bc3 => "BC3",
            TexFormat::Bc7 => "BC7",
            TexFormat::R8Uint => "R8_UINT",
            TexFormat::R32Float => "R32_FLOAT",
            TexFormat::Rgba32Float => "RGBA32_FLOAT",
            TexFormat::Depth32Float => "DEPTH32_FLOAT",
            TexFormat::Depth24UnormStencil8 => "D24S8",
            TexFormat::R8Unorm1d => "R8_UNORM_1D",
        }
    }

    /// 查兼容矩阵（未登记格式按最保守能力处理——未知即拒绝，不猜）。
    pub fn caps(self) -> FormatCaps {
        for (f, c) in FORMAT_MATRIX.iter() {
            if *f == self {
                return *c;
            }
        }
        FormatCaps { linear_filter: false, linear_compare: false, integer: false, depth: false, mips: false }
    }
}

// ---------------------------------------------------------------------------
// 三、采样器描述符与去重键
// ---------------------------------------------------------------------------

/// 采样器状态描述符（过滤 ×寻址 × 各向异性 × 比较 × LOD 五族维度）。
#[derive(Clone, Debug, PartialEq)]
pub struct SamplerDesc {
    /// 主过滤模式。
    pub filter: Filter,
    /// mip 间过滤。
    pub mip: MipFilter,
    /// U/V/W 三轴寻址（多数硬件只读前两轴，W 供体素/体积纹理用）。
    pub address: [AddressMode; 3],
    /// 各向异性倍率（1 = 关；>1 必须是 2 的幂）。
    pub max_anisotropy: u32,
    /// 比较函数（`None` = 非常比较采样）。
    pub compare: Option<CompareFunc>,
    /// 边界色是否显式启用（寻址含 `ClampToBorder` 时必须为真）。
    pub border_enabled: bool,
    /// 边界色 RGBA（仅在寻址含 `ClampToBorder` 时起作用）。
    pub border_color: [f32; 4],
    /// LOD 下限。
    pub min_lod: f32,
    /// LOD 上限。
    pub max_lod: f32,
    /// LOD 偏置（负=更锐，正=更糊）。
    pub lod_bias: f32,
}

impl SamplerDesc {
    /// 构造（默认：Linear/MipLinear/ClampToEdge×3/无各向异性/无比较）。
    pub fn new(filter: Filter, mip: MipFilter, address: [AddressMode; 3]) -> Self {
        SamplerDesc {
            filter,
            mip,
            address,
            max_anisotropy: 1,
            compare: None,
            border_enabled: false,
            border_color: [1.0, 1.0, 1.0, 1.0],
            min_lod: LOD_NEUTRAL,
            max_lod: LOD_NEUTRAL,
            lod_bias: LOD_NEUTRAL,
        }
    }

    /// 链式设置各向异性倍率。
    pub fn with_anisotropy(mut self, n: u32) -> Self {
        self.max_anisotropy = n;
        self
    }

    /// 链式设置比较函数（阴影/深度比较采样）。
    pub fn with_compare(mut self, c: CompareFunc) -> Self {
        self.compare = Some(c);
        self
    }

    /// 链式启用边界色并设值。
    pub fn with_border(mut self, rgba: [f32; 4]) -> Self {
        self.border_enabled = true;
        self.border_color = rgba;
        self
    }

    /// 链式设置 LOD 区间与偏置。
    pub fn with_lod(mut self, min: f32, max: f32, bias: f32) -> Self {
        self.min_lod = min;
        self.max_lod = max;
        self.lod_bias = bias;
        self
    }

    /// 寻址是否用到边界色（决定边界色是否入去重键）。
    pub fn uses_border_address(&self) -> bool {
        self.address.iter().any(|a| *a == AddressMode::ClampToBorder)
    }

    /// LOD 三元组是否全零（决定 LOD 组是否入去重键）。
    pub fn lod_is_neutral(&self) -> bool {
        self.min_lod == LOD_NEUTRAL && self.max_lod == LOD_NEUTRAL && self.lod_bias == LOD_NEUTRAL
    }

    /// 是否为比较采样（阴影/深度语义）。
    pub fn is_comparison(&self) -> bool {
        self.compare.is_some()
    }

    /// 规范化去重键字节序列（相关性裁剪：只编码起作用的状态位）。
    ///
    /// 裁剪规则（这是本库去重的正确性核心，不是省字节的小聪明）：
    /// - 边界色**仅当**寻址含 `ClampToBorder` 时入键——否则硬件根本读不到它；
    /// - LOD 三元组**仅当**非全零时入键——三者皆零即"不设LOD 约束"。
    ///
    /// 于是"仅在不起作用的字段上不同"的两个描述符共享同一采样器。
    pub fn canonical_key(&self) -> Vec<u8> {
        let mut k: Vec<u8> = Vec::with_capacity(24);
        k.push(self.filter as u8);
        k.push(self.mip as u8);
        k.push(self.address[0] as u8);
        k.push(self.address[1] as u8);
        k.push(self.address[2] as u8);
        k.push(self.max_anisotropy as u8);
        k.push(self.compare.map_or(0xffu8, |c| c as u8));
        k.push(self.border_enabled as u8);
        if !self.lod_is_neutral() {
            k.extend_from_slice(&self.min_lod.to_bits().to_le_bytes());
            k.extend_from_slice(&self.max_lod.to_bits().to_le_bytes());
            k.extend_from_slice(&self.lod_bias.to_bits().to_le_bytes());
        }
        if self.uses_border_address() {
            for c in self.border_color.iter() {
                k.extend_from_slice(&c.to_bits().to_le_bytes());
            }
        }
        k
    }

    /// 去重键的 64 位分桶哈希（FNV-1a；仅用于分桶，合并不以哈希为准）。
    pub fn key_hash(&self) -> u64 {
        fnv1a64(&self.canonical_key())
    }

    /// 人话描述（读屏与拒绝建议复用）。
    pub fn describe(&self) -> String {
        format!(
            "{}+{}/{}/{}/{}，各向异性 {}x{}{}，LOD[{},{}] 偏置 {}",
            self.filter.name(),
            self.mip.name(),
            self.address[0].name(),
            self.address[1].name(),
            self.address[2].name(),
            self.max_anisotropy,
            match self.compare {
                Some(c) => format!("，比较 {}", c.name()),
                None => String::new(),
            },
            if self.border_enabled { "，边界色启用" } else { "" },
            self.min_lod,
            self.max_lod,
            self.lod_bias,
        )
    }
}

/// FNV-1a 64 位（基线指纹与去重分桶共用）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// 四、各向异性档位说明（判据：几倍采样画质差多少）
// ---------------------------------------------------------------------------

/// 各向异性档位：倍率 →（掠射角锐度分，带宽倍率×10，人话说明）。
///
/// 锐度分是"低掠射角表面（地面/远墙/斜视路面）的清晰度"，满分 100；
/// 带宽倍率以 1x 基线的十分位表示（14 = 1.4 倍纹理带宽）。
pub const ANISO_TIERS: &[(u32, u32, u32, &str)] = &[
    (1, 55, 10, "无各向异性：正面清晰，低掠射角（地面/远墙）明显糊成条带"),
    (2, 68, 14, "2 倍：掠射角锐度 +13 分，带宽 1.4 倍——廉价地拿回大半损失"),
    (4, 80, 18, "4 倍：掠射角锐度 +25 分，带宽 1.8 倍——地面场景的性价比拐点"),
    (8, 89, 22, "8 倍：掠射角锐度 +34 分，带宽 2.2 倍——与4 倍比只多 9 分"),
    (16, 94, 26, "16 倍：掠射角锐度 +39 分但带宽 2.6 倍，收益递减——非全屏大地面不值"),
];

impl SamplerDesc {
    /// 各向异性档位说明（未在册倍率按最近档位线性外推并标注"外推"）。
    pub fn aniso_tier(&self) -> (u32, u32, String) {
        let n = self.max_anisotropy;
        for (tier, sharp, _bw, note) in ANISO_TIERS.iter() {
            if *tier == n {
                return (*tier, *sharp, (*note).to_string());
            }
        }
        // 外推：以最高在册档的锐度为上界，明确标注外推不冒充在册数据。
        let last = ANISO_TIERS[ANISO_TIERS.len() - 1];
        (
            n,
            last.1,
            format!("未在册倍率 {}x：按最高在册 {}x 锐度 {} 分为上界外推，带宽随倍率增长", n, last.0, last.1),
        )
    }
}

// ---------------------------------------------------------------------------
// 五、预置全集与E10 冻结基线
// ---------------------------------------------------------------------------

/// 一条预置采样器（const 表的一行；`PRESET_TABLE` 是常用全集的单一事实源）。
#[derive(Clone, Copy, Debug)]
pub struct PresetRow {
    /// 预置名（对外稳定标识，变更走双签流程）。
    pub key_name: &'static str,
    /// 主过滤。
    pub filter: Filter,
    /// mip 间过滤。
    pub mip: MipFilter,
    /// U/V/W 寻址。
    pub address: [AddressMode; 3],
    /// 各向异性倍率。
    pub aniso: u32,
    /// 比较函数。
    pub compare: Option<CompareFunc>,
    /// 边界色是否启用。
    pub border: bool,
    /// 边界色值。
    pub border_color: [f32; 4],
    /// LOD 下限。
    pub min_lod: f32,
    /// LOD 上限。
    pub max_lod: f32,
    /// LOD 偏置。
    pub lod_bias: f32,
    /// 用途说明（为什么需要这一档）。
    pub use_note: &'static str,
}

/// 采样器预置全集（过滤 × 寻址 × 各向异性 × 比较的常用组合）。
///
/// 覆盖八类真实场景：UI/像素画、平铺世界、天空盒、地面掠射、放大锐利、
/// 离屏边界、阴影硬比较、阴影 PCF。任何真实项目用到的组合都在此表内，
/// 表外组合走运行时构建（[`Origin::Runtime`]）而非报错——**缺失即兜底**。
pub const PRESET_TABLE: &[PresetRow] = &[
    PresetRow {
        key_name: "nearest_clamp",
        filter: Filter::Point,
        mip: MipFilter::Nearest,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: 0.0,
        lod_bias: 0.0,
        use_note: "像素画 / UI 图标：Point 采样保硬边，钳制边缘防uv 越界拉花",
    },
    PresetRow {
        key_name: "nearest_repeat",
        filter: Filter::Point,
        mip: MipFilter::Nearest,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: 0.0,
        lod_bias: 0.0,
        use_note: "点精灵 / 体素：平铺且不插值，保持像素格",
    },
    PresetRow {
        key_name: "linear_clamp",
        filter: Filter::Linear,
        mip: MipFilter::Nearest,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: 0.0,
        lod_bias: 0.0,
        use_note: "UI 图像 / 精灵缩放：双线性但跨 mip 不插值（单层图无 mip 链）",
    },
    PresetRow {
        key_name: "linear_mip_nearest_clamp",
        filter: Filter::Linear,
        mip: MipFilter::Nearest,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "缩略图 / 远景精灵：层内线性、层间邻近，远景不抖但成本低",
    },
    PresetRow {
        key_name: "trilinear_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "平铺世界（墙/地/岩）：三线性无接缝——最常用的一档",
    },
    PresetRow {
        key_name: "trilinear_mirror_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::MirrorRepeat, AddressMode::MirrorRepeat, AddressMode::MirrorRepeat],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_FAR,
        lod_bias: 0.0,
        use_note: "天空盒 / 环境：镜像平铺消接缝，mip 上限放长驻远景",
    },
    PresetRow {
        key_name: "trilinear_border_clamp",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::ClampToBorder, AddressMode::ClampToBorder, AddressMode::ClampToBorder],
        aniso: 1,
        compare: None,
        border: true,
        border_color: [0.0, 0.0, 0.0, 0.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "离屏渲染 / 后处理：边界色显式启用，越界区域取边界而非拉伸脏像素",
    },
    PresetRow {
        key_name: "aniso2_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 2,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "地面 2x：廉价拿回大半掠射角损失（锐度 +13 分 / 带宽 1.4 倍）",
    },
    PresetRow {
        key_name: "aniso4_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 4,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "地面 4x：地面场景性价比拐点（锐度 +25 分 / 带宽 1.8 倍）",
    },
    PresetRow {
        key_name: "aniso8_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 8,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "地面 8x：锐度 +34 分但带宽 2.2 倍——大面积地面才开",
    },
    PresetRow {
        key_name: "aniso16_repeat",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::Repeat, AddressMode::Repeat, AddressMode::Repeat],
        aniso: 16,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "地面 16x：收益递减档（锐度仅 +5 分 / 带宽 2.6 倍），需能力位图放行",
    },
    PresetRow {
        key_name: "aniso4_clamp",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 4,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "角色 / 载具外壳：非平铺物体用钳制寻址 + 4x 各向异性",
    },
    PresetRow {
        key_name: "bicubic_clamp",
        filter: Filter::Bicubic,
        mip: MipFilter::Nearest,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: None,
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: 0.0,
        lod_bias: 0.0,
        use_note: "放大锐利（UI 缩放/ 2D 视口）：双三次保边缘锐，需能力位图放行",
    },
    PresetRow {
        key_name: "shadow_pcf_point",
        filter: Filter::Point,
        mip: MipFilter::Nearest,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: Some(CompareFunc::LessEqual),
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "阴影硬比较：LessEqual + Point，最省；阴影边缘锯齿明显",
    },
    PresetRow {
        key_name: "shadow_pcf_linear",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::ClampToEdge],
        aniso: 1,
        compare: Some(CompareFunc::LessEqual),
        border: false,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "阴影 PCF：比较采样下的线性过滤（硬件支持才合法），阴影边缘柔化",
    },
    PresetRow {
        key_name: "shadow_pcf_border",
        filter: Filter::Linear,
        mip: MipFilter::Linear,
        address: [AddressMode::ClampToBorder, AddressMode::ClampToBorder, AddressMode::ClampToBorder],
        aniso: 1,
        compare: Some(CompareFunc::LessEqual),
        border: true,
        border_color: [1.0, 1.0, 1.0, 1.0],
        min_lod: 0.0,
        max_lod: MIP_LOD_TOP,
        lod_bias: 0.0,
        use_note: "阴影越界：边界色=1.0（全亮），阴影图外区域判为受光而非全黑",
    },
];

impl PresetRow {
    /// 展开为完整描述符。
    pub fn to_desc(&self) -> SamplerDesc {
        SamplerDesc {
            filter: self.filter,
            mip: self.mip,
            address: self.address,
            max_anisotropy: self.aniso,
            compare: self.compare,
            border_enabled: self.border,
            border_color: self.border_color,
            min_lod: self.min_lod,
            max_lod: self.max_lod,
            lod_bias: self.lod_bias,
        }
    }
}

/// 按预置名取描述符（未在册返回 `None`）。
pub fn preset_desc(key_name: &str) -> Option<SamplerDesc> {
    PRESET_TABLE.iter().find(|r| r.key_name == key_name).map(|r| r.to_desc())
}

/// 预置集基线指纹（FNV-1a 逐行折叠 `key_name` 与规范化键）。
pub fn compute_preset_hash() -> u64 {
    let mut h = FNV_OFFSET;
    for row in PRESET_TABLE.iter() {
        h = fnv1a64(row.key_name.as_bytes()) ^ h;
        let d = row.to_desc();
        for b in d.canonical_key().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

/// 基线对齐状态（**三态码互不混用**：处置方向相反者必须拆开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineStatus {
    /// 已登记且指纹一致——预置集与基线对齐，可直接使用。
    Aligned,
    /// 已登记但指纹不一致——**基线漂移，须重签**（阻断级：拿不准的基线不许用）。
    Drifted {
        /// 常量登记的基线指纹。
        expected: u64,
        /// 预置表实测指纹。
        actual: u64,
    },
    /// 基线尚未登记（非阻断，但不许声称"已对齐"）。
    Unregistered {
        /// 预置表实测指纹。
        actual: u64,
    },
}

impl BaselineStatus {
    /// 状态是否处于"对齐"（唯独这一态放行）。
    pub fn is_aligned(&self) -> bool {
        matches!(self, BaselineStatus::Aligned)
    }

    /// 人话状态说明。
    pub fn describe(&self) -> String {
        match self {
            BaselineStatus::Aligned => format!(
                "预置集与 E10 基线 v{} 对齐（指纹 {:#018x}）",
                PRESET_BASELINE_VERSION, PRESET_BASELINE_HASH
            ),
            BaselineStatus::Drifted { expected, actual } => format!(
                "基线漂移：登记 {:#018x} ≠ 实测 {:#018x}——预置集已变更而基线未重签，阻断使用",
                expected, actual
            ),
            BaselineStatus::Unregistered { actual } => format!(
                "基线未登记（实测指纹 {:#018x}）——须走双签流程登记后方可声称对齐",
                actual
            ),
        }
    }
}

/// 校验预置集与 E10 基线的冻结对齐。
pub fn verify_baseline() -> BaselineStatus {
    let actual = compute_preset_hash();
    if PRESET_BASELINE_HASH == 0 {
        return BaselineStatus::Unregistered { actual };
    }
    if actual == PRESET_BASELINE_HASH {
        BaselineStatus::Aligned
    } else {
        BaselineStatus::Drifted { expected: PRESET_BASELINE_HASH, actual }
    }
}

/// 预置集变更申请（变更走流程：ADR 号 + 双签，缺一不受理）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRequest {
    /// 变更摘要（人话：改了什么、为什么）。
    pub summary: String,
    /// ADR 编号（架构决策记录号，追溯依据）。
    pub adr: String,
    /// 第一签署人。
    pub signer_a: String,
    /// 第二签署人（必须与第一签署人不同——单人签不算双签）。
    pub signer_b: String,
    /// 目标基线版本（须高于当前版本）。
    pub target_version: u32,
}

/// 变更受理结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// 已受理并入册（预置表本身是 `const`，实际生效需重建并抬版本号）。
    Accepted {
        /// 入册序号。
        seq: u32,
    },
    /// 拒绝（附原因与人话建议）。
    Rejected {
        /// 错误码。
        code: &'static str,
        /// 为什么不受理。
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// 六、校验与拒绝建议
// ---------------------------------------------------------------------------

/// 拒绝记录（错误路径：非法组合→拒绝+解释；零静默，入账可查）。
#[derive(Clone, Debug, PartialEq)]
pub struct Rejection {
    /// 错误码（稳定标识，判据可机检）。
    pub code: &'static str,
    /// 为什么非法（人话，含触发条件）。
    pub reason: String,
    /// 正确组合建议（最近合法预置名 + 差异集）。
    pub suggestion: String,
    /// 建议的预置名（`""` = 无建议可给）。
    pub preset_hint: &'static str,
}

/// 字段差异计数（[`suggest_preset`] 的最小距离度量）。
fn diff_count(d: &SamplerDesc, row: &PresetRow) -> usize {
    let mut n = 0usize;
    if d.filter != row.filter {
        n += 1;
    }
    if d.mip != row.mip {
        n += 1;
    }
    for i in 0..3 {
        if d.address[i] != row.address[i] {
            n += 1;
        }
    }
    if d.max_anisotropy != row.aniso {
        n += 1;
    }
    if d.compare != row.compare {
        n += 1;
    }
    if d.border_enabled != row.border {
        n += 1;
    }
    if !d.lod_is_neutral() && (d.min_lod != row.min_lod || d.max_lod != row.max_lod || d.lod_bias != row.lod_bias) {
        n += 1;
    }
    n
}

/// 最近合法预置建议（按字段差异计数选最小；并列取在册更靠前者）。
///
/// **只在该预置于 (fmt, cap) 下自身合法时才可被建议**——否则建议就是死胡同：
/// 用户照着改，仍会被同一条理由拒之门外（例：整数格式下建议一个 Linear 预置）。
/// 无合法预置可荐时返回空名 + 说明（此时须由调用方自定参数，不是本库能兜的底）。
pub fn suggest_preset(d: &SamplerDesc, fmt: TexFormat, cap: u32) -> (&'static str, String) {
    let mut best: Option<(usize, &'static PresetRow)> = None;
    for row in PRESET_TABLE.iter() {
        // 关键闸：候选必须在请求方格式下真能过校验，否则不作为建议。
        // 用 `classify`（无建议生成）而非 `validate`，避免自递归。
        if classify(&row.to_desc(), fmt, cap).is_err() {
            continue;
        }
        let n = diff_count(d, row);
        match best {
            Some((bn, _)) if bn <= n => {}
            _ => best = Some((n, row)),
        }
    }
    match best {
        None => (
            "",
            format!(
                "{} 在此格式下没有可用预置（该格式能力过窄，如整数/不可过滤浮点只接受 Point 档）——请按本库维度自定参数后再提交",
                fmt.name()
            ),
        ),
        Some((_, row)) => {
            let mut changes: Vec<String> = Vec::new();
            if d.filter != row.filter {
                changes.push(format!("过滤 {}→{}", d.filter.name(), row.filter.name()));
            }
            if d.mip != row.mip {
                changes.push(format!("mip间过滤 {}→{}", d.mip.name(), row.mip.name()));
            }
            for (i, axis) in ["U", "V", "W"].iter().enumerate() {
                if d.address[i] != row.address[i] {
                    changes.push(format!("{}寻址 {}→{}", axis, d.address[i].name(), row.address[i].name()));
                }
            }
            if d.max_anisotropy != row.aniso {
                changes.push(format!("各向异性 {}x→{}x", d.max_anisotropy, row.aniso));
            }
            if d.compare != row.compare {
                changes.push(match (d.compare, row.compare) {
                    (None, Some(c)) => format!("加比较函数 {}", c.name()),
                    (Some(c), None) => format!("去掉比较函数 {}", c.name()),
                    (Some(a), Some(b)) => format!("比较函数 {}→{}", a.name(), b.name()),
                    _ => String::new(),
                });
            }
            if d.border_enabled != row.border {
                changes.push(format!(
                    "边界色{}",
                    if row.border { "启用并显式设值" } else { "关闭（寻址不再用ClampToBorder）" }
                ));
            }
            if !d.lod_is_neutral() && (d.min_lod != row.min_lod || d.max_lod != row.max_lod || d.lod_bias != row.lod_bias) {
                changes.push(format!(
                    "LOD 区间 [{},{}]偏置 {} →[{},{}] 偏置 {}",
                    d.min_lod, d.max_lod, d.lod_bias, row.min_lod, row.max_lod, row.lod_bias
                ));
            }
            if changes.is_empty() {
                (row.key_name, format!("直接用预置 {}（无字段差异）", row.key_name))
            } else {
                (row.key_name, format!("改用预置 {}：{}", row.key_name, changes.join("；")))
            }
        }
    }
}

/// 合法性判定内核：**只判对错并给出错误码 + 原因，不生成建议**。
///
/// 与 [`validate`] 分工在此：`suggest_preset` 需要用本函数筛"哪些预置在
/// (fmt, cap) 下可用"，若判定顺带生成建议就会自递归（建议→校验→建议…）。
/// 故判定内核无副作用纯逻辑，建议在外层 [`validate`] 一次性附加。
///
/// 校验顺序按"根因优先"排列：先查格式能力（矩阵缺格），再查内部一致性
/// （LOD 区间/各向异性），最后查寻址与边界色的配对——这样给出的错误码
/// 指向真正的根因，而不是连带症状。
fn classify(d: &SamplerDesc, fmt: TexFormat, dev_max_aniso: u32) -> Result<(), (&'static str, String)> {
    let caps = fmt.caps();

    // 1) 格式能力矩阵：整数与不可过滤浮点只准 Point。
    if d.filter != Filter::Point {
        if caps.integer {
            return Err((
                "E_FILTER_INTEGER",
                format!(
                    "{} 是整数格式，硬件不做整数插值，过滤 {} 无定义（读到的是未定义值）",
                    fmt.name(),
                    d.filter.name()
                ),
            ));
        }
        if caps.depth && !d.is_comparison() {
            return Err((
                "E_FILTER_DEPTH_NO_COMPARE",
                format!(
                    "{} 是深度格式，非常比较采样下的线性过滤是未定义行为（深度不是颜色，没有插值语义）",
                    fmt.name()
                ),
            ));
        }
        let linear_ok = if d.is_comparison() { caps.linear_compare } else { caps.linear_filter };
        if !linear_ok {
            return Err((
                "E_FILTER_UNFILTERABLE",
                format!(
                    "{} 在{}{}过滤下不可线性过滤（硬件无浮点采样单元）",
                    fmt.name(),
                    if d.is_comparison() { "比较采样" } else { "" },
                    d.filter.name()
                ),
            ));
        }
    }

    // 2) mip 链存在性：无 mip 的格式不得声明 LOD 下限。
    if !caps.mips && d.min_lod != LOD_NEUTRAL {
        return Err((
            "E_MIP_UNSUPPORTED",
            format!("{} 是一维格式、无 mip 链，声明 LOD 下限 {} 不会被采样器采纳", fmt.name(), d.min_lod),
        ));
    }

    // 3) LOD 区间内部一致性。
    if d.max_lod < d.min_lod {
        return Err((
            "E_LOD_RANGE_INVERTED",
            format!("LOD 区间反了：上限 {} < 下限 {}——该区间为空，纹理永远采不到", d.max_lod, d.min_lod),
        ));
    }

    // 4) 各向异性：非 2 的幂 / 超出设备能力 / 与 Point 过滤语义矛盾。
    if d.max_anisotropy > 1 && !d.max_anisotropy.is_power_of_two() {
        return Err((
            "E_ANISO_NOT_POW2",
            format!("各向异性 {}x 不是 2 的幂——硬件只接受 1/2/4/8/16，硬件会向下取整或行为未定义", d.max_anisotropy),
        ));
    }
    if d.max_anisotropy > dev_max_aniso {
        return Err((
            "E_ANISO_OVER_CAP",
            format!(
                "各向异性 {}x 超出设备能力上限 {}x（F0007 能力位图注入）",
                d.max_anisotropy, dev_max_aniso
            ),
        ));
    }
    if d.max_anisotropy > 1 && d.filter == Filter::Point {
        return Err((
            "E_ANISO_POINT_FILTER",
            format!("Point 过滤下开{}x 各向异性自相矛盾——Point 不插值，各向异性无从采样", d.max_anisotropy),
        ));
    }

    // 5) 比较采样 × 双三次：软渲与多数移动端不支持该组合。
    if d.is_comparison() && d.filter == Filter::Bicubic {
        return Err((
            "E_COMPARE_BICUBIC",
            "比较采样 + 双三次过滤在主流硬件（含多数移动端与软渲路径）无实现".to_string(),
        ));
    }

    // 6) 寻址与边界色的配对：用 ClampToBorder 就必须显式启用边界色。
    if d.uses_border_address() && !d.border_enabled {
        return Err((
            "E_BORDER_NOT_ENABLED",
            "寻址含 ClampToBorder 但边界色未启用——越界采样返回未定义值（多数实现返回全 0，等于凭空多一条黑边）".to_string(),
        ));
    }

    Ok(())
}

/// 采样器状态合法性校验（判据：非法组合拒绝 + 兼容矩阵 + 正确建议）。
///
/// 对外唯一校验入口：在 [`classify`] 的判定之上附加**可走的建议**
/// （见 [`suggest_preset`]），构成"错误码 + 为什么 + 怎么改"三要素。
pub fn validate(d: &SamplerDesc, fmt: TexFormat, dev_max_aniso: u32) -> Result<(), Rejection> {
    match classify(d, fmt, dev_max_aniso) {
        Ok(()) => Ok(()),
        Err((code, reason)) => {
            let (hint, suggestion) = suggest_preset(d, fmt, dev_max_aniso);
            Err(Rejection { code, reason, suggestion, preset_hint: hint })
        }
    }
}

// ---------------------------------------------------------------------------
// 七、采样器状态库（去重器 + 兜底 + 淘汰 + 统计）
// ---------------------------------------------------------------------------

/// 运行时来源（判据：缺失→运行时构建；去重→共享）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// 命中预置全集。
    Preset(usize),
    /// 运行时构建，但与既有动态状态同参→共享（去重命中）。
    Dedup(usize),
    /// 运行时新构建。
    Runtime(usize),
}

impl Origin {
    /// 来源名（读屏用）。
    pub fn name(self) -> &'static str {
        match self {
            Origin::Preset(_) => "预置",
            Origin::Dedup(_) => "去重共享",
            Origin::Runtime(_) => "运行时构建",
        }
    }
}

/// 解析结果（槽位 + 来源 + 去重键）。
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    /// 动态区槽位下标（预置来源时为 `usize::MAX`——预置不占动态槽）。
    pub slot: usize,
    /// 来源。
    pub origin: Origin,
    /// 去重键哈希（分桶用）。
    pub key_hash: u64,
}

/// 动态区一条状态（运行时构建 / 去重共享的实体）。
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeState {
    /// 描述符。
    pub desc: SamplerDesc,
    /// 规范化键（精确比对用；哈希碰撞不误合）。
    pub key: Vec<u8>,
    /// 最后使用 tick（LRU 淘汰依据）。
    pub last_used: u64,
    /// 引用计数（释放时可归因）。
    pub refs: u32,
}

/// 拒绝台账条目（谁在什么 tick 因什么被拒——可归因）。
#[derive(Clone, Debug, PartialEq)]
pub struct RejectionRecord {
    /// 逻辑 tick。
    pub tick: u64,
    /// 错误码。
    pub code: &'static str,
    /// 触发描述符的人话摘要。
    pub request: String,
    /// 目标格式。
    pub format: TexFormat,
    /// 建议预置名。
    pub preset_hint: &'static str,
}

/// 库统计（去重省了多少可查）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibStats {
    /// 预置命中次数。
    pub preset_hits: u32,
    /// 去重命中次数（同参共享，未新建状态）。
    pub dedup_hits: u32,
    /// 运行时新建次数。
    pub runtime_built: u32,
    /// LRU 淘汰次数。
    pub evictions: u32,
    /// 非法组合拒绝次数。
    pub rejections: u32,
    /// 校验通过次数。
    pub accepted: u32,
}

impl LibStats {
    /// 总解析次数。
    pub fn total_resolves(&self) -> u32 {
        self.preset_hits + self.dedup_hits + self.runtime_built
    }

    /// 预置命中率（百分比；无解析时为 0）。
    pub fn preset_hit_pct(&self) -> u32 {
        let t = self.total_resolves();
        if t == 0 {
            return 0;
        }
        self.preset_hits * 100 / t
    }

    /// 去重省下的状态数（未新建的重复状态个数）。
    pub fn dedup_saved_states(&self) -> u32 {
        self.dedup_hits
    }

    /// 去重省下的显存字节（按 [`SAMPLER_STATE_BYTES`] 折算）。
    pub fn dedup_saved_bytes(&self) -> u64 {
        self.dedup_hits as u64 * SAMPLER_STATE_BYTES
    }

    /// 淘汰挽回的显存字节（每淘汰一个动态状态省一份）。
    pub fn eviction_saved_bytes(&self) -> u64 {
        self.evictions as u64 * SAMPLER_STATE_BYTES
    }
}

/// 采样器状态库（预置全集 + 去重器 + 运行时兜底 + LRU 淘汰）。
pub struct SamplerLibrary {
    /// 动态状态区（运行时构建 + 去重共享；预置不在此区）。
    pub states: Vec<RuntimeState>,
    /// 拒绝台账（零静默）。
    pub rejections: Vec<RejectionRecord>,
    /// 变更申请台账（E10 冻结流程）。
    pub changes: Vec<ChangeRequest>,
    /// 各预置的命中次数（预置热度可查——冷预置是待裁剪候选）。
    pub preset_hits: Vec<u32>,
    /// 设备各向异性上限（F0007 能力位图注入）。
    pub device_max_aniso: u32,
    /// 库统计。
    pub stats: LibStats,
    /// 受理的变更申请计数。
    pub change_seq: u32,
    tick: u64,
}

impl SamplerLibrary {
    /// 构造（预置热度计数与预置表等长）。
    pub fn new() -> Self {
        SamplerLibrary {
            states: Vec::new(),
            rejections: Vec::new(),
            changes: Vec::new(),
            preset_hits: vec![0u32; PRESET_TABLE.len()],
            device_max_aniso: DEFAULT_MAX_ANISOTROPY,
            stats: LibStats::default(),
            change_seq: 0,
            tick: 0,
        }
    }

    /// 注入设备各向异性能力（F0007 能力位图 → 本库上限）。
    pub fn set_device_max_anisotropy(&mut self, n: u32) {
        self.device_max_aniso = n;
    }

    /// 逻辑时钟推进。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 预置是否与给定格式兼容（预置扫描时跳过不兼容项——不是缺陷，是矩阵在筛）。
    ///
    /// **必须走 [`classify`] 而非 [`validate`]**：[`validate`] 在判定失败时会调
    /// [`suggest_preset`] 生成建议，而建议本身要扫全预置表并对每行再判定一次。
    /// 若这里用 `validate`，则「扫 18 行 × 每行失败再扫 18 行」退化为 O(行数²)——
    /// 在不可过滤格式（R8_UINT / R32_FLOAT）下几乎每行都失败，二次方必然发生。
    /// [`classify`] 是纯判定内核、无副作用，正是为这类内层筛选用而设。
    fn preset_compatible(row: &PresetRow, fmt: TexFormat, dev_max_aniso: u32) -> bool {
        let d = row.to_desc();
        classify(&d, fmt, dev_max_aniso).is_ok()
    }

    /// 解析一个采样器请求：预置 → 去重 → 运行时构建（判据四项主流程）。
    ///
    /// 顺序不可换：预置最省（零构建、零显存），去重次之（不建新状态），
    /// 运行时构建最后（确有需要才付代价）。非法组合在任何一步之前先校验——
    /// 绝不"先建了再发现非法"。
    pub fn resolve(&mut self, d: &SamplerDesc, fmt: TexFormat) -> Result<Resolution, Rejection> {
        self.tick = self.tick.saturating_add(1);

        // 门1：非法组合拒绝 + 解释 + 建议（入账，不静默）。
        if let Err(r) = validate(d, fmt, self.device_max_aniso) {
            self.stats.rejections += 1;
            self.rejections.push(RejectionRecord {
                tick: self.tick,
                code: r.code,
                request: d.describe(),
                format: fmt,
                preset_hint: r.preset_hint,
            });
            return Err(r);
        }
        self.stats.accepted += 1;

        let want = d.canonical_key();
        let want_hash = fnv1a64(&want);

        // 门2：预置全集命中（精确键比对，哈希碰撞不误合）。
        for (i, row) in PRESET_TABLE.iter().enumerate() {
            if !Self::preset_compatible(row, fmt) {
                continue;
            }
            if row.to_desc().canonical_key() == want {
                self.preset_hits[i] += 1;
                self.stats.preset_hits += 1;
                return Ok(Resolution { slot: usize::MAX, origin: Origin::Preset(i), key_hash: want_hash });
            }
        }

        // 门3：动态区去重（同参共享——不新建状态，引用计数 +1）。
        for (i, s) in self.states.iter_mut().enumerate() {
            if s.key == want {
                s.refs = s.refs.saturating_add(1);
                s.last_used = self.tick;
                self.stats.dedup_hits += 1;
                return Ok(Resolution { slot: i, origin: Origin::Dedup(i), key_hash: want_hash });
            }
        }

        // 门4：库膨胀 → LRU 淘汰最冷动态项（预置永不动）。
        if self.states.len() >= LIBRARY_CAP {
            if let Some(cold) = self
                .states
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.last_used, s.refs))
                .map(|(i, _)| i)
            {
                self.states.remove(cold);
                self.stats.evictions += 1;
            }
        }

        // 门5：运行时构建兜底（预置未覆盖的合法组合——缺失即兜底，不报错）。
        let slot = self.states.len();
        self.states.push(RuntimeState {
            desc: d.clone(),
            key: want,
            last_used: self.tick,
            refs: 1,
        });
        self.stats.runtime_built += 1;
        Ok(Resolution { slot, origin: Origin::Runtime(slot), key_hash: want_hash })
    }

    /// 释放一个动态槽的引用（归零即失引用；淘汰由容量压力驱动，不在这里做）。
    pub fn release(&mut self, slot: usize) -> bool {
        if slot >= self.states.len() {
            return false;
        }
        self.states[slot].refs = self.states[slot].refs.saturating_sub(1);
        true
    }

    /// 动态区当前占用（读屏与水位用）。
    pub fn runtime_len(&self) -> usize {
        self.states.len()
    }

    /// 空闲动态槽（预置不占槽）。
    pub fn runtime_free(&self) -> usize {
        LIBRARY_CAP.saturating_sub(self.states.len())
    }

    /// 最冷的三个预置（裁剪候选；预置不得私自裁，须走变更流程）。
    pub fn coldest_presets(&self, n: usize) -> Vec<(&'static str, u32)> {
        let mut v: Vec<(&'static str, u32)> = PRESET_TABLE
            .iter()
            .enumerate()
            .map(|(i, r)| (r.key_name, self.preset_hits[i]))
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
        v.truncate(n);
        v
    }

    /// 预置集变更受理（判据：预置集变更走流程）。
    ///
    /// 四道闸：ADR 号非空 / 双签且两人不同 / 目标版本高于当前 / 摘要非空。
    /// 受理≠ 立即生效——预置表是 `const`，改动必须走代码评审 + 重建 + 抬版本号，
    /// 本函数只管"流程是否齐备"，不代劳改表。
    pub fn queue_preset_change(&mut self, req: ChangeRequest) -> ChangeOutcome {
        if req.summary.is_empty() {
            return ChangeOutcome::Rejected {
                code: "E_CHANGE_NO_SUMMARY",
                reason: "变更摘要为空——后来人无法判断改了什么".to_string(),
            };
        }
        if req.adr.is_empty() {
            return ChangeOutcome::Rejected {
                code: "E_CHANGE_NO_ADR",
                reason: "缺ADR 编号——预置基线变更必须挂架构决策记录，否则基线成了无人负责的口头约定".to_string(),
            };
        }
        if req.signer_a.is_empty() || req.signer_b.is_empty() {
            return ChangeOutcome::Rejected {
                code: "E_CHANGE_NO_DUAL_SIGN",
                reason: "缺双签——预置基线是全项目共用资产，单人改动无法回溯".to_string(),
            };
        }
        if req.signer_a == req.signer_b {
            return ChangeOutcome::Rejected {
                code: "E_CHANGE_SAME_SIGNER",
                reason: format!("签署人 {} 与第二签署人相同——单人签不算双签", req.signer_a),
            };
        }
        if req.target_version <= PRESET_BASELINE_VERSION {
            return ChangeOutcome::Rejected {
                code: "E_CHANGE_VERSION_NOT_RAISED",
                reason: format!(
                    "目标版本 {} 未高于当前基线 v{}——变更须抬版本号，否则新旧基线无法区分",
                    req.target_version, PRESET_BASELINE_VERSION
                ),
            };
        }
        self.change_seq = self.change_seq.saturating_add(1);
        self.changes.push(req);
        ChangeOutcome::Accepted { seq: self.change_seq }
    }

    /// 状态表逐行读屏文本（判据：状态表读屏可达；隐私——只含渲染参数，零用户内容）。
    pub fn state_table_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = Vec::with_capacity(PRESET_TABLE.len() + self.states.len() + 3);
        rows.push(format!("采样器状态表：预置 {} 条，运行时 {} 条（容量 {}，空闲 {}）", PRESET_TABLE.len(), self.states.len(), LIBRARY_CAP, self.runtime_free()));
        rows.push(format!(
            "解析统计：预置命中 {}（{}%），去重共享 {}，运行时新建 {}，淘汰 {}，拒绝 {}",
            self.stats.preset_hits,
            self.stats.preset_hit_pct(),
            self.stats.dedup_hits,
            self.stats.runtime_built,
            self.stats.evictions,
            self.stats.rejections,
        ));
        rows.push(format!(
            "去重收益：省下 {} 个状态，折合显存 {} 字节；淘汰挽回 {} 字节",
            self.stats.dedup_saved_states(),
            self.stats.dedup_saved_bytes(),
            self.stats.eviction_saved_bytes(),
        ));
        for (i, row) in PRESET_TABLE.iter().enumerate() {
            rows.push(format!(
                "预置[{}] {} — {}（命中 {} 次）：{}",
                i, row.key_name, row.to_desc().describe(), self.preset_hits[i], row.use_note
            ));
        }
        for (i, s) in self.states.iter().enumerate() {
            rows.push(format!(
                "运行时[{}] {} — {}（引用 {}，最后使用 tick {}）",
                i,
                Origin::Runtime(i).name(),
                s.desc.describe(),
                s.refs,
                s.last_used
            ));
        }
        rows
    }

    /// 人话摘要（读屏播报首行）。
    pub fn screen_text(&self) -> String {
        format!(
            "采样器状态库：预置全集 {} 条全在册，运行时 {} 条；预置命中 {}%，去重省 {} 个状态（{} 字节）；拒绝 {} 次；基线 v{} {}",
            PRESET_TABLE.len(),
            self.states.len(),
            self.stats.preset_hit_pct(),
            self.stats.dedup_saved_states(),
            self.stats.dedup_saved_bytes(),
            self.stats.rejections,
            PRESET_BASELINE_VERSION,
            match verify_baseline() {
                BaselineStatus::Aligned => "已冻结对齐".to_string(),
                BaselineStatus::Drifted { .. } => "已漂移须重签".to_string(),
                BaselineStatus::Unregistered { .. } => "未登记".to_string(),
            },
        )
    }
}

impl Default for SamplerLibrary {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 八、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0018 域自检（判据逐条映射见 `vea18_checks.rs`）。
pub fn run_vea18_checks() -> CheckSet {
    super::vea18_checks::run_vea18_checks()
}

// ---------------------------------------------------------------------------
// 九、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea18_preset_universe_is_complete_and_named() {
        // 预置全集：每行有唯一名、非空用途说明、描述符合法（自身不自相矛盾）。
        let mut names: Vec<&str> = PRESET_TABLE.iter().map(|r| r.key_name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "预置名唯一");
        for row in PRESET_TABLE.iter() {
            assert!(!row.use_note.is_empty(), "{} 缺用途说明", row.key_name);
            let d = row.to_desc();
            validate(&d, TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY)
                .unwrap_or_else(|e| panic!("预置 {} 自身非法：{}", row.key_name, e.reason));
        }
        // 覆盖八类场景：过滤三档、寻址四档、各向异性多档、比较采样在册。
        assert!(PRESET_TABLE.iter().any(|r| r.filter == Filter::Point));
        assert!(PRESET_TABLE.iter().any(|r| r.filter == Filter::Linear));
        assert!(PRESET_TABLE.iter().any(|r| r.filter == Filter::Bicubic));
        for am in [AddressMode::Repeat, AddressMode::MirrorRepeat, AddressMode::ClampToEdge, AddressMode::ClampToBorder] {
            assert!(PRESET_TABLE.iter().any(|r| r.address[0] == am), "寻址档 {} 未进预置全集", am.name());
        }
        for tier in [2u32, 4, 8, 16] {
            assert!(PRESET_TABLE.iter().any(|r| r.aniso == tier), "各向异性 {}x 未进预置全集", tier);
        }
        assert!(PRESET_TABLE.iter().any(|r| r.compare.is_some()), "无比较采样预置");
    }

    #[test]
    fn vea18_dedup_shares_relevance_equal_states() {
        // 两个描述符仅在"不起作用的字段"上不同 → 必须共享同一采样器。
        let a = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_lod(0.0, MIP_LOD_TOP, 0.0);
        let mut b = a.clone();
        b.border_color = [0.1, 0.2, 0.3, 0.4]; // 寻址不含 ClampToBorder → 边界色无效
        assert_eq!(a.canonical_key(), b.canonical_key(), "无效字段不得影响去重键");
        assert_eq!(a.key_hash(), b.key_hash());

        // 反之：LOD 非全零时必须入键（此时 LOD 真的起作用）。
        let mut c = a.clone();
        c.lod_bias = 0.5;
        assert_ne!(a.canonical_key(), c.canonical_key(), "起作用的 LOD 偏置必须入键");
    }

    #[test]
    fn vea18_resolve_order_preset_dedup_runtime() {
        let mut lib = SamplerLibrary::new();
        let preset_req = preset_desc("aniso4_repeat").expect("预置在册");
        let r1 = lib.resolve(&preset_req, TexFormat::Rgba8Unorm).expect("预置应可用");
        assert_eq!(r1.origin, Origin::Preset(8), "命中 aniso4_repeat 预置");

        // 同一请求再来一次：仍走预置（预置不占动态槽）。
        let r2 = lib.resolve(&preset_req, TexFormat::Rgba8Unorm).expect("预置应可用");
        assert_eq!(r2.origin, Origin::Preset(8));
        assert_eq!(lib.runtime_len(), 0, "预置命中不得消耗动态槽");

        // 表外合法组合 → 运行时构建。
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3])
            .with_anisotropy(4)
            .with_lod(0.0, 3.0, 0.25);
        let r3 = lib.resolve(&odd, TexFormat::Rgba8Unorm).expect("合法组合应兜底");
        assert!(matches!(r3.origin, Origin::Runtime(_)), "表外合法组合走运行时构建");
        let first_slot = match r3.origin {
            Origin::Runtime(s) => s,
            _ => unreachable!(),
        };

        // 键等价但字段书写不同 → 去重共享，不新建。
        let mut odd_alias = odd.clone();
        odd_alias.border_color = [9.0, 9.0, 9.0, 9.0]; // 无效字段
        let r4 = lib.resolve(&odd_alias, TexFormat::Rgba8Unorm).expect("应去重共享");
        assert_eq!(r4.origin, Origin::Dedup(first_slot), "同参必须共享");
        assert_eq!(lib.runtime_len(), 1, "去重不得新建状态");
        assert_eq!(lib.stats.dedup_saved_states(), 1);
        assert_eq!(lib.stats.dedup_saved_bytes(), SAMPLER_STATE_BYTES);
    }

    #[test]
    fn vea18_illegal_combos_rejected_with_suggestion() {
        // 整数格式 + Linear → 拒绝并给建议。
        let bad = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
        let e = validate(&bad, TexFormat::R8Uint, DEFAULT_MAX_ANISOTROPY).expect_err("整数格式须拒绝线性");
        assert_eq!(e.code, "E_FILTER_INTEGER");
        assert!(!e.suggestion.is_empty(), "拒绝必须带建议");
        assert!(!e.preset_hint.is_empty(), "拒绝必须指向一个可用的预置");

        // 深度格式 + 非常比较 + Linear → 拒绝。
        let e2 = validate(&bad, TexFormat::Depth32Float, DEFAULT_MAX_ANISOTROPY).expect_err("深度非常比较须拒绝线性");
        assert_eq!(e2.code, "E_FILTER_DEPTH_NO_COMPARE");

        // 深度格式 + 比较采样 + Linear → 放行（PCF 合法）。
        let pcf = bad.clone().with_compare(CompareFunc::LessEqual);
        assert!(validate(&pcf, TexFormat::Depth32Float, DEFAULT_MAX_ANISOTROPY).is_ok(), "PCF 应放行");

        // 其余非法：非 2 的幂、超能力、Point+各向异性、ClampToBorder 未启用边界色、
        // LOD 区间反了、比较+Bicubic。
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(3), TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY)
                .expect_err("3x 非 2 的幂")
                .code,
            "E_ANISO_NOT_POW2"
        );
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(16), TexFormat::Rgba8Unorm, 4)
                .expect_err("16x 超设备上限 4x")
                .code,
            "E_ANISO_OVER_CAP"
        );
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Point, MipFilter::Nearest, [AddressMode::Repeat; 3]).with_anisotropy(4), TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY)
                .expect_err("Point 不该开各向异性")
                .code,
            "E_ANISO_POINT_FILTER"
        );
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToBorder; 3]), TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY)
                .expect_err("ClampToBorder 须启用边界色")
                .code,
            "E_BORDER_NOT_ENABLED"
        );
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(4.0, 2.0, 0.0), TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY)
                .expect_err("LOD 区间反了")
                .code,
            "E_LOD_RANGE_INVERTED"
        );
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Bicubic, MipFilter::Nearest, [AddressMode::ClampToEdge; 3]).with_compare(CompareFunc::LessEqual), TexFormat::Depth32Float, DEFAULT_MAX_ANISOTROPY)
                .expect_err("比较+Bicubic 无实现")
                .code,
            "E_COMPARE_BICUBIC"
        );
        // 无 mip 格式不得声明 LOD 下限。
        assert_eq!(
            validate(&SamplerDesc::new(Filter::Linear, MipFilter::Nearest, [AddressMode::Repeat; 3]).with_lod(1.0, 2.0, 0.0), TexFormat::R8Unorm1d, DEFAULT_MAX_ANISOTROPY)
                .expect_err("一维格式无 mip 链")
                .code,
            "E_MIP_UNSUPPORTED"
        );
    }

    #[test]
    fn vea18_rejections_are_recorded_not_silent() {
        let mut lib = SamplerLibrary::new();
        let bad = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
        assert!(lib.resolve(&bad, TexFormat::R8Uint).is_err());
        assert_eq!(lib.stats.rejections, 1);
        assert_eq!(lib.rejections.len(), 1, "拒绝必须入账（零静默）");
        assert_eq!(lib.rejections[0].code, "E_FILTER_INTEGER");
        assert_eq!(lib.rejections[0].format, TexFormat::R8Uint);
        assert!(!lib.rejections[0].request.is_empty(), "台账须留请求摘要");
        assert!(!lib.rejections[0].preset_hint.is_empty());
        // 拒绝不得留下状态。
        assert_eq!(lib.runtime_len(), 0, "非法请求绝不允许先建后废");
    }

    #[test]
    fn vea18_aniso_tiers_explain_quality_and_cost() {
        // 档位递增有序：锐度随倍率上升，带宽同步上升。
        let mut prev_sharp = 0;
        let mut prev_bw = 0;
        for (tier, sharp, bw, note) in ANISO_TIERS.iter() {
            assert!(*sharp > prev_sharp, "锐度应随倍率递增（{}x）", tier);
            assert!(*bw > prev_bw, "带宽应随倍率递增（{}x）", tier);
            assert!(!note.is_empty(), "{}x 缺档位说明", tier);
            prev_sharp = *sharp;
            prev_bw = *bw;
        }
        // 8x→16x 收益递减必须写明（判据点名"不值就别开"）。
        let (_, s8, note8) = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(8).aniso_tier();
        let (_, s16, note16) = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(16).aniso_tier();
        assert!(s16 - s8 <= 6, "16x 相对 8x 收益须递减");
        assert!(note16.contains("收益递减"));
        assert!(!note8.is_empty());
        // 未在册倍率明确标注外推，不冒充在册数据。
        let (_, _, note_x) = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(32).aniso_tier();
        assert!(note_x.contains("外推"));
    }

    #[test]
    fn vea18_library_evicts_lru_and_keeps_presets() {
        let mut lib = SamplerLibrary::new();
        lib.set_device_max_anisotropy(8);
        // 填满动态区：每档 LOD 上限不同且偏置非零（预置的偏置恒为 0，
        // 故本组键与任何预置都不撞——保证 64 条全部走运行时构建）。
        for i in 0..LIBRARY_CAP {
            let d = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
                .with_lod(0.0, 1.0 + i as f32, i as f32 * 0.25);
            lib.resolve(&d, TexFormat::Rgba8Unorm).expect("合法组合应兜底");
        }
        assert_eq!(lib.runtime_len(), LIBRARY_CAP);
        assert_eq!(lib.runtime_free(), 0);
        assert_eq!(lib.stats.evictions, 0);

        // 再来一个新键 → 触发 LRU 淘汰（容量仍封顶，不许无限膨胀）。
        let fresh = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_lod(0.0, 99.0, 77.5);
        lib.resolve(&fresh, TexFormat::Rgba8Unorm).expect("合法组合应兜底");
        assert_eq!(lib.runtime_len(), LIBRARY_CAP, "库膨胀必须封顶");
        assert_eq!(lib.stats.evictions, 1);
        assert!(lib.stats.eviction_saved_bytes() > 0, "淘汰挽回显存须可查");

        // 预置永不被淘汰：表长不变、预置命中仍可达。
        assert_eq!(PRESET_TABLE.len(), 16);
        let p = preset_desc("trilinear_repeat").expect("预置在册");
        let r = lib.resolve(&p, TexFormat::Rgba8Unorm).expect("预置应可用");
        assert_eq!(r.origin, Origin::Preset(4));
    }

    #[test]
    fn vea18_baseline_freeze_three_states_and_change_flow() {
        let actual = compute_preset_hash();
        // 三态互不混用：未登记 / 对齐 / 漂移，处置方向相反故必须拆开。
        let st = verify_baseline();
        match st {
            BaselineStatus::Unregistered { actual: a } => assert_eq!(a, actual),
            BaselineStatus::Aligned => assert_eq!(PRESET_BASELINE_HASH, actual),
            BaselineStatus::Drifted { expected, actual: a } => {
                assert_ne!(expected, a);
                assert!(!st.is_aligned());
            }
        }
        assert!(!st.describe().is_empty());

        // 变更流程四道闸。
        let mut lib = SamplerLibrary::new();
        let ok = ChangeRequest {
            summary: "新增 aniso32 预置档".to_string(),
            adr: "ADR-VE-0181".to_string(),
            signer_a: "render-a".to_string(),
            signer_b: "render-b".to_string(),
            target_version: PRESET_BASELINE_VERSION + 1,
        };
        assert!(matches!(lib.queue_preset_change(ok.clone()), ChangeOutcome::Accepted { .. }));

        let mut bad = ok.clone();
        bad.adr = String::new();
        assert!(matches!(lib.queue_preset_change(bad), ChangeOutcome::Rejected { code: "E_CHANGE_NO_ADR", .. }));

        let mut bad2 = ok.clone();
        bad2.signer_b = bad2.signer_a.clone();
        assert!(matches!(lib.queue_preset_change(bad2), ChangeOutcome::Rejected { code: "E_CHANGE_SAME_SIGNER", .. }));

        let mut bad3 = ok.clone();
        bad3.target_version = PRESET_BASELINE_VERSION;
        assert!(matches!(lib.queue_preset_change(bad3), ChangeOutcome::Rejected { code: "E_CHANGE_VERSION_NOT_RAISED", .. }));

        let mut bad4 = ok.clone();
        bad4.summary = String::new();
        assert!(matches!(lib.queue_preset_change(bad4), ChangeOutcome::Rejected { code: "E_CHANGE_NO_SUMMARY", .. }));

        assert_eq!(lib.changes.len(), 1, "只有齐备的那一件入册");
    }

    #[test]
    fn vea18_format_matrix_is_declared_for_every_format() {
        // 矩阵不得有缺格：每个格式都能查到声明（未登记即最保守，不是放行）。
        let fmts = [
            TexFormat::Rgba8Unorm,
            TexFormat::Bc1,
            TexFormat::Bc3,
            TexFormat::Bc7,
            TexFormat::R8Uint,
            TexFormat::R32Float,
            TexFormat::Rgba32Float,
            TexFormat::Depth32Float,
            TexFormat::Depth24UnormStencil8,
            TexFormat::R8Unorm1d,
        ];
        for f in fmts.iter() {
            assert!(
                FORMAT_MATRIX.iter().any(|(m, _)| m == f),
                "{} 未登记兼容矩阵——缺格会让未声明能力被默认放行",
                f.name()
            );
        }
        assert_eq!(FORMAT_MATRIX.len(), fmts.len());
        // 整数格式一律不可线性过滤；深度格式仅比较采样可线性。
        for (f, c) in FORMAT_MATRIX.iter() {
            if c.integer {
                assert!(!c.linear_filter && !c.linear_compare, "{} 是整数格式，不该可线性过滤", f.name());
            }
            if c.depth {
                assert!(!c.linear_filter, "{} 是深度格式，非常比较不可线性", f.name());
                assert!(c.linear_compare, "{} 是深度格式，应支持比较采样线性（PCF）", f.name());
            }
        }
        // 不可过滤浮点：矩阵须显式关掉线性过滤。
        assert!(!TexFormat::R32Float.caps().linear_filter);
        assert!(!TexFormat::Rgba32Float.caps().linear_filter);
        assert!(!TexFormat::R8Unorm1d.caps().mips);
    }

    #[test]
    fn vea18_screen_reader_surface() {
        let mut lib = SamplerLibrary::new();
        let p = preset_desc("shadow_pcf_linear").expect("预置在册");
        lib.resolve(&p, TexFormat::Depth32Float).expect("PCF 预置应可用");
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(0.0, 2.5, 0.1);
        lib.resolve(&odd, TexFormat::Rgba8Unorm).expect("应兜底");
        lib.resolve(&odd, TexFormat::Rgba8Unorm).expect("应去重");

        let rows = lib.state_table_rows();
        // 逐行可达：3 行汇总 + 每预置一行 + 每个运行时状态一行。
        assert_eq!(rows.len(), 3 + PRESET_TABLE.len() + 1);
        assert!(rows[0].contains("容量"));
        assert!(rows[1].contains("去重共享 1"));
        assert!(rows[2].contains("省下 1 个状态"));
        for r in rows.iter() {
            assert!(!r.is_empty(), "读屏行不得为空");
        }
        let s = lib.screen_text();
        assert!(s.contains("采样器状态库"));
        assert!(s.contains("去重省 1 个状态"));
        // 隐私：状态表不含用户内容（只含渲染参数与预置名）。
        for r in rows.iter() {
            assert!(!r.contains('\\'), "读屏文本不应含转义噪声：{}", r);
        }
    }

    #[test]
    fn vea18_preset_compatibility_gates_by_format() {
        // 阴影预置只对深度格式放行；对颜色格式其比较语义不成立 → 矩阵在筛。
        let mut lib = SamplerLibrary::new();
        let pcf = preset_desc("shadow_pcf_linear").expect("预置在册");
        assert!(lib.resolve(&pcf, TexFormat::Depth32Float).is_ok(), "PCF 对深度格式应放行");
        // 整数格式对所有预置都不可线性过滤 → 只能落到 Point 类预置。
        let lin = preset_desc("trilinear_repeat").expect("预置在册");
        assert!(lib.resolve(&lin, TexFormat::R8Uint).is_err(), "整数格式不得用三线性预置");
        let pt = preset_desc("nearest_clamp").expect("预置在册");
        assert!(lib.resolve(&pt, TexFormat::R8Uint).is_ok(), "整数格式可用 Point 预置");
    }

    #[test]
    fn vea18_suggest_preset_points_to_nearest_legal_row() {
        // 构造一个"非法但接近 aniso4_repeat"的请求，建议应指向它或同档近邻。
        let mut d = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(3);
        d.lod_bias = 0.25;
        let (name, note) = suggest_preset(&d, TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY);
        assert!(!name.is_empty());
        assert!(note.contains(name), "建议正文须点名该预置：{}", note);
        // 建议本身必须是真能用的预置（不许建议一个非法预置）。
        let target = preset_desc(name).expect("建议的预置必须在册");
        assert!(validate(&target, TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY).is_ok(), "建议的预置自身须合法");
        // 空差异时给出"直接用"。
        let exact = preset_desc("nearest_clamp").expect("预置在册");
        let (n2, note2) = suggest_preset(&exact, TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY);
        assert_eq!(n2, "nearest_clamp");
        assert!(note2.contains("无字段差异"));
    }

    /// 建议**不得是死胡同**：凡给出预置名，该预置在**请求方格式**下必须真能过校验。
    ///
    /// 曾经的真实缺陷：整数/不可过滤浮点/深度格式下，建议引擎只比字段差异，
    /// 会推荐一个 Linear 预置——用户照改仍被同一条理由拒之门外。建议若不能
    /// 指路就不如不指，故此处逐案断言"建议可用或明确无可用预置"。
    #[test]
    fn vea18_suggestion_is_never_a_dead_end() {
        let lin = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
        let probes: [(SamplerDesc, TexFormat); 6] = [
            (lin.clone(), TexFormat::R8Uint),
            (lin.clone(), TexFormat::R32Float),
            (lin.clone(), TexFormat::Rgba32Float),
            (lin.clone(), TexFormat::Depth32Float),
            (lin.clone(), TexFormat::R8Unorm1d),
            (
                SamplerDesc::new(Filter::Bicubic, MipFilter::Nearest, [AddressMode::ClampToEdge; 3])
                    .with_compare(CompareFunc::LessEqual),
                TexFormat::Depth32Float,
            ),
        ];
        for (d, f) in probes.iter() {
            let Err(e) = validate(d, *f, DEFAULT_MAX_ANISOTROPY) else {
                continue;
            };
            if e.preset_hint.is_empty() {
                // 无可荐：必须明说"没有可用预置"，而不是留空让人猜。
                assert!(
                    e.suggestion.contains("没有可用预置"),
                    "{} 无建议时须说明原因：{}",
                    f.name(),
                    e.suggestion
                );
            } else {
                let p = preset_desc(e.preset_hint).unwrap_or_else(|| panic!("{} 建议的预置不在册", f.name()));
                assert!(
                    validate(&p, *f, DEFAULT_MAX_ANISOTROPY).is_ok(),
                    "死胡同建议：[{}] {} 推荐 {} 但其在同格式下仍非法——建议必须能指路",
                    f.name(),
                    e.code,
                    e.preset_hint
                );
            }
        }
    }
}