//! VE-F1011 · PngSuite 互操作语料表（34 正常 + 17 畸形 = 51）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1011`
//!
//! 本文件是**纯数据**模块：特征位、块名、四字节码、滤镜/压缩/透明三张
//! 场景枚举、34 张正常图与 17 张畸形图的场景表，以及拒绝理由分类。
//!
//! 三条纪律：
//!
//! 1. **表即数据，不是代码**。全部条目 `const` + `Copy`：任何需要运行期
//!    才能决定的分支（打包、滤波选择、压缩）都不许进表——它们在
//!    `super::vef13_pngsuite` 的生成器里。这样「语料退化成什么」由表本身
//!    回答，不靠读代码推理。
//! 2. **注入方式与期望分开声明**。畸形表里的 [`Mutation`] 只说「改哪个
//!    字段、CRC 重不重算」，[`RejectReason`] 是**独立写下的断言**。若期望
//!    由注入方式推导，矩阵就只是在验自己的推导函数。
//! 3. **逐图一个特性**。每张正常图带一个 `rationale` 字段说明它专测什么，
//!    特性位集则供覆盖矩阵逐条对账。特性位少一个 → 覆盖矩阵报缺口；
//!    特性位多一个（声明了却没真注入）→ 覆盖矩阵的「声明即事实」核对项
//!    会红。

#![allow(clippy::needless_range_loop)]

use alloc::vec::Vec;

use super::vef01_pngdec as dec;
use super::vef01_pngdec::ColorType;

// ===========================================================================
// 一、特性位集
// ===========================================================================

/// 特性位：颜色类型（5）。
pub const F_GRAY: u32 = 1 << 0;
pub const F_RGB: u32 = 1 << 1;
pub const F_PALETTE: u32 = 1 << 2;
pub const F_GRAY_ALPHA: u32 = 1 << 3;
pub const F_RGBA: u32 = 1 << 4;

/// 特性位：位深（5）。
pub const F_DEPTH_1: u32 = 1 << 5;
pub const F_DEPTH_2: u32 = 1 << 6;
pub const F_DEPTH_4: u32 = 1 << 7;
pub const F_DEPTH_8: u32 = 1 << 8;
pub const F_DEPTH_16: u32 = 1 << 9;

/// 特性位：结构与语义。
pub const F_INTERLACE: u32 = 1 << 10;
pub const F_TRNS_PALETTE: u32 = 1 << 11;
pub const F_TRNS_GRAY: u32 = 1 << 12;
pub const F_TRNS_RGB: u32 = 1 << 13;
pub const F_TEXT: u32 = 1 << 14;
pub const F_COLOR_MGMT: u32 = 1 << 15;
pub const F_BACKGROUND: u32 = 1 << 16;
pub const F_PHYSICAL: u32 = 1 << 17;
pub const F_SPLIT_IDAT: u32 = 1 << 18;

/// 特性位：五种滤波器。
pub const F_FILTER_NONE: u32 = 1 << 19;
pub const F_FILTER_SUB: u32 = 1 << 20;
pub const F_FILTER_UP: u32 = 1 << 21;
pub const F_FILTER_AVG: u32 = 1 << 22;
pub const F_FILTER_PAETH: u32 = 1 << 23;

/// 特性位：三种 deflate 块形态。
pub const F_COMPRESS_STORED: u32 = 1 << 24;
pub const F_COMPRESS_FIXED: u32 = 1 << 25;
pub const F_COMPRESS_DYNAMIC: u32 = 1 << 26;

/// 特性位：几何与数据形态边界。
pub const F_TINY: u32 = 1 << 27;
pub const F_ODD_WIDTH: u32 = 1 << 28;
pub const F_SPARSE_TRNS: u32 = 1 << 29;
pub const F_FULL_PALETTE: u32 = 1 << 30;
pub const F_ALPHA_ALL: u32 = 1 << 31;

/// 五种滤波器的位掩码（轮转/自适应场景用）。
pub const FILTER_MASK: u32 =
    F_FILTER_NONE | F_FILTER_SUB | F_FILTER_UP | F_FILTER_AVG | F_FILTER_PAETH;

/// 三种压缩形态的位掩码。
pub const COMP_MASK: u32 = F_COMPRESS_STORED | F_COMPRESS_FIXED | F_COMPRESS_DYNAMIC;

/// **必须被覆盖**的特性位（覆盖矩阵的判定集合）。
///
/// 这张表是**下界**：某特性覆盖更多不算缺口，但一个都不覆盖立即红
/// （例如删光全部交错图，`F_INTERLACE` 当场变缺口）。每位的语义见
/// `super::vef13_pngsuite::coverage`。
pub const REQUIRED_FEATURES: [u32; 30] = [
    F_GRAY,
    F_RGB,
    F_PALETTE,
    F_GRAY_ALPHA,
    F_RGBA,
    F_DEPTH_1,
    F_DEPTH_2,
    F_DEPTH_4,
    F_DEPTH_8,
    F_DEPTH_16,
    F_INTERLACE,
    F_TRNS_PALETTE,
    F_TRNS_GRAY,
    F_TRNS_RGB,
    F_TEXT,
    F_COLOR_MGMT,
    F_BACKGROUND,
    F_PHYSICAL,
    F_SPLIT_IDAT,
    F_FILTER_NONE,
    F_FILTER_SUB,
    F_FILTER_UP,
    F_FILTER_AVG,
    F_FILTER_PAETH,
    F_COMPRESS_STORED,
    F_COMPRESS_FIXED,
    F_COMPRESS_DYNAMIC,
    F_TINY,
    F_ODD_WIDTH,
    F_ALPHA_ALL,
];

/// 规范定义的全部 15 种合法「位深 × 颜色类型」组合。
pub const LEGAL_COMBOS: [(ColorType, u8); 15] = [
    (ColorType::Gray, 1),
    (ColorType::Gray, 2),
    (ColorType::Gray, 4),
    (ColorType::Gray, 8),
    (ColorType::Gray, 16),
    (ColorType::Rgb, 8),
    (ColorType::Rgb, 16),
    (ColorType::Palette, 1),
    (ColorType::Palette, 2),
    (ColorType::Palette, 4),
    (ColorType::Palette, 8),
    (ColorType::GrayAlpha, 8),
    (ColorType::GrayAlpha, 16),
    (ColorType::Rgba, 8),
    (ColorType::Rgba, 16),
];

/// 本语料**声明覆盖**的合法组合（与 [`LEGAL_COMBOS`] 同序，本单全覆盖）。
pub const COVERED_COMBOS: [(ColorType, u8); 15] = LEGAL_COMBOS;

// ===========================================================================
// 二、四字节块名
// ===========================================================================

/// 4 字节块名（PNG 块名全是大写 ASCII，天然可折成 `u32`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fourcc(pub u32);

impl Fourcc {
    pub const IHDR: Fourcc = Fourcc(0x4948_4452);
    pub const PLTE: Fourcc = Fourcc(0x504C_5445);
    pub const IDAT: Fourcc = Fourcc(0x4944_4154);
    pub const IEND: Fourcc = Fourcc(0x4945_4E44);
    pub const TRNS: Fourcc = Fourcc(0x7452_4E53);
    pub const GAMA: Fourcc = Fourcc(0x6741_4D41);
    pub const CHRM: Fourcc = Fourcc(0x6348_524D);
    pub const SRGB: Fourcc = Fourcc(0x7352_4247);
    pub const BKGD: Fourcc = Fourcc(0x624B_4744);
    pub const PHYS: Fourcc = Fourcc(0x7048_5973);
    pub const TEXT: Fourcc = Fourcc(0x7445_5874);
    pub const ZTXT: Fourcc = Fourcc(0x7A54_5874);
    pub const ITXT: Fourcc = Fourcc(0x6954_5874);
    pub const ABCD: Fourcc = Fourcc(0x4142_4344);

    /// 拆成 4 字节（先长度高位，再类型四字节）。
    pub fn as_bytes(self) -> [u8; 4] {
        [(self.0 >> 24) as u8, (self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8]
    }

    /// 由 4 字节折回（长度不足给 `None`，不猜测）。
    pub fn from_bytes(b: &[u8]) -> Option<Fourcc> {
        if b.len() < 4 {
            return None;
        }
        Some(Fourcc(
            ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32),
        ))
    }

    /// 是否 critical（类型首字节第 5 位大写 = 关键块）。
    pub fn is_critical(self) -> bool {
        (self.as_bytes()[0] & 0x20) == 0
    }
}

// ===========================================================================
// 三、三张场景枚举
// ===========================================================================

/// 滤波策略（语料侧：写哪种滤波**由场景指定**，不借编码器的自适应试探）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterPlan {
    /// 全部行用同一种滤波。
    Fixed(u8),
    /// 逐行轮转五种滤波（每种至少走过一行）。
    Cycle5,
    /// 规范最小和启发式（逐行试五种，取绝对差和最小者）。
    Adaptive,
}

impl FilterPlan {
    /// 该策略会用到哪些滤波器的特性位。
    pub fn feature_mask(self) -> u32 {
        match self {
            FilterPlan::Fixed(f) if f <= 4 => 1 << (19 + f as u32),
            _ => FILTER_MASK,
        }
    }
}

/// 压缩块形态（语料侧：本单自带 deflate，不借 F1002）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompPlan {
    /// stored 块（BTYPE=00）。
    Stored,
    /// fixed Huffman 块（BTYPE=01），字面量流无匹配。
    FixedLiteral,
    /// dynamic Huffman 块（BTYPE=10），带完整表头。
    DynamicLiteral,
}

impl CompPlan {
    /// 对应的特性位。
    pub fn feature(self) -> u32 {
        match self {
            CompPlan::Stored => F_COMPRESS_STORED,
            CompPlan::FixedLiteral => F_COMPRESS_FIXED,
            CompPlan::DynamicLiteral => F_COMPRESS_DYNAMIC,
        }
    }
}

/// 透明语义（语料侧声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrnsPlan {
    /// 无 tRNS。
    None,
    /// 调色板逐项 alpha（长度 = 声明值；短于 PLTE 即稀疏形态）。
    PaletteAlpha(usize),
    /// 灰度色键（键值 = 像素 (0,0) 的样本，保证命中且唯一）。
    GrayKey,
    /// 真彩色键（键值 = 像素 (0,0) 的三分量）。
    RgbKey,
}

impl TrnsPlan {
    /// 对应的特性位（稀疏形态另由场景显式加 `F_SPARSE_TRNS`）。
    pub fn feature(self) -> u32 {
        match self {
            TrnsPlan::None => 0,
            TrnsPlan::PaletteAlpha(_) => F_TRNS_PALETTE,
            TrnsPlan::GrayKey => F_TRNS_GRAY,
            TrnsPlan::RgbKey => F_TRNS_RGB,
        }
    }
}

// ===========================================================================
// 四、34 张正常语料图
// ===========================================================================

/// 一张正常语料图的场景配置（**全字段 `Copy` + 表 `const`**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ValidScenario {
    /// 图 ID（PngSuite 命名法：`族 + 类型 + 位深`）。
    pub id: &'static str,
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 颜色类型。
    pub color: ColorType,
    /// 位深。
    pub depth: u8,
    /// 是否 Adam7 隔行。
    pub interlace: bool,
    /// 调色板项数（0 = 无调色板）。
    pub palette_entries: u16,
    /// 透明语义。
    pub trns: TrnsPlan,
    /// 滤波策略。
    pub filter: FilterPlan,
    /// 压缩块形态。
    pub comp: CompPlan,
    /// 附加 ancillary 块（顺序即写入顺序）。
    pub ancillary: &'static [Fourcc],
    /// IDAT 分片数（1 = 单块）。
    pub idat_split: u8,
    /// alpha 全 0。
    pub alpha_all_zero: bool,
    /// 注入的特性位集。
    pub features: u32,
    /// 本图专测的那一个特性（锚点「每张图测试一个特定特性组合」）。
    pub rationale: &'static str,
}

/// 34 张正常语料图。
///
/// 分族：`basn` 15 张（规范 15 种合法组合全覆盖）、`cs` 6 张（色彩管理 +
/// 文本 + 分片）、`f` 8 张（透明与元数据特性族，含锚点点名的 `f03n`）、
/// `xd` 5 张（Adam7 隔行）。
pub const VALID_SCENARIOS: [ValidScenario; 34] = [
    ValidScenario {
        id: "basn0g01",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 1,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_1 | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "1 位灰度 + stored 块：位深下限的取样序与滤波字节布局",
    },
    ValidScenario {
        id: "basn0g02",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 2,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_2 | F_FILTER_SUB | F_COMPRESS_STORED,
        rationale: "2 位灰度 + Sub 滤波：字节内前像素引用",
    },
    ValidScenario {
        id: "basn0g04",
        width: 11,
        height: 8,
        color: ColorType::Gray,
        depth: 4,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(2),
        comp: CompPlan::FixedLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_4 | F_FILTER_UP | F_ODD_WIDTH | F_COMPRESS_FIXED,
        rationale: "4 位灰度 + 11 像素宽：非字节对齐行宽的跨字节取样",
    },
    ValidScenario {
        id: "basn0g08",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(3),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_8 | F_FILTER_AVG | F_COMPRESS_STORED,
        rationale: "8 位灰度 + Average 滤波：进位语义与半字节舍入",
    },
    ValidScenario {
        id: "basn0g16",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(4),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_16 | F_FILTER_PAETH | F_COMPRESS_STORED,
        rationale: "16 位灰度：大端样本 + Paeth 预测",
    },
    ValidScenario {
        id: "basn2c08",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::FixedLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB | F_DEPTH_8 | F_FILTER_NONE | F_COMPRESS_FIXED,
        rationale: "8 位真彩：bpp=3 的滤波先行字节",
    },
    ValidScenario {
        id: "basn2c16",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB | F_DEPTH_16 | F_FILTER_SUB | F_COMPRESS_STORED,
        rationale: "16 位真彩：通道宽化后的 Sub（bpp=6）",
    },
    ValidScenario {
        id: "basn3p01",
        width: 16,
        height: 16,
        color: ColorType::Palette,
        depth: 1,
        interlace: false,
        palette_entries: 4,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE | F_DEPTH_1 | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "1 位调色板：索引位打包 + PLTE 查色",
    },
    ValidScenario {
        id: "basn3p02",
        width: 16,
        height: 16,
        color: ColorType::Palette,
        depth: 2,
        interlace: false,
        palette_entries: 4,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE | F_DEPTH_2 | F_FILTER_SUB | F_COMPRESS_STORED,
        rationale: "2 位调色板：2 像素一字节的索引取样序",
    },
    ValidScenario {
        id: "basn3p04",
        width: 11,
        height: 8,
        color: ColorType::Palette,
        depth: 4,
        interlace: false,
        palette_entries: 16,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(2),
        comp: CompPlan::FixedLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE | F_DEPTH_4 | F_FILTER_UP | F_ODD_WIDTH | F_COMPRESS_FIXED,
        rationale: "4 位调色板 + 非字节对齐宽：索引跨字节 + Up 滤波",
    },
    ValidScenario {
        id: "basn3p08",
        width: 8,
        height: 8,
        color: ColorType::Palette,
        depth: 8,
        interlace: false,
        palette_entries: 256,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(3),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE | F_DEPTH_8 | F_FILTER_AVG | F_FULL_PALETTE | F_COMPRESS_STORED,
        rationale: "8 位调色板 + 256 项满表：末项索引与 PLTE 上界",
    },
    ValidScenario {
        id: "basn4a08",
        width: 8,
        height: 8,
        color: ColorType::GrayAlpha,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(4),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY_ALPHA | F_DEPTH_8 | F_FILTER_PAETH | F_COMPRESS_STORED,
        rationale: "灰度 + alpha：bpp=2 与 alpha 通道的 Paeth",
    },
    ValidScenario {
        id: "basn4a16",
        width: 8,
        height: 8,
        color: ColorType::GrayAlpha,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY_ALPHA | F_DEPTH_16 | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "16 位灰度 + alpha：四字节大端样本的滤波先行字节",
    },
    ValidScenario {
        id: "basn6a08",
        width: 8,
        height: 8,
        color: ColorType::Rgba,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::FixedLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGBA | F_DEPTH_8 | F_FILTER_SUB | F_COMPRESS_FIXED,
        rationale: "8 位真彩 + alpha：bpp=4 的 Sub",
    },
    ValidScenario {
        id: "basn6a16",
        width: 8,
        height: 8,
        color: ColorType::Rgba,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(2),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGBA | F_DEPTH_16 | F_FILTER_UP | F_COMPRESS_STORED,
        rationale: "16 位真彩 + alpha：bpp=8 的 Up",
    },
    ValidScenario {
        id: "cs1g08",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Cycle5,
        comp: CompPlan::Stored,
        ancillary: &[Fourcc::GAMA, Fourcc::CHRM, Fourcc::SRGB],
        idat_split: 3,
        alpha_all_zero: false,
        features: F_GRAY
            | F_DEPTH_8
            | F_COLOR_MGMT
            | F_SPLIT_IDAT
            | FILTER_MASK
            | F_COMPRESS_STORED,
        rationale: "色彩管理三块并存 + IDAT 拆三片：跨片 zlib 连续性 + 五种滤波轮转",
    },
    ValidScenario {
        id: "cs2c08",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Adaptive,
        comp: CompPlan::FixedLiteral,
        ancillary: &[Fourcc::GAMA, Fourcc::CHRM, Fourcc::SRGB],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB
            | F_DEPTH_8
            | F_COLOR_MGMT
            | FILTER_MASK
            | F_COMPRESS_FIXED,
        rationale: "真彩色彩管理三块 + 自适应滤波（规范最小和启发式）",
    },
    ValidScenario {
        id: "cs2c16",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(4),
        comp: CompPlan::Stored,
        ancillary: &[Fourcc::GAMA],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB | F_DEPTH_16 | F_FILTER_PAETH | F_COLOR_MGMT | F_COMPRESS_STORED,
        rationale: "16 位真彩 + gAMA：16 位样本下的色彩管理联动",
    },
    ValidScenario {
        id: "cs3p08",
        width: 16,
        height: 16,
        color: ColorType::Palette,
        depth: 8,
        interlace: false,
        palette_entries: 16,
        trns: TrnsPlan::PaletteAlpha(3),
        filter: FilterPlan::Adaptive,
        comp: CompPlan::DynamicLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE
            | F_DEPTH_8
            | F_TRNS_PALETTE
            | F_SPARSE_TRNS
            | FILTER_MASK
            | F_COMPRESS_DYNAMIC,
        rationale: "稀疏 tRNS（短于 PLTE 的缺省不透明）+ dynamic Huffman 表头",
    },
    ValidScenario {
        id: "cs4a08",
        width: 8,
        height: 8,
        color: ColorType::GrayAlpha,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[Fourcc::TEXT],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY_ALPHA | F_DEPTH_8 | F_TEXT | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "tEXt 与逐行解码互不干扰（ancillary 不影响像素）",
    },
    ValidScenario {
        id: "cs6a16",
        width: 8,
        height: 8,
        color: ColorType::Rgba,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(3),
        comp: CompPlan::FixedLiteral,
        ancillary: &[Fourcc::TEXT, Fourcc::ZTXT, Fourcc::ITXT],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGBA | F_DEPTH_16 | F_TEXT | F_FILTER_AVG | F_COMPRESS_FIXED,
        rationale: "三块文本并存（tEXt/zTXt/iTXt）：未知 ancillary 一律跳过",
    },
    ValidScenario {
        id: "f01n3p08",
        width: 8,
        height: 8,
        color: ColorType::Palette,
        depth: 8,
        interlace: false,
        palette_entries: 8,
        trns: TrnsPlan::PaletteAlpha(8),
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_PALETTE | F_DEPTH_8 | F_TRNS_PALETTE | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "满长 tRNS 调色板数组（长度 = PLTE 项数）",
    },
    ValidScenario {
        id: "f02n0g08",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::GrayKey,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_8 | F_TRNS_GRAY | F_FILTER_SUB | F_COMPRESS_STORED,
        rationale: "8 位灰度色键：精确匹配，邻值不得被抠掉",
    },
    ValidScenario {
        id: "f03n2c08",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::RgbKey,
        filter: FilterPlan::Fixed(2),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB | F_DEPTH_8 | F_TRNS_RGB | F_FILTER_UP | F_COMPRESS_STORED,
        rationale: "8 位真彩色键（锚点点名的 f03n）：三分量逐一全等",
    },
    ValidScenario {
        id: "f04n0g16",
        width: 8,
        height: 8,
        color: ColorType::Gray,
        depth: 16,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::GrayKey,
        filter: FilterPlan::Fixed(3),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_16 | F_TRNS_GRAY | F_FILTER_AVG | F_COMPRESS_STORED,
        rationale: "16 位灰度色键：2 字节大端键对 16 位样本的精确匹配",
    },
    ValidScenario {
        id: "f05n6a08",
        width: 8,
        height: 8,
        color: ColorType::Rgba,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Cycle5,
        comp: CompPlan::Stored,
        ancillary: &[Fourcc::TEXT, Fourcc::BKGD],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGBA
            | F_DEPTH_8
            | F_TEXT
            | F_BACKGROUND
            | FILTER_MASK
            | F_COMPRESS_STORED,
        rationale: "alpha 图 + bKGD（真彩类的 6 字节三元组背景色）",
    },
    ValidScenario {
        id: "f06n2c08",
        width: 8,
        height: 8,
        color: ColorType::Rgb,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(4),
        comp: CompPlan::FixedLiteral,
        ancillary: &[Fourcc::PHYS, Fourcc::BKGD],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB
            | F_DEPTH_8
            | F_PHYSICAL
            | F_BACKGROUND
            | F_FILTER_PAETH
            | F_COMPRESS_FIXED,
        rationale: "pHYs 物理尺寸 + bKGD：元数据块与像素路径的隔离",
    },
    ValidScenario {
        id: "f07n6a08",
        width: 8,
        height: 8,
        color: ColorType::Rgba,
        depth: 8,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 4,
        alpha_all_zero: true,
        features: F_RGBA
            | F_DEPTH_8
            | F_SPLIT_IDAT
            | F_ALPHA_ALL
            | F_FILTER_NONE
            | F_COMPRESS_STORED,
        rationale: "全透明（alpha 全 0）+ IDAT 拆四片：最坏的分片边界",
    },
    ValidScenario {
        id: "f08n0g01",
        width: 1,
        height: 1,
        color: ColorType::Gray,
        depth: 1,
        interlace: false,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(0),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_1 | F_TINY | F_FILTER_NONE | F_COMPRESS_STORED,
        rationale: "1×1 最小图：单行单像素（滤波的 prev 全零）",
    },
    ValidScenario {
        id: "xd0g08",
        width: 16,
        height: 16,
        color: ColorType::Gray,
        depth: 8,
        interlace: true,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Cycle5,
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_GRAY | F_DEPTH_8 | F_INTERLACE | FILTER_MASK | F_COMPRESS_STORED,
        rationale: "隔行灰度：16×16 使七遍全非空",
    },
    ValidScenario {
        id: "xd2c08",
        width: 16,
        height: 16,
        color: ColorType::Rgb,
        depth: 8,
        interlace: true,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(1),
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB | F_DEPTH_8 | F_INTERLACE | F_FILTER_SUB | F_COMPRESS_STORED,
        rationale: "隔行真彩：遍内行宽随遍变化，滤波 bpp 恒 3",
    },
    ValidScenario {
        id: "xd2c16",
        width: 16,
        height: 16,
        color: ColorType::Rgb,
        depth: 16,
        interlace: true,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(4),
        comp: CompPlan::FixedLiteral,
        ancillary: &[],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGB
            | F_DEPTH_16
            | F_INTERLACE
            | F_FILTER_PAETH
            | F_COMPRESS_FIXED,
        rationale: "隔行 16 位真彩：遍内大端样本与 Paeth",
    },
    ValidScenario {
        id: "xd4a08",
        width: 16,
        height: 16,
        color: ColorType::GrayAlpha,
        depth: 8,
        interlace: true,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Adaptive,
        comp: CompPlan::Stored,
        ancillary: &[],
        idat_split: 2,
        alpha_all_zero: false,
        features: F_GRAY_ALPHA
            | F_DEPTH_8
            | F_INTERLACE
            | F_SPLIT_IDAT
            | FILTER_MASK
            | F_COMPRESS_STORED,
        rationale: "隔行灰度 + alpha + IDAT 分片：三者叠加",
    },
    ValidScenario {
        id: "xd6a08",
        width: 16,
        height: 16,
        color: ColorType::Rgba,
        depth: 8,
        interlace: true,
        palette_entries: 0,
        trns: TrnsPlan::None,
        filter: FilterPlan::Fixed(2),
        comp: CompPlan::DynamicLiteral,
        ancillary: &[Fourcc::PHYS],
        idat_split: 1,
        alpha_all_zero: false,
        features: F_RGBA
            | F_DEPTH_8
            | F_INTERLACE
            | F_FILTER_UP
            | F_PHYSICAL
            | F_COMPRESS_DYNAMIC,
        rationale: "隔行真彩 + alpha + dynamic 表头 + ancillary",
    },
];

// ===========================================================================
// 五、拒绝理由分类（两侧解码器归约到的公共语）
// ===========================================================================

/// 拒绝理由分类。
///
/// **为什么不直接比两套错误枚举**：被测侧区分「critical CRC 错」与
/// 「ancillary CRC 告警」，参考侧不区分——直接比类型会把「分类口径差异」
/// 误报成「解码不一致」。分类层是二者之间**唯一**的公共语；两侧各自
/// 归约到这里再比，口径差异与解码差异因此可分离。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectReason {
    /// 签名不符。
    BadSignature,
    /// IHDR 字段非法（颜色类型未定义 / 方法非 0 / 位深不在定义域）。
    IhdrField,
    /// 位深 × 颜色类型组合非法。
    IllegalCombo,
    /// 宽高为 0 或超上限。
    Dimension,
    /// 像素总量超预算。
    PixelBudget,
    /// IHDR 缺失。
    MissingIhdr,
    /// critical 块 CRC 不符。
    CrcCritical,
    /// 字节流在块结构或扫描线数据上截断。
    TruncatedChunk,
    /// zlib 流损坏（头非法 / 展开失败 / Adler 不符）。
    Zlib,
    /// 滤波号不在 0..=4。
    BadFilter,
    /// 调色板非法（长度非 3 倍数 / 索引越界 / 类型不允许）。
    Palette,
    /// tRNS 长度与颜色类型不匹配。
    TrnsLen,
    /// IDAT 缺失。
    NoIdat,
    /// 调色板图缺 PLTE。
    PaletteRequired,
    /// 未知 critical 块。
    UnknownCritical,
}

/// 拒绝深度（容器级 / 解码级）——畸形表按此对账，防止把「该在解码层拒的」
/// 写成「容器级」而让防线提前。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectDepth {
    /// 容器级：块游标 / IHDR 校验阶段即拒。
    Container,
    /// 解码级：zlib 展开 / 反滤波 / 色彩展开阶段才拒。
    Decode,
}

impl RejectReason {
    /// 被测侧（F1001）故障类别 → 分类。
    ///
    /// `_ => None`：ancillary CRC 告警、隔行未覆盖、缓冲不足、IDAT 块数闸
    /// **不在畸形语料的期望集合里**——它们不是「这张图坏了」的判据
    /// （前者照旧解码，后者是内部资源闸）。给出 `None` 而不是硬塞一个分类，
    /// 是为了让「没对上」表现为**缺项**而非「凑出一个错的分类」。
    pub fn from_fault(k: dec::FaultKind) -> Option<RejectReason> {
        use dec::FaultKind as K;
        Some(match k {
            K::BadSignature => RejectReason::BadSignature,
            K::IhdrField => RejectReason::IhdrField,
            K::IllegalCombo => RejectReason::IllegalCombo,
            K::Dimension => RejectReason::Dimension,
            K::PixelBudget => RejectReason::PixelBudget,
            K::MissingIhdr => RejectReason::MissingIhdr,
            K::CrcCritical => RejectReason::CrcCritical,
            K::TruncatedChunk => RejectReason::TruncatedChunk,
            K::Zlib => RejectReason::Zlib,
            K::BadFilter => RejectReason::BadFilter,
            K::Palette => RejectReason::Palette,
            K::TrnsLen => RejectReason::TrnsLen,
            K::NoIdat => RejectReason::NoIdat,
            K::PaletteRequired => RejectReason::PaletteRequired,
            K::UnknownCritical => RejectReason::UnknownCritical,
            _ => return None,
        })
    }

    /// 中文短名（矩阵行「理由」列的可读形态）。
    pub fn label(self) -> &'static str {
        match self {
            RejectReason::BadSignature => "签名不符",
            RejectReason::IhdrField => "IHDR 字段非法",
            RejectReason::IllegalCombo => "位深×类型非法组合",
            RejectReason::Dimension => "尺寸非法",
            RejectReason::PixelBudget => "像素预算超限",
            RejectReason::MissingIhdr => "IHDR 缺失",
            RejectReason::CrcCritical => "critical CRC 错",
            RejectReason::TruncatedChunk => "字节流截断",
            RejectReason::Zlib => "zlib 流损坏",
            RejectReason::BadFilter => "滤波号非法",
            RejectReason::Palette => "调色板非法",
            RejectReason::TrnsLen => "tRNS 长度不符",
            RejectReason::NoIdat => "IDAT 缺失",
            RejectReason::PaletteRequired => "调色板图缺 PLTE",
            RejectReason::UnknownCritical => "未知 critical 块",
        }
    }

    /// 该分类要求的拒绝深度。
    pub fn depth(self) -> RejectDepth {
        match self {
            RejectReason::BadSignature
            | RejectReason::IhdrField
            | RejectReason::IllegalCombo
            | RejectReason::Dimension
            | RejectReason::PixelBudget
            | RejectReason::MissingIhdr
            | RejectReason::CrcCritical
            | RejectReason::TrnsLen
            | RejectReason::NoIdat
            | RejectReason::PaletteRequired
            | RejectReason::UnknownCritical => RejectDepth::Container,
            RejectReason::TruncatedChunk | RejectReason::Zlib | RejectReason::BadFilter
            | RejectReason::Palette => RejectDepth::Decode,
        }
    }

    /// 分类全集（覆盖矩阵按此逐条对账）。
    pub const ALL: [RejectReason; 15] = [
        RejectReason::BadSignature,
        RejectReason::IhdrField,
        RejectReason::IllegalCombo,
        RejectReason::Dimension,
        RejectReason::PixelBudget,
        RejectReason::MissingIhdr,
        RejectReason::CrcCritical,
        RejectReason::TruncatedChunk,
        RejectReason::Zlib,
        RejectReason::BadFilter,
        RejectReason::Palette,
        RejectReason::TrnsLen,
        RejectReason::NoIdat,
        RejectReason::PaletteRequired,
        RejectReason::UnknownCritical,
    ];
}

// ===========================================================================
// 六、17 张畸形语料图
// ===========================================================================

/// 畸形注入方式（**计划层**变异：改字段，不改字节）。
///
/// 计划层变异能说清「改了哪个字段、CRC 重不重算」；字节层乱敲只能说
/// 「某字节翻了」——后者没法预期拒绝理由，而锚点要的正是逐图核对理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// 签名第 2 字节改成 'X'。
    BreakSignature,
    /// IHDR 颜色类型改成未定义值 7（CRC 重算）。
    IhdrColorUndefined,
    /// IHDR 位深改成 3（不在 1/2/4/8/16 单点定义域；CRC 重算）。
    IhdrDepthUndefined,
    /// 真彩 + 位深 4：位深合法而组合非法（两级闸的分界；CRC 重算）。
    IhdrIllegalCombo,
    /// IHDR 宽度改成 0（CRC 重算）。
    IhdrWidthZero,
    /// IHDR 尺寸改成 2^31−1 方形（CRC 重算）：结构合法、预算超限。
    IhdrHugeDims,
    /// 假尺寸：宽度翻倍且 CRC 重算——只有真解码才会发现扫描线不够。
    FakeDimsGrow,
    /// 首个块不是 IHDR（块序违规）。
    DropIhdr,
    /// 指定 critical 块的 CRC 翻一位。
    BreakCrc(Fourcc),
    /// IEND 的 CRC 翻一位（尾部块也要验）。
    BreakIendCrc,
    /// IDAT 载荷中途截断（块长同步改短、CRC 重算）：zlib 流不完整。
    TruncateIdatPayload,
    /// 签名后只留 4 字节：块头中途断掉。
    TruncateChunkHeader,
    /// zlib 头的 CM 改成 9（压缩方法域非法）。
    ZlibBadMethod,
    /// 第 0 行滤波号改成 5（规范只允许 0..=4）。
    BadFilterByte,
    /// 调色板索引越界（索引 = PLTE 项数）。
    PaletteIndexOutOfRange,
    /// 灰度 tRNS 载荷改成 1 字节（规范要求 2）。
    TrnsShortGray,
    /// 不写 IDAT。
    DropIdat,
    /// 调色板图不写 PLTE。
    DropPlte,
    /// 插入未知 critical 块 `ABCD`。
    UnknownCriticalChunk,
}

/// 一张畸形语料图的场景配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CorruptScenario {
    /// 图 ID。
    pub id: &'static str,
    /// 变异基图（必须是 [`VALID_SCENARIOS`] 里的 ID——畸形图不是凭空造的）。
    pub base: &'static str,
    /// 注入方式。
    pub mutation: Mutation,
    /// **独立声明**的期望拒绝理由。
    pub expect: RejectReason,
    /// 机制说明（逐图核对拒绝理由的「核对凭据」）。
    pub mechanism: &'static str,
}

/// 17 张畸形语料图。
///
/// 覆盖 [`RejectReason::ALL`] 的全部 15 个分类（CRC 一类三图、截断一类两图，
/// 用来证明**同一分类下不同注入点也各自成立**），由 `reason_coverage` 逐条断言。
pub const CORRUPT_SCENARIOS: [CorruptScenario; 17] = [
    CorruptScenario {
        id: "xsig01",
        base: "basn0g08",
        mutation: Mutation::BreakSignature,
        expect: RejectReason::BadSignature,
        mechanism: "签名第 2 字节非 'N'：容器解析第一步即拒",
    },
    CorruptScenario {
        id: "xihd01",
        base: "basn2c08",
        mutation: Mutation::IhdrColorUndefined,
        expect: RejectReason::IhdrField,
        mechanism: "颜色类型 7 未定义（CRC 正确——证明拒的不是 CRC）",
    },
    CorruptScenario {
        id: "xihd02",
        base: "basn0g08",
        mutation: Mutation::IhdrDepthUndefined,
        expect: RejectReason::IhdrField,
        mechanism: "位深 3 不在 1/2/4/8/16 单点定义域",
    },
    CorruptScenario {
        id: "xcmb01",
        base: "basn2c08",
        mutation: Mutation::IhdrIllegalCombo,
        expect: RejectReason::IllegalCombo,
        mechanism: "真彩 + 位深 4：位深合法而组合非法（两级闸的分界）",
    },
    CorruptScenario {
        id: "xhdr01",
        base: "basn0g08",
        mutation: Mutation::FakeDimsGrow,
        expect: RejectReason::TruncatedChunk,
        mechanism: "假尺寸：宽度翻倍且 CRC 重算——只有真解码才会发现扫描线不够",
    },
    CorruptScenario {
        id: "xdim01",
        base: "basn0g08",
        mutation: Mutation::IhdrWidthZero,
        expect: RejectReason::Dimension,
        mechanism: "宽度 0：规范 §4.2.1 尺寸域的零位",
    },
    CorruptScenario {
        id: "xbud01",
        base: "basn0g08",
        mutation: Mutation::IhdrHugeDims,
        expect: RejectReason::PixelBudget,
        mechanism: "2^31−1 方形：结构合法但像素总量超预算闸",
    },
    CorruptScenario {
        id: "xihdM01",
        base: "basn0g08",
        mutation: Mutation::DropIhdr,
        expect: RejectReason::MissingIhdr,
        mechanism: "首个块不是 IHDR：块序违规",
    },
    CorruptScenario {
        id: "xcrc01",
        base: "basn2c08",
        mutation: Mutation::BreakCrc(Fourcc::IHDR),
        expect: RejectReason::CrcCritical,
        mechanism: "IHDR CRC 翻一位（锚点点名的 xcrc）",
    },
    CorruptScenario {
        id: "xcrc02",
        base: "basn0g08",
        mutation: Mutation::BreakCrc(Fourcc::IDAT),
        expect: RejectReason::CrcCritical,
        mechanism: "IDAT CRC 翻一位：数据块级的完整性闸",
    },
    CorruptScenario {
        id: "xcrc03",
        base: "basn0g08",
        mutation: Mutation::BreakIendCrc,
        expect: RejectReason::CrcCritical,
        mechanism: "IEND CRC 翻一位：尾部块也要验（漏验等于没验）",
    },
    CorruptScenario {
        id: "xsng01",
        base: "basn0g08",
        mutation: Mutation::TruncateIdatPayload,
        expect: RejectReason::Zlib,
        mechanism: "IDAT 载荷中途截断（锚点点名的 xsng）：zlib 流不完整",
    },
    CorruptScenario {
        id: "xsng02",
        base: "basn0g08",
        mutation: Mutation::TruncateChunkHeader,
        expect: RejectReason::TruncatedChunk,
        mechanism: "块头中途断掉：与载荷截断是两条不同的路径",
    },
    CorruptScenario {
        id: "xzlb01",
        base: "basn0g08",
        mutation: Mutation::ZlibBadMethod,
        expect: RejectReason::Zlib,
        mechanism: "zlib 头 CM=9：压缩方法域非法",
    },
    CorruptScenario {
        id: "xflt01",
        base: "basn0g08",
        mutation: Mutation::BadFilterByte,
        expect: RejectReason::BadFilter,
        mechanism: "第 0 行滤波号 5：反滤波前必须查域",
    },
    CorruptScenario {
        id: "xplt01",
        base: "basn3p08",
        mutation: Mutation::PaletteIndexOutOfRange,
        expect: RejectReason::Palette,
        mechanism: "调色板索引 = 项数：越界索引必须显性拒绝（夹取到末项 = 解出错图）",
    },
    CorruptScenario {
        id: "xtrn01",
        base: "f02n0g08",
        mutation: Mutation::TrnsShortGray,
        expect: RejectReason::TrnsLen,
        mechanism: "灰度 tRNS 载荷 1 字节（规范要求 2）：长度错必须拒而非补零",
    },
];

/// 按 ID 取畸形场景（找不到给 `None`）。
pub fn corrupt_index_of(id: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < CORRUPT_SCENARIOS.len() {
        if CORRUPT_SCENARIOS[i].id == id {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 按 ID 取正常场景（找不到给 `None`）。
pub fn valid_index_of(id: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < VALID_SCENARIOS.len() {
        if VALID_SCENARIOS[i].id == id {
            return Some(i);
        }
        i += 1;
    }
    None
}
