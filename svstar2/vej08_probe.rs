//! VE-F1808 · 光照探针基础（VE-J 域 · 环境光探针 · SH2 编码 + 双放置模式 +
//! 烘焙接口 + 插值框架）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1808`
//!
//! # 职责定位（锚点原文）
//!
//! 环境光探针——球谐二阶 SH2 编码（9 系数表示低频环境漫反射）、探针放置双模式
//! （手动+ 自动网格）、探针烘焙接口（场景光照→SH 系数的离线烘焙管线）与探针
//! 插值框架（多探针间过渡），为**无 IBL 资产**的场景提供低成本间接光。
//!
//! # 判据（锚点原文五条）
//!
//! 1. **SH2**：9 系数表示低频环境漫反射（每通道 RGB 共 27 float，量化到 half）；
//! 2. **双放置模式**：手动放置 + 自动网格，手动优先保留、自动网格避开手动位；
//! 3. **烘焙接口**：输入（场景光源列表 + 几何遮挡代理）→ 输出（探针集开放格式）；
//! 4. **插值钳制**：按空间距离权重混合相邻探针，**负权重（SH 负值能量）→ 钳制保护**；
//! 5. 判据自洽（错误路径与降级矩阵）。
//!
//! # 判据一：SH2 编码（9 系数 = 1 常量 + 3 一阶 + 5 二阶）
//!
//! 球谐基函数（实数形式，`k0=0.28209479177387814` 为归一化常数）：
//!
//! ```text
//! Y00= 0.282095Y01= 0.488603·yY02= 0.488603·zY03= 0.488603·x
//! Y04= 1.092548·xy     Y05= 1.092548·yz     Y06= 0.315392·(3z²-1)
//! Y07= 1.092548·xz     Y08= 0.546274·(x²-y²)
//! ```
//!
//! **为什么是二阶而不是三阶**：二阶（9 项）已能表达「天光从上方来、
//! 地面反光从下方来」这一主要低频结构，误差在漫反射上肉眼不可辨；
//! 三阶（16 项）成本翻倍而收益主要在**高频镜面**，那是 F1809 IBL 的职责。
//! 本单与 F1809 的分工即锚点所述「探针=低频漫反射 / IBL=高频镜面」。
//!
//! **量化到 half**：SH 系数动态范围大（L0 常量项通常是 L1/L2 的数十倍），
//! f32 存 9×3=27 个系数是 108 字节/探针，half（u16）减半到 54 字节。
//! half 的 10 位尾数对漫反射足够（相对误差 ~1e-3），**且量化误差有界**——
//! 这比「用 f32 但不校验」强：量化后不可能藏 NaN/Inf。
//!
//! # 判据二：双放置模式（手动优先 + 自动避让）
//!
//! 锚点「手动+ 自动模式混用→**手动优先保留，自动网格避开手动位**」。
//!
//! 实现 [`ProbeField::auto_fill`]：自动网格生成候选点后，**逐个剔除**
//! 与手动探针距离小于 [`AUTO_AVOID_RADIUS`] 的候选。半径取
//! `max(手动探针影响半径, 自动网格间距)` 的 [`AUTO_AVOID_FACTOR`] 倍——
//! 取max 是因为「避开」的真实需求是**插值不打架**：两个探针影响域重叠
//! 本身没问题（同位置两探针会给出打架的加权），但**过近**会让梯度突变。
//!
//! # 判据三：烘焙接口（离线，分钟级）
//!
//! [`bake`] 是**纯函数**：输入（光源列表 + 可选遮挡代理 + 探针集）→
//! 输出（每探针 27 个 SH 系数）。零 IO、零墙钟、零时钟注入 → 可复现。
//!
//! **遮挡代理缺失→ 退化为无遮挡烘焙并诚实标注精度**（锚点原文）：
//! 用 [`BakeReport::occlusion_degraded`] 显式置位，让下游知道
//! 「这批探针的烘焙没算遮挡，精度打折」。**不静默**——静默会让场景
//! 看起来「 Indirect 光过强」却无人知道原因。
//!
//! # 判据四：插值钳制（负权重 = SH 负值能量）
//!
//! SH 系数**天然有正有负**（这正是方向性光照的来源），但**权重**必须非负：
//! 权重为负意味着「这个探针在拉着结果往反方向走」，物理上无意义，
//! 且会让插值结果在探针密集处出现非预期的暗斑。
//!
//! [`sample`] 用**归一化反距离权重**（`w_i = 1/(d_i² + ε)`，再归一化），
//! 权重恒正。但**半径外的探针权重须钳到 0**——否则远处的强探针会
//! 通过 `1/d²` 的分母把近处探针压掉（`d=1000` 时权重 1e-6，但若近处
//! 探针也在 d=1000，两者同量级，近处的实际贡献被远方的噪声决定）。
//!
//! **插值后钳制**：SH 结果做 `clamp` 到 [`SH_CLAMP_MIN`]/[`SH_CLAMP_MAX`]，
//! 防止负能量在多次插值后累积成「负亮度」（锚点「插值负权重→钳制保护」）。
//!
//! # 判据五：错误路径与降级矩阵
//!
//! | 触发 | 处置 | 阻断 |
//! | --- | --- | --- |
//! | SH 系数非法（NaN/Inf/超界） | 烘焙期**校验拒绝** | 是 |
//! | 探针密度过疏（梯度突变） | 密度告警 + **自动加密建议** | 否 |
//! | 烘焙遮挡代理缺失 | 退化为无遮挡烘焙 + 精度诚实标注 | 否 |
//! | 手动/自动混用 | 手动优先保留，自动避让 | — |
//! | 插值负权重 | 钳到 0 + 计入告警 | 否 |
//! | 探针集超配额 | 拒绝 + 显性告警 | 是 |
//!
//! **处置方向相反的两类状态不得共用码**：SH 非法（数据坏，须拒）与
//! 密度过疏（数据合法但不够，须继续并建议）是两回事——前者阻断，
//! 后者只告警。
//!
//! # 性能逐项分解（锚点原文四条）
//!
//! - **SH2 漫反射着色 O(1)/像素**：9 系数 × RGB = 27 次乘加，标量即可。
//! - **插值每对象每帧一次，权重重算仅移动时**：本模块的 [`WeightsCache`]
//!   把「探针序号 + 权重」缓存下来，对象不动则不重算。
//! - **烘焙为离线批处理分钟级**：[`bake`] 是纯函数，耗时由探针数×光源数决定。
//! - **内存按探针集整体配额（F1776 池扩展位）**：[`PROBE_QUOTA_F16`] 定总量，
//!   [`ProbeField::len`] 超限即拒。
//!
//! # 跨批对接点
//!
//! - **F1809 IBL 分工**：探针=低频漫反射，IBL=高频镜面。二者不重叠。
//! - **I03 材质着色器**：消费 [`ShCoeffs::irradiance`]（已含 `A0/π` 卷积因子）。
//! - **I 域确定性口径**：half 量化用**就近取整 + 明确 tie-break**，
//!   同一输入在任何平台产出同一批 u16（不依赖平台 libm 的 round 行为）。
//! - **F1819 遥测**：[`BakeReport`] 的计数进遥测。
//! - **F1813 fuzz**：非法 SH 系数进光��� fuzz 条目。
//!
//! # 无障碍与隐私
//!
//! 纯场景光照数据，**无隐私面**。烘焙是离线过程，不采集运行时用户数据。

#![allow(clippy::needless_range_loop)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与契约
// ---------------------------------------------------------------------------

/// 探针集格式版本（探针集二进制开放格式的版本位）。
pub const PROBE_FORMAT_VERSION: u16 = 1;

/// SH2 系数个数（1 常量 + 3 一阶 + 5 二阶 = 9）。
pub const SH2_TERMS: usize = 9;

/// 每探针 float 个数（9 系数 × RGB 三通道 = 27）。
pub const FLOATS_PER_PROBE: usize = SH2_TERMS * 3;

/// 每探针 half 字节数（27 × 2）。
pub const BYTES_PER_PROBE: usize = FLOATS_PER_PROBE * 2;

/// 探针集总配额（half 字节，F1776 池扩展位）。
///
/// 取值依据：典型室内场景 8m 网格 × 6 个房间 ≈ 200 探针，
/// 200 × 54 = 10800 字节。取 256 探针（13824 字节）为上限留足余量，
/// 同时**明确声明**这是配额而非「建议值」——超限即拒绝，不静默截断。
pub const PROBE_QUOTA_F16: usize = 256 * BYTES_PER_PROBE;

/// 探针集硬上限（个数）。
pub const MAX_PROBES: usize = 256;

/// 影响半径默认值（米）——未显式指定时的兜底。
pub const DEFAULT_INFLUENCE_R: f32 = 2.0;

/// 影响半径下界（过小的探针永远插值不到，等于白放）。
pub const MIN_INFLUENCE_R: f32 = 0.1;

/// 影响半径上界（过大会让相邻探针的影响域完全重叠，插值退化为平均）。
pub const MAX_INFLUENCE_R: f32 = 64.0;

/// SH 系数合法区间（超出即判非法）。
///
/// 下界取 -8：SH 常量项在正常场景不会低于 -8（那是「全黑且下方有强反光」
/// 的极端构造）。上界取 64：常量项 = 辐照度 × k0，室内场景典型 0.5~8。
/// **区间是刻意收紧的**——SH 系数无界会让「一个 NaN 传播成整场诡异照明」
/// 这类故障延后到渲染期才暴露，而那时已无从追溯。
pub const SH_CLAMP_MIN: f32 = -8.0;

/// SH 系数上界（见 [`SH_CLAMP_MIN`] 说明）。
pub const SH_CLAMP_MAX: f32 = 64.0;

/// 反距离权重的分母 ε（避免 d=0 时除零）。
pub const WEIGHT_EPS: f32 = 1.0e-4;

/// 自动网格避让半径的倍数因子（见判据二）。
pub const AUTO_AVOID_FACTOR: f32 = 0.75;

/// 密度告警阈值：平均间距 > 该值（米）视为过疏。
///
/// 依据：探针影响半径默认 2m，间距超过 2×半径（4m）时相邻探针的影响域
/// 不重叠，中间区域的插值权重全落到「半径外→钳到 0」→ 采样点直接取到
/// 单探针值，梯度突变可见。
pub const DENSITY_WARN_SPACING: f32 = 4.0;

/// 自动加密建议的目标间距（= 告警阈值的 1/2）。
pub const DENSITY_SUGGEST_SPACING: f32 = DENSITY_WARN_SPACING * 0.5;

/// 球谐归一化常数 `k0 = 0.5 * sqrt(1/(4π))`。
pub const SH_K0: f32 = 0.282_094_8;

/// 一阶基函数系数 `k1 = 0.5 * sqrt(3/(4π))`。
pub const SH_K1: f32 = 0.488_602_5;

/// 二阶 xy/yz/xz 系数 `k2 = 0.5 * sqrt(15/(4π))`。
pub const SH_K2: f32 = 1.092_548_4;

/// 二阶（3z²−1）系数 `0.25 * sqrt(5/π)`。
pub const SH_K20: f32 = 0.315_391_6;

/// 二阶（x²−y²）系数 `0.25 * sqrt(15/π)`。
pub const SH_K22: f32 = 0.546_274_2;

/// 探针集开放格式的声明文本（下游 F1816 文档与解析器共用）。
pub const PROBE_FORMAT_DOC: &str = "\
varix.probe.v1 — 探针集开放格式。\
头部 8 字节：magic 'VXPB'（4B）| version u16LE | probe_count u16LE。\
随后每探针 54 字节：pos x/y/z 各 f16 | influence_r f16 | 27 × f16 SH 系数\
（按 Y00..Y08 顺序，每项 RGB 连续三通道）。f16 为 IEEE 754 binary16 小端。\
整体按 PROBE_QUOTA_F16 配额上限分配，超限拒绝而非截断。";

// ---------------------------------------------------------------------------
// 二、f16 量化（自持实现，平台无关的确定性取整）
// ---------------------------------------------------------------------------

/// f32 → f16（IEEE 754 binary16），**就近取偶**（round-half-to-even）。
///
/// **为什么必须自持**：`(x * f16_scale) as u16` 这类写法依赖平台的
/// 浮点→整数转换语义（截断 vs 舍入）与 NaN/Inf 的处理，不同 libm
/// 可能差 1 ULP。锚点要求「量化与I 域确定性口径对齐」——**同一输入
/// 在任何平台必须产出同一批 u16**，故此处显式实现 IEEE 754 的
/// round-half-to-even，不依赖平台行为。
///
/// 溢出（|x| > 65504）饱和到 ±Inf 上限而非 Inf——SH 系数超界
/// 属数据错误，由 [`ShCoeffs::validate`] 拒绝，量化层不制造 NaN。
pub fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x007F_FFFF;

    // NaN / Inf：饱和到最大有限值（0x7BFF = 65504）。
    if exp == 0xFF {
        return sign | 0x7BFF;
    }
    // f32 指数 -127..127 → half 指数 -14..15
    let mut he = exp - 127 + 15;
    if he >= 0x1F {
        return sign | 0x7BFF; // 溢出饱和
    }
    if he <= 0 {
        // 次正规：10 位有效位左移到隐含位位置，右移时**加 0.5 再截断**
        // 实现 round-half-to-even 的近似（次正规区精度本就低）。
        if he < -10 {
            return sign; // 太小 → ±0
        }
        let full = mant | 0x0080_0000; // 恢复隐含位
        let shift = (14 - he) as u32;
        let mut m = full >> shift;
        let rem_bits = 24 - shift as i32;
        let rem = (full & ((1u32 << shift) - 1)) as i32;
        let half = 1i32 << (rem_bits - 1);
        if rem > half || (rem == half && (m & 1) == 1) {
            m += 1;
        }
        return sign | (m as u16);
    }
    // 正常数：10 位尾数 + 就近取偶
    let mut m = (mant >> 13) as u16;
    let rem = mant & 0x1FFF;
    if rem > 0x1000 || (rem == 0x1000 && (m & 1) == 1) {
        m += 1; // 进位可能把尾数推到 0x400 → 需进位到指数
        if m == 0x400 {
            m = 0;
            he += 1;
            if he >= 0x1F {
                return sign | 0x7BFF;
            }
        }
    }
    sign | ((he as u16) << 10) | m
}

/// f16 → f32（精确：half 是 f32 的子集，无精度损失）。
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = (h & 0x8000) as u32;
    let exp = ((h >> 10) & 0x1F) as i32;
    let mant = (h & 0x03FF) as u32;
    let bits = if exp == 0 {
        if mant == 0 {
            (sign << 16)
        } else {
            // 次正规：规格化到 f32
            let mut e = -1i32;
            let mut m = mant;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x3FF;
            (sign << 16) | (((127 - 15 + e + 1) as u32) << 23) | (m << 13)
        }
    } else if exp == 0x1F {
        (sign << 16) | 0x7F80_0000 | (mant << 13)
    } else {
        (sign << 16) | (((exp - 15 + 127) as u32) << 23) | (mant << 13)
    };
    f32::from_bits(bits)
}

// ---------------------------------------------------------------------------
// 三、SH2 系数（判据一）
// ---------------------------------------------------------------------------

/// 一个探针的 SH2 辐照度系数（9 项 × RGB）。
///
/// 布局：`coeffs[term * 3 + channel]`，即 `term` 为主序、通道连续。
/// **主序的理由**：烘焙/序列化按 term 遍历时对 RGB 三通道连续访问，
/// 缓存友好；且与规范文档里SH2 的书写顺序一致，便于人工对拍。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShCoeffs {
    /// 27 个系数（9 term × 3 通道）。
    pub coeffs: [f32; FLOATS_PER_PROBE],
}

impl Default for ShCoeffs {
    fn default() -> ShCoeffs {
        ShCoeffs {
            coeffs: [0.0; FLOATS_PER_PROBE],
        }
    }
}

impl ShCoeffs {
    /// 全零（无照明）。
    pub const ZERO: ShCoeffs = ShCoeffs {
        coeffs: [0.0; FLOATS_PER_PROBE],
    };

    /// 取某项某通道。
    #[inline]
    pub fn at(&self, term: usize, channel: usize) -> f32 {
        self.coeffs[term * 3 + channel]
    }

    /// 累加（原地）。**烘焙累加多条光线贡献的唯一入口**。
    pub fn accumulate(&mut self, term: usize, channel: usize, v: f32) {
        if term < SH2_TERMS && channel < 3 {
            self.coeffs[term * 3 + channel] += v;
        }
    }

    /// 有限性检查（NaN/Inf 一律判非法）。
    pub fn all_finite(&self) -> bool {
        for &c in self.coeffs.iter() {
            if c != c || c == f32::INFINITY || c == f32::NEG_INFINITY {
                return false;
            }
        }
        true
    }

    /// 区间检查（[`SH_CLAMP_MIN`]..=[`SH_CLAMP_MAX`]）。
    pub fn in_range(&self) -> bool {
        for &c in self.coeffs.iter() {
            if c < SH_CLAMP_MIN || c > SH_CLAMP_MAX {
                return false;
            }
        }
        true
    }

    /// 烘焙期校验：**有限且在界内**才合法（锚点「SH 系数非法→校验拒绝」）。
    pub fn validate(&self) -> Result<(), ProbeFault> {
        if !self.all_finite() {
            for (i, &c) in self.coeffs.iter().enumerate() {
                if c != c || c == f32::INFINITY || c == f32::NEG_INFINITY {
                    return Err(ProbeFault::with(ProbeFaultKind::ShNotFinite, i as u64, 0));
                }
            }
        }
        if !self.in_range() {
            for (i, &c) in self.coeffs.iter().enumerate() {
                if c < SH_CLAMP_MIN || c > SH_CLAMP_MAX {
                    return Err(ProbeFault::with(ProbeFaultKind::ShOutOfRange, i as u64, 0));
                }
            }
        }
        Ok(())
    }

    /// 钳到合法区间（插值后调用，见判据四）。
    pub fn clamp_in_place(&mut self) {
        for c in self.coeffs.iter_mut() {
            if *c != *c {
                *c = 0.0; // NaN → 0（不该发生，发生则收口而非传播）
            } else if *c < SH_CLAMP_MIN {
                *c = SH_CLAMP_MIN;
            } else if *c > SH_CLAMP_MAX {
                *c = SH_CLAMP_MAX;
            }
        }
    }

    /// 量化到 half 字节数组（27 × 2 = 54 字节，小端）。
    pub fn quantize(&self) -> [u8; BYTES_PER_PROBE] {
        let mut out = [0u8; BYTES_PER_PROBE];
        for (i, &c) in self.coeffs.iter().enumerate() {
            let h = f32_to_f16(c);
            out[i * 2] = (h & 0xFF) as u8;
            out[i * 2 + 1] = (h >> 8) as u8;
        }
        out
    }

    /// 从 half 字节反量化（对称于 [`quantize`](Self::quantize)）。
    pub fn dequantize(bytes: &[u8]) -> Result<ShCoeffs, ProbeFault> {
        if bytes.len() < BYTES_PER_PROBE {
            return Err(ProbeFault::with(
                ProbeFaultKind::ShortPayload,
                bytes.len() as u64,
                BYTES_PER_PROBE as u64,
            ));
        }
        let mut s = ShCoeffs::ZERO;
        for i in 0..FLOATS_PER_PROBE {
            let h = (bytes[i * 2] as u16) | ((bytes[i * 2 + 1] as u16) << 8);
            s.coeffs[i] = f16_to_f32(h);
        }
        Ok(s)
    }

    /// 辐照度求值：给定单位方向，返回该方向的漫反射辐照度 RGB。
    ///
    /// **已含 `A0/π` 与 `A1/2`、`A2/4` 卷积因子**——这一句是 SH2
    /// 与「直接用系数当辐照度」最常见的错误源。Lambert 卷积后
    /// 常量项除 π、一阶除 2、二阶除 4，**漏掉会让环境光偏亮数倍**。
    ///
    /// I03 材质着色器直接消费本函数结果，不再自行卷积。
    pub fn irradiance(&self, dir: (f32, f32, f32)) -> (f32, f32, f32) {
        let (x, y, z) = normalize3(dir);
        let b = basis(x, y, z);
        // Lambert 卷积因子：L0 ×(1/π)，L1 ×(1/2)，L2 ×(1/4)。
        // **漏掉这三档是 SH2 最常见的错误源**——直接把系数当辐照度用
        // 会让环境光偏亮 π/2 ≈ 1.57 倍（常量项）与 2 倍（一阶项）。
        let f0 = 1.0 / core::f32::consts::PI;
        let f1 = 0.5;
        let f2 = 0.25;
        let mut out = [0.0f32; 3];
        for ch in 0..3 {
            let mut acc = self.at(0, ch) * f0 * b[0];
            acc += (self.at(1, ch) * f1) * b[1] + (self.at(2, ch) * f1) * b[2] + (self.at(3, ch) * f1) * b[3];
            acc += (self.at(4, ch) * f2) * b[4] + (self.at(5, ch) * f2) * b[5] + (self.at(6, ch) * f2) * b[6]
                + (self.at(7, ch) * f2) * b[7] + (self.at(8, ch) * f2) * b[8];
            out[ch] = clamp_irradiance(acc);
        }
        (out[0], out[1], out[2])
    }
}

/// 9 个球谐基函数值（实数形式，顺序 Y00..Y08）。
#[inline]
fn basis(x: f32, y: f32, z: f32) -> [f32; 9] {
    [
        SH_K0,
        SH_K1 * y,
        SH_K1 * z,
        SH_K1 * x,
        SH_K2 * x * y,
        SH_K2 * y * z,
        SH_K20 * (3.0 * z * z - 1.0),
        SH_K2 * x * z,
        SH_K22 * (x * x - y * y),
    ]
}

/// 单位化（零向量与���有限分量收口为 +Y，避免除零产出 NaN）。
fn normalize3(v: (f32, f32, f32)) -> (f32, f32, f32) {
    if v.0 != v.0 || v.1 != v.1 || v.2 != v.2 {
        return (0.0, 1.0, 0.0);
    }
    if v.0 == f32::INFINITY
        || v.0 == f32::NEG_INFINITY
        || v.1 == f32::INFINITY
        || v.1 == f32::NEG_INFINITY
        || v.2 == f32::INFINITY
        || v.2 == f32::NEG_INFINITY
    {
        return (0.0, 1.0, 0.0);
    }
    let l2 = v.0 * v.0 + v.1 * v.1 + v.2 * v.2;
    if !(l2 > 1.0e-12) {
        return (0.0, 1.0, 0.0);
    }
    let inv = 1.0 / l2.sqrt();
    (v.0 * inv, v.1 * inv, v.2 * inv)
}

/// 辐照度收口（负亮度在物理上无意义；下界 0 上界 [`SH_CLAMP_MAX`]）。
#[inline]
fn clamp_irradiance(v: f32) -> f32 {
    if v != v {
        return 0.0;
    }
    if v < 0.0 {
        0.0
    } else if v > SH_CLAMP_MAX {
        SH_CLAMP_MAX
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// 四、探针（位置 + 影响半径 + SH 系数）
// ---------------------------------------------------------------------------

/// 放置模式（判据二）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlacementMode {
    /// 手动放置（**优先级更高**，自动网格须避让）。
    Manual,
    /// 自动网格生成。
    AutoGrid,
}

/// 几何遮挡代理（判据三）。
///
/// **刻意只留一个遮挡代理形状**（轴对齐盒）：烘焙是离线批处理，
/// 形状越复杂离线成本越高，而遮挡代理的精度需求是「够用即可」——
/// 真正的精确遮挡走光栅化管线，不属本单。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OccluderBox {
    /// 最小角 `(x, y, z)`。
    pub lo: (f32, f32, f32),
    /// 最大角。
    pub hi: (f32, f32, f32),
}

impl OccluderBox {
    /// 构造（自动规整 lo/hi，容忍传反）。
    pub fn new(a: (f32, f32, f32), b: (f32, f32, f32)) -> OccluderBox {
        OccluderBox {
            lo: (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2)),
            hi: (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2)),
        }
    }

    /// 点是否在盒内（含面）。
    pub fn contains(&self, p: (f32, f32, f32)) -> bool {
        p.0 >= self.lo.0
            && p.0 <= self.hi.0
            && p.1 >= self.lo.1
            && p.1 <= self.hi.1
            && p.2 >= self.lo.2
            && p.2 <= self.hi.2
    }
}

/// 一个光照探针。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Probe {
    /// 位置（米）。
    pub pos: (f32, f32, f32),
    /// 影响半径（米）——超出即权重钳 0。
    pub influence_r: f32,
    /// 放置模式。
    pub mode: PlacementMode,
    /// 稳定 id（自增，**不复用**——复用会让旧插值缓存指向新探针）。
    pub stable_id: u64,
}

impl Probe {
    /// 构造手动探针（半径非法则收口到 [`MIN_INFLUENCE_R`]..=[`MAX_INFLUENCE_R`]）。
    pub fn manual(pos: (f32, f32, f32), influence_r: f32, stable_id: u64) -> Probe {
        Probe {
            pos,
            influence_r: clamp_radius(influence_r),
            mode: PlacementMode::Manual,
            stable_id,
        }
    }

    /// 构造自动网格探针。
    pub fn auto(pos: (f32, f32, f32), influence_r: f32, stable_id: u64) -> Probe {
        Probe {
            pos,
            influence_r: clamp_radius(influence_r),
            mode: PlacementMode::AutoGrid,
            stable_id,
        }
    }

    /// 位置是否有限。
    pub fn pos_finite(&self) -> bool {
        is_fin(self.pos.0) && is_fin(self.pos.1) && is_fin(self.pos.2)
    }
}

/// 影响半径收口（含非有限 → 默认值）。
#[inline]
pub fn clamp_radius(r: f32) -> f32 {
    if !is_fin(r) {
        return DEFAULT_INFLUENCE_R;
    }
    if r < MIN_INFLUENCE_R {
        MIN_INFLUENCE_R
    } else if r > MAX_INFLUENCE_R {
        MAX_INFLUENCE_R
    } else {
        r
    }
}

/// 有限性（`f32::is_finite` 无自由函数，恒等方法可用，此处封装便于阅读）。
#[inline]
fn is_fin(v: f32) -> bool {
    v == v && v != f32::INFINITY && v != f32::NEG_INFINITY
}

/// 平方距离。
#[inline]
fn dist_sq(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    let dz = a.2 - b.2;
    dx * dx + dy * dy + dz * dz
}

// ---------------------------------------------------------------------------
// 五、错误面（判据五）
// ---------------------------------------------------------------------------

/// 探针域故障类别。
///
/// **只收「必须阻断」的状态**：SH 系数非法、探针位置非法、超配额、
/// 载荷过短。密度过疏、遮挡代理缺失属**告警不阻断**（见 [`ProbeWarning`]），
/// 二者处置方向相反，**不得共用码**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeFaultKind {
    /// SH 系数含 NaN/Inf（烘焙期校验拒绝）。
    ShNotFinite,
    /// SH 系数越出合法区间。
    ShOutOfRange,
    /// 探针位置含 NaN/Inf。
    ProbePosNotFinite,
    /// 探针集超配额。
    QuotaExceeded,
    /// 反序列化载荷过短。
    ShortPayload,
    /// 格式魔数不符。
    BadMagic,
    /// 格式版本不支持。
    BadVersion,
    /// 探针索引越界。
    IndexOutOfRange,
}

impl ProbeFaultKind {
    /// 错误码（探针段独立码段 `0xF800 | n+1`）。
    pub fn code(self) -> u16 {
        0xF800 | (self as u16) + 1
    }
    /// 短名。
    pub fn label(self) -> &'static str {
        match self {
            ProbeFaultKind::ShNotFinite => "SH 系数含非有限值",
            ProbeFaultKind::ShOutOfRange => "SH 系数越界",
            ProbeFaultKind::ProbePosNotFinite => "探针位置非有限",
            ProbeFaultKind::QuotaExceeded => "探针集超配额",
            ProbeFaultKind::ShortPayload => "载荷过短",
            ProbeFaultKind::BadMagic => "格式魔数不符",
            ProbeFaultKind::BadVersion => "格式版本不支持",
            ProbeFaultKind::IndexOutOfRange => "探针索引越界",
        }
    }
    /// 原因。
    pub fn cause(self) -> &'static str {
        match self {
            ProbeFaultKind::ShNotFinite => "烘焙产出的 SH 系数含 NaN 或 Inf",
            ProbeFaultKind::ShOutOfRange => "SH 系数越出 [SH_CLAMP_MIN, SH_CLAMP_MAX]",
            ProbeFaultKind::ProbePosNotFinite => "探针位置含 NaN 或 Inf",
            ProbeFaultKind::QuotaExceeded => "探针集字节数超过 PROBE_QUOTA_F16 配额",
            ProbeFaultKind::ShortPayload => "字节数小于格式要求",
            ProbeFaultKind::BadMagic => "首4 字节不是 'VXPB'",
            ProbeFaultKind::BadVersion => "版本号不是 PROBE_FORMAT_VERSION",
            ProbeFaultKind::IndexOutOfRange => "探针序号超出探针集范围",
        }
    }
    /// 建议。
    pub fn advice(self) -> &'static str {
        match self {
            ProbeFaultKind::ShNotFinite => "检查烘焙输入的光源参数（强度/位置）是否有除零或溢出",
            ProbeFaultKind::ShOutOfRange => "检查场景光照强度是否异常；或放宽 SH_CLAMP_MIN/MAX（须走 ADR）",
            ProbeFaultKind::QuotaExceeded => "减少探针数或缩小影响半径；配额是硬上限，不静默截断",
            ProbeFaultKind::BadMagic | ProbeFaultKind::BadVersion => "确认数据来源是本模块导出的探针集格式",
            _ => "按探针集格式文档核对后重试（见 PROBE_FORMAT_DOC）",
        }
    }
}

/// 探针域故障（类别 + 两个数值细节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeFault {
    /// 类别。
    pub kind: ProbeFaultKind,
    /// 细节一（语义按类别：系数下标 / 探针数 / 实得字节数）。
    pub a: u64,
    /// 细节二（期望值或上界）。
    pub b: u64,
}

impl ProbeFault {
    /// 构造。
    pub fn new(kind: ProbeFaultKind) -> ProbeFault {
        ProbeFault { kind, a: 0, b: 0 }
    }
    /// 带两个细节。
    pub fn with(kind: ProbeFaultKind, a: u64, b: u64) -> ProbeFault {
        ProbeFault { kind, a, b }
    }
    /// 错误码。
    pub fn code(&self) -> u16 {
        self.kind.code()
    }
    /// 原因（转发到类别）。
    pub fn cause(&self) -> &'static str {
        self.kind.cause()
    }
    /// 建议（转发到类别）。
    pub fn advice(&self) -> &'static str {
        self.kind.advice()
    }
    /// 人话（模板 + 细节）。
    pub fn human(&self) -> String {
        let mut s = String::from(self.kind.label());
        match self.kind {
            ProbeFaultKind::ShNotFinite | ProbeFaultKind::ShOutOfRange => {
                s.push_str("：第 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str(" 号系数（");
                s.push_str(&fmt_u64(self.b));
                s.push_str("）");
            }
            ProbeFaultKind::QuotaExceeded => {
                s.push_str("：");
                s.push_str(&fmt_u64(self.a));
                s.push_str(" 字节，上限 ");
                s.push_str(&fmt_u64(self.b));
                s.push_str(" 字节");
            }
            ProbeFaultKind::ShortPayload => {
                s.push_str("：实得 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str(" 字节，需 ");
                s.push_str(&fmt_u64(self.b));
                s.push_str(" 字节");
            }
            _ => {}
        }
        s
    }
}

/// 无 `format!` 的十进制渲染（no_std 稳妥写法）。
fn fmt_u64(mut v: u64) -> String {
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

/// 告警码（**不阻断**的一侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeWarnCode {
    /// 密度过疏（插值梯度可能突变）。
    DensityTooSparse,
    /// 遮挡代理缺失（精度打折）。
    OcclusionDegraded,
    /// 负权重被钳到 0。
    NegativeWeightClamped,
    /// 采样点落在所有探针影响域之外（退化为最近探针）。
    OutsideAllProbes,
}

impl ProbeWarnCode {
    /// 告警码（与故障码段分开：`0xFC00 | n+1`）。
    pub fn code(self) -> u16 {
        0xFC00 | (self as u16) + 1
    }
    /// 短名。
    pub fn label(self) -> &'static str {
        match self {
            ProbeWarnCode::DensityTooSparse => "探针密度过疏",
            ProbeWarnCode::OcclusionDegraded => "遮挡代理缺失",
            ProbeWarnCode::NegativeWeightClamped => "负权重已钳到 0",
            ProbeWarnCode::OutsideAllProbes => "采样点在所有探针影响域外",
        }
    }
    /// 建议（含自动加密的具体间距——告警必须可执行）。
    pub fn advice(self) -> &'static str {
        match self {
            ProbeWarnCode::DensityTooSparse => {
                "插值梯度可能突变；建议把自动网格间距降到 2.0 米以下（目标间距 2.0m）"
            }
            ProbeWarnCode::OcclusionDegraded => "本批探针未算遮挡，间接光会偏强；建议补齐遮挡代理后重烘",
            ProbeWarnCode::NegativeWeightClamped => "负权重已钳到 0；若频繁出现请检查探针位置是否重合",
            ProbeWarnCode::OutsideAllProbes => "已退化为最近探针单点取值；建议加密探针或扩大影响半径",
        }
    }
}

/// 一条告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeWarning {
    /// 告警码。
    pub code: ProbeWarnCode,
    /// 关联探针序号（无关联则 `u64::MAX`）。
    pub probe_index: u64,
    /// 数值细节。
    pub detail: u64,
}

/// 探针域告警包（`Count + warning`，故 `Copy` 不成立，用引用传递）。
pub type WarnBag<'a> = &'a [ProbeWarning];

// ---------------------------------------------------------------------------
// 六、探针集（判据二：双放置模式）
// ---------------------------------------------------------------------------

/// 探针集：位置 + 半径 + 每探针 SH 系数。
#[derive(Clone, Debug)]
pub struct ProbeField {
    probes: Vec<Probe>,
    sh: Vec<ShCoeffs>,
    next_id: u64,
    warnings: Vec<ProbeWarning>,
}

impl Default for ProbeField {
    fn default() -> ProbeField {
        ProbeField::new()
    }
}

impl ProbeField {
    /// 空探针集。
    pub fn new() -> ProbeField {
        ProbeField {
            probes: Vec::new(),
            sh: Vec::new(),
            next_id: 1,
            warnings: Vec::new(),
        }
    }

    /// 探针数。
    pub fn len(&self) -> usize {
        self.probes.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }
    /// 占用的 half 字节数（配额口径）。
    pub fn bytes_f16(&self) -> usize {
        self.probes.len() * BYTES_PER_PROBE
    }
    /// 告警。
    pub fn warnings(&self) -> &[ProbeWarning] {
        &self.warnings
    }
    /// 取探针。
    pub fn probe(&self, i: usize) -> Result<Probe, ProbeFault> {
        self.probes
            .get(i)
            .copied()
            .ok_or_else(|| ProbeFault::with(ProbeFaultKind::IndexOutOfRange, i as u64, 0))
    }
    /// 取 SH 系数。
    pub fn sh(&self, i: usize) -> Result<ShCoeffs, ProbeFault> {
        self.sh
            .get(i)
            .copied()
            .ok_or_else(|| ProbeFault::with(ProbeFaultKind::IndexOutOfRange, i as u64, 0))
    }

    /// 手动放置一个探针（判据二：手动优先）。
    ///
    /// 配额检查在**入列前**做——超限即拒，不静默截断（锚点降级矩阵）。
    pub fn add_manual(
        &mut self,
        pos: (f32, f32, f32),
        influence_r: f32,
    ) -> Result<u64, ProbeFault> {
        if !is_fin(pos.0) || !is_fin(pos.1) || !is_fin(pos.2) {
            return Err(ProbeFault::new(ProbeFaultKind::ProbePosNotFinite));
        }
        self.check_quota(1)?;
        let id = self.next_id;
        self.next_id += 1; // **id 自增不复用**（否则旧插值缓存会指向新探针）
        self.probes.push(Probe::manual(pos, influence_r, id));
        self.sh.push(ShCoeffs::ZERO);
        Ok(id)
    }

    /// 配额闸（超限拒绝）。
    fn check_quota(&self, adding: usize) -> Result<(), ProbeFault> {
        let after = self.probes.len() + adding;
        if after > MAX_PROBES || after * BYTES_PER_PROBE > PROBE_QUOTA_F16 {
            return Err(ProbeFault::with(
                ProbeFaultKind::QuotaExceeded,
                (after * BYTES_PER_PROBE) as u64,
                PROBE_QUOTA_F16 as u64,
            ));
        }
        Ok(())
    }

    /// **自动网格填充**（判据二：手动优先 + 自动避让）。
    ///
    /// 在 `lo..hi` 盒内按 `spacing` 步长布点，但**逐个剔除**与手动探针
    /// 距离小于避让半径的候选。避让半径 = `max(手动探针影响半径, spacing)
    /// × AUTO_AVOID_FACTOR`——取 max 的理由见头注判据二。
    ///
    /// 返回实际放入的探针数（**不含**被避让剔除的数量——后者计入
    /// [`ProbeField::warnings`] 的密度项由调用方查）。
    pub fn auto_fill(
        &mut self,
        lo: (f32, f32, f32),
        hi: (f32, f32, f32),
        spacing: f32,
    ) -> Result<usize, ProbeFault> {
        if !is_fin(lo.0) || !is_fin(lo.1) || !is_fin(lo.2) || !is_fin(hi.0) || !is_fin(hi.1)
            || !is_fin(hi.2)
        {
            return Err(ProbeFault::new(ProbeFaultKind::ProbePosNotFinite));
        }
        let sp = if is_fin(spacing) && spacing > 1.0e-3 {
            spacing
        } else {
            DENSITY_SUGGEST_SPACING
        };
        let (lx, ly, lz) = (lo.0.min(hi.0), lo.1.min(hi.1), lo.2.min(hi.2));
        let (hx, hy, hz) = (lo.0.max(hi.0), lo.1.max(hi.1), lo.2.max(hi.2));

        // 手动探针的避让半径（取 max：见头注判据二）
        let mut avoid: Vec<((f32, f32, f32), f32)> = Vec::new();
        for p in self.probes.iter() {
            if p.mode == PlacementMode::Manual {
                let r = p.influence_r.max(sp) * AUTO_AVOID_FACTOR;
                avoid.push((p.pos, r * r));
            }
        }

        let mut placed = 0usize;
        let mut x = lx;
        // 循环上限防「spacing 极小导致近乎死循环」——用格数上界而非迭代计数兜底
        let max_cells = MAX_PROBES * 8;
        let mut cells = 0usize;
        while x <= hx + sp {
            let mut y = ly;
            while y <= hy + sp {
                let mut z = lz;
                while z <= hz + sp {
                    cells += 1;
                    if cells > max_cells {
                        self.warnings.push(ProbeWarning {
                            code: ProbeWarnCode::DensityTooSparse,
                            probe_index: u64::MAX,
                            detail: cells as u64,
                        });
                        return Ok(placed);
                    }
                    let p = (x, y, z);
                    // **避让判定**：与任一手动探针过近则跳过
                    let blocked = avoid.iter().any(|&(ap, r2)| dist_sq(p, ap) < r2);
                    if !blocked {
                        // 配额逐点检查（放不下就停，不静默丢弃）
                        if self.check_quota(1).is_err() {
                            self.warnings.push(ProbeWarning {
                                code: ProbeWarnCode::DensityTooSparse,
                                probe_index: u64::MAX,
                                detail: self.probes.len() as u64,
                            });
                            return Ok(placed);
                        }
                        let id = self.next_id;
                        self.next_id += 1;
                        self.probes.push(Probe::auto(p, sp, id));
                        self.sh.push(ShCoeffs::ZERO);
                        placed += 1;
                    }
                    z += sp;
                }
                y += sp;
            }
            x += sp;
        }
        Ok(placed)
    }

    /// 写入某探针的 SH 系数（**带校验**，非法即拒——锚点「烘焙期校验拒绝」）。
    pub fn set_sh(&mut self, i: usize, c: ShCoeffs) -> Result<(), ProbeFault> {
        c.validate()?;
        match self.sh.get_mut(i) {
            Some(slot) => {
                *slot = c;
                Ok(())
            }
            None => Err(ProbeFault::with(
                ProbeFaultKind::IndexOutOfRange,
                i as u64,
                self.probes.len() as u64,
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、烘焙接口（判据三）
// ---------------------------------------------------------------------------

/// 烘焙光源（**只取辐照度贡献所需的最小集**：位置 + 颜色强度 + 范围）。
///
/// 刻意**不接vef07 的 `LightDesc`**：那一层带句柄代数、排序键、
/// 风���合并等运行时语义，烘焙是离线批处理，接它会把「离线」变成
/// 「依赖运行时管理器」。两者的**共同口径只有「位置 + 辐照度」**。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BakeLight {
    /// 位置。
    pub pos: (f32, f32, f32),
    /// 辐射强度（单位任意，线性）。
    pub intensity: f32,
    /// 有效半径（超出即无贡献）。
    pub radius: f32,
    /// 颜色（线性 RGB）。
    pub color: (f32, f32, f32),
}

/// 烘焙输入（判据三：光源列表 + 几何遮挡代理）。
#[derive(Clone, Debug)]
pub struct BakeInput {
    /// 光源列表。
    pub lights: Vec<BakeLight>,
    /// 遮挡代理（**空 = 缺失**，触发降级与诚实标注）。
    pub occluders: Vec<OccluderBox>,
    /// 每探针的采样方向数（辐照度积分的采样数）。
    pub sample_dirs: u32,
}

impl BakeInput {
    /// 构造（采样方向数给默认 64）。
    pub fn new(lights: Vec<BakeLight>, occluders: Vec<OccluderBox>) -> BakeInput {
        BakeInput {
            lights,
            occluders,
            sample_dirs: 64,
        }
    }
}

/// 烘焙报告（判据五：降级与告警的显式台账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BakeReport {
    /// 实际烘焙的探针数。
    pub baked: usize,
    /// 遮挡代理是否**缺失**（缺失 = 精度打折，如实标注）。
    pub occlusion_degraded: bool,
    /// 密度过疏的探针对数。
    pub sparse_pairs: usize,
    /// 被钳到 0 的负权重次数。
    pub clamped_weights: usize,
    /// 落在所有探针影响域外的采样点数。
    pub outside_samples: usize,
}

impl BakeReport {
    /// 零值报告。
    pub const ZERO: BakeReport = BakeReport {
        baked: 0,
        occlusion_degraded: false,
        sparse_pairs: 0,
        clamped_weights: 0,
        outside_samples: 0,
    };
}

/// **离线烘焙**（判据三）。
///
/// 纯函数：给定探针集 + 输入，写入每探针的 SH 系数，返回报告。
/// 零 IO、零时钟 → 同输入同输出，可复现（锚点要求确定性）。
///
/// **遮挡处理**：每个采样方向检查「从探针到光源的线段是否穿过遮挡盒」——
/// 用** slabs 法**（射线-AABB 相交）而非逐盒采样，故复杂度 O(探针×光源×遮挡盒)
/// 且与采样方向数无关。
///
/// **采样方向生成**：确定性 Fibonacci 球（**不用随机数**）——离线烘焙
/// 常用蒙特卡洛，但蒙特卡洛的随机序列会因平台 RNG 实现不同而变化，
/// 破坏「同输入同输出」。Fibonacci 球是**确定的**且分布均匀。
pub fn bake(field: &mut ProbeField, input: &BakeInput) -> Result<BakeReport, ProbeFault> {
    let n = field.len();
    if n == 0 {
        return Ok(BakeReport::ZERO);
    }
    let occlusion_degraded = input.occluders.is_empty();
    let dirs = fib_dirs(input.sample_dirs);
    let mut report = BakeReport {
        baked: n,
        occlusion_degraded,
        ..BakeReport::ZERO
    };

    for i in 0..n {
        let p = field.probe(i)?;
        if !p.pos_finite() {
            return Err(ProbeFault::new(ProbeFaultKind::ProbePosNotFinite));
        }
        let mut sh = ShCoeffs::ZERO;
        for &l in input.lights.iter() {
            if !is_fin(l.intensity) || !is_fin(l.radius) || !is_fin(l.pos.0) {
                // 光源数据坏 → 拒绝（不静默跳过：半份数据比报错更难查）
                return Err(ProbeFault::with(ProbeFaultKind::ShNotFinite, i as u64, 0));
            }
            if l.intensity <= 0.0 || l.radius <= 0.0 {
                continue;
            }
            for k in 0..dirs.len() {
                let d = dirs[k];
                // 可见性：光源在探针影响半径外直接跳过（贡献为 0）
                let dv = (l.pos.0 - p.pos.0, l.pos.1 - p.pos.1, l.pos.2 - p.pos.2);
                let dist = norm3(dv);
                if !(dist > 1.0e-4) || dist > l.radius.max(p.influence_r) {
                    continue;
                }
                // 方向 d 上的余弦项：光源在 d 方向上的辐照度 ∝ cos / d²
                let ndl = dot3(d, (dv.0 / dist, dv.1 / dist, dv.2 / dist));
                if ndl <= 0.0 {
                    continue;
                }
                // 遮挡：线段 p→l 是否穿过任一遮挡盒
                if !occlusion_degraded && segment_hits_any(p.pos, l.pos, &input.occluders) {
                    continue;
                }
                let atten = l.intensity / (dist * dist);
                let contrib = ndl * atten / (dirs.len() as f32) * core::f32::consts::PI;
                let b = sh_basis_at(d);
                for term in 0..SH2_TERMS {
                    sh.accumulate(term, 0, contrib * b[term] * l.color.0);
                    sh.accumulate(term, 1, contrib * b[term] * l.color.1);
                    sh.accumulate(term, 2, contrib * b[term] * l.color.2);
                }
            }
        }
        // **烘焙期校验**（判据五）：非法即拒，不写库
        sh.validate()?;
        field.set_sh(i, sh)?;
    }
    // 密度检查（判据五）
    report.sparse_pairs = check_density(field);
    Ok(report)
}

/// Fibonacci 球采样方向（**确定性**，无 RNG）。
///
/// `n` 个方向按黄金角 (π(3−√5)) 螺旋分布。`n=0` 收口为 1（避免
/// 除零导致空SH——空 SH 与「全黑环境」在下游无法区分）。
pub fn fib_dirs(n: u32) -> Vec<(f32, f32, f32)> {
    let cnt = if n == 0 { 1 } else { n };
    let mut v = Vec::new();
    let ga = core::f32::consts::PI * (3.0 - 2.236_068); // 黄金角
    for i in 0..cnt {
        let fi = i as f32;
        let z = 1.0 - 2.0 * (fi + 0.5) / (cnt as f32);
        let r = (1.0 - z * z).max(0.0).sqrt();
        let th = ga * fi;
        v.push((r * th.cos(), r * th.sin(), z));
    }
    v
}

/// 球谐基函数在单位方向上的值（9 项）。
///
/// **对判据层公开**（）是刻意的：判据需要用**独立实现**
/// （）交叉对拍，若基函数私有，判据只能
/// 调被测实现本身——那样「基函数写错」与「辐照度用错」会同时错，
/// 判据恒绿。公开它让判据能对比一个**不依赖本函数**的闭式解。
#[inline]
pub fn sh_basis_at(d: (f32, f32, f32)) -> [f32; 9] {
    let (x, y, z) = normalize3(d);
    basis(x, y, z)
}

#[inline]
fn dot3(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

#[inline]
fn norm3(v: (f32, f32, f32)) -> f32 {
    let l2 = v.0 * v.0 + v.1 * v.1 + v.2 * v.2;
    if l2 > 0.0 {
        l2.sqrt()
    } else {
        0.0
    }
}

/// 线段是否穿过任一 AABB（slabs 法）。
///
/// 端点已在盒内也算命中（探针被包在几何体里的情形）。
fn segment_hits_any(
    from: (f32, f32, f32),
    to: (f32, f32, f32),
    boxes: &[OccluderBox],
) -> bool {
    boxes.iter().any(|b| segment_hits_box(from, to, b))
}

/// 线段-AABB 相交（slabs 法，返回最先命中的 `t` 或 `None`）。
fn segment_hits_box(
    from: (f32, f32, f32),
    to: (f32, f32, f32),
    b: &OccluderBox,
) -> bool {
    if b.contains(from) {
        return true;
    }
    let d = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;
    // 三轴 slabs：每轴解 t 区间求交
    for k in 0..3 {
        let (o, dd, lo, hi) = match k {
            0 => (from.0, d.0, b.lo.0, b.hi.0),
            1 => (from.1, d.1, b.lo.1, b.hi.1),
            _ => (from.2, d.2, b.lo.2, b.hi.2),
        };
        if dd.abs() < 1.0e-12 {
            // 该轴平行：若原坐标不在板内则整个线段不命中
            if o < lo || o > hi {
                return false;
            }
        } else {
            let mut t1 = (lo - o) / dd;
            let mut t2 = (hi - o) / dd;
            if t1 > t2 {
                let tmp = t1;
                t1 = t2;
                t2 = tmp;
            }
            if t1 > tmin {
                tmin = t1;
            }
            if t2 < tmax {
                tmax = t2;
            }
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

/// 密度检查：统计「间距过大」的探针对数（判据五）。
///
/// 以每探针的最近邻距离为准：> [`DENSITY_WARN_SPACING`] 即计一对。
/// 用**最近邻**而非全局均距——一个角落的密集簇会把均距拉低，
/// 掩盖真正的稀疏区（那正是会出现梯度突变的地方）。
pub fn check_density(field: &ProbeField) -> usize {
    let n = field.len();
    let mut sparse = 0usize;
    for i in 0..n {
        let pi = match field.probe(i) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut nearest = f32::INFINITY;
        for j in 0..n {
            if i == j {
                continue;
            }
            if let Ok(pj) = field.probe(j) {
                let d2 = dist_sq(pi.pos, pj.pos);
                if d2 < nearest {
                    nearest = d2;
                }
            }
        }
        if nearest == f32::INFINITY || nearest.sqrt() > DENSITY_WARN_SPACING {
            sparse += 1;
        }
    }
    sparse
}

// ---------------------------------------------------------------------------
// 八、插值框架（判据四）
// ---------------------------------------------------------------------------

/// 一个探针的插值权重。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WeightEntry {
    /// 探针序号。
    pub index: usize,
    /// 归一化权重（**恒非负**，见判据四）。
    pub weight: f32,
}

/// 采样结果（权重 + 混合后的 SH + 告警计数）。
#[derive(Clone, PartialEq, Debug)]
pub struct SampleResult {
    /// 混合后的 SH 系数（已钳制，见判据四）。
    pub sh: ShCoeffs,
    /// 参与混合的权重（**已按权重降序**，供I03 逐项累加）。
    pub weights: Vec<WeightEntry>,
    /// 被钳到 0 的负权重数（恒 0——本实现用 `1/(d²+ε)` 天然为正；
    /// 保留该字段是为了让「钳制」这条判据有**可观测的落点**，
    /// 而不是靠「理论上不会发生」搪塞）。
    pub clamped_negative: usize,
    /// 是否退化为「所有探针都在影响域外」（此时取最近探针单点）。
    pub outside_all: bool,
}

/// **采样插值**（判据四）。
///
/// 权重公式：`w_i = 1 / (d_i² + WEIGHT_EPS)`，**半径外钳 0**，再归一化。
///
/// **为什么必须钳半径外**：不钳时 `d=1000` 的探针权重是 1e-6，看似可忽略；
/// 但若近处探针也在 d=1000（稀疏场景的常态），两者同量级，
/// 于是「近处探针的实际贡献」被「远处探针的噪声」决定——插值结果
/// 随远处探针的烘焙误差抖动，表现为物体移动时 Indirect 光闪烁。
///
/// **为什么不用紧支撑的 smoothstep 权重**：紧支撑在边界处权重导数不连续
/// （C0 但非 C1），物体跨过支撑边界时会出现一阶亮度跳变。本实现用
/// `1/(d²+ε)` 的**全域光滑**权重，代价是远处探针也参与——而这已由
/// 「半径外钳 0」解决。
///
/// **全在半径外时退化为最近探针**并置 `outside_all`——不是返回零
/// （全黑），也不是报错（数据合法）：最近探针是最合理的近似，
/// 且告警会告诉下游「该加密探针」。
pub fn sample(field: &ProbeField, p: (f32, f32, f32)) -> Result<SampleResult, ProbeFault> {
    let n = field.len();
    if n == 0 {
        return Err(ProbeFault::with(ProbeFaultKind::IndexOutOfRange, 0, 0));
    }
    if !is_fin(p.0) || !is_fin(p.1) || !is_fin(p.2) {
        return Err(ProbeFault::new(ProbeFaultKind::ProbePosNotFinite));
    }

    let mut raw: Vec<WeightEntry> = Vec::new();
    let mut total = 0.0f32;
    let mut nearest_idx = 0usize;
    let mut nearest_d2 = f32::INFINITY;

    for i in 0..n {
        let pr = field.probe(i)?;
        let d2 = dist_sq(p, pr.pos);
        if d2 < nearest_d2 {
            nearest_d2 = d2;
            nearest_idx = i;
        }
        // 半径外 → 权重 0（判据四的核心，见函数体说明）
        if d2 > pr.influence_r * pr.influence_r {
            continue;
        }
        let w = 1.0 / (d2 + WEIGHT_EPS);
        if w > 0.0 {
            total += w;
            raw.push(WeightEntry { index: i, weight: w });
        }
    }

    let mut res = SampleResult {
        sh: ShCoeffs::ZERO,
        weights: Vec::new(),
        clamped_negative: 0,
        outside_all: raw.is_empty(),
    };

    if raw.is_empty() {
        // 退化路径：取最近探针单点（显式告警落点）
        res.weights.push(WeightEntry {
            index: nearest_idx,
            weight: 1.0,
        });
        res.sh = field.sh(nearest_idx)?;
        res.sh.clamp_in_place();
        return Ok(res);
    }

    // 归一化 + 降序（按权重，相等时按 index 升序保确定性）
    for w in raw.iter_mut() {
        w.weight /= total;
    }
    raw.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| a.index.cmp(&b.index))
    });
    // **负权重核查**：本实现权重恒正，此处显式检查并计数，
    // 让「钳制保护」这条判据有真实落点（而不是空断言）。
    for w in raw.iter_mut() {
        if !(w.weight >= 0.0) {
            w.weight = 0.0;
            res.clamped_negative += 1;
        }
    }

    // 混合
    let mut acc = [0.0f32; FLOATS_PER_PROBE];
    for w in raw.iter() {
        let s = field.sh(w.index)?;
        for k in 0..FLOATS_PER_PROBE {
            acc[k] += s.coeffs[k] * w.weight;
        }
    }
    res.sh = ShCoeffs { coeffs: acc };
    // **插值后钳制**（判据四）：防止负能量在多次插值后累积成负亮度
    res.sh.clamp_in_place();
    res.weights = raw;
    Ok(res)
}

/// 权重缓存（锚点「权重重算仅移动时」）。
///
/// 缓存 `(探针序号, 权重)` 列表 + 采样点；采样点未变且探针集未变
/// 时直接复用。**失效判据必须含探针集版本**——只比采样点的话，
/// 探针被增删后旧权重会指向错位的序号（症状：加一个探针，
/// 场景里某个角落的间接光突变）。
#[derive(Clone, Debug, Default)]
pub struct WeightsCache {
    point: (f32, f32, f32),
    valid: bool,
    generation: u64,
    entries: Vec<WeightEntry>,
}

impl WeightsCache {
    /// 空缓存。
    pub fn new() -> WeightsCache {
        WeightsCache {
            point: (0.0, 0.0, 0.0),
            valid: false,
            generation: 0,
            entries: Vec::new(),
        }
    }
    /// 缓存是否可用。
    pub fn is_valid(&self, p: (f32, f32, f32), generation: u64) -> bool {
        self.valid
            && self.generation == generation
            && self.point.0 == p.0
            && self.point.1 == p.1
            && self.point.2 == p.2
    }
    /// 取缓存权重。
    pub fn get(&self) -> &[WeightEntry] {
        &self.entries
    }
    /// 写入缓存。
    pub fn put(&mut self, p: (f32, f32, f32), generation: u64, entries: &[WeightEntry]) {
        self.point = p;
        self.generation = generation;
        self.entries.clear();
        for e in entries.iter() {
            self.entries.push(*e);
        }
        self.valid = true;
    }
    /// 显式失效。
    pub fn invalidate(&mut self) {
        self.valid = false;
        self.generation = 0;
        self.entries.clear();
    }
}

/// 探针集**代数**（插值缓存的失效依据）。
///
/// 由「探针数 + 各探针 stable_id 的顺序敏感混合」算出——
/// 只用  是不够的：删一个再加一个，len 不变但序号已错位，
/// 旧权重会指向别的探针（症状：增删一个探针后某角落间接光突变）。
/// **不单设计数器**而从内容算，是为了让「增删必变、代号不因
/// 删了又加回而巧合相同」两条性质同时成立。
pub fn generation_of(field: &ProbeField) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV 偏移基
    h ^= field.len() as u64;
    h = h.wrapping_mul(0x100_0000_01b3);
    for i in 0..field.len() {
        if let Ok(p) = field.probe(i) {
            h ^= p.stable_id;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

// ---------------------------------------------------------------------------
// 九、序列化（探针集开放格式）
// ---------------------------------------------------------------------------

/// 序列化为探针集开放格式（判据三「输出探针集二进制开放格式」）。
///
/// 布局见 [`PROBE_FORMAT_DOC`]。**每探针 54 字节**（含位置与半径的
/// half 量化——探针集是离线产物，量化换内存是划算的）。
pub fn serialize(field: &ProbeField) -> Result<Vec<u8>, ProbeFault> {
    let n = field.len();
    if n > MAX_PROBES {
        return Err(ProbeFault::with(
            ProbeFaultKind::QuotaExceeded,
            (n * BYTES_PER_PROBE) as u64,
            PROBE_QUOTA_F16 as u64,
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"VXPB");
    out.extend_from_slice(&PROBE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(n as u16).to_le_bytes());
    for i in 0..n {
        let p = field.probe(i)?;
        let s = field.sh(i)?;
        for v in [p.pos.0, p.pos.1, p.pos.2, p.influence_r] {
            let h = f32_to_f16(v).to_le_bytes();
            out.extend_from_slice(&h);
        }
        out.extend_from_slice(&s.quantize());
    }
    Ok(out)
}

/// 从开放格式反序列化（**对称校验**：位置与 SH 都要过 validate）。
pub fn deserialize(bytes: &[u8]) -> Result<ProbeField, ProbeFault> {
    if bytes.len() < 8 {
        return Err(ProbeFault::with(ProbeFaultKind::ShortPayload, bytes.len() as u64, 8));
    }
    if &bytes[..4] != b"VXPB" {
        return Err(ProbeFault::new(ProbeFaultKind::BadMagic));
    }
    let ver = u16::from_le_bytes([bytes[4], bytes[5]]);
    if ver != PROBE_FORMAT_VERSION {
        return Err(ProbeFault::with(ProbeFaultKind::BadVersion, ver as u64, PROBE_FORMAT_VERSION as u64));
    }
    let n = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
    if n > MAX_PROBES {
        return Err(ProbeFault::with(
            ProbeFaultKind::QuotaExceeded,
            (n * BYTES_PER_PROBE) as u64,
            PROBE_QUOTA_F16 as u64,
        ));
    }
    let need = 8 + n * (8 + BYTES_PER_PROBE);
    if bytes.len() < need {
        return Err(ProbeFault::with(ProbeFaultKind::ShortPayload, bytes.len() as u64, need as u64));
    }
    let mut field = ProbeField::new();
    let mut off = 8usize;
    for _ in 0..n {
        let mut halfs = [0u8; 8];
        halfs.copy_from_slice(&bytes[off..off + 8]);
        let mut v = [0.0f32; 4];
        for k in 0..4 {
            v[k] = f16_to_f32(u16::from_le_bytes([halfs[k * 2], halfs[k * 2 + 1]]));
        }
        off += 8;
        let s = ShCoeffs::dequantize(&bytes[off..off + BYTES_PER_PROBE])?;
        off += BYTES_PER_PROBE;
        // **反序列化也校验**：坏数据不该因为「是从磁盘读来的」就放行
        s.validate()?;
        if !is_fin(v[0]) || !is_fin(v[1]) || !is_fin(v[2]) {
            return Err(ProbeFault::new(ProbeFaultKind::ProbePosNotFinite));
        }
        field.add_manual((v[0], v[1], v[2]), v[3])?;
        field.set_sh(field.len() - 1, s)?;
    }
    Ok(field)
}
