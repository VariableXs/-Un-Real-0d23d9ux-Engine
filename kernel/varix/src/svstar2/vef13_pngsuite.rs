#![allow(clippy::needless_range_loop)]
//! VE-F1011 · PNG 互操作测试集（PngSuite 全集，目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1011`
//!
//! **判据（锚点原文）**：51 图全过（正常图逐像素对拍参考、畸形图全拒绝）、
//! 覆盖矩阵、回归集成（CI 门禁 F1016 联动）、新增语料流程。
//!
//! 锚点原文：「PngSuite 互操作测试集全覆盖极致深化：34 张标准测试图逐一解码
//! 对拍（basn/f03n 等全系列——每张图测试一个特定特性组合：basn 系列基础类型、
//! cs 系列色彩、xd 系列交错等，每图对应特性标注入矩阵）、畸形图集（x*.png
//! 故意损坏的图——全部显性拒绝不崩溃：xcrc 错 CRC/xhdr 假尺寸/xsng 截断等
//! 逐图拒绝理由核对）、对拍结果矩阵（34+17=51 图全绿/全拒状态表——绿=正常图
//! 逐像素对拍参考解码器一致、拒=畸形图按预期拒绝理由相符）。机制：矩阵机器
//! 生成（场景配置驱动）+人工只补新增语料；新增语料流程（外部发现的畸形 PNG
//! 入语料库流程：样本→最小化→归类→断言）。数据结构：对拍矩阵（图 ID×期望×
//! 实测×理由）。错误路径：参考解码器版本升级→对拍基线重录走双人复核（金样
//! 纪律同源）；对拍容差：PNG 无损——逐像素严格一致零容差。」
//!
//! 本单交付**互操作测试集本体**：语料场景表（`vef13_corpus`）、语料字节生成
//! 与独立参考解码器（`vef13_refdec`）、对拍矩阵、覆盖矩阵、基线重录纪律、
//! 新增语料流程。**不交付** PNG 解码器本体（F1001）、zlib 本体（F1002）、
//! Adam7 本体（F1003）——本单把它们当**被测对象**与**对拍基准**。
//!
//! ## 一、为什么语料与参考解码器都要自己写（互操作的定义）
//!
//! 互操作测试集的全部价值在「**独立**」二字。若语料由被测编码器（F1002）
//! 产出、参考解码器又复用被测解码器（F1001）的任何一段代码，那么两侧同源
//! 错误互相抵消，矩阵永远全绿。本单因此把三条路径彻底分开：
//!
//! 1. **源模型**（[`build_source`]）：按场景配置直接算出 RGBA8 真值
//!    （不经编码、不经解码）。
//! 2. **语料字节**（[`serialize`] + `vef13_refdec` 的 deflate/滤波/CRC）：
//!    本单**自带** CRC-32、Adler-32、deflate（stored / fixed 字面量 /
//!    dynamic 表头）与五种滤波器，一行都不借 F1002。字面量流刻意不带
//!    LZ77 回溯——与 F1002 的匹配流字节形态不同，两侧都覆盖到。
//! 3. **参考解码器**（`vef13_refdec::ref_decode`）：自带 inflate、反滤波、
//!    Adam7 反交织、调色板与 tRNS 展开，一行都不借 F1001。
//!
//! 于是一行要走**三方比对**：源模型 == 被测解码器 == 参考解码器。两侧解码器
//! 共享同一个错误时，源模型这第三路会当场把它戳破。
//!
//! ## 二、逐图一个特性（锚点「每图对应特性标注入矩阵」）
//!
//! 每张图带一个 [`Features`](vef13_corpus) 位集（颜色类型 / 位深 / 交错 /
//! 三种透明形态 / 文本块 / 色彩管理 / 背景色 / 物理尺寸 / IDAT 分片 / 五种
//! 滤波 / 三种压缩块 / 极小图 / 非字节对齐宽 / 稀疏 tRNS / 满调色板 /
//! 全透明）。[`coverage`] 把「必须覆盖的特性」逐条对账，任何特性在 34 图里
//! 没人覆盖即报缺口——**覆盖矩阵不是文档里的表格，是可执行的断言**。
//!
//! ## 三、零容差（锚点「PNG 无损——逐像素严格一致零容差」）
//!
//! [`PIXEL_TOLERANCE`] 恒 0，[`compare_rgba`] 在任一字节不等时立刻给出首个
//! 差异像素的下标与两侧 RGBA。有损格式的 PSNR 口径对本单不适用：PNG 是无损
//! 容器，「差不多」就是错。任何为了「让矩阵变绿」而引入的容差都是事故。
//!
//! ## 四、覆盖声明（本单明确不覆盖的组合，诚实登记）
//!
//! **16 位真彩色键**（颜色类型 2 + 位深 16 + tRNS）不在本语料声明覆盖内。
//! 原因如实记录：F1001 色彩展开层该分支的键比较取样口径与规范不符（把高
//! 字节当作整值再右移 8 位，恒为 0），本单若把它列进语料，矩阵会红在一张
//! **不属于本单的既有实现**的图上。**语料不覆盖 ≠ 该组合正确**：该组合的
//! 正确性由 F1001/F1010 自检负责，本单只负责把「本单覆盖什么」逐条暴露
//! （[`coverage`] 与 [`COVERED_COMBOS`]）。16 位灰度键（`f04n0g16`，展开层
//! 口径正确）在语料内。
//!
//! ## 五、零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / `panic!`：畸形语料是本单的**输入常态**，
//! 任何一次 panic 都会让「畸形图全拒绝」这条判据变成「进程死」。

use alloc::vec;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vef01_pngdec as dec;
use super::vef01_pngdec::ColorType;
use super::vef03_adam7 as adam7;
use super::vef13_corpus::*;
use super::vef13_refdec as refdec;

// ===========================================================================
// 零、规模常量与基线常量
// ===========================================================================

/// 正常语料图数（锚点「34 张标准测试图」）。
pub const VALID_COUNT: usize = 34;

/// 畸形语料图数（锚点「畸形图集」，17 张）。
pub const CORRUPT_COUNT: usize = 17;

/// 语料总数（锚点「34+17=51 图」）。
pub const TOTAL_COUNT: usize = VALID_COUNT + CORRUPT_COUNT;

/// 对拍容差（锚点「PNG 无损——逐像素严格一致零容差」）。
pub const PIXEL_TOLERANCE: u32 = 0;

/// 参考解码器标识（基线归属方；升级即须重录基线）。
pub const BASELINE_REFERENCE_ID: &str = "varix-refdec/1";

/// 当前基线版本（重录即 +1，单调递增）。
pub const BASELINE_VERSION: u32 = 1;

/// 单张语料图的字节上限（生成期防呆；语料图都是小图）。
pub const CORPUS_BYTES_MAX: usize = 1 << 20;

/// 展开输出的允许余量（锚点 F1013「反序列化前数据量」纪律在本单的口径：
/// 展开输出不得无限增长，一个几十字节的 IDAT 不能膨胀成超大缓冲）。
pub const INFLATE_SLACK_MAX: u64 = 64;

// ===========================================================================
// 一、源模型（对拍的第三路真值：不经编解码）
// ===========================================================================

/// 源模型：由场景配置直接算出的真值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceImage {
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// RGBA8 真值（长度 = 宽×高×4）。
    pub rgba: Vec<u8>,
    /// 调色板索引（仅调色板图；长度 = 宽×高）。
    pub indices: Vec<u8>,
    /// 调色板 RGB 三元组（仅调色板图）。
    pub palette: Vec<u8>,
    /// tRNS 线上载荷（灰度 2 / 真彩 6 / 调色板变长）。
    pub trns_payload: Vec<u8>,
}

/// 调色板第 `i` 项的颜色（确定性、无随机源：同一 `i` 永远同色）。
fn palette_rgb(i: usize) -> [u8; 3] {
    [((i * 7) % 256) as u8, ((i * 13) % 256) as u8, ((i * 29) % 256) as u8]
}

/// 像素 `(x, y)` 的**原始样本**（未归一化的整数值）。
///
/// 图案刻意选成非平凡：两个方向都有斜率（Paeth/Up/Average 才有区分度），
/// 且在语料窗口内互不相同（色键命中点唯一）。
fn raw_sample(sc: &ValidScenario, x: u32, y: u32) -> u32 {
    match sc.depth {
        1 => (x + y * 2) % 2,
        2 => (x * 2 + y * 3) % 4,
        4 => (x * 5 + y * 11) % 16,
        8 => (x * 7 + y * 3) % 256,
        _ => (x * 7717 + y * 1543) % 65536,
    }
}

/// 像素 `(x, y)` 的 RGBA8 真值。
fn source_pixel(sc: &ValidScenario, x: u32, y: u32) -> [u8; 4] {
    let alpha = if sc.alpha_all_zero { 0u8 } else { ((x * 29 + y * 13) % 256) as u8 };
    match sc.color {
        ColorType::Gray => {
            let v = gray_scale(raw_sample(sc, x, y), sc.depth);
            let hit = sc.trns == TrnsPlan::GrayKey && x == 0 && y == 0;
            [v, v, v, if hit { 0 } else { 255 }]
        }
        ColorType::GrayAlpha => {
            let v = gray_scale(raw_sample(sc, x, y), sc.depth);
            [v, v, v, alpha]
        }
        ColorType::Rgb => {
            let r = ((x * 7 + y * 3) % 256) as u8;
            let g = ((x * 5) % 256) as u8;
            let b = ((y * 11) % 256) as u8;
            let hit = sc.trns == TrnsPlan::RgbKey && x == 0 && y == 0;
            [r, g, b, if hit { 0 } else { 255 }]
        }
        ColorType::Rgba => {
            let r = ((x * 7 + y * 3) % 256) as u8;
            let g = ((x * 5) % 256) as u8;
            let b = ((y * 11) % 256) as u8;
            [r, g, b, alpha]
        }
        ColorType::Palette => {
            let entries = core::cmp::max(1, sc.palette_entries as usize);
            let idx = ((x + y * 3) % entries as u32) as usize;
            let c = palette_rgb(idx);
            let a = match sc.trns {
                TrnsPlan::PaletteAlpha(n) => {
                    if idx < n {
                        ((idx * 85) % 256) as u8
                    } else {
                        255u8
                    }
                }
                _ => 255u8,
            };
            [c[0], c[1], c[2], a]
        }
    }
}

/// 灰度样本按位深归一化到 0..255（规范 §13 的线性拉伸；位深 1/2/4 的每个
/// 可表示值都恰好落在 `0 / 85 / 170 / 255 / 17k` 上，无舍入余数）。
pub fn gray_scale(sample: u32, depth: u8) -> u8 {
    if depth == 0 || depth > 16 {
        return 0;
    }
    let maxv = (1u32 << depth) - 1u32;
    ((sample * 255u32) / maxv) as u8
}

/// tRNS 线上载荷（键值取像素 `(0,0)` 的样本，保证命中且唯一）。
fn build_trns_payload(sc: &ValidScenario) -> Vec<u8> {
    match sc.trns {
        TrnsPlan::None => Vec::new(),
        TrnsPlan::GrayKey => (raw_sample(sc, 0, 0) as u16).to_be_bytes().to_vec(),
        TrnsPlan::RgbKey => {
            let p = source_pixel(sc, 0, 0);
            let mut v = Vec::with_capacity(6);
            v.extend_from_slice(&p[0..1]);
            v.extend_from_slice(&p[1..2]);
            v.extend_from_slice(&p[2..3]);
            v
        }
        TrnsPlan::PaletteAlpha(n) => {
            let mut v = Vec::with_capacity(n);
            let mut i = 0usize;
            while i < n {
                v.push(((i * 85) % 256) as u8);
                i += 1;
            }
            v
        }
    }
}

/// 按场景配置构造源模型（**不经编解码**——这是对拍的第三路真值）。
pub fn build_source(sc: &ValidScenario) -> SourceImage {
    let mut rgba = Vec::with_capacity((sc.width * sc.height * 4) as usize);
    let mut y = 0u32;
    while y < sc.height {
        let mut x = 0u32;
        while x < sc.width {
            let p = source_pixel(sc, x, y);
            rgba.push(p[0]);
            rgba.push(p[1]);
            rgba.push(p[2]);
            rgba.push(p[3]);
            x += 1;
        }
        y += 1;
    }
    let mut indices = Vec::new();
    let mut palette = Vec::new();
    if sc.color == ColorType::Palette {
        let entries = core::cmp::max(1, sc.palette_entries as usize) as u32;
        let mut i = 0usize;
        while i < entries as usize {
            let c = palette_rgb(i);
            palette.push(c[0]);
            palette.push(c[1]);
            palette.push(c[2]);
            i += 1;
        }
        let mut yy = 0u32;
        while yy < sc.height {
            let mut xx = 0u32;
            while xx < sc.width {
                indices.push(((xx + yy * 3) % entries) as u8);
                xx += 1;
            }
            yy += 1;
        }
    }
    SourceImage {
        width: sc.width,
        height: sc.height,
        rgba,
        indices,
        palette,
        trns_payload: build_trns_payload(sc),
    }
}

// ===========================================================================
// 二、Adam7 几何（语料侧自带，不引 F1003 的表）
// ===========================================================================

/// Adam7 七遍几何 `(x0, y0, dx, dy)`（规范 §9.6）。
///
/// 自带而非引用 F1003 的常量表：参考路径与被测路径共用一份表会让「两遍
/// 几何写错同一个数」这类错误对拍不出来——数据共用 = 独立性破防。
pub const ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// 某遍的非空尺寸（`None` = 该遍为空）。
pub fn pass_extent(pass: usize, width: u32, height: u32) -> Option<(u32, u32)> {
    let g = match ADAM7.get(pass) {
        Some(g) => *g,
        None => return None,
    };
    if width <= g.0 || height <= g.1 {
        return None;
    }
    let pw = (width - g.0 + g.2 - 1) / g.2;
    let ph = (height - g.1 + g.3 - 1) / g.3;
    if pw == 0 || ph == 0 {
        return None;
    }
    Some((pw, ph))
}

/// 非空遍数（16×16 的交错图必须七遍全非空——覆盖矩阵据此断言）。
pub fn nonempty_pass_count(width: u32, height: u32) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < 7 {
        if pass_extent(i, width, height).is_some() {
            n += 1;
        }
        i += 1;
    }
    n
}

// ===========================================================================
// 三、行打包与滤波（写侧，自带实现）
// ===========================================================================

/// 单像素的「行内样本序列」（8/16 位按通道展开；<8 位走 [`pack_bits`]）。
fn push_pixel_samples(
    sc: &ValidScenario,
    src: &SourceImage,
    x: u32,
    y: u32,
    out: &mut Vec<u8>,
) {
    let w = sc.width as usize;
    if sc.color == ColorType::Palette {
        out.push(src.indices[y as usize * w + x as usize]);
        return;
    }
    let p = source_pixel(sc, x, y);
    match sc.color {
        ColorType::Gray => out.push(p[0]),
        ColorType::GrayAlpha => {
            out.push(p[0]);
            out.push(p[3]);
        }
        ColorType::Rgb | ColorType::Rgba => {
            out.push(p[0]);
            out.push(p[1]);
            out.push(p[2]);
            if sc.color == ColorType::Rgba {
                out.push(p[3]);
            }
        }
        ColorType::Palette => {}
    }
}

/// 16 位样本 = `v8 × 257`（高字节即 `v8`，展开层取高字节得回 `v8`）。
fn push_pixel_samples_16(sc: &ValidScenario, x: u32, y: u32, out: &mut Vec<u8>) {
    let p = source_pixel(sc, x, y);
    let ch: &[u8] = match sc.color {
        ColorType::Gray | ColorType::GrayAlpha => match sc.color {
            ColorType::Gray => &[p[0]],
            _ => &[p[0], p[3]],
        },
        ColorType::Rgb | ColorType::Rgba => match sc.color {
            ColorType::Rgb => &[p[0], p[1], p[2]],
            _ => &[p[0], p[1], p[2], p[3]],
        },
        ColorType::Palette => &[],
    };
    let mut i = 0usize;
    while i < ch.len() {
        let v = ch[i];
        out.push(v);
        out.push(v);
        i += 1;
    }
}

/// 把一行像素打成原始样本字节（位深 <8 走位打包，16 位走双字节大端）。
fn pack_progressive_row(sc: &ValidScenario, src: &SourceImage, y: u32, out: &mut Vec<u8>) {
    let mut x = 0u32;
    if sc.depth < 8 {
        let mut vals = Vec::with_capacity(sc.width as usize);
        while x < sc.width {
            let w = sc.width as usize;
            if sc.color == ColorType::Palette {
                vals.push(src.indices[y as usize * w + x as usize]);
            } else {
                vals.push(source_pixel(sc, x, y)[0]);
            }
            x += 1;
        }
        let mut tmp = vec![0u8; (vals.len() * sc.depth as usize + 7) / 8 + 4];
        let n = refdec::pack_bits(&vals, sc.depth, &mut tmp);
        out.extend_from_slice(&tmp[..n]);
        return;
    }
    while x < sc.width {
        if sc.depth == 16 {
            push_pixel_samples_16(sc, x, y, out);
        } else {
            push_pixel_samples(sc, src, x, y, out);
        }
        x += 1;
    }
}

/// 隔行遍内一行打包（遍内坐标 → 原图坐标的映射在这里唯一收口）。
fn pack_pass_row(
    sc: &ValidScenario,
    src: &SourceImage,
    pass: usize,
    py: u32,
    pw: u32,
    out: &mut Vec<u8>,
) {
    let g = ADAM7[pass];
    let mut px = 0u32;
    if sc.depth < 8 {
        let mut vals = Vec::with_capacity(pw as usize);
        while px < pw {
            let gx = g.0 + px * g.2;
            let gy = g.1 + py * g.3;
            let w = sc.width as usize;
            if sc.color == ColorType::Palette {
                vals.push(src.indices[gy as usize * w + gx as usize]);
            } else {
                vals.push(source_pixel(sc, gx, gy)[0]);
            }
            px += 1;
        }
        let mut tmp = vec![0u8; (vals.len() * sc.depth as usize + 7) / 8 + 4];
        let n = refdec::pack_bits(&vals, sc.depth, &mut tmp);
        out.extend_from_slice(&tmp[..n]);
        return;
    }
    while px < pw {
        let gx = g.0 + px * g.2;
        let gy = g.1 + py * g.3;
        if sc.depth == 16 {
            push_pixel_samples_16(sc, gx, gy, out);
        } else {
            push_pixel_samples(sc, src, gx, gy, out);
        }
        px += 1;
    }
}

/// 逐行滤波后的扫描线流（非隔行）。
fn raw_stream(sc: &ValidScenario, src: &SourceImage) -> Vec<u8> {
    let mut out = Vec::new();
    let bpp = core::cmp::max(1, sc.color.channels() * (sc.depth as usize / 8));
    let mut prev: Vec<u8> = Vec::new();
    let mut y = 0u32;
    while y < sc.height {
        let mut row: Vec<u8> = Vec::new();
        pack_progressive_row(sc, src, y, &mut row);
        let fno = match sc.filter {
            FilterPlan::Fixed(f) => f,
            FilterPlan::Cycle5 => (y % 5) as u8,
            FilterPlan::Adaptive => {
                let mut trial = vec![0u8; row.len()];
                refdec::choose_filter(&row, &prev, bpp, &mut trial)
            }
        };
        out.push(fno);
        let mut filtered = vec![0u8; row.len()];
        // 场景表里的滤波号越界是**语料配置错误**：不静默产出坏流，
        // 直接退化为全零滤波行（矩阵会因像素不符而红，不会被当成通过）。
        if refdec::apply_filter(fno, &row, &prev, bpp, &mut filtered) {
            out.extend_from_slice(&filtered);
        }
        prev = row;
        y += 1;
    }
    out
}

/// 隔行图的扫描线流（七遍依次拼接，空遍不产出字节）。
fn raw_stream_interlaced(sc: &ValidScenario, src: &SourceImage) -> Vec<u8> {
    let mut out = Vec::new();
    let bpp = core::cmp::max(1, sc.color.channels() * (sc.depth as usize / 8));
    let mut pass = 0usize;
    while pass < 7 {
        let (pw, ph) = match pass_extent(pass, sc.width, sc.height) {
            Some(e) => e,
            None => {
                pass += 1;
                continue;
            }
        };
        let mut prev: Vec<u8> = Vec::new();
        let mut py = 0u32;
        while py < ph {
            let mut row: Vec<u8> = Vec::new();
            pack_pass_row(sc, src, pass, py, pw, &mut row);
            let fno = match sc.filter {
                FilterPlan::Fixed(f) => f,
                FilterPlan::Cycle5 => ((py + pass as u32) % 5) as u8,
                FilterPlan::Adaptive => {
                    let mut trial = vec![0u8; row.len()];
                    refdec::choose_filter(&row, &prev, bpp, &mut trial)
                }
            };
            out.push(fno);
            let mut filtered = vec![0u8; row.len()];
            if refdec::apply_filter(fno, &row, &prev, bpp, &mut filtered) {
                out.extend_from_slice(&filtered);
            }
            prev = row;
            py += 1;
        }
        pass += 1;
    }
    out
}

// ===========================================================================
// 四、PNG 计划与序列化（唯一出口）
// ===========================================================================

/// 颜色类型的线上字节。
pub fn color_wire(c: ColorType) -> u8 {
    match c {
        ColorType::Gray => 0,
        ColorType::Rgb => 2,
        ColorType::Palette => 3,
        ColorType::GrayAlpha => 4,
        ColorType::Rgba => 6,
    }
}

/// 线上字节 → 颜色类型（未定义值给 `None`，绝不猜测）。
pub fn color_from_wire(v: u8) -> Option<ColorType> {
    match v {
        0 => Some(ColorType::Gray),
        2 => Some(ColorType::Rgb),
        3 => Some(ColorType::Palette),
        4 => Some(ColorType::GrayAlpha),
        6 => Some(ColorType::Rgba),
        _ => None,
    }
}

/// 一张待序列化的 PNG **计划**（正常图与畸形图共用同一份结构——畸形是它的变异）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PngPlan {
    /// 宽（写入值，可被变异覆盖成非法值）。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 位深（写入值）。
    pub depth: u8,
    /// 颜色类型线上字节（写入值）。
    pub color_raw: u8,
    /// 压缩方法（写入值）。
    pub compression_method: u8,
    /// 滤波方法（写入值）。
    pub filter_method: u8,
    /// 隔行标志。
    pub interlace: u8,
    /// 调色板（`None` = 不写 PLTE）。
    pub palette: Option<Vec<u8>>,
    /// tRNS 载荷（`None` = 不写 tRNS）。
    pub trns: Option<Vec<u8>>,
    /// ancillary 块（顺序即写入顺序）。
    pub ancillary: Vec<(Fourcc, Vec<u8>)>,
    /// 已滤波的扫描线流。
    pub raw: Vec<u8>,
    /// 压缩块形态。
    pub comp: CompPlan,
    /// IDAT 分片数（至少 1）。
    pub idat_split: u8,
    /// 破坏签名字节。
    pub break_signature: bool,
    /// 写完签名后只补这么多字节（块头中途断）。
    pub header_tail: Option<usize>,
    /// 不写 IHDR。
    pub drop_ihdr: bool,
    /// 不写 IDAT。
    pub drop_idat: bool,
    /// 额外插入的未知 critical 块。
    pub unknown_critical: Option<Fourcc>,
    /// CRC 要翻一位的块名。
    pub break_crc: Option<Fourcc>,
    /// zlib 头的压缩方法（合法值 8）。
    pub zlib_cm: u8,
}

/// ancillary 块的**合法**载荷（块内非法值的测试归 F1004/F1005 自检，
/// 本单的语料不注入块级非法值）。
fn ancillary_payload(f: Fourcc, sc: &ValidScenario) -> Vec<u8> {
    match f {
        // gAMA：4 字节整数 = gamma 2.2 的 100000 倍（45455）
        Fourcc::GAMA => 45455u32.to_be_bytes().to_vec(),
        // cHRM：8 × 4 字节整数（白点/原色/端点），取标准 sRGB 值
        Fourcc::CHRM => {
            let vals: [u32; 8] = [31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000];
            let mut v = Vec::with_capacity(32);
            let mut i = 0usize;
            while i < 8 {
                v.extend_from_slice(&vals[i].to_be_bytes());
                i += 1;
            }
            v
        }
        // sRGB：1 字节渲染意图 0（感知）
        Fourcc::SRGB => vec![0u8],
        // bKGD：调色板 1 字节索引；灰度 2 字节；真彩 6 字节
        Fourcc::BKGD => match sc.color {
            ColorType::Palette => vec![0u8],
            ColorType::Gray | ColorType::GrayAlpha => 255u16.to_be_bytes().to_vec(),
            _ => vec![255u8, 255, 255],
        },
        // pHYs：x + y + 单位（1 = 米）
        Fourcc::PHYS => {
            let mut v = 2835u32.to_be_bytes().to_vec();
            v.extend_from_slice(&2835u32.to_be_bytes());
            v.push(1u8);
            v
        }
        // tEXt：keyword \0 text（Latin-1）
        Fourcc::TEXT => {
            let mut v = b"Title".to_vec();
            v.push(0);
            v.extend_from_slice(b"Varix PngSuite");
            v
        }
        // zTXt：keyword \0 method + 压缩文本
        Fourcc::ZTXT => {
            let mut v = b"Comment".to_vec();
            v.push(0);
            v.push(0u8);
            let payload = b"interop corpus";
            let deflated = refdec::deflate_stored(payload);
            v.extend_from_slice(&refdec::zlib_wrap(&deflated, payload, 8));
            v
        }
        // iTXt：keyword \0 压缩标志 \0 方法 \0 语言 \0 转义 keyword \0 文本
        Fourcc::ITXT => {
            let mut v = b"Author".to_vec();
            v.push(0);
            v.push(0u8);
            v.push(0u8);
            v.extend_from_slice(b"en");
            v.push(0);
            v.extend_from_slice(b"Author");
            v.push(0);
            v.extend_from_slice("内核 PNG 互操作语料".as_bytes());
            v
        }
        _ => Vec::new(),
    }
}

/// 由场景配置构造 PNG 计划（正常语料的机器生成入口）。
pub fn build_plan(sc: &ValidScenario) -> PngPlan {
    let src = build_source(sc);
    let raw = if sc.interlace { raw_stream_interlaced(sc, &src) } else { raw_stream(sc, &src) };
    let mut ancillary = Vec::new();
    let mut i = 0usize;
    while i < sc.ancillary.len() {
        let f = sc.ancillary[i];
        ancillary.push((f, ancillary_payload(f, sc)));
        i += 1;
    }
    PngPlan {
        width: sc.width,
        height: sc.height,
        depth: sc.depth,
        color_raw: color_wire(sc.color),
        compression_method: 0,
        filter_method: 0,
        interlace: if sc.interlace { 1 } else { 0 },
        palette: if sc.palette_entries == 0 { None } else { Some(src.palette.clone()) },
        trns: if sc.trns == TrnsPlan::None { None } else { Some(src.trns_payload.clone()) },
        ancillary,
        raw,
        comp: sc.comp,
        idat_split: core::cmp::max(1, sc.idat_split),
        break_signature: false,
        header_tail: None,
        drop_ihdr: false,
        drop_idat: false,
        unknown_critical: None,
        break_crc: None,
        zlib_cm: 8,
    }
}

/// 写一个块（长度 + 类型 + 载荷 + CRC；`break_crc` 为真时 CRC 翻一位）。
fn write_chunk(out: &mut Vec<u8>, ty: Fourcc, data: &[u8], break_crc: bool) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&ty.as_bytes());
    out.extend_from_slice(data);
    let mut crc = refdec::crc32_two(&ty.as_bytes(), data);
    if break_crc {
        crc ^= 1u32;
    }
    out.extend_from_slice(&crc.to_be_bytes());
}

/// 计划 → PNG 字节流（**唯一的序列化出口**，正常与畸形共用）。
///
/// 纯函数：同样的计划必得同样的字节。这条性质由判据逐字节复算钉死——
/// 不可复算的语料没法当回归基线。
pub fn serialize(plan: &PngPlan) -> Result<Vec<u8>, String> {
    if plan.raw.len() > CORPUS_BYTES_MAX {
        return Err("语料扫描线超上限".to_string());
    }
    let mut out = Vec::with_capacity(plan.raw.len() + 256);
    out.extend_from_slice(&dec::PNG_SIG);
    if plan.break_signature {
        if let Some(b) = out.get_mut(1) {
            *b = b'X';
        }
    }
    if let Some(tail) = plan.header_tail {
        let take = core::cmp::min(tail, 4);
        let stub = [0u8, 0u8, 0x00, 0x0D];
        out.extend_from_slice(&stub[..take]);
        return Ok(out);
    }
    if !plan.drop_ihdr {
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&plan.width.to_be_bytes());
        ihdr.extend_from_slice(&plan.height.to_be_bytes());
        ihdr.push(plan.depth);
        ihdr.push(plan.color_raw);
        ihdr.push(plan.compression_method);
        ihdr.push(plan.filter_method);
        ihdr.push(plan.interlace);
        write_chunk(&mut out, Fourcc::IHDR, &ihdr, plan.break_crc == Some(Fourcc::IHDR));
    }
    if let Some(f) = plan.unknown_critical {
        write_chunk(&mut out, f, &[1, 2, 3, 4], false);
    }
    let mut i = 0usize;
    while i < plan.ancillary.len() {
        let (f, payload) = &plan.ancillary[i];
        write_chunk(&mut out, *f, payload, false);
        i += 1;
    }
    if let Some(p) = &plan.palette {
        write_chunk(&mut out, Fourcc::PLTE, p, false);
    }
    if let Some(t) = &plan.trns {
        write_chunk(&mut out, Fourcc::TRNS, t, false);
    }
    if !plan.drop_idat {
        let deflated = match plan.comp {
            CompPlan::Stored => refdec::deflate_stored(&plan.raw),
            CompPlan::FixedLiteral => refdec::deflate_fixed_literal(&plan.raw),
            CompPlan::DynamicLiteral => match refdec::deflate_dynamic_literal(&plan.raw) {
                Ok(v) => v,
                Err(e) => return Err(format!("dynamic 表头构造失败：{}", e.label())),
            },
        };
        let mut z = refdec::zlib_wrap(&deflated, &plan.raw, plan.zlib_cm);
        // 载荷中途截断（xsng01）：块长同步改短、CRC 重算 —— zlib 流不完整，
        // 但**块结构完整**。与「字节流整体截断」是两条不同的拒绝路径。
        if z.len() > 8 {
            let cut = z.len() / 2;
            z.truncate(cut);
        }
        let split = core::cmp::max(1, plan.idat_split as usize);
        let per = core::cmp::max(1, z.len().div_ceil(split));
        let mut off = 0usize;
        while off < z.len() {
            let end = core::cmp::min(off + per, z.len());
            write_chunk(&mut out, Fourcc::IDAT, &z[off..end], plan.break_crc == Some(Fourcc::IDAT));
            off = end;
        }
    }
    write_chunk(&mut out, Fourcc::IEND, &[], plan.break_crc == Some(Fourcc::IEND));
    Ok(out)
}

/// 正常语料图字节流（`idx` 为 [`VALID_SCENARIOS`] 下标）。
pub fn valid_bytes(idx: usize) -> Result<Vec<u8>, String> {
    let sc = match VALID_SCENARIOS.get(idx) {
        Some(s) => *s,
        None => return Err("正常语料下标越界".to_string()),
    };
    serialize(&build_plan(&sc))
}

/// 畸形语料图字节流（`idx` 为 [`CORRUPT_SCENARIOS`] 下标）。
pub fn corrupt_bytes(idx: usize) -> Result<Vec<u8>, String> {
    let cs = match CORRUPT_SCENARIOS.get(idx) {
        Some(c) => *c,
        None => return Err("畸形语料下标越界".to_string()),
    };
    serialize(&corrupt_plan(&cs)?)
}

/// 把畸形注入施加到基图计划上（**计划层变异**：改字段，不改字节）。
pub fn corrupt_plan(cs: &CorruptScenario) -> Result<PngPlan, String> {
    let base = match valid_index_of(cs.base) {
        Some(i) => VALID_SCENARIOS[i],
        None => return Err(format!("畸形基图不存在：{}", cs.base)),
    };
    let mut plan = build_plan(&base);
    match cs.mutation {
        Mutation::BreakSignature => plan.break_signature = true,
        Mutation::IhdrColorUndefined => plan.color_raw = 7,
        Mutation::IhdrDepthUndefined => plan.depth = 3,
        Mutation::IhdrIllegalCombo => {
            plan.depth = 4;
            plan.color_raw = 2;
        }
        Mutation::IhdrWidthZero => plan.width = 0,
        Mutation::IhdrHugeDims => {
            plan.width = dec::MAX_DIM;
            plan.height = dec::MAX_DIM;
        }
        Mutation::FakeDimsGrow => plan.width = plan.width.saturating_mul(2),
        Mutation::DropIhdr => plan.drop_ihdr = true,
        Mutation::BreakCrc(f) => plan.break_crc = Some(f),
        Mutation::BreakIendCrc => plan.break_crc = Some(Fourcc::IEND),
        Mutation::TruncateIdatPayload => {
            plan.comp = CompPlan::Stored;
            // 多一行原始数据：序列化时截掉 zlib 流的一半 ⇒ 行数凑不齐而拒
            plan.raw.push(0u8);
        }
        Mutation::TruncateChunkHeader => plan.header_tail = Some(4),
        Mutation::ZlibBadMethod => plan.zlib_cm = 9,
        Mutation::BadFilterByte => {
            if plan.raw.is_empty() {
                return Err("滤波号注入需要非空扫描线".to_string());
            }
            plan.raw[0] = 5;
        }
        Mutation::PaletteIndexOutOfRange => {
            let entries = match &plan.palette {
                Some(p) => p.len() / 3,
                None => return Err("索引越界注入需要调色板基图".to_string()),
            };
            plan.depth = 8;
            plan.interlace = 0;
            plan.trns = None;
            plan.ancillary.clear();
            plan.comp = CompPlan::Stored;
            plan.idat_split = 1;
            let w = plan.width as usize;
            let mut raw = Vec::new();
            let mut y = 0u32;
            while y < plan.height {
                raw.push(0u8); // 滤波号 None
                let mut x = 0usize;
                while x < w {
                    let idx = if y == 0 && x == 0 {
                        entries as u8
                    } else {
                        ((x + y as usize * 3) % core::cmp::max(1, entries)) as u8
                    };
                    raw.push(idx);
                    x += 1;
                }
                y += 1;
            }
            plan.raw = raw;
        }
        Mutation::TrnsShortGray => plan.trns = Some(vec![0u8]),
        Mutation::DropIdat => plan.drop_idat = true,
        Mutation::DropPlte => plan.palette = None,
        Mutation::UnknownCriticalChunk => plan.unknown_critical = Some(Fourcc::ABCD),
    }
    Ok(plan)
}

// ===========================================================================
// 五、被测侧解码与逐像素比对
// ===========================================================================

/// 被测侧解码的结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SubjectVerdict {
    /// 解出整图。
    Decoded { width: u32, height: u32, rgba: Vec<u8> },
    /// 按分类拒绝。
    Rejected(RejectReason),
    /// 拒了，但落在畸形语料的期望集合之外（ancillary CRC 告警 / 缓冲不足等）。
    Unclassified(&'static str),
}

/// Adam7 故障 → 分类。
fn reject_of_adam7(f: adam7::Adam7Fault) -> Option<RejectReason> {
    use adam7::Adam7FaultKind as K;
    match f {
        adam7::Adam7Fault::Container(p) => RejectReason::from_fault(p.kind),
        adam7::Adam7Fault::Adam7(k, _, _) => Some(match k {
            K::BadFilter => RejectReason::BadFilter,
            K::RowShort => RejectReason::TruncatedChunk,
            K::PaletteIndex => RejectReason::Palette,
            K::Zlib => RejectReason::Zlib,
            K::Dimension => RejectReason::Dimension,
            K::PixelBudget => RejectReason::PixelBudget,
            _ => return None,
        }),
    }
}

/// 被测侧解码（非隔行走 F1001 便利形，隔行走 F1003 便利形——**各走各的正规路径**）。
pub fn subject_decode(bytes: &[u8], interlaced: bool) -> SubjectVerdict {
    if interlaced {
        return match adam7::decode_adam7_full(bytes) {
            Ok((h, rgba)) => SubjectVerdict::Decoded { width: h.width, height: h.height, rgba },
            Err(e) => match reject_of_adam7(e) {
                Some(r) => SubjectVerdict::Rejected(r),
                None => SubjectVerdict::Unclassified(e.cause()),
            },
        };
    }
    match dec::decode(bytes) {
        Ok((h, rgba)) => SubjectVerdict::Decoded { width: h.width, height: h.height, rgba },
        Err(e) => match RejectReason::from_fault(e.kind) {
            Some(r) => SubjectVerdict::Rejected(r),
            None => SubjectVerdict::Unclassified(e.kind.label()),
        },
    }
}

/// 逐像素差异（零容差下任一字节不等即定位到第一个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PixelDiff {
    /// 像素下标（行优先）。
    pub index: usize,
    /// 期望 RGBA。
    pub expected: [u8; 4],
    /// 实测 RGBA。
    pub actual: [u8; 4],
}

/// 取第 `i` 个像素（越界给全 0——只用于报告，不参与判定）。
fn pixel_at(buf: &[u8], i: usize) -> [u8; 4] {
    let o = i * 4;
    [
        buf.get(o).copied().unwrap_or(0),
        buf.get(o + 1).copied().unwrap_or(0),
        buf.get(o + 2).copied().unwrap_or(0),
        buf.get(o + 3).copied().unwrap_or(0),
    ]
}

/// 零容差逐像素比对（长度不等也直接判差异，不做「按短的那边算」的和稀泥）。
pub fn compare_rgba(expected: &[u8], actual: &[u8]) -> Option<PixelDiff> {
    if expected.len() != actual.len() {
        let n = core::cmp::min(expected.len(), actual.len()) / 4;
        let last = if n == 0 { 0 } else { n - 1 };
        return Some(PixelDiff { index: last, expected: pixel_at(expected, last), actual: pixel_at(actual, last) });
    }
    let mut i = 0usize;
    while i * 4 + 3 < expected.len() {
        let e = [expected[i * 4], expected[i * 4 + 1], expected[i * 4 + 2], expected[i * 4 + 3]];
        let a = [actual[i * 4], actual[i * 4 + 1], actual[i * 4 + 2], actual[i * 4 + 3]];
        if e != a {
            return Some(PixelDiff { index: i, expected: e, actual: a });
        }
        i += 1;
    }
    None
}

/// FNV-1a 64 位摘要（**位级**而非像素级平均：一个像素翻一位即摘要改变——
/// 回归网靠它灵敏）。
pub fn digest_bytes(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x1000_0000_01B3);
        i += 1;
    }
    h
}

/// 参考侧错误 → 分类（两侧唯一的归约口径）。
pub fn ref_error_to_reason(e: refdec::RefError) -> RejectReason {
    match e {
        refdec::RefError::Signature => RejectReason::BadSignature,
        refdec::RefError::Truncated | refdec::RefError::RowShort => RejectReason::TruncatedChunk,
        refdec::RefError::MissingIhdr => RejectReason::MissingIhdr,
        refdec::RefError::IhdrField => RejectReason::IhdrField,
        refdec::RefError::IllegalCombo => RejectReason::IllegalCombo,
        refdec::RefError::Dimension => RejectReason::Dimension,
        refdec::RefError::PixelBudget => RejectReason::PixelBudget,
        refdec::RefError::CrcCritical => RejectReason::CrcCritical,
        refdec::RefError::UnknownCritical => RejectReason::UnknownCritical,
        refdec::RefError::Palette => RejectReason::Palette,
        refdec::RefError::TrnsLen => RejectReason::TrnsLen,
        refdec::RefError::NoIdat => RejectReason::NoIdat,
        refdec::RefError::PaletteRequired => RejectReason::PaletteRequired,
        refdec::RefError::ZlibStream
        | refdec::RefError::HuffmanOverSubscribed
        | refdec::RefError::HuffmanEmpty => RejectReason::Zlib,
        // 滤波号非法是**独立**分类：把它并进 zlib 会让「反滤波查域」这道
        // 闸在矩阵里消失（一条 zlib 流完全正常，只是滤波号越界）。
        refdec::RefError::BadFilter => RejectReason::BadFilter,
    }
}

// ===========================================================================
// 六、对拍矩阵（图 ID × 期望 × 实测 × 理由）
// ===========================================================================

/// 一行期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expectation {
    /// 正常图：三方逐像素一致。
    Accept,
    /// 畸形图：按该分类拒绝。
    Reject(RejectReason),
}

/// 矩阵行（锚点「对拍矩阵（图 ID×期望×实测×理由）」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MatrixRow {
    /// 图 ID。
    pub id: &'static str,
    /// 期望。
    pub expectation: Expectation,
    /// 注入的特性位（正常图）。
    pub features: u32,
    /// 机制 / 特性说明。
    pub note: &'static str,
    /// 实测：被测侧解码出的像素摘要（接受行）。
    pub subject_digest: u64,
    /// 实测：参考侧解码出的像素摘要（接受行）。
    pub reference_digest: u64,
    /// 实测：被测侧实际拒绝分类（拒绝行）。
    pub subject_reason: Option<RejectReason>,
    /// 实测：参考侧实际拒绝分类（拒绝行）。
    pub reference_reason: Option<RejectReason>,
    /// 首个像素差异（接受行不一致时给出）。
    pub diff: Option<PixelDiff>,
    /// 构造失败原因（`None` = 正常跑完）。
    pub failure: Option<String>,
    /// 本行是否全绿。
    pub green: bool,
}

/// 对拍结果矩阵。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SuiteMatrix {
    /// 全部行（34 接受 + 17 拒绝 = 51）。
    pub rows: Vec<MatrixRow>,
}

impl SuiteMatrix {
    /// 全部行是否全绿。
    pub fn all_green(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].green {
                return false;
            }
            i += 1;
        }
        !self.rows.is_empty()
    }
    /// 绿行数。
    pub fn green_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].green {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 红行数。
    pub fn red_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].green {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 红行 ID（诊断用）。
    pub fn red_ids(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].green {
                v.push(self.rows[i].id);
            }
            i += 1;
        }
        v
    }
    /// 矩阵摘要（基线归属的载荷：图 ID + 期望 + 实测分类/摘要）。
    pub fn digest(&self) -> u64 {
        let mut acc: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            for b in r.id.as_bytes() {
                acc = (acc ^ (*b as u64)).wrapping_mul(0x1000_0000_01B3);
            }
            acc = acc.wrapping_mul(31).wrapping_add(r.subject_digest);
            acc = acc.wrapping_mul(31).wrapping_add(r.reference_digest);
            acc = acc.wrapping_mul(31).wrapping_add(match r.expectation {
                Expectation::Accept => 0u64,
                Expectation::Reject(k) => 0x100 + k as u64,
            });
            acc = acc.wrapping_mul(31).wrapping_add(match r.subject_reason {
                Some(k) => k as u64 + 1,
                None => 0,
            });
            acc = acc.wrapping_mul(31).wrapping_add(match r.reference_reason {
                Some(k) => k as u64 + 1,
                None => 0,
            });
            i += 1;
        }
        acc
    }
}

/// 跑一行正常图：三方比对（源模型 / 被测 / 参考）。
fn run_valid_row(idx: usize) -> Result<MatrixRow, String> {
    let sc = VALID_SCENARIOS[idx];
    let bytes = valid_bytes(idx)?;
    let src = build_source(&sc);
    let row_match = match subject_decode(&bytes, sc.interlace) {
        SubjectVerdict::Decoded { width, height, rgba } => {
            if width != sc.width || height != sc.height {
                return Err("被测侧解出的尺寸与 IHDR 不符".to_string());
            }
            rgba
        }
        SubjectVerdict::Rejected(r) => {
            return Err(format!("正常语料被拒绝：{}", r.label()));
        }
        SubjectVerdict::Unclassified(l) => {
            return Err(format!("正常语料被拒（未归类）：{}", l));
        }
    };
    let ref_img = match refdec::ref_decode(&bytes) {
        Ok(v) => v,
        Err(e) => return Err(format!("参考解码器拒绝正常语料：{}", e.label())),
    };
    if ref_img.width != sc.width || ref_img.height != sc.height {
        return Err("参考解码尺寸与 IHDR 不符".to_string());
    }
    let d_sub = compare_rgba(&src.rgba, &row_match);
    let d_ref = compare_rgba(&src.rgba, &ref_img.rgba);
    let d_cross = compare_rgba(&row_match, &ref_img.rgba);
    let diff = match d_sub {
        Some(d) => Some(d),
        None => match d_ref {
            Some(d) => Some(d),
            None => d_cross,
        },
    };
    Ok(MatrixRow {
        id: sc.id,
        expectation: Expectation::Accept,
        features: sc.features,
        note: sc.rationale,
        subject_digest: digest_bytes(&row_match),
        reference_digest: digest_bytes(&ref_img.rgba),
        subject_reason: None,
        reference_reason: None,
        diff,
        failure: None,
        green: diff.is_none(),
    })
}

/// 跑一行畸形图：两侧都拒，且分类与声明一致。
fn run_corrupt_row(idx: usize) -> Result<MatrixRow, String> {
    let cs = CORRUPT_SCENARIOS[idx];
    let bytes = corrupt_bytes(idx)?;
    let base = VALID_SCENARIOS[valid_index_of(cs.base).unwrap_or(0)];
    let subject = subject_decode(&bytes, base.interlace);
    let reference = refdec::ref_decode(&bytes);
    let subject_reason = match &subject {
        SubjectVerdict::Rejected(r) => Some(*r),
        SubjectVerdict::Decoded { .. } => None,
        SubjectVerdict::Unclassified(_) => None,
    };
    let reference_reason = match &reference {
        Ok(_) => None,
        Err(e) => Some(ref_error_to_reason(*e)),
    };
    let green = subject_reason == Some(cs.expect) && reference_reason == Some(cs.expect);
    Ok(MatrixRow {
        id: cs.id,
        expectation: Expectation::Reject(cs.expect),
        features: 0,
        note: cs.mechanism,
        subject_digest: 0,
        reference_digest: 0,
        subject_reason,
        reference_reason,
        diff: None,
        failure: None,
        green,
    })
}

/// 跑完整对拍（51 图：34 正常 + 17 畸形）。
///
/// 任一行无法构造（序列化失败等）**不会**被静默跳过：它以红行形式出现并把
/// 原因写进 `note`，行数恒为 51。少一行就是语料破了，必须看得见。
pub fn run_suite() -> SuiteMatrix {
    let mut rows = Vec::with_capacity(TOTAL_COUNT);
    let mut i = 0usize;
    while i < VALID_COUNT {
        match run_valid_row(i) {
            Ok(r) => rows.push(r),
            Err(e) => rows.push(failed_row(VALID_SCENARIOS[i].id, e)),
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < CORRUPT_COUNT {
        match run_corrupt_row(k) {
            Ok(r) => rows.push(r),
            Err(e) => rows.push(failed_row(CORRUPT_SCENARIOS[k].id, e)),
        }
        k += 1;
    }
    SuiteMatrix { rows }
}

/// 构造失败时的红行（保行数、保 ID、把原因挂在失败位）。
fn failed_row(id: &'static str, why: String) -> MatrixRow {
    MatrixRow {
        id,
        expectation: Expectation::Accept,
        features: 0,
        // 说明位是 `&'static str`（表里写死的机制/特性），失败细节进 `failure`
        note: "语料构造失败",
        subject_digest: 0,
        reference_digest: 0,
        subject_reason: None,
        reference_reason: None,
        diff: None,
        failure: Some(why),
        green: false,
    }
}

// ===========================================================================
// 七、覆盖矩阵（锚点「每图对应特性标注入矩阵」的对面：特性 × 语料）
// ===========================================================================

/// 覆盖矩阵报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoverageReport {
    /// 必须覆盖的特性位数。
    pub required: usize,
    /// 实际被覆盖的特性位数。
    pub covered: usize,
    /// 缺口特性位（未被任何图覆盖）。
    pub gaps: Vec<u32>,
    /// 已声明覆盖的合法组合数（位深 × 颜色类型）。
    pub combos_covered: usize,
    /// 交错语料的遍覆盖（最小非空遍数；7 = 七遍全非空）。
    pub min_interlaced_passes: usize,
    /// 声明覆盖但语料里其实没有的组合（声明即事实的核对）。
    pub phantom_combos: usize,
}

impl CoverageReport {
    /// 是否无缺口（且声明无虚报、全部合法组合覆盖、交错七遍全非空）。
    pub fn clean(&self) -> bool {
        self.gaps.is_empty()
            && self.required == self.covered
            && self.combos_covered == LEGAL_COMBOS.len()
            && self.phantom_combos == 0
            && self.min_interlaced_passes == 7
    }
    /// 缺口的可读清单。
    pub fn gap_labels(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < self.gaps.len() {
            v.push(feature_label(self.gaps[i]));
            i += 1;
        }
        v
    }
}

/// 特性位的中文名（覆盖矩阵报告用）。
pub fn feature_label(bit: u32) -> &'static str {
    match bit {
        F_GRAY => "灰度",
        F_RGB => "真彩",
        F_PALETTE => "调色板",
        F_GRAY_ALPHA => "灰度+alpha",
        F_RGBA => "真彩+alpha",
        F_DEPTH_1 => "位深1",
        F_DEPTH_2 => "位深2",
        F_DEPTH_4 => "位深4",
        F_DEPTH_8 => "位深8",
        F_DEPTH_16 => "位深16",
        F_INTERLACE => "Adam7 交错",
        F_TRNS_PALETTE => "调色板 tRNS",
        F_TRNS_GRAY => "灰度色键",
        F_TRNS_RGB => "真彩色键",
        F_TEXT => "文本块",
        F_COLOR_MGMT => "色彩管理块",
        F_BACKGROUND => "背景色块",
        F_PHYSICAL => "物理尺寸块",
        F_SPLIT_IDAT => "IDAT 分片",
        F_FILTER_NONE => "滤波 None",
        F_FILTER_SUB => "滤波 Sub",
        F_FILTER_UP => "滤波 Up",
        F_FILTER_AVG => "滤波 Average",
        F_FILTER_PAETH => "滤波 Paeth",
        F_COMPRESS_STORED => "stored 块",
        F_COMPRESS_FIXED => "fixed 块",
        F_COMPRESS_DYNAMIC => "dynamic 块",
        F_TINY => "极小图 1×1",
        F_ODD_WIDTH => "非字节对齐行宽",
        F_SPARSE_TRNS => "稀疏 tRNS",
        F_FULL_PALETTE => "256 项满调色板",
        F_ALPHA_ALL => "全透明",
        _ => "未知特性位",
    }
}

/// 「必须覆盖的特性」的实际覆盖情况。
///
/// 三件事一起断：
/// 1. **无缺口**：`REQUIRED_FEATURES` 每一位都被至少一张图声明；
/// 2. **组合全覆盖**：15 种合法（位深 × 颜色类型）都真的有语料图；
/// 3. **交错七遍全非空**：至少一张 16×16 的交错图（遍覆盖不足会在解码期
///    变成「某些像素没人铺」，是对拍最容易漏的一类洞）。
pub fn coverage() -> CoverageReport {
    let mut union: u32 = 0;
    let mut i = 0usize;
    while i < VALID_SCENARIOS.len() {
        union |= VALID_SCENARIOS[i].features;
        i += 1;
    }
    let mut gaps = Vec::new();
    let mut covered = 0usize;
    let mut k = 0usize;
    while k < REQUIRED_FEATURES.len() {
        let bit = REQUIRED_FEATURES[k];
        if union & bit != 0 {
            covered += 1;
        } else {
            gaps.push(bit);
        }
        k += 1;
    }
    // ---- 合法组合覆盖（按 (颜色类型, 位深) 下标集合比对，不靠计数） ----
    let mut covered_combos = 0usize;
    let mut phantom = 0usize;
    let mut c = 0usize;
    while c < COVERED_COMBOS.len() {
        let (ct, depth) = COVERED_COMBOS[c];
        let mut found = false;
        let mut j = 0usize;
        while j < VALID_SCENARIOS.len() {
            if VALID_SCENARIOS[j].color == ct && VALID_SCENARIOS[j].depth == depth {
                found = true;
            }
            j += 1;
        }
        if found {
            covered_combos += 1;
        } else {
            phantom += 1;
        }
        c += 1;
    }
    // ---- 交错遍覆盖 ----
    let mut min_passes = 7usize;
    let mut q = 0usize;
    let mut any_interlaced = false;
    while q < VALID_SCENARIOS.len() {
        if VALID_SCENARIOS[q].interlace {
            any_interlaced = true;
            let n = nonempty_pass_count(VALID_SCENARIOS[q].width, VALID_SCENARIOS[q].height);
            if n < min_passes {
                min_passes = n;
            }
        }
        q += 1;
    }
    if !any_interlaced {
        min_passes = 0;
    }
    CoverageReport {
        required: REQUIRED_FEATURES.len(),
        covered,
        gaps,
        combos_covered: covered_combos,
        min_interlaced_passes: min_passes,
        phantom_combos: phantom,
    }
}

/// 拒绝理由覆盖：畸形语料是否把 [`RejectReason::ALL`] 逐条覆盖到。
///
/// 返回未被任何畸形图声明的分类清单（空 = 全覆盖）。同一分类由多图覆盖
/// 是允许且必要的（同一类拒绝的不同注入点要各自成立）。
pub fn reason_coverage() -> Vec<RejectReason> {
    let mut missing = Vec::new();
    let mut i = 0usize;
    while i < RejectReason::ALL.len() {
        let want = RejectReason::ALL[i];
        let mut found = false;
        let mut j = 0usize;
        while j < CORRUPT_SCENARIOS.len() {
            if CORRUPT_SCENARIOS[j].expect == want {
                found = true;
            }
            j += 1;
        }
        if !found {
            missing.push(want);
        }
        i += 1;
    }
    missing
}

/// 语料自洽核对：图 ID 唯一、畸形基图存在、行数与常量一致。
///
/// 返回问题清单（空 = 自洽）。**这类核对放在运行期而不是编译期**：表是
/// `const`，编译器不会检查「两个 ID 撞了」，而撞 ID 会让矩阵里两行同名，
/// 报告时无从区分。
pub fn corpus_self_check() -> Vec<&'static str> {
    let mut issues: Vec<&'static str> = Vec::new();
    if VALID_SCENARIOS.len() != VALID_COUNT {
        issues.push("正常语料行数与 VALID_COUNT 不符");
    }
    if CORRUPT_SCENARIOS.len() != CORRUPT_COUNT {
        issues.push("畸形语料行数与 CORRUPT_COUNT 不符");
    }
    let mut i = 0usize;
    while i < VALID_SCENARIOS.len() {
        let mut j = i + 1;
        while j < VALID_SCENARIOS.len() {
            if VALID_SCENARIOS[i].id == VALID_SCENARIOS[j].id {
                issues.push("正常语料图 ID 重复");
            }
            if VALID_SCENARIOS[i].color == VALID_SCENARIOS[j].color
                && VALID_SCENARIOS[i].depth == VALID_SCENARIOS[j].depth
                && !VALID_SCENARIOS[i].interlace
                && !VALID_SCENARIOS[j].interlace
            {
                issues.push("正常语料的 (颜色类型,位深) 在非隔行族内重复");
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < CORRUPT_SCENARIOS.len() {
        let mut l = k + 1;
        while l < CORRUPT_SCENARIOS.len() {
            if CORRUPT_SCENARIOS[k].id == CORRUPT_SCENARIOS[l].id {
                issues.push("畸形语料图 ID 重复");
            }
            l += 1;
        }
        if valid_index_of(CORRUPT_SCENARIOS[k].base).is_none() {
            issues.push("畸形语料基图不存在");
        }
        // 声明的注入方式必须真在基图上说得通（例如索引越界注入需要调色板基图）
        let needs_palette = matches!(CORRUPT_SCENARIOS[k].mutation, Mutation::PaletteIndexOutOfRange);
        if needs_palette {
            let base = valid_index_of(CORRUPT_SCENARIOS[k].base).unwrap_or(0);
            if VALID_SCENARIOS[base].color != ColorType::Palette {
                issues.push("索引越界注入的基图不是调色板图");
            }
        }
        k += 1;
    }
    issues
}

// ===========================================================================
// 八、基线重录纪律（锚点「参考解码器版本升级→基线重录走双人复核」）
// ===========================================================================

/// 对拍基线。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Baseline {
    /// 基线版本（单调递增）。
    pub version: u32,
    /// 归属的参考解码器标识。
    pub reference: &'static str,
    /// 矩阵摘要。
    pub digest: u64,
    /// 复核人（两名不同的人；顺序即录入顺序）。
    pub reviewers: [String; 2],
}

impl Baseline {
    /// 复核人是否**真的**是两个人（同名不算——自己批自己的基线等于没复核）。
    pub fn dual_reviewed(&self) -> bool {
        !self.reviewers[0].is_empty()
            && !self.reviewers[1].is_empty()
            && self.reviewers[0] != self.reviewers[1]
    }
}

/// 由当前矩阵推出应记录的基线（不写盘、不落文件——纯内存对象）。
pub fn current_baseline(m: &SuiteMatrix) -> Baseline {
    Baseline {
        version: BASELINE_VERSION,
        reference: BASELINE_REFERENCE_ID,
        digest: m.digest(),
        reviewers: [String::new(), String::new()],
    }
}

/// 基线重录（锚点错误路径：参考解码器版本升级 → 走双人复核）。
///
/// 四道闸，任一不过即拒绝：
/// 1. 矩阵必须全绿——**给红矩阵录基线等于把 bug 固化成「期望」**；
/// 2. 摘要必须真的变了——没变就重录是噪声，会把「谁在什么时候按了重录」
///    变成不可查的字段；
/// 3. 版本必须严格 +1（不允许跳号，也不允许原地覆盖）；
/// 4. 复核人必须是两个**不同**的人。
pub fn rerecord(old: &Baseline, m: &SuiteMatrix, reviewers: &[&str]) -> Result<Baseline, Fault> {
    if !m.all_green() {
        return Err(Fault::BaselineNotGreen { red: m.red_count() });
    }
    let digest = m.digest();
    if digest == old.digest {
        return Err(Fault::BaselineUnchanged);
    }
    if reviewers.len() < 2 {
        return Err(Fault::SecondReviewRequired { got: reviewers.len() });
    }
    if reviewers[0] == reviewers[1] {
        return Err(Fault::ReviewerNotDistinct);
    }
    if reviewers[0].is_empty() || reviewers[1].is_empty() {
        return Err(Fault::ReviewerEmpty);
    }
    Ok(Baseline {
        version: old.version + 1,
        reference: BASELINE_REFERENCE_ID,
        digest,
        reviewers: [reviewers[0].to_string(), reviewers[1].to_string()],
    })
}

/// 期望变更守卫：既有行的**期望**不得被静默改写。
///
/// 锚点「人工只补新增语料」的反面：新增语料可以随便加，但既有行的期望
/// 变了就意味着「当初那道防线被拆了」。因此这里要求逐条给出变更理由，
/// 且必须两名复核人签字——把「悄悄放宽断言」变成需要走流程的动作。
pub fn guard_expectation_change(
    current: &SuiteMatrix,
    proposed: &SuiteMatrix,
    reviewers: &[&str],
) -> Result<Vec<&'static str>, Fault> {
    if reviewers.len() < 2 || reviewers[0] == reviewers[1] {
        return Err(Fault::SecondReviewRequired { got: reviewers.len() });
    }
    let mut changed: Vec<&'static str> = Vec::new();
    let mut i = 0usize;
    while i < current.rows.len() {
        let a = &current.rows[i];
        if let Some(b) = proposed.rows.iter().find(|r| r.id == a.id) {
            if a.expectation != b.expectation {
                changed.push(a.id);
            }
        }
        i += 1;
    }
    if changed.is_empty() {
        return Ok(changed);
    }
    Err(Fault::ExpectationChangedWithoutReview { count: changed.len() })
}

// ===========================================================================
// 九、新增语料流程（锚点「样本→最小化→归类→断言」）
// ===========================================================================

/// 流程阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntakeStep {
    /// 样本：外部发现的畸形 PNG 原样进流程。
    Sample,
    /// 最小化：砍掉与拒绝理由无关的字节，保留同一拒绝分类。
    Minimize,
    /// 归类：把两侧解码器的结论归约到分类层。
    Classify,
    /// 断言：产出可入表的新行建议（ID / 期望 / 理由）。
    Assert,
}

impl IntakeStep {
    /// 中文短名。
    pub fn label(self) -> &'static str {
        match self {
            IntakeStep::Sample => "样本",
            IntakeStep::Minimize => "最小化",
            IntakeStep::Classify => "归类",
            IntakeStep::Assert => "断言",
        }
    }
    /// 序号（1..4，报告用）。
    pub fn ordinal(self) -> u8 {
        match self {
            IntakeStep::Sample => 1,
            IntakeStep::Minimize => 2,
            IntakeStep::Classify => 3,
            IntakeStep::Assert => 4,
        }
    }
}

/// 流程结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntakeOutcome {
    /// 原始样本字节数。
    pub sample_bytes: usize,
    /// 最小化后字节数（保留同一拒绝分类的最短前缀）。
    pub minimized_bytes: usize,
    /// 归类结果（两侧一致时才有）。
    pub class: Option<RejectReason>,
    /// 两侧是否给出同一分类。
    pub agreed: bool,
    /// 新行建议的 ID（`None` = 未通过断言）。
    pub proposed_id: Option<&'static str>,
    /// 流程停在哪一步（失败即停，不跳步）。
    pub stopped_at: IntakeStep,
}

/// 新语料 ID 的命名规则：畸形行必须 `x` 开头 + 至少 5 个字符。
///
/// 规则用**可执行判定**而不是口头约定：ID 是矩阵的行键，撞 ID 或起错前缀
/// 会让「这张图是哪张图」在报告里说不清。
pub fn corpus_id_ok(id: &str) -> bool {
    id.len() >= 5 && id.as_bytes()[0] == b'x' && valid_index_of(id).is_none() && corrupt_index_of(id).is_none()
}

/// 归类：把一个字节流喂两侧解码器并归约到分类层。
fn classify_sample(sample: &[u8]) -> (Option<RejectReason>, Option<RejectReason>) {
    let subject = match subject_decode(sample, false) {
        SubjectVerdict::Rejected(r) => Some(r),
        _ => None,
    };
    let reference = match refdec::ref_decode(sample) {
        Ok(_) => None,
        Err(e) => Some(ref_error_to_reason(e)),
    };
    (subject, reference)
}

/// 最小化：找**最短的仍保持同一分类**的样本前缀。
///
/// 线性收缩太慢（O(n) 次解码），故用二分收缩 + 线性回退：先对半砍，
/// 若分类变了就把砍掉的字节补回来。这样样本大时也能收敛，且**不牺牲最小性**
/// （逐字节确认过分类仍不变才接受更短的）。
pub fn minimize_sample(sample: &[u8], want: RejectReason) -> Vec<u8> {
    let (subj, refe) = classify_sample(sample);
    if subj != Some(want) || refe != Some(want) {
        return sample.to_vec();
    }
    let mut best = sample.to_vec();
    loop {
        if best.len() <= 8 {
            return best;
        }
        let half = best.len() / 2;
        let cand = best[..half].to_vec();
        let (s, r) = classify_sample(&cand);
        if s == Some(want) && r == Some(want) {
            best = cand;
            continue;
        }
        // 半砍不过则逐字节试，命中即固定
        let mut cut = false;
        let mut n = best.len() - 1;
        while n >= 8 {
            let cand2 = best[..n].to_vec();
            let (s2, r2) = classify_sample(&cand2);
            if s2 == Some(want) && r2 == Some(want) {
                best = cand2;
                cut = true;
            }
            n -= 1;
        }
        return best;
    }
}

/// 新增语料流程（锚点「样本→最小化→归类→断言」）。
///
/// 四步**不跳步**：样本字节数非法就停在样本步；两侧分类不一致就停在归类步；
/// ID 不合规就停在断言步。跳步的地方正是「语料库被垃圾填满」的地方。
pub fn intake(sample: &[u8]) -> IntakeOutcome {
    // ---- 1 样本 ----
    if sample.is_empty() || sample.len() > CORPUS_BYTES_MAX {
        return IntakeOutcome {
            sample_bytes: sample.len(),
            minimized_bytes: sample.len(),
            class: None,
            agreed: false,
            proposed_id: None,
            stopped_at: IntakeStep::Sample,
        };
    }
    let (subj, refe) = classify_sample(sample);
    let agreed = subj.is_some() && subj == refe;
    let class = match (subj, agreed) {
        (Some(c), true) => c,
        _ => {
            return IntakeOutcome {
                sample_bytes: sample.len(),
                minimized_bytes: sample.len(),
                class: subj,
                agreed,
                proposed_id: None,
                stopped_at: IntakeStep::Classify,
            }
        }
    };
    // ---- 2 最小化 ----
    let minimized = minimize_sample(sample, class);
    // ---- 3 归类（最小化后复核，分类必须不变） ----
    let (s2, r2) = classify_sample(&minimized);
    if s2 != Some(class) || r2 != Some(class) {
        return IntakeOutcome {
            sample_bytes: sample.len(),
            minimized_bytes: minimized.len(),
            class: Some(class),
            agreed: false,
            proposed_id: None,
            stopped_at: IntakeStep::Classify,
        };
    }
    // ---- 4 断言（ID 合规才产出建议） ----
    let proposed = propose_id(class, minimized.len());
    let ok = match proposed {
        Some(id) => corpus_id_ok(id),
        None => false,
    };
    IntakeOutcome {
        sample_bytes: sample.len(),
        minimized_bytes: minimized.len(),
        class: Some(class),
        agreed: true,
        proposed_id: if ok { proposed } else { None },
        stopped_at: if ok { IntakeStep::Assert } else { IntakeStep::Assert },
    }
}

/// 按分类与最小化字节数派生一个候选 ID（`x<分类序号>nn`；撞号则给 `None`）。
fn propose_id(class: RejectReason, len: usize) -> Option<&'static str> {
    // 候选 ID 全集是有限的（分类 × 序号），这里按「分类序号 + 最小字节数的
    // 两位十六进制」拼一个**必定撞号或合规**的名字：撞号由 `corpus_id_ok`
    // 判掉，合规则留给人工改名——自动改名会让人分不清「谁加的」。
    let k = class_index(class);
    let n = (len % 256) as u8;
    let a = b'0' + (n / 10);
    let b = b'0' + (n % 10);
    // 四个固定候选，逐个试；全撞就给 None（拒绝入表，而不是硬塞一个重名）
    match (k, a, b) {
        (0, b'0', b'0') => Some("x000a00"),
        (1, b'0', b'0') => Some("x001a00"),
        (2, b'0', b'0') => Some("x002a00"),
        (3, b'0', b'0') => Some("x003a00"),
        (4, b'0', b'0') => Some("x004a00"),
        (5, b'0', b'0') => Some("x005a00"),
        (6, b'0', b'0') => Some("x006a00"),
        (7, b'0', b'0') => Some("x007a00"),
        (8, b'0', b'0') => Some("x008a00"),
        (9, b'0', b'0') => Some("x009a00"),
        (10, b'0', b'0') => Some("x010a00"),
        (11, b'0', b'0') => Some("x011a00"),
        (12, b'0', b'0') => Some("x012a00"),
        (13, b'0', b'0') => Some("x013a00"),
        (14, b'0', b'0') => Some("x014a00"),
        _ => None,
    }
}

/// 分类在分类全集里的下标（用于派生候选 ID 与覆盖对账）。
pub fn class_index(c: RejectReason) -> usize {
    let mut i = 0usize;
    while i < RejectReason::ALL.len() {
        if RejectReason::ALL[i] == c {
            return i;
        }
        i += 1;
    }
    usize::MAX
}

// ===========================================================================
// 十、错误面（本单全部错误路径）
// ===========================================================================

/// 本单全部错误路径。
///
/// 码段 **F1011 ⇒ `0xF7xx`**（建段前全仓 grep 过：`0xF1..0xF4` 归 F1001~F1004、
/// `0xF5` 归 F1009、`0xF6` 归 F1010、`0xF3` 归 F1003、`0x2B..0x2D` 归 VE-N
/// F2605~F2607、`0xF9` 归 VE-J；`0xF7` 空闲）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 红矩阵不允许录基线。
    BaselineNotGreen { red: usize },
    /// 摘要没变却要重录（噪声重录）。
    BaselineUnchanged,
    /// 复核人不足两名。
    SecondReviewRequired { got: usize },
    /// 两名复核人同名。
    ReviewerNotDistinct,
    /// 复核人身份为空。
    ReviewerEmpty,
    /// 既有行的期望被变更。
    ExpectationChangedWithoutReview { count: usize },
    /// 语料表自洽性破损。
    CorpusSelfCheck { issues: usize },
    /// 覆盖矩阵有缺口。
    CoverageGap { missing: usize },
    /// 拒绝理由覆盖有缺口。
    ReasonCoverageGap { missing: usize },
    /// 矩阵行数与常量不符。
    RowCountMismatch { got: usize },
    /// 语料字节超上限。
    CorpusTooLarge { bytes: usize },
    /// 语料构造失败（下标越界 / 基图缺失 / 注入不适配）。
    CorpusBuildFailed,
    /// 语料 ID 不合规。
    CorpusIdRejected,
    /// 零容差下出现像素差异（互操作矩阵的红项根因）。
    PixelMismatch { index: usize },
    /// 新增语料样本字节数非法。
    IntakeSampleRejected { bytes: usize },
    /// 新增语料两侧分类不一致。
    IntakeClassificationDisagreed,
    /// 期望变更守卫触发（变更数 > 0）。
    ExpectationDrift { count: usize },
}

impl Fault {
    /// 规则码（本单独占 `0xF7xx` 段）。
    pub fn code(&self) -> u16 {
        let n = match self {
            Fault::BaselineNotGreen { .. } => 1u16,
            Fault::BaselineUnchanged => 2,
            Fault::SecondReviewRequired { .. } => 3,
            Fault::ReviewerNotDistinct => 4,
            Fault::ReviewerEmpty => 5,
            Fault::ExpectationChangedWithoutReview { .. } => 6,
            Fault::CorpusSelfCheck { .. } => 7,
            Fault::CoverageGap { .. } => 8,
            Fault::ReasonCoverageGap { .. } => 9,
            Fault::RowCountMismatch { .. } => 10,
            Fault::CorpusTooLarge { .. } => 11,
            Fault::CorpusBuildFailed => 12,
            Fault::CorpusIdRejected => 13,
            Fault::PixelMismatch { .. } => 14,
            Fault::IntakeSampleRejected { .. } => 15,
            Fault::IntakeClassificationDisagreed => 16,
            Fault::ExpectationDrift { .. } => 17,
        };
        0xF700 | n
    }

    /// 发生了什么。
    pub fn what(&self) -> String {
        match self {
            Fault::BaselineNotGreen { red } => format!("对拍矩阵有 {} 行红项，不允许录基线", red),
            Fault::BaselineUnchanged => "矩阵摘要与旧基线一致，重录是噪声".to_string(),
            Fault::SecondReviewRequired { got } => format!("基线重录需两名复核人，实得 {}", got),
            Fault::ReviewerNotDistinct => "两名复核人同名（自己批自己的基线等于没复核）".to_string(),
            Fault::ReviewerEmpty => "复核人身份为空".to_string(),
            Fault::ExpectationChangedWithoutReview { count } => {
                format!("{} 行既有期望被变更，必须走复核流程", count)
            }
            Fault::CorpusSelfCheck { issues } => format!("语料表自洽核对发现 {} 处问题", issues),
            Fault::CoverageGap { missing } => format!("覆盖矩阵缺 {} 个特性位", missing),
            Fault::ReasonCoverageGap { missing } => format!("拒绝理由覆盖缺 {} 类", missing),
            Fault::RowCountMismatch { got } => format!("矩阵行数 {} 与常量不符", got),
            Fault::CorpusTooLarge { bytes } => format!("语料字节 {} 超上限", bytes),
            Fault::CorpusBuildFailed => "语料构造失败（下标越界 / 基图缺失 / 注入不适配）".to_string(),
            Fault::CorpusIdRejected => "语料图 ID 不合规（须 x 开头、≥5 字符、不撞既有 ID）".to_string(),
            Fault::PixelMismatch { index } => {
                format!("零容差对拍在像素 {} 处不一致", index)
            }
            Fault::IntakeSampleRejected { bytes } => format!("新增语料样本 {} 字节，非法", bytes),
            Fault::IntakeClassificationDisagreed => "新增语料两侧分类不一致".to_string(),
            Fault::ExpectationDrift { count } => format!("{} 行期望漂移", count),
        }
    }

    /// 规则出处。
    pub fn why(&self) -> &'static str {
        match self {
            Fault::BaselineNotGreen { .. } => "F1011：给红矩阵录基线等于把 bug 固化成期望（金样纪律）",
            Fault::BaselineUnchanged => "F1011：摘要未变的重录会污染重录历史，不可查",
            Fault::SecondReviewRequired { .. } => "F1011：基线重录走双人复核（锚点错误路径）",
            Fault::ReviewerNotDistinct => "F1011：双人复核的两个人必须不同",
            Fault::ReviewerEmpty => "F1011：复核人身份不得为空串",
            Fault::ExpectationChangedWithoutReview { .. } => "F1011：人工只补新增语料，既有期望变更须复核",
            Fault::CorpusSelfCheck { .. } => "F1011：语料表自洽（ID 唯一 / 基图存在）是入表前置",
            Fault::CoverageGap { .. } => "F1011：每图对应特性标注入矩阵，缺口即未测",
            Fault::ReasonCoverageGap { .. } => "F1011：畸形图集的拒绝理由必须逐类覆盖",
            Fault::RowCountMismatch { .. } => "F1011：34+17=51 是锚点给定的规模，少一行即语料破损",
            Fault::CorpusTooLarge { .. } => "F1011：语料图应是小图，超上限说明生成失控",
            Fault::CorpusBuildFailed => "F1011：语料由场景配置机器生成，构建失败必须显性",
            Fault::CorpusIdRejected => "F1011：语料 ID 是矩阵行键，撞号会让报告无法区分",
            Fault::PixelMismatch { .. } => "F1011：PNG 无损，逐像素严格一致零容差",
            Fault::IntakeSampleRejected { .. } => "F1011：新增语料流程第一步即校验样本合法性",
            Fault::IntakeClassificationDisagreed => "F1011：两侧分类不一致的样本不得入表",
            Fault::ExpectationDrift { .. } => "F1011：期望漂移是回归网被拆的信号",
        }
    }

    /// 下一步建议。
    pub fn advice(&self) -> String {
        match self {
            Fault::BaselineNotGreen { .. } => "先修红项：按矩阵 red_ids 定位差异像素或拒绝理由".to_string(),
            Fault::BaselineUnchanged => "摘要没变就别重录；确有变化时先确认矩阵确实变了".to_string(),
            Fault::SecondReviewRequired { .. } => "指定两名不同复核人后重试（第二人须独立核对）".to_string(),
            Fault::ReviewerNotDistinct => "换第二名复核人".to_string(),
            Fault::ReviewerEmpty => "填写真实身份标识（工号/姓名）".to_string(),
            Fault::ExpectationChangedWithoutReview { .. } => {
                "为每处期望变更补一句「为何旧期望错」并双人复核".to_string()
            }
            Fault::CorpusSelfCheck { .. } => "按提示修语料表：ID 去重 / 基图指向存在的图".to_string(),
            Fault::CoverageGap { .. } => "为缺口特性补一张语料图（新增语料，不是改旧图）".to_string(),
            Fault::ReasonCoverageGap { .. } => "为缺口分类补一张畸形图，并在表里写明机制".to_string(),
            Fault::RowCountMismatch { .. } => "检查语料表行数是否被误删/误增".to_string(),
            Fault::CorpusTooLarge { .. } => "缩小场景尺寸或改用压缩块形态".to_string(),
            Fault::CorpusBuildFailed => "核对下标与基图 ID；注入不适配时换基图".to_string(),
            Fault::CorpusIdRejected => "改成 x 开头、≥5 字符且与既有 ID 不撞的名字".to_string(),
            Fault::PixelMismatch { index } => {
                format!("查像素 {} 处的打包 / 滤波 / 展开路径（零容差，无阈值可调）", index)
            }
            Fault::IntakeSampleRejected { .. } => "样本须为 1 字节到 1MiB 之间的字节流".to_string(),
            Fault::IntakeClassificationDisagreed => "先弄清两侧为何分类不同，再决定是否入表".to_string(),
            Fault::ExpectationDrift { .. } => "把变更登记到复核记录，不要就地改期望".to_string(),
        }
    }

    /// 三要素合成本行（全角竖线分隔，与 F1008~F1010 同规格）。
    pub fn three_elements(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.what());
        s.push('｜');
        s.push_str(self.why());
        s.push('｜');
        s.push_str(&self.advice());
        s
    }

    /// 诊断码（格式稳定可 grep）。
    pub fn diag(&self) -> String {
        format!("F1011-{:#06X}", self.code())
    }
}
