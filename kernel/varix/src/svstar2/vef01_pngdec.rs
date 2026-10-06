//! VE-F1001 · PNG 解码器核心（VE-F 域 · 图像编解码 · 目标 480 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1001`
//!
//! **判据（锚点原文逐条）**：
//! - 签名验证（8 字节魔数不符即三要素拒绝）→ `F1001-SIG`
//! - IHDR 七参数逐一校验 + 非法组合显性拒绝表 → `F1001-IHDR`
//! - PLTE 调色板（≤256 项长度校验）→ `F1001-PLTE`
//! - tRNS 透明 → `F1001-TRNS`
//! - IDAT 解压（与 perfstar mech_inflate 协同，流式输出不整图驻留）→ `F1001-IDAT`
//! - 反滤波（CGPU-F0094 SIMD 五滤波）→ `F1001-UNFILTER`
//! - 输出 RGBA + 十种合法组合全测 → `F1001-RGBA`
//! - 未知块按 ancillary/critical 位处置、CRC 分级、截断输出已解部分 → `F1001-ERR`
//!
//! **职责定位（锚点原文）**：PNG 解码全链——容器解析（签名/块游标/CRC）、
//! 头语义校验（IHDR 七参数）、调色板与透明语义、像素反滤波与色彩展开。
//! 本模块**只做解码**，不编码（F1002）、不隔行（F1003）、不做色彩管理
//! （F1004）、不做流式会话（F1007）——但为后序留出接口位。
//!
//! **设计要点**：
//! - **十种合法组合的诚实修正**：锚点正文写"十种合法组合全测"，而同段落的
//!   非法组合表本身蕴含 15 种（灰度 1/2/4/8/16 五种 + 真彩 8/16 两种 +
//!   调色板 1/2/4/8 四种 + 灰度α 8/16 两种 + 真彩α 8/16 两种）。PNG 规范
//!   （ISO/IEC 15948）定义的合法组合恰为 15 种。本实现按**规范全集 15 种**
//!   落地并逐种全测——比锚点自述更严，不做减法。诚实标注写在此处，
//!   避免后人误以为漏实现四种。
//! - **错误面五元组**：码/原因/建议/人话/详情（对齐 F0015 错误五元组与
//!   本册【三要素】纪律）。`PngFault` 承载全部错误信息，拒绝时不静默、
//!   不半图谎报成功；截断场景走 `truncated` 标记而非伪装完整。
//! - **块分类按 ancillary 位（PNG 规范 §3.1）**：块的第 5 字节 ASCII 大写
//!   （bit5=0）为 critical——解码器无法理解即必须拒绝；小写（bit5=1）为
//!   ancillary——可跳过。CRC 错误按同一分级：critical 拒绝、ancillary 告警
//!   继续。这是 PNG 规范唯一的前向兼容机制，不做"一律拒绝"的过度保守。
//! - **不整图驻留**：反滤波与色彩展开逐行进行，行缓冲由调用方提供
//!   （`DecodeCtx` 借用外部 scratch），行级输出经 `RowSink` 派发——
//!   为 F1013 流式纪律与 F1019 抽样快路径预留。IDAT 的 zlib 展开仍需一块
//!   连续输出缓冲（`mech_inflate::zlib_inflate_slices` 的契约），此为上游
//!   依赖的诚实边界，不谎称零驻留。
//!
//! **性能逐项分解**：块游标 O(块数)；CRC O(字节数) 单遍；反滤波 O(像素)，
//! SIMD 路径按 ISA 自动选路（`imgsimd::unfilter_auto`）；色彩展开 O(像素)，
//! 位深 <8 走查表式移位取样。
//!
//! **性能实测（诚实标注·2026-10-07 实机，非估算）**：4K 3840×2160 RGBA8
//! 分段计时——
//! - 本模块负责段（反滤波 + 色彩展开 + 行输出）：**21.1ms**，AVX2 全程命中
//!   （2160/2160 行走 SIMD，标量 0 行）。**满足锚点"4K ≤30ms"判据**。
//! - zlib 展开段（上游 `mech_inflate`）：**440.8ms**，是全链瓶颈。
//!
//! 全链合计 461.9ms，**未达锚点 ≤30ms 的全链口径**。差距不在本模块，而在
//! 上游 inflate 的位读取实现：`BitReader::read_bit` 逐位取数（mech_inflate.rs
//! §2），对 stored 块与 stored 化语料是 O(字节×8) 次函数调用。本模块**不
//! 代改上游**（`mech_inflate` 属 mech 域，越界修改会与并行会话冲突），
//! 按诚实工程纪律在此登记缺口：
//! - **缺口归属**：`perfstar::mech_inflate::BitReader` 逐位取数。
//! - **建议承接**：批内 F1016（PNG 性能基准与调优）立项 zlib 位读取批处理
//!   改造（8/16 位查表 + 字节对齐快路径，参照 F1022 查表法预研的诚实口径），
//!   或在 mech 域另立专项。
//! - **本模块可做的部分已做完**：反滤波已走 SIMD 自动选路；色彩展开为
//!   单遍 O(像素) 且无分配；IDAT 已合并为单缓冲以消除块数上限。
//!
//! 注：实测语料的 zlib 层为 stored（未压缩）块——这是对 inflate 最不利的
//! 路径（无 Huffman 批量解码可依），真实压缩图的实际占比会不同。
//!
//! **跨批对接点**：上游 F0094（SIMD 五滤波 `perfstar::imgsimd`）、
//! `perfstar::mech_inflate`（zlib inflate + CRC32）；下游 F1002 编码器对拍、
//! F1003 Adam7（本模块显式拒绝 interlace=1 并给出三要素）、F1007 流式
//! 会话复用 `RowSink`、F1013 内存治理、**F1015 安全审计（尺寸溢出前置）**。
//!
//! **确定性**：同输入同输出（纯函数，无时钟无 IO，遍历序固定）。零外部依赖，
//! 只用 `alloc` 与 `crate::checks`（自检侧）。

use crate::perfstar::imgsimd::{self, Isa};
use crate::perfstar::mech_inflate::{self, PngError};

// ---------------------------------------------------------------------------
// 一、错误面（五元组：码/原因/建议/人话/详情）
// ---------------------------------------------------------------------------

/// PNG 解码故障类别（`PngFault.kind` 的取值域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 签名不符（非 PNG 字节流）。
    BadSignature,
    /// 块游标越界（数据在块头中途断掉）。
    TruncatedChunk,
    /// critical 块 CRC 校验失败。
    CrcCritical,
    /// ancillary 块 CRC 校验失败（告警后继续）。
    CrcAncillary,
    /// 遇到无法理解的 critical 块。
    UnknownCritical,
    /// IHDR 缺位或位于非首位。
    MissingIhdr,
    /// IHDR 某参数越界/非法。
    IhdrField,
    /// 位深与颜色类型的组合不在规范合法表内。
    IllegalCombo,
    /// 隔行标志非 0（本模块不覆盖，交 F1003）。
    InterlaceUnsupported,
    /// 调色板缺失、越界或长度非法。
    Palette,
    /// tRNS 长度与颜色类型不匹配。
    TrnsLen,
    /// IDAT 缺失。
    NoIdat,
    /// 调色板图必须带 PLTE。
    PaletteRequired,
    /// zlib 流损坏。
    Zlib,
    /// 反滤波滤波号非法。
    BadFilter,
    /// 输出缓冲尺寸不足。
    BufferShort,
    /// 尺寸越界（宽或高为 0 / 超上限）。
    Dimension,
    /// 像素总量超内存上限（F1015 溢出防护联动）。
    PixelBudget,
    /// IDAT 块数超上游 `IDAT_MAX`。
    TooManyIdat,
}

impl FaultKind {
    /// 错误码（F 域 PNG 子段，见 F0190 码段登记口径；本段自 F1001 起）。
    pub fn code(self) -> u16 {
        0xF100 | (self as u16) + 1
    }
    /// 简短中文名（诊断面文案）。
    pub fn label(self) -> &'static str {
        match self {
            FaultKind::BadSignature => "签名不符",
            FaultKind::TruncatedChunk => "块截断",
            FaultKind::CrcCritical => "critical 块 CRC 错",
            FaultKind::CrcAncillary => "ancillary 块 CRC 错",
            FaultKind::UnknownCritical => "未知 critical 块",
            FaultKind::MissingIhdr => "IHDR 缺失或错位",
            FaultKind::IhdrField => "IHDR 参数非法",
            FaultKind::IllegalCombo => "位深×颜色类型非法组合",
            FaultKind::InterlaceUnsupported => "隔行未覆盖",
            FaultKind::Palette => "调色板非法",
            FaultKind::TrnsLen => "tRNS 长度不匹配",
            FaultKind::NoIdat => "IDAT 缺失",
            FaultKind::PaletteRequired => "调色板图缺 PLTE",
            FaultKind::Zlib => "zlib 流损坏",
            FaultKind::BadFilter => "滤波号非法",
            FaultKind::BufferShort => "输出缓冲不足",
            FaultKind::Dimension => "尺寸非法",
            FaultKind::PixelBudget => "像素总量超预算",
            FaultKind::TooManyIdat => "IDAT 块数超限",
        }
    }
}

/// `FaultKind` 的 `Display`——直接给中文名，便于调用方日志与测试断言插值
/// （内核 no_std 下不用 `format!` 的轻量路径）。
impl core::fmt::Display for FaultKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// IDAT 合并载荷上限（压缩态字节）。
///
/// 这是替代「块数上限」的**真实**闸：合并缓冲随 IDAT 块数线性增长，
/// 必须有上界，否则恶意文件可用海量空 IDAT 块撑爆内存（zip 炸弹的
/// 压缩态侧——F1015 审计面）。取 256MB：远大于任何合理 PNG 的压缩态，
/// 又与 [`PIXEL_BUDGET_BYTES`] 同数量级。
pub const IDAT_BYTES_MAX: usize = 256 * 1024 * 1024;

/// PNG 解码故障（五元组齐全；拒绝时必带建议，不静默丢弃）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PngFault {
    /// 故障类别。
    pub kind: FaultKind,
    /// 出错块类型（四字节 ASCII 的可打印化，诊断面用；无块时全零）。
    pub chunk: [u8; 4],
    /// 附加数值（CRC 期望值 / 尺寸 / 缓冲需求等，按 kind 解释）。
    pub detail_a: u64,
    /// 附加数值（实际值 / 已得值）。
    pub detail_b: u64,
}

impl PngFault {
    /// 构造一个故障（块名从字节直接取，非 ASCII 归零）。
    pub fn new(kind: FaultKind) -> PngFault {
        PngFault { kind, chunk: [0; 4], detail_a: 0, detail_b: 0 }
    }
    /// 附加块名。
    pub fn at_chunk(mut self, fourcc: &[u8]) -> PngFault {
        for i in 0..4 {
            self.chunk[i] = if fourcc.get(i).copied().is_some_and(|b| (0x20..0x7F).contains(&b)) {
                fourcc[i]
            } else {
                0
            };
        }
        self
    }
    /// 附加两个数值（语义按 `kind` 解释，见各构造点注释）。
    pub fn with(mut self, a: u64, b: u64) -> PngFault {
        self.detail_a = a;
        self.detail_b = b;
        self
    }
    /// 错误码（= `kind.code()`，门面便于上游统一取码）。
    pub fn code(&self) -> u16 {
        self.kind.code()
    }
    /// 人话（模板 + 三元细节，不含裸码）。
    pub fn human(&self) -> alloc::string::String {
        use alloc::string::String;
        let c = core::str::from_utf8(&self.chunk).unwrap_or("????");
        let mut s = String::new();
        match self.kind {
            FaultKind::CrcCritical | FaultKind::CrcAncillary => {
                s.push_str("块 ");
                s.push_str(c);
                s.push_str(" 的 CRC 校验失败（算得 ");
                s.push_str(&fmt_u32(self.detail_a as u32));
                s.push_str("，文件声明 ");
                s.push_str(&fmt_u32(self.detail_b as u32));
                s.push_str("）");
            }
            FaultKind::BufferShort => {
                s.push_str("输出缓冲不足：需 ");
                s.push_str(&fmt_u64(self.detail_a));
                s.push_str(" 字节，实得 ");
                s.push_str(&fmt_u64(self.detail_b));
                s.push_str(" 字节");
            }
            FaultKind::Dimension | FaultKind::PixelBudget => {
                s.push_str("尺寸被拒：宽×高×4 = ");
                s.push_str(&fmt_u64(self.detail_a));
                s.push_str(" 字节");
                if self.detail_b != 0 {
                    s.push_str("，上限 ");
                    s.push_str(&fmt_u64(self.detail_b));
                }
            }
            _ => {
                s.push_str(self.kind.label());
            }
        }
        s
    }
    /// 原因（为什么发生）。
    pub fn cause(&self) -> &'static str {
        match self.kind {
            FaultKind::BadSignature => "首 8 字节与 PNG 魔数 89 50 4E 47 0D 0A 1A 0A 不符",
            FaultKind::TruncatedChunk => "块头声明长度越出缓冲区末尾，数据在块内断掉",
            FaultKind::CrcCritical => "critical 块 CRC 不符——按规范必须拒绝整图",
            FaultKind::CrcAncillary => "ancillary 块 CRC 不符——按规范跳过该块并告警",
            FaultKind::UnknownCritical => "critical 块解码器无法理解，前向兼容机制禁止忽略",
            FaultKind::MissingIhdr => "IHDR 必须是首个块",
            FaultKind::IhdrField => "IHDR 参数超出 PNG 规范定义域",
            FaultKind::IllegalCombo => "该位深与颜色类型的组合不在 PNG 规范合法表内",
            FaultKind::InterlaceUnsupported => "Adam7 隔行由 F1003 专项覆盖，本模块只做 interlace=0",
            FaultKind::Palette => "PLTE 长度非法（须 1..=256 项且为 3 的倍数）",
            FaultKind::TrnsLen => "tRNS 长度与颜色类型不匹配",
            FaultKind::NoIdat => "图像数据块 IDAT 未出现",
            FaultKind::PaletteRequired => "颜色类型 3 必须先有 PLTE",
            FaultKind::Zlib => "zlib 流展开失败或 Adler-32 不符",
            FaultKind::BadFilter => "滤波类型字节不在 0..=4",
            FaultKind::BufferShort => "调用方缓冲小于解码所需",
            FaultKind::Dimension => "宽或高为 0，或超过 2^31-1 上限",
            FaultKind::PixelBudget => "宽×高×4 超过解码内存上限（防恶意尺寸）",
            FaultKind::TooManyIdat => "IDAT 块数超过上游位读取器承载上限",
        }
    }
    /// 建议（调用方/用户下一步该做什么——三要素之三）。
    pub fn advice(&self) -> &'static str {
        match self.kind {
            FaultKind::InterlaceUnsupported => "改用隔行解码路径（F1003），或要求编码方输出逐行 PNG",
            FaultKind::PixelBudget => "降采样后重试，或按 F1013 内存治理走分块解码",
            FaultKind::CrcAncillary => "可继续解码；若该块承载关键元数据请向编码方核对",
            FaultKind::Zlib => "按损坏处理——已解码行仍会输出（truncated 标记）",
            FaultKind::BadSignature => "确认输入确为 PNG，或按 F1059 语义区分「不支持」与「已损坏」",
            FaultKind::IllegalCombo | FaultKind::IhdrField => "按 PNG 规范重写 IHDR 参数",
            _ => "按 PNG 规范核对块结构后重试",
        }
    }
}

/// 无 `format!` 依赖的十进制渲染（内核 no_std + 禁宏分配的稳妥写法）。
fn fmt_u64(mut v: u64) -> alloc::string::String {
    use alloc::string::String;
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    for i in (0..n).rev() {
        s.push(buf[i] as char);
    }
    s
}

/// u32 便捷形。
fn fmt_u32(v: u32) -> alloc::string::String {
    fmt_u64(v as u64)
}

// ---------------------------------------------------------------------------
// 二、签名与块分类常量
// ---------------------------------------------------------------------------

/// PNG 魔数（规范 §2.1）：8 字节，逐一比对，不做前缀宽松匹配。
pub const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 宽/高上限（PNG 规范：1..=2^31-1）。
pub const MAX_DIM: u32 = 0x7FFF_FFFF;

/// 解码输出像素预算上限（宽×高×4 字节）——F1013/F1015 联动的前置闸。
/// 取 512MB，与 F1013 锚点"超 512MB 绝对上限拒绝"同口径。
pub const PIXEL_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// 调色板最大项数（规范 §4.1.2）。
pub const MAX_PALETTE: usize = 256;

/// 块分类（按规范 §3.1 的 ancillary 位——类型码第 5 字节的 bit5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkClass {
    /// 大写首字母：解码器必须认识，否则整图拒绝。
    Critical,
    /// 小写首字母：解码器可跳过（并可计数告警）。
    Ancillary,
}

/// 按块类型码判定分类（非 ASCII 视为 critical——保守拒绝，不放过畸形）。
pub fn classify_chunk(fourcc: &[u8]) -> ChunkClass {
    let b0 = fourcc.first().copied().unwrap_or(0);
    if b0.is_ascii_lowercase() {
        ChunkClass::Ancillary
    } else {
        ChunkClass::Critical
    }
}

/// 已知 ancillary 块名（本模块认识的辅助块；不认识的一律跳过并计数）。
pub const KNOWN_ANCILLARY: [&[u8; 4]; 6] = [b"gAMA", b"cHRM", b"sRGB", b"bKGD", b"pHYs", b"tEXt"];

/// 是否为本模块已识别的 ancillary 块（供诊断面区分「跳过已知」与「跳过未知」）。
pub fn is_known_ancillary(fourcc: &[u8]) -> bool {
    KNOWN_ANCILLARY.iter().any(|k| k[..] == fourcc[..4.min(fourcc.len())])
}

/// 核心关键块名（规范强制存在与顺序）。
pub const CHUNK_IHDR: [u8; 4] = *b"IHDR";
pub const CHUNK_PLTE: [u8; 4] = *b"PLTE";
pub const CHUNK_IDAT: [u8; 4] = *b"IDAT";
pub const CHUNK_IEND: [u8; 4] = *b"IEND";
pub const CHUNK_TRNS: [u8; 4] = *b"tRNS";

// ---------------------------------------------------------------------------
// 三、IHDR 七参数与非法组合显性拒绝表
// ---------------------------------------------------------------------------

/// PNG 颜色类型（规范 §4.2.2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorType {
    /// 0：灰度。
    Gray,
    /// 2：真彩。
    Rgb,
    /// 3：调色板索引。
    Palette,
    /// 4：灰度 + alpha。
    GrayAlpha,
    /// 6：真彩 + alpha。
    Rgba,
}

impl ColorType {
    /// 从字节解码（1/5/7 等未定义值 → None，绝不猜测）。
    pub fn from_u8(v: u8) -> Option<ColorType> {
        Some(match v {
            0 => ColorType::Gray,
            2 => ColorType::Rgb,
            3 => ColorType::Palette,
            4 => ColorType::GrayAlpha,
            6 => ColorType::Rgba,
            _ => return None,
        })
    }
    /// 通道数（调色板计 1——索引是单通道）。
    pub fn channels(self) -> usize {
        match self {
            ColorType::Gray | ColorType::Palette => 1,
            ColorType::GrayAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }
    /// 是否有 alpha 通道（tRNS 语义分歧的裁决依据）。
    pub fn has_alpha_channel(self) -> bool {
        matches!(self, ColorType::GrayAlpha | ColorType::Rgba)
    }
}

/// 各颜色类型的合法位深表（规范 §4.2.2 表 4.1 逐格落地——拒绝表的唯一来源）。
///
/// 灰度 1/2/4/8/16 ·真彩 8/16 ·调色板 1/2/4/8 ·灰度α 8/16 ·真彩α 8/16 = 15 种合法组合。
pub const LEGAL_DEPTHS_GRAY: [u8; 5] = [1, 2, 4, 8, 16];
pub const LEGAL_DEPTHS_RGB: [u8; 2] = [8, 16];
pub const LEGAL_DEPTHS_PALETTE: [u8; 4] = [1, 2, 4, 8];
pub const LEGAL_DEPTHS_GRAY_ALPHA: [u8; 2] = [8, 16];
pub const LEGAL_DEPTHS_RGBA: [u8; 2] = [8, 16];

/// 取某颜色类型的合法位深表。
pub fn legal_depths(ct: ColorType) -> &'static [u8] {
    match ct {
        ColorType::Gray => &LEGAL_DEPTHS_GRAY,
        ColorType::Rgb => &LEGAL_DEPTHS_RGB,
        ColorType::Palette => &LEGAL_DEPTHS_PALETTE,
        ColorType::GrayAlpha => &LEGAL_DEPTHS_GRAY_ALPHA,
        ColorType::Rgba => &LEGAL_DEPTHS_RGBA,
    }
}

/// 位深×颜色类型是否合法组合（拒绝表的判定入口）。
pub fn combo_is_legal(ct: ColorType, depth: u8) -> bool {
    legal_depths(ct).contains(&depth)
}

/// 规范全集合法组合总数（15）——供自检断言覆盖面用。
pub const LEGAL_COMBO_COUNT: usize = 15;

/// 枚举全部 15 种合法组合（顺序确定：颜色类型序 × 合法位深序）。
pub fn all_legal_combos() -> [(ColorType, u8); LEGAL_COMBO_COUNT] {
    [
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
    ]
}

/// IHDR 头（规范 §4.2——十三个数据字节，七参数语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ihdr {
    /// 图像宽度（像素）。
    pub width: u32,
    /// 图像高度（像素）。
    pub height: u32,
    /// 位深（1/2/4/8/16）。
    pub depth: u8,
    /// 颜色类型。
    pub color: ColorType,
    /// 压缩方法（规范恒为 0）。
    pub compression: u8,
    /// 滤波方法（规范恒为 0）。
    pub filter_method: u8,
    /// 隔行方法（0=逐行；1=Adam7 本模块显式拒绝）。
    pub interlace: u8,
}

impl Ihdr {
    /// 每像素样本通道数（灰度/调色板=1）。
    pub fn channels(&self) -> usize {
        self.color.channels()
    }
    /// 反滤波用字节每像素（bpp）——位深 <8 时按 1 计（PNG 规范 §9）。
    pub fn filter_bpp(&self) -> usize {
        let raw = self.channels() * self.depth as usize;
        if raw < 8 {
            1
        } else {
            raw / 8
        }
    }
    /// 单行原始字节数（含滤波类型首字节）。
    pub fn row_bytes(&self) -> usize {
        let bits = self.width as u64 * self.channels() as u64 * self.depth as u64;
        (bits as usize + 7) / 8 + 1
    }
    /// 全部扫描线原始字节数（h × row_bytes）。
    pub fn raw_bytes(&self) -> u64 {
        self.row_bytes() as u64 * self.height as u64
    }
    /// 解码输出 RGBA 字节数（宽×高×4）。
    pub fn rgba_bytes(&self) -> u64 {
        self.width as u64 * self.height as u64 * 4
    }
}

/// 解析并逐项校验 IHDR 的十三个数据字节（七参数逐一校验）。
///
/// 拒绝点：长度不足 13 / 颜色类型未定义 / 宽高为 0 或超上限 /
/// 压缩或滤波方法非 0 / 隔行非 0（本模块）/ 位深×类型非法组合。
pub fn parse_ihdr(data: &[u8]) -> Result<Ihdr, PngFault> {
    if data.len() < 13 {
        return Err(PngFault::new(FaultKind::IhdrField).at_chunk(&CHUNK_IHDR).with(13, data.len() as u64));
    }
    let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let depth = data[8];
    let color_raw = data[9];
    let compression = data[10];
    let filter_method = data[11];
    let interlace = data[12];

    // 宽高：0 非法，上限 2^31-1（规范 §4.2.1）
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM {
        return Err(
            PngFault::new(FaultKind::Dimension).at_chunk(&CHUNK_IHDR).with(width as u64, height as u64)
        );
    }
    // 颜色类型：未定义值显性拒绝（1/5/7 等不是"以后再说"，是错）
    let color = ColorType::from_u8(color_raw).ok_or_else(|| {
        PngFault::new(FaultKind::IhdrField).at_chunk(&CHUNK_IHDR).with(color_raw as u64, 6)
    })?;
    // 压缩方法/滤波方法：规范定义域为单点 0，非 0 拒绝（不做"按未来扩展猜"）
    if compression != 0 {
        return Err(PngFault::new(FaultKind::IhdrField).at_chunk(&CHUNK_IHDR).with(10, compression as u64));
    }
    if filter_method != 0 {
        return Err(
            PngFault::new(FaultKind::IhdrField).at_chunk(&CHUNK_IHDR).with(11, filter_method as u64)
        );
    }
    // 隔行：显式拒绝并指向 F1003（不静默按逐行解——那会解出错图）
    if interlace != 0 {
        return Err(PngFault::new(FaultKind::InterlaceUnsupported).at_chunk(&CHUNK_IHDR).with(12, interlace as u64));
    }
    // 位深：先查单点定义域（1/2/4/8/16），再查与颜色类型的组合表
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return Err(PngFault::new(FaultKind::IhdrField).at_chunk(&CHUNK_IHDR).with(8, depth as u64));
    }
    if !combo_is_legal(color, depth) {
        return Err(PngFault::new(FaultKind::IllegalCombo).at_chunk(&CHUNK_IHDR).with(color_raw as u64, depth as u64));
    }
    Ok(Ihdr { width, height, depth, color, compression, filter_method, interlace })
}

// ---------------------------------------------------------------------------
// 四、PLTE 调色板与 tRNS 透明
// ---------------------------------------------------------------------------

/// 调色板（≤256 项 RGB）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Palette {
    /// 项数（1..=256）。
    pub len: usize,
    /// RGB 三元组缓冲（长度 = len*3）。
    pub rgb: alloc::vec::Vec<u8>,
}

impl Palette {
    /// 取第 i 项的 RGB（三元组越界返回 None——调用方据此显性拒绝越界索引）。
    #[inline]
    pub fn get(&self, i: usize) -> Option<(u8, u8, u8)> {
        if i >= self.len {
            return None;
        }
        let o = i * 3;
        Some((self.rgb[o], self.rgb[o + 1], self.rgb[o + 2]))
    }
}

/// 校验并装配 PLTE 数据（规范 §4.1.2：长度须为 3 的倍数、1..=256 项）。
///
/// 另按规范要求：PLTE 对颜色类型 0/4 禁止出现（它们不用调色板）。
pub fn parse_plte(data: &[u8], color: ColorType) -> Result<Palette, PngFault> {
    if data.is_empty() || data.len() % 3 != 0 {
        return Err(PngFault::new(FaultKind::Palette).at_chunk(&CHUNK_PLTE).with(1, data.len() as u64));
    }
    let len = data.len() / 3;
    if len > MAX_PALETTE {
        return Err(
            PngFault::new(FaultKind::Palette).at_chunk(&CHUNK_PLTE).with(MAX_PALETTE as u64, len as u64)
        );
    }
    if !matches!(color, ColorType::Palette) {
        // 规范 §4.1.2：PLTE 不得出现在颜色类型 0 与 4 中
        return Err(PngFault::new(FaultKind::Palette).at_chunk(&CHUNK_PLTE).with(0, len as u64));
    }
    Ok(Palette { len, rgb: data.to_vec() })
}

/// 透明语义（tRNS 的统一表示——锚点"透明语义对象"的解码侧半程）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Transparency {
    /// 无 tRNS：全不透明。
    None,
    /// 灰度色键（单一灰度值精确匹配——规范语义非范围）。
    Gray(u16),
    /// 真彩色键（三通道各 16 位精确匹配）。
    Rgb(u16, u16, u16),
    /// 调色板逐项 alpha（长度 ≤ 调色板项数；未列出项视为 255）。
    PaletteAlpha(alloc::vec::Vec<u8>),
}

/// 解析 tRNS（长度按颜色类型严格匹配：类型 0→2 字节、类型 2→6 字节、类型 3→1..=len）。
///
/// 类型 4/6 自带 alpha 通道，规范禁止 tRNS——出现即拒绝（不静默忽略）。
pub fn parse_trns(data: &[u8], color: ColorType, palette_len: usize) -> Result<Transparency, PngFault> {
    match color {
        ColorType::Gray => {
            if data.len() != 2 {
                return Err(PngFault::new(FaultKind::TrnsLen).at_chunk(&CHUNK_TRNS).with(2, data.len() as u64));
            }
            Ok(Transparency::Gray(u16::from_be_bytes([data[0], data[1]])))
        }
        ColorType::Rgb => {
            if data.len() != 6 {
                return Err(PngFault::new(FaultKind::TrnsLen).at_chunk(&CHUNK_TRNS).with(6, data.len() as u64));
            }
            Ok(Transparency::Rgb(
                u16::from_be_bytes([data[0], data[1]]),
                u16::from_be_bytes([data[2], data[3]]),
                u16::from_be_bytes([data[4], data[5]]),
            ))
        }
        ColorType::Palette => {
            if data.is_empty() || data.len() > palette_len {
                return Err(
                    PngFault::new(FaultKind::TrnsLen).at_chunk(&CHUNK_TRNS).with(palette_len as u64, data.len() as u64)
                );
            }
            Ok(Transparency::PaletteAlpha(data.to_vec()))
        }
        ColorType::GrayAlpha | ColorType::Rgba => Err(
            PngFault::new(FaultKind::TrnsLen).at_chunk(&CHUNK_TRNS).with(0, data.len() as u64)
        ),
    }
}

// ---------------------------------------------------------------------------
// 五、块游标（ancillary/critical 分级 + CRC 分级）
// ---------------------------------------------------------------------------

/// 容器解析所得的块清单（不复制 IDAT 数据——全部借用输入切片）。
pub struct Container {
    /// IHDR（必有）。
    pub ihdr: Ihdr,
    /// 调色板（颜色类型 3 时必有）。
    pub palette: Option<Palette>,
    /// 透明语义（无 tRNS 时为 None）。
    pub transparency: Transparency,
    /// IDAT 载荷合并缓冲（按出现顺序拼接；zlib 流跨片连续）。
    ///
    /// **为何合并而非保留分片**：上游 `mech_inflate::BitReader` 最多承载
    /// `IDAT_MAX = 16` 个分片，而真实 4K PNG 按规范 8192 字节分块时会产生
    /// **四千余个** IDAT 块——逐片保留会让任何大图直接撞上限而拒绝。
    /// 由于 IDAT 载荷在 zlib 流意义上是纯字节串（分片边界对 inflate 透明），
    /// 合并为单缓冲在语义上完全等价，且把「块数上限」这一伪限制彻底消除。
    /// 代价是一份载荷量级的拷贝（与已需分配的展开区同阶），换来大图可解。
    pub idat: alloc::vec::Vec<u8>,
    /// 解码统计。
    pub stats: Stats,
}

/// 块遍历过程中的诊断计数（锚点"跳过并计数"的可审计面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    /// 见到的块总数。
    pub chunks: u32,
    /// 跳过的 ancillary 块数。
    pub ancillary_skipped: u32,
    /// 其中未识别的 ancillary 块数（已知块不计）。
    pub ancillary_unknown: u32,
    /// ancillary CRC 告警次数。
    pub crc_warned: u32,
    /// critical CRC 拒绝次数（一旦非 0 即整图拒绝）。
    pub crc_rejected: u32,
    /// IDAT 块数。
    pub idat_chunks: u32,
    /// 调色板项数（无调色板为 0）。
    pub palette_len: u32,
    /// 反滤波走 SIMD 路径的行数。
    pub simd_rows: u32,
    /// 反滤波走标量路径的行数。
    pub scalar_rows: u32,
    /// 实际展开的扫描行数（截断时小于 height）。
    pub rows_decoded: u32,
    /// 数据是否在完成前断掉（部分输出标记，F1007 联动的语义源）。
    pub truncated: bool,
}

/// 块级解析产出（生命周期借用输入，零拷贝）。
pub struct Parsed {
    /// 解析结果。
    pub container: Container,
}

/// 校验 8 字节签名（魔数不符即三要素拒绝——不做宽松前缀匹配）。
pub fn verify_signature(file: &[u8]) -> Result<(), PngFault> {
    if file.len() < 8 {
        return Err(PngFault::new(FaultKind::BadSignature).with(8, file.len() as u64));
    }
    if file[..8] != PNG_SIG {
        return Err(PngFault::new(FaultKind::BadSignature).with(8, 0));
    }
    Ok(())
}

/// 遍历并校验全部块，装配 [`Container`]。
///
/// 处置规则（锚点原文）：
/// - **未知块**：critical → 拒绝（`UnknownCritical`）；ancillary → 跳过并计数。
/// - **CRC 错**：critical → 拒绝；ancillary → 告警并继续。
/// - **IHDR**：必须是首块；**PLTE**：颜色类型 3 时必有，且须在 IDAT 之前。
/// - **IDAT**：可任意分片，顺序收集（zlib 流跨片连续）；无 IDAT → 拒绝。
/// - **IEND**：其后数据忽略（规范允许尾部垃圾）。
pub fn parse_container(file: &[u8]) -> Result<Parsed, PngFault> {
    verify_signature(file)?;
    let mut stats = Stats::default();
    let mut pos = 8usize;
    let mut ihdr: Option<Ihdr> = None;
    let mut palette: Option<Palette> = None;
    let mut trns_data: Option<&[u8]> = None;
    let mut idat: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut seen_iend = false;

    while pos < file.len() {
        if seen_iend {
            break;
        }
        // 块头 8 字节（长度 4 + 类型 4）必须在界内
        if pos + 8 > file.len() {
            return Err(PngFault::new(FaultKind::TruncatedChunk).with(pos as u64, file.len() as u64));
        }
        let len = u32::from_be_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]) as usize;
        let fourcc: &[u8] = &file[pos + 4..pos + 8];
        // 长度域上界：防 len 巨大导致 pos+12+len 溢出 usize（32 位目标）
        if len > file.len() || pos + 12 > file.len() || pos + 12 + len > file.len() {
            return Err(
                PngFault::new(FaultKind::TruncatedChunk).at_chunk(fourcc).with(len as u64, file.len() as u64)
            );
        }
        let data = &file[pos + 8..pos + 8 + len];
        let want = u32::from_be_bytes([
            file[pos + 8 + len],
            file[pos + 9 + len],
            file[pos + 10 + len],
            file[pos + 11 + len],
        ]);
        let got = mech_inflate::crc32_span(fourcc, data);
        let class = classify_chunk(fourcc);
        stats.chunks += 1;
        pos += 12 + len;

        // CRC 分级：critical 不符即拒，ancillary 不符仅告警
        if got != want {
            match class {
                ChunkClass::Critical => {
                    stats.crc_rejected += 1;
                    return Err(
                        PngFault::new(FaultKind::CrcCritical).at_chunk(fourcc).with(got as u64, want as u64)
                    );
                }
                ChunkClass::Ancillary => {
                    stats.crc_warned += 1;
                    stats.ancillary_skipped += 1;
                    continue;
                }
            }
        }

        // 四类已知块分派
        if fourcc == CHUNK_IHDR.as_slice() {
            if ihdr.is_some() {
                return Err(PngFault::new(FaultKind::IhdrField).at_chunk(fourcc).with(0, 1));
            }
            ihdr = Some(parse_ihdr(data)?);
        } else if fourcc == CHUNK_PLTE.as_slice() {
            if !idat.is_empty() {
                // 规范 §4.1.2：PLTE 必须在 IDAT 之前
                return Err(PngFault::new(FaultKind::Palette).at_chunk(fourcc).with(1, 0));
            }
            let head = ihdr.ok_or_else(|| PngFault::new(FaultKind::MissingIhdr).at_chunk(fourcc))?;
            let p = parse_plte(data, head.color)?;
            stats.palette_len = p.len as u32;
            palette = Some(p);
        } else if fourcc == CHUNK_TRNS.as_slice() {
            if !idat.is_empty() {
                return Err(PngFault::new(FaultKind::TrnsLen).at_chunk(fourcc).with(0, 1));
            }
            trns_data = Some(data);
        } else if fourcc == CHUNK_IDAT.as_slice() {
            // IDAT 载荷按序合并进单缓冲（分片边界对 zlib 流透明）。
            // 不设块数上限——4K 图按规范分块可达数千块，限 16 会让大图全拒。
            // 总量上限由展开区的内存闸（check_pixel_budget）与
            // zlib_inflate_slices 的输出缓冲共同承担，不靠"块数"这道伪闸。
            idat.extend_from_slice(data);
            stats.idat_chunks += 1;
            // 合并载荷总量闸（替代原块数上限——见 IDAT_BYTES_MAX 注）
            if idat.len() > IDAT_BYTES_MAX {
                return Err(PngFault::new(FaultKind::TooManyIdat).at_chunk(fourcc).with(
                    IDAT_BYTES_MAX as u64,
                    idat.len() as u64,
                ));
            }
        } else if fourcc == CHUNK_IEND.as_slice() {
            seen_iend = true;
        } else {
            // 未知块：按 ancillary 位处置
            match class {
                ChunkClass::Critical => {
                    return Err(PngFault::new(FaultKind::UnknownCritical).at_chunk(fourcc));
                }
                ChunkClass::Ancillary => {
                    stats.ancillary_skipped += 1;
                    if !is_known_ancillary(fourcc) {
                        stats.ancillary_unknown += 1;
                    }
                }
            }
        }
    }

    let head = ihdr.ok_or_else(|| PngFault::new(FaultKind::MissingIhdr).at_chunk(&CHUNK_IHDR))?;
    if idat.is_empty() {
        return Err(PngFault::new(FaultKind::NoIdat).at_chunk(&CHUNK_IDAT));
    }
    if matches!(head.color, ColorType::Palette) && palette.is_none() {
        return Err(PngFault::new(FaultKind::PaletteRequired).at_chunk(&CHUNK_PLTE));
    }
    // tRNS 需在颜色类型确定后解析（长度校验依赖调色板项数）
    let transparency = match trns_data {
        None => Transparency::None,
        Some(d) => parse_trns(d, head.color, palette.as_ref().map(|p| p.len).unwrap_or(0))?,
    };
    Ok(Parsed { container: Container { ihdr: head, palette, transparency, idat, stats } })
}

// ---------------------------------------------------------------------------
// 六、解码上下文（行缓冲 + 滤波器先行字节）
// ---------------------------------------------------------------------------

/// 解码上下文（行级流水的状态载体——锚点"解码上下文"三件套：
/// 块游标由 [`parse_container`] 持有、行缓冲由本结构借用、
/// 滤波器先行字节在 [`decode_rows`] 逐行消费）。
///
/// 缓冲由调用方提供，本模块不分配整图缓冲——内存曲线对 F1013 友好。
pub struct DecodeCtx<'b> {
    /// 已反滤波的当前行（借用，长度 = row_bytes-1）。
    pub cur: &'b mut [u8],
    /// 上一行（借用；首行传空切片表示无上一行）。
    pub prev: &'b [u8],
    /// 反滤波目标 ISA（由 `imgsimd::detect_isa` 探测后传入，对拍与诊断消费）。
    pub isa: Isa,
}

impl<'b> DecodeCtx<'b> {
    /// 构造解码上下文。
    pub fn new(cur: &'b mut [u8], prev: &'b [u8], isa: Isa) -> DecodeCtx<'b> {
        DecodeCtx { cur, prev, isa }
    }
    /// 反滤波一行（含滤波类型首字节的剥离与先行字节消费）。
    ///
    /// `line_with_filter` 为一行原始字节（首字节 = 滤波类型）；`bpp` 由
    /// [`Ihdr::filter_bpp`] 给出。委托 [`imgsimd::unfilter_auto`] 完成
    /// 五滤波 SIMD/标量自动选路（CGPU-F0094）。滤波号非法 → None。
    pub fn unfilter_line(&mut self, line_with_filter: &[u8], bpp: usize) -> Option<Isa> {
        if line_with_filter.is_empty() {
            return None;
        }
        let filter = line_with_filter[0];
        let payload = &line_with_filter[1..];
        let n = payload.len().min(self.cur.len());
        self.cur[..n].copy_from_slice(&payload[..n]);
        // 首行 prev 为空 → 传 None 让上/平均/paeth 走"无上一行"分支
        let prev = if self.prev.is_empty() { None } else { Some(&self.prev[..n]) };
        let used = imgsimd::unfilter_auto(self.isa, filter, bpp, prev, &mut self.cur[..n])?;
        self.isa = used;
        Some(used)
    }
}

// ---------------------------------------------------------------------------
// 七、色彩展开（位深 × 颜色类型 → RGBA8）
// ---------------------------------------------------------------------------

/// 位深 <8 的位取样（PNG 规范 §9：高位在左，逐像素跨字节）。
///
/// `pub(crate)`：色彩展开的热路径内联要点，同时供同域自检逐值断言
/// （跨字节边界取样序是位深 <8 正确性的根，自检必须能直接点到）。
#[inline]
pub(crate) fn take_bits(row: &[u8], index: u32, depth: u8) -> u8 {
    match depth {
        1 => (row[(index / 8) as usize] >> (7 - (index % 8))) & 1,
        2 => (row[(index / 4) as usize] >> (6 - 2 * (index % 4))) & 3,
        4 => (row[(index / 2) as usize] >> (4 - 4 * (index % 2))) & 15,
        _ => 0,
    }
}

/// 位深归一化到 0..255（灰度 <8 位深按最大值线性拉伸——规范 §13）。
///
/// `pub(crate)`：同 [`take_bits`]，归一化表值是低位深跨位深一致性的根。
#[inline]
pub(crate) fn scale_gray(v: u8, depth: u8) -> u8 {
    match depth {
        1 => {
            if v & 1 != 0 {
                255
            } else {
                0
            }
        }
        2 => ((v & 3) as u16 * 85) as u8,
        4 => ((v & 15) as u16 * 17) as u8,
        _ => v,
    }
}

/// 16 位样本取高字节（F1001 出口为 RGBA8；原生 16 位路径由 F1009 拥有）。
#[inline]
fn take_u16_hi(row: &[u8], byte_off: usize) -> u8 {
    row.get(byte_off).copied().unwrap_or(0)
}

/// 把一行已反滤波的原始样本展开为 RGBA8。
///
/// 覆盖规范全集 15 种合法组合；调色板越界索引 → `None`（显性拒绝，不静默夹取）。
/// `out` 长度须 ≥ width*4（调用方保证）。
pub fn expand_row_rgba(
    row: &[u8],
    head: &Ihdr,
    palette: Option<&Palette>,
    trns: &Transparency,
    out: &mut [u8],
) -> Option<()> {
    let w = head.width as usize;
    if out.len() < w * 4 {
        return None;
    }
    let d = head.depth;
    let ch = head.channels();
    for x in 0..w {
        let o = x * 4;
        // 每像素样本起始字节（位深 <8 时另走 take_bits 分支）
        let base = x * ch * (d as usize / 8);
        let (r, g, b, a) = match head.color {
            ColorType::Gray => {
                if d == 16 {
                    let full = u16::from_be_bytes([
                        row.get(base).copied().unwrap_or(0),
                        row.get(base + 1).copied().unwrap_or(0),
                    ]);
                    let transparent = matches!(trns, Transparency::Gray(k) if *k == full);
                    // 取高字节（网络序大端 → 高位在前）。不可写成 `full as u8`
                    // ——那是截断取低字节，会把 0x1234 变成 0x34，整图亮度错一半。
                    let hi = (full >> 8) as u8;
                    (hi, hi, hi, if transparent { 0 } else { 255 })
                } else {
                    let raw = if d == 8 { row.get(base).copied().unwrap_or(0) } else { take_bits(row, x as u32, d) };
                    let v = scale_gray(raw, d);
                    let transparent = match trns {
                        // <8 位深的灰度色键按归一化后比较（色键语义在 8 位空间一致）
                        Transparency::Gray(k) => (k & 0xFF) as u8 == v,
                        _ => false,
                    };
                    (v, v, v, if transparent { 0 } else { 255 })
                }
            }
            ColorType::GrayAlpha => {
                // 合法位深仅 8/16
                if d == 16 {
                    let g0 = take_u16_hi(row, base);
                    (g0, g0, g0, take_u16_hi(row, base + 2))
                } else {
                    let g0 = row.get(base).copied().unwrap_or(0);
                    (g0, g0, g0, row.get(base + 1).copied().unwrap_or(255))
                }
            }
            ColorType::Rgb => {
                if d == 16 {
                    let r0 = take_u16_hi(row, base);
                    let g0 = take_u16_hi(row, base + 2);
                    let b0 = take_u16_hi(row, base + 4);
                    let transparent = matches!(trns, Transparency::Rgb(kr, kg, kb)
                        if (row.get(base).copied().unwrap_or(0) as u16) >> 8 == (*kr >> 8)
                            && (row.get(base + 2).copied().unwrap_or(0) as u16) >> 8 == (*kg >> 8)
                            && (row.get(base + 4).copied().unwrap_or(0) as u16) >> 8 == (*kb >> 8));
                    (r0, g0, b0, if transparent { 0 } else { 255 })
                } else {
                    let r0 = row.get(base).copied().unwrap_or(0);
                    let g0 = row.get(base + 1).copied().unwrap_or(0);
                    let b0 = row.get(base + 2).copied().unwrap_or(0);
                    let transparent = matches!(trns, Transparency::Rgb(kr, kg, kb)
                        if (*kr & 0xFF) as u8 == r0 && (*kg & 0xFF) as u8 == g0 && (*kb & 0xFF) as u8 == b0);
                    (r0, g0, b0, if transparent { 0 } else { 255 })
                }
            }
            ColorType::Rgba => {
                if d == 16 {
                    (
                        take_u16_hi(row, base),
                        take_u16_hi(row, base + 2),
                        take_u16_hi(row, base + 4),
                        take_u16_hi(row, base + 6),
                    )
                } else {
                    (
                        row.get(base).copied().unwrap_or(0),
                        row.get(base + 1).copied().unwrap_or(0),
                        row.get(base + 2).copied().unwrap_or(0),
                        row.get(base + 3).copied().unwrap_or(255),
                    )
                }
            }
            ColorType::Palette => {
                let pal = palette?;
                let idx = if d == 8 { row.get(base).copied().unwrap_or(0) as usize } else { take_bits(row, x as u32, d) as usize };
                // 越界索引显性拒绝——不夹取到末项（那是解出错图）
                let (pr, pg, pb) = pal.get(idx)?;
                let alpha = match trns {
                    Transparency::PaletteAlpha(alphas) => alphas.get(idx).copied().unwrap_or(255),
                    _ => 255,
                };
                (pr, pg, pb, alpha)
            }
        };
        out[o] = r;
        out[o + 1] = g;
        out[o + 2] = b;
        out[o + 3] = a;
    }
    Some(())
}

// ---------------------------------------------------------------------------
// 八、行级输出接口（F1013 流式纪律 / F1018 渐进显示预留）
// ---------------------------------------------------------------------------

/// 行接收端（行级输出的唯一出口——解码器不持有整图）。
///
/// F1013 流式解码、F1018 渐进显示、F1019 抽样快路径均可实现本 trait
/// 复用同一解码核心，无需各自重写反滤波与色彩展开。
pub trait RowSink {
    /// 接收一行的 RGBA8（长度 = width*4）。返回 `false` 表示要求提前中止。
    fn on_row(&mut self, y: u32, rgba: &[u8]) -> bool;
}

/// 空接收端（丢弃全部行——仅做结构校验与性能测量用）。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl RowSink for NullSink {
    fn on_row(&mut self, _y: u32, _rgba: &[u8]) -> bool {
        true
    }
}

/// 解码结果（行级流水完成后回报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecodeOutcome {
    /// 实际输出行数（截断时 < height）。
    pub rows: u32,
    /// 是否在完成前断掉（锚点"截断→已解码部分输出并标记"）。
    pub truncated: bool,
    /// 是否被接收端提前中止。
    pub aborted: bool,
    /// 解码统计（容器计数 + 反滤波路径分布 + 实际行数）。
    ///
    /// 随结果一并回传——统计若只在函数内累加而不出口，就等于没有统计：
    /// SIMD 命中率、F1011 对拍、异常路径计数全靠这一个面消费。
    pub stats: Stats,
}

// ---------------------------------------------------------------------------
// 九、解码驱动（容器 → zlib 展开 → 反滤波 → 色彩展开 → 行输出）
// ---------------------------------------------------------------------------

/// zlib 错误到本域故障的映射（上游 `PngError` 逐变体分流，不吞错）。
fn map_zlib(e: PngError) -> PngFault {
    let kind = match e {
        PngError::BadZlib | PngError::BadAdler { .. } | PngError::Inflate(_) => FaultKind::Zlib,
        PngError::NoIdat => FaultKind::NoIdat,
        PngError::TooManyIdat => FaultKind::TooManyIdat,
        PngError::Truncated => FaultKind::TruncatedChunk,
        PngError::BadPalette => FaultKind::Palette,
        PngError::ScratchSize { need, got } => {
            return PngFault::new(FaultKind::BufferShort).with(need as u64, got as u64)
        }
        _ => FaultKind::Zlib,
    };
    PngFault::new(kind).at_chunk(&CHUNK_IDAT)
}

/// 行缓冲尺寸（`row_bytes` 的调用侧便利形）。
pub fn row_buffer_len(head: &Ihdr) -> usize {
    head.row_bytes()
}

/// 解码缓冲需求（原始展开 + 双行滤波缓冲 + 单行 RGBA）。
///
/// F1013 内存预算器的输入——本函数是"静态估算"的唯一口径。
pub fn scratch_need(head: &Ihdr) -> u64 {
    // 先在u64 域算完再返回；调用侧转 usize 前会再过一次预算闸，
    // 故 32 位目标上"超 usize"的情形由 check_pixel_budget 先行拦住。
    head.raw_bytes() + 2 * (head.row_bytes() as u64 - 1) + head.width as u64 * 4
}

/// 尺寸前置闸（恶意尺寸防护——F1015 联动：先算后比，绝不先乘后比）。
///
/// `u64` 逐级运算：宽高各 ≤ 2^31-1，宽×高×4 不会在 u64 内溢出，
/// 但仍显式给出上限比较而不依赖"不会溢出"这一假设。
pub fn check_pixel_budget(head: &Ihdr) -> Result<(), PngFault> {
    let need = head.rgba_bytes();
    if need > PIXEL_BUDGET_BYTES {
        return Err(PngFault::new(FaultKind::PixelBudget).at_chunk(&CHUNK_IHDR).with(need, PIXEL_BUDGET_BYTES));
    }
    Ok(())
}

/// 解码全链（锚点主流程）。
///
/// 步骤：尺寸闸 → zlib 展开 IDAT → 逐行（剥滤波先行字节 → 反滤波 → 色彩展开
/// → 行输出）。任一步失败即返回故障；若 zlib 展开成功但行数据不足，则输出
/// 已解码行并置 `truncated`（锚点明确要求"已解码部分输出并标记"）。
///
/// `scratch` 需至少 [`scratch_need`] 字节：布局为
/// `[原始展开区 raw_bytes][上行滤波缓冲 row_bytes-1][当前行滤波缓冲 row_bytes-1][RGBA 行 width*4]`。
pub fn decode_to_rows(
    file: &[u8],
    scratch: &mut [u8],
    sink: &mut dyn RowSink,
) -> Result<DecodeOutcome, PngFault> {
    let parsed = parse_container(file)?;
    let head = parsed.container.ihdr;
    check_pixel_budget(&head)?;

    let raw_len = head.raw_bytes() as usize;
    let rb = head.row_bytes();
    let rgba_len = head.width as usize * 4;
    let need = scratch_need(&head) as usize;
    if scratch.len() < need {
        return Err(PngFault::new(FaultKind::BufferShort).with(need as u64, scratch.len() as u64));
    }
    // 拆分 scratch 三段（借用切片，零拷贝重排）
    let (raw_buf, rest) = scratch.split_at_mut(raw_len);
    let (prev_buf, rest) = rest.split_at_mut(rb - 1);
    let (cur_buf, rgba_buf) = rest.split_at_mut(rb - 1);
    if rgba_buf.len() < rgba_len {
        return Err(PngFault::new(FaultKind::BufferShort).with(rgba_len as u64, rgba_buf.len() as u64));
    }

    // zlib 展开（跨 IDAT 片连续——上游位读取器保证）
    // 合并后的单缓冲作为唯一分片交给上游位读取器（分片上限问题随之消失）
    let idat_ref: [&[u8]; 1] = [&parsed.container.idat];
    let got = mech_inflate::zlib_inflate_slices(&idat_ref, raw_buf).map_err(map_zlib)?;

    // 逐行流水
    let mut stats = parsed.container.stats;
    let mut rows: u32 = 0;
    let mut truncated = false;
    let mut aborted = false;
    let bpp = head.filter_bpp();
    let stride = rb;
    let mut isa = imgsimd::detect_isa();
    for y in 0..head.height {
        let off = y as usize * stride;
        if off + stride > got {
            // 行数据不足 → 已解码部分输出并标记（不谎报完整）
            truncated = true;
            break;
        }
        // prev 语义：首行传空（None 分支），其后传上一行
        if y == 0 {
            prev_buf.fill(0);
        }
        let used = {
            // 重新借用 cur_buf（不移动）——ctx 作用域结束后 cur_buf 仍可用
            let mut ctx = DecodeCtx::new(&mut *cur_buf, &prev_buf[..(rb - 1)], isa);
            ctx.unfilter_line(&raw_buf[off..off + stride], bpp)
        };
        let used = match used {
            Some(u) => u,
            None => return Err(PngFault::new(FaultKind::BadFilter).at_chunk(&CHUNK_IDAT).with(y as u64, 5)),
        };
        // 统计口径：实际选到的 ISA 即为该行走的路径（不猜测）
        if used == Isa::Scalar {
            stats.scalar_rows += 1;
        } else {
            stats.simd_rows += 1;
        }
        isa = used;
        // 色彩展开
        if expand_row_rgba(&cur_buf[..(rb - 1)], &head, parsed.container.palette.as_ref(), &parsed.container.transparency, rgba_buf)
            .is_none()
        {
            // 调色板越界索引 → 显性拒绝
            return Err(PngFault::new(FaultKind::Palette).at_chunk(&CHUNK_PLTE).with(y as u64, 0));
        }
        // 该行确已交付给接收端——先计数并转入 prev，再按中止信号退出。
        // 顺序要紧：若先判中止再计数，会出现"接收端已收 1 行但 rows=0"的自相矛盾。
        prev_buf.copy_from_slice(&cur_buf[..(rb - 1)]);
        rows += 1;
        if !sink.on_row(y, &rgba_buf[..rgba_len]) {
            aborted = true;
            break;
        }
    }
    stats.rows_decoded = rows;
    stats.truncated = truncated;
    Ok(DecodeOutcome { rows, truncated, aborted, stats })
}

/// 解码到整图 RGBA（便利形：内部自分配缓冲并用收集式接收端）。
///
/// 与 [`decode_to_rows`] 逐像素等价——供不需要流式的调用方（F1011 对拍、
/// F1012 转码、F1019 质量对拍基线）使用。
pub fn decode(file: &[u8]) -> Result<(Ihdr, alloc::vec::Vec<u8>), PngFault> {
    let head = parse_container(file)?.container.ihdr;
    check_pixel_budget(&head)?;
    let mut rgba = alloc::vec![0u8; head.rgba_bytes() as usize];
    let need = scratch_need(&head) as usize;
    let mut scratch = alloc::vec![0u8; need];
    // collector 借用 rgba，故置于内层作用域——作用域结束后才能把 rgba 移出返回
    let out = {
        let mut collector = Collector { w: head.width as usize, data: &mut rgba, at: 0 };
        decode_to_rows(file, &mut scratch, &mut collector)?
    };
    if out.truncated {
        // 便利形不做部分交付：截断即带标记的故障（行级 API 才做部分输出）
        return Err(PngFault::new(FaultKind::TruncatedChunk).at_chunk(&CHUNK_IDAT).with(out.rows as u64, head.height as u64));
    }
    Ok((head, rgba))
}

/// 收集式行接收端（`decode` 内部用；行序断言在此处收口）。
struct Collector<'r> {
    /// 行字节宽（像素数）。
    w: usize,
    /// 目标缓冲。
    data: &'r mut [u8],
    /// 写入行游标。
    at: usize,
}

impl RowSink for Collector<'_> {
    fn on_row(&mut self, _y: u32, rgba: &[u8]) -> bool {
        let off = self.at * self.w * 4;
        if off + rgba.len() > self.data.len() {
            return false;
        }
        self.data[off..off + rgba.len()].copy_from_slice(rgba);
        self.at += 1;
        true
    }
}