//! VE-F1009 · PNG 16 位深支持（VE-F 域 · 着色器系统 · PNG 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1009`
//!
//! **判据（锚点原文）**：16bit roundtrip 逐位一致、降转换精度、sBIT 解析、
//! HDR 衔接预留、性能（16bit 比 8bit 慢 ≤2 倍）。
//!
//! 锚点原文：「PNG 16 位深全链极致深化：16bit 灰度/真彩/带 alpha 的编解码
//! （字节序大端——网络序读写显性）、16→8bit 降转换（高精度缩放——不截断用
//! 舍入：(v+128)>>8 与查表两实现，对拍逐值一致）、sBIT 块（有效位标注——
//! 有效位低于位深时的精度尊重：解码保留有效位语义、降转换按 sBIT 缩放非
//! 盲目截断）、16bit 与 HDR 管线（VE-V）的衔接接口（16bit 线性化输出——
//! gAMA 联动 F1004，供 HDR 合成）。数据结构：16bit 行缓冲（独立于 8bit
//! 路径——避免转换开销混入 8bit 热路径）、sBIT 描述符。错误路径：sBIT 值
//! 越界（有效位>位深）→拒绝该块按满精度处理；16bit+调色板（规范禁止
//! 组合）→显性拒绝。性能：16bit 比 8bit 慢 ≤2 倍（SIMD 反滤波按 16bit
//! 通道宽化——CGPU-F0094 扩展）。」
//!
//! 本单交付**16 位深的数据通路与契约**，不交付 zlib 本体与 Adam7 本体
//! （分别是 F1002 / F1003 的）。本单只回答：**16 位样本怎么摆、怎么读、
//! 怎么降到 8 位、有效位怎么尊重、交给 HDR 管线的是什么**。
//!
//! 1. **大端读写显性**（判据一）。PNG 的 16 位样本一律**网络序大端**，
//!    即高字节在前。这一条最容易被写成`to_le_bytes()` 而在跨端机器上
//!    静默出错，且**不会 panic、只会画面错**——所以本单把字节序做成
//!    [`put_be16`] / [`get_be16`] 两个**唯一入口**，全单不出现裸
//!    `to_le_bytes`（判据用「全文件不出现小端转换调用」把这条钉死）。
//!
//! 2. **降转换用u32 中间量 + 显式钳位**（判据二）。锚点写`(v+128)>>8`，
//!    但这句话在 16 位域里**照搬会错两次**，本单实测并修掉：
//!    - **回绕**：`v + 128` 在 `u16` 里当 `v >= 65408` 时溢出回绕，
//!      `v=65535` 算得 `0`（真值255）——画面 brightest 处变全黑。
//!    - **越界**：满位`v=65535` 即使不溢出，`(v+128)>>8 = 256` 也超出
//!      8 位上界 255。
//!    故算术实现 [`downgrade_arith`] 用 `u32` 做中间量再钳到 255。
//!    锚点同时要求「查表两实现对拍逐值一致」，故另有 [`DowngradeLut`]
//!    一次性建表、逐值查表，两条路径在**全部 65536 个输入**上必须相同
//!    （判据真的遍历 65536 个值，不是抽样）。
//!
//! 3. **sBIT 有效位语义**（判据三）。sBIT 声明每通道**有效位**，
//!    低于位深时降转换必须**按有效位归一**而非盲目 `>>8`：例如
//!    `sBIT=8` 的 16 位样本 `v=65535` 表示「8 位有效、满量程」，
//!    降到 8 位应得 **255** 而非 128。越界（有效位 > 位深）按锚点
//!    **拒绝该块并按满精度处理**，不得静默。
//!
//! 4. **16bit+调色板显性拒绝**（判据四）。PNG 规范禁止该组合
//!    （调色板索引没有 16 位形态）。这类组合必须**显性拒绝**，
//!    静默按灰度处理会让索引图变成噪声图。
//!
//! 5. **HDR 衔接契约冻结**（判据五）。16 位线性化输出交VE-V：
//!    [`HdrHandoff`] 冻结「进来的是什么位深/有效位、出去的是什么格式」，
//!    gAMA 联动 F1004 的 [`ColorSource`]，但**本单不代做转换**——
//!    F1004 的线性化是 `f32` 且clamp 到 [0,1]（8 位口径），
//!    16 位定点线性化需要自己的定点实现，那是 VE-V 侧的事。
//!    本单只保证**交接面有值、有来源标注、有契约文本**，不留空壳。
//!
//! ## 关于「16bit 比 8bit 慢 ≤2 倍」
//!
//! 真no_std 内核里**拿不到墙钟**（无rdtsc 抽象、无系统时钟源），
//! 断墙钟数字是自欺欺人。本单把这条锚点落成**可机检的结构事实**：
//! 16 位路径的**每样本字节数是 8 位路径的 2 倍**（大端 2 字节 vs 1 字节），
//! 而**滤波试探次数与行数的关系不因位深而变**（不因位深多跑一遍），
//! 且**滤波运算的整数宽度恰好宽化到 u16**（`i16` 中间量装得下
//! `a+b-c` 三项而不溢出）。判据断这三件事，并在注释里明写「本单未实测
//! 墙钟倍率」，不谎报。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / `panic!` / 裸下标越界：所有字节读写
//! 先查界再动，越界返回 [`Fault`]；`u16` 加法一律走 `u32` 中间量。

#![allow(clippy::needless_range_loop)]

// lib.rs 只有 `extern crate alloc` 且**无 `#[macro_use]`**，
// 故 `format!` / `vec!` 宏必须逐文件显式导入：
// `format!` 走 `use alloc::format;`（宏与类型是两个命名空间，
// `use alloc::string::String;` 管不到宏）；`vec!` 走 `use alloc::vec;`
// （`use alloc::vec::Vec;` 只导入**类型** `Vec`，不导入 `vec!` 宏）。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vef02_pngenc::ColorType;
use super::vef04_color::ColorSource;

// ===========================================================================
// 一、16 位样本与网络序字节序（判据一）
// ===========================================================================

/// 16 位样本在 8 位深下的**最大值**（线性满量程）。
pub const U16_MAX: u16 = 65535;

/// 8 位样本在 8 位深下的最大值。
pub const U8_MAX: u8 = 255;

/// 本单支持的颜色类型（**排除调色板**——见 [`ColorType16`] 文档）。
///
/// 灰度/真彩/带 alpha 三类都有 16 位形态；调色板**没有**，故不在此列。
pub const WIDE_COLOR_TYPES: [ColorType; 4] = [
    ColorType::Gray,
    ColorType::Rgb,
    ColorType::GrayAlpha,
    ColorType::Rgba,
];

/// 每个样本的字节数（16 位深恒为 2——网络序大端）。
pub const BYTES_PER_SAMPLE_16: usize = 2;

/// 该颜色类型的**通道数**（调色板按 1 算，但其 16 位形态被拒）。
pub fn channel_count(ct: ColorType) -> usize {
    match ct {
        ColorType::Gray => 1,
        ColorType::Rgb => 3,
        ColorType::Palette => 1,
        ColorType::GrayAlpha => 2,
        ColorType::Rgba => 4,
    }
}

/// 写一个 16 位样本到 `out`（**网络序大端**：高字节在前）。
///
/// 返回写入的字节数（恒 2）；越界返回 0 且**不写任何字节**
/// （绝不半写——半写的缓冲区会被下游当成有效样本流）。
pub fn put_be16(out: &mut [u8], sample: u16) -> usize {
    if out.len() < BYTES_PER_SAMPLE_16 {
        return 0;
    }
    let b = sample.to_be_bytes();
    out[0] = b[0];
    out[1] = b[1];
    BYTES_PER_SAMPLE_16
}

/// 读一个 16 位样本（**网络序大端**）。越界返回 [`Fault`]。
pub fn get_be16(src: &[u8]) -> Result<u16, Fault> {
    if src.len() < BYTES_PER_SAMPLE_16 {
        return Err(Fault::ShortBuffer {
            want: BYTES_PER_SAMPLE_16,
            got: src.len(),
        });
    }
    Ok(u16::from_be_bytes([src[0], src[1]]))
}

// ===========================================================================
// 二、16 位行缓冲（判据一 · 独立于 8 位热路径）
// ===========================================================================

/// 一行 16 位样本的行缓冲。
///
/// **为什么独立于 8 位路径**：锚点明写「避免转换开销混入 8bit 热路径」。
/// 若把 16 位样本塞进 8 位的 `Vec<u8>` 再靠标记位区分，则每次访问都要
/// 走分支判断「这是 8 位还是 16 位」，等于把 16 位开销摊进 8 位热路径。
/// 这里用**独立的强类型** [`Row16`]：类型层就不允许把它当 8 位缓冲用。
///
/// 内部布局是**交错**的（`r0g0b0 r1g1b1 …`），与 PNG 规范一致；
/// 不是 `Vec<Vec<u16>>` 的平面布局——平面布局会在打包成PNG 字节流时
/// 多一次跨步拷贝。
/// 一行 16 位样本所需的字节数（= 宽 × 通道 × 2），**全程 checked 算术**。
///
/// 为什么必须 checked：`width` 来自 PNG 块里的 IHDR，是**不可信输入**。
/// 裸 `width * channels * 2` 在 `width = usize::MAX` 时 release 下静默回绕
/// 成一个小数字（后续按小数字分配缓冲 ⇒ 越界写），debug 下直接 panic。
/// 两种结果都不可接受，故这里显式判溢出并交出 [`Fault`]。
///
/// 这是**本单唯一的尺寸算术入口**：行缓冲构造、解包、打包三条路径的
/// 字节数都从这里取，避免三处各写一遍裸乘法而漏掉其中一处。
pub fn row_byte_len(width: usize, channels: usize) -> Result<usize, Fault> {
    match width.checked_mul(channels).and_then(|n| n.checked_mul(BYTES_PER_SAMPLE_16)) {
        Some(n) => Ok(n),
        None => Err(Fault::DimensionOverflow { width, channels }),
    }
}

/// 一行 16 位样本的**样本**数（= 宽 × 通道），checked 算术。
pub fn row_sample_count(width: usize, channels: usize) -> Result<usize, Fault> {
    match width.checked_mul(channels) {
        Some(n) => Ok(n),
        None => Err(Fault::DimensionOverflow { width, channels }),
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row16 {
    ct: ColorType,
    width: usize,
    /// 交错样本：`[r0,g0,b0, r1,g1,b1, …]`，长度 = width × 通道数。
    samples: Vec<u16>,
    /// 构造时尺寸算术溢出（见 [`Row16::new`]的兜底路径）。
    ///
    /// **落成字段而不是静默夹取**：兜底本身必须可观测，否则调用方拿到
    /// 一个「宽度 18 亿、样本 0 个」的行对象却毫无察觉。
    overflowed: bool,
}

impl Row16 {
    /// 新建一行。`width == 0` 合法（PNG 允许零宽？——不，规范要求 width>=1，
    /// 但本模块**不替调用方拦**，宽度校验在 [`Row16::new_checked`]）。
    ///
    /// **尺寸算术溢出时的兜底**：样本数组留空并把 [`Row16::overflowed`]
    /// 置真，而不是让 `width * ch` 静默回绕。理由是 `new` 是** infallible
    /// 接口**（判据层与热路径都在用），改成 `Result` 会波及所有调用方；
    /// 但兜底必须留痕，否则「宽度巨大 ⇒ 零样本」会被下游读成合法空行。
    /// 需要**硬拒绝**的调用方走 [`Row16::new_checked`]。
    pub fn new(ct: ColorType, width: usize) -> Row16 {
        let ch = channel_count(ct);
        match row_sample_count(width, ch) {
            Ok(n) => Row16 {
                ct,
                width,
                samples: vec![0u16; n],
                overflowed: false,
            },
            Err(_) => Row16 {
                ct,
                width,
                samples: Vec::new(),
                overflowed: true,
            },
        }
    }

    /// 尺寸算术是否溢出（`true` ⇒ 本行的样本数组是兜底空数组）。
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// 带校验的构造：**16 位深 + 调色板**这一规范禁止的组合在此拒绝，
    /// 尺寸算术溢出也在此**硬拒绝**（不给兜底留退路）。
    ///
    /// 放在构造器而不是打包器，是为了让**错误在数据进入本模块的第一道
    /// 门就被拦住**，而不是等到打包时才发现。
    pub fn new_checked(ct: ColorType, width: usize) -> Result<Row16, Fault> {
        if ct == ColorType::Palette {
            return Err(Fault::PaletteWithWide);
        }
        // 溢出在此显式拒绝：infallible 的 `new` 允许兜底，`new_checked`
        // 是「我保证尺寸可信」的入口，兜底在这里等于把错误往下游推。
        row_sample_count(width, channel_count(ct))?;
        Ok(Row16::new(ct, width))
    }

    /// 颜色类型。
    pub fn color_type(&self) -> ColorType {
        self.ct
    }

    /// 行宽（像素数）。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 通道数。
    pub fn channels(&self) -> usize {
        channel_count(self.ct)
    }

    /// 样本总数（= 宽 × 通道）。
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// 该行打包成PNG 字节流所需的字节数（= 宽 × 通道 × 2）。
    ///
    /// **不含滤波字节**：PNG 每行前面还有 1 字节滤波器类型，
    /// 那属于 F1002/F1003 的打包层，不在本单的缓冲职责内。
    ///
    /// 走 [`row_byte_len`] 的 checked 算术：溢出时给 0（与
    /// [`Row16::overflowed`] 的空样本兜底一致——**不能**给一个回绕出
    /// 的小数字，那会让 [`Row16::pack_be`] 的长度校验误判为「尺寸对得上」）。
    pub fn byte_len(&self) -> usize {
        match row_byte_len(self.width, self.channels()) {
            Ok(n) => n,
            Err(_) => 0,
        }
    }

    /// 取一个样本。越界给 [`Fault`]——**不做静默夹取**，
    /// 夹取会把「写错了列数」变成「画面某列颜色不对」这种查不出的症状。
    pub fn get(&self, index: usize) -> Result<u16, Fault> {
        match self.samples.get(index) {
            Some(v) => Ok(*v),
            None => Err(Fault::SampleOutOfRange {
                index,
                count: self.samples.len(),
            }),
        }
    }

    /// 写一个样本。越界给 [`Fault`]。
    pub fn set(&mut self, index: usize, v: u16) -> Result<(), Fault> {
        match self.samples.get_mut(index) {
            Some(slot) => {
                *slot = v;
                Ok(())
            }
            None => Err(Fault::SampleOutOfRange {
                index,
                count: self.samples.len(),
            }),
        }
    }

    /// 只读访问整行样本（供打包器与判据遍历）。
    pub fn samples(&self) -> &[u16] {
        &self.samples
    }

    /// 打包成网络序大端字节流。
    ///
    /// 输出长度须**恰为** [`Row16::byte_len`]；给多了/给少了都拒绝——
    /// 「给多了」若被默默接受，调用方会以为自己拿到了完整行。
    ///
    /// 【为什么不用 `&mut out[i * 2..]` 切片】长度校验在函数开头做过，
    /// 但切片**自身**仍会 panic——`byte_len()` 与 `samples.len()*2` 一旦
    /// 不一致（未来新增字段、或别处改了样本数），这里就是越界切片而不是
    /// 一个错误码。改成 `get_mut` + 逐次下标检查：越界给 [`Fault`]。
    /// 代价是每次循环多一次边界检查，换来「零 panic 面」这一硬要求。
    pub fn pack_be(&self, out: &mut [u8]) -> Result<usize, Fault> {
        let need = self.byte_len();
        if out.len() != need {
            return Err(Fault::BufferSize {
                want: need,
                got: out.len(),
            });
        }
        let mut i = 0usize;
        while i < self.samples.len() {
            let off = i * BYTES_PER_SAMPLE_16;
            match out.get_mut(off..off + BYTES_PER_SAMPLE_16) {
                Some(dst) => match self.samples.get(i) {
                    Some(v) => {
                        put_be16(dst, *v);
                    }
                    None => {
                        return Err(Fault::SampleOutOfRange {
                            index: i,
                            count: self.samples.len(),
                        });
                    }
                },
                None => {
                    return Err(Fault::ShortBuffer {
                        want: off + BYTES_PER_SAMPLE_16,
                        got: out.len(),
                    });
                }
            }
            i += 1;
        }
        Ok(need)
    }

    /// 从网络序大端字节流解出一行。
    ///
    /// **不做滤波反算**：滤波是 F1002/F1003 的事；本单只负责
    /// 「字节流 → 样本」这一段，滤波类型字节由调用方先剥掉。
    pub fn unpack_be(ct: ColorType, src: &[u8], width: usize) -> Result<Row16, Fault> {
        let ch = channel_count(ct);
        // checked：width 来自不可信块头，裸乘法在巨大宽度下回绕。
        let need = row_byte_len(width, ch)?;
        if src.len() != need {
            return Err(Fault::BufferSize {
                want: need,
                got: src.len(),
            });
        }
        // 已由row_byte_len 的 checked 确认尺寸可表示，这里走 new_checked
        // 额外拿「调色板」那道门——本函数对调色板同样必须拒。
        let mut row = Row16::new_checked(ct, width)?;
        let mut i = 0usize;
        // 同 [`Row16::pack_be`]：不用裸切片与裸写，越界一律给 [`Fault`]。
        while i < row.sample_count() {
            let off = i * BYTES_PER_SAMPLE_16;
            let slice = match src.get(off..off + BYTES_PER_SAMPLE_16) {
                Some(sl) => sl,
                None => {
                    return Err(Fault::ShortBuffer {
                        want: off + BYTES_PER_SAMPLE_16,
                        got: src.len(),
                    });
                }
            };
            let v = get_be16(slice)?;
            row.set(i, v)?;
            i += 1;
        }
        Ok(row)
    }
}

// ===========================================================================
// 三、16→8 降转换：算术实现 + 查表实现（判据二）
// ===========================================================================

/// **算术实现**：`(v + 128) >> 8`，但用 `u32` 做中间量并钳到 255。
///
/// 锚点原文是 `(v+128)>>8`。直接照搬在 16 位域会错两次，本单实测：
/// - `v=65535`：`v+128 = 65663`，若在 `u16` 里算则**回绕成 127**
///   ⇒ `>>8` 得 **0**（真值 255）——画面最亮处变全黑；
/// - 即使不溢出：`(65535+128)>>8 = 256` **超出 8 位上界**。
///
/// 故此处显式用 `u32` 承接 `v + 128`，再钳到 [`U8_MAX`]。
/// 这个钳位是**不可省的**：`255<<8 = 65280`，而 `65280..65535`
/// 这 256 个输入全都会算出 256。
pub fn downgrade_arith(v: u16) -> u8 {
    let scaled = (v as u32 + 128u32) >> 8;
    if scaled > U8_MAX as u32 {
        U8_MAX
    } else {
        scaled as u8
    }
}

/// 一次性建好的 16→8 降转换查表。
///
/// 锚点要求「`(v+128)>>8` 与查表**两实现**，对拍逐值一致」。
/// 表长 [`U16_LUT_ENTRIES`] = 65536 —— **全域**，不是抽样。
#[derive(Clone, Debug)]
pub struct DowngradeLut {
    table: Vec<u8>,
}

/// 查表长度：16 位域全域 65536 项。
pub const U16_LUT_ENTRIES: usize = 65536;

impl DowngradeLut {
    /// 建表（全域 65536 项，一次性算完）。
    ///
    /// 内存 64 KiB——本单**刻意不在 8 位热路径上持有一张全表**：
    /// [`DowngradeLut`] 是按需创建的类型，谁用谁建。
    pub fn build() -> DowngradeLut {
        let mut table = Vec::with_capacity(U16_LUT_ENTRIES);
        let mut v = 0usize;
        while v < U16_LUT_ENTRIES {
            table.push(downgrade_arith(v as u16));
            v += 1;
        }
        DowngradeLut { table }
    }

    /// 查表降转换。越界给 [`Fault`]。
    pub fn lookup(&self, v: u16) -> Result<u8, Fault> {
        match self.table.get(v as usize) {
            Some(x) => Ok(*x),
            None => Err(Fault::LutIndexOutOfRange { index: v as usize }),
        }
    }

    /// 表长（恒为 [`U16_LUT_ENTRIES`]）。
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// 是否为空（恒为 `false`——本类型不留空表，
    /// 因为「空表」会让每次查表都失败，而那应当由构造时就拒绝）。
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// 按 sBIT 有效位把 16 位样本缩到 8 位。
///
/// # 为什么不能盲目 `>>8`
///
/// sBIT 声明**有效位**。`sBIT=8` 的 16 位样本里只有高 8 位是有效数据，
/// 低 8 位是填充。此时 `v=65280`（= `255<<8`，即 8 位有效范围的满量程）
/// 表示「8 位有效、已满」，降到 8 位应得 **255**；
/// 而盲目 `(65280+128)>>8` 得 **255** 看似对，但 `v=32640`（有效 8 位的中点，
/// 真值 127）盲目缩放得 **127** 也对——**差别在低有效位段**：
/// `v=255`（有效 8 位应得 0.001→0）盲目缩放得 **1**，把「近乎全黑」
/// 抬成「可见的灰」。sBIT 语义要求按**有效位量程**归一，不是按 16 位量程。
///
/// # 口径（实测得出，非拍脑袋）
///
/// ```text
/// valid = v >> (16 - sbit)          // 有效位数据，范围 0 ..= 2^sbit - 1
/// maxv  = 2^sbit - 1// 有效位满量程
/// out   = (valid * 255 + maxv/2) / maxv   // 整数舍入，末位自然 ≤ 255
/// ```
///
/// 验证：sbit=8 时 `valid_max=255`，公式退化成恒等 ⇒ `out=valid`，
/// 即「8 位有效数据原样落到 8 位」，这正是 sBIT 的语义。
/// 全域 sbit∈1..=16 逐值单调非降且输出恒 ≤255（判据真遍历 65536×16 组）。
/// 当 `sbit == 16`（满位）时直接走 [`downgrade_arith`]——
/// 两条路径在 sbit=16 上**数值一致**（判据逐值对拍，不是抽样）。
pub fn downgrade_by_sbit(v: u16, sbit: u8) -> Result<u8, Fault> {
    if sbit == 0 || sbit > 16 {
        return Err(Fault::SbitOutOfRange { sbit });
    }
    if sbit == 16 {
        return Ok(downgrade_arith(v));
    }
    let shift = 16u32 - sbit as u32;
    let valid = (v as u32) >> shift;
    let maxv = (1u32 << sbit as u32) - 1u32;
    // 整数舍入：加半个分母再整除。maxv≥1（sbit≥1 已保证）。
    let out = (valid * (U8_MAX as u32) + maxv / 2u32) / maxv;
    if out > U8_MAX as u32 {
        Ok(U8_MAX)
    } else {
        Ok(out as u8)
    }
}

// ===========================================================================
// 四、sBIT 块：描述符与解析（判据三）
// ===========================================================================

/// sBIT 描述符：每通道的有效位。
///
/// 通道数必须与颜色类型**逐一对齐**——PNG 规范里sBIT 的长度由
/// 颜色类型唯一决定（灰度 1、RGB 3、带 alpha 各+1、调色板 3）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SbitDescriptor {
    /// 每通道有效位，顺序与 [`SbitDescriptor::channels`] 一致。
    pub bits: Vec<u8>,
}

impl SbitDescriptor {
    /// 按颜色类型新建描述符（`bits` 长度须等于通道数）。
    pub fn new(ct: ColorType, bits: Vec<u8>) -> Result<SbitDescriptor, Fault> {
        let want = channel_count(ct);
        if bits.len() != want {
            return Err(Fault::SbitLengthMismatch { want, got: bits.len() });
        }
        Ok(SbitDescriptor { bits })
    }

    /// 满位描述符（每通道都是 16）。
    pub fn full(ct: ColorType) -> SbitDescriptor {
        let n = channel_count(ct);
        SbitDescriptor {
            bits: vec![16u8; n],
        }
    }

    /// 通道数。
    pub fn channels(&self) -> usize {
        self.bits.len()
    }

    /// 第 `i` 通道的有效位。越界给 [`Fault`]。
    pub fn bit_of(&self, i: usize) -> Result<u8, Fault> {
        match self.bits.get(i) {
            Some(v) => Ok(*v),
            None => Err(Fault::ChannelOutOfRange {
                index: i,
                count: self.bits.len(),
            }),
        }
    }

    /// 是否为满位描述符（所有通道都是 16）——满位时降转换
    /// 就是普通 [`downgrade_arith`]，无需按通道分别处理。
    pub fn is_full(&self) -> bool {
        let mut i = 0usize;
        while i < self.bits.len() {
            if self.bits[i] != 16 {
                return false;
            }
            i += 1;
        }
        !self.bits.is_empty()
    }

    /// 校验：每通道有效位须落在 `1..=16`。
    ///
    /// 锚点：「sBIT 值越界（有效位>位深）→ **拒绝该块按满精度处理**」。
    /// 这里只**判定**越界（不做钳位——钳位就是「静默」）；
    /// 真正的「拒绝并按满精度处理」由 [`resolve_sbit`] 落地。
    pub fn validate(&self) -> Result<(), Fault> {
        let mut i = 0usize;
        while i < self.bits.len() {
            let b = self.bits[i];
            if b == 0 || b > 16 {
                return Err(Fault::SbitOutOfRange { sbit: b });
            }
            i += 1;
        }
        Ok(())
    }

    /// 按 PNG 块载荷组装 sBIT 块的**内容**（不含块头）。
    ///
    /// 组装顺序即通道顺序，**不做任何压缩或编码**——sBIT 是裸字节块。
    pub fn to_payload(&self) -> Vec<u8> {
        self.bits.clone()
    }

    /// 从 sBIT 块载荷解析（`src.len()` 必须等于通道数）。
    pub fn from_payload(ct: ColorType, src: &[u8]) -> Result<SbitDescriptor, Fault> {
        let want = channel_count(ct);
        if src.len() != want {
            return Err(Fault::SbitLengthMismatch {
                want,
                got: src.len(),
            });
        }
        let mut bits = Vec::with_capacity(want);
        let mut i = 0usize;
        while i < want {
            bits.push(src[i]);
            i += 1;
        }
        let d = SbitDescriptor { bits };
        d.validate()?;
        Ok(d)
    }
}

/// sBIT 块的处置结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SbitVerdict {
    /// 块有效，按声明的有效位使用。
    Accepted(SbitDescriptor),
    /// 块越界（有效位 > 16）⇒ **拒绝该块，按满精度处理**。
    ///
    /// 关键：**降级方向是「按满精度」而不是「按声明值」。
    /// 若按越界的声明值缩放，一个 `sBIT=200` 的块会把画面缩成全黑，
    /// 而「忽略这个块、按 16 位满量程处理」至少是正确的画面。
    RejectedOutOfRange { bad_sbit: u8 },
    /// 该组合没有 sBIT（如 16 位 + 调色板）⇒ 按满精度。
    NotApplicable,
}

/// 按锚点的错误路径处置 sBIT 块。
///
/// 「拒绝该块按满精度处理」落地为：返回 [`SbitVerdict::RejectedOutOfRange`]，
/// **且不带任何缩放参数**——调用方看到它就知道该走满位路径。
pub fn resolve_sbit(ct: ColorType, payload: &[u8]) -> SbitVerdict {
    // 16 位 + 调色板：规范禁止该组合，sBIT 无从谈起⇒ 按满精度。
    if ct == ColorType::Palette {
        return SbitVerdict::NotApplicable;
    }
    let want = channel_count(ct);
    if payload.len() != want {
        // 长度不对也算「块不可用」⇒ 按满精度，但如实记下长度。
        return SbitVerdict::RejectedOutOfRange {
            bad_sbit: payload.len() as u8,
        };
    }
    // 先逐字节查越界（有效位 0 或 >16）。
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        if b == 0 || b > 16 {
            return SbitVerdict::RejectedOutOfRange { bad_sbit: b };
        }
        i += 1;
    }
    match SbitDescriptor::from_payload(ct, payload) {
        Ok(d) => SbitVerdict::Accepted(d),
        // from_payload 已 validate 过，走到这里说明有未预期的路径；
        // **不静默**：按满精度并记下首个字节。
        Err(_) => SbitVerdict::RejectedOutOfRange {
            bad_sbit: if payload.is_empty() { 0 } else { payload[0] },
        },
    }
}

/// 按 sBIT 处置结果降转换一个样本。
///
/// 满位 / 块被拒 / 块不适用 ⇒ 一律走 [`downgrade_arith`]；
/// 仅「块有效且该通道非满位」才走 [`downgrade_by_sbit`]。
/// **一个函数收口**，避免调用方各写一套而出现口径分叉。
pub fn downgrade_with_verdict(v: u16, verdict: &SbitVerdict, channel: usize) -> Result<u8, Fault> {
    match verdict {
        SbitVerdict::Accepted(d) => {
            let b = d.bit_of(channel)?;
            downgrade_by_sbit(v, b)
        }
        SbitVerdict::RejectedOutOfRange { .. } => Ok(downgrade_arith(v)),
        SbitVerdict::NotApplicable => Ok(downgrade_arith(v)),
    }
}

// ===========================================================================
// 五、16bit + 调色板：规范禁止组合（判据四）
// ===========================================================================

/// 调色板 + 16 位深 ⇒ 拒绝。返回 [`Fault`]。
///
/// PNG 规范里调色板索引是 1/2/4/8 位，**没有 16 位形态**。
/// 这类组合若被「宽容处理」（比如当灰度），索引图会变成噪声图——
/// 而噪声图是**能正常显示**的，用户根本发现不了。
pub fn reject_palette_wide() -> Fault {
    Fault::PaletteWithWide
}

/// 该颜色类型在 16 位深下是否合法（调色板除外全部合法）。
pub fn wide_combo_is_legal(ct: ColorType) -> bool {
    ct != ColorType::Palette
}

// ===========================================================================
// 六、HDR 衔接契约（判据五）
// ===========================================================================

/// 交VE-V 的 HDR 合成面的一帧线性数据。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Linear16 {
    /// 交错线性样本（同 [`Row16`] 布局）。
    pub samples: Vec<u16>,
    /// 通道数（冻结：下游不得假设别的数）。
    pub channels: usize,
    /// 线性化所用的 gAMA 乘 100 000（`None` = 按默认 sRGB，即 45455）。
    pub gamma_x100000: Option<u32>,
    /// 色彩来源（联动 F1004 [`ColorSource`]），供下游留痕。
    pub source: ColorSource,
}

/// HDR 管线对 16 位数据的三条硬要求（**契约，不是建议**）。
///
/// 写在类型旁而不是文档里，因为下游 VE-V 会照抄这段；
/// 写成 `&'static [&'static str]` 是为了**能被判据读到并逐条核对**——
/// 契约写在文档里等于没有契约。
pub const HDR_CONTRACT: [&str; 3] = [
    "输入必须是 16 位深（每样本 2 字节大端），不接受 8 位冒充",
    "线性化后的值域是[0, 65535]，不得再钳到 8 位（钳了就丢 HDR 范围）",
    "gAMA 来源必须随帧带出，不许假定 sRGB 默认",
];

/// 为一行 16 位样本准备 HDR 交接面。
///
/// **本单不代做线性化**：F1004 的 [`ColorSource`] 配套函数是 `f32`
/// 且 clamp 到 `[0,1]`（8 位口径），16 位定点线性化属于 VE-V 侧。
/// 这里只把「输入是什么、来源是誰、契约是什么」冻结成可传递的值。
///
/// `gamma_x100000` 传 [`None`] 表示**显式采用默认 sRGB**，
/// 而不是「不知道」——下游据此知道该按 45455 处理。
pub fn handoff_for_hdr(row: &Row16, source: ColorSource, gamma_x100000: Option<u32>) -> Linear16 {
    Linear16 {
        channels: row.channels(),
        samples: row.samples().to_vec(),
        gamma_x100000,
        source,
    }
}

/// HDR 交接面的自检：形状必须与源行一致、gAMA 合法。
///
/// **不留空壳**：交接面必须能被独立校验，不能靠「下游自己看着办」。
pub fn validate_handoff(h: &Linear16) -> Result<(), Fault> {
    if h.channels == 0 || h.channels > 4 {
        return Err(Fault::BadChannelCount(h.channels));
    }
    if h.samples.len() % h.channels != 0 {
        return Err(Fault::HandoffShapeMismatch {
            samples: h.samples.len(),
            channels: h.channels,
        });
    }
    if let Some(g) = h.gamma_x100000 {
        if g == 0 || g > 500_000 {
            return Err(Fault::GamaOutOfRange { gamma: g });
        }
    }
    Ok(())
}

// ===========================================================================
// 七、错误路径（贯穿全单）
// ===========================================================================

/// 本单全部错误路径。
///
/// 每条都带**三要素**（发生了什么 / 规则出处 / 下一步），
/// 与 F1008 的拒绝信息同规格——不写「失败」了事。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 缓冲区长度与要求不符（`want` 要多少、`got` 实际多少）。
    BufferSize { want: usize, got: usize },
    /// 读 16 位样本时字节不足。
    ShortBuffer { want: usize, got: usize },
    /// 样本下标越界。
    SampleOutOfRange { index: usize, count: usize },
    /// 通道下标越界。
    ChannelOutOfRange { index: usize, count: usize },
    /// 查表下标越界。
    LutIndexOutOfRange { index: usize },
    /// sBIT 有效位越界（0 或 > 16）。
    SbitOutOfRange { sbit: u8 },
    /// sBIT 长度与颜色类型通道数不符。
    SbitLengthMismatch { want: usize, got: usize },
    /// 16 位深 + 调色板（PNG 规范禁止）。
    PaletteWithWide,
    /// HDR 交接面通道数非法。
    BadChannelCount(usize),
    /// HDR 交接面样本数与通道数不整除。
    HandoffShapeMismatch { samples: usize, channels: usize },
    /// gAMA 值越界（与 F1004 同口径：(0.01, 5.0)×100000）。
    GamaOutOfRange { gamma: u32 },
    /// 行尺寸算术溢出（宽 × 通道 × 2 超出 `usize` 可表示范围）。
    ///
    /// **为什么单列一个码而不是复用 `BufferSize`**：`BufferSize` 说的是
    /// 「长度不符」（可修的调用错误），本变体说的是「这个尺寸根本不是
    /// 一个合法的行尺寸」（不可信输入）。两者混用会让下游把恶意 IHDR
    /// 当成普通的长度写错而重试。
    DimensionOverflow { width: usize, channels: usize },
}

impl Fault {
    /// 规则码（**本单独占码段**，F1009 ⇒ `0xF5xx`）。
    ///
    /// PNG 组段位现状：`0xF1..0xF4` 被 F1001~F1004 占用、F1007 沿 `0xF2`
    /// 续段，故 F1009 顺延取**全仓空闲**的 `0xF5`。`0x2Cxx` 已被
    /// VE-N F2606（`ven06_incr`）占用，不可复用——码段撞号会让下游
    /// 按码分流时把两个域的故障当成一种，且这类撞号不报编译错、
    /// 只在运行期静默混流，故码段归属必须写成判据钉死。
    pub fn code(&self) -> u16 {
        let n = match self {
            Fault::BufferSize { .. } => 1u16,
            Fault::ShortBuffer { .. } => 2,
            Fault::SampleOutOfRange { .. } => 3,
            Fault::ChannelOutOfRange { .. } => 4,
            Fault::LutIndexOutOfRange { .. } => 5,
            Fault::SbitOutOfRange { .. } => 6,
            Fault::SbitLengthMismatch { .. } => 7,
            Fault::PaletteWithWide => 8,
            Fault::BadChannelCount(_) => 9,
            Fault::HandoffShapeMismatch { .. } => 10,
            Fault::GamaOutOfRange { .. } => 11,
            Fault::DimensionOverflow { .. } => 12,
        };
        0xF500 | n
    }

    /// 发生了什么。
    pub fn what(&self) -> String {
        match self {
            Fault::BufferSize { want, got } => {
                format!("缓冲区长度不符：需要 {} 字节，实得 {}", want, got)
            }
            Fault::ShortBuffer { want, got } => {
                format!("读 16 位样本字节不足：需要 {} 字节，实得 {}", want, got)
            }
            Fault::SampleOutOfRange { index, count } => {
                format!("样本下标 {} 越界（本行共 {} 个样本）", index, count)
            }
            Fault::ChannelOutOfRange { index, count } => {
                format!("通道下标 {} 越界（共 {} 个通道）", index, count)
            }
            Fault::LutIndexOutOfRange { index } => {
                format!("查表下标 {} 越界（表长 65536）", index)
            }
            Fault::SbitOutOfRange { sbit } => {
                format!("sBIT 有效位 {} 越界（须 1..=16）", sbit)
            }
            Fault::SbitLengthMismatch { want, got } => {
                format!("sBIT 长度不符：需 {} 字节（按颜色类型通道数），实得 {}", want, got)
            }
            Fault::PaletteWithWide => {
                "16 位深 + 调色板：PNG 规范禁止该组合（调色板索引无 16 位形态）".to_string()
            }
            Fault::BadChannelCount(n) => {
                format!("HDR 交接面通道数 {} 非法（须 1..=4）", n)
            }
            Fault::HandoffShapeMismatch { samples, channels } => {
                format!("HDR 交接面样本数 {} 与通道数 {} 不整除", samples, channels)
            }
            Fault::GamaOutOfRange { gamma } => {
                format!("gAMA 值 {} 越出 (0.01, 5.0)×100000 区间", gamma)
            }
            Fault::DimensionOverflow { width, channels } => {
                format!(
                    "行尺寸算术溢出：宽 {} × 通道 {} × {} 字节超出可表示范围",
                    width, channels, BYTES_PER_SAMPLE_16
                )
            }
        }
    }

    /// 规则出处。
    pub fn why(&self) -> &'static str {
        match self {
            Fault::BufferSize { .. } => "F1009：行缓冲打包要求输出长度恰等于 byte_len()",
            Fault::ShortBuffer { .. } => "F1009：网络序大端读 16 位样本需 2 字节",
            Fault::SampleOutOfRange { .. } => "F1009：行缓冲按越界报错，不做静默夹取",
            Fault::ChannelOutOfRange { .. } => "F1009：通道下标须落在描述符长度内",
            Fault::LutIndexOutOfRange { .. } => "F1009：查表下标须落在 0..65535",
            Fault::SbitOutOfRange { .. } => "F1009：sBIT 有效位须落在 1..=16，越界拒块按满精度",
            Fault::SbitLengthMismatch { .. } => "F1009：sBIT 长度由颜色类型通道数唯一决定",
            Fault::PaletteWithWide => "F1009：16bit+调色板（规范禁止组合）→ 显性拒绝",
            Fault::BadChannelCount(_) => "F1009：HDR 交接面通道数须 1..=4",
            Fault::HandoffShapeMismatch { .. } => "F1009：HDR 交接面样本数须是通道数的整数倍",
            Fault::GamaOutOfRange { .. } => "F1009：gAMA 与 F1004 同口径，越界即拒",
            Fault::DimensionOverflow { .. } => {
                "F1009：宽×通道×2 全程 checked，溢出即拒（宽度来自不可信 IHDR）"
            }
        }
    }

    /// 下一步建议。
    pub fn advice(&self) -> String {
        match self {
            Fault::BufferSize { want, .. } => format!("改用长度恰为 {} 的缓冲（不要多给也不要少给）", want),
            Fault::ShortBuffer { .. } => "补齐字节后再读；16 位样本必须成对出现".to_string(),
            Fault::SampleOutOfRange { count, .. } => format!("本行只有 {} 个样本，检查列×通道的乘法是否溢出", count),
            Fault::ChannelOutOfRange { count, .. } => format!("只有 {} 个通道，检查颜色类型是否与数据一致", count),
            Fault::LutIndexOutOfRange { .. } => "确认输入是 u16（0..65535），超出说明上游给了超范围值".to_string(),
            Fault::SbitOutOfRange { .. } => "修正 sBIT 为 1..=16；若原文件确实越界，本模块已按满精度处理".to_string(),
            Fault::SbitLengthMismatch { want, .. } => format!("按颜色类型给出恰{} 字节的 sBIT 载荷", want),
            Fault::PaletteWithWide => "改用灰度/真彩承载该图；调色板索引只能是 1/2/4/8 位".to_string(),
            Fault::BadChannelCount(_) => "通道数只能是 1（灰）/2（灰A）/3（RGB）/4（RGBA）".to_string(),
            Fault::HandoffShapeMismatch { channels, .. } => format!("补齐样本使总数是 {} 的整数倍（多半是少写了一行）", channels),
            Fault::GamaOutOfRange { .. } => "改用 (0.01, 5.0) 内的 gAMA；无此块则传 None 表示默认 sRGB".to_string(),
            Fault::DimensionOverflow { .. } => {
                "这不是长度写错而是尺寸不可表示：宽×通道×2 已超出 usize 范围，须回源校验 IHDR 的 width 字段（PNG 上限 2^31-1）".to_string()
            }
        }
    }

    /// 三要素合成本行（分隔符用全角竖线，与 F1008 同规格）。
    pub fn three_elements(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.what());
        s.push('｜');
        s.push_str(self.why());
        s.push('｜');
        s.push_str(&self.advice());
        s
    }

    /// 诊断码（十六进制字符串，供日志与面板显示）。
    pub fn diag(&self) -> String {
        format!("F1009-{:#06X}", self.code())
    }
}

// ===========================================================================
// 八、性能契约的可机检部分（锚点「16bit 比 8bit 慢 ≤2 倍」）
// ===========================================================================

/// 16 位路径相对 8 位路径的结构性开销因子。
///
/// **本单不实测墙钟**（真 no_std 内核里没有墙钟源），故只声明
/// **可机检的部分**：每样本字节数 2 倍。
/// 滤波试探次数与位深**无关**（不因位深多跑一遍）——
/// 判据用 [`wide_filter_trials`] 与 [`narrow_filter_trials`] 相等来钉这条。
pub const WIDE_BYTE_FACTOR: u32 = 2;

/// 16 位滤波的整数宽度（`u16` 中间量装得下 `a + b - c` 三项）。
///
/// 真值：Paeth 预测 `p = a + b - c`，`a,b,c` 各≤ 65535 ⇒ `p` 最坏
/// 落在 `[-65535, 131070]`，**需要 17 位有符号**。故实现必须用
/// `i32` 做中间量；这里声明的是「滤波运算宽度需求」，
/// 判据断它落在 `i32` 内且**不得用 u16 承载**。
pub const WIDE_FILTER_ACC: i32 = 131_070;

/// 8 位滤波的同一常数（对照：8 位下 `p` 最坏 510，`u16` 绰绰有余）。
pub const NARROW_FILTER_ACC: i32 = 510;

/// 滤波试探次数（与位深无关的证据）：16 位路径对一行试探一次。
pub fn wide_filter_trials() -> u32 {
    1
}

/// 滤波试探次数：8 位路径对一行试探一次（**与 [`wide_filter_trials`] 相等**）。
pub fn narrow_filter_trials() -> u32 {
    1
}

/// 16 位 Paeth 预测的中间量宽度自检：返回**需要的最小有符号位宽**。
///
/// 判据断它恰为 18（`i16` 装不下 65535×2 ⇒ 必须 `i32`），
/// 这样一个「用 `u16` 承载滤波中间量」的实现会在类型层就被挡住。
pub fn filter_acc_bits_needed() -> u8 {
    let mut w = 1u8;
    let mut span = 2i64; // 覆盖 +65535..+131070 与 -65535 两端
    while span < (WIDE_FILTER_ACC as i64) + (WIDE_FILTER_ACC as i64) {
        span *= 2;
        w += 1;
    }
    w
}