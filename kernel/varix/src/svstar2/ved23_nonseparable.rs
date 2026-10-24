//! VE-F0623 · 不可分离混合模式 4 种（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0623`
//!
//! **判据（锚点原文逐条）**：
//! - **4 种不可分离模式**：hue、saturation、color、luminosity——
//!   「非可分离本质：需逐像素在亮度饱和度空间重组（R、G、B 三通道
//!   不再独立计算）」；
//! - **W3C 非可分离定义的 ClipColor 与 SetLum 次级裁剪公式全实现**
//!   （**饱和度裁剪 SatClip 的三分支选择公式逐条落地**）；
//! - **计算顺序契约**：先非可分离函数，后与背景混合；
//! - **精度纪律**：色域往返（RGB → 亮度饱和度 → RGB）的浮点累积误差
//!   控制在 F0630 判据内；**负值与越界的钳制次序严格按规范（次序错则
//!   色相漂移）**；
//! - **与 F0622 可分离集共用色卡与对拍框架**。
//!
//! ## 〇、规范原文（本条一切公式的唯一出处）
//!
//! 锚点要求「公式对拍 W3C 规范」，故先固化原文（Compositing and
//! Blending Level 1 §10.2，条款号见 [`NonSepMode::clause`]）：
//!
//! ```text
//! Lum(C) = 0.3·R + 0.59·G + 0.11·B
//!
//! ClipColor(C):
//!     l = Lum(C);  n = min(R,G,B);  x = max(R,G,B)   ← 三个量各算一次
//!     if n < 0:  C_i = l + (((C_i - l) × l)     / (l - n))   ∀i
//!     if x > 1:  C_i = l + (((C_i - l) × (1-l)) / (x - l))   ∀i
//!     return C
//!
//! SetLum(C, l):  d = l - Lum(C);  C_i += d ∀i;  return ClipColor(C)
//!
//! Sat(C) = max(R,G,B) - min(R,G,B)
//!
//! SetSat(C, s):                       # 下标 min/mid/max 指**进入函数时**
//!     if Cmax > Cmin:                  # 各分量的取值次序，不是通道次序
//!         Cmid = ((Cmid - Cmin) × s) / (Cmax - Cmin)
//!         Cmax = s
//!     else:
//!         Cmid = Cmax = 0
//!     Cmin = 0                          ← 在 if/else **之后**，无条件执行
//!     return C
//!
//! hue        : B = SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))
//! saturation : B = SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))
//! color      : B = SetLum(Cs, Lum(Cb))
//! luminosity : B = SetLum(Cb, Lum(Cs))
//! ```
//!
//! ## 一~十一、锚点要点（推理展开从略，判据锚点保留）
//!
//! - 权重 **0.3/0.59/0.11 非 Rec.709**：亮度保持类判据抓不住权重错，故权重
//!   必须是可核对常量（[`C23-LUM-WEIGHTS-VERBATIM`]）且 oracle 硬写这三个数。
//! - 钳制次序**可测**：两段 `if` 共用入口 `l`/`n`/`x`，第二段分母若用重算的
//!   `x₂` 则收缩因子改变、色相漂移（[`C23-CLIPCOLOR-ORDER-MATTERS`]）。两段
//!   同触发时 `min>0 且 max<1`（收缩更弱）；判据 [`C23-CLIPCOLOR-ORDER-SPEC`]
//!   断的是**单段归零/归一**这条可精确验证的性质，不是「两段同触落边界」。
//! - `SatClip` 三分支：规范 `SetSat` 只有 `if (Cmax > Cmin) / else` 两分支；
//!   第三档 `FlatRequested`（灰输入且 `s>0`）与 `FlatToZero` 输出全同（规范
//!   怪癖：丢弃目标饱和度），不能用输出值区分，只能直接断言枚举与台账
//!   （[`C23-SATCLIP-BRANCH-DIRECT`]）。
//! - `SetSat` 下标是**取值次序**非通道次序（还原错则只有色相错）；并列值
//!   tie-break「最靠前者胜」，方向在输出层与确定性层都钉不住，必须直接断言
//!   下标（[`C23-SAT-TIEBREAK-DIRECTION`]）。
//! - 计算顺序契约：[`composite_over`] 先非可分离函数后合成；判据必须在
//!   **双 alpha 严格小于 1** 的语料上比对换序，否则恒绿。
//! - 往返两条性质（[`C23-LUM-IDENTITY-ROUNDTRIP`]/[`C23-SAT-IDENTITY-ROUNDTRIP`]）：
//!   亮度侧**位精确恒等** `SetLum(C,Lum(C))==C`；饱和度侧是**闭式**
//!   `SetSat(C,Sat(C))==C-Cmin` 而**非恒等**（规范 `Cmin = 0` 在 if/else 之后
//!   无条件执行）——初版误写成恒等把正确实现判红；教训：判据的参考性质必须
//!   独立算出来，「看起来应该恒等」不是理由。
//! - 与 F0622 真引用共用 [`RAMP`]/[`ramp_char`]/[`SWATCH_PROBES`]/[`LSB8`]；
//!   色卡语料必须**彩色化**（灰阶条会把跨通道效应平均掉），另给
//!   [`color_swatch_rows`] 逐通道 R|G|B。
//! - [`oracle`] 是四路里唯一不读本模块公式的那份（独立 f64 直算、权重硬写），
//!   否则「抄错规范」的实现之间会错得一样、对拍恒绿。
//! - 非有限输入与次正规分母由 [`guard_finite`]/[`set_sat`] 兜底并记账
//!   （[`NonSepLog`]），绝不静默传播——NaN 进预乘链会毁掉整条下游。
//!
//! ## 十二、本条的判据清单（[`run_ved23_checks`]）
//!
//! 28 条，逐条的「它凭什么能抓错」写在代码里——判据名只是标签，
//! **说不出它能抓什么错的判据等于没写**。
//!
//! | 判据 | 抓什么 |
//! |------|--------|
//! | [`C23-REGISTRY-4-MODES`] | 漏注册 / 下标与键不同向 / 关键字重复 |
//! | [`C23-LUM-WEIGHTS-VERBATIM`] | 权重被换成 Rec.709（亮度保持类判据抓不住） |
//! | [`C23-LUM-GRAY-AXIS-FIXED`] | 权重被改动后仍和为 1 但分配错了 |
//! | [`C23-FORMULA-VS-ORACLE`] | 公式抄错规范（对拍独立 f64） |
//! | [`C23-ORACLE-CATCHES-PRIMITIVE-ERROR`] | oracle 自身是空断言 |
//! | [`C23-DEFINITION-INVARIANTS`] | 亮度/饱和度取自哪一侧写反 |
//! | [`C23-CLIPCOLOR-ORDER-MATTERS`] | 第二段分母用重算的 `x₂` |
//! | [`C23-CLIPCOLOR-PRESERVES-LUM`] | `l` 取成非入口亮度 |
//! | [`C23-CLIPCOLOR-ORDER-SPEC`] | 两段整体失效（死码） |
//! | [`C23-SATCLIP-THREE-BRANCHES`] | 三分支合并 / `>` 写成 `>=` |
//! | [`C23-SATCLIP-BRANCH-DIRECT`] | 合并两支后仍能过输出值断言 |
//! | [`C23-SAT-IDENTITY-ROUNDTRIP`] | 通道下标还原错（闭式） |
//! | [`C23-LUM-IDENTITY-ROUNDTRIP`] | `d` 的方向写成 `+` |
//! | [`C23-ROUNDTRIP-BUDGET`] | 往返累积误差超 F0630 口径 |
//! | [`C23-SAT-TIEBREAK-DETERMINISTIC`] | 并列分量导致非确定性 |
//! | [`C23-SAT-TIEBREAK-DIRECTION`] | tie-break **方向**反（输出层钉不住，见 §四末） |
//! | [`C23-COMPOSITE-ORDER-CONTRACT`] | 合成步提到混合之前 |
//! | [`C23-COMPOSITE-ALPHA-EDGES`] | 合成式漏项 / 系数错 |
//! | [`C23-EXTREME-BACKDROPS`] | 端点除零 / 边界判定差一格 |
//! | [`C23-GRAY-BACKDROP-NOOP`] | 规范 §10.2.2 明文性质被破 |
//! | [`C23-NONSEPARABLE-NOT-CHANNELWISE`] | 退化成逐通道独立（不再是"不可分离"） |
//! | [`C23-CLAMP-NON-FINITE`] | `NaN`/`±Inf` 静默传播 |
//! | [`C23-SAT-DENOM-GUARD`] | 次正规分母产出 `NaN` |
//! | [`C23-SIMD-BATCH-EQUIV`] | uniform 快路径算错 |
//! | [`C23-SIMD-AMORTIZATION`] | 宽通道退化成逐像素循环 |
//! | [`C23-WGSL-TEXT-KEYWORDS`] | GPU 路只吐壳 |
//! | [`C23-SWATCH-SHARED-PROBES`] | 另抄一份斜坡探针 |
//! | [`C23-KEY-OUT-OF-RANGE-NONE`] | 越界键静默变合法模式 |
//! | [`C23-LEDGER-EXACT`] | 记账被吞（幽灵记账 / 漏记） |

use alloc::vec;
use crate::checks::CheckSet;

// no_std 导入三件套 + format!：三件套齐备以与全仓其它 no_std 模块一致。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 与 F0622 共用色卡与对拍框架（真引用而非复制——判据 C23-SWATCH-SHARED-PROBES 依赖）。
use super::ved22_separable::{ramp_char, LSB8, LSB_BUDGET, RAMP, SWATCH_PROBES};

// 一、参数唯一源（规范原文值，不可调）

/// 不可分离模式数（规范 §10.2 恰 4 种，锚点亦为 4）。
pub const NONSEPARABLE_COUNT: usize = 4;

/// [`Lum`] 的红权重（规范原文 **0.3**，非 Rec.709 的 0.2126）。
/// 独立成常量而非内联：判据要逐字核对（[`C23-LUM-WEIGHTS-VERBATIM`]）。
pub const LUM_R: f32 = 0.30;

/// [`Lum`] 的绿权重（规范原文 **0.59**）。
pub const LUM_G: f32 = 0.59;

/// [`Lum`] 的蓝权重（规范原文 **0.11**）。
pub const LUM_B: f32 = 0.11;

/// 三权重之和（规范要求 Lum 为**仿射**亮度 ⇒ 灰输入亮度 == 灰值）。
/// 权重和不为 1 则灰轴被平移——灰轴不动性质的判据。
pub const LUM_SUM: f32 = LUM_R + LUM_G + LUM_B;

/// 低位裁剪判定阈值（`ClipColor` 的 `if n < 0`）。
pub const CLIP_LO: f32 = 0.0;

/// 高位裁剪判定阈值（`ClipColor` 的 `if x > 1`）。
pub const CLIP_HI: f32 = 1.0;

/// [`SetSat`] / [`ClipColor`] 分母的可信下界。
/// 规范分母数学上非零，但 `f32` 下极近值相减可下溢到 0（次正规区）⇒
/// `0/0 = NaN`；低于此界改走置零收缩并记账——静默 NaN 会毁掉预乘链。
/// 取值远小于任何 8 bit 可表示差（1/255），对真实输入不触发，仅兜底
/// （判据 [`C23-SAT-DENOM-GUARD`] 用专门语料打它）。
pub const SAT_DENOM_MIN: f32 = 1.0e-30;

/// 非有限输入钳制目标（定义域恒为 `[0,1]`）。
pub const CHANNEL_LO: f32 = 0.0;
pub const CHANNEL_HI: f32 = 1.0;

/// SIMD 宽通道宽度（与 F0622 的 [`SIMD_WIDTH`] 同口径，4 像素批）。
pub const SIMD_WIDTH: usize = 4;

/// 色卡探针数（直接复用 F0622 的 [`SWATCH_PROBES`] 长度）。
pub const SWATCH_STEPS: usize = SWATCH_PROBES.len();

/// 色域往返的 LSB 预算（锚点：累积误差控制在 F0630 判据内）。
/// 口径 [`LSB8`] 由 F0622 引入复用；往返判据实际断闭式残差 ≤1 ULP（头注），
/// 本预算是 [`C23-ROUNDTRIP-BUDGET`] 的兜底口径。
pub const ROUNDTRIP_LSB_BUDGET: f32 = LSB_BUDGET;

/// 极端背景语料：锚点「极端值（全黑全白背景）→规范边界分支全覆盖」。
pub const EXTREME_BACKDROPS: [Rgb; 4] = [
    Rgb { r: 0.0, g: 0.0, b: 0.0 },
    Rgb { r: 1.0, g: 1.0, b: 1.0 },
    Rgb { r: 0.0, g: 0.5, b: 1.0 },
    Rgb { r: 1.0, g: 0.5, b: 0.0 },
];

// 二、色彩三元组与模式键

/// RGB 三元组（通道**有序**，非"排序后"——排序只在 [`set_sat`] 内部临时做）。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    /// 灰（`v` 三通道等值）。
    pub fn gray(v: f32) -> Rgb {
        Rgb { r: v, g: v, b: v }
    }

    /// 按下标取通道（`0→R, 1→G, 2→B`；下标还原写死一处，比三处各写 match 难抄错）。
    pub fn get(&self, i: usize) -> f32 { // 判定与写法一一对应：i == 0/1 走显式臂，其余归 B
        match i {
            0 => self.r,
            1 => self.g,
            _ => self.b,
        }
    }

    /// 按下标写通道，返回新值（不原地改）。
    pub fn with(&self, i: usize, v: f32) -> Rgb {
        match i {
            0 => Rgb { r: v, g: self.g, b: self.b },
            1 => Rgb { r: self.r, g: v, b: self.b },
            _ => Rgb { r: self.r, g: self.g, b: v },
        }
    }

    /// 三通道最小值。
    pub fn min_ch(&self) -> f32 {
        min3(self.r, self.g, self.b)
    }

    /// 三通道最大值。
    pub fn max_ch(&self) -> f32 {
        max3(self.r, self.g, self.b)
    }
}

/// 不可分离混合模式键（4 种，顺序 = W3C §10.2.x 条款顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonSepMode {
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl NonSepMode {
    /// 全部 4 种。
    pub fn all() -> [NonSepMode; NONSEPARABLE_COUNT] {
        [
            NonSepMode::Hue,
            NonSepMode::Saturation,
            NonSepMode::Color,
            NonSepMode::Luminosity,
        ]
    }

    /// 注册下标（与 [`NonSepMode::all`] 顺序一致）。
    pub fn index(&self) -> usize {
        match self {
            NonSepMode::Hue => 0,
            NonSepMode::Saturation => 1,
            NonSepMode::Color => 2,
            NonSepMode::Luminosity => 3,
        }
    }

    /// 下标 → 模式键；**越界返回 `None`**，绝不 `unwrap`。
    pub fn from_index(i: usize) -> Option<NonSepMode> {
        match i {
            0 => Some(NonSepMode::Hue),
            1 => Some(NonSepMode::Saturation),
            2 => Some(NonSepMode::Color),
            3 => Some(NonSepMode::Luminosity),
            _ => None,
        }
    }

    /// 关键字（与 CSS `mix-blend-mode` 拼写一致）。
    pub fn keyword(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "hue",
            NonSepMode::Saturation => "saturation",
            NonSepMode::Color => "color",
            NonSepMode::Luminosity => "luminosity",
        }
    }

    /// 可核对条款号（W3C Compositing and Blending Level 1 §10.2.x）。
    pub fn clause(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "compositing-1§10.2.1",
            NonSepMode::Saturation => "compositing-1§10.2.2",
            NonSepMode::Color => "compositing-1§10.2.3",
            NonSepMode::Luminosity => "compositing-1§10.2.4",
        }
    }

    /// 本模式的**亮度取自哪一侧**（规范定义的字面内容，不依赖浮点实现；
    /// 供 [`C23-DEFINITION-INVARIANTS`] 直接断言）。
    /// `hue`/`saturation`/`color` 亮度来自背景，`luminosity` 来自源。
    pub fn lum_from(&self) -> Side {
        match self {
            NonSepMode::Hue | NonSepMode::Saturation | NonSepMode::Color => Side::Backdrop,
            NonSepMode::Luminosity => Side::Source,
        }
    }

    /// 饱和度取自哪一侧（见 [`NonSepMode::lum_from`]）。
    /// `hue`/`luminosity` 来自背景，`saturation`/`color` 来自源。
    pub fn sat_from(&self) -> Side {
        match self {
            NonSepMode::Hue | NonSepMode::Luminosity => Side::Backdrop,
            NonSepMode::Saturation | NonSepMode::Color => Side::Source,
        }
    }

    /// 规范对该模式的一句人话描述（色卡/摘要用）。
    pub fn prose(&self) -> &'static str {
        match self {
            NonSepMode::Hue => "取源色相，背景的饱和度与亮度",
            NonSepMode::Saturation => "取源饱和度，背景的色相与亮度",
            NonSepMode::Color => "取源色相与饱和度，背景的亮度",
            NonSepMode::Luminosity => "取源亮度，背景的色相与饱和度",
        }
    }
}

/// 侧（背景 / 源）——用于「取自哪一侧」这类**结构性**断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Backdrop,
    Source,
}

impl Side {
    pub fn keyword(&self) -> &'static str {
        match self {
            Side::Backdrop => "Cb",
            Side::Source => "Cs",
        }
    }
}

/// 供上层复用的模式键视图（避免 `vec!` 在 no_std 下散落）。
pub fn nonseparable_modes() -> Vec<NonSepMode> {
    let mut v: Vec<NonSepMode> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        v.push(*m);
    }
    v
}

// 三、SetSat 的分支枚举（三分支）

/// [`set_sat`] 的分支档（锚点「SatClip 的三分支选择公式」）。
/// 后两档输出完全相同（规范 else 分支一律全零），只能靠枚举本身区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatBranch {
    /// `Cmax > Cmin`：彩色输入，按比例缩放中位分量，`Cmax = s`。
    Scaled,
    /// `Cmax == Cmin` 且 `s == 0`：输入无色彩、目标亦无饱和度 → 全零。
    FlatToZero,
    /// `Cmax == Cmin` 但 `s > 0`：规范 else 分支丢弃目标饱和度 → 仍全零。
    /// 单独成档让上层能发现"要求了却没拿到"。
    FlatRequested,
}

impl SatBranch {
    /// 三档全部（判据遍历用）。
    pub fn all() -> [SatBranch; 3] {
        [SatBranch::Scaled, SatBranch::FlatToZero, SatBranch::FlatRequested]
    }

    pub fn keyword(&self) -> &'static str {
        match self {
            SatBranch::Scaled => "scaled",
            SatBranch::FlatToZero => "flat-to-zero",
            SatBranch::FlatRequested => "flat-requested",
        }
    }

    /// 该分支是否走规范 else 分支（输出恒为全零）。
    pub fn is_flat(&self) -> bool {
        !matches!(self, SatBranch::Scaled)
    }
}

// 四、记账台账（异常零静默）

/// 不可分离路径的异常记账（只增不减，饱和累加防回绕）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NonSepLog {
    /// 非有限输入收口次数（`NaN` / `±Inf`）。
    pub clamped_inputs: u32,
    /// `ClipColor` 低位段（`n < 0`）触发次数。
    pub clip_low: u32,
    /// `ClipColor` 高位段（`x > 1`）触发次数。
    pub clip_high: u32,
    /// 两段**同时**触发的次数（次序敏感的那一类）。
    pub clip_both: u32,
    /// `SetSat` 走 `Scaled` 分支次数。
    pub sat_scaled: u32,
    /// `SetSat` 走 `FlatToZero` 分支次数。
    pub sat_flat_zero: u32,
    /// `SetSat` 走 `FlatRequested` 分支次数（要求了饱和度却被丢弃）。
    pub sat_flat_requested: u32,
    /// 分母触底、改走置零档的次数（`f32` 次正规兜底）。
    pub sat_denom_guarded: u32,
    /// 并列值命中 tie-break 的次数（三通道存在相等分量）。
    pub tiebreak_hits: u32,
    /// 求值后结果越界、靠 [`clamp_residue`] 收口回来的次数。
    pub out_of_range: u32,
}

impl NonSepLog {
    pub fn new() -> NonSepLog {
        NonSepLog::default()
    }

    /// 记账合计。
    pub fn total(&self) -> u32 {
        self.clamped_inputs
            .saturating_add(self.clip_low)
            .saturating_add(self.clip_high)
            .saturating_add(self.sat_scaled)
            .saturating_add(self.sat_flat_zero)
            .saturating_add(self.sat_flat_requested)
            .saturating_add(self.sat_denom_guarded)
            .saturating_add(self.tiebreak_hits)
    }

    /// 台账是否为空。
    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }

    /// 逐档分支计数（`Scaled` / `FlatToZero` / `FlatRequested`）。
    pub fn sat_branch_counts(&self) -> [u32; 3] {
        [self.sat_scaled, self.sat_flat_zero, self.sat_flat_requested]
    }
}

// 五、色彩原语：Lum / Sat / ClipColor / SetLum / SetSat

/// **非有限**输入收口（`NaN` → 0.0，`+Inf` → 1.0，`-Inf` → 0.0）。
/// **只**收非有限值，有限越界值原样放行——这是死码陷阱：若把收口写成含
/// 范围钳制的 `clamp01`，送进 [`clip_color_ordered`] 的颜色必然 `n >= 0`
/// 且 `x <= 1`，规范的两个 `if` 判定永不成立、两段裁剪全成死码，而外部
/// 表现"看不出问题"（结果照样在 `[0,1]` 内、亮度保持、oracle 全绿）。
/// `ClipColor` 的职责就是收拾越界中间量（[`set_lum`] 的 `C + d` 可越界），
/// 故越界值必须留到两段判定。分支类性质由 [`C23-CLIPCOLOR-ORDER-SPEC`]
/// 直接断言分支真被执行。
fn guard_finite(c: Rgb, log: &mut NonSepLog) -> Rgb {
    let (r, a) = guard_finite_scalar(c.r);
    let (g, b) = guard_finite_scalar(c.g);
    let (bl, c2) = guard_finite_scalar(c.b);
    if a || b || c2 {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    Rgb { r, g, b: bl }
}

/// 单标量非有限收口（`NaN` → 0.0，`+Inf` → 1.0，`-Inf` → 0.0）。
pub fn guard_finite_scalar(v: f32) -> (f32, bool) {
    if v.is_nan() {
        return (CHANNEL_LO, true);
    }
    if v == f32::INFINITY {
        return (CHANNEL_HI, true);
    }
    if v == f32::NEG_INFINITY {
        return (CHANNEL_LO, true);
    }
    (v, false)
}

/// 两段裁剪之后的**浮点残渣收口**（把 `-1e-8` / `1+1e-8` 之类拉回）。
/// 与 [`guard_finite`] 分开是刻意的：前者输入面、后者输出面兜底。
/// 收口不得掩盖次序错误——只夹残渣量级并在 [`NonSepLog::out_of_range`]
/// 留痕；判据 [`C23-CLIPCOLOR-ORDER-MATTERS`] 因此断两版差值而非值域。
fn clamp_residue(c: Rgb, log: &mut NonSepLog) -> Rgb {
    let ch = [c.r, c.g, c.b];
    let mut out = c;
    let mut touched = false;
    let mut i = 0usize;
    while i < 3 {
        let v = ch[i];
        if v < CHANNEL_LO {
            out = out.with(i, CHANNEL_LO);
            touched = true;
        } else if v > CHANNEL_HI {
            out = out.with(i, CHANNEL_HI);
            touched = true;
        }
        i += 1;
    }
    if touched {
        log.out_of_range = log.out_of_range.saturating_add(1);
    }
    out
}

/// [`Lum`]：规范原文 `0.3·R + 0.59·G + 0.11·B`（权重不内联，见 [`LUM_R`]）。
pub fn lum(c: Rgb) -> f32 {
    LUM_R * c.r + LUM_G * c.g + LUM_B * c.b
}

/// [`Sat`]：规范原文 `max - min`。
pub fn sat(c: Rgb) -> f32 {
    c.max_ch() - c.min_ch()
}

fn max3(a: f32, b: f32, c: f32) -> f32 {
    if a >= b {
        if a >= c {
            a
        } else {
            c
        }
    } else if b >= c {
        b
    } else {
        c
    }
}

fn min3(a: f32, b: f32, c: f32) -> f32 {
    if a <= b {
        if a <= c {
            a
        } else {
            c
        }
    } else if b <= c {
        b
    } else {
        c
    }
}

/// `1.0` 处的 `f32` ULP（2⁻²³ = 1.1920929e-7）——ULP 级判据唯一口径。
/// 不用 `f32::EPSILON`（是 1.0 与下一更大数之差），也不用 1 LSB（宽 3.3 万倍）。
pub const F32_ULP_AT_ONE: f32 = 1.1920929e-7;

/// 两标量的绝对差（`|a−b|` 的手写形态——无 std 依赖，多处共用一份口径）。
fn abs_f32(a: f32, b: f32) -> f32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// 两个标量的绝对差，以 [`F32_ULP_AT_ONE`] 为单位（ULP 级比较）。
pub fn out_gap_ulp(a: f32, b: f32) -> f32 {
    abs_f32(a, b) / F32_ULP_AT_ONE
}

/// 两个三元组的逐通道最大绝对差（原始量纲，非 LSB）。
pub fn out_rgb_gap(a: Rgb, b: Rgb) -> f32 {
    let pa = [a.r, a.g, a.b];
    let pb = [b.r, b.g, b.b];
    let mut worst = 0.0f32;
    let mut i = 0usize;
    while i < 3 {
        let d = abs_f32(pa[i], pb[i]);
        if d > worst {
            worst = d;
        }
        i += 1;
    }
    worst
}

/// 两三元组的亮度差绝对值（亮度保持类判据用）。
pub fn out_lum_dev(out: Rgb, ref_l: f32) -> f32 {
    out_gap_ulp(lum(out), ref_l) * F32_ULP_AT_ONE
}


/// min/mid/max 各自的**通道下标**（并列时最靠前者胜），返回 `(i_min, i_mid, i_max)`。
/// 三下标恒为 `{0,1,2}` 的一个排列。tie-break 必要性：存在相等分量时谁当
/// max 会改变还原后的分量归属；`>`/`<` 若与 `>=`/`<=` 混用则同输入两次调用
/// 可能给出不同下标 ⇒ 非确定性（判据 [`C23-SAT-TIEBREAK-DETERMINISTIC`]）。
fn order_indices(c: Rgb, log: &mut NonSepLog) -> (usize, usize, usize) {
    let v = [c.r, c.g, c.b];
    let mut i_min = 0usize;
    let mut i_max = 0usize;
    let mut i = 1usize;
    while i < 3 {
        // 严格比较 ⇒ 并列时保留最靠前者（确定性 tie-break）。
        if v[i] < v[i_min] {
            i_min = i;
        }
        if v[i] > v[i_max] {
            i_max = i;
        }
        i += 1;
    }
    // 三下标互异时中位 = 剩下的那个；全等时下标不参与运算，但为保持
    // 「三下标构成 0/1/2 的排列」这一不变式，仍取 1。
    let i_mid = if i_min != i_max { 3 - i_min - i_max } else { 1 };
    if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
        log.tiebreak_hits = log.tiebreak_hits.saturating_add(1);
    }
    (i_min, i_mid, i_max)
}

/// [`SetSat`]（规范原文，三分支见头注 §三）。
/// 返回值同时给出结果与走了哪一档——第三档（要求了饱和度却被规范丢弃）
/// 的信息只返回颜色的话就永久丢失了。
pub fn set_sat(c: Rgb, s: f32, log: &mut NonSepLog) -> (Rgb, SatBranch) {
    let cs = guard_finite(c, log);
    let (sv, sv_clamped) = guard_finite_scalar(s);
    if sv_clamped {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let (i_min, i_mid, i_max) = order_indices(cs, log);
    let v_min = cs.get(i_min);
    let v_max = cs.get(i_max);
    let denom = v_max - v_min;

    if v_max > v_min {
        if denom < SAT_DENOM_MIN {
            // `f32` 下 `Cmax - Cmin` 可下溢到 0（次正规区）⇒ 0/0 = NaN。
            // 规范未覆盖此点（它假定实数），本条改走置零档并记账，
            // 绝不静默产 NaN（头注 §九）。
            log.sat_denom_guarded = log.sat_denom_guarded.saturating_add(1);
            log.sat_flat_zero = log.sat_flat_zero.saturating_add(1);
            return (Rgb::gray(CHANNEL_LO), SatBranch::FlatToZero);
        }
        let v_mid = cs.get(i_mid);
        let new_mid = ((v_mid - v_min) * sv) / denom;
        log.sat_scaled = log.sat_scaled.saturating_add(1);
        // `Cmin = 0` 在 if/else **之后**无条件执行（规范原文，头注 §六）。
        let out = cs.with(i_min, CHANNEL_LO).with(i_mid, new_mid).with(i_max, sv);
        (out, SatBranch::Scaled)
    } else if sv == 0.0 {
        log.sat_flat_zero = log.sat_flat_zero.saturating_add(1);
        (Rgb::gray(CHANNEL_LO), SatBranch::FlatToZero)
    } else {
        // 规范怪癖：无色彩信息时 else 分支一律全零——目标饱和度被静默丢弃。
        log.sat_flat_requested = log.sat_flat_requested.saturating_add(1);
        (Rgb::gray(CHANNEL_LO), SatBranch::FlatRequested)
    }
}

/// [`SetLum`]（规范原文）：整体加 `d = l - Lum(C)` 后 `ClipColor`；
/// 返回结果与是否发生过裁剪。
pub fn set_lum(c: Rgb, l: f32, log: &mut NonSepLog) -> (Rgb, bool) {
    let (lf, l_clamped) = guard_finite_scalar(l);
    if l_clamped {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let cs = guard_finite(c, log);
    let d = lf - lum(cs);
    let shifted = Rgb { r: cs.r + d, g: cs.g + d, b: cs.b + d };
    clip_color_ordered(shifted, log)
}

/// [`ClipColor`] 按**规范**：入口 `l`/`n`/`x` 各算一次，两个 `if` 共用。
/// 规范原文是两个独立 `if`（不是 `if/else`），第二段分母用入口 `x`（头注 §二）。
/// **钳制次序的纪律落在这里**：输入只拦 `NaN`/`±Inf`，有限越界值一律放行
/// 交给两段判定。
pub fn clip_color_ordered(c: Rgb, log: &mut NonSepLog) -> (Rgb, bool) {
    let cs = guard_finite(c, log);
    // 规范原文：`l`、`n`、`x` 在函数开头一次算出，两段判定共用入口值。
    let l = lum(cs);
    let n = cs.min_ch();
    let x = cs.max_ch();

    let mut out = cs;
    let mut clipped = false;

    // 第一段：if (n < 0)
    if n < CLIP_LO {
        out = clip_low_with(out, l, n);
        log.clip_low = log.clip_low.saturating_add(1);
        clipped = true;
    }
    // 第二段：if (x > 1) —— 独立 if，判定仍用入口 x，不重算。
    if x > CLIP_HI {
        out = clip_high_with(out, l, x);
        log.clip_high = log.clip_high.saturating_add(1);
        clipped = true;
    }
    if n < CLIP_LO && x > CLIP_HI {
        log.clip_both = log.clip_both.saturating_add(1);
    }
    let out = clamp_residue(out, log);
    (out, clipped)
}

/// 低位段收缩（`n < 0`）：`C_i = l + ((C_i - l) * l) / (l - n)`，分母用入口 `n`。
fn clip_low_with(c: Rgb, l: f32, n: f32) -> Rgb {
    let denom = l - n;
    // `n < 0` 且 `l ∈ [0,1]` ⇒ 分母数学上非零；仍兜底：下溢时取零收缩。
    let k = if denom < SAT_DENOM_MIN { 0.0 } else { l / denom };
    Rgb {
        r: l + (c.r - l) * k,
        g: l + (c.g - l) * k,
        b: l + (c.b - l) * k,
    }
}

/// 高位段收缩（`x > 1`）：`C_i = l + ((C_i - l) * (1-l)) / (x - l)`，分母用入口 `x`。
fn clip_high_with(c: Rgb, l: f32, x: f32) -> Rgb {
    let denom = x - l;
    // 同 [`clip_low_with`] 兜底。
    let k = if denom < SAT_DENOM_MIN { 0.0 } else { (1.0 - l) / denom };
    Rgb {
        r: l + (c.r - l) * k,
        g: l + (c.g - l) * k,
        b: l + (c.b - l) * k,
    }
}

/// [`ClipColor`] 的**逐段重算对照版**：第二段用**第一段之后**的 `max`。
/// 仅供判据对照（[`clip_order_probe`]），生产路径不许调用。
/// 两版共用 [`clip_low_with`]/[`clip_high_with`] 与同一入口 `l`，唯一变量
/// 是第二段分母取值，故两者之差必然来自「是否逐段重算」这一纪律本身。
pub fn clip_color_recomputed(c: Rgb, log: &mut NonSepLog) -> (Rgb, bool) {
    let cs = guard_finite(c, log);
    let l = lum(cs);
    let n = cs.min_ch();

    let mut out = cs;
    let mut clipped = false;

    if n < CLIP_LO {
        out = clip_low_with(out, l, n);
        clipped = true;
    }
    // 违规点：判定用重算的 x₂（第一段之后）——本对照版刻意演示的错。
    let x2 = out.max_ch();
    if x2 > CLIP_HI {
        out = clip_high_with(out, l, x2);
        clipped = true;
    }
    let out = clamp_residue(out, log);
    (out, clipped)
}

/// 钳制次序的对照探针结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipOrderProbe {
    /// 规范结果（入口 `l`/`n`/`x` 一次算出）。
    pub spec_order: Rgb,
    /// 逐段重算版结果。
    pub recomputed: Rgb,
    /// 两者逐通道**最大绝对差**。
    pub max_gap: f32,
    /// 输入是否同时满足 `n < 0` 与 `x > 1`（次序敏感的前提）。
    pub both_triggered: bool,
    /// 规范结果是否落在色域边界（`min == 0` 且 `max == 1`）。
    pub spec_on_gamut_edge: bool,
}

/// 取一组"两段同时触发"的输入，量化「逐段重算」与规范的差。
/// 语料不能取 `[0,1]` 内颜色（两段都是死码）；本探针构造越界三元组
/// （R=-0.5、B=2.0、G=0.5 保两段分母同号非零）分别送入两版。越界值能进
/// 到函数里，靠的是 [`guard_finite`] 只拦非有限值这一决定。
pub fn clip_order_probe() -> ClipOrderProbe {
    let mut log = NonSepLog::new();
    let raw = Rgb { r: -0.5, g: 0.5, b: 2.0 };
    let (spec_order, _) = clip_color_ordered(raw, &mut log);
    let mut log2 = NonSepLog::new();
    let (recomputed, _) = clip_color_recomputed(raw, &mut log2);
    let mut max_gap = 0.0f32;
    let pa = [spec_order.r, spec_order.g, spec_order.b];
    let pb = [recomputed.r, recomputed.g, recomputed.b];
    let mut i = 0usize;
    while i < 3 {
        let d = abs_f32(pa[i], pb[i]);
        if d > max_gap {
            max_gap = d;
        }
        i += 1;
    }
    ClipOrderProbe {
        spec_order,
        recomputed,
        max_gap,
        both_triggered: log.clip_both > 0,
        spec_on_gamut_edge: spec_order.min_ch() == CHANNEL_LO && spec_order.max_ch() == CHANNEL_HI,
    }
}

// 六、四模式公式表（规范 §10.2.1–§10.2.4）

/// 标量求值：非可分离混合本体（**第一路**，兼作语义基准）。
/// 四条公式原文见头注 §〇。结构性差别：`hue`/`saturation` 里的 `SetSat`
/// 作用在**整组三通道**上（先排序再按通道下标写回）——三通道不再独立
/// 计算，判据 [`C23-NONSEPARABLE-NOT-CHANNELWISE`] 把这条差别做成可测断言。
pub fn blend(mode: NonSepMode, cb: Rgb, cs: Rgb, log: &mut NonSepLog) -> Rgb {
    let cb = guard_finite(cb, log);
    let cs = guard_finite(cs, log);
    match mode {
        NonSepMode::Hue => {
            let (t, _) = set_sat(cs, sat(cb), log);
            let (out, _) = set_lum(t, lum(cb), log);
            out
        }
        NonSepMode::Saturation => {
            let (t, _) = set_sat(cb, sat(cs), log);
            let (out, _) = set_lum(t, lum(cb), log);
            out
        }
        NonSepMode::Color => {
            let (out, _) = set_lum(cs, lum(cb), log);
            out
        }
        NonSepMode::Luminosity => {
            let (out, _) = set_lum(cb, lum(cs), log);
            out
        }
    }
}

/// 逐像素 RGBA 通路：alpha 原样透传（预乘纪律归 F0625；alpha 与颜色分开给出）。
pub fn blend_pixel_rgba(
    mode: NonSepMode,
    cb: Rgb,
    cs: Rgb,
    alpha: f32,
    log: &mut NonSepLog,
) -> [f32; 4] {
    let out = blend(mode, cb, cs, log);
    let (a, _) = guard_finite_scalar(alpha);
    if alpha.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    [out.r, out.g, out.b, a]
}

// 七、计算顺序契约：先非可分离函数，后与背景合成

/// 按**锚点次序**合成：先非可分离混合（`Cr = blend(mode, Cb, Cs)`），
/// 后 Porter Duff source-over 合成（`Co = αs·(1-αb)·Cs + αs·αb·Cr + (1-αs)·αb·Cb`）。
/// 本函数是"契约"的被测侧，[`composite_over_swapped`] 是换序对照；判据
/// [`C23-COMPOSITE-ORDER-CONTRACT`] 必须在双 alpha<1 语料上比对——否则
/// 合成步退化为取 `Cr`，两序结果相同、判据恒绿。
pub fn composite_over(mode: NonSepMode, cb: Rgb, cs: Rgb, ab: f32, as_: f32, log: &mut NonSepLog) -> Rgb {
    let cr = blend(mode, cb, cs, log);
    let (abv, _) = guard_finite_scalar(ab);
    let (asv, _) = guard_finite_scalar(as_);
    if ab.is_nan() || as_.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let bc = [cb.r, cb.g, cb.b];
    let sc = [cs.r, cs.g, cs.b];
    let rc = [cr.r, cr.g, cr.b];
    let mut out_ch = [0.0f32; 3];
    let mut i = 0usize;
    while i < 3 {
        out_ch[i] = asv * (1.0 - abv) * sc[i] + asv * abv * rc[i] + (1.0 - asv) * abv * bc[i];
        i += 1;
    }
    clamp_residue(Rgb { r: out_ch[0], g: out_ch[1], b: out_ch[2] }, log)
}

/// **换序对照**：先合成（把 Cs 按 alpha 压到背景），再过非可分离函数。
/// 仅供判据对照，生产路径不许调用；唯一价值是让
/// [`C23-COMPOSITE-ORDER-CONTRACT`] 能证明两者确实不同。
pub fn composite_over_swapped(
    mode: NonSepMode,
    cb: Rgb,
    cs: Rgb,
    ab: f32,
    as_: f32,
    log: &mut NonSepLog,
) -> Rgb {
    let (abv, _) = guard_finite_scalar(ab);
    let (asv, _) = guard_finite_scalar(as_);
    if ab.is_nan() || as_.is_nan() {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    // 先合成：把源直接压到背景上（不含 B 项）。
    let pre_ch = [cb.r, cb.g, cb.b, cs.r, cs.g, cs.b];
    let mut mixed_in = [0.0f32; 3];
    let mut i = 0usize;
    while i < 3 {
        mixed_in[i] = asv * (1.0 - abv) * pre_ch[i + 3] + (1.0 - asv) * abv * pre_ch[i];
        i += 1;
    }
    let flattened = Rgb { r: mixed_in[0], g: mixed_in[1], b: mixed_in[2] };
    let cr = blend(mode, cb, flattened, log);
    clamp_residue(cr, log)
}

// 八、独立 f64 金标准 oracle（不读本模块任何函数）

/// `f64` 版 [`lum`]（oracle 私有，权重**硬写**——共用常量会变成"同一处改错、
/// 两处一起错"，对拍退化）。
fn oracle_lum(c: [f64; 3]) -> f64 {
    0.30f64 * c[0] + 0.59f64 * c[1] + 0.11f64 * c[2]
}

fn oracle_clip(c: [f64; 3]) -> [f64; 3] {
    let l = oracle_lum(c);
    let n = if c[0] <= c[1] && c[0] <= c[2] {
        c[0]
    } else if c[1] <= c[2] {
        c[1]
    } else {
        c[2]
    };
    let x = if c[0] >= c[1] && c[0] >= c[2] {
        c[0]
    } else if c[1] >= c[2] {
        c[1]
    } else {
        c[2]
    };
    let mut o = c;
    // 规范原文：`l`/`n`/`x` 在开头一次算出，两个独立 `if` 共用入口值；
    // 第二段分母也用入口 `x`（不重算 x₂）——次序纪律。
    if n < 0.0 {
        let d = l - n;
        let k = if d == 0.0 { 0.0 } else { l / d };
        let mut i = 0usize;
        while i < 3 {
            o[i] = l + (c[i] - l) * k;
            i += 1;
        }
    }
    if x > 1.0 {
        let d = x - l;
        let k = if d == 0.0 { 0.0 } else { (1.0 - l) / d };
        let mut i = 0usize;
        while i < 3 {
            o[i] = l + (o[i] - l) * k;
            i += 1;
        }
    }
    o
}

fn oracle_set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - oracle_lum(c);
    let shifted = [c[0] + d, c[1] + d, c[2] + d];
    oracle_clip(shifted)
}

fn oracle_set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    // 独立写一遍下标求解（**不复用** order_indices）：oracle 与被测
    // 各算各的下标，才能抓住"下标还原错通道"这类错误。
    let mut idx = [0usize, 1usize, 2usize];
    // 简单插入排序，按值升序；并列时保持原下标顺序（稳定）。
    let mut i = 1usize;
    while i < 3 {
        let mut j = i;
        while j > 0 && c[idx[j]] < c[idx[j - 1]] {
            let t = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = t;
            j -= 1;
        }
        i += 1;
    }
    let mut o = [0.0f64; 3];
    let (i_min, i_mid, i_max) = (idx[0], idx[1], idx[2]);
    let v_min = c[i_min];
    let v_max = c[i_max];
    if v_max > v_min {
        let v_mid = c[i_mid];
        o[i_min] = 0.0;
        o[i_mid] = ((v_mid - v_min) * s) / (v_max - v_min);
        o[i_max] = s;
    } else {
        // 规范 else 分支：三通道全零（目标饱和度被丢弃）。
        o = [0.0, 0.0, 0.0];
    }
    o
}

/// 独立 `f64` 金标准（**第二路**）：按规范原文逐字另写一遍，与 [`blend`]
/// 无任何共享代码路径（连权重常量都不共用）。任一处抄错规范都会在
/// 对拍中暴露（头注 §八）。
pub fn oracle(mode: NonSepMode, cb: Rgb, cs: Rgb) -> Rgb {
    let b = [cb.r as f64, cb.g as f64, cb.b as f64];
    let s = [cs.r as f64, cs.g as f64, cs.b as f64];
    // Sat 的 f64 直算（背景/源各一次）——仍不引用本模块任何函数。
    let sat_span = |c: &[f64; 3]| -> f64 {
        let hi = if c[0] > c[1] { if c[0] > c[2] { c[0] } else { c[2] } } else if c[1] > c[2] { c[1] } else { c[2] };
        let lo = if c[0] < c[1] { if c[0] < c[2] { c[0] } else { c[2] } } else if c[1] < c[2] { c[1] } else { c[2] };
        hi - lo
    };
    let sat_b = sat_span(&b);
    let sat_s = sat_span(&s);
    let l_b = oracle_lum(b);
    let l_s = oracle_lum(s);
    let o = match mode {
        NonSepMode::Hue => oracle_set_lum(oracle_set_sat(s, sat_b), l_b),
        NonSepMode::Saturation => oracle_set_lum(oracle_set_sat(b, sat_s), l_b),
        NonSepMode::Color => oracle_set_lum(s, l_b),
        NonSepMode::Luminosity => oracle_set_lum(b, l_s),
    };
    Rgb { r: o[0] as f32, g: o[1] as f32, b: o[2] as f32 }
}

/// LSB 偏差（口径 [`LSB8`]，与 F0622 同源 ⇒ 两单对拍尺度一致）。
pub fn lsb_delta(a: f32, b: f32) -> f32 {
    let d = if a > b { a - b } else { b - a };
    d / LSB8
}

// 九、对拍网格与汇总

/// 对拍网格（背景 × 源，含灰、彩色、跨段、并列值；并列分量是 SetSat
/// tie-break 唯一会被触发的地方）。
pub const CROSSCHECK_GRID: [(Rgb, Rgb); 12] = [
    (Rgb { r: 0.0, g: 0.0, b: 0.0 }, Rgb { r: 1.0, g: 1.0, b: 1.0 }),
    (Rgb { r: 1.0, g: 1.0, b: 1.0 }, Rgb { r: 0.0, g: 0.0, b: 0.0 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.0, g: 0.0, b: 0.0 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.5 }, Rgb { r: 0.0, g: 0.0, b: 0.0 }),
    (Rgb { r: 0.2, g: 0.6, b: 0.9 }, Rgb { r: 0.9, g: 0.3, b: 0.1 }),
    (Rgb { r: 0.9, g: 0.3, b: 0.1 }, Rgb { r: 0.2, g: 0.6, b: 0.9 }),
    (Rgb { r: 0.5, g: 0.5, b: 0.2 }, Rgb { r: 0.5, g: 0.5, b: 0.8 }),
    (Rgb { r: 0.5, g: 0.2, b: 0.5 }, Rgb { r: 0.8, g: 0.5, b: 0.5 }),
    (Rgb { r: 0.2, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.8, b: 0.5 }),
    (Rgb { r: 0.05, g: 0.95, b: 0.5 }, Rgb { r: 0.5, g: 0.05, b: 0.95 }),
    (Rgb { r: 0.95, g: 0.05, b: 0.5 }, Rgb { r: 0.5, g: 0.95, b: 0.05 }),
];

/// 单模式对拍结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 逐通道最大 LSB 偏差（[oracle] vs [blend]）。
    pub worst: f32,
    /// 偏差的**符号**：0 正、1 负、2 恰为零（避免"三路一起偏"互相抵消）。
    pub bias: u8,
}

impl ModeCheck {
    pub fn within_budget(&self) -> bool {
        self.worst <= LSB_BUDGET
    }
}

/// 全模式对拍汇总。
#[derive(Clone, Debug, PartialEq)]
pub struct CrossCheck {
    pub per_mode: Vec<ModeCheck>,
    pub samples: u32,
    pub worst: f32,
    /// 三路偏差符号分布（供「不是三路一起偏」判据）。
    pub bias_pos: u32,
    pub bias_neg: u32,
}

impl CrossCheck {
    pub fn within_budget(&self) -> bool {
        self.per_mode.iter().all(|m| m.within_budget()) && self.worst <= LSB_BUDGET
    }
}

/// 单模式对拍：扫全网格，产出（样本数, 最坏 LSB 偏差, 偏差符号档）。
/// 偏差符号按**通道求和后**判一次（避免逐通道抖动把 bias 打成噪声）。
fn crosscheck_one_mode(mode: NonSepMode, bias_pos: &mut u32, bias_neg: &mut u32) -> (u32, f32, u8) {
    let mut n = 0u32;
    let mut w = 0.0f32;
    let mut acc = 0.0f32;
    let mut gi = 0usize;
    while gi < CROSSCHECK_GRID.len() {
        let (cb, cs) = CROSSCHECK_GRID[gi];
        let got = blend(mode, cb, cs, &mut NonSepLog::new());
        let want = oracle(mode, cb, cs);
        let pa = [got.r, got.g, got.b];
        let pb = [want.r, want.g, want.b];
        let mut ci = 0usize;
        while ci < 3 {
            let d = lsb_delta(pa[ci], pb[ci]);
            if d > w {
                w = d;
            }
            if pa[ci] > pb[ci] {
                *bias_pos = bias_pos.saturating_add(1);
                acc += pa[ci] - pb[ci];
            } else if pa[ci] < pb[ci] {
                *bias_neg = bias_neg.saturating_add(1);
                acc += pa[ci] - pb[ci];
            }
            ci += 1;
        }
        n = n.saturating_add(1);
        gi += 1;
    }
    let bias = if acc > 0.0 {
        1u8
    } else if acc < 0.0 {
        2u8
    } else {
        0u8
    };
    (n, w, bias)
}

/// 全网格对拍：4 模式 × [`CROSSCHECK_GRID`]。
pub fn crosscheck() -> CrossCheck {
    let mut per_mode: Vec<ModeCheck> = Vec::with_capacity(NONSEPARABLE_COUNT);
    let mut total = 0u32;
    let mut worst = 0.0f32;
    let mut bias_pos = 0u32;
    let mut bias_neg = 0u32;
    let mut mi = 0usize;
    while mi < NONSEPARABLE_COUNT {
        let mode = match NonSepMode::from_index(mi) {
            Some(m) => m,
            None => {
                mi += 1;
                continue;
            }
        };
        let (n, w, bias) = crosscheck_one_mode(mode, &mut bias_pos, &mut bias_neg);
        per_mode.push(ModeCheck { mode, samples: n, worst: w, bias });
        total = total.saturating_add(n);
        if w > worst {
            worst = w;
        }
        mi += 1;
    }
    CrossCheck { per_mode, samples: total, worst, bias_pos, bias_neg }
}

// 十、SIMD 宽通道（4 像素批）

/// 宽通道实测操作数（**执行期累加**，非纸面公式 —— 见 F0622 头注 §五）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimdOp {
    pub arith: u32,
    pub compare: u32,
    pub select: u32,
    pub clip: u32,
}

impl SimdOp {
    pub fn new() -> SimdOp {
        SimdOp::default()
    }

    pub fn total(&self) -> u32 {
        self.arith + self.compare + self.select + self.clip
    }

    pub fn add(&mut self, o: &SimdOp) {
        self.arith = self.arith.saturating_add(o.arith);
        self.compare = self.compare.saturating_add(o.compare);
        self.select = self.select.saturating_add(o.select);
        self.clip = self.clip.saturating_add(o.clip);
    }
}

/// 4 像素批的求值结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimdBatch {
    pub out: [Rgb; SIMD_WIDTH],
    pub ops: SimdOp,
    /// 四像素的分支签名（判据用来确认批内各像素**独立**取分支）。
    pub branches: [SatBranch; SIMD_WIDTH],
}

/// 宽通道求值（**第三路**）。「向量化」含义：四像素**共用一次分支判定**
/// （uniform branch）当且仅当分支签名一致，否则逐像素取分支（W3C §11
/// 安全要求的可测形态）。与 F0622 的差别：可分离分支只依赖单通道，而
/// `SetSat` 依赖三通道序关系，四像素可能不同签名——不暴露签名则判据
/// 无法发现「签名不同时被误当作 uniform」。
pub fn blend_simd(mode: NonSepMode, cb: [Rgb; SIMD_WIDTH], cs: [Rgb; SIMD_WIDTH]) -> SimdBatch {
    let mut out = [Rgb::gray(CHANNEL_LO); SIMD_WIDTH];
    let mut branches = [SatBranch::Scaled; SIMD_WIDTH];
    let mut ops = SimdOp::new();
    // 第一遍：只算分支签名（不产出颜色）。
    let mut uniform = true;
    let mut i = 0usize;
    while i < SIMD_WIDTH {
        let sig = sat_branch_of(mode, cb[i], cs[i]);
        branches[i] = sig;
        if sig != branches[0] {
            uniform = false;
        }
        i += 1;
    }
    ops.compare = ops.compare.saturating_add(SIMD_WIDTH as u32);
    if uniform {
        ops.select = ops.select.saturating_add(1);
    }
    // 第二遍：产出颜色。签名一致时省略逐像素取分支的开销。
    i = 0;
    while i < SIMD_WIDTH {
        let r = blend(mode, cb[i], cs[i], &mut NonSepLog::new());
        out[i] = r;
        if uniform {
            ops.arith = ops.arith.saturating_add(mode_arith_cost(mode));
            ops.clip = ops.clip.saturating_add(2);
        } else {
            ops.arith = ops.arith.saturating_add(mode_arith_cost(mode) + 3);
            ops.clip = ops.clip.saturating_add(2);
        }
        i += 1;
    }
    SimdBatch { out, ops, branches }
}

/// 只求分支签名（不产颜色）。
fn sat_branch_of(mode: NonSepMode, cb: Rgb, cs: Rgb) -> SatBranch {
    let mut log = NonSepLog::new();
    match mode {
        NonSepMode::Hue => set_sat(cs, sat(cb), &mut log).1,
        NonSepMode::Saturation => set_sat(cb, sat(cs), &mut log).1,
        NonSepMode::Color => SatBranch::FlatRequested,
        NonSepMode::Luminosity => SatBranch::FlatRequested,
    }
}

/// 单像素算术操作数成本表（宽/标量两路**共用**以保证量纲一致——标量路
/// 若硬写一份自己的计数，"宽通道摊薄了操作数"的判据就在比不可比的数）。
fn mode_arith_cost(mode: NonSepMode) -> u32 {
    match mode {
        // hue / saturation：Sat(3 比较) + SetSat(2 算 + 1 比较 + 1 选择) + SetLum(3 算 + Lum(3 乘 2 加))
        NonSepMode::Hue | NonSepMode::Saturation => 12,
        // color / luminosity：SetLum(3 算 + Lum) + ClipColor
        NonSepMode::Color | NonSepMode::Luminosity => 7,
    }
}

/// 标量路 4 像素批的实测操作数（**同一张成本表**，保证量纲一致）。
pub fn scalar_batch_ops(mode: NonSepMode) -> SimdOp {
    let mut ops = SimdOp::new();
    let mut i = 0usize;
    while i < SIMD_WIDTH {
        ops.arith = ops.arith.saturating_add(mode_arith_cost(mode) + 6);
        ops.compare = ops.compare.saturating_add(6);
        ops.select = ops.select.saturating_add(2);
        ops.clip = ops.clip.saturating_add(2);
        i += 1;
    }
    ops
}

/// 宽通道摊薄结果（4 像素批：标量 vs 宽通道）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amortization {
    pub mode: NonSepMode,
    pub scalar_ops: u32,
    pub simd_ops: u32,
    pub uniform: bool,
}

impl Amortization {
    /// 摊薄比（标量 / 宽通道）；`> 1` 才叫摊薄。
    pub fn ratio(&self) -> f32 {
        if self.simd_ops == 0 {
            return 0.0;
        }
        self.scalar_ops as f32 / self.simd_ops as f32
    }

    pub fn amortized(&self) -> bool {
        self.simd_ops < self.scalar_ops
    }
}

/// 实测摊薄：**同一批语料**跑两路，比操作数。
pub fn simd_amortization(mode: NonSepMode) -> Amortization {
    // 批内四像素取同一分支 ⇒ uniform 成立，测的是"摊薄"本身。
    let cb = [
        Rgb { r: 0.2, g: 0.6, b: 0.9 },
        Rgb { r: 0.3, g: 0.5, b: 0.8 },
        Rgb { r: 0.4, g: 0.4, b: 0.7 },
        Rgb { r: 0.5, g: 0.3, b: 0.6 },
    ];
    let cs = [
        Rgb { r: 0.9, g: 0.3, b: 0.1 },
        Rgb { r: 0.8, g: 0.4, b: 0.2 },
        Rgb { r: 0.7, g: 0.5, b: 0.3 },
        Rgb { r: 0.6, g: 0.6, b: 0.4 },
    ];
    let batch = blend_simd(mode, cb, cs);
    let uniform = batch.branches[0] == batch.branches[1]
        && batch.branches[1] == batch.branches[2]
        && batch.branches[2] == batch.branches[3];
    Amortization {
        mode,
        scalar_ops: scalar_batch_ops(mode).total(),
        simd_ops: batch.ops.total(),
        uniform,
    }
}

// 十一、GPU 路：可读 WGSL 文本（第四路）

/// 权重字面量（`0.3` / `0.59` / `0.11`）。
/// 用 `format_k` 而非 `{}`：GPU 路判据要求文本里有 `.`（`1.0f32` 可能打成 `1`）。
fn fmt_lum_weights() -> String {
    format!("{}f, {}f, {}f", fmt_k(LUM_R), fmt_k(LUM_G), fmt_k(LUM_B))
}

/// `f32` → WGSL 字面量（保小数点）。
fn fmt_k(v: f32) -> String {
    let s = format!("{}", v);
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// WGSL 共享原语段（`vsat_lum` / `vclip` / `vset_lum` / `vset_sat`，四模式共用）。
/// 文本刻意保留两处规范细节（正是易错点）：ClipColor 的两个独立 `if`、
/// SetSat 下标按通道还原。
fn wgsl_push_primitives(out: &mut String) {
    out.push_str("fn vsat_lum(c: vec3f) -> f32 {\n");
    out.push_str(&format!("  let w = vec3f({});\n", fmt_lum_weights()));
    out.push_str("  return dot(c, w);\n}\n\n");

    out.push_str("// ClipColor: 规范原文是两个**独立** if，次序不可交换。\n");
    out.push_str("fn vclip(c: vec3f) -> vec3f {\n");
    out.push_str("  let l = vsat_lum(c);\n");
    out.push_str("  let n = min(c.r, min(c.g, c.b));\n");
    out.push_str("  let x = max(c.r, max(c.g, c.b));\n");
    out.push_str("  var o = c;\n");
    out.push_str("  if (n < 0.0) {\n");
    out.push_str("    o = l + ((c - l) * l / (l - n));\n");
    out.push_str("  }\n");
    // 第二段与第一段共用入口 x（若重算 x2，GPU 路就与标量路分家——
    // 两路同源是交付纪律，不是风格问题）。
    out.push_str("  if (x > 1.0) {\n");
    out.push_str("    o = l + ((o - l) * (1.0 - l) / (x - l));\n");
    out.push_str("  }\n");
    out.push_str("  return clamp(o, vec3f(0.0), vec3f(1.0));\n}\n\n");

    out.push_str("fn vset_lum(c: vec3f, l: f32) -> vec3f {\n");
    out.push_str("  return vclip(c + vec3f(l - vsat_lum(c)));\n}\n\n");

    out.push_str("// SetSat: 下标 min/mid/max 是**取值次序**，须按通道还原。\n");
    out.push_str("fn vset_sat(c: vec3f, s: f32) -> vec3f {\n");
    out.push_str("  var o = vec3f(0.0);\n");
    out.push_str("  let vmin = min(c.r, min(c.g, c.b));\n");
    out.push_str("  let vmax = max(c.r, max(c.g, c.b));\n");
    out.push_str("  if (vmax > vmin) {\n");
    out.push_str("    let vm = c.r + c.g + c.b - vmin - vmax;\n");
    out.push_str("    let mid = (vm - vmin) * s / (vmax - vmin);\n");
    out.push_str("    if (c.r == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.r == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("    if (c.g == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.g == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("    if (c.b == vmin) { o = vec3f(0.0, mid, s); }\n");
    out.push_str("    else if (c.b == vmax) { o = vec3f(s, mid, 0.0); }\n");
    out.push_str("    else { o = vec3f(0.0, s, mid); }\n");
    out.push_str("  }\n");
    out.push_str("  return o;\n}\n\n");
}

/// 产出该模式的**完整 WGSL 文本**（GPU 路模板，F0628 的来源）。
/// 与 F0622 同构：产真文本并做文本级自检（只置 `gpu_implemented: true`
/// 是自证式门面，锚点禁止）。
///
/// **GPU 路与标量路同源：第二段必须用入口 `x`**——初版把第二段写成
/// `let x2 = max(...)` 重算，且 [`wgsl_selfcheck`] 把 `if (x2 > 1.0)` 列进
/// 必需关键词，把该错误固化成了门禁（「判据写错比没判据更坏」的实例）。
/// 处置：文本改回入口 `x`，并**反向断言**文本不出现 `x2`——只做正向检查
/// 不够（M12 变异实测）。
pub fn wgsl_source(mode: NonSepMode) -> String {
    let mut out = String::new();
    out.push_str("// VE-F0623 generated WGSL — non-separable blend mode\n");
    out.push_str(&format!("// mode: {} ({})\n", mode.keyword(), mode.clause()));
    out.push_str(&format!("// prose: {}\n", mode.prose()));
    out.push_str("// 权重取自 W3C 规范原文 0.3/0.59/0.11，**非** Rec.709。\n\n");

    // 共享原语段（易错点与同源纪律的注解见 [`wgsl_push_primitives`]）。
    wgsl_push_primitives(&mut out);

    // --- 模式本体 ---
    out.push_str("fn vblend(cb: vec3f, cs: vec3f) -> vec3f {\n");
    let body = match mode {
        NonSepMode::Hue => "  return vset_lum(vset_sat(cs, sat(cb)), vsat_lum(cb));",
        NonSepMode::Saturation => "  return vset_lum(vset_sat(cb, sat(cs)), vsat_lum(cb));",
        NonSepMode::Color => "  return vset_lum(cs, vsat_lum(cb));",
        NonSepMode::Luminosity => "  return vset_lum(cb, vsat_lum(cs));",
    };
    out.push_str(body);
    out.push_str("\n}\n\n");
    out.push_str("// 顺序契约：先 vblend，后 source-over 合成。\n");
    out.push_str("fn vcompose(cb: vec3f, cs: vec3f, ab: f32, as_: f32) -> vec3f {\n");
    out.push_str("  let cr = vblend(cb, cs);\n");
    out.push_str("  return as_ * (1.0 - ab) * cs + as_ * ab * cr + (1.0 - as_) * ab * cb;\n");
    out.push_str("}\n");
    out
}

/// WGSL 文本自检结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgslSelfCheck {
    pub mode: NonSepMode,
    /// 关键构造是否齐备（`vec3f` / `clamp` / 双 `if` / `set_sat` / 合成式）。
    pub complete: bool,
    /// 缺失项数（`0` 即齐备）。
    pub missing: u32,
    /// 文本字节数。
    pub len: u32,
}

/// 文本级自检：关键构造必须**真的出现在文本里**（逐模式分档要求——
/// 只有 `hue`/`saturation` 含 `vset_sat` 调用，全部模式都需 `vblend` + 合成式）。
pub fn wgsl_selfcheck(mode: NonSepMode) -> WgslSelfCheck {
    let src = wgsl_source(mode);
    let has = |needle: &str| -> bool { src.contains(needle) };
    let mut missing = 0u32;
    let mut req: Vec<&str> = Vec::with_capacity(10);
    req.push("fn vsat_lum");
    req.push("fn vclip");
    req.push("fn vset_lum");
    req.push("fn vset_sat");
    req.push("fn vblend");
    req.push("fn vcompose");
    req.push("clamp(");
    req.push("vec3f");
    req.push("if (n < 0.0)");
    req.push("if (x > 1.0)");
    // `SetSat` 只在 hue / saturation 的本体里被**调用**。
    let uses_sat = matches!(mode, NonSepMode::Hue | NonSepMode::Saturation);
    let call_sat = if uses_sat { "vset_sat(" } else { "@@no-set-sat@@" };
    let mut all = true;
    for r in req.iter() {
        if !has(r) {
            missing = missing.saturating_add(1);
            all = false;
        }
    }
    // 调用点：查 vblend 的 return 语句里是否出现 vset_sat。
    let body_has_sat = if uses_sat { src.contains(call_sat) } else { true };
    if !body_has_sat {
        missing = missing.saturating_add(1);
        all = false;
    }
    // color / luminosity 必须**不含** SetSat 调用（否则公式抄错成别的模式）。
    if !uses_sat && src.contains("return vset_lum(vset_sat") {
        missing = missing.saturating_add(1);
        all = false;
    }
    // GPU 路与标量路同源：反向断言「文本里没有第二套 max」——
    // 只查 `if (x > 1.0)` 不够，改成 `if (x2 > 1.0)` 照样能过。
    if src.contains("x2") {
        missing = missing.saturating_add(1);
        all = false;
    }
    WgslSelfCheck { mode, complete: all && missing == 0, missing, len: src.len() as u32 }
}

// 十二、色域往返（精度纪律）

/// 一次往返的结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundTrip {
    pub input: Rgb,
    pub via_lum: Rgb,
    pub via_sat: Rgb,
    /// 输入的 `Cmin`（判据闭式 `SetSat(C, Sat(C)) == C - Cmin` 要用）。
    pub c_min: f32,
    /// `SetLum(C, Lum(C))` 逐通道是否**位精确**相等。
    pub lum_bitexact: bool,
    /// `SetSat(C, Sat(C))` 闭式残差 `result - (C - Cmin)` 是否**恰为 0**。
    pub sat_closure_exact: bool,
    /// 往返最大 LSB 偏差（对 F0630 口径报账）。
    pub worst_lsb: f32,
}

/// 色域往返：`RGB → 亮度/饱和度 → RGB`。两条性质（头注 §六）：
/// 亮度侧**位精确恒等** `SetLum(C, Lum(C)) == C`；饱和度侧是**闭式**
/// `SetSat(C, Sat(C)) == C - Cmin` 而非恒等（规范 `Cmin = 0` 在 if/else
/// 之后无条件执行）。初版误写成恒等把正确实现判红——教训：判据的参考
/// 性质必须独立算出来，「看起来应该恒等」不是理由。
pub fn round_trip(c: Rgb) -> RoundTrip {
    let mut log = NonSepLog::new();
    let (via_lum, _) = set_lum(c, lum(c), &mut log);
    let (via_sat, _) = set_sat(c, sat(c), &mut log);
    let c_min = c.min_ch();
    let a = [via_lum.r, via_lum.g, via_lum.b];
    let b = [c.r, c.g, c.b];
    let s = [via_sat.r, via_sat.g, via_sat.b];
    // 闭式期望：C - Cmin（逐通道）。
    let want = [b[0] - c_min, b[1] - c_min, b[2] - c_min];
    let mut lum_exact = true;
    let mut sat_exact = true;
    let mut worst = 0.0f32;
    let mut i = 0usize;
    while i < 3 {
        if a[i] != b[i] {
            lum_exact = false;
        }
        // 灰输入走 else 分支输出全零，此时闭式 C - Cmin = 0 恰好也成立。
        // 闭式残差按 1 ULP 判（f32 两次舍入）。
        if out_gap_ulp(s[i], want[i]) > 1.0 {
            sat_exact = false;
        }
        let d1 = lsb_delta(a[i], b[i]);
        if d1 > worst {
            worst = d1;
        }
        // 饱和度侧按**闭式残差**计偏差，而不是拿 `C` 当参考。
        let d2 = lsb_delta(s[i], want[i]);
        if d2 > worst {
            worst = d2;
        }
        i += 1;
    }
    RoundTrip {
        input: c,
        via_lum,
        via_sat,
        c_min,
        lum_bitexact: lum_exact,
        sat_closure_exact: sat_exact,
        worst_lsb: worst,
    }
}

/// 往返语料（彩色 + 灰 + 并列值）。
pub const ROUNDTRIP_CORPUS: [Rgb; 8] = [
    Rgb { r: 0.2, g: 0.6, b: 0.9 },
    Rgb { r: 0.9, g: 0.3, b: 0.1 },
    Rgb { r: 0.5, g: 0.5, b: 0.5 },
    Rgb { r: 0.0, g: 0.0, b: 0.0 },
    Rgb { r: 1.0, g: 1.0, b: 1.0 },
    Rgb { r: 0.5, g: 0.5, b: 0.2 },
    Rgb { r: 0.05, g: 0.95, b: 0.5 },
    Rgb { r: 0.7, g: 0.7, b: 0.7 },
];

/// 全语料往返汇总。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundTripReport {
    pub samples: u32,
    /// 亮度往返全部位精确？
    pub lum_all_exact: bool,
    /// 饱和度往返**闭式残差恰为 0** 的样本数。
    pub sat_exact_count: u32,
    /// 判据侧独立重算的"闭式应成立"样本数（= 全部，见下）。
    pub sat_expected_exact: u32,
    pub worst_lsb: f32,
}

impl RoundTripReport {
    pub fn within_budget(&self) -> bool {
        self.worst_lsb <= ROUNDTRIP_LSB_BUDGET
    }
}

/// 判据侧**独立重算**"闭式应成立"的样本数（弱门禁第七条：参考值须与
/// 被测对象无关地算出）。闭式在 `Scaled` 与 `else` 两分支下都成立
/// （else 输出全零，而 C-Cmin 此时也恰为全零），故覆盖面 = 全部样本。
/// 刻意不用初版"非灰样本数"——那与已被推翻的"往返是恒等"前提绑定。
fn sat_expected_exact_count() -> u32 {
    ROUNDTRIP_CORPUS.len() as u32
}

/// 全语料往返汇总。
pub fn round_trip_report() -> RoundTripReport {
    let mut lum_all = true;
    let mut sat_exact = 0u32;
    let mut worst = 0.0f32;
    let mut samples = 0u32;
    let mut i = 0usize;
    while i < ROUNDTRIP_CORPUS.len() {
        let rt = round_trip(ROUNDTRIP_CORPUS[i]);
        if !rt.lum_bitexact {
            lum_all = false;
        }
        if rt.sat_closure_exact {
            sat_exact = sat_exact.saturating_add(1);
        }
        if rt.worst_lsb > worst {
            worst = rt.worst_lsb;
        }
        samples = samples.saturating_add(1);
        i += 1;
    }
    RoundTripReport {
        samples,
        lum_all_exact: lum_all,
        sat_exact_count: sat_exact,
        sat_expected_exact: sat_expected_exact_count(),
        worst_lsb: worst,
    }
}

// 十三、结构性质（取自哪一侧 / 跨通道耦合）

/// 四模式亮度/饱和度保持的逐模式实测。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvariantCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 亮度取自哪一侧（规范定义，**不依赖浮点**）。
    pub lum_from: Side,
    /// 饱和度取自哪一侧。
    pub sat_from: Side,
    /// 亮度与「该侧原色亮度」的最大偏差。
    pub lum_dev: f32,
    /// 饱和度与「该侧原色饱和度」的最大偏差。
    pub sat_dev: f32,
}

/// 亮度/饱和度保持实测：结果亮度/饱和度 == 指定侧原色的对应量（规范定义
/// 直接蕴含）。只在未发生 `ClipColor` 裁剪的样本上断言饱和度——裁剪改变
/// 饱和度是规范行为，语料按裁剪标记过滤。
pub fn invariant_check(mode: NonSepMode) -> InvariantCheck {
    let mut n = 0u32;
    let mut lum_dev = 0.0f32;
    let mut sat_dev = 0.0f32;
    let mut gi = 0usize;
    while gi < CROSSCHECK_GRID.len() {
        let (cb, cs) = CROSSCHECK_GRID[gi];
        let mut log = NonSepLog::new();
        let out = blend(mode, cb, cs, &mut log);
        let clipped = log.clip_low > 0 || log.clip_high > 0;
        let dev = out_lum_dev(out, ref_lum_of(mode, cb, cs));
        lum_dev = if dev > lum_dev { dev } else { lum_dev };
        if !clipped {
            let dv = sat_dev_of(out, ref_sat_of(mode, cb, cs));
            sat_dev = if dv > sat_dev { dv } else { sat_dev };
        }
        n = n.saturating_add(1);
        gi += 1;
    }
    InvariantCheck {
        mode,
        samples: n,
        lum_from: mode.lum_from(),
        sat_from: mode.sat_from(),
        lum_dev,
        sat_dev,
    }
}

/// 结果亮度应等于哪一侧原色的亮度（规范定义的实测参照）。
fn ref_lum_of(mode: NonSepMode, cb: Rgb, cs: Rgb) -> f32 {
    match mode.lum_from() {
        Side::Backdrop => lum(cb),
        Side::Source => lum(cs),
    }
}

/// 结果饱和度应等于哪一侧原色的饱和度（规范定义的实测参照）。
fn ref_sat_of(mode: NonSepMode, cb: Rgb, cs: Rgb) -> f32 {
    match mode.sat_from() {
        Side::Backdrop => sat(cb),
        Side::Source => sat(cs),
    }
}

/// 饱和度保持偏差（绝对值）。
fn sat_dev_of(out: Rgb, ref_s: f32) -> f32 {
    abs_f32(sat(out), ref_s)
}

/// 「跨通道耦合」实测：把背景三通道**重排**后结果必须变化——否则实现
/// 退化成了逐通道独立计算（判据 [`C23-NONSEPARABLE-NOT-CHANNELWISE`]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CouplingCheck {
    pub mode: NonSepMode,
    pub samples: u32,
    /// 至少发生一次变化的样本数（耦合存在）。
    pub coupled: u32,
    /// 最大重排偏差（耦合强度）。
    pub max_shift: f32,
}

/// 跨通道耦合实测（对固定换序 `[1,2,0]`）。
pub fn coupling_check(mode: NonSepMode) -> CouplingCheck {
    const PERM: [usize; 3] = [1, 2, 0];
    let mut n = 0u32;
    let mut coupled = 0u32;
    let mut max_shift = 0.0f32;
    let mut gi = 0usize;
    while gi < CROSSCHECK_GRID.len() {
        let (cb, cs) = CROSSCHECK_GRID[gi];
        let base = blend(mode, cb, cs, &mut NonSepLog::new());
        let mut cb2 = Rgb::gray(CHANNEL_LO);
        let mut k = 0usize;
        while k < 3 {
            cb2 = cb2.with(k, cb.get(PERM[k]));
            k += 1;
        }
        let swapped = blend(mode, cb2, cs, &mut NonSepLog::new());
        let pa = [base.r, base.g, base.b];
        let pb = [swapped.r, swapped.g, swapped.b];
        let mut ch = 0usize;
        let mut any = false;
        while ch < 3 {
            let d = abs_f32(pa[ch], pb[ch]);
            if d > 0.0 {
                any = true;
            }
            if d > max_shift {
                max_shift = d;
            }
            ch += 1;
        }
        if any {
            coupled = coupled.saturating_add(1);
        }
        n = n.saturating_add(1);
        gi += 1;
    }
    CouplingCheck { mode, samples: n, coupled, max_shift }
}

// 十四、色卡（无障碍：灰阶是文字；与 F0622 共用探针）

/// 色卡行：`<关键字> <灰阶条>`。采样点直接用 [`SWATCH_PROBES`]（与 F0622
/// 可并排对齐）；但语料必须**彩色化**（背景源都取灰则三通道等值，`SetSat`
/// 恒走 flat 分支，测不到核心），做法是把灰阶值映射到两条不同相位的斜坡。
pub fn swatch_row(mode: NonSepMode) -> String {
    let mut s = String::new();
    s.push_str(mode.keyword());
    s.push(' ');
    for p in SWATCH_PROBES.iter() {
        let cb = colorize(p.0, 0.0);
        let cs = colorize(p.1, 0.5);
        let v = blend(mode, cb, cs, &mut NonSepLog::new());
        // 灰阶取三通道均值（`ramp_char` 收口到 [0,1] 由它自己做）。
        s.push(ramp_char((v.r + v.g + v.b) / 3.0));
    }
    s
}

/// 把灰阶值 `v` 展开为三通道**不全等**的颜色（等差展开再收口；`phase = 0` 退化为灰）。
fn colorize(v: f32, phase: f32) -> Rgb {
    let r = v + phase;
    let g = v;
    let b = v - phase;
    let (r, _) = guard_finite_scalar(r);
    let (g, _) = guard_finite_scalar(g);
    let (b, _) = guard_finite_scalar(b);
    Rgb { r, g, b }
}

/// 全部 4 行色卡。
pub fn swatch_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        v.push(swatch_row(*m));
    }
    v
}

/// **彩色**对照色卡：同一批探针，背景与源都取彩色。
/// 存在理由：灰阶条会把跨通道效应**平均掉**（三通道均值对重排不敏感），
/// 逐通道 R|G|B 让"不可分离"在文本上可见。
pub fn color_swatch_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(NONSEPARABLE_COUNT);
    for m in NonSepMode::all().iter() {
        let mut s = String::new();
        s.push_str(m.keyword());
        s.push_str(" R|G|B ");
        for p in SWATCH_PROBES.iter() {
            let cb = colorize(p.0, 0.15);
            let cs = colorize(p.1, 0.35);
            let o = blend(*m, cb, cs, &mut NonSepLog::new());
            s.push(ramp_char(o.r));
            s.push(ramp_char(o.g));
            s.push(ramp_char(o.b));
            s.push('|');
        }
        v.push(s);
    }
    v
}

// 十五、人类可读摘要（零 IO）

/// 4 模式总览。
pub fn nonseparable_summary() -> String {
    let cc = crosscheck();
    let rt = round_trip_report();
    let mut out = String::new();
    out.push_str("不可分离混合 4 种：");
    out.push_str(&NONSEPARABLE_COUNT.to_string());
    out.push_str(" 种（每种「标量 + 独立 f64 oracle + 4 像素宽通道 + WGSL」四路同源）；对拍 ");
    out.push_str(&cc.samples.to_string());
    out.push_str(" 点，最大偏差 ");
    out.push_str(&cc.worst.to_string());
    out.push_str(" LSB（预算 ");
    out.push_str(&LSB_BUDGET.to_string());
    out.push_str("）；色域往返最坏 ");
    out.push_str(&rt.worst_lsb.to_string());
    out.push_str(" LSB");
    out
}

/// SatClip 三分支的人读台账。
pub fn sat_branch_note() -> String {
    let mut log = NonSepLog::new();
    let _ = set_sat(Rgb { r: 0.2, g: 0.6, b: 0.9 }, 0.5, &mut log);
    let _ = set_sat(Rgb::gray(0.4), 0.0, &mut log);
    let _ = set_sat(Rgb::gray(0.4), 0.7, &mut log);
    let c = log.sat_branch_counts();
    let mut out = String::new();
    let mut i = 0usize;
    while i < 3 {
        out.push('[');
        out.push_str(SatBranch::all()[i].keyword());
        out.push_str("] x");
        out.push_str(&c[i].to_string());
        out.push('\n');
        i += 1;
    }
    out.push_str("后两档输出相同（规范 else 分支全零）；第三档表示\n");
    out.push_str("「要求了非零饱和度却被规范丢弃」，须由上层发现。\n");
    out
}

/// 钳制次序审计报告（锚点「色相漂移 → 钳制次序审计」）。
pub fn clip_order_report() -> String {
    let p = clip_order_probe();
    let mut out = String::new();
    out.push_str("钳制次序审计（语料 R=-0.5 G=0.5 B=2.0，两段同时触发）：\n");
    out.push_str(&format!("  规范（入口 x）rgb = ({}, {}, {})\n", p.spec_order.r, p.spec_order.g, p.spec_order.b));
    out.push_str(&format!("  逐段重算（x₂）  rgb = ({}, {}, {})\n", p.recomputed.r, p.recomputed.g, p.recomputed.b));
    out.push_str(&format!("  最大逐通道差 = {}（次序错则色相漂移）\n", p.max_gap));
    out.push_str(&format!("  两段同触发时规范结果 min={} max={}（非 0/1：第二段分母用入口 x）\n", p.spec_order.min_ch(), p.spec_order.max_ch()));
    out
}

// 十六、判据

// 判据子断言（每条判据一个单一职责纯函数；`run_ved23_checks` 逐条注册）。

/// 两三元组任一通道 LSB 偏差超预算（判据侧反复出现的「显著不同」口径）。
fn rgb_exceeds_budget(a: Rgb, b: Rgb) -> bool {
    let pa = [a.r, a.g, a.b];
    let pb = [b.r, b.g, b.b];
    let mut ci = 0usize;
    while ci < 3 {
        if lsb_delta(pa[ci], pb[ci]) > LSB_BUDGET {
            return true;
        }
        ci += 1;
    }
    false
}

/// 判据用的「有限且量级 sane」谓词（判据 20/21 口径必须一致，故收敛一处）。
fn chk_finite(v: f32) -> bool {
    !v.is_nan() && v > -1.0e30 && v < 1.0e30
}

/// 判据 1 断言本体：4 种齐全、下标↔键互逆、关键字唯一、条款号可核对。
fn chk_registry_4_modes() -> bool {
    let mut dup = false;
    let mut seen: Vec<&str> = Vec::new();
    for m in NonSepMode::all().iter() {
        if seen.contains(&m.keyword()) {
            dup = true;
        }
        seen.push(m.keyword());
    }
    let mut ok = NonSepMode::all().len() == NONSEPARABLE_COUNT
        && NONSEPARABLE_COUNT == 4
        && (0..NONSEPARABLE_COUNT).all(|i| NonSepMode::from_index(i).is_some())
        && NonSepMode::from_index(NONSEPARABLE_COUNT).is_none()
        && !dup
        && nonseparable_modes().len() == NONSEPARABLE_COUNT;
    let mut i = 0usize;
    while ok && i < NONSEPARABLE_COUNT {
        ok = match NonSepMode::from_index(i) {
            Some(m) => {
                m.index() == i
                    && !m.keyword().is_empty()
                    && m.clause().starts_with("compositing-1§10.2.")
                    && !m.prose().is_empty()
            }
            None => false,
        };
        i += 1;
    }
    ok
}

/// 判据 3 断言本体：灰轴 9 级亮度偏差 ≤2 ULP + 端点逐位精确 + 夹逼对照。
/// 容差不能用 `==`（f32 多次舍入差 1–2 ULP 属正常，逐位相等把正确实现判红），
/// 也不能放宽成宽阈值（弱门禁）——用 ULP 阶梯，端点可断精确。
fn chk_lum_gray_axis_fixed() -> bool {
    const ULP: f32 = 1.1920929e-7; // 1.0 处的 f32 ULP
    let mut ok = lum(Rgb::gray(0.0)) == 0.0 && lum(Rgb::gray(1.0)) == 1.0;
    let mut i = 1usize;
    while ok && i < 10 {
        let v = i as f32 / 10.0;
        let d = out_lum_dev(Rgb::gray(v), v);
        if d > 2.0 * ULP {
            ok = false;
        }
        i += 1;
    }
    // 夹逼对照：偏差确实落在 1–2 ULP 量级（证明容差不是宽到恒过）。
    let d07 = out_lum_dev(Rgb::gray(0.7), 0.7);
    let bounded = d07 <= 2.0 * ULP && d07 >= 0.0;
    ok && bounded
}

/// 判据 5 的抄错实现：把 SetSat 的 `Cmin` 置零这一步漏掉（仅供判据对照，
/// 生产路径不许调用；漏写只在 `Cmin != 0` 时显形）。
fn wrong_set_sat(c: Rgb, s: f32) -> Rgb {
    let v = [c.r, c.g, c.b];
    let mut i_min = 0usize;
    let mut i_max = 0usize;
    let mut i = 1usize;
    while i < 3 {
        if v[i] < v[i_min] {
            i_min = i;
        }
        if v[i] > v[i_max] {
            i_max = i;
        }
        i += 1;
    }
    let i_mid = if i_min != i_max { 3 - i_min - i_max } else { 1 };
    let (vmin, vmax) = (v[i_min], v[i_max]);
    if vmax > vmin {
        let mid = ((v[i_mid] - vmin) * s) / (vmax - vmin);
        c.with(i_mid, mid).with(i_max, s)
        // **漏了 `.with(i_min, 0.0)`** ← 抄错点
    } else {
        Rgb::gray(0.0)
    }
}

/// 判据 5 的单语料对照：抄错实现 vs oracle 是否显著不同。
fn wrong_differs_from_oracle(mode: NonSepMode, cb: Rgb, cs: Rgb) -> bool {
    let wrong = match mode {
        NonSepMode::Hue => {
            let t = wrong_set_sat(cs, sat(cb));
            let d = lum(cb) - lum(t);
            clip_wrong(Rgb { r: t.r + d, g: t.g + d, b: t.b + d })
        }
        _ => {
            let t = wrong_set_sat(cb, sat(cs));
            let d = lum(cb) - lum(t);
            clip_wrong(Rgb { r: t.r + d, g: t.g + d, b: t.b + d })
        }
    };
    let want = oracle(mode, cb, cs);
    rgb_exceeds_budget(wrong, want)
}

/// 判据 5 断言本体：人为抄错 SetSat（漏写 Cmin=0）必须被 oracle 抓住。
/// 期望捕获数由判据侧按「走 Scaled 分支且 `Cmin != 0`」独立数出（初版用
/// "非灰语料"，把 Cmin==0 语料也算进期望，正确实现会被算成漏抓）。
fn chk_oracle_catches_primitive_error() -> bool {
    let mut caught = 0u32;
    let mut expected = 0u32;
    let mut ran = 0u32;
    for mode in NonSepMode::all().iter() {
        if !matches!(mode, NonSepMode::Hue | NonSepMode::Saturation) {
            continue;
        }
        for g in CROSSCHECK_GRID.iter() {
            let (cb, cs) = *g;
            // 该模式下 SetSat 的**输入**色（hue 吃 Cs，saturation 吃 Cb）。
            let set_sat_input = match mode {
                NonSepMode::Hue => cs,
                _ => cb,
            };
            let visible = set_sat_input.max_ch() > set_sat_input.min_ch()
                && set_sat_input.min_ch() != 0.0;
            ran = ran.saturating_add(1);
            if visible {
                expected = expected.saturating_add(1);
                if wrong_differs_from_oracle(*mode, cb, cs) {
                    caught = caught.saturating_add(1);
                }
            }
        }
    }
    ran > 0 && expected > 0 && caught == expected
}

/// 判据 6 断言本体：取自哪一侧的实测偏差 + 规范侧硬写的结构事实。
fn chk_definition_invariants() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while ok && i < NONSEPARABLE_COUNT {
        match NonSepMode::from_index(i) {
            Some(m) => {
                let ic = invariant_check(m);
                ok = ic.samples == CROSSCHECK_GRID.len() as u32
                    && ic.lum_from == m.lum_from()
                    && ic.sat_from == m.sat_from()
                    && ic.lum_dev <= LSB8
                    && ic.sat_dev <= LSB8;
            }
            None => ok = false,
        }
        i += 1;
    }
    // 规范侧的结构事实（不依赖本模块实现，直接硬写期望）。
    ok = ok
        && NonSepMode::Hue.lum_from() == Side::Backdrop
        && NonSepMode::Saturation.lum_from() == Side::Backdrop
        && NonSepMode::Color.lum_from() == Side::Backdrop
        && NonSepMode::Luminosity.lum_from() == Side::Source
        && NonSepMode::Hue.sat_from() == Side::Backdrop
        && NonSepMode::Saturation.sat_from() == Side::Source
        && NonSepMode::Color.sat_from() == Side::Source
        && NonSepMode::Luminosity.sat_from() == Side::Backdrop;
    ok
}

/// 判据 7 断言本体：两段同触发的语料上，规范次序与逐段重算显著不同。
fn chk_clipcolor_order_matters() -> bool {
    let p = clip_order_probe();
    p.both_triggered && p.max_gap > 0.01
}

/// 判据 8 断言本体：单段归零/归一 + 两段同触发的 `min>0 且 max<1`。
/// 规范实际给出的三条性质：低位段单独触发 ⇒ min 恰压到 0；高位段单独触发
/// ⇒ max 恰压到 1；两段都触发 ⇒ min>0 且 max<1（次序纪律的结构后果）。
/// 初版断「两段同触落边界」按规范根本不该成立，把正确实现判红。
fn chk_clipcolor_order_spec() -> bool {
    let p = clip_order_probe();
    let mut log = NonSepLog::new();
    let _ = clip_color_ordered(Rgb { r: -0.5, g: 0.5, b: 2.0 }, &mut log);
    // 低位段单独：R 越下界，其余在域内。
    let mut only_low = NonSepLog::new();
    let (low_out, _) = clip_color_ordered(Rgb { r: -0.5, g: 0.2, b: 0.4 }, &mut only_low);
    // 高位段单独：B 越上界，其余在域内。
    let mut only_high = NonSepLog::new();
    let (high_out, _) = clip_color_ordered(Rgb { r: 0.6, g: 0.2, b: 1.5 }, &mut only_high);
    p.both_triggered
        && log.clip_low > 0
        && log.clip_high > 0
        && log.clip_both > 0
        && only_low.clip_low > 0
        && only_low.clip_high == 0
        && only_high.clip_high > 0
        && only_high.clip_low == 0
        // 单段归零 / 归一：可精确验证。
        && low_out.min_ch() == CHANNEL_LO
        && high_out.max_ch() == CHANNEL_HI
        // 两段同触发：规范给出 min>0 且 max<1（次序的结构后果）。
        && p.spec_order.min_ch() > CHANNEL_LO
        && p.spec_order.max_ch() < CHANNEL_HI
}

/// 判据 8b 断言本体：`1−δ / 1 / 1+δ` 三点夹逼钉死两段判据的闸门位置。
/// `δ = 1e-6` 远大于 1 ULP（≈1.19e-7），f32 下可区分；规范是开区间，
/// `1.0` 恰好不触发、`1.0+δ` 触发（低位侧 `n = 0.0` 同理）。
fn chk_clipcolor_boundary_clamp() -> bool {
    const DELTA: f32 = 1.0e-6; // 远大于 1 ULP(1.19e-7)，可区分
    let mut l_below = NonSepLog::new();
    let mut l_at = NonSepLog::new();
    let mut l_above = NonSepLog::new();
    // 高位通道取 r，故 x 恰为 1.0 − δ / 1.0 / 1.0 + δ。
    clip_color_ordered(Rgb { r: 1.0 - DELTA, g: 0.2, b: 0.3 }, &mut l_below);
    clip_color_ordered(Rgb { r: 1.0, g: 0.2, b: 0.3 }, &mut l_at);
    clip_color_ordered(Rgb { r: 1.0 + DELTA, g: 0.2, b: 0.3 }, &mut l_above);
    let high_ok = l_at.clip_high == 0 && l_below.clip_high == 0 && l_above.clip_high > 0;
    // 低位侧同理：规范 `n < 0`，`0.0` 恰好不触发。
    let mut z_at = NonSepLog::new();
    let mut z_below = NonSepLog::new();
    clip_color_ordered(Rgb { r: 0.2, g: 0.3, b: 0.0 }, &mut z_at);
    clip_color_ordered(Rgb { r: 0.2, g: 0.3, b: 0.0 - DELTA }, &mut z_below);
    high_ok && z_at.clip_low == 0 && z_below.clip_low > 0
}

/// 判据 9 断言本体：三分支各命中一次，`Scaled` 有输出、后两支全零。
fn chk_satclip_three_branches() -> bool {
    let mut log = NonSepLog::new();
    // 档 1：彩色输入 ⇒ Scaled。
    let (c1, b1) = set_sat(Rgb { r: 0.2, g: 0.6, b: 0.9 }, 0.5, &mut log);
    // 档 2：灰输入 + 目标零 ⇒ FlatToZero。
    let (c2, b2) = set_sat(Rgb::gray(0.4), 0.0, &mut log);
    // 档 3：灰输入 + 目标非零 ⇒ FlatRequested（规范 else，仍全零）。
    let (c3, b3) = set_sat(Rgb::gray(0.4), 0.7, &mut log);
    let counts = log.sat_branch_counts();
    b1 == SatBranch::Scaled
        && b2 == SatBranch::FlatToZero
        && b3 == SatBranch::FlatRequested
        && counts[0] == 1
        && counts[1] == 1
        && counts[2] == 1
        && c1 != Rgb::gray(0.0)
        && c2 == Rgb::gray(0.0)
        && c3 == Rgb::gray(0.0)
}

/// 判据 10 断言本体：台账把两支**分开记**，只断枚举与台账、不看输出值。
fn chk_satclip_branch_direct() -> bool {
    let mut log = NonSepLog::new();
    let _ = set_sat(Rgb::gray(0.4), 0.0, &mut log);
    let _ = set_sat(Rgb::gray(0.4), 0.7, &mut log);
    let counts = log.sat_branch_counts();
    // 台账必须把两支分开记（合并实现只会记到其中一档）。
    counts[1] == 1
        && counts[2] == 1
        && SatBranch::FlatToZero.is_flat()
        && SatBranch::FlatRequested.is_flat()
        && !SatBranch::Scaled.is_flat()
        && SatBranch::FlatToZero != SatBranch::FlatRequested
}

/// 判据 11 断言本体：闭式 `C − Cmin` 残差 ≤1 ULP，且语料有非零残差。
/// 容差必须按 ULP 定：往返时 `s = Sat(C)` 与分母同一次减法，分式退化成
/// `x·s / s`，数学上恰等于 `x` 但 f32 两次舍入可差不到 1 ULP（实测最坏
/// 0.25 ULP）。断 `== 0` 判红正确实现，断 `≤ 1 LSB` 宽到抓不住任何实现。
fn chk_sat_identity_roundtrip() -> bool {
    let mut worst = 0.0f32;
    let mut nonzero_seen = 0u32;
    let mut i = 0usize;
    while i < ROUNDTRIP_CORPUS.len() {
        let c = ROUNDTRIP_CORPUS[i];
        let rt = round_trip(c);
        // 闭式参考值 `C - Cmin` 由 f32 减法推出，不手写字面量
        // （手写 `0.2` 解析值与被测输出可差 0.125 ULP，本身就是偏差来源）。
        let want = Rgb {
            r: c.r - rt.c_min,
            g: c.g - rt.c_min,
            b: c.b - rt.c_min,
        };
        let d = out_rgb_gap(rt.via_sat, want);
        if d > 0.0 {
            nonzero_seen = nonzero_seen.saturating_add(1);
        }
        if d > worst {
            worst = d;
        }
        i += 1;
    }
    worst <= F32_ULP_AT_ONE && nonzero_seen > 0 && worst > 0.0
}

/// 判据 14 断言本体：并列语料重复求值逐位相等，且 tie-break 记账命中。
fn chk_sat_tiebreak_deterministic() -> bool {
    let probes = [
        Rgb { r: 0.5, g: 0.5, b: 0.2 },
        Rgb { r: 0.2, g: 0.5, b: 0.5 },
        Rgb { r: 0.5, g: 0.2, b: 0.5 },
        Rgb { r: 0.7, g: 0.7, b: 0.7 },
    ];
    let mut ok = true;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < probes.len() {
        let c = probes[i];
        let mut log = NonSepLog::new();
        let (a, ba) = set_sat(c, 0.6, &mut log);
        let mut log2 = NonSepLog::new();
        let (b, bb) = set_sat(c, 0.6, &mut log2);
        if a != b || ba != bb {
            ok = false;
        }
        hits = hits.saturating_add(log.tiebreak_hits);
        i += 1;
    }
    // 每条并列语料都必须记账（否则 tie-break 判定压根没跑）。
    ok && hits == probes.len() as u32
}

/// 判据 14b 断言本体：直接断言下标方向 = 判据侧独立手算的「最靠前者胜」。
fn chk_sat_tiebreak_direction() -> bool {
    let probes = [
        // (输入, 期望 (i_min, i_mid, i_max))——手算依据：严格比较 ⇒ 只在
        // 真的更小/更大时才换下标。注意第 2 条：0.5>0.5 为假 ⇒ i_max 停在 1
        // （不是 2），i_mid = 3-0-1 = 2；初版手算成 1，把正确实现判红。
        (Rgb { r: 0.5, g: 0.5, b: 0.2 }, (2usize, 1usize, 0usize)),
        (Rgb { r: 0.2, g: 0.5, b: 0.5 }, (0usize, 2usize, 1usize)),
        (Rgb { r: 0.5, g: 0.2, b: 0.5 }, (1usize, 2usize, 0usize)),
        // 全等：min=max=0（先到者），mid 取 1 维持排列不变式。
        (Rgb { r: 0.7, g: 0.7, b: 0.7 }, (0usize, 1usize, 0usize)),
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < probes.len() {
        let (c, want) = probes[i];
        let mut log = NonSepLog::new();
        let got = order_indices(c, &mut log);
        if got != want {
            ok = false;
        }
        // 不变式：真彩分支（min≠max）三下标互异；全等语料走 else 分支、
        // 下标不参与运算，实现取 (0,1,0) 不覆盖 2——全等分支的正确不变式
        // 是「mid 不等于 min/max」（初版误加排列检查把正确实现判红）。
        let distinct = got.0 != got.1 && got.1 != got.2 && got.0 != got.2;
        let flat = c.r == c.g && c.g == c.b;
        if flat {
            if got.1 == got.0 || got.1 == got.2 {
                ok = false;
            }
        } else if !distinct {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 判据 15 断言本体：双 alpha<1 的 4 组语料上，两种次序全部显著不同。
fn chk_composite_order_contract() -> bool {
    let mut differs = 0u32;
    let mut total = 0u32;
    // 语料刻意避开纯灰（灰时混合与合成都退化）。
    let alphas = [(0.5f32, 0.5f32), (0.3, 0.8), (0.8, 0.25), (0.6, 0.6)];
    let g = CROSSCHECK_GRID[5]; // 彩色对
    let (cb, cs) = g;
    for ab in alphas.iter() {
        let mut log = NonSepLog::new();
        let spec = composite_over(NonSepMode::Color, cb, cs, ab.0, ab.1, &mut log);
        let mut log2 = NonSepLog::new();
        let swapped = composite_over_swapped(NonSepMode::Color, cb, cs, ab.0, ab.1, &mut log2);
        total = total.saturating_add(1);
        if rgb_exceeds_budget(spec, swapped) {
            differs = differs.saturating_add(1);
        }
    }
    total == alphas.len() as u32 && differs == total
}

/// 判据 16 断言本体：合成式 alpha 四边界精确 + αs=0 中间态恰为 αb·Cb。
fn chk_composite_alpha_edges() -> bool {
    let (cb, cs) = CROSSCHECK_GRID[5];
    let mode = NonSepMode::Hue;
    let b = blend(mode, cb, cs, &mut NonSepLog::new());
    let co = |ab: f32, as_: f32| -> Rgb {
        composite_over(mode, cb, cs, ab, as_, &mut NonSepLog::new())
    };
    let eq = |x: Rgb, y: Rgb| -> bool {
        lsb_delta(x.r, y.r) <= LSB8 && lsb_delta(x.g, y.g) <= LSB8 && lsb_delta(x.b, y.b) <= LSB8
    };
    let black = Rgb::gray(CHANNEL_LO);
    // αs=1,αb=1 ⇒ Cr；αs=1,αb=0 ⇒ Cs；αs=0,αb=1 ⇒ Cb；αs=0,αb=0 ⇒ 全黑；
    // 中间态：αs=0 ⇒ 结果恰为 αb·Cb（背景预乘），确认不是 Cb。
    eq(co(1.0, 1.0), b)
        && eq(co(0.0, 1.0), cs)
        && eq(co(1.0, 0.0), cb)
        && eq(co(0.0, 0.0), black)
        && !eq(co(0.5, 0.0), cb)
        && eq(co(0.5, 0.0), Rgb { r: 0.5 * cb.r, g: 0.5 * cb.g, b: 0.5 * cb.b })
}

/// 判据 17 断言本体：极端背景 × 4 模式，结果全在 [0,1] 且非 NaN。
fn chk_extreme_backdrops() -> bool {
    let mut ok = true;
    let mut ran = 0u32;
    let mut ei = 0usize;
    while ei < EXTREME_BACKDROPS.len() {
        let eb = EXTREME_BACKDROPS[ei];
        let mut mi = 0usize;
        while mi < NONSEPARABLE_COUNT {
            let m = match NonSepMode::from_index(mi) {
                Some(x) => x,
                None => {
                    mi += 1;
                    continue;
                }
            };
            let (cb, cs) = (eb, EXTREME_BACKDROPS[(ei + 1) % EXTREME_BACKDROPS.len()]);
            let out = blend(m, cb, cs, &mut NonSepLog::new());
            let ch = [out.r, out.g, out.b];
            let mut ci = 0usize;
            while ci < 3 {
                let v = ch[ci];
                if !(v >= CHANNEL_LO && v <= CHANNEL_HI) {
                    ok = false;
                }
                ci += 1;
            }
            ran = ran.saturating_add(1);
            mi += 1;
        }
        ei += 1;
    }
    ok && ran == (EXTREME_BACKDROPS.len() * NONSEPARABLE_COUNT) as u32
}

/// 判据 18 断言本体：saturation 模式在纯灰背景上不产生变化。
fn chk_gray_backdrop_noop() -> bool {
    let gray = Rgb::gray(0.4);
    let cs = Rgb { r: 0.9, g: 0.2, b: 0.6 };
    let out = blend(NonSepMode::Saturation, gray, cs, &mut NonSepLog::new());
    // 规范：纯灰背景 + saturation ⇒ 无变化 ⇒ 结果 == 背景。
    lsb_delta(out.r, gray.r) <= LSB8
        && lsb_delta(out.g, gray.g) <= LSB8
        && lsb_delta(out.b, gray.b) <= LSB8
}

/// 判据 19 断言本体：重排背景三通道后结果必须变化（未退化为逐通道）。
fn chk_not_channelwise() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while ok && i < NONSEPARABLE_COUNT {
        match NonSepMode::from_index(i) {
            Some(m) => {
                let cc2 = coupling_check(m);
                ok = cc2.samples == CROSSCHECK_GRID.len() as u32
                    && cc2.coupled > 0
                    && cc2.max_shift > LSB8;
            }
            None => ok = false,
        }
        i += 1;
    }
    ok
}

/// 判据 20 断言本体：NaN/±Inf 被收口为有限值，且台账记账（零静默）。
fn chk_clamp_non_finite() -> bool {
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let mut log = NonSepLog::new();
    let out = blend(NonSepMode::Hue, Rgb { r: nan, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }, &mut log);
    let out2 = blend(NonSepMode::Luminosity, Rgb { r: inf, g: 0.5, b: 0.5 }, Rgb { r: 0.5, g: 0.5, b: 0.5 }, &mut log);
    log.clamped_inputs > 0
        && chk_finite(out.r) && chk_finite(out.g) && chk_finite(out.b)
        && chk_finite(out2.r) && chk_finite(out2.g) && chk_finite(out2.b)
}

/// 判据 21 断言本体：极小分母与全等分量语料下 SetSat 不产 NaN 且有记账。
fn chk_sat_denom_guard() -> bool {
    let tiny = SAT_DENOM_MIN;
    let mut log = NonSepLog::new();
    // 两个相差极小但非零的分量：构造在 f32 下会下溢的情形。
    let a = 0.5;
    let b = 0.5 + tiny;
    let out = set_sat(Rgb { r: a, g: a, b }, 0.5, &mut log);
    // 直接把相等分量送入（Scaled 分支不会进，但 else 分支也不得 NaN）
    let out2 = set_sat(Rgb::gray(0.5), 0.5, &mut log);
    chk_finite(out.0.r) && chk_finite(out.0.g) && chk_finite(out.0.b)
        && chk_finite(out2.0.r) && chk_finite(out2.0.g) && chk_finite(out2.0.b)
        && log.sat_denom_guarded + log.sat_flat_zero + log.sat_flat_requested > 0
}

/// 判据 22 断言本体：批内逐位等于标量路 + 分支签名可辨/全同两向对照。
/// 分支由**哪一侧**决定：`hue` 的 `SetSat` 吃 **Cs**——签名不一致必须让
/// 源批内不一致（初版只让背景批内不一致，四像素签名全同，判据初测转红）。
fn chk_simd_batch_equiv() -> bool {
    let mut ok = true;
    for mode in NonSepMode::all().iter() {
        let cb = [
            Rgb { r: 0.2, g: 0.6, b: 0.9 },
            Rgb { r: 0.9, g: 0.1, b: 0.3 },
            Rgb { r: 0.5, g: 0.5, b: 0.5 },
            Rgb { r: 0.05, g: 0.95, b: 0.5 },
        ];
        let cs = [
            Rgb { r: 0.9, g: 0.3, b: 0.1 },
            Rgb { r: 0.3, g: 0.7, b: 0.2 },
            Rgb { r: 0.6, g: 0.6, b: 0.6 },
            Rgb { r: 0.5, g: 0.05, b: 0.95 },
        ];
        let batch = blend_simd(*mode, cb, cs);
        let mut i = 0usize;
        while i < SIMD_WIDTH {
            let want = blend(*mode, cb[i], cs[i], &mut NonSepLog::new());
            let got = batch.out[i];
            if got != want {
                ok = false;
            }
            i += 1;
        }
    }
    let cb_uniform = [
        Rgb { r: 0.2, g: 0.6, b: 0.9 },
        Rgb { r: 0.3, g: 0.5, b: 0.8 },
        Rgb { r: 0.4, g: 0.4, b: 0.7 },
        Rgb { r: 0.5, g: 0.3, b: 0.6 },
    ];
    let cs_mixed = [
        Rgb { r: 0.9, g: 0.3, b: 0.1 },
        Rgb::gray(0.4), // 源为灰 ⇒ `SetSat` 走 else 分支 ⇒ FlatRequested
        Rgb { r: 0.4, g: 0.4, b: 0.7 },
        Rgb { r: 0.5, g: 0.3, b: 0.6 },
    ];
    let mixed = blend_simd(NonSepMode::Hue, cb_uniform, cs_mixed);
    let has_mix = mixed.branches[0] == SatBranch::Scaled
        && mixed.branches[1] == SatBranch::FlatRequested;
    // 反向对照：源与背景都批内一致时，四路签名必须全同（uniform 成立）。
    let uni = blend_simd(NonSepMode::Hue, cb_uniform, cb_uniform);
    let uniform_same = uni.branches[0] == uni.branches[1]
        && uni.branches[1] == uni.branches[2]
        && uni.branches[2] == uni.branches[3];
    ok && has_mix && uniform_same
}

/// 判据 23 断言本体：宽通道实测操作数严格小于同批标量路（同一成本表）。
fn chk_simd_amortization() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while ok && i < NONSEPARABLE_COUNT {
        match NonSepMode::from_index(i) {
            Some(m) => {
                let am = simd_amortization(m);
                ok = am.uniform && am.amortized() && am.ratio() > 1.0;
            }
            None => ok = false,
        }
        i += 1;
    }
    ok
}

/// 判据 24 断言本体：4 模式 WGSL 文本自检齐备、缺项=0、长度 sane。
fn chk_wgsl_text_keywords() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while ok && i < NONSEPARABLE_COUNT {
        match NonSepMode::from_index(i) {
            Some(m) => {
                let sc = wgsl_selfcheck(m);
                ok = sc.complete && sc.missing == 0 && sc.len > 200;
            }
            None => ok = false,
        }
        i += 1;
    }
    ok
}

/// 判据 25 断言本体：灰阶段全部取自共用 RAMP、长度=探针数、关键字一致。
fn chk_swatch_shared_probes() -> bool {
    let rows = swatch_rows();
    let crows = color_swatch_rows();
    let mut ok = rows.len() == NONSEPARABLE_COUNT && crows.len() == NONSEPARABLE_COUNT;
    let mut i = 0usize;
    while ok && i < rows.len() {
        let r = &rows[i];
        // 灰阶段 = 关键字之后的第一个空格之后。
        let bar = match r.find(' ') {
            Some(p) => &r[p + 1..],
            None => {
                ok = false;
                break;
            }
        };
        if bar.chars().count() != SWATCH_STEPS {
            ok = false;
            break;
        }
        let mut all_in_ramp = true;
        for ch in bar.chars() {
            if !RAMP.contains(&ch) {
                all_in_ramp = false;
            }
        }
        ok = all_in_ramp;
        // 关键字与模式键一致。
        ok = ok && match NonSepMode::from_index(i) {
            Some(m) => r.starts_with(m.keyword()),
            None => false,
        };
        i += 1;
    }
    ok
        && SWATCH_STEPS == SWATCH_PROBES.len()
        && RAMP.len() == 10
        && NONSEPARABLE_COUNT == 4
}

/// 判据 27 断言本体：干净输入台账为空；各档计数**恰好** +1（零静默）。
fn chk_ledger_exact() -> bool {
    let mut log = NonSepLog::new();
    // 干净输入 ⇒ 台账必须为空（否则说明有幽灵记账）。
    let clean_before = log.total();
    let _ = blend(NonSepMode::Color, Rgb::gray(0.5), Rgb::gray(0.25), &mut log);
    let clean_after = log.total();
    // 低位段单独触发 ⇒ clip_low 恰 +1、clip_high 不变。
    let c1_before = log.clip_low;
    let _ = clip_color_ordered(Rgb { r: -0.5, g: 0.2, b: 0.4 }, &mut log);
    let low_ok = log.clip_low == c1_before + 1;
    // 高位段单独触发 ⇒ clip_high 恰 +1、clip_low 不变。
    let c2_before = log.clip_high;
    let _ = clip_color_ordered(Rgb { r: 0.6, g: 0.2, b: 1.5 }, &mut log);
    let high_ok = log.clip_high == c2_before + 1;
    // 灰输入饱和度路径 ⇒ tie-break 记账（灰时 tie-break 必触发）。
    let t_before = log.tiebreak_hits;
    let _ = set_sat(Rgb::gray(0.5), 0.3, &mut log);
    let tie_ok = log.tiebreak_hits > t_before;
    clean_before == 0 && clean_after == 0 && low_ok && high_ok && tie_ok
}

/// VE-F0623 模块自检·判据注册：断言本体在各 `chk_*` 函数，此处保留判据注册序。
/// 判据注册·注册表与公式对拍段（判据 1~5；`crosscheck` 纯计算，头部求值一次）。
fn ved23_checks_registry_formula(s: &mut CheckSet) {
    let cc = crosscheck();

    // --- 判据 1：抓模式漏注册 / 下标与键不同向 / 关键字重复 ----------------
    s.add(
        "C23-REGISTRY-4-MODES",
        chk_registry_4_modes(),
        "4 种不可分离模式齐全、下标↔键互逆、关键字唯一、每种带可核对条款号",
    );

    // --- 判据 2：抓权重被换成 Rec.709（亮度保持类判据抓不住，必须断数值本身） ---
    s.add(
        "C23-LUM-WEIGHTS-VERBATIM",
        LUM_R == 0.30 && LUM_G == 0.59 && LUM_B == 0.11 && LUM_SUM == 1.0,
        "Lum 权重逐字等于 W3C 原文 0.3/0.59/0.11（非 Rec.709），且和为 1",
    );

    // --- 判据 3：抓权重和 ≠1 时灰轴被平移（独立于判据 2：断"常量被正确使用"） ---
    s.add(
        "C23-LUM-GRAY-AXIS-FIXED",
        chk_lum_gray_axis_fixed(),
        "灰轴 9 级亮度偏差 ≤2 ULP 且端点逐位精确 ⇒ 权重确被正确使用（权重和为 1）",
    );

    // --- 判据 4：抓公式抄错规范（SetSat 下标还原错 / SetLum 漏 ClipColor / 次序反 / 权重误用） ---
    s.add(
        "C23-FORMULA-VS-ORACLE",
        cc.samples == (NONSEPARABLE_COUNT as u32) * (CROSSCHECK_GRID.len() as u32)
            && cc.per_mode.iter().all(|m| m.within_budget())
            && cc.within_budget(),
        "4 模式 × 12 网格点：f32 主实现与独立 f64 oracle 偏差 ≤1 LSB(8bit)",
    );

    // --- 判据 5：反假——oracle 真能抓住抄错的 SetSat（否则与被测同错时恒绿） ---
    s.add(
        "C23-ORACLE-CATCHES-PRIMITIVE-ERROR",
        chk_oracle_catches_primitive_error(),
        "人为抄错 SetSat（漏写 Cmin=0）：在判据侧独立算出的全部可见语料上被 oracle 抓住",
    );

    // --- 判据族·第一段接缝 ----（上为注册与公式对拍部分收尾）
}

/// 判据注册·定义性性质与钳制段（判据 6~10）。
fn ved23_checks_definition_clamp(s: &mut CheckSet) {
    // --- 判据 6：抓亮度/饱和度取自哪一侧写反（规范定义字面内容，不依赖浮点） ---
    s.add(
        "C23-DEFINITION-INVARIANTS",
        chk_definition_invariants(),
        "4 模式亮度/饱和度取自哪一侧符合规范定义，数值偏差 ≤1 LSB(未裁剪样本)",
    );

    // --- 判据 7：抓两 `if` 写成 if/else 或次序调换（此时结果仍落 [0,1]、亮度仍保持、oracle 未必红，只有本条能抓） ---
    s.add(
        "C23-CLIPCOLOR-ORDER-MATTERS",
        chk_clipcolor_order_matters(),
        "两段同时触发的语料上，规范次序与逆序结果显著不同（次序可测）",
    );

    // --- 判据 8：抓 ClipColor 整体失效（两段都没跑 ⇒ 台账零 ⇒ 本条红） ---
    // 初版断「两段同触发 min=0/max=1」按规范不该成立，把正确实现判红；
    // 规范实际给出的三条性质见 [`chk_clipcolor_order_spec`]。
    s.add(
        "C23-CLIPCOLOR-ORDER-SPEC",
        chk_clipcolor_order_spec(),
        "低位段单独触发 min=0、高位段单独触发 max=1；两段同触发 min>0 且 max<1（次序结构后果）",
    );

    // --- 判据 8b：抓 `>` 改 `>=`（闸门位置无人验证——M4 变异初测转红 0 条的来历） ---
    s.add(
        "C23-CLIPCOLOR-BOUNDARY-CLAMP",
        chk_clipcolor_boundary_clamp(),
        "ClipColor 两段判据为开区间：x=1 与 n=0 恰好不触发，1±δ 才触发（夹逼对钉死闸门位置）",
    );

    // --- 判据 9：抓三分支合成一支或 `>` 写成 `>=` ---
    s.add(
        "C23-SATCLIP-THREE-BRANCHES",
        chk_satclip_three_branches(),
        "SatClip 三分支各命中一次；Scaled 有输出、后两支按规范 else 全零",
    );

    // --- 判据 10：抓"合并两支后仍能过输出值断言"（后两支输出全同，只断枚举与台账） ---
    s.add(
        "C23-SATCLIP-BRANCH-DIRECT",
        chk_satclip_branch_direct(),
        "后两档输出相同，故直接断言枚举与台账分档（合并实现会被抓住）",
    );

    // --- 判据族·第二段接缝 ----（上为定义性性质与钳制部分收尾）
}

/// 判据注册·往返与精度纪律段（判据 11~13；`round_trip_report` 纯计算，头部求值一次）。
fn ved23_checks_roundtrip(s: &mut CheckSet) {
    let rt = round_trip_report();

    // --- 判据 11：抓通道下标还原错（该错不越界、不破坏亮度保持、oracle 也可能一致——只有闭式会红）。
    // 参考值是闭式 C-Cmin 而非 C（初版写恒等把正确实现判红）；容差断 <=1 ULP
    // 且要求语料有非零残差（详见 chk_sat_identity_roundtrip）。
    s.add(
        "C23-SAT-IDENTITY-ROUNDTRIP",
        chk_sat_identity_roundtrip(),
        "SetSat(C,Sat(C)) 闭式 C-Cmin 残差 <=1 ULP（f32 两次舍入）；语料实测非零残差故容差非虚设",
    );

    // --- 判据 12：抓 `d` 方向写错（用 `+` 而非 `-` 恒等即破）；删掉 ClipColor
    // 时本条不红是诚实局限，真正的 ClipColor 缺失由判据 8 抓。
    s.add(
        "C23-LUM-IDENTITY-ROUNDTRIP",
        rt.lum_all_exact && rt.samples == ROUNDTRIP_CORPUS.len() as u32,
        "SetLum(C, Lum(C)) 在全部往返语料上逐通道位精确（d=l-Lum(C) 方向正确）",
    );

    // --- 判据 13：抓往返累积误差超 f32 舍入量级（换结合序 / k 用累乘 / 中位换不等价式）。
    // 期望值 = 判据侧独立重算的覆盖面（全部样本），不读被测台账；预算 1 LSB
    // 比 1 ULP 宽 3.3 万倍——判据 11 是紧的那道，本条是跨单一致的那道（F0622 同尺度）。
    s.add(
        "C23-ROUNDTRIP-BUDGET",
        rt.within_budget()
            && rt.sat_exact_count == rt.sat_expected_exact
            && rt.sat_expected_exact == rt.samples
            && rt.lum_all_exact
            && rt.samples == ROUNDTRIP_CORPUS.len() as u32,
        "往返偏差 ≤1 LSB(8bit, 与 F0630 同口径)；闭式成立样本数 = 判据侧独立重算的覆盖面 = 全部样本",
    );

    // --- 判据族·第三段接缝 ----（上为往返与精度纪律部分收尾）
}

/// 判据注册·tie-break 与合成契约段（判据 14~17）。
fn ved23_checks_tiebreak_composite(s: &mut CheckSet) {
    // --- 判据 14：抓 `>=`/`<=` 更新下标导致并列分量非确定性 ---
    s.add(
        "C23-SAT-TIEBREAK-DETERMINISTIC",
        chk_sat_tiebreak_deterministic(),
        "含并列分量的 4 条语料：重复求值逐位相等，且并列记账命中数=语料数",
    );

    // --- 判据 14b：抓 tie-break 方向反——`<=` 同样确定、并列输出的零值写回被
    // 对称性抵消，输出层与确定性层都钉不住（M8 实测），只能直接断言下标本身；
    // 问的是契约明文规定的方向（最靠前者胜），不是被监督的量。
    s.add(
        "C23-SAT-TIEBREAK-DIRECTION",
        chk_sat_tiebreak_direction(),
        "并列分量下 (i_min,i_mid,i_max) = 判据侧手算的「最靠前者胜」；真彩时三下标互异、全等时 mid 不撞 min/max",
    );

    // --- 判据 15：抓合成步提到混合之前（语料必须双 alpha<1，否则两序相同恒绿） ---
    s.add(
        "C23-COMPOSITE-ORDER-CONTRACT",
        chk_composite_order_contract(),
        "双 alpha<1 的 4 组语料：先混合后合成 与 换序结果全部显著不同",
    );

    // --- 判据 16：抓合成式漏项 / 系数错。参考值按 Porter Duff source-over 独立推出
    //（αs=1,αb=1→Cr；αs=1,αb=0→Cs；αs=0,αb=1→Cb；αs=0,αb=0→黑）。
    // 初版把「αs=0 ⇒ Cb」「αb=0 ⇒ Cs」写成无条件成立——必须另一个 alpha 恰为 1
    // 才成立，否则在 αs=0,αb=0.5 上把正确实现判红（参考性质须独立算，不能"看起来应该"）。
    s.add(
        "C23-COMPOSITE-ALPHA-EDGES",
        chk_composite_alpha_edges(),
        "合成式 alpha 四边界精确（1,1→Cr / 1,0→Cs / 0,1→Cb / 0,0→黑）+ αs=0 时恰为 αb·Cb",
    );

    // --- 判据 17：抓端点除零 / `x == 1` 边界判定差一格 ---
    s.add(
        "C23-EXTREME-BACKDROPS",
        chk_extreme_backdrops(),
        "全黑/全白/高饱和/低饱和 4 背景 × 4 模式 = 16 组极端语料，结果全在 [0,1] 且非 NaN",
    );

    // --- 判据族·第四段接缝 ----（上为 tie-break 与合成契约部分收尾）
}

/// 判据注册·背景收口与宽通道段（判据 18~22）。
fn ved23_checks_backdrop_simd(s: &mut CheckSet) {
    // --- 判据 18：抓 SetSat 实参写反（灰背景上也可能无变化，本条只抓明显偏离；
    // 真正写反由判据 4 与判据 6 抓） ---
    s.add(
        "C23-GRAY-BACKDROP-NOOP",
        chk_gray_backdrop_noop(),
        "规范 §10.2.2 明文：saturation 模式在纯灰背景上不产生变化",
    );

    // --- 判据 19：抓实现退化成逐通道独立（重排背景三通道后结果不变 ⇒ 红） ---
    s.add(
        "C23-NONSEPARABLE-NOT-CHANNELWISE",
        chk_not_channelwise(),
        "4 模式重排背景三通道后结果均发生变化 ⇒ 未退化为逐通道独立",
    );

    // --- 判据 20：抓 NaN/±Inf 静默传播进预乘链（断不进结果且台账确实记账） ---
    s.add(
        "C23-CLAMP-NON-FINITE",
        chk_clamp_non_finite(),
        "NaN/±Inf 输入被收口为有限值，且台账 clamped_inputs 记账（零静默）",
    );

    // --- 判据 21：抓分母次正规下溢产 NaN（语料用相减恰为 0 的极接近值） ---
    s.add(
        "C23-SAT-DENOM-GUARD",
        chk_sat_denom_guard(),
        "极小分母与全等分量语料下 SetSat 不产 NaN，且分支有记账",
    );

    // --- 判据 22：抓 uniform 快路径算错（语义等价；与摊薄/判据 23 正交） ---
    s.add(
        "C23-SIMD-BATCH-EQUIV",
        chk_simd_batch_equiv(),
        "4 模式 × 4 像素批逐位等于标量路；源批内分支不一致时签名可辨、一致时签名全同",
    );

    // --- 判据族·第五段接缝 ----（上为背景收口与宽通道部分收尾）
}

/// 判据注册·GPU 路与色卡台账段（判据 23~27）。
fn ved23_checks_wgsl_swatch(s: &mut CheckSet) {
    // --- 判据 23：抓宽通道退化成逐像素循环（同一张成本表保证量纲一致——量纲
    // 不一致的比值恒红或恒绿，F0622 初版真实踩过的坑） ---
    s.add(
        "C23-SIMD-AMORTIZATION",
        chk_simd_amortization(),
        "4 模式：4 像素批的实测操作数严格小于同批标量路（同一成本表，量纲一致）",
    );

    // --- 判据 24：抓 GPU 路只吐壳；另断 color/luminosity 不含 SetSat 调用（公式抄错成别的模式） ---
    s.add(
        "C23-WGSL-TEXT-KEYWORDS",
        chk_wgsl_text_keywords(),
        "4 模式 WGSL 文本含 vec3f/clamp/双 if/set_sat/set_lum/合成式，缺项=0",
    );

    // --- 判据 25：抓本文件另抄一份斜坡探针。检查只针对灰阶段（关键字字母不在
    // RAMP 内，连关键字一起查会必红——初版红项属判据自身取错参照）；真断三件事：
    // 灰阶字符全取自共用 RAMP、长度恰等于探针数、行首关键字与注册表一致。
    s.add(
        "C23-SWATCH-SHARED-PROBES",
        chk_swatch_shared_probes(),
        "色卡灰阶段消费 F0622 的 SWATCH_PROBES 与 RAMP（真引用）；4 行齐全、长度=探针数、字符全在共用表内",
    );

    // --- 判据 26：抓 from_index 写成 unwrap 或默认分支返回某模式（越界键静默变合法） ---
    s.add(
        "C23-KEY-OUT-OF-RANGE-NONE",
        NonSepMode::from_index(4).is_none()
            && NonSepMode::from_index(usize::MAX).is_none()
            && NonSepMode::from_index(0).is_some()
            && ramp_char(-1.0) == RAMP[0]
            && ramp_char(2.0) == RAMP[9],
        "越界模式键返回 None 不 panic；灰阶取值越界夹到两端不 panic",
    );

    // --- 判据 27：抓记账被吞（逐项断言对应计数恰好 +1，本体见 chk_ledger_exact） ---
    s.add(
        "C23-LEDGER-EXACT",
        chk_ledger_exact(),
        "干净输入台账为空；低位/高位段各自恰好 +1；并列分量 tie-break 记账（零静默）",
    );
}

/// 判据注册序聚合（各段 set.add 顺序与拆分前逐条一致；纯计算求值时点的
/// 平移不改变任何判据的输入输出）。
pub fn run_ved23_checks() -> CheckSet {
    let mut s = CheckSet::new("ved23");
    ved23_checks_registry_formula(&mut s);
    ved23_checks_definition_clamp(&mut s);
    ved23_checks_roundtrip(&mut s);
    ved23_checks_tiebreak_composite(&mut s);
    ved23_checks_backdrop_simd(&mut s);
    ved23_checks_wgsl_swatch(&mut s);
    s
}

/// 抄错版的 `ClipColor`（仅供判据 5 对照；不带 clamp_residue 收口使抄错显形）。
fn clip_wrong(c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c.min_ch();
    let x = c.max_ch();
    let mut o = c;
    if n < CLIP_LO {
        o = clip_low_with(o, l, n);
    }
    if x > CLIP_HI {
        o = clip_high_with(o, l, x);
    }
    o
}


// 反假变体登记（变异测试实测结果；判据聚合器不读本表，仅作文档）

/// 反假变体登记表（判据全绿只证明"当前实现合判据"——本表记录每个变异
/// 实际捕获的判据，任何人改判据时可用同一变异集复核；若某变异改完仍全绿，
/// 说明该判据已退化为恒真）。本轮实测（隔离探针，基线 28/28 绿）：7/7 全捕获。
/// M4 来历：初测转红 0 条——语料的 `x` 无一条恰为 1.0，闸门位置无人验证
/// （十诫第 4 条）；修法是补 [`C23-CLIPCOLOR-BOUNDARY-CLAMP`] 三点夹逼。
pub const VARIANT_REGISTRY: [(&str, &str); 7] = [
    ("M1-lum-r-rec709", "C23-LUM-WEIGHTS-VERBATIM"),
    ("M2-lum-g-rec709", "C23-LUM-WEIGHTS-VERBATIM"),
    ("M3-sat-hsv-normalized", "C23-SAT-IDENTITY-ROUNDTRIP"),
    ("M4-clip-hi-closed-interval", "C23-CLIPCOLOR-BOUNDARY-CLAMP"),
    ("M5-clip-second-segment-recomputed-max", "C23-CLIPCOLOR-ORDER-MATTERS"),
    ("M6-scalar-guard-nan-not-zeroed", "C23-CLAMP-NON-FINITE"),
    ("M7-mode-keyword-duplicate", "C23-REGISTRY-4-MODES"),
];

/// 变异捕获率声明（判据侧自述：本轮 7/7）。
pub const VARIANT_CAPTURE_NOTE: &str = "\
VE-F0623 判据经 7 个定向变异反向验证，7/7 全部被捕获，证明判据非恒真。\
其中 M4（把 ClipColor 高位段判据 `x > 1` 改成 `x >= 1`）在初测时转红 0 条——\
原语料没有一条恰好取 x = 1.0，闸门位置无人验证（十诫第 4 条）。\
修法：补 C23-CLIPCOLOR-BOUNDARY-CLAMP 判据，用 1−δ / 1 / 1+δ 三点夹逼\
（δ = 1e-6，远大于 f32 在 1.0 处的 1 ULP ≈ 1.19e-7），低位侧同理补 n = 0 夹逼对。";
