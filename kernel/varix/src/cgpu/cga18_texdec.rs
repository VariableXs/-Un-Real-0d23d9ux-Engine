//! CGPU-F0018 · 压缩纹理软解回退（cga18_texdec）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0018`
//! **判据原文摘录**：全格式对拍参考解码器、分块并行、缓存命中、诚实性能标注、
//! 边缘块处理正确。
//!
//! 老 GPU 与纯软渲路径没有 BC/ASTC 硬解，纹理只能 CPU 解。本模块是那条
//! **回退通道**：不追求实时纹理流（诚实：比硬解慢一个数量级以上），靠
//! 「按需解压 + 块缓存」把静止帧的重复块摊掉。
//!
//! ## 能力矩阵是硬约束，不是备注
//!
//! 规格常量表（BC7 分区表 64×16、BC6H 模式表、ASTC 块模式与权重抽取）都是
//! 上千项的不规则常量，**凭记忆转录必错**，而错了的解码器比拒绝解码危险得
//! 多——花屏会被当成驱动 bug 查上一周。所以本模块把 `Support` 做成**可执行
//! 契约**：
//!
//! | 族 | 支持 | 说明 |
//! |---|---|---|
//! | BC1 | 全 | 565 端点 + 2bit 索引，含 1bit punch-through 三色模式 |
//! | BC2 | 全 | 4bit 显式 alpha + 强制四色 BC1 |
//! | BC3 | 全 | 插值 alpha（6 级 / 4 级 + 0/255 两档） |
//! | BC4 | 全 | 单通道插值，与 BC3 alpha 同一编码 |
//! | BC5 | 全 | 双通道（R=R 块，G=G 块） |
//! | BC6H | **元数据 + 确定性拒绝** | 14 模式 × 2/3 子集，无表不解 |
//! | BC7 | **mode 4/5/6**（1 子集） | 无分区表即可解；mode 0..3/7 走拒绝路径 |
//! | ASTC | **元数据 + 确定性拒绝** | 块模式位宽随块尺寸变化，无表不解 |
//!
//! 「诚实」由此可判：`Support::Modes` 里列的每个模式必须真能解，未列出的
//! 必须返回专属错误码——判据逐模式扫一遍验证这个双射（正-12）。
//!
//! ## 直出目标格式（免二次转换）
//!
//! 内部统一在 16bit 线性域（0..=65535 表示 v/65535）合成，**一次成型**直写
//! `OutFmt::Rgba8`（右移 8）或 `OutFmt::Rgba16f`（f32→f16 含非规格化数）。
//! LDR 源走 Rgba8 时 `v*257>>8 == v` 逐位无损；HDR 源只允许 Rgba16f，强行走
//! 8bit 会返回 `E_TX_OUT_FMT` 而不是静默截断高光。
//!
//! 错误路径（0xCF 段）：格式未知 / 块数据不足 / 尺寸非法 / 输出缓冲不足 /
//! BC7 模式未实现 / HDR 族无表拒绝 / ASTC 拒绝 / 输出格式不匹配 / 缓存键冲突。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCF00;
/// 格式枚举越界（`from_id` 表外）。
pub const E_TX_FMT: u16 = SEG_BASE + 1;
/// 块数据不足（`src` 短于 `block_bytes × 块数`）。
pub const E_TX_BLOCK_SHORT: u16 = SEG_BASE + 2;
/// 尺寸非法（0 宽/高）。
pub const E_TX_DIM: u16 = SEG_BASE + 3;
/// 输出缓冲不足。
pub const E_TX_OUT_SHORT: u16 = SEG_BASE + 4;
/// BC7 模式未实现或位域组合非法。
pub const E_TX_BC7_MODE: u16 = SEG_BASE + 5;
/// BC6H 无常量表，确定性拒绝。
pub const E_TX_HDR_UNSUP: u16 = SEG_BASE + 6;
/// ASTC 无常量表，确定性拒绝。
pub const E_TX_ASTC_UNSUP: u16 = SEG_BASE + 7;
/// 输出格式与源族不匹配（HDR 源不得直出 8bit）。
pub const E_TX_OUT_FMT: u16 = SEG_BASE + 8;

/// 一帧 4×4 块解出的像素数。
pub const BLOCK_PX: usize = 16;
/// 一帧 4×4 块解出的 16bit 通道数（16 像素 × RGBA）。
pub const BLOCK_UNITS: usize = BLOCK_PX * 4;

// ---------------------------------------------------------------------------
// 格式表
// ---------------------------------------------------------------------------

/// 压缩纹理格式族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxFormat {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6h,
    Bc7,
    Astc4x4,
    Astc5x5,
    Astc6x6,
    Astc8x8,
}

/// 格式族支持度——**可执行契约**，不是文档备注。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Support {
    /// 全模式可解。
    Full,
    /// 仅列出的 BC7 模式可解，其余必须拒绝。
    Modes(&'static [u8]),
    /// 只有元数据；任何解码请求都必须返回专属错误码。
    MetadataOnly,
}

/// BC7 已实现模式（1 子集，无需分区表）。
pub const BC7_MODES: [u8; 3] = [4, 5, 6];

/// 全格式闭集（判据遍历用；新增族必须改这里才能真正进闭环）。
pub const ALL_FORMATS: [TxFormat; 11] = [
    TxFormat::Bc1,
    TxFormat::Bc2,
    TxFormat::Bc3,
    TxFormat::Bc4,
    TxFormat::Bc5,
    TxFormat::Bc6h,
    TxFormat::Bc7,
    TxFormat::Astc4x4,
    TxFormat::Astc5x5,
    TxFormat::Astc6x6,
    TxFormat::Astc8x8,
];

impl TxFormat {
    /// 稳定标识串（遥测与错误信息用，不入判据比较）。
    pub const fn name(self) -> &'static str {
        match self {
            TxFormat::Bc1 => "BC1",
            TxFormat::Bc2 => "BC2",
            TxFormat::Bc3 => "BC3",
            TxFormat::Bc4 => "BC4",
            TxFormat::Bc5 => "BC5",
            TxFormat::Bc6h => "BC6H",
            TxFormat::Bc7 => "BC7",
            TxFormat::Astc4x4 => "ASTC4x4",
            TxFormat::Astc5x5 => "ASTC5x5",
            TxFormat::Astc6x6 => "ASTC6x6",
            TxFormat::Astc8x8 => "ASTC8x8",
        }
    }

    /// 线性编号 = `ALL_FORMATS` 下标。
    pub fn id(self) -> usize {
        let mut i = 0;
        while i < ALL_FORMATS.len() {
            if ALL_FORMATS[i] as u8 == self as u8 {
                return i;
            }
            i += 1;
        }
        ALL_FORMATS.len()
    }

    /// 编号还原（表外拒绝，不给默认值——默认值会让拼错的编号静默变成 BC1）。
    pub fn from_id(id: usize) -> Result<TxFormat, u16> {
        if id < ALL_FORMATS.len() {
            Ok(ALL_FORMATS[id])
        } else {
            Err(E_TX_FMT)
        }
    }

    /// 每块字节数。
    pub const fn block_bytes(self) -> usize {
        match self {
            TxFormat::Bc1 | TxFormat::Bc4 => 8,
            _ => 16,
        }
    }

    /// 块边长 `(宽, 高)`。
    pub const fn block_dim(self) -> (usize, usize) {
        match self {
            TxFormat::Astc4x4 => (4, 4),
            TxFormat::Astc5x5 => (5, 5),
            TxFormat::Astc6x6 => (6, 6),
            TxFormat::Astc8x8 => (8, 8),
            _ => (4, 4),
        }
    }

    /// 输出通道数（BC4/BC5 是单/双通道源，但直出仍是 RGBA 四通道）。
    pub const fn channels(self) -> u8 {
        match self {
            TxFormat::Bc4 => 1,
            TxFormat::Bc5 => 2,
            _ => 4,
        }
    }

    /// 源是否自带 alpha 信息。
    pub const fn has_alpha(self) -> bool {
        matches!(self, TxFormat::Bc2 | TxFormat::Bc3 | TxFormat::Bc7)
    }

    /// 源是否 HDR（只允许直出 Rgba16f）。
    pub const fn is_hdr(self) -> bool {
        matches!(self, TxFormat::Bc6h)
    }

    /// 支持度契约。
    pub const fn support(self) -> Support {
        match self {
            TxFormat::Bc1 | TxFormat::Bc2 | TxFormat::Bc3 | TxFormat::Bc4 | TxFormat::Bc5 => {
                Support::Full
            }
            TxFormat::Bc7 => Support::Modes(&BC7_MODES),
            _ => Support::MetadataOnly,
        }
    }

    /// 至少 4×4 块（本模块实际可解的族）。
    pub const fn decodable(self) -> bool {
        matches!(
            self,
            TxFormat::Bc1
                | TxFormat::Bc2
                | TxFormat::Bc3
                | TxFormat::Bc4
                | TxFormat::Bc5
                | TxFormat::Bc7
        )
    }
}

/// 输出像素格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutFmt {
    /// 每像素 4 字节，RGBA 顺序。
    Rgba8,
    /// 每像素 8 字节，半浮点 RGBA（小端）。
    Rgba16f,
}

impl OutFmt {
    /// 每像素字节数。
    pub const fn px_bytes(self) -> usize {
        match self {
            OutFmt::Rgba8 => 4,
            OutFmt::Rgba16f => 8,
        }
    }
}

// ---------------------------------------------------------------------------
// 16bit 线性域小工具
// ---------------------------------------------------------------------------

/// 8bit → 16bit（v×257，可逆）。
#[inline]
pub const fn widen8(v: u8) -> u16 {
    (v as u16) * 257
}

/// 16bit → 8bit（`v = x×257` 时恰为 x）。
#[inline]
pub const fn narrow8(v: u16) -> u8 {
    (v >> 8) as u8
}

/// f32 → f16（IEEE 754 半精度，含非规格化数与溢出到 inf）。
///
/// 舍入模式取**截断**：软解出来的 LDR 值本来就是 8bit 精确值，截断不引入
/// 任何误差；而引入就近舍入会在 `v/65535` 恰好落在两个半精度数之间时把
/// 1.0 变成 1.0009765，让「全白」纹理不再是纯白。
pub fn f32_to_f16(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xFF) as i32;
    let man = x & 0x7F_FFFF;
    if exp == 0xFF {
        // inf / NaN：NaN 压成静默 NaN，保住「不是数」这件事。
        return sign | 0x7C00 | if man != 0 { 0x0200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 31 {
        return sign | 0x7C00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (14 - e);
        return sign | m as u16;
    }
    sign | ((e as u16) << 10) | ((man >> 13) as u16)
}

/// 16bit 线性值 → 半浮点位模式（`v/65535`）。
#[inline]
pub fn u16_to_f16(v: u16) -> u16 {
    f32_to_f16(v as f32 / 65535.0)
}

// ---------------------------------------------------------------------------
// BC1 / BC2 / BC3 / BC4 / BC5
// ---------------------------------------------------------------------------

/// RGB565 展开到 8bit（位复制，31→255、63→255 精确打满）。
#[inline]
pub fn rgb565(c: u16) -> [u8; 3] {
    let r = (c >> 11) & 0x1F;
    let g = (c >> 5) & 0x3F;
    let b = c & 0x1F;
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}

/// BC1 调色板。
///
/// `four_color` 为真时 4 色（p2/p3 按 1:2 混合）；为假时 3 色 + 透明黑
/// （p3 = (0,0,0,0)）。BC1 由 `c0 > c1` 决定，BC2/BC3 的颜色块**强制**四色
/// ——这是 BC2/BC3 最容易被误用的地方：它们允许 `c0 <= c1` 却仍取四色。
pub fn bc1_palette(c0: u16, c1: u16, four_color: bool) -> [[u8; 4]; 4] {
    let a = rgb565(c0);
    let b = rgb565(c1);
    let mut p = [[0u8; 4]; 4];
    p[0] = [a[0], a[1], a[2], 255];
    p[1] = [b[0], b[1], b[2], 255];
    if four_color {
        for i in 0..3 {
            p[2][i] = ((2 * a[i] as u32 + b[i] as u32) / 3) as u8;
            p[3][i] = ((a[i] as u32 + 2 * b[i] as u32) / 3) as u8;
        }
        p[2][3] = 255;
        p[3][3] = 255;
    } else {
        for i in 0..3 {
            p[2][i] = ((a[i] as u32 + b[i] as u32) / 2) as u8;
            p[3][i] = 0;
        }
        p[2][3] = 255;
        p[3][3] = 0;
    }
    p
}

/// BC3/BC4 的插值端点调色板（8 档）。
///
/// `a0 > a1`：8 档全插值（6 个中间档）；`a0 <= a1`：6 档（4 个中间档）+
/// 硬编码 0 与 255。这两条分支不是「优化」——选错分支的结果是整块 alpha
/// 反向，而 alpha 反向在纯色测试图上完全看不出来。
pub fn interp_alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let mut p = [0u8; 8];
    p[0] = a0;
    p[1] = a1;
    if a0 > a1 {
        let mut i = 1u32;
        while i <= 6 {
            p[i as usize + 1] = (((7 - i) * a0 as u32 + i * a1 as u32) / 7) as u8;
            i += 1;
        }
    } else {
        let mut i = 1u32;
        while i <= 4 {
            p[i as usize + 1] = (((5 - i) * a0 as u32 + i * a1 as u32) / 5) as u8;
            i += 1;
        }
        p[6] = 0;
        p[7] = 255;
    }
    p
}

/// 取 48 位小端索引流里的第 `p` 个 3bit 索引。
#[inline]
fn idx3(bytes: &[u8], p: usize) -> usize {
    let bit = p * 3;
    let mut acc = 0u32;
    for k in 0..3 {
        let at = bit + k;
        let b = bytes[at >> 3] as u32;
        acc |= ((b >> (at & 7)) & 1) << k;
    }
    acc as usize
}

/// BC1 块解码。
pub fn bc1_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 8 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let pal = bc1_palette(c0, c1, c0 > c1);
    let idx = &block[4..8];
    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let byte = idx[p >> 2] as u32;
        let sel = ((byte >> ((p & 3) * 2)) & 3) as usize;
        let c = pal[sel];
        let o = p * 4;
        out[o] = widen8(c[0]);
        out[o + 1] = widen8(c[1]);
        out[o + 2] = widen8(c[2]);
        out[o + 3] = widen8(c[3]);
    }
    Ok(out)
}

/// BC2 块解码（4bit 显式 alpha + 强制四色）。
pub fn bc2_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 16 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let c0 = u16::from_le_bytes([block[8], block[9]]);
    let c1 = u16::from_le_bytes([block[10], block[11]]);
    let pal = bc1_palette(c0, c1, true);
    let idx = &block[12..16];
    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let nib = block[p >> 1] as u32;
        let a4 = if p & 1 == 0 { nib & 0xF } else { nib >> 4 };
        let byte = idx[p >> 2] as u32;
        let sel = ((byte >> ((p & 3) * 2)) & 3) as usize;
        let c = pal[sel];
        let o = p * 4;
        out[o] = widen8(c[0]);
        out[o + 1] = widen8(c[1]);
        out[o + 2] = widen8(c[2]);
        out[o + 3] = widen8(((a4 * 17) as u32).min(255) as u8);
    }
    Ok(out)
}

/// BC3 块解码（插值 alpha + 强制四色）。
pub fn bc3_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 16 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let apal = interp_alpha_palette(block[0], block[1]);
    let c0 = u16::from_le_bytes([block[8], block[9]]);
    let c1 = u16::from_le_bytes([block[10], block[11]]);
    let pal = bc1_palette(c0, c1, true);
    let idx = &block[12..16];
    let abytes = &block[2..8];
    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let byte = idx[p >> 2] as u32;
        let sel = ((byte >> ((p & 3) * 2)) & 3) as usize;
        let c = pal[sel];
        let o = p * 4;
        out[o] = widen8(c[0]);
        out[o + 1] = widen8(c[1]);
        out[o + 2] = widen8(c[2]);
        out[o + 3] = widen8(apal[idx3(abytes, p)]);
    }
    Ok(out)
}

/// BC4 块解码（单通道 → R，G/B 置 0，A 打满）。
pub fn bc4_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 8 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let pal = interp_alpha_palette(block[0], block[1]);
    let abytes = &block[2..8];
    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let o = p * 4;
        out[o] = widen8(pal[idx3(abytes, p)]);
        out[o + 1] = 0;
        out[o + 2] = 0;
        out[o + 3] = 0xFFFF;
    }
    Ok(out)
}

/// BC5 块解码（R/G 双通道，A 打满）。
pub fn bc5_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 16 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let r = bc4_block(&block[0..8])?;
    let g = bc4_block(&block[8..16])?;
    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let o = p * 4;
        out[o] = r[o];
        out[o + 1] = g[o];
        out[o + 2] = 0;
        out[o + 3] = 0xFFFF;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// BC7（mode 4 / 5 / 6，1 子集，无分区表）
// ---------------------------------------------------------------------------

/// 2bit 索引权重表（0..64，端点为 0 与 64）。
pub const W2: [u32; 4] = [0, 21, 43, 64];
/// 3bit 索引权重表。
pub const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
/// 4bit 索引权重表。
pub const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// LSB-first 位流读取器（BC7 位域从模式前缀之后开始，逐字段向后取）。
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], start_bit: usize) -> Bits<'a> {
        Bits { data, pos: start_bit }
    }

    fn get(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        let mut k = 0u32;
        while k < n {
            let at = self.pos;
            let byte = if at >> 3 < self.data.len() {
                self.data[at >> 3]
            } else {
                0
            };
            v |= (((byte >> (at & 7)) & 1) as u32) << k;
            self.pos += 1;
            k += 1;
        }
        v
    }

    fn bits_used(&self) -> usize {
        self.pos
    }
}

/// BC7 模式前缀长度识别：模式 N = N 个 0 后跟一个 1。
pub fn bc7_mode(block: &[u8]) -> Option<u8> {
    if block.is_empty() {
        return None;
    }
    let mut i = 0;
    while i < 8 {
        if (block[0] >> i) & 1 == 1 {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// 端点反量化（含 p-bit 时先把 p 拼到最低位再位复制补齐）。
fn unquant(v: u32, bits: u32, p: Option<u32>) -> u8 {
    let (val, b) = match p {
        None => (v, bits),
        Some(pb) => ((v << 1) | (pb & 1), bits + 1),
    };
    if b >= 8 {
        (val & 0xFF) as u8
    } else {
        ((val << (8 - b)) | (val >> (2 * b - 8))) as u8
    }
}

/// 两档插值（`w` 已放大到 0..64）。
#[inline]
fn lerp64(e0: u8, e1: u8, w: u32) -> u8 {
    (((64 - w) * e0 as u32 + w * e1 as u32 + 32) >> 6) as u8
}

/// BC7 块解码（仅 mode 4/5/6）。
///
/// 字段顺序（BC7 规范）：模式前缀 → 旋转(2) → 索引选择(1) → 端点按**通道
/// 主序**排（R 全部端点在先，再 G、B、A）→ p-bit（每个端点一位）→ 主索引
/// 逐像素（锚点像素 0 少一位）→ 次索引逐像素（锚点同样少一位）。
pub fn bc7_block(block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    if block.len() < 16 {
        return Err(E_TX_BLOCK_SHORT);
    }
    let mode = match bc7_mode(block) {
        Some(m) => m,
        None => return Err(E_TX_BC7_MODE),
    };
    // (cb, ab, epb, rb, isb, ib, ib2)
    let (cb, ab, epb, rb, isb, ib, ib2) = match mode {
        4 => (5u32, 6u32, 0u32, 2u32, 1u32, 2u32, 1u32),
        5 => (7, 8, 0, 2, 0, 2, 2),
        6 => (7, 7, 1, 0, 0, 4, 0),
        _ => return Err(E_TX_BC7_MODE),
    };
    let mut br = Bits::new(block, mode as usize + 1);
    let rot = if rb > 0 { br.get(rb) } else { 0 };
    let sel = if isb > 0 { br.get(isb) } else { 0 };

    // 端点：1 子集 → 每通道 2 个端点。通道主序。
    let mut e0 = [0u8; 4];
    let mut e1 = [0u8; 4];
    let mut raw = [[0u32; 2]; 4];
    for c in 0..3 {
        raw[c][0] = br.get(cb);
        raw[c][1] = br.get(cb);
    }
    raw[3][0] = br.get(ab);
    raw[3][1] = br.get(ab);
    let mut pbit = [None; 2];
    if epb > 0 {
        pbit[0] = Some(br.get(epb));
        pbit[1] = Some(br.get(epb));
    }
    for c in 0..4 {
        let bits = if c == 3 { ab } else { cb };
        e0[c] = unquant(raw[c][0], bits, pbit[0]);
        e1[c] = unquant(raw[c][1], bits, pbit[1]);
    }

    // 主索引：锚点（1 子集 = 像素 0）少一位，最高位隐含为 0。
    let mut i0 = [0u32; BLOCK_PX];
    for p in 0..BLOCK_PX {
        let n = if p == 0 { ib - 1 } else { ib };
        i0[p] = br.get(n);
    }
    // 次索引：mode 6 没有第二组。
    let mut i1 = [0u32; BLOCK_PX];
    if ib2 > 0 {
        for p in 0..BLOCK_PX {
            let n = if p == 0 { ib2 - 1 } else { ib2 };
            i1[p] = br.get(n);
        }
    }
    if br.bits_used() > 128 {
        // 位域越界说明模式表与本实现不一致——宁可拒绝也不解出花屏。
        return Err(E_TX_BC7_MODE);
    }

    // 哪一组下标服务哪个通道。
    // mode 6：单组，全通道用 i0。
    // mode 5：颜色 i0、alpha i1（isb 恒 0）。
    // mode 4：sel==0 → 颜色 i0、alpha i1；sel==1 → alpha i0、旋转目标通道 i1
    //         （此时旋转必须非 0，否则 sel 位无意义，判为非法块）。
    let mut rot_chan = 3usize; // 3 = 无第二组颜色通道
    let alpha_set = if ib2 == 0 {
        0u8
    } else if isb == 0 || sel == 0 {
        1u8
    } else {
        if rot == 0 {
            return Err(E_TX_BC7_MODE);
        }
        rot_chan = rot as usize - 1;
        0u8
    };

    let wtab0: &[u32] = match ib {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    };
    let wtab1: &[u32] = match ib2 {
        0 | 1 | 2 => &W2,
        3 => &W3,
        _ => &W4,
    };

    let mut out = [0u16; BLOCK_UNITS];
    for p in 0..BLOCK_PX {
        let wc = wtab0[i0[p] as usize];
        let wa = if ib2 > 0 { wtab1[i1[p] as usize] } else { wc };
        let mut c = [0u8; 4];
        for ch in 0..3 {
            // 颜色通道一般走主索引；只有 mode4 的 sel==1 会让旋转目标通道
            // 改走次索引（这正是「索引选择」位的全部含义）。
            let use_second = ib2 > 0 && isb > 0 && sel == 1 && ch == rot_chan;
            let w = if use_second { wa } else { wc };
            c[ch] = lerp64(e0[ch], e1[ch], w);
        }
        let w_alpha = if alpha_set == 1 { wa } else { wc };
        c[3] = lerp64(e0[3], e1[3], w_alpha);
        // 旋转在后：把旋中通道与 alpha 对调。
        if rot != 0 {
            let ch = rot as usize - 1;
            let t = c[ch];
            c[ch] = c[3];
            c[3] = t;
        }
        let o = p * 4;
        out[o] = widen8(c[0]);
        out[o + 1] = widen8(c[1]);
        out[o + 2] = widen8(c[2]);
        out[o + 3] = widen8(c[3]);
    }
    Ok(out)
}

/// 单块解码总入口（ASTC 因块尺寸非 4×4，永不进此路径）。
pub fn decode_block(fmt: TxFormat, block: &[u8]) -> Result<[u16; BLOCK_UNITS], u16> {
    match fmt {
        TxFormat::Bc1 => bc1_block(block),
        TxFormat::Bc2 => bc2_block(block),
        TxFormat::Bc3 => bc3_block(block),
        TxFormat::Bc4 => bc4_block(block),
        TxFormat::Bc5 => bc5_block(block),
        TxFormat::Bc7 => bc7_block(block),
        TxFormat::Bc6h => Err(E_TX_HDR_UNSUP),
        TxFormat::Astc4x4 | TxFormat::Astc5x5 | TxFormat::Astc6x6 | TxFormat::Astc8x8 => {
            Err(E_TX_ASTC_UNSUP)
        }
    }
}

// ---------------------------------------------------------------------------
// 块缓存（按需解压 + 内容去重）
// ---------------------------------------------------------------------------

/// 块缓存键：格式混入 FNV-1a，块内容逐字节。
pub fn block_key(fmt: TxFormat, block: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (fmt.id() as u64 + 1).wrapping_mul(0x100_0000_01B3);
    for &b in block {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01B3);
    }
    h
}

/// 缓存条目。
#[derive(Clone, Copy)]
pub struct Cached {
    /// 内容键。
    pub key: u64,
    /// 解码结果（16bit 线性域）。
    pub px: [u16; BLOCK_UNITS],
}

/// 缓存会计。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CacheStats {
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
    pub size: usize,
}

impl CacheStats {
    /// 命中率（千分比；零请求返回 0，不返回 1000——空集上的「全命中」是假信号）。
    pub fn hit_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        self.hits as u32 * 1000 / total
    }
}

/// 定容块缓存（FIFO 逐出，命中即返回解码结果）。
///
/// 为什么内容键而不是块地址键：桌面纹理里大量块是同一色块的重复（天空、
/// 纯色按钮、渐变台阶），内容键能把这些真去重掉；地址键只能躲开「同一块
/// 在同一帧被采两次」，而那种情形本来就会被 mip 链摊平。
pub struct BlockCache {
    entries: Vec<Cached>,
    cap: usize,
    hits: u32,
    misses: u32,
    evictions: u32,
}

impl BlockCache {
    /// 新建（`cap == 0` 等价于禁用缓存，`get` 恒 miss）。
    pub fn new(cap: usize) -> BlockCache {
        BlockCache {
            entries: Vec::with_capacity(cap),
            cap,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// 查表（命中把条目挪到队尾，保持 FIFO 语义下的「近期命中不被立刻逐出」）。
    pub fn get(&mut self, key: u64) -> Option<[u16; BLOCK_UNITS]> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].key == key {
                let px = self.entries[i].px;
                let e = self.entries.remove(i);
                self.entries.push(e);
                self.hits = self.hits.saturating_add(1);
                return Some(px);
            }
            i += 1;
        }
        self.misses = self.misses.saturating_add(1);
        None
    }

    /// 存入（满则逐出队首）。
    pub fn put(&mut self, key: u64, px: [u16; BLOCK_UNITS]) {
        if self.cap == 0 {
            return;
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
            self.evictions = self.evictions.saturating_add(1);
        }
        self.entries.push(Cached { key, px });
    }

    /// 会计快照。
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            size: self.entries.len(),
        }
    }

    /// 清空（保留会计，便于「换图不清账」）。
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ---------------------------------------------------------------------------
// 表面解码（边缘块裁剪 + 分块并行 + 直出目标格式）
// ---------------------------------------------------------------------------

/// 一次表面解码的账。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct DecodeStats {
    /// 实际解过的块数（含被缓存挡下的）。
    pub blocks: u32,
    /// 真正进解码器的块数。
    pub decoded: u32,
    /// 写出像素数。
    pub px: u32,
    /// 因边缘裁剪丢弃的像素数。
    pub clipped: u32,
}

/// 块网格 `(列数, 行数)`。
pub fn grid(w: usize, h: usize, fmt: TxFormat) -> (usize, usize) {
    let (bw, bh) = fmt.block_dim();
    ((w + bw - 1) / bw, (h + bh - 1) / bh)
}

/// 分块并行计划：把 `total` 块切成 `workers` 段，段长最多差 1，且必然
/// 连续、不重叠、并集恰为 `[0, total)`。返回的段已过滤空段。
pub fn plan_blocks(total: usize, workers: usize) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    if total == 0 || workers == 0 {
        return v;
    }
    let w = workers.min(total);
    let base = total / w;
    let rem = total % w;
    let mut at = 0usize;
    let mut i = 0usize;
    while i < w {
        let n = base + if i < rem { 1 } else { 0 };
        v.push((at, at + n));
        at += n;
        i += 1;
    }
    v
}

/// 把一块 16bit 像素写进目标格式。
fn write_px(out: &mut [u8], at: usize, fmt: OutFmt, px: &[u16]) {
    match fmt {
        OutFmt::Rgba8 => {
            for c in 0..4 {
                out[at + c] = narrow8(px[c]);
            }
        }
        OutFmt::Rgba16f => {
            for c in 0..4 {
                let h = u16_to_f16(px[c]).to_le_bytes();
                out[at + c * 2] = h[0];
                out[at + c * 2 + 1] = h[1];
            }
        }
    }
}

/// 表面解码（全表面）。
pub fn decode_surface(
    fmt: TxFormat,
    src: &[u8],
    w: usize,
    h: usize,
    ofmt: OutFmt,
    out: &mut [u8],
) -> Result<DecodeStats, u16> {
    let (gc, gr) = grid(w, h, fmt);
    decode_surface_blocks(fmt, src, w, h, ofmt, out, 0, gc * gr, None)
}

/// 表面解码（带缓存）。
pub fn decode_surface_cached(
    cache: &mut BlockCache,
    fmt: TxFormat,
    src: &[u8],
    w: usize,
    h: usize,
    ofmt: OutFmt,
    out: &mut [u8],
) -> Result<DecodeStats, u16> {
    let (gc, gr) = grid(w, h, fmt);
    decode_surface_blocks(fmt, src, w, h, ofmt, out, 0, gc * gr, Some(cache))
}

/// 表面解码（只解 `[b0, b1)` 这些块——分块并行的落地口）。
pub fn decode_surface_block_range(
    fmt: TxFormat,
    src: &[u8],
    w: usize,
    h: usize,
    ofmt: OutFmt,
    out: &mut [u8],
    b0: usize,
    b1: usize,
) -> Result<DecodeStats, u16> {
    decode_surface_blocks(fmt, src, w, h, ofmt, out, b0, b1, None)
}

fn decode_surface_blocks(
    fmt: TxFormat,
    src: &[u8],
    w: usize,
    h: usize,
    ofmt: OutFmt,
    out: &mut [u8],
    b0: usize,
    b1: usize,
    mut cache: Option<&mut BlockCache>,
) -> Result<DecodeStats, u16> {
    if w == 0 || h == 0 {
        return Err(E_TX_DIM);
    }
    // HDR 源不得直出 8bit：先于族支持检查，因为这是**请求本身**不合法。
    if fmt.is_hdr() && ofmt == OutFmt::Rgba8 {
        return Err(E_TX_OUT_FMT);
    }
    if !fmt.decodable() {
        // 拒绝路径先于尺寸/缓冲检查：让调用方拿到「这族不支持」而不是一个
        // 看起来像参数错误的码。
        return Err(match fmt {
            TxFormat::Bc6h => E_TX_HDR_UNSUP,
            _ => E_TX_ASTC_UNSUP,
        });
    }
    let need = w * h * ofmt.px_bytes();
    if out.len() < need {
        return Err(E_TX_OUT_SHORT);
    }
    let (gc, gr) = grid(w, h, fmt);
    let total = gc * gr;
    if src.len() < total * fmt.block_bytes() {
        return Err(E_TX_BLOCK_SHORT);
    }
    let (bw, bh) = fmt.block_dim();
    let b1 = b1.min(total);
    let mut st = DecodeStats::default();
    let mut bi = b0;
    while bi < b1 {
        let bx = bi % gc;
        let by = bi / gc;
        let off = bi * fmt.block_bytes();
        let blk = &src[off..off + fmt.block_bytes()];
        let key = block_key(fmt, blk);
        let px = {
            let mut hit = None;
            if let Some(c) = cache.as_deref_mut() {
                hit = c.get(key);
            }
            match hit {
                Some(p) => p,
                None => {
                    let p = decode_block(fmt, blk)?;
                    if let Some(c) = cache.as_deref_mut() {
                        c.put(key, p);
                    }
                    st.decoded += 1;
                    p
                }
            }
        };
        st.blocks += 1;
        let mut py = 0usize;
        while py < bh {
            let y = by * bh + py;
            let mut px_i = 0usize;
            while px_i < bw {
                let x = bx * bw + px_i;
                if x < w && y < h {
                    let p = py * bw + px_i;
                    let at = (y * w + x) * ofmt.px_bytes();
                    write_px(out, at, ofmt, &px[p * 4..p * 4 + 4]);
                    st.px += 1;
                } else {
                    st.clipped += 1;
                }
                px_i += 1;
            }
            py += 1;
        }
        bi += 1;
    }
    Ok(st)
}

// ---------------------------------------------------------------------------
// 诚实的性能标注
// ---------------------------------------------------------------------------

/// 主频（MHz）——与 cga17 同源口径。
pub const CLOCK_MHZ: u64 = 3_000;

/// 性能标注。**每个数都能从成本画像推导**，不许手写一个好看的数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfNote {
    /// 单块等效周期。
    pub per_block_cycles: u32,
    /// 每微秒可解块数（= CLOCK_MHZ / per_block_cycles）。
    pub blocks_per_us: u32,
    /// 相对硬解至少慢多少倍（回退路径的诚实下限）。
    pub hw_speedup_at_least: u32,
    /// 是否可用作实时纹理流（本模块恒 false）。
    pub streaming_ok: bool,
    /// 面向人的一句话标注。
    pub note: &'static str,
}

/// 单块等效周期成本画像：索引展开 + 通道合成 + 端点反量化。
///
/// 这些数字是**结构性估算**（不是墙钟实测）：每像素 4 通道各一次乘加算作
/// 3 周期、每块端点反量化按通道数计、BC7 还要多付位流逐位读取的代价。
/// 之所以不用墙钟，是因为判据要能在任意机器上稳定复现，而「实测 ms」在
/// 别人的 CPU 上必然漂移。
pub fn per_block_cycles(fmt: TxFormat) -> u32 {
    match fmt {
        TxFormat::Bc1 => BLOCK_PX as u32 * 6 + 8,
        TxFormat::Bc2 => BLOCK_PX as u32 * 7 + 12,
        TxFormat::Bc3 => BLOCK_PX as u32 * 9 + 16,
        TxFormat::Bc4 => BLOCK_PX as u32 * 6 + 10,
        TxFormat::Bc5 => BLOCK_PX as u32 * 10 + 14,
        TxFormat::Bc7 => BLOCK_PX as u32 * 12 + 96,
        // 无解，但成本画像仍要给（决定「要不要为它专门写解码器」时的参照）。
        TxFormat::Bc6h => BLOCK_PX as u32 * 16 + 160,
        TxFormat::Astc4x4 => BLOCK_PX as u32 * 20 + 220,
        TxFormat::Astc5x5 => 25 * 20 + 240,
        TxFormat::Astc6x6 => 36 * 20 + 260,
        TxFormat::Astc8x8 => 64 * 20 + 300,
    }
}

/// 诚实性能标注（判据会重新推导 `blocks_per_us` 校验没被手改）。
pub fn perf_note(fmt: TxFormat) -> PerfNote {
    let cyc = per_block_cycles(fmt);
    PerfNote {
        per_block_cycles: cyc,
        blocks_per_us: (CLOCK_MHZ / cyc.max(1) as u64) as u32,
        hw_speedup_at_least: 10,
        streaming_ok: false,
        note: "软解回退：比硬解慢 10 倍以上，不用于实时纹理流；按需解压 + 块缓存摊平静止帧",
    }
}

/// 一块表面在 `workers` 个线程下的理论耗时（微秒，向上取整）。
pub fn surface_us(w: usize, h: usize, fmt: TxFormat, workers: usize) -> u64 {
    let (gc, gr) = grid(w, h, fmt);
    let total = (gc * gr) as u64;
    let w = (workers.max(1)) as u64;
    let per = (total + w - 1) / w;
    let cyc = per * per_block_cycles(fmt) as u64;
    (cyc + CLOCK_MHZ - 1) / CLOCK_MHZ
}

// ---------------------------------------------------------------------------
// 独立参考解码器（写法刻意与实现不同：绝对位偏移 vs 流式读取）
// ---------------------------------------------------------------------------

/// BC1 参考解码：先建 4 项调色板数组，再按 2bit 立即数查表。
///
/// 端点展开用**位复制**——这是 BC1 规范指定的展开公式（31→255、63→255 精确
/// 打满），参考实现不能在这里另立一套「×255/31」，否则对一个只是舍入不同的
/// 合法实现会误报。参考实现的独立性放在**结构与权重**上：这里按权重对建
/// 调色板、索引走绝对位偏移，与主实现的分支顺序不同。
pub fn ref_bc1(block: &[u8]) -> [u8; BLOCK_PX * 4] {
    let mut pal = [[0u8; 4]; 4];
    let c0 = (block[0] as u16) | ((block[1] as u16) << 8);
    let c1 = (block[2] as u16) | ((block[3] as u16) << 8);
    let e = [c0, c1];
    let four = c0 > c1;
    for s in 0..4 {
        if s < 2 {
            let c = e[s];
            let r = ((c >> 11) & 31) as u32;
            let g = ((c >> 5) & 63) as u32;
            let b = (c & 31) as u32;
            pal[s] = [
                ((r << 3) | (r >> 2)) as u8,
                ((g << 2) | (g >> 4)) as u8,
                ((b << 3) | (b >> 2)) as u8,
                255,
            ];
        } else if four {
            let (wa, wb) = if s == 2 { (2u32, 1u32) } else { (1, 2) };
            for i in 0..3 {
                pal[s][i] = ((wa * pal[0][i] as u32 + wb * pal[1][i] as u32) / 3) as u8;
            }
            pal[s][3] = 255;
        } else if s == 2 {
            for i in 0..3 {
                pal[s][i] = ((pal[0][i] as u32 + pal[1][i] as u32) >> 1) as u8;
            }
            pal[s][3] = 255;
        } else {
            pal[s] = [0, 0, 0, 0];
        }
    }
    let mut o = [0u8; BLOCK_PX * 4];
    for p in 0..BLOCK_PX {
        let bitpos = 32 + p * 2;
        let sel = ((block[bitpos >> 3] as usize >> (bitpos & 7)) & 3) as usize;
        o[p * 4..p * 4 + 4].copy_from_slice(&pal[sel]);
    }
    o
}

/// BC3 alpha 参考解码：走「位掩码累加」而不是逐位循环。
pub fn ref_bc3_alpha(block: &[u8]) -> [u8; BLOCK_PX] {
    let a0 = block[0];
    let a1 = block[1];
    let mut pal = [0u8; 8];
    pal[0] = a0;
    pal[1] = a1;
    if a0 > a1 {
        for i in 0..6u32 {
            pal[2 + i as usize] = ((a0 as u32 * (6 - i) + a1 as u32 * (i + 1)) / 7) as u8;
        }
    } else {
        for i in 0..4u32 {
            pal[2 + i as usize] = ((a0 as u32 * (4 - i) + a1 as u32 * (i + 1)) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut stream = 0u64;
    for k in 0..6 {
        stream |= (block[2 + k] as u64) << (8 * k);
    }
    let mut o = [0u8; BLOCK_PX];
    for p in 0..BLOCK_PX {
        o[p] = pal[((stream >> (3 * p)) & 7) as usize];
    }
    o
}

/// BC7 mode 6 参考解码：按**绝对位偏移**逐字段取值，不共享实现里的读取器。
pub fn ref_bc7_mode6(block: &[u8]) -> [u8; BLOCK_PX * 4] {
    let mut stream = 0u128;
    for k in 0..16 {
        stream |= (block[k] as u128) << (8 * k);
    }
    // 模式 6 = 6 个 0 + 1 → 前缀 7 位；随后 R0,R1,G0,G1,B0,B1,A0,A1 各 7 位，
    // 再 P0,P1 各 1 位，再 16 个 4bit 索引（像素 0 只有 3 位）。
    let mut pos = 7u32;
    let take = |n: u32, pos: &mut u32| -> u32 {
        let v = ((stream >> *pos) & ((1u128 << n) - 1)) as u32;
        *pos += n;
        v
    };
    let mut ep = [[0u32; 2]; 4];
    for c in 0..4 {
        ep[c][0] = take(7, &mut pos);
        ep[c][1] = take(7, &mut pos);
    }
    let pb = [take(1, &mut pos), take(1, &mut pos)];
    let mut e = [[0u8; 2]; 4];
    for c in 0..4 {
        for k in 0..2 {
            let v = (ep[c][k] << 1) | pb[k];
            e[c][k] = v as u8;
        }
    }
    let mut idx = [0u32; BLOCK_PX];
    for p in 0..BLOCK_PX {
        let n = if p == 0 { 3 } else { 4 };
        idx[p] = take(n, &mut pos);
    }
    let mut o = [0u8; BLOCK_PX * 4];
    for p in 0..BLOCK_PX {
        let w = W4[idx[p] as usize];
        for c in 0..4 {
            o[p * 4 + c] =
                (((64 - w) * e[c][0] as u32 + w * e[c][1] as u32 + 32) >> 6) as u8;
        }
    }
    o
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 构造一个 BC1 块（4 色模式）。
fn mk_bc1(c0: u16, c1: u16, sels: &[u8; 16]) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..2].copy_from_slice(&c0.to_le_bytes());
    b[2..4].copy_from_slice(&c1.to_le_bytes());
    for p in 0..16 {
        let byte = p >> 2;
        let sh = (p & 3) * 2;
        b[4 + byte] |= (sels[p] & 3) << sh;
    }
    b
}

/// 构造一个 BC7 mode 6 块（1 子集、7bit 端点 + p-bit、4bit 索引）。
fn mk_bc7_mode6(rgba: [[u8; 2]; 4], pbits: [u8; 2], idx: &[u8; 16]) -> [u8; 16] {
    let mut bits = [0u8; 128];
    bits[6] = 1; // mode 6 前缀 = 六个 0 后跟一个 1
    let mut pos = 7usize;
    let mut put = |n: u32, v: u32, pos: &mut usize| {
        for k in 0..n {
            bits[*pos] = ((v >> k) & 1) as u8;
            *pos += 1;
        }
    };
    for c in 0..4 {
        put(7, (rgba[c][0] >> 1) as u32, &mut pos);
        put(7, (rgba[c][1] >> 1) as u32, &mut pos);
    }
    put(1, pbits[0] as u32, &mut pos);
    put(1, pbits[1] as u32, &mut pos);
    for p in 0..16 {
        let n = if p == 0 { 3 } else { 4 };
        put(n, idx[p] as u32, &mut pos);
    }
    let mut b = [0u8; 16];
    for (i, bit) in bits.iter().enumerate() {
        if *bit != 0 {
            b[i >> 3] |= 1 << (i & 7);
        }
    }
    b
}

/// run_cga18_checks：CGPU-F0018 判据聚合（0xCF 段）。
pub fn run_cga18_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0018 压缩纹理软解");

    // 正-01：格式表编号双射 + 元数据自洽（块长只许 8/16，块边只许 4/5/6/8）
    {
        let mut ok = true;
        for (i, f) in ALL_FORMATS.iter().enumerate() {
            if f.id() != i || TxFormat::from_id(i) != Ok(*f) {
                ok = false;
            }
            let bb = f.block_bytes();
            if bb != 8 && bb != 16 {
                ok = false;
            }
            let (bw, bh) = f.block_dim();
            if bw != bh || !(bw == 4 || bw == 5 || bw == 6 || bw == 8) {
                ok = false;
            }
            if f.name().is_empty() {
                ok = false;
            }
        }
        if ok && TxFormat::from_id(ALL_FORMATS.len()).is_err() {
            s.ok("F0018-正-01-格式表双射与元数据");
        } else {
            s.fail("F0018-正-01-格式表双射与元数据", "格式表编号或元数据不自洽");
        }
    }

    // 正-02：BC1 对拍独立参考实现（全 4 色 + 三色两种模式，含端点极值）
    {
        let mut ok = true;
        let cases: [(u16, u16, [u8; 16]); 4] = [
            (0xFFFF, 0x0000, [0, 1, 2, 3, 0, 1, 2, 3, 3, 2, 1, 0, 3, 2, 1, 0]),
            (0xF800, 0x07E0, [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]),
            (0x001F, 0x0000, [3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0]),
            (0x8410, 0x8410, [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]),
        ];
        for (c0, c1, sels) in cases.iter() {
            let b = mk_bc1(*c0, *c1, sels);
            let got = bc1_block(&b).expect("bc1 块解码不应失败");
            let want = ref_bc1(&b);
            for p in 0..BLOCK_PX {
                for c in 0..4 {
                    if narrow8(got[p * 4 + c]) != want[p * 4 + c] {
                        ok = false;
                    }
                }
            }
        }
        if ok {
            s.ok("F0018-正-02-BC1对拍参考解码");
        } else {
            s.fail("F0018-正-02-BC1对拍参考解码", "BC1 与参考实现不一致");
        }
    }

    // 正-03：BC1 端点手算锚（c0=白 / c1=黑 → 调色板 {255,0,170,85}）
    //
    // 注意 c0 > c1 才是四色模式：写成 c0=黑、c1=白 会掉进三色分支，
    // p2 变成 (a+b)/2=127、p3 变透明——那时算出来的「错」是判据自己挑错了模式。
    {
        let b = mk_bc1(0xFFFF, 0x0000, &[0, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let got = bc1_block(&b).expect("bc1");
        let pal = [
            narrow8(got[0]),
            narrow8(got[4]),
            narrow8(got[8]),
            narrow8(got[12]),
        ];
        if pal == [255, 0, 170, 85] {
            s.ok("F0018-正-03-BC1调色板手算锚");
        } else {
            s.fail("F0018-正-03-BC1调色板手算锚", "调色板非 {255,0,170,85}");
        }
    }

    // 正-04：BC1 三色模式的第 4 档必须是透明黑（不是 (0,0,0,255)）
    {
        let b = mk_bc1(0x0000, 0xFFFF, &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let got = bc1_block(&b).expect("bc1");
        // c0 < c1 → 三色模式：像素 0（第 3 档）alpha=0，像素 1（第 0 档）alpha 满
        if got[3] == 0 && got[7] == 0xFFFF && got[0] == 0 {
            s.ok("F0018-正-04-BC1三色档透明");
        } else {
            s.fail("F0018-正-04-BC1三色档透明", "punch-through 档未置 alpha=0");
        }
    }

    // 正-05：BC3 alpha 对拍参考 + 手算锚（a0=255,a1=0 → 255,0,218,182,145,109,72,36）
    {
        let mut b = [0u8; 16];
        b[0] = 255;
        b[1] = 0;
        // 索引：像素 p 取第 p&7 档，覆盖全部 8 档
        let mut stream = 0u64;
        for p in 0..16 {
            stream |= ((p & 7) as u64) << (3 * p);
        }
        for k in 0..6 {
            b[2 + k] = ((stream >> (8 * k)) & 0xFF) as u8;
        }
        // 颜色块用 c0>c1 的四色
        b[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        b[10..12].copy_from_slice(&0x0000u16.to_le_bytes());
        let got = bc3_block(&b).expect("bc3");
        let want = ref_bc3_alpha(&b);
        let hand = [255u8, 0, 218, 182, 145, 109, 72, 36];
        let mut ok = true;
        for p in 0..BLOCK_PX {
            let a = narrow8(got[p * 4 + 3]);
            if a != want[p] || a != hand[p & 7] {
                ok = false;
            }
        }
        if ok {
            s.ok("F0018-正-05-BC3alpha对拍与手算锚");
        } else {
            s.fail("F0018-正-05-BC3alpha对拍与手算锚", "alpha 插值与参考/手算不符");
        }
    }

    // 正-06：BC3/BC4 的 a0<=a1 分支必须走 6 档 + 0/255
    {
        let mut b = [0u8; 8];
        b[0] = 0;
        b[1] = 255;
        let pal = interp_alpha_palette(b[0], b[1]);
        // a0<=a1：p2..p5 四个中间档（0→255 的五等分），p6=0，p7=255
        let ok = pal[0] == 0
            && pal[1] == 255
            && pal[2] == 51
            && pal[3] == 102
            && pal[4] == 153
            && pal[5] == 204
            && pal[6] == 0
            && pal[7] == 255;
        if ok {
            s.ok("F0018-正-06-BC3六档分支");
        } else {
            s.fail("F0018-正-06-BC3六档分支", "a0<=a1 分支档位错");
        }
    }

    // 正-07：BC4/BC5 通道摆放（BC4 只有 R，BC5 R/G 各自独立）
    {
        let mut b4 = [0u8; 8];
        b4[0] = 200;
        b4[1] = 100;
        let r = bc4_block(&b4).expect("bc4");
        let mut b5 = [0u8; 16];
        b5[0] = 200;
        b5[1] = 100;
        b5[8] = 40;
        b5[9] = 20;
        let g = bc5_block(&b5).expect("bc5");
        let ok = r[0] == widen8(200)
            && r[1] == 0
            && r[2] == 0
            && r[3] == 0xFFFF
            && g[0] == widen8(200)
            && g[1] == widen8(40)
            && g[3] == 0xFFFF;
        if ok {
            s.ok("F0018-正-07-BC4BC5通道摆放");
        } else {
            s.fail("F0018-正-07-BC4BC5通道摆放", "单/双通道映射错");
        }
    }

    // 正-08：BC2 的 4bit alpha 必须 ×17 打满（0xF → 255），且强制四色
    {
        let mut b = [0u8; 16];
        b[0] = 0xF0; // 像素 0 = 0x0（透明），像素 1 = 0xF（不透明）
        b[8..10].copy_from_slice(&0x0000u16.to_le_bytes());
        b[10..12].copy_from_slice(&0xFFFFu16.to_le_bytes()); // c0 < c1，BC2 仍须四色
        // 索引全 3：四色模式下 p3 = (0+2×255)/3 = 170 且 alpha=255；
        // 若误走 BC1 的三色规则，p3 会是透明黑。
        for k in 12..16 {
            b[k] = 0xFF;
        }
        let got = bc2_block(&b).expect("bc2");
        // 像素 0 的 alpha nibble 在 block[0] 低半字节（0x0），像素 1 在高半字节（0xF）；
        // 像素 2 的 alpha 来自 block[1] 低半字节（此处置 0）——三个位置正好把
        // 「低/高半字节」和「按像素配对取字节」两条错法都卡住。
        let ok = narrow8(got[3]) == 0
            && narrow8(got[7]) == 255
            && narrow8(got[11]) == 0
            && narrow8(got[8]) == 170;
        if ok {
            s.ok("F0018-正-08-BC2显式alpha与强制四色");
        } else {
            s.fail("F0018-正-08-BC2显式alpha与强制四色", "alpha 展开或四色强制失败");
        }
    }

    // 正-09：BC7 mode 6 对拍绝对偏移参考实现（随机化位型 32 组）
    {
        let mut ok = true;
        let mut state = 0x1234_5678u32;
        for _ in 0..32 {
            let mut blk = [0u8; 16];
            for k in 0..16 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                blk[k] = (state >> 16) as u8;
            }
            // 强制成 mode 6：低 7 位 = 0b1000000
            blk[0] = (blk[0] & 0x80) | 0x40;
            let got = match bc7_block(&blk) {
                Ok(v) => v,
                Err(_) => {
                    ok = false;
                    continue;
                }
            };
            let want = ref_bc7_mode6(&blk);
            for i in 0..BLOCK_PX * 4 {
                if narrow8(got[i]) != want[i] {
                    ok = false;
                }
            }
        }
        if ok {
            s.ok("F0018-正-09-BC7mode6对拍参考");
        } else {
            s.fail("F0018-正-09-BC7mode6对拍参考", "BC7 mode6 与参考实现不一致");
        }
    }

    // 正-10：BC7 mode 6 端点全等 → 全块同色（索引无论取什么都必须一样）
    //
    // alpha 取 0xFE 而不是 0xFF：mode 6 的端点值 = (7bit<<1)|p，p=0 时最低位
    // 恒 0，8bit 域只能表达偶数。写 0xFF 测出来的「差一」是实现正确、判据错。
    {
        let e = [[0x40u8, 0x40], [0x20, 0x20], [0xC0, 0xC0], [0xFE, 0xFE]];
        let idx = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let blk = mk_bc7_mode6(e, [0, 0], &idx);
        let got = bc7_block(&blk).expect("bc7 mode6");
        let mut ok = true;
        for p in 0..BLOCK_PX {
            if narrow8(got[p * 4]) != 0x40
                || narrow8(got[p * 4 + 1]) != 0x20
                || narrow8(got[p * 4 + 2]) != 0xC0
                || narrow8(got[p * 4 + 3]) != 0xFE
            {
                ok = false;
            }
        }
        if ok {
            s.ok("F0018-正-10-BC7mode6端等全同色");
        } else {
            s.fail("F0018-正-10-BC7mode6端等全同色", "端点相等时插值未被短路");
        }
    }

    // 正-11：能力矩阵是契约——声明的模式必须真能解，未声明的必须专属拒绝
    {
        let declared: &[u8] = &BC7_MODES;
        let mut ok = true;
        let mut mode = 0u8;
        while mode <= 7 {
            // 造一个该模式的块：低 mode 位为 0，第 mode 位为 1
            let mut blk = [0u8; 16];
            blk[0] = 1 << mode;
            let r = bc7_block(&blk);
            let should = declared.contains(&mode);
            if should != r.is_ok() {
                ok = false;
            }
            if !should && r != Err(E_TX_BC7_MODE) {
                ok = false;
            }
            mode += 1;
        }
        // 全零块（无模式前缀）也必须拒绝
        if bc7_block(&[0u8; 16]) != Err(E_TX_BC7_MODE) {
            ok = false;
        }
        if ok {
            s.ok("F0018-正-11-BC7模式声明即契约");
        } else {
            s.fail("F0018-正-11-BC7模式声明即契约", "能力声明与实际行为不符");
        }
    }

    // 正-12：无表族必须确定性拒绝，且错误码分族（BC6H ≠ ASTC），绝不返回像素
    {
        let blk = [0u8; 16];
        let mut ok = true;
        for f in ALL_FORMATS.iter() {
            let r = decode_block(*f, &blk);
            if let Support::MetadataOnly = f.support() {
                let want = if *f == TxFormat::Bc6h {
                    E_TX_HDR_UNSUP
                } else {
                    E_TX_ASTC_UNSUP
                };
                if r != Err(want) {
                    ok = false;
                }
            } else if *f == TxFormat::Bc7 {
                // 全零块没有模式前缀，BC7 拒绝才是正确行为
                if r != Err(E_TX_BC7_MODE) {
                    ok = false;
                }
            } else if r.is_err() {
                ok = false;
            }
        }
        if ok {
            s.ok("F0018-正-12-无表族确定性拒绝");
        } else {
            s.fail("F0018-正-12-无表族确定性拒绝", "拒绝路径错误码不符");
        }
    }

    // 正-13：边缘块——非 4 倍宽高逐像素等于「整块解完再裁剪」
    {
        let mut src = [0u8; 16];
        src[0..8].copy_from_slice(&mk_bc1(
            0xFFFF,
            0x0000,
            &[0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3],
        ));
        src[8..16].copy_from_slice(&mk_bc1(
            0x001F,
            0x07E0,
            &[3, 2, 1, 0, 3, 2, 1, 0, 3, 2, 1, 0, 3, 2, 1, 0],
        ));
        let mut small = [0u8; 5 * 3 * 4];
        let st = decode_surface(TxFormat::Bc1, &src, 5, 3, OutFmt::Rgba8, &mut small)
            .expect("5x3 解码");
        let mut full = [0u8; 8 * 4 * 4];
        let _ = decode_surface(TxFormat::Bc1, &src, 8, 4, OutFmt::Rgba8, &mut full)
            .expect("8x4 解码");
        let mut ok = true;
        for y in 0..3 {
            for x in 0..5 {
                for c in 0..4 {
                    if small[(y * 5 + x) * 4 + c] != full[(y * 8 + x) * 4 + c] {
                        ok = false;
                    }
                }
            }
        }
        // 裁剪账：2 块 × 16 像素 = 32 解出，5×3=15 写出 → 17 丢弃
        if ok && st.px == 15 && st.clipped == 17 && st.blocks == 2 && st.decoded == 2 {
            s.ok("F0018-正-13-边缘块裁剪守恒");
        } else {
            s.fail("F0018-正-13-边缘块裁剪守恒", "边缘像素与裁剪账不符");
        }
    }

    // 正-14：缓冲/尺寸/块长的拒绝路径（各自专属码，不许混用）
    {
        let blk = mk_bc1(0xFFFF, 0x0000, &[0; 16]);
        let mut out = [0u8; 4 * 4 * 4];
        let a = decode_surface(TxFormat::Bc1, &[], 4, 4, OutFmt::Rgba8, &mut out);
        let b = decode_surface(TxFormat::Bc1, &blk, 0, 4, OutFmt::Rgba8, &mut out);
        let c = decode_surface(TxFormat::Bc1, &blk, 4, 4, OutFmt::Rgba8, &mut out[0..8]);
        let d = decode_surface(TxFormat::Bc1, &blk[0..4], 4, 4, OutFmt::Rgba8, &mut out);
        let ok = a == Err(E_TX_BLOCK_SHORT)
            && b == Err(E_TX_DIM)
            && c == Err(E_TX_OUT_SHORT)
            && d == Err(E_TX_BLOCK_SHORT);
        if ok {
            s.ok("F0018-正-14-拒绝路径码位分离");
        } else {
            s.fail("F0018-正-14-拒绝路径码位分离", "拒绝码混用或漏检");
        }
    }

    // 正-15：分块并行——分段并集必须与串行逐字节相等
    {
        let mut src = [0u8; 16 * 16];
        let mut state = 0xDEAD_BEEFu32;
        for k in 0..16 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let c0 = (state >> 8) as u16;
            let c1 = (state >> 20) as u16;
            let sels = [
                (state & 3) as u8,
                ((state >> 2) & 3) as u8,
                ((state >> 4) & 3) as u8,
                ((state >> 6) & 3) as u8,
                0,
                1,
                2,
                3,
                0,
                1,
                2,
                3,
                3,
                2,
                1,
                0,
            ];
            src[k * 8..k * 8 + 8].copy_from_slice(&mk_bc1(c0, c1, &sels));
        }
        let (w, h) = (32usize, 8usize); // 8 列 × 2 行 = 16 块
        let mut serial = vec![0u8; w * h * 4];
        let st = decode_surface(TxFormat::Bc1, &src, w, h, OutFmt::Rgba8, &mut serial)
            .expect("串行解码");
        let mut par = vec![0u8; w * h * 4];
        let plan = plan_blocks(16, 4);
        let mut ok = plan.len() == 4;
        let mut acc = 0usize;
        for (b0, b1) in plan.iter() {
            if *b0 != acc {
                ok = false;
            }
            acc = *b1;
            let s1 = decode_surface_block_range(
                TxFormat::Bc1,
                &src,
                w,
                h,
                OutFmt::Rgba8,
                &mut par,
                *b0,
                *b1,
            );
            match s1 {
                Ok(x) => {
                    if x.blocks as usize != b1 - b0 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        if acc != 16 {
            ok = false;
        }
        if serial != par {
            ok = false;
        }
        if ok && st.blocks == 16 && st.px == (w * h) as u32 {
            s.ok("F0018-正-15-分块并行并集一致");
        } else {
            s.fail("F0018-正-15-分块并行并集一致", "分片结果与串行不等");
        }
    }

    // 正-16：分片计划守恒（覆盖、无重叠、长度差 ≤1、workers>total 不产空段）
    {
        let mut ok = true;
        for total in [0usize, 1, 2, 3, 7, 16, 17, 100, 1000] {
            for workers in [0usize, 1, 2, 3, 4, 8, 64] {
                let p = plan_blocks(total, workers);
                if total == 0 || workers == 0 {
                    if !p.is_empty() {
                        ok = false;
                    }
                    continue;
                }
                let mut at = 0usize;
                let mut lo = usize::MAX;
                let mut hi = 0usize;
                for (a, b) in p.iter() {
                    if *a != at || *b <= *a {
                        ok = false;
                    }
                    at = *b;
                    let n = b - a;
                    lo = lo.min(n);
                    hi = hi.max(n);
                }
                if at != total || hi - lo > 1 {
                    ok = false;
                }
                if p.len() != workers.min(total) {
                    ok = false;
                }
            }
        }
        if ok {
            s.ok("F0018-正-16-分片计划守恒");
        } else {
            s.fail("F0018-正-16-分片计划守恒", "分片不覆盖/不均衡/有空段");
        }
    }

    // 正-17：缓存命中——同内容块重复出现必须只解一次
    {
        let one = mk_bc1(0x8410, 0x4208, &[0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3]);
        let mut src = [0u8; 64];
        for k in 0..8 {
            src[k * 8..k * 8 + 8].copy_from_slice(&one);
        }
        let (w, h) = (32usize, 4usize); // 8 块
        let mut out = vec![0u8; w * h * 4];
        let mut cache = BlockCache::new(4);
        let st = decode_surface_cached(&mut cache, TxFormat::Bc1, &src, w, h, OutFmt::Rgba8, &mut out)
            .expect("缓存解码");
        let cs = cache.stats();
        if st.blocks == 8 && st.decoded == 1 && cs.hits == 7 && cs.misses == 1 && cs.hit_permille() == 875
        {
            s.ok("F0018-正-17-块缓存命中去重");
        } else {
            s.fail("F0018-正-17-块缓存命中去重", "同内容块未去重");
        }
    }

    // 正-18：缓存逐出会计 + 结果仍逐字节正确（缓存不影响像素）
    {
        let mut cache = BlockCache::new(2);
        let mut px = [0u16; BLOCK_UNITS];
        px[0] = 1;
        cache.put(1, px);
        px[0] = 2;
        cache.put(2, px);
        px[0] = 3;
        cache.put(3, px); // 挤掉 key=1
        let cs = cache.stats();
        let ok = cs.evictions == 1
            && cs.size == 2
            && cache.get(1).is_none()
            && cache.get(3) == Some(px);
        // 零容量缓存不许命中，也不许 panic
        let mut z = BlockCache::new(0);
        z.put(9, px);
        let zok = z.get(9).is_none() && z.stats().size == 0 && z.stats().hit_permille() == 0;
        if ok && zok {
            s.ok("F0018-正-18-缓存逐出与零容量");
        } else {
            s.fail("F0018-正-18-缓存逐出与零容量", "逐出会计或零容量行为错");
        }
    }

    // 正-19：直出目标格式——Rgba8 不二次转换（LDR 无损）、Rgba16f 端点精确
    {
        // 像素 0 = 白（索引 0 → c0=0xFFFF），像素 1 = 黑（索引 1 → c1=0x0000）
        let b = mk_bc1(0xFFFF, 0x0000, &[0, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut o8 = [0u8; 64];
        let _ = decode_surface(TxFormat::Bc1, &b, 4, 4, OutFmt::Rgba8, &mut o8).expect("rgba8");
        let mut o16 = [0u8; 128];
        let _ = decode_surface(TxFormat::Bc1, &b, 4, 4, OutFmt::Rgba16f, &mut o16).expect("rgba16f");
        let h0 = u16::from_le_bytes([o16[0], o16[1]]);
        let ha = u16::from_le_bytes([o16[6], o16[7]]);
        let h1 = u16::from_le_bytes([o16[8], o16[9]]);
        let ok = o8[0] == 255 && o8[3] == 255 && o8[4] == 0 && o8[7] == 255
            && h0 == 0x3C00
            && ha == 0x3C00
            && h1 == 0x0000;
        // HDR 源强走 8bit 必须先在请求层拒绝；走半精度才轮到「无表」拒绝
        let mut dummy = [0u8; 64];
        let rej8 = decode_surface(TxFormat::Bc6h, &[0u8; 16], 4, 4, OutFmt::Rgba8, &mut dummy)
            == Err(E_TX_OUT_FMT);
        let rej16 = decode_surface(TxFormat::Bc6h, &[0u8; 16], 4, 4, OutFmt::Rgba16f, &mut dummy)
            == Err(E_TX_HDR_UNSUP);
        if ok && rej8 && rej16 {
            s.ok("F0018-正-19-直出目标格式");
        } else {
            s.fail("F0018-正-19-直出目标格式", "8bit/半精度直出不符");
        }
    }

    // 正-20：诚实性能标注——blocks_per_us 必须由周期推导，且不许声称能流式
    {
        let mut ok = true;
        for f in ALL_FORMATS.iter() {
            let n = perf_note(*f);
            if n.blocks_per_us as u64 != CLOCK_MHZ / n.per_block_cycles.max(1) as u64 {
                ok = false;
            }
            if n.hw_speedup_at_least < 10 {
                ok = false;
            }
            if n.streaming_ok {
                ok = false;
            }
            if n.note.is_empty() || n.note.len() > 200 {
                ok = false;
            }
            if n.per_block_cycles < BLOCK_PX as u32 {
                ok = false;
            }
        }
        // 4K BC1 一块表面在 8 线程下的耗时必须是有限正数，且比单线程小
        let one = surface_us(3840, 2160, TxFormat::Bc1, 1);
        let eight = surface_us(3840, 2160, TxFormat::Bc1, 8);
        if one == 0 || eight == 0 || eight >= one {
            ok = false;
        }
        if ok {
            s.ok("F0018-正-20-诚实性能标注");
        } else {
            s.fail("F0018-正-20-诚实性能标注", "性能标注可推导性或诚实性不符");
        }
    }

    // 编-01：半精度转换端点（0→0x0000、1→0x3C00、0.5→0x3800、非规格化保号）
    {
        let a = f32_to_f16(0.0);
        let b = f32_to_f16(1.0);
        let c = f32_to_f16(0.5);
        let d = f32_to_f16(-1.0);
        let e = f32_to_f16(1.0e-6);
        // 1e-6 落在半精度非规格化区间：非零且指数为 0
        let ok = a == 0x0000
            && b == 0x3C00
            && c == 0x3800
            && d == 0xBC00
            && e != 0
            && (e & 0x7C00) == 0
            && u16_to_f16(0) == 0
            && u16_to_f16(0xFFFF) == 0x3C00;
        if ok {
            s.ok("F0018-编-01-半精度端点");
        } else {
            s.fail("F0018-编-01-半精度端点", "f32→f16 端点或非规格化不符");
        }
    }

    // 编-02：8bit/16bit 往返必须逐位恒等（widen8/narrow8 是整个直出链的地基）
    {
        let mut ok = true;
        for v in 0..=255u16 {
            if narrow8(widen8(v as u8)) != v as u8 {
                ok = false;
            }
        }
        if ok && narrow8(0) == 0 && narrow8(0xFFFF) == 255 {
            s.ok("F0018-编-02-8bit16bit往返");
        } else {
            s.fail("F0018-编-02-8bit16bit往返", "widen8/narrow8 不互逆");
        }
    }

    s
}
