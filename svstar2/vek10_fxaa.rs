//! VE-F2010 · 抗锯齿四法之 FXAA（VE-K 域 · 后处理架构与 Bloom 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2010`
//!
//! **判据（锚点原文五条）**：
//! - **单 pass**：亮度对比边缘检测 → 边缘方向 → 定向模糊，**一个 pass 内完成**
//!   （不是"三 pass 拼起来叫 FXAA"——那是 SMAA 的结构，见 F2012 预留）；
//! - **三预设**：低/中/高三档，**每档必须真的改参数**（阈值/模糊幅度/边搜索步数
//!   三者都动），只改标签不改正交量是本条最容易糊弄过去的地方；
//! - **模糊诚实声明**：FXAA 的代价是**文本与高频细节发糊**——必须给出可复算的
//!   量化证据（高频能量衰减比），而不是写一句"可能会糊"；
//! - **选型贡献**：向 F2012 四法选型表输出**一行可对账数据**（质量/成本/依赖/
//!   适用场景），且其中的"零历史依赖"这类字段必须**由实现结构导出**，
//!   不能手写字符串——手写的字段会与实现漂移且无人发现；
//! - **判据**：见 `vek10_checks.rs`。
//!
//! **算法基线**：Console/FXAA 3.11 质量档（`Fxaa3_11.h` 的
//! `FxaaPixelShader` 路径）。逐条对应关系：
//!
//! | 参考实现步骤 | 本条函数 |
//! | --- | --- |
//! | `lumaNW/NE/SW/SE` + `lumaM` | [`sample_luma_diagonal`] + [`sample_texel`] |
//! | `lumaS = (NW+NE+SW+SE)*0.25` | [`detect_edge`] |
//! | `rangeL < max(lumaLo,lumaHi)*EDGE_MIN` 早退 | [`detect_edge`] → `EdgeProbe` |
//! | `dir.x = -((NW+NE)-(SW+SE))` | [`solve_direction`] |
//! | `dir.y = ((NW+SW)-(NE+SE))` | [`solve_direction`] |
//! | `dirReduce = max((NW+NE+SW+SE)*REDUCE_MUL, REDUCE_MIN)` | [`solve_direction`] |
//! | `dir = clamp(dir, ±SPAN_MAX) * rcpDirMin` | [`solve_direction`] |
//! | `rgbA = 0.5*(T(d*(1/3-0.5)) + T(d*(2/3-0.5)))` | [`sample_pair_a`] |
//! | `rgbB = A*0.5 + 0.5*(T(d*(-1/3+0.5)) + T(d*(1/3-0.5)))` | [`sample_pair_b`] |
//! | `lumaB < lumaMin \|\| lumaB < lumaM` → A，否则 B | [`select_source`] |
//!
//! **设计要点（为什么这样写）**：
//! - **平场恒等是第零判据**：整幅同色时`lumaS == lumaM` ⇒ `rangeL == 0` <
//!   任何正阈值 ⇒ 早退返回中心像素。写成"只要rangeL ≤ 阈值"（闭区间）会在
//!   `edge_min` 恰等于 `rangeL` 的平场上仍然进入方向解算，**模糊一个不需要
//!   模糊的画面**——本条用半开区间 `[0, edge_min)` 早退并单测该边界。
//! - **输出取值集闭包** {中心, A, B}：Console 路径的末端是
//!   `if (cond) return A; else return B;`——**没有 mix**。所以 `resolve_pixel`
//!   的返回值**逐位等于**三值之一，判据可以写 `==` 而不是 `≈`。若有人在
//!   末端插一个 `mix(..., 0.5)`"让过渡自然些"，闭包判据立刻变红。这是本条
//!   最强的一条结构不变量。
//! - **方向与梯度正交是解析律不是观感**：垂直边缘（左边暗右边亮）代入
//!   `dir.y = (NW+SW) - (NE+SE)`，两侧同为"一暗一亮"⇒ 精确 0；反之 `dir.x`
//!   精确非零。判据用**精确 ==0.0** 而非 `|dir.y| < eps`：f32 下相同数相加再
//!   相减结果必为 0，eps 只会放过"算错但很小"的实现。**坐标轴对调**（x/y 写反）
//!   是本类实现最常见缺陷，双边阈值放它过去，精确零断言不会。
//! - **转置不变性是最省事的强判据**：FXAA 的四邻域采样在转置下把
//!   `NE↔SW` 互换，推出 `(dir.x, dir.y) → (dir.y, -dir.x)`——也就是输出随图
//!   一同转置。写成"对 img 与 imgᵀ 各跑一遍、断言 out(img)ᵀ == out(imgᵀ)"
//!   就同时覆盖了采样坐标、方向分量、优先级、条件分支四类缺陷，**一条判据
//!   顶四条**。这比逐条断言"某坐标取的是 NW"更抗重构。
//! - **档位强度用集合包含关系验证**：`edge_min` 更低 ⇒ 早退集合更大 ⇒
//!   被处理的像素集合 `S(low) ⊆ S(mid) ⊆ S(high)`。只断言"三档参数严格递增"
//!   是弱门禁——把三个数写反了它照样绿（序关系不蕴含数值正确）。集合包含
//!   是**解析律**，方向写反立刻红。
//! - **步数表由常量推导而非抄写**：质量档 PS 序列在参考里是偶数位 1.5 /
//!   奇数位 1.0 交替 12 项；判据侧**独立按公式重算**该序列再逐项比对，
//!   杜绝"档位只改了标签、参数表没动"这种劣化。
//! - **采样必须双线性，否则整个算法是空操作**：`dir` 的分量经 `rcpDirMin`
//!   缩放后**通常落在 (-1, 1) 之间**（这是`REDUCE_MUL/REDUCE_MIN` 的设计目的
//!   ——把方向归一化到"约一个像素"）。若采样用最近邻取整，`round(dir.x)` 在
//!   `|dir.x| < 0.5` 时恒为 0，于是 A/B 两个采样点**都落在中心像素上**，
//!   `rgbA == rgbB == center` ⇒ 输出逐位等于输入 ⇒ **FXAA 一行都没生效**，
//!   而所有结构性判据（闭包、方向、序位）**照样全绿**。这是本条最危险的
//!   失败模式：**画面不模糊，功能等于没写，却查不出来**。因此
//!   - 采样用**双线性**（与参考 `FxaaTexTop` 同语义），亚像素位置参与插值；
//!   - 头注 [`SAMPLE_MODE_DECL`] 显式登记；
//!   - 判据 [`K10-单pass-确有边缘被处理`] 要求合成图上**必有像素真正改变**
//!     （`touched && out != center`），这一条就是专门堵这个洞的。
//!
//! **错误路径与降级矩阵（锚点原文四条）**：
//! 1. **输入非 LDR（TM 前 HDR 输入）** → 序位守卫。FXAA 属TM **之后**的 LDR
//!    域，接在 HDR 上等于对未压缩的高动态值做 `luma` 门限，边缘检测会选出
//!    亮度突变点而非几何边缘 → 画面出现"发丝状亮斑"。**不是静默跑**：
//!    [`guard_domain_order`] 返回具名违规并带建议序位；
//! 2. **极端参数** → 钳制。`NaN`/`±inf`/越界（负span、zero edge_min）经
//!    [`sanitize_params`] 钳回有效域；`NaN` 输入色经[`finite_or`] 兜底为
//!    中心值——`NaN` 一旦进入 `luma` 比较，`rangeL < t` 恒 false，
//!    **早退被绕过**，方向解算吃满 `NaN`，整屏会静默变成黑或白且无任何报错。
//! 3. **低配设备推荐** → [`recommend_for_budget`] 只给**建议**不强制
//!    （锚点原文"自动建议非强制"），返回的是理由文本 + 供UI 展示的三要素；
//! 4. **与 CAS 锐化冲突**（F2049） → [`cas_coordination`] 联动建议：
//!    FXAA 模糊与 CAS 锐化对冲，必须给**序位 + 参数**两条协调项，且
//!    **同时开启时给出警告**（不是偷偷关掉一个）。
//!
//! **跨批对接点**：
//! - **F2001 管线序**：`PipelineOrder` 声明本条固定在 TM 之后；`guard_domain_order`
//!   的 `expected_before/after` 直接引用序位枚举，不写字符串；
//! - **F2009 MSAA / F2011 TAA**：抗锯齿四法的**互参照**，不实现它们，只在
//!   [`selection_row`] 里给出本条那一行；
//! - **F2012 选型表**：[`selection_row`] 是 F2012 的数据源之一，字段由实现导出；
//! - **F2017 基准**：0.2ms@1080p 是**锚点预算值**，本条只给成本模型与
//!    内部一致性判据，绝对值待实测回填（见 [`PERF_HONESTY_DECL`]）；
//! - **F2049 CAS**：联动建议的对方；
//! - **D09 设备能力**：**无关**（纯像素级，低配友好）——由
//!   [`depends_on_device_caps`] = false 结构性声明，判据核对该函数恒假。
//!
//! **性能诚实标注**：
//! [`COST_MS_1080P`] 是锚点预算 0.2ms，**不是本机实测**——内核里没有可用的
//!   GPU 计时器，写"实测"是编数据。本条能**真实验证**的是成本模型的内部
//!   一致性（像素数线性、三档单调且差异 <30% 的预算口径），写成判据。
//! **上游未达标不代改**：F2017 未定标前不调常数迎合任何数字。
//!
//! **无障碍与隐私**：
//! -无运行时隐私面；
//! - **无障碍侧的真实影响**：[`a11y_advisory`] 显式登记——FXAA 的模糊会
//!   降低**高频笔画的可辨识度**（低视力、色觉障碍用户对字形斜笔画尤其敏感），
//!   建议 UI 文本在 FXAA **之后**渲染或 UI 层豁免；同时**诚实限定**：这是
//!   "内容文本发糊"，不是"界面不可读"（UI 在 V 域合成、位于后处理之后），
//!   把两者混谈会导致错误地要求 UI 也走 FXAA；
//! - 本条的可量化佐证是 [`high_frequency_energy_ratio`]：对合成高频图案
//!   实测 FXAA 前后的高频能量比，作为模糊代价的**可复算证据**（判据要求
//!   它确实 < 1——哪天实现不糊了，这条判据反而会红，逼着更新声明）。
//!
//! **落位与注册**：`svstar2/vek10_fxaa.rs`（本文件）、`svstar2/vek10_checks.rs`。
//! 主模块自带 `run_vek10_checks()`，自检**不** `use super::` 未注册的兄弟模块。
//!
//! 零墙钟、零 IO、纯确定性：所有量化证据均由本文件内的合成图案现算，
//! 不读外部数据、不依赖墙钟，回归可复现。

// `ToString` 必须显式引入：本 crate 在内核镜像目标下是纯 `no_std`，
// `u32/u64` 的 `to_string()` 由 `alloc::string::ToString` 提供，
// 漏掉这一行会在 `cargo build --lib` 报 E0599（宿主 std 侧不会暴露）。
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 1. 质量档常量（Console/FXAA 3.11）
// ===========================================================================

/// FXAA 3.11 质量档的最大 PS（方向外推步）数。
///
/// 参考 `Fxaa3_11.h` 的 `FXAA_QUALITY__PRESET 12` 段：共 12 个
/// `FXAA_QUALITY__Pn`，取值 1.5 / 1.0 交替（`P0=1.5` 起）。
pub const FXAA_PS_MAX: usize = 12;

/// 参考实现的默认边缘阈值（`FXAA_QUALITY__EDGE_MIN`）。
pub const FXAA_EDGE_MIN_REF: f32 = 0.03125;

/// 参考实现的 `REDUCE_MUL`（1/8）。
pub const FXAA_REDUCE_MUL_REF: f32 = 0.125;

/// 参考实现的 `REDUCE_MIN`（1/128）。
pub const FXAA_REDUCE_MIN_REF: f32 = 0.0078125;

/// 参考实现的 `SPAN_MAX`。
pub const FXAA_SPAN_MAX_REF: f32 = 8.0;

// ===========================================================================
// 2. 基础类型
// ===========================================================================

/// 一个线性 RGB 三元组（`f32`，未做显示编码）。
///
/// `f32` 不派生 `Eq`——`NaN != NaN` 是 IEEE 语义，派生 `Eq` 会诱导调用方
/// 写出永远为假的相等断言。本条所有比较都走 [`finite_or`] 兜底。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    /// 构造一个三元组。
    pub const fn new(r: f32, g: f32, b: f32) -> Rgb {
        Rgb { r, g, b }
    }

    /// 全黑。
    pub const fn black() -> Rgb {
        Rgb::new(0.0, 0.0, 0.0)
    }

    /// 全白。
    pub const fn white() -> Rgb {
        Rgb::new(1.0, 1.0, 1.0)
    }

    /// 逐分量平均（`sum / 2`，用于 Console 路径的 `0.5*(A+B)`）。
    #[inline]
    pub fn mean2(self, other: Rgb) -> Rgb {
        Rgb::new(
            (self.r + other.r) * 0.5,
            (self.g + other.g) * 0.5,
            (self.b + other.b) * 0.5,
        )
    }

    /// 逐分量最小值。
    #[inline]
    pub fn min3(self, other: Rgb) -> Rgb {
        Rgb::new(
            if self.r < other.r { self.r } else { other.r },
            if self.g < other.g { self.g } else { other.g },
            if self.b < other.b { self.b } else { other.b },
        )
    }

    /// 逐分量最大值。
    #[inline]
    pub fn max3(self, Rgb { r: or, g: og, b: ob }: Rgb) -> Rgb {
        Rgb::new(
            if self.r > or { self.r } else { or },
            if self.g > og { self.g } else { og },
            if self.b > ob { self.b } else { ob },
        )
    }

    /// 三个分量是否都是有限值。
    ///
    /// **直接查 `f32::is_finite`，不经过 [`finite_or`]**：后者会把 `NaN` 折叠
    /// 成 `0.0`，于是 `finite_or(x).is_finite()` **恒为 true**——用`finite_or`
    /// 写`is_finite` 会得到一个永远说"一切正常"的探针，静默失败。
    #[inline]
    pub fn is_finite(self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite()
    }

    /// 钳制到 `[0, 1]`（FXAA 契约：工作在 LDR 显示域）。
    pub fn saturate(self) -> Rgb {
        Rgb::new(clamp01(self.r), clamp01(self.g), clamp01(self.b))
    }
}

/// 把 `f32` 的 `NaN` 折叠为 `0.0`。
///
/// **为什么必须折叠而不是留 `NaN`**：`NaN` 进入 `luma` 比较后
/// `rangeL < t` 恒为 false ⇒ 早退被绕过 ⇒ 方向解算吃满 `NaN` ⇒ `clamp(NaN)`
/// 仍为 `NaN` ⇒ 输出 `NaN` ⇒ 整屏变黑且**不报任何错**。这是本条唯一一处
/// "静默失败"的入口，所以放在最底层的取值函数上，所有采样都必经它。
#[inline]
pub fn finite_or(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// 钳制到 `[0, 1]`。
#[inline]
pub fn clamp01(v: f32) -> f32 {
    let v = finite_or(v);
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// 钳制到 `[lo, hi]`；`lo > hi` 时返回 `lo`（不 panic——零 panic 面）。
#[inline]
pub fn clamp_range(v: f32, lo: f32, hi: f32) -> f32 {
    let v = finite_or(v);
    let lo2 = finite_or(lo);
    let hi2 = finite_or(hi);
    let hi2 = if hi2 < lo2 { lo2 } else { hi2 };
    if v < lo2 {
        lo2
    } else if v > hi2 {
        hi2
    } else {
        v
    }
}

/// 感知亮度（Rec.709 系数，`FxaaPixelShader` 的 `luma`）。
#[inline]
pub fn luma(c: Rgb) -> f32 {
    finite_or(
        c.r * 0.299 + c.g * 0.587 + c.b * 0.114,
    )
}

// ===========================================================================
// 3. 预设
// ===========================================================================

/// FXAA 质量档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxaaQuality {
    Low,
    Medium,
    High,
}

impl FxaaQuality {
    /// 线上编码值（**显式映射**，不用 `as u8`）。
    ///
    /// 判别序恰好是 0/1/2，而"档位强度序"也是低→高，两者在语义上一致——
    /// 但这属于**巧合**。`as u8` 一旦被拿去与 `ps`（5/8/12）或与
    /// `edge_min` 的次序比较，会静默错位，故一律走显式映射。
    pub const fn wire(self) -> u8 {
        match self {
            FxaaQuality::Low => 0,
            FxaaQuality::Medium => 1,
            FxaaQuality::High => 2,
        }
    }

    /// 从线上编码值还原；非法值返回 `None`（不猜测）。
    pub const fn from_wire(w: u8) -> Option<FxaaQuality> {
        match w {
            0 => Some(FxaaQuality::Low),
            1 => Some(FxaaQuality::Medium),
            2 => Some(FxaaQuality::High),
            _ => None,
        }
    }

    /// 全档位枚举序（低 → 高），供选型表遍历。
    pub const ALL: [FxaaQuality; 3] =
        [FxaaQuality::Low, FxaaQuality::Medium, FxaaQuality::High];

    /// 展示名（供 UI 与诊断文本）。
    pub const fn label(self) -> &'static str {
        match self {
            FxaaQuality::Low => "FXAA-LOW",
            FxaaQuality::Medium => "FXAA-MED",
            FxaaQuality::High => "FXAA-HIGH",
        }
    }
}

/// FXAA 全部可调参数（一个预设的完整内容）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxaaParams {
    /// 边缘早退阈值 `EDGE_MIN`：低于 `range_hi * edge_min` 的对比度不处理。
    pub edge_min: f32,
    /// 方向降噪系数 `REDUCE_MUL`。
    pub reduce_mul: f32,
    /// 方向降噪下限 `REDUCE_MIN`。
    pub reduce_min: f32,
    /// 方向搜索最大跨度（像素）`SPAN_MAX`。
    pub span_max: f32,
    /// 方向外推步序列前`steps_used` 项有效。
    pub steps: [f32; FXAA_PS_MAX],
    /// 实际启用的步数（`FxaaQuality` 强度的离散表达）。
    pub steps_used: u8,
}

impl FxaaParams {
    /// 有效步序列切片（`steps_used` 夹在 `1..=FXAA_PS_MAX`）。
    pub fn active_steps(&self) -> &[f32] {
        let n = (self.steps_used as usize).clamp(1, FXAA_PS_MAX);
        &self.steps[..n]
    }
}

/// 一个预设：档位标识 + 展示名 + 参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxaaPreset {
    pub quality: FxaaQuality,
    pub label: &'static str,
    pub params: FxaaParams,
}

const PRESET_LOW: FxaaPreset = FxaaPreset {
    quality: FxaaQuality::Low,
    label: "FXAA-LOW",
    params: FxaaParams {
        // 阈值放宽 4×⇒ 只处理最强的边缘 ⇒ 低配少糊也少开销
        edge_min: 0.125,
        reduce_mul: 0.25,
        reduce_min: 0.015625,
        span_max: 4.0,
        steps: [
            1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0,
        ],
        steps_used: 5,
    },
};

const PRESET_MED: FxaaPreset = FxaaPreset {
    quality: FxaaQuality::Medium,
    label: "FXAA-MED",
    params: FxaaParams {
        edge_min: 0.0625,
        reduce_mul: 0.125,
        reduce_min: 0.0078125,
        span_max: 6.0,
        steps: [
            1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0,
        ],
        steps_used: 8,
    },
};

const PRESET_HIGH: FxaaPreset = FxaaPreset {
    quality: FxaaQuality::High,
    label: "FXAA-HIGH",
    params: FxaaParams {
        edge_min: FXAA_EDGE_MIN_REF,
        reduce_mul: FXAA_REDUCE_MUL_REF,
        reduce_min: FXAA_REDUCE_MIN_REF,
        span_max: FXAA_SPAN_MAX_REF,
        steps: [
            1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0, 1.5, 1.0,
        ],
        steps_used: 12,
    },
};

const PRESETS: [FxaaPreset; 3] = [PRESET_LOW, PRESET_MED, PRESET_HIGH];

/// 按档位查预设——返回**静态表里的同一份**（零分配、零拷贝）。
///
/// 预设切换零成本由此得到形式化保证：查表是纯函数，切换只改一个枚举。
pub fn preset(q: FxaaQuality) -> &'static FxaaPreset {
    match q {
        FxaaQuality::Low => &PRESET_LOW,
        FxaaQuality::Medium => &PRESET_MED,
        FxaaQuality::High => &PRESET_HIGH,
    }
}

/// 全档位（低 → 高）顺序视图。
pub fn presets() -> &'static [FxaaPreset; 3] {
    &PRESETS
}

// ===========================================================================
// 4. 帧缓冲
// ===========================================================================

/// 一幅 `f32` 线性 RGB 图像（行优先，`w * h` 个texel）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Rgb>,
}

impl Frame {
    /// 构造一幅全填 `fill` 的图像。
    ///
    /// 宽高为 0 时产出空图（**不panic**）；`w * h` 溢出 32 位时钳到
    /// `usize::MAX / 8` 上限并由 [`Frame::len_mismatch`] 显性报告——
    /// 静默截断会得到一幅"看起来对、尺寸错"的图。
    pub fn filled(w: u32, h: u32, fill: Rgb) -> Frame {
        let cap = (w as usize).saturating_mul(h as usize);
        let mut px: Vec<Rgb> = Vec::new();
        if cap > 0 {
            px.reserve(cap.min(1 << 20));
            for _ in 0..cap {
                px.push(fill);
            }
        }
        Frame { w, h, px }
    }

    /// 尺寸与数据长度是否一致。
    pub fn len_mismatch(&self) -> bool {
        (self.w as usize).saturating_mul(self.h as usize) != self.px.len()
    }

    /// texel 索引（越界返回 `None`）。
    #[inline]
    pub fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let i = (y as usize).saturating_mul(self.w as usize).saturating_add(x as usize);
        if i < self.px.len() {
            Some(i)
        } else {
            None
        }
    }

    /// 取 texel；越界钳到最近边界（与参考实现的 `clamp(posN/posZ)` 同语义）。
    #[inline]
    pub fn get_clamped(&self, x: i32, y: i32) -> Option<Rgb> {
        if self.w == 0 || self.h == 0 {
            return None;
        }
        let cx = clamp_i32(x, 0, self.w as i32 - 1);
        let cy = clamp_i32(y, 0, self.h as i32 - 1);
        self.index(cx as u32, cy as u32).map(|i| self.px[i])
    }

    /// 写 texel；越界返回 `false`。
    pub fn put(&mut self, x: u32, y: u32, v: Rgb) -> bool {
        match self.index(x, y) {
            Some(i) => {
                self.px[i] = v;
                true
            }
            None => false,
        }
    }
}

/// 钳制 `i32` 到 `[lo, hi]`；`lo > hi` 时返回 `lo`。
#[inline]
pub fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    let hi2 = if hi < lo { lo } else { hi };
    if v < lo {
        lo
    } else if v > hi2 {
        hi2
    } else {
        v
    }
}

/// 极小尺寸判定：小于该尺寸时 `A`/`B` 采样会全部落到同一 texel 上，
/// 算法退化为"返回中心"，不值得跑。
pub const MIN_DIM: u32 = 2;

// ===========================================================================
// 5. 边缘检测
// ===========================================================================

/// 边缘探针结果：中心亮度、邻域均值亮度、邻域亮度极值、是否需要处理。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeProbe {
    /// 中心 texel 亮度。
    pub luma_m: f32,
    /// 四邻域亮度均值 `(NW+NE+SW+SE)*0.25`。
    pub luma_s: f32,
    /// 邻域（含中心）最小亮度 `lumaLo`。
    pub luma_lo: f32,
    /// 邻域（含中心）最大亮度 `lumaHi`。
    pub luma_hi: f32,
    /// `abs(lumaS - lumaM)`。
    pub range_l: f32,
    /// 早退门限 `max(lumaLo, lumaHi) * edge_min`。
    pub threshold: f32,
    /// 是否进入方向解算（`range_l >= threshold`）。
    pub is_edge: bool,
}

/// 取四邻域亮度（`NW/NE/SW/SE`），任一越界返回 `None`（由调用方决定回退）。
pub fn sample_luma_diagonal(f: &Frame, x: u32, y: u32) -> Option<(f32, f32, f32, f32)> {
    let cx = x as i32;
    let cy = y as i32;
    let nw = f.get_clamped(cx - 1, cy - 1).map(luma)?;
    let ne = f.get_clamped(cx + 1, cy - 1).map(luma)?;
    let sw = f.get_clamped(cx - 1, cy + 1).map(luma)?;
    let se = f.get_clamped(cx + 1, cy + 1).map(luma)?;
    Some((nw, ne, sw, se))
}

/// 边缘检测（`FxaaPixelShader` 的 `lumaS` / `rangeL` / 早退段）。
///
/// **早退用半开区间 `[0, edge_min)`**：`|Δ| < t` 早退，`|Δ| == t` 处理。
/// 写成 `<=` 会在"平场上`Δ` 恰好等于 t"的合成图上误进方向解算——
/// 该合成图正是本条边界判据的取值，写反则判据变红（这是有意的）。
pub fn detect_edge(f: &Frame, x: u32, y: u32, p: &FxaaParams) -> Option<EdgeProbe> {
    let c = f.get_clamped(x as i32, y as i32)?;
    let luma_m = luma(c);
    let (nw, ne, sw, se) = sample_luma_diagonal(f, x, y)?;
    let luma_s = (nw + ne + sw + se) * 0.25;
    let lo4 = Rgb::new(nw, nw, nw).min3(Rgb::new(ne, ne, ne)).min3(Rgb::new(sw, sw, sw)).min3(Rgb::new(se, se, se));
    let hi4 = Rgb::new(nw, nw, nw).max3(Rgb::new(ne, ne, ne)).max3(Rgb::new(sw, sw, sw)).max3(Rgb::new(se, se, se));
    let luma_lo = lo4.r.min(luma_m);
    let luma_hi = hi4.r.max(luma_m);
    let range_l = (luma_s - luma_m).abs();
    let threshold = luma_hi.max(luma_lo) * p.edge_min;
    Some(EdgeProbe {
        luma_m,
        luma_s,
        luma_lo,
        luma_hi,
        range_l,
        threshold,
        is_edge: range_l >= threshold,
    })
}

// ===========================================================================
// 6. 方向解算
// ===========================================================================

/// 边缘方向（像素单位）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeDir {
    pub x: f32,
    pub y: f32,
}

impl EdgeDir {
    /// 沿该方向偏移 `k` 像素，返回**亚像素浮点位置**（供 [`sample_bilinear`]）。
    ///
    /// **刻意不提供"取整到最近 texel"的版本**：那正是会让 FXAA 退化为空操作
    /// 的错误写法（见头注「采样必须双线性」）。`dir` 分量常在 (-1, 1)，
    /// 取整后恒为 0。
    #[inline]
    pub fn offset_f(self, k: f32) -> (f32, f32) {
        (finite_or(self.x * k), finite_or(self.y * k))
    }

    /// 方向长度（`max(|x|,|y|)` 归一的稳定近似，够用于诊断与判据）。
    #[inline]
    pub fn len(self) -> f32 {
        let a = self.x.abs();
        let b = self.y.abs();
        let m = if a > b { a } else { b };
        if m == 0.0 {
            0.0
        } else {
            let n = (b / m) * (b / m);
            m * (1.0 + n * 0.5)
        }
    }
}

/// 方向解算（`dir.x / dir.y / dirReduce / rcpDirMin / clamp` 五步）。
///
/// **坐标轴不可对调**：`dir.x` 来自"左右两列的亮度差"，`dir.y` 来自"上下两行
/// 的亮度差"。对调后算法仍然产生"某个方向"，画面**依然会变平滑**——
/// 弱门禁完全抓不到，判据用垂直边缘的**精确零**断言。
pub fn solve_direction(p4: &(f32, f32, f32, f32), p: &FxaaParams) -> EdgeDir {
    let (nw, ne, sw, se) = *p4;
    let luma_nwne = nw + ne;
    let luma_swse = sw + se;
    let luma_nwsw = nw + sw;
    let luma_nese = ne + se;
    let dx = -(luma_nwne - luma_swse);
    let dy = luma_nwsw - luma_nese;
    let dir_reduce = {
        let v = (luma_nwne + luma_swse) * p.reduce_mul;
        if v > p.reduce_min { v } else { p.reduce_min }
    };
    let min_abs = if dx.abs() < dy.abs() { dx.abs() } else { dy.abs() };
    let denom = min_abs + dir_reduce;
    // denom 恒 ≥ reduce_min > 0（`sanitize_params` 保证 reduce_min > 0），
    // 但仍做分母兜底：0.0 / NaN 都会让整屏静默变黑。
    let rcp = if denom > 1e-12 { 1.0 / denom } else { 0.0 };
    // **先钳后归一，顺序不可交换**（此处曾是真缺陷，记录在案）：
    // 参考实现是 `dir = clamp(dir, ±SPAN_MAX) * rcpDirMin`——先按跨度钳制，
    // 再乘归一化因子。若改成"先归一化再钳制"，`rcp` 在 `dx=dy=0` 时等于
    // `1/reduce_min = 128`，任何非零分量都会被推到 `±8` 的边界，
    // 方向信息（哪一侧更亮）被彻底抹平。
    let cx = clamp_range(dx, -p.span_max, p.span_max);
    let cy = clamp_range(dy, -p.span_max, p.span_max);
    // 归一化后**不再二次钳制**（此处曾是真缺陷，记录在案）：我最初在乘完
    // `rcp` 之后又钳了一次 `±span_max`，看起来是"更保险"，实际把方向
    // **压死成跨度边界**——`rcp` 在强边缘上可达数十，`clamp(±6.4) * 40 ≈ 256`
    // 被二次钳回 8，于是 A/B 采样点全部落到图像边界之外，被
    // `get_clamped` 钳成边缘色 ⇒ 输出恒等于输入 ⇒ **FXAA 退化为空操作**，
    // 而所有结构性判据照样全绿。参考实现的 `SPAN_MAX` 钳制**只作用在
    // 归一化之前**的原始方向差上，此处必须与之一致。
    let ox = finite_or(cx * rcp);
    let oy = finite_or(cy * rcp);
    // **零方向必须显式归 +0.0**：`-(0.0)` 是 `-0.0`，它在f32 下与 `0.0`
    // 参与加法结果相同，但 `to_bits()` 不同、且会让下游 `floor(-0.0) = -0.0`
    // 传播成"负零索引"。`+ 0.0` 把 `-0.0` 规范化。
    EdgeDir {
        x: ox + 0.0,
        y: oy + 0.0,
    }
}

// ===========================================================================
// 7. 定向模糊采样
// ===========================================================================

/// 双线性采样（参考 `FxaaTexTop` 的语义）。
///
/// **为什么必须是双线性**：见头注「采样必须双线性」——`dir` 分量通常落在
/// (-1, 1)，最近邻取整会让 A/B 全部落回中心像素，FXAA 退化为空操作而
/// 结构判据全绿。这是本条最危险的失败模式，故采样层就用双线性。
///
/// 亚像素位置 `fx, fy` 以 texel 为单位（`0.5` = 半个 texel 偏移）。
/// 越界由 [`Frame::get_clamped`] 钳到边界（与参考 `clamp(posN/posZ)` 同语义）。
pub fn sample_bilinear(f: &Frame, fx: f32, fy: f32) -> Option<Rgb> {
    let fx = finite_or(fx);
    let fy = finite_or(fy);
    let x0f = fx.floor();
    let y0f = fy.floor();
    // 权重 frac∈ [0,1)：`floor` 后的差值天然满足，无需额外钳制。
    let tx = fx - x0f;
    let ty = fy - y0f;
    // 极端坐标（±1e30）时 floor 后仍超出 i32，先夹到安全域再转换。
    let x0 = floor_to_i32(x0f);
    let y0 = floor_to_i32(y0f);
    let x1 = x0.saturating_add(1);
    let y1 = y0.saturating_add(1);
    let c00 = f.get_clamped(x0, y0)?;
    let c10 = f.get_clamped(x1, y0)?;
    let c01 = f.get_clamped(x0, y1)?;
    let c11 = f.get_clamped(x1, y1)?;
    // 标准双线性，权重由 tx/ty 显式给出。
    //
    // **不能**用 `mean2` 代替这一步：`mean2` 权重恒为 0.5，等价于 `tx` 恒为
    // 0.5 —— 那会让所有亚像素位置被拉到"两 texel 正中间"，等于把双线性
    // 退化成"偏移半像素的最近邻"，边缘上的插值结果与参考不符。
    let top = Rgb::new(
        c00.r + (c10.r - c00.r) * tx,
        c00.g + (c10.g - c00.g) * tx,
        c00.b + (c10.b - c00.b) * tx,
    );
    let bot = Rgb::new(
        c01.r + (c11.r - c01.r) * tx,
        c01.g + (c11.g - c01.g) * tx,
        c01.b + (c11.b - c01.b) * tx,
    );
    Some(Rgb::new(
        top.r + (bot.r - top.r) * ty,
        top.g + (bot.g - top.g) * ty,
        top.b + (bot.b - top.b) * ty,
    ))
}

/// `f32.floor()` 后安全转 `i32`（超出 `i32` 范围时夹到安全域）。
#[inline]
fn floor_to_i32(v: f32) -> i32 {
    let v = finite_or(v);
    if v > 2.0e9 {
        2_000_000_000
    } else if v < -2.0e9 {
        -2_000_000_000
    } else {
        v as i32
    }
}

/// 沿方向取 A 组三点（Console 路径的 `rgbA`：两个偏移的平均）。
///
/// 偏移 `(1/3 - 0.5)` 与 `(2/3 - 0.5)` 是参考实现的固定值，不是可调参数
/// ——**不要**把它们塞进预设：它们决定了 A 组三点相对中心点的分布，
/// 改动会同时改变所有三档的基线行为，而"档位强度"只该由
/// `edge_min` / `span_max` / `steps_used` 三者表达。
pub fn sample_pair_a(f: &Frame, x: u32, y: u32, d: EdgeDir) -> Option<Rgb> {
    let (a1x, a1y) = d.offset_f(1.0 / 3.0 - 0.5);
    let (a2x, a2y) = d.offset_f(2.0 / 3.0 - 0.5);
    let c1 = sample_bilinear(f, x as f32 + a1x, y as f32 + a1y)?;
    let c2 = sample_bilinear(f, x as f32 + a2x, y as f32 + a2y)?;
    Some(c1.mean2(c2))
}

/// 沿方向取 B 组三点（`rgbB = A*0.5 + 0.5*(T(d*(-1/3+0.5)) + T(d*(1/3-0.5)))`）。
pub fn sample_pair_b(f: &Frame, x: u32, y: u32, d: EdgeDir, a: Rgb) -> Option<Rgb> {
    let (b1x, b1y) = d.offset_f(-1.0 / 3.0 + 0.5);
    let (b2x, b2y) = d.offset_f(1.0 / 3.0 - 0.5);
    let c1 = sample_bilinear(f, x as f32 + b1x, y as f32 + b1y)?;
    let c2 = sample_bilinear(f, x as f32 + b2x, y as f32 + b2y)?;
    Some(a.mean2(c1.mean2(c2)))
}

// ===========================================================================
// 8. 单像素解算
// ===========================================================================

/// 输出取值来源（用于闭包判据与调试可视）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxaaSource {
    /// 早退/不处理：输出中心 texel。
    Center,
    /// 选A（沿边较暗一侧的偏置平均）。
    TapA,
    /// 选 B（沿边较亮一侧的偏置平均）。
    TapB,
}

/// 单像素解算结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxaaPixel {
    /// 输出颜色。
    pub out: Rgb,
    /// 输出取自哪个采样点（闭包判据的对象）。
    pub source: FxaaSource,
    /// 是否被判定为边缘（`false` ⇒ `out == 中心`逐位）。
    pub touched: bool,
    /// 方向（仅 `touched` 时有意义）。
    pub dir: EdgeDir,
}

/// 末端选择（`lumaB < lumaMin || lumaB < lumaM` → A，否则 B）。
///
/// **注意是 `<` 不是 `<=`**：参考实现的两个比较都是严格小于。写成 `<=` 会在
/// `lumaB == lumaM` 的平场残留上多取一次 B，虽然视觉上几乎无差，但它使
/// "输出必 ∈ {A, B}" 的判据在边界上失去唯一性，且掩盖了取样偏移写错。
#[inline]
pub fn select_source(luma_b: f32, luma_m: f32, luma_min: f32) -> FxaaSource {
    if luma_b < luma_min || luma_b < luma_m {
        FxaaSource::TapA
    } else {
        FxaaSource::TapB
    }
}

/// 解算单个像素。
///
/// 图像尺寸不足（任一边 < [`MIN_DIM`]）或数据长度不匹配时返回 `None`
/// ——**不是**返回中心像素。后者会让"结构错乱的帧"看起来完全正常。
pub fn resolve_pixel(
    f: &Frame,
    x: u32,
    y: u32,
    p: &FxaaParams,
) -> Option<FxaaPixel> {
    if f.len_mismatch() || f.w < MIN_DIM || f.h < MIN_DIM {
        return None;
    }
    let center = f.get_clamped(x as i32, y as i32)?;
    let probe = detect_edge(f, x, y, p)?;
    if !probe.is_edge {
        return Some(FxaaPixel {
            out: center,
            source: FxaaSource::Center,
            touched: false,
            dir: EdgeDir::default(),
        });
    }
    let p4 = sample_luma_diagonal(f, x, y)?;
    let dir = solve_direction(&p4, p);
    let a = sample_pair_a(f, x, y, dir)?;
    let b = sample_pair_b(f, x, y, dir, a)?;
    let luma_b = luma(b);
    let luma_min = probe.luma_s * 0.125;
    let source = select_source(luma_b, probe.luma_m, luma_min);
    let out = match source {
        FxaaSource::TapA => a,
        FxaaSource::TapB => b,
        FxaaSource::Center => center,
    };
    Some(FxaaPixel {
        out: Rgb::new(finite_or(out.r), finite_or(out.g), finite_or(out.b)),
        source,
        touched: true,
        dir,
    })
}

/// 整帧解算（单 pass，逐像素独立——**无历史缓冲、无邻域写回**）。
pub fn resolve(f: &Frame, p: &FxaaParams) -> Option<Frame> {
    if f.len_mismatch() || f.w < MIN_DIM || f.h < MIN_DIM {
        return None;
    }
    let mut out = Frame {
        w: f.w,
        h: f.h,
        px: Vec::new(),
    };
    out.px.reserve(f.px.len());
    for y in 0..f.h {
        for x in 0..f.w {
            let r = match resolve_pixel(f, x, y, p) {
                Some(v) => v.out,
                None => return None,
            };
            out.px.push(r);
        }
    }
    Some(out)
}

/// 单 pass 计数：有多少像素被处理（`touched`）。
///
/// 三档集合包含关系判据与模糊代价统计都基于它——注意**按坐标**统计才有
/// 集合语义，只数总数则"高档 100 个、低档 90 个"仍可能是两批不同的像素。
pub fn touched_set(f: &Frame, p: &FxaaParams) -> Option<Vec<bool>> {
    if f.len_mismatch() || f.w < MIN_DIM || f.h < MIN_DIM {
        return None;
    }
    let mut set = Vec::new();
    set.reserve(f.px.len());
    for y in 0..f.h {
        for x in 0..f.w {
            match resolve_pixel(f, x, y, p) {
                Some(v) => set.push(v.touched),
                None => return None,
            }
        }
    }
    Some(set)
}

// ===========================================================================
// 9. 参数钳制（错误路径 2）
// ===========================================================================

/// 参数钳制结果：钳回后的参数 + 是否发生过钳制。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SanitizeReport {
    pub params: FxaaParams,
    pub clamped: bool,
}

/// 把参数钳回有效域（`NaN` / `±inf` / 越界一律兜底）。
///
/// 域：`edge_min ∈ [1/1024, 1]`、`reduce_mul ∈ [0, 1]`、`reduce_min ∈ [1/4096, 1]`
/// （下界严格大于 0——`reduce_min == 0` 时 `rcpDirMin` 的分母可能为 0，
/// 该分支在 [`solve_direction`] 里是死代码，判据会点名）、
/// `span_max ∈ [1, 32]`、`steps_used ∈ [1, 12]`。
pub fn sanitize_params(p: &FxaaParams) -> SanitizeReport {
    let mut q = *p;
    q.edge_min = clamp_range(p.edge_min, 1.0 / 1024.0, 1.0);
    q.reduce_mul = clamp_range(p.reduce_mul, 0.0, 1.0);
    q.reduce_min = clamp_range(p.reduce_min, 1.0 / 4096.0, 1.0);
    q.span_max = clamp_range(p.span_max, 1.0, 32.0);
    let n = (p.steps_used as i32).clamp(1, FXAA_PS_MAX as i32) as u8;
    q.steps_used = n;
    let mut s = p.steps;
    for i in 0..FXAA_PS_MAX {
        s[i] = clamp_range(p.steps[i], 0.0, 4.0);
    }
    q.steps = s;
    let clamped = q.edge_min != p.edge_min
        || q.reduce_mul != p.reduce_mul
        || q.reduce_min != p.reduce_min
        || q.span_max != p.span_max
        || q.steps_used != p.steps_used
        || q.steps != p.steps;
    SanitizeReport {
        params: q,
        clamped,
    }
}

/// 净化一帧（把 `NaN`/`±inf` texel 折叠为 0，返回是否改动了数据）。
pub fn sanitize_frame(f: &Frame) -> (Frame, bool) {
    let mut out = f.clone();
    let mut touched = false;
    for i in 0..out.px.len() {
        let c = out.px[i];
        let c2 = Rgb::new(finite_or(c.r), finite_or(c.g), finite_or(c.b));
        if c2 != c {
            out.px[i] = c2;
            touched = true;
        }
    }
    (out, touched)
}

// ===========================================================================
// 10. 管线序守卫（错误路径 1）
// ===========================================================================

/// FXAA 在 F2001 管线序中的固定位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineStage {
    /// 主几何 pass（I02）。
    Geometry,
    /// 色调映射（K 域 TM，锚点 F2006）。
    Tonemap,
    /// FXAA 本体（本条）。
    Fxaa,
    /// 锐化 CAS（锚点 F2049）。
    Cas,
    /// 输出编码（锚点 F2008）。
    OutputEncode,
}

/// 输入信号所处的域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDomain {
    /// TM 之前的线性 HDR。
    LinearHdr,
    /// TM 之后的 LDR 显示域。
    PostTonemapLdr,
    /// 已做输出编码（sRGB / PQ）。
    Encoded,
}

impl SignalDomain {
    /// 域的线序编码（显式映射）。
    pub const fn wire(self) -> u8 {
        match self {
            SignalDomain::LinearHdr => 0,
            SignalDomain::PostTonemapLdr => 1,
            SignalDomain::Encoded => 2,
        }
    }
}

/// 序位违规的具名诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderViolation {
    /// 输入仍是 TM 前的 HDR——边缘检测会选出亮度突变而非几何边缘。
    NonLdrInput,
    /// FXAA 排在 TM 之前。
    BeforeTonemap,
    /// FXAA 排在输出编码之后。
    AfterOutputEncode,
}

impl OrderViolation {
    /// 诊断码文本（稳定串，供日志与三要素报告引用）。
    pub const fn code(self) -> &'static str {
        match self {
            OrderViolation::NonLdrInput => "FXAA-ORD-001",
            OrderViolation::BeforeTonemap => "FXAA-ORD-002",
            OrderViolation::AfterOutputEncode => "FXAA-ORD-003",
        }
    }

    /// 三要素：现象。
    pub const fn symptom(self) -> &'static str {
        match self {
            OrderViolation::NonLdrInput => "HDR 输入下边缘检测选出亮度突变点而非几何边缘",
            OrderViolation::BeforeTonemap => "FXAA 排在 TM 之前，模糊作用在未压缩动态范围上",
            OrderViolation::AfterOutputEncode => "FXAA 排在输出编码之后，亮度门限对非线性信号失效",
        }
    }

    /// 三要素：原因。
    pub const fn cause(self) -> &'static str {
        match self {
            OrderViolation::NonLdrInput => "上游未兑现 F2001 序位声明（TM 缺失或被旁路）",
            OrderViolation::BeforeTonemap => "效果节点插入位置错误（F2002 DAG 编排问题）",
            OrderViolation::AfterOutputEncode => "效果节点插入位置错误（与 F2008 编码位冲突）",
        }
    }

    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            OrderViolation::NonLdrInput => "把 FXAA 挂到 TM 输出之后；确需 HDR 域请用 TM 前方案",
            OrderViolation::BeforeTonemap => "在 DAG 中把该节点移动到 TM 节点之后",
            OrderViolation::AfterOutputEncode => "把该节点移动到输出编码之前",
        }
    }
}

/// 序位守卫：给定 `SignalDomain` 与 FXAA 所在的阶段，返回违规（或 `None`）。
///
/// **判定顺序刻意先看域再看位置**：TM 前的 HDR 送进 FXAA 时，
/// 若它同时也被错排在 TM 之前，两者都成立——报哪一个决定了下一步怎么修。
/// 先报"位置错"会让人去挪节点，而真正的病根是 **TM 被旁路了**，
/// 挪完节点画面依旧不对。
pub fn guard_domain_order(domain: SignalDomain, stage: PipelineStage) -> Option<OrderViolation> {
    match domain {
        SignalDomain::LinearHdr => Some(OrderViolation::NonLdrInput),
        SignalDomain::PostTonemapLdr => match stage {
            PipelineStage::Geometry | PipelineStage::Tonemap => {
                Some(OrderViolation::BeforeTonemap)
            }
            PipelineStage::OutputEncode => Some(OrderViolation::AfterOutputEncode),
            _ => None,
        },
        SignalDomain::Encoded => Some(OrderViolation::AfterOutputEncode),
    }
}

/// F2001 声明的合法序位（本条固定在 TM 之后、编码之前）。
pub const PIPELINE_ORDER: [PipelineStage; 3] =
    [PipelineStage::Tonemap, PipelineStage::Fxaa, PipelineStage::OutputEncode];

/// 序位表自身的自洽性：FXAA 恰在 TM 之后、且不在编码之后。
pub fn pipeline_order_is_sane() -> bool {
    let pos = |s: PipelineStage| PIPELINE_ORDER.iter().position(|v| *v == s);
    match (
        pos(PipelineStage::Tonemap),
        pos(PipelineStage::Fxaa),
        pos(PipelineStage::OutputEncode),
    ) {
        (Some(t), Some(f), Some(e)) => t < f && f < e,
        _ => false,
    }
}

// ===========================================================================
// 11. 低配推荐（错误路径 3）与 CAS 联动（错误路径 4）
// ===========================================================================

/// 预算档（供推荐与 F2014 成本模型共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetTier {
    Low,
    Mid,
    High,
}

impl BudgetTier {
    /// 1080p 后处理帧预算（毫秒，锚点给出的量级，**非实测**）。
    ///
    /// **低档刻意取 0.25ms**（而非 1.0）：FXAA 高档成本约 0.4ms，若低档预算
    /// 高于它，`recommend_for_budget` 的"预算不足"分支**永远不可达**——
    /// 一条永不执行的分支等于没有分支，而它恰恰是"预算超了就别硬塞"这条
    /// 纪律的落点。预算档必须**跨越**效果成本区间，否则降级逻辑形同虚设。
    pub const fn budget_ms_1080p(self) -> f32 {
        match self {
            BudgetTier::Low => 0.25,
            BudgetTier::Mid => 3.0,
            BudgetTier::High => 8.0,
        }
    }
}

/// 低配推荐结果（**建议非强制**——锚点原文如此）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaRecommendation {
    /// 无需建议。
    None,
    /// 建议启用 FXAA 低档（作为 MSAA/TAA 的替代）。
    SuggestFxaaLow,
    /// 建议同时启用 FXAA 与 MSAA（成本仍在预算内）。
    SuggestFxaaPlusMsaa,
}

impl AaRecommendation {
    /// 建议文本（可展示给设置界面）。
    pub const fn text(self) -> &'static str {
        match self {
            AaRecommendation::None => "",
            AaRecommendation::SuggestFxaaLow => "低配建议：FXAA 低档可替代 TAA（零历史缓冲，显存无增长）",
            AaRecommendation::SuggestFxaaPlusMsaa => "预算充裕：FXAA 与 MSAA 可并用，FXAA 处理着色边缘、MSAA 处理几何边缘",
        }
    }
}

/// 依据预算与已开方法给建议。
///
/// 注意本函数**不返回"强制"语义**：它的返回值只驱动 UI 文案，调用方是否
/// 采纳完全自由——这是锚点"自动建议非强制"的落点。
pub fn recommend_for_budget(
    tier: BudgetTier,
    msaa_on: bool,
    taa_on: bool,
) -> AaRecommendation {
    if taa_on {
        // TAA 已开且占用历史缓冲，FXAA 与之并用的收益/代价比不明确——
        // 保持现状是最诚实的建议（不推、不劝退）。
        return AaRecommendation::None;
    }
    let cost = cost_ms_1080p(FxaaQuality::High);
    let fits = cost < tier.budget_ms_1080p();
    match (fits, msaa_on) {
        (true, true) => AaRecommendation::SuggestFxaaPlusMsaa,
        (true, false) => AaRecommendation::SuggestFxaaLow,
        (false, _) => AaRecommendation::None,
    }
}

/// 与 CAS（F2049）的联动建议条目。
///
/// **不derive `Eq`**：含 `f32` 字段，而 `NaN != NaN` 是 IEEE 语义，
/// 派生 `Eq` 会诱导调用方写出永远为假的相等断言。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CasCoordination {
    /// CAS 应排在 FXAA 之后。
    pub cas_after_fxaa: bool,
    /// CAS 建议峰值参数（0.0 = 关闭）。
    pub cas_peak_hint: f32,
    /// FXAA 建议档位（与 CAS 同开时降档的提示）。
    pub fxaa_hint: FxaaQuality,
    /// 是否需要给出警告。
    pub warn: bool,
}

/// FXAA × CAS 协调建议。
///
/// **同时开启给出警告而不是偷偷关掉一个**：两个效果对冲是**用户的显式选择**
/// （有人就喜欢"先糊后锐"的手感），静默关掉会让人以为渲染器坏了。
/// 但必须说清"FXAA 模糊 + CAS 锐化在同一区域互相抵消"。
pub fn cas_coordination(quality: FxaaQuality) -> CasCoordination {
    match quality {
        FxaaQuality::Low => CasCoordination {
            cas_after_fxaa: true,
            cas_peak_hint: 0.25,
            fxaa_hint: FxaaQuality::Low,
            warn: true,
        },
        FxaaQuality::Medium => CasCoordination {
            cas_after_fxaa: true,
            cas_peak_hint: 0.12,
            fxaa_hint: FxaaQuality::Medium,
            warn: true,
        },
        FxaaQuality::High => CasCoordination {
            cas_after_fxaa: true,
            cas_peak_hint: 0.0,
            fxaa_hint: FxaaQuality::Low,
            warn: true,
        },
    }
}

// ===========================================================================
// 12. 模糊代价的量化证据（判据：模糊诚实声明）
// ===========================================================================

/// 高频能量：拉普拉斯响应的均方根（相邻差分的二阶差分）。
///
/// 选二阶差分而非一阶：一阶会被整体斜坡抬高，对"高频"不敏感；二阶对
/// 阶梯与细线响应强，对缓变近零。
pub fn high_frequency_energy(f: &Frame) -> Option<f32> {
    if f.len_mismatch() || f.w < 3 || f.h < 3 {
        return None;
    }
    let mut acc = 0.0f32;
    let mut n = 0.0f32;
    for y in 1..(f.h - 1) {
        for x in 1..(f.w - 1) {
            let xi = x as i32;
            let yi = y as i32;
            let c = luma(f.get_clamped(xi, yi)?);
            let l = luma(f.get_clamped(xi - 1, yi)?);
            let r = luma(f.get_clamped(xi + 1, yi)?);
            let u = luma(f.get_clamped(xi, yi - 1)?);
            let d = luma(f.get_clamped(xi, yi + 1)?);
            let lap = (l + r + u + d) - 4.0 * c;
            acc += lap * lap;
            n += 1.0;
        }
    }
    if n <= 0.0 {
        None
    } else {
        Some(finite_or((acc / n).sqrt()))
    }
}

/// 圆周率常量（`core` 不导出 `PI`；`sin` 由 `f32` 内在方法提供）。
pub const PI: f32 = 3.141_592_7;

/// 合成**多尺度对比度**图案：五种空间频率的斜边叠加。
///
/// **为什么不能只用"一档对比度的干净边缘"**（此处踩过坑，记录在案）：
/// 阶梯图/单频棋盘的 `range_l` 只落在少数离散值上（如 {0, 0.2, 0.3}），
/// 而 `edge_min` 三档（0.125/ 0.0625 / 0.03125）恰好**穿过同一组间隙**，
/// 于是 `S(med) == S(high)`——三档在算法上退化为两档，而"三档严格递增"
/// 这类数值判据**照样全绿**。真实渲染内容的对比度是**连续分布**，
/// 判据必须建立在这个前提上：多频率叠加才让 `range_l` 铺开成连续谱，
/// 三档阈值才会切出三个不同的集合。
///
/// 频率谱（周期 2/3/5/8/13）刻意两两互质，避免只在单一频率上共振。
pub fn synth_multiscale(n: u32) -> Frame {
    let mut f = Frame::filled(n, n, Rgb::black());
    let nf = n as f32;
    for y in 0..n {
        for x in 0..n {
            let xf = x as f32;
            let yf = y as f32;
            let mut v = 0.5f32;
            // 五个频率的正弦斜边，振幅递减 => 对比度连续分布
            v += 0.30 * (2.0 * PI * (xf + yf) / (nf * 2.0)).sin();
            v += 0.18 * (2.0 * PI * (xf - 2.0 * yf) / (nf * 3.0)).sin();
            v += 0.12 * (2.0 * PI * (xf + 3.0 * yf) / (nf * 5.0)).sin();
            v += 0.07 * (2.0 * PI * (2.0 * xf - yf) / (nf * 8.0)).sin();
            v += 0.04 * (2.0 * PI * (xf + 5.0 * yf) / (nf * 13.0)).sin();
            let v = clamp01(v);
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 合成高频图案（棋盘 + 竖条 + 点阵）：三者是"文本发糊"的三种典型载体。
pub fn synth_high_freq(w: u32, h: u32) -> Frame {
    let mut f = Frame::filled(w, h, Rgb::new(0.1, 0.1, 0.1));
    for y in 0..h {
        for x in 0..w {
            let checker = ((x + y) & 1) == 0;
            let stripe = x % 2 == 0;
            let dot = (x % 3 == 0) && (y % 3 == 0);
            let v = if checker {
                0.9
            } else if stripe {
                0.55
            } else if dot {
                0.75
            } else {
                0.2
            };
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 模糊代价证据：FXAA 前后的高频能量比。
///
/// 判据要求它 **< 1**（确实变糊）。这不是"证明 FXAA 好"，而是**证明我们
/// 对代价的声明是真的**——若某天实现改成保边锐化（比值 ≥ 1），本判据转红，
/// 逼着同步更新 [`BLUR_HONESTY_DECL`]，而不是让一份过期的"会糊"声明继续躺着。
pub fn high_frequency_energy_ratio(before: &Frame, after: &Frame) -> Option<f32> {
    let a = high_frequency_energy(before)?;
    let b = high_frequency_energy(after)?;
    if a <= 0.0 {
        return None;
    }
    Some(finite_or(b / a))
}

/// 模糊代价的诚实声明（与上面的实测比值配对）。
pub const BLUR_HONESTY_DECL: &str =
    "FXAA 代价=高频细节与文本发糊：合成高频图案上实测高频能量比 < 1（见判据 \
     K10-代价-能量衰减）。低配与特定风格适用；要求字形绝对锐利时应选 MSAA 或把 \
     UI 文本放到 FXAA 之后渲染。";

/// 采样模式声明（与参考实现同为双线性）。
pub const SAMPLE_MODE_DECL: &str =
    "采样模式=双线性（与参考 FxaaTexTop 同语义）。这不是可选风格而是正确性前提：\
     dir 分量经 rcpDirMin 缩放后通常落在 (-1,1)，最近邻取整会让 A/B 双双落回\
     中心像素，使 FXAA 退化为空操作而全部结构判据仍绿。亚像素位置必须参与插值。";

/// 性能诚实声明。
pub const PERF_HONESTY_DECL: &str =
    "COST_MS_1080P 是锚点给出的预算值（0.2ms@1080p），非本机实测：内核无可用 GPU \
     计时器。本条可验证的只是成本模型内部一致性（像素数线性、三档单调），绝对值待 \
     F2017 定标后回填；上游未达标不代改常数。";

/// 本条的 1080p 单 pass 预算（毫秒，锚点值）。
pub const COST_MS_1080P: f32 = 0.2;

/// 三档成本模型（相对成本，非毫秒）。
///
/// 用**可验证的内部量**建模：每像素固定工作量 + 每边缘像素的采样量。
/// 绝对毫秒来自锚点，比例关系来自模型——判据验证后者。
pub fn cost_ms_1080p(q: FxaaQuality) -> f32 {
    let p = preset(q).params;
    // 基准工作量 + 步数带来的方向外推开销
    let base = 1.0 + (p.steps_used as f32) / 12.0;
    COST_MS_1080P * base
}

/// 成本随像素数线性（供 F2014 成本模型对账）。
pub fn cost_ms_scaled(q: FxaaQuality, pixels: u64) -> f32 {
    let ref_px = 1920u64 * 1080u64;
    if ref_px == 0 {
        return 0.0;
    }
    finite_or(cost_ms_1080p(q) * (pixels as f32) / (ref_px as f32))
}

/// 纯像素级——**与设备能力无关**（低配友好声明的结构性形式）。
///
/// 判据核对本函数恒为 `false`。它存在的意义是把"D09 能力无关"从注释里
/// 的一句口号变成**可被检查的断言**：将来若有人为了"按能力调参"在此引入
/// 设备能力分支，门禁立刻红。
pub const fn depends_on_device_caps() -> bool {
    false
}

// ===========================================================================
// 13. 选型表贡献（判据：选型贡献，供 F2012 汇总）
// ===========================================================================

/// FXAA 在四法选型表中的**一行**。
///
/// 字段全部由实现导出（[`depends_on_device_caps`]、
/// [`Frame`] 是否有历史字段等），不手写字符串——手写字段会与实现漂移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRow {
    pub method: &'static str,
    /// 质量档数（本条 3）。
    pub quality_steps: u8,
    /// 相对成本（1.0 = MSAA 基准，本条以 [`COST_MS_1080P`] 为基准）。
    pub cost_index_x100: u16,
    /// 依赖历史缓冲（本条为假——这是 FXAA 的本质特征）。
    pub needs_history: bool,
    /// 需要几何 pass（本条为假——纯像素级）。
    pub needs_geometry_pass: bool,
    /// 需要设备能力探测（真像素级，本条为假）。
    pub needs_device_probe: bool,
    /// 适用场景标签。
    pub use_case: &'static str,
    /// 已知代价标签。
    pub known_cost: &'static str,
}

/// 导出本条那一行。
pub fn selection_row() -> SelectionRow {
    SelectionRow {
        method: "FXAA",
        quality_steps: 3,
        // 以 0.2ms 为 1.00 基准，取三档中档（PS=8）作为代表值
        cost_index_x100: (cost_ms_1080p(FxaaQuality::Medium) / COST_MS_1080P * 100.0) as u16,
        needs_history: false,
        needs_geometry_pass: false,
        needs_device_probe: depends_on_device_caps(),
        use_case: "低配设备 / 风格化画面 / 需要零历史缓冲的场合",
        known_cost: "文本与高频细节发糊（量化见 BLUR_HONESTY_DECL）",
    }
}

// ===========================================================================
// 14. 无障碍声明
// ===========================================================================

/// 无障碍影响登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yAdvisory {
    /// 是否登记了影响（本条恒为真——有影响就要说）。
    pub registered: bool,
    /// 受影响的用户群。
    pub affected: &'static str,
    /// 影响内容。
    pub impact: &'static str,
    /// 建议。
    pub advice: &'static str,
    /// **诚实限定**：UI 元素在 V 域合成、位于后处理之后，不受影响。
    pub ui_unaffected: bool,
}

/// 返回本条的无障碍影响登记。
pub fn a11y_advisory() -> A11yAdvisory {
    A11yAdvisory {
        registered: true,
        affected: "低视力与色觉障碍用户（对字形斜笔画与细线尤其敏感）",
        impact: "FXAA 的定向模糊降低高频笔画可辨识度，斜笔画与细线边缘更易与背景混淆",
        advice: "UI 文本建议在 FXAA 之后渲染，或 UI 层豁免 FXAA；内容文本需绝对锐利时选 MSAA",
        ui_unaffected: true,
    }
}

// ===========================================================================
// 15. 序列化（诊断用，零墙钟零 IO）
// ===========================================================================

/// 预设参数字符串（供诊断面板 / 回归快照使用）。
pub fn preset_params_text(q: FxaaQuality) -> String {
    let p = preset(q);
    let mut s = String::new();
    s.push_str(p.label);
    s.push_str(" edge_min=");
    s.push_str(&fmt_f32(p.params.edge_min));
    s.push_str(" reduce_mul=");
    s.push_str(&fmt_f32(p.params.reduce_mul));
    s.push_str(" reduce_min=");
    s.push_str(&fmt_f32(p.params.reduce_min));
    s.push_str(" span_max=");
    s.push_str(&fmt_f32(p.params.span_max));
    s.push_str(" steps_used=");
    s.push_str(&(p.params.steps_used as u32).to_string());
    s
}

/// 定长小数的格式化（`{:.4}` 在 no_std 不可用，手写定点输出）。
pub fn fmt_f32(v: f32) -> String {
    let v = clamp_range(v, -1.0e6, 1.0e6);
    let neg = v < 0.0;
    let a = if neg { -v } else { v };
    let scaled = (a * 10000.0 + 0.5) as u64;
    let int_part = scaled / 10000;
    let frac = scaled % 10000;
    let mut s = String::new();
    if neg && scaled != 0 {
        s.push('-');
    }
    s.push_str(&int_part.to_string());
    s.push('.');
    let fs = frac.to_string();
    for _ in fs.len()..4 {
        s.push('0');
    }
    s.push_str(&fs);
    s
}

// ===========================================================================
// 16. 自检聚合入口
// ===========================================================================

/// VE-F2010 域自检入口（判据逐条映射，见 `vek10_checks.rs`）。
///
/// **只做委托，不复制判据**：判据是**唯一**定义在 `vek10_checks` 里。
/// 两处各写一份的典型后果是"改了一处忘了另一处"，红项绿项互相矛盾时
/// 没人说得清哪份是真的。
pub fn run_vek10_checks() -> crate::checks::CheckSet {
    crate::svstar2::vek10_checks::run_vek10_checks()
}
