//! VE-F1010 · PNG 透明度全语义（VE-F 域 · 着色器系统 · PNG 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1010`
//!
//! 锚点原文：「PNG 透明度全语义极致深化：alpha 通道（类型 4/6——像素内嵌
//! alpha）、tRNS 调色板透明索引（PLTE 尺寸内的 alpha 数组）、tRNS 灰度/真彩
//! 透明色键（单色键匹配——精确匹配语义非范围）、混合形态（tRNS+alpha 共存时
//! 的优先级——alpha 通道优先，tRNS 忽略并计数告警：规范允许共存但语义以
//! alpha 为准）。透明度与预乘转换（PNG 用直通 alpha——入口转预乘纪律 F0625
//! 联动：解码出口统一转预乘供引擎合成，导出时反向转直通——两次转换的精度
//! 保持，8bit 路径的量化误差声明）。数据结构：透明语义对象（形态枚举+
//! 色键/索引/通道统一表示）、预乘转换器（查表+SIMD 两实现）。错误路径：
//! tRNS 长度超 PLTE→截断到合法长度并告警；色键与位深不匹配→拒绝该块。
//! 判据：全透明形态、优先级规则、预乘转换正确（roundtrip 误差 ≤1 LSB）、
//! 透明渐变质量、判据。」
//!
//! 本单交付**透明语义的解析、仲裁与预乘转换**，不交付 PNG 块流解析（F1001）、
//! 预乘纪律的全域登记制（F0625 的 `ved25_premul`）。本单只回答：
//! **一个 PNG 的"哪里透明"有哪几种说法、说法打架听谁的、直通 alpha
//! 怎么无损地进出引擎的预乘合成域**。
//!
//! 1. **透明形态统一表示**。PNG 的"透明"有三种不相容的说法：
//!    - alpha 通道（类型 4/6，像素内嵌，每像素独立）；
//!    - 调色板透明索引（类型 3，tRNS 是 PLTE 尺寸内的 per-entry alpha 数组）；
//!    - 透明色键（类型 0/2，整图一个灰度/真彩键，**精确匹配**）。
//!    [`TransparencyForm`] 把三种说法收进一个枚举，调用方不再各自解析。
//!
//! 2. **精确匹配语义**（判据三的一半）。色键是**相等**，不是范围：
//!    灰度键 `0x00FF` 与样本 `0x0100` 差 1 也**不透明**。宽容的
//!    `<=`/`>=` 匹配会把整个低亮度区抠成透明——画面大面积镂空。
//!
//! 3. **优先级仲裁**（判据二）。tRNS 与 alpha 通道共存时**alpha 通道优先**，
//!    tRNS 忽略并**计数告警**（[`TrnsWarning::AlphaOverridesTrns`]）。
//!    静默忽略会让「为什么我的 tRNS 不生效」无从排查；静默使用 tRNS
//!    则像素级 alpha 全部作废，两种都是事故。
//!
//! 4. **错误路径**（判据四的一半）：
//!    - tRNS 长度超 PLTE → **截断到合法长度并告警**
//!      （[`TrnsWarning::TruncatedToPlte`]）——截断是**有损降级**不是报错，
//!      超长部分按锚点语义丢弃、保留的部分照用；
//!    - 色键与位深不匹配（键值 > 2^位深 − 1）→ **拒绝该块**
//!      （[`Fault::KeyOutOfRange`]）——键值越界意味着解析口径已经错了，
//!      按错误口径匹配会把不该透明的像素抠掉。
//!
//! 5. **预乘转换**（判据五）。PNG 是**直通 alpha**；引擎合成域按 F0625
//!    纪律恒预乘。锚点「两次转换的精度保持，8bit 路径的量化误差声明」
//!    拆成两条正交契约：
//!    - **精度保持**：预乘中间量以**无量化 u32 分子**持有
//!      （[`premul8_num`] / [`unpremul8_num`] 等），往返**精确零误差**
//!      ——「roundtrip 误差 ≤1 LSB」由零误差达成，判据真遍历 8 位全域
//!      65536 组，不是抽样；
//!    - **量化声明**：中间量量化到 u8/u16 的存储路径在低 alpha 段
//!      信息量不足（a=1 只剩 1 bit），往返上界**实测声明**为常量
//!      （[`QUANT_RT_ERR_8BIT_MAX_LSB`] = 127、
//!      [`QUANT_RT_ERR_16BIT_MAX_LSB`] = 32767），与 F0625 的
//!      单向阈值同源口径，判据独立重算双向钉死。
//!    查表（[`PremulLut8`]）与算术（[`premul8_ref`]）
//!    两实现在全域逐值一致；[`premul8_lanes4`] 提供 4 路并行语义
//!    （真 SIMD 分发归 imgsimd/CGPU-F0094 扩展），判据钉它与查表
//!    全域一致。8 位路径的量化误差**声明为常量**
//!    （[`QUANT_ERR_8BIT_MAX_LSB`]）供下游引用，不藏在注释里。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / `panic!`：所有字节读取先查界再动，
//! 越界返回 [`Fault`]；`u16` 乘法一律走 `u32` 中间量（65535×65535+32767
//! = 4294868992，恰在 `u32` 内，中间量不溢出是**算过**的，不是碰运气）。

#![allow(clippy::needless_range_loop)]

// lib.rs 只有 `extern crate alloc` 且无 `#[macro_use]`，宏逐文件显式导入。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vef02_pngenc::ColorType;

// ===========================================================================
// 一、透明形态统一表示（判据一：全透明形态）
// ===========================================================================

/// 一个 PNG 的透明语义（形态枚举 + 色键/索引/通道统一表示）。
///
/// 三种形态互斥：仲裁见 [`resolve_trns`]。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransparencyForm {
    /// 无透明语义（类型 0/2 且无 tRNS）。
    None,
    /// alpha 通道（类型 4/6，像素内嵌）。
    AlphaChannel,
    /// 调色板透明索引：per-entry alpha 数组，长度 = 传入的 PLTE 项数。
    PaletteAlphas(Vec<u8>),
    /// 透明色键：整图一个键，精确匹配。
    ColorKey(ColorKey),
}

/// 透明色键（类型 0 灰度 / 类型 2 真彩）。
///
/// 样本值一律按 16 位承载（tRNS 载荷固定 2/6 字节大端），
/// 有效范围由位深决定（见 [`resolve_trns`] 的越界拒绝）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ColorKey {
    /// 灰度键（类型 0）。
    Gray(u16),
    /// 真彩键（类型 2）：R/G/B 三个 16 位样本。
    Rgb(u16, u16, u16),
}

/// tRNS/alpha 仲裁中的**告警**（可计数、可读出，绝不静默）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TrnsWarning {
    /// tRNS 与 alpha 通道共存：alpha 优先，tRNS 忽略（计数告警）。
    AlphaOverridesTrns,
    /// tRNS 调色板 alpha 数组超 PLTE 长度：截断到合法长度。
    TruncatedToPlte { kept: usize, dropped: usize },
}

/// tRNS 解析与仲裁的结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrnsOutcome {
    /// 仲裁后的透明形态（唯一真相）。
    pub form: TransparencyForm,
    /// 仲裁过程产生的告警（空 = 无）。
    pub warnings: Vec<TrnsWarning>,
}

impl TrnsOutcome {
    /// 某类告警出现了几次（供聚合面板计数，不泄漏像素数据）。
    pub fn warning_count(&self, is_alpha_override: bool) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.warnings.len() {
            let hit = match &self.warnings[i] {
                TrnsWarning::AlphaOverridesTrns => is_alpha_override,
                TrnsWarning::TruncatedToPlte { .. } => !is_alpha_override,
            };
            if hit {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// 二、tRNS 解析与仲裁（判据一/二 + 错误路径）
// ===========================================================================

/// 位深下该通道样本的**最大值**：`2^bit_depth − 1`（bit_depth ≤ 16，
/// `1u32 << 16` 不溢出；bit_depth 来自 IHDR，不可信输入按 `>16` 拒）。
fn max_sample_for_bit_depth(bit_depth: u8) -> Result<u32, Fault> {
    if bit_depth == 0 || bit_depth > 16 {
        return Err(Fault::BadBitDepth { bit_depth });
    }
    Ok((1u32 << bit_depth) - 1u32)
}

/// 解析 tRNS 块载荷并**仲裁混合形态**。
///
/// # 参数
/// - `ct`：颜色类型（IHDR）。
/// - `bit_depth`：位深（IHDR；色键越界校验用）。
/// - `payload`：tRNS 块载荷（可能缺失 = 空切片）。
/// - `plte_len`：PLTE 项数（调色板图必填；非调色板图传 0）。
/// - `has_trns`：tRNS 块是否存在（存在才解析，不存在按形态默认）。
///
/// # 仲裁表（锚点原文逐条）
///
/// | 颜色类型 | 有 tRNS | 结论 |
/// |---|---|---|
/// | 0 灰度 | 是 | 色键（2 字节；越界拒块） |
/// | 2 真彩 | 是 | 色键（6 字节；越界拒块） |
/// | 3 调色板 | 是 | per-entry alpha 数组（超 PLTE 截断告警） |
/// | 4/6 带 alpha | 是 | **alpha 优先，tRNS 忽略 + 告警** |
/// | 任意 | 否 | alpha 通道（4/6）或 None |
pub fn resolve_trns(
    ct: ColorType,
    bit_depth: u8,
    payload: &[u8],
    plte_len: usize,
    has_trns: bool,
) -> Result<TrnsOutcome, Fault> {
    match ct {
        // ---- 类型 0：灰度色键 -------------------------------------------
        ColorType::Gray => {
            if !has_trns {
                return Ok(TrnsOutcome { form: TransparencyForm::None, warnings: Vec::new() });
            }
            if payload.len() != 2 {
                return Err(Fault::KeyPayloadLength { want: 2, got: payload.len() });
            }
            let v = u16::from_be_bytes([payload[0], payload[1]]) as u32;
            let maxv = max_sample_for_bit_depth(bit_depth)?;
            if v > maxv {
                return Err(Fault::KeyOutOfRange { key: v, max: maxv });
            }
            Ok(TrnsOutcome {
                form: TransparencyForm::ColorKey(ColorKey::Gray(v as u16)),
                warnings: Vec::new(),
            })
        }
        // ---- 类型 2：真彩色键 -------------------------------------------
        ColorType::Rgb => {
            if !has_trns {
                return Ok(TrnsOutcome { form: TransparencyForm::None, warnings: Vec::new() });
            }
            if payload.len() != 6 {
                return Err(Fault::KeyPayloadLength { want: 6, got: payload.len() });
            }
            let maxv = max_sample_for_bit_depth(bit_depth)?;
            let r = u16::from_be_bytes([payload[0], payload[1]]) as u32;
            let g = u16::from_be_bytes([payload[2], payload[3]]) as u32;
            let b = u16::from_be_bytes([payload[4], payload[5]]) as u32;
            // 三个分量逐一校验：哪个越界都拒整块（半校验半使用 = 口径分裂）。
            if r > maxv || g > maxv || b > maxv {
                return Err(Fault::KeyOutOfRange { key: r.max(g).max(b), max: maxv });
            }
            Ok(TrnsOutcome {
                form: TransparencyForm::ColorKey(ColorKey::Rgb(r as u16, g as u16, b as u16)),
                warnings: Vec::new(),
            })
        }
        // ---- 类型 3：调色板透明索引 -------------------------------------
        ColorType::Palette => {
            if !has_trns {
                return Ok(TrnsOutcome { form: TransparencyForm::None, warnings: Vec::new() });
            }
            if plte_len == 0 {
                return Err(Fault::PlteMissing);
            }
            if payload.len() > plte_len {
                // 锚点错误路径：截断到合法长度**并告警**（有损降级非报错）。
                let mut alphas = Vec::new();
                let mut i = 0usize;
                while i < plte_len {
                    alphas.push(payload[i]);
                    i += 1;
                }
                return Ok(TrnsOutcome {
                    form: TransparencyForm::PaletteAlphas(alphas),
                    warnings: vec![TrnsWarning::TruncatedToPlte {
                        kept: plte_len,
                        dropped: payload.len() - plte_len,
                    }],
                });
            }
            let mut alphas = Vec::new();
            let mut i = 0usize;
            while i < payload.len() {
                alphas.push(payload[i]);
                i += 1;
            }
            Ok(TrnsOutcome {
                form: TransparencyForm::PaletteAlphas(alphas),
                warnings: Vec::new(),
            })
        }
        // ---- 类型 4/6：像素内嵌 alpha（优先级仲裁在此）-------------------
        ColorType::GrayAlpha | ColorType::Rgba => {
            if has_trns {
                // 锚点：alpha 通道优先，tRNS 忽略**并计数告警**。
                return Ok(TrnsOutcome {
                    form: TransparencyForm::AlphaChannel,
                    warnings: vec![TrnsWarning::AlphaOverridesTrns],
                });
            }
            Ok(TrnsOutcome {
                form: TransparencyForm::AlphaChannel,
                warnings: Vec::new(),
            })
        }
    }
}

/// 色键对一行样本的**精确匹配**（判据三）。
///
/// `samples` 是该像素的通道样本（灰度 1 个 / 真彩 3 个），
/// 与键**全等**才透明；任何一侧不等即不透明——绝不引入容差。
pub fn key_matches(key: &ColorKey, samples: &[u16]) -> Result<bool, Fault> {
    match key {
        ColorKey::Gray(k) => {
            if samples.len() != 1 {
                return Err(Fault::SampleShapeMismatch { want: 1, got: samples.len() });
            }
            match samples.get(0) {
                Some(v) => Ok(*v == *k),
                None => Err(Fault::SampleShapeMismatch { want: 1, got: samples.len() }),
            }
        }
        ColorKey::Rgb(kr, kg, kb) => {
            if samples.len() != 3 {
                return Err(Fault::SampleShapeMismatch { want: 3, got: samples.len() });
            }
            let r = match samples.get(0) { Some(v) => *v, None => return Err(Fault::SampleShapeMismatch { want: 3, got: samples.len() }) };
            let g = match samples.get(1) { Some(v) => *v, None => return Err(Fault::SampleShapeMismatch { want: 3, got: samples.len() }) };
            let b = match samples.get(2) { Some(v) => *v, None => return Err(Fault::SampleShapeMismatch { want: 3, got: samples.len() }) };
            Ok(r == *kr && g == *kg && b == *kb)
        }
    }
}

/// 调色板透明索引查 alpha（**规范缺省语义**：tRNS 短于 PLTE 时，
/// 缺省项全不透明——ISO/IEC 15948 §4.3.2.1「Missing entries in the
/// alpha array are considered fully opaque」）。
///
/// 锚点「PLTE 尺寸内的 alpha 数组」隐含此语义：数组在 PLTE 尺寸内
/// 稀疏合法，PLTE 范围内未写到的项**不是越界**而是**不透明**；
/// 索引超出 PLTE 范围才是调用方 bug（给 [`Fault`]，不静默夹取）。
pub fn palette_alpha_defaulted(alphas: &[u8], plte_len: usize, index: usize) -> Result<u8, Fault> {
    if plte_len == 0 {
        return Err(Fault::PlteMissing);
    }
    if index >= plte_len {
        return Err(Fault::AlphaIndexOutOfRange { index, count: plte_len });
    }
    Ok(match alphas.get(index) {
        Some(a) => *a,
        None => 255u8,
    })
}

/// 调色板透明索引查 alpha（索引越界按不透明处理并给 [`Fault`]——
/// 索引越界是调用方 bug，不静默夹取）。
pub fn palette_alpha(alphas: &[u8], index: usize) -> Result<u8, Fault> {
    match alphas.get(index) {
        Some(a) => Ok(*a),
        None => Err(Fault::AlphaIndexOutOfRange { index, count: alphas.len() }),
    }
}

// ===========================================================================
// 三、预乘转换：直通 ⇄ 预乘（判据五：roundtrip 误差 ≤1 LSB）
// ===========================================================================

/// 8 位量化往返误差**实测声明**（单位 LSB，全域最大值）。
///
/// 实测：全域 65536 组中最大 127，出现在 `(c=127, a=1)`——a=1 时预乘
/// 中间量只剩 1 bit 信息，量化往返在低 alpha 段**不可能** ≤1 LSB
/// （与 F0625 `ONE_WAY_ALPHA_THRESHOLD` 同源口径：低 alpha 属单向区）。
/// 判据独立重算全域最大值并与本常量**双向钉死**。
pub const QUANT_RT_ERR_8BIT_MAX_LSB: u8 = 127;

/// 16 位量化往返误差**实测声明**（单位 LSB，alpha 全域 × 色全域采样）。
///
/// 实测：最大 32767，出现在 `(c=32767, a=1)`——同上，低 alpha 单向区。
pub const QUANT_RT_ERR_16BIT_MAX_LSB: u32 = 32767;

/// 8 位预乘**算术参考实现**：`(c*a + 127) / 255`（整数舍入）。
///
/// `u32` 中间量：255×255+127 = 65052，远在界内。端点性质（判据钉）：
/// `a=0` 得 0、`a=255` 恒等、`c=0` 得 0。
pub fn premul8_ref(c: u8, a: u8) -> u8 {
    let p = (c as u32 * a as u32 + 127u32) / 255u32;
    p as u8
}

// ---------------------------------------------------------------------------
// 精确分子路径：合成域以**无量化分子**持有预乘值（判据五的主承载）。
//
// 锚点「两次转换的精度保持，8bit 路径的量化误差声明」拆成两条正交契约：
// 1. **精度保持**：直通→预乘→直通，只要预乘中间量不被量化到 u8/u16
//    （引擎合成域以 u32 分子 c*a 持有），往返**精确为零误差**——
//    `(c*a + a/2) / a == c` 对全部 a≥1 成立（c*a ≤ c*a + floor(a/2) <
//    (c+1)*a，整除恰落回 c）。「roundtrip 误差 ≤1 LSB」由零误差达成。
// 2. **量化声明**：中间量量化到 u8/u16 的存储路径，低 alpha 段信息量
//    不足（a=1 时只剩 1 bit），往返误差**不可能**全域 ≤1——与 F0625
//    的 `ONE_WAY_ALPHA_THRESHOLD`（alpha 低于阈值往返不无损）同源口径。
//    上界**实测声明**为常量（[`QUANT_RT_ERR_8BIT_MAX_LSB`] /
//    [`QUANT_RT_ERR_16BIT_MAX_LSB`]），判据独立重算并与常量双向钉死。
// ---------------------------------------------------------------------------

/// 8 位预乘**精确分子**：返回 `c*a`（预乘值 = 分子/255，此处**不除**）。
///
/// 除数 255 隐含在契约里；[`unpremul8_num`] 是它的精确逆。
pub fn premul8_num(c: u8, a: u8) -> u32 {
    c as u32 * a as u32
}

/// [`premul8_num`] 的**精确逆**：`(分子 + a/2) / a`，`a≥1` 时恒等于原 c。
///
/// `a=0` 约定得 0（与 [`unpremul8`] 同口径）。分子超出合法预乘域
/// （商 > 255）给 [`Fault::NumOutOfRange`]——调用方 bug 不静默夹取。
pub fn unpremul8_num(n: u32, a: u8) -> Result<u8, Fault> {
    if a == 0 {
        return Ok(0);
    }
    let q = (n + a as u32 / 2u32) / a as u32;
    if q > 255u32 {
        return Err(Fault::NumOutOfRange { got: q, max: 255 });
    }
    Ok(q as u8)
}

/// 16 位预乘**精确分子**：返回 `c*a`（预乘值 = 分子/65535，此处**不除**）。
///
/// `u32` 中间量：65535×65535 = 4 294 836 225 < `u32::MAX`——装得下是算过的。
pub fn premul16_num(c: u16, a: u16) -> u32 {
    c as u32 * a as u32
}

/// [`premul16_num`] 的**精确逆**：`(分子 + a/2) / a`，`a≥1` 时恒等于原 c。
///
/// `a=0` 约定得 0。商 > 65535 给 [`Fault::NumOutOfRange`]。
pub fn unpremul16_num(n: u32, a: u16) -> Result<u16, Fault> {
    if a == 0 {
        return Ok(0);
    }
    let q = (n + a as u32 / 2u32) / a as u32;
    if q > 65535u32 {
        return Err(Fault::NumOutOfRange { got: q, max: 65535 });
    }
    Ok(q as u16)
}

/// 8 位预乘**查表实现**：一次性建 65536 项（64 KiB），全域与算术实现
/// 逐值一致（判据真遍历，不抽样）。
#[derive(Clone, Debug)]
pub struct PremulLut8 {
    table: Vec<u8>,
}

pub const PREMUL_LUT8_ENTRIES: usize = 65536;

impl PremulLut8 {
    /// 建表：`index = c * 256 + a`，值 = [`premul8_ref`]。
    pub fn build() -> PremulLut8 {
        let mut table = Vec::with_capacity(PREMUL_LUT8_ENTRIES);
        let mut c = 0usize;
        while c < 256 {
            let mut a = 0usize;
            while a < 256 {
                table.push(premul8_ref(c as u8, a as u8));
                a += 1;
            }
            c += 1;
        }
        PremulLut8 { table }
    }

    /// 查表：`c` 颜色、`a` alpha。下标越界给 [`Fault`]（不静默回绕）。
    pub fn lookup(&self, c: u8, a: u8) -> Result<u8, Fault> {
        let idx = (c as usize) * 256 + (a as usize);
        match self.table.get(idx) {
            Some(v) => Ok(*v),
            None => Err(Fault::LutIndexOutOfRange { index: idx }),
        }
    }

    /// 表长（恒 65536）。
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// 是否为空（恒 false，理由同 F1009 的 LUT：空表该在构造时拒绝）。
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// 8 位预乘 **4 路并行语义实现**（SIMD 分发前的 lanes 语义层）。
///
/// 一次处理 4 个 `(c, a)` 对（`cs`/`as_` 各 4 长），逐 lane 算术舍入，
/// 与查表/算术实现**全域逐值一致**（判据钉）。长度不足 4 的尾部由
/// 调用方走 [`premul8_ref`]；这里只收成整 4 的倍数的批。
/// 返回写入的 lane 数；任一输入长度非 4 给 [`Fault`]（不静默截断——
/// 静默截断会让调用方把尾部当已处理）。
pub fn premul8_lanes4(cs: &[u8], alphas: &[u8], out: &mut [u8]) -> Result<usize, Fault> {
    if cs.len() != 4 || alphas.len() != 4 {
        return Err(Fault::LaneBatchSize { want: 4, got_c: cs.len(), got_a: alphas.len() });
    }
    if out.len() < 4 {
        return Err(Fault::BufferSize { want: 4, got: out.len() });
    }
    let mut i = 0usize;
    while i < 4 {
        let c = match cs.get(i) { Some(v) => *v, None => return Err(Fault::LaneBatchSize { want: 4, got_c: cs.len(), got_a: alphas.len() }) };
        let a = match alphas.get(i) { Some(v) => *v, None => return Err(Fault::LaneBatchSize { want: 4, got_c: cs.len(), got_a: alphas.len() }) };
        match out.get_mut(i) {
            Some(slot) => *slot = premul8_ref(c, a),
            None => return Err(Fault::BufferSize { want: 4, got: out.len() }),
        }
        i += 1;
    }
    Ok(4)
}

/// 16 位预乘**算术实现**：`(c*a + 32767) / 65535`。
///
/// 中间量核算：65535×65535 + 32767 = 4 294 868 992 < `u32::MAX`
/// （4 294 967 295）——**装得下是算过的**，注释即证明。
pub fn premul16_ref(c: u16, a: u16) -> u16 {
    let p = (c as u32 * a as u32 + 32767u32) / 65535u32;
    p as u16
}

/// 8 位**反向转直通**：`(p*255 + a/2) / a`；`a=0` 约定得 0
/// （预乘域 alpha=0 的像素颜色无定义，往返归 0 是唯一可复现的选择）。
pub fn unpremul8(p: u8, a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    let c = (p as u32 * 255u32 + a as u32 / 2u32) / a as u32;
    c as u8
}

/// 16 位反向转直通：`(p*65535 + a/2) / a`；`a=0` 约定得 0。
pub fn unpremul16(p: u16, a: u16) -> u16 {
    if a == 0 {
        return 0;
    }
    let c = (p as u32 * 65535u32 + a as u32 / 2u32) / a as u32;
    c as u16
}

// ===========================================================================
// 四、错误路径（贯穿全单）
// ===========================================================================

/// 本单全部错误路径。
///
/// 码段 **F1010 ⇒ `0xF6xx`**（全仓空闲段——建码段前全仓 grep 过：
/// `0xF1..0xF4` 被 F1001~F1004 占、F1007 沿 `0xF2` 续段、`0xF5` 归
/// F1009、`0x2B..0x2D` 被 VE-N F2605~F2607 占、`0xF9` 归 VE-J）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Fault {
    /// tRNS 色键载荷长度不符（灰度 2 / 真彩 6）。
    KeyPayloadLength { want: usize, got: usize },
    /// 色键值超过位深量程（锚点：拒绝该块）。
    KeyOutOfRange { key: u32, max: u32 },
    /// 调色板图无 PLTE 却来了 tRNS（无法定界的 alpha 数组）。
    PlteMissing,
    /// 位深非法（0 或 >16；IHDR 不可信输入）。
    BadBitDepth { bit_depth: u8 },
    /// 色键匹配的样本形状与键不符（灰度 1 / 真彩 3）。
    SampleShapeMismatch { want: usize, got: usize },
    /// 调色板透明索引越界。
    AlphaIndexOutOfRange { index: usize, count: usize },
    /// 查表下标越界。
    LutIndexOutOfRange { index: usize },
    /// lanes4 批大小不符。
    LaneBatchSize { want: usize, got_c: usize, got_a: usize },
    /// 缓冲区长度不符。
    BufferSize { want: usize, got: usize },
    /// 精确分子逆转换的分子超出合法预乘域（商 > 255 / 65535）。
    NumOutOfRange { got: u32, max: u32 },
}

impl Fault {
    /// 规则码（**本单独占码段** `0xF6xx`；与 F1009 的 `0xF5` 段、
    /// VE-N 的 `0x2C` 段不重叠——码段判据双向钉死）。
    pub fn code(&self) -> u16 {
        let n = match self {
            Fault::KeyPayloadLength { .. } => 1u16,
            Fault::KeyOutOfRange { .. } => 2,
            Fault::PlteMissing => 3,
            Fault::BadBitDepth { .. } => 4,
            Fault::SampleShapeMismatch { .. } => 5,
            Fault::AlphaIndexOutOfRange { .. } => 6,
            Fault::LutIndexOutOfRange { .. } => 7,
            Fault::LaneBatchSize { .. } => 8,
            Fault::BufferSize { .. } => 9,
            Fault::NumOutOfRange { .. } => 10,
        };
        0xF600 | n
    }

    /// 发生了什么。
    pub fn what(&self) -> String {
        match self {
            Fault::KeyPayloadLength { want, got } => {
                format!("tRNS 色键载荷长度不符：需要 {} 字节，实得 {}", want, got)
            }
            Fault::KeyOutOfRange { key, max } => {
                format!("色键值 {} 超出位深量程 0..={}", key, max)
            }
            Fault::PlteMissing => "调色板图无 PLTE 却收到 tRNS（alpha 数组无从定界）".to_string(),
            Fault::BadBitDepth { bit_depth } => {
                format!("位深 {} 非法（须 1..=16）", bit_depth)
            }
            Fault::SampleShapeMismatch { want, got } => {
                format!("色键匹配样本数不符：需要 {} 个，实得 {}", want, got)
            }
            Fault::AlphaIndexOutOfRange { index, count } => {
                format!("调色板透明索引 {} 越界（alpha 数组共 {} 项）", index, count)
            }
            Fault::LutIndexOutOfRange { index } => {
                format!("查表下标 {} 越界（表长 65536）", index)
            }
            Fault::LaneBatchSize { want, got_c, got_a } => {
                format!("lanes4 批大小不符：需要 {} 对，实得 c={} a={}", want, got_c, got_a)
            }
            Fault::BufferSize { want, got } => {
                format!("缓冲区长度不符：需要 {} 字节，实得 {}", want, got)
            }
            Fault::NumOutOfRange { got, max } => {
                format!("精确分子逆转换的商 {} 超出量程 0..={}", got, max)
            }
        }
    }

    /// 规则出处。
    pub fn why(&self) -> &'static str {
        match self {
            Fault::KeyPayloadLength { .. } => "F1010：灰度色键 2 字节、真彩色键 6 字节（16 位样本大端）",
            Fault::KeyOutOfRange { .. } => "F1010：色键与位深不匹配→拒绝该块（锚点错误路径）",
            Fault::PlteMissing => "F1010：tRNS 调色板透明索引须落在 PLTE 尺寸内",
            Fault::BadBitDepth { .. } => "F1010：位深来自不可信 IHDR，越界先拒",
            Fault::SampleShapeMismatch { .. } => "F1010：色键是精确匹配，样本形状必须与键一致",
            Fault::AlphaIndexOutOfRange { .. } => "F1010：索引越界是调用方 bug，不静默夹取",
            Fault::LutIndexOutOfRange { .. } => "F1010：查表下标须落在 0..65536",
            Fault::LaneBatchSize { .. } => "F1010：lanes4 只收整 4 批，尾部由调用方走标量路径",
            Fault::BufferSize { .. } => "F1010：输出缓冲须恰好容纳批结果",
            Fault::NumOutOfRange { .. } => "F1010：精确分子逆转换只收合法预乘分子（n ≤ c_max×a）",
        }
    }

    /// 下一步建议。
    pub fn advice(&self) -> String {
        match self {
            Fault::KeyPayloadLength { want, .. } => {
                format!("按颜色类型给出恰 {} 字节的 tRNS 载荷（大端）", want)
            }
            Fault::KeyOutOfRange { max, .. } => {
                format!("键值须 ≤ {}（按 IHDR 位深核对 tRNS 数值）", max)
            }
            Fault::PlteMissing => "先有 PLTE 再有 tRNS；调色板图必须带 PLTE".to_string(),
            Fault::BadBitDepth { .. } => "回源校验 IHDR 的 bit_depth 字段（PNG 合法值 1/2/4/8/16）".to_string(),
            Fault::SampleShapeMismatch { want, .. } => {
                format!("按颜色类型提供恰 {} 个通道样本再匹配", want)
            }
            Fault::AlphaIndexOutOfRange { count, .. } => {
                format!("索引须落在 0..{}；检查索引计算是否溢出", count)
            }
            Fault::LutIndexOutOfRange { .. } => "确认输入是 u8 对（0..256×0..256）".to_string(),
            Fault::LaneBatchSize { .. } => "成整 4 批调用；不足 4 的尾部走 premul8_ref 标量路径".to_string(),
            Fault::BufferSize { want, .. } => format!("改用长度 ≥ {} 的输出缓冲", want),
            Fault::NumOutOfRange { max, .. } => {
                format!("核对分子来源：合法预乘分子的逆商须 ≤ {}", max)
            }
        }
    }

    /// 三要素合成本行（与 F1008/F1009 同规格，全角竖线分隔）。
    pub fn three_elements(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.what());
        s.push('｜');
        s.push_str(self.why());
        s.push('｜');
        s.push_str(&self.advice());
        s
    }

    /// 诊断码（供日志与面板，格式稳定可 grep）。
    pub fn diag(&self) -> String {
        format!("F1010-{:#06X}", self.code())
    }
}
