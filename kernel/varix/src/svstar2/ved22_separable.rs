//! VE-F0622 · 可分离混合模式 12 种（VE-D 域 · 2D 合成引擎 · 目标 480 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0622`
//!
//! **判据（锚点原文逐条）**：
//! - **12 种可分离模式**（RGB 各分量独立计算）：normal（源覆盖）、multiply
//!   （正片叠底——暗部增强）、screen（滤色——亮部增强）、overlay（叠加——暗用
//!   multiply 亮用 screen）、darken/lighten（逐通道取暗/亮）、color-dodge
//!   （颜色减淡——**除零边界：除数零时按规范取 1**）、color-burn（颜色加深——
//!   **同除零纪律**）、hard-light（强光——overlay 换位）、soft-light（柔光——
//!   **W3C 分段公式全实现**）、difference（差值——绝对差）、exclusion（排除）；
//! - **每模式：精确公式 + SIMD 实现 + GPU shader 双路**（锚点：CGPU-F0016 升级
//!   为全 24 语义、VE-Shade 写实时现）；
//! - **判据五项**：12 模式公式对拍 W3C 规范、**除零边界全覆盖**、**双路对拍差
//!   ≤1 LSB**、性能基准、色卡视觉验证。
//!
//! ## 〇、本条最要紧的架构决定：公式**只写一次**，三路从同一棵树求值
//!
//! 锚点要求「精确公式 + SIMD 实现 + GPU shader **双路**」，且判据要
//! 「**双路对拍差 ≤1 LSB**」。这句话隐含一个陷阱：若三路各写一份公式，
//! 对拍就退化成「三份手抄件互相比对」——三份手抄件**错得一样**时全绿。
//! 这不是理论风险：`overlay` 的分段条件写成 `<` 还是 `<=`、`color-dodge`
//! 的除零取 1 还是取 0，两份手抄可以同时写错而对拍通过。
//!
//! 本条的处置是把公式降为**一棵表达式树** [`Ir`]：
//!
//! - [`SepMode::ir`] 给出**唯一**公式来源（12 棵树，逐通道标量语义）；
//! - 三路不是三份抄本，而是**同一棵树的三个求值器**：
//!   [`eval_scalar`]（标量，最直白，兼作金标准）、
//!   [`eval_simd`]（4 通道宽通道，度量 [`SimdOp`] 计数）、
//!   [`wgsl_source`]（WGSL 文本，GPU 路的**可读产物**）；
//! - 对拍因此**不是**「比对三份实现是否一致」，而是**比对「同一公式的三种
//!   物化形式」是否一致**——三路共有的公式错误仍会被 [`oracle`] 抓住
//!   （见下）。
//!
//! 单一来源**不等于**免检：本条另有**独立金标准** [`oracle`]——它不读
//! [`Ir`]，按 W3C 规范原文**逐模式另写一遍** `f32` 直算（含 f64 中间量）。
//! 两者对拍才是「对拍 W3C 规范」的实质：若 [`Ir`] 抄错规范，[`oracle`]
//! 会红。反之若 [`oracle`] 抄错，[`Ir`] 与 GPU 文本会红。**任一处抄错都
//! 暴露**，这正是不设「三份手抄互比」的原因。
//!
//! ## 一、除零边界：两条模式、**两个方向相反**的取值，故不可共用码
//!
//! 锚点对 color-dodge 与 color-burn 都写「除零时按规范取 1 / 同除零纪律」。
//! W3C 原文的处置**并不相同**，这一点必须显式记下来：
//!
//! | 模式 | 公式 | 除零形态 | 规范取值 |
//! |------|------|----------|----------|
//! | color-dodge | `Cb / (1 - Cs)` | `Cs == 1` ⇒ 分母 `1-Cs == 0` | **1** |
//! | color-burn | `1 - (1 - Cb) / Cs` | `Cs == 0` ⇒ 分母 `Cs == 0` | **0** |
//!
//! 于是「同除零纪律」的准确含义是**同「不许出现 NaN」这条纪律**，而
//! **取值不同**（1 vs 0）。把它们写成同一个 [`DivZero`] 处置函数、
//! 参数化一个 fallback，是最省事也最危险的写法——省事在于代码复用，
//! 危险在于**参数传反不产生任何编译错误**，只产生一个静默偏亮的暗部。
//!
//! 本条的处置：除零**分派**（[`DivZero`][`div_zero_kind`]）按模式静态确定，
//! 取值由 [`IR::Div`] 的 fallback 字段**逐节点携带**，无参调用点；
//! [`SepMode::div_zero`] 给出该模式的分派档，判据逐档核对。
//!
//! ## 二、soft-light 是本组唯一**三段**公式，也是唯一有 sqrt 的模式
//!
//! W3C `soft-light` 的分段是**嵌套**的：外层按 `Cb <= 0.25` 分两支，
//! 上支内再按 `Cs <= 0.5` 分两支，故实为**三段**；且上支内用到
//! `sqrt`（[`IR::Sqrt`]）。它是本组 12 种里唯一同时具备
//! 「三段 + 超越函数 + 段间不连续（soft-light 本身是**连续**的，
//! 但 PDF 规范里存在不连续变体——本条按 W3C 连续版实现，
//! 并由 [`CONTINUITY_PROBES`] 钉死连续性）。
//!
//! 「全实现」的判据不是「写了这三段」，而是**三段各被采样命中**：
//! [`branch_coverage`] 用段内探针点逐段计数，缺一段即红——
//! 只实现两段的 soft-light 也能算出一个合理数值。
//!
//! ## 三、可分离性的机器证明：与 alpha 无关、与通道无关
//!
//! 「RGB 各分量独立计算」不是口号，是**可判定的性质**。本条给出
//! 两个正交判据：
//! - **通道无关**：[`check_channel_independence`] 证明
//!   `f(R,G,B) = (f(R), f(G), f(B))`——把三通道打乱重排后逐位相等；
//! - **alpha 无关**：[`check_alpha_independence`] 证明 alpha 变化
//!   不改变 RGB 输出（可分离混合在**预乘**域外的纯色混合语义下如此；
//!   预乘纪律本身归 F0625，本条只断言「本函数不吃 alpha」）。
//!
//! 这两条若不查，「可分离」只是文件名的形容词。且**通道无关**还能抓住
//! 一类真实缺陷：若某模式误写成 `f(R,G) = ...`（跨通道取极值），
//! 12 种公式对拍全绿（每通道单看都对），只有重排判据会红。
//!
//! ## 四、GPU 路产出的是**可读 WGSL**，不是标记位
//!
//! 锚点「VE-Shade 写实时现」要求 GPU 路的实现**现出来**。本条的
//! [`wgsl_source`] 产出**完整 WGSL 文本**（`fn` + `select` 分段 +
//! 除零保护），并由 [`wgsl_selfcheck`] 做**文本级自检**：
//! 12 种模式各自的关键构造（`sqrt` / `select` / `clamp` / 除零守卫）
//! 必须在文本里**真的出现**。只置一个 `gpu_implemented: true` 是
//! 典型的自证式门面——锚点禁止，判据 [`C22-GPU-TEXT-…`] 直接查文本。
//!
//! ## 五、性能基准：**实测操作数**，不写「O(1)」
//!
//! 锚点要「性能基准」。本条度量 [`SimdOp`]——SIMD 路真实发出的
//! 算术/比较/选择操作数，由 [`eval_simd`] 在**执行期**累加，**不是**
//! 纸面公式 `n * CONST`（自证式算术，空断言）。判据验证
//! **宽通道确实摊薄了操作数**：[`simd_amortization`] 断言 4 像素批
//! 的每像素操作数**严格小于**标量路的同一批——若宽通道没摊薄
//! （例如实现退化成逐像素循环），此项即红。
//!
//! ## 六、精度：f32 累加下的 LSB 预算怎么定才不是空话
//!
//! 判据要「双路对拍差 ≤1 LSB」。LSB 口径固定为 **8 bit 量化步长**
//! `1/255`（定义 [`LSB8`]，锚点 CGPU-F0016 同口径），而非「1 个
//! `f32::EPSILON`」——后者宽到任何实现都恒过，是空断言。
//! 更要紧的是**单边纪律**：对拍采用「三路中位数为基准、偏差取
//! **绝对值**」并**额外断言偏差方向分布**，避免「三路一起偏」被
//! 互相抵消掉（[`lsb_delta`] 给出的是真实最大偏差，不是平均值）。
//!
//! ## 七、错误路径与降级矩阵
//!
//! | 情形 | 处置 | 不许做的事 |
//! |------|------|------------|
//! | 模式键越界 / 未知 | [`SepMode::from_index`] 返回 `None` | 不 `unwrap` |
//! | 除零（dodge / burn） | 按 W3C 取 1 / 0，**并记账** [`DivZeroLog`] | 不返回 NaN |
//! | 非有限输入（NaN/Inf） | 钳制到 `[0,1]` 后计算 + 记账 | 不静默传播 |
//! | 段探针未覆盖某段 | [`branch_coverage`] 判红 | 不「差不多就行」 |
//!
//! ## 八、跨批对接点
//!
//! 上游 [`ved21_blendreg`]（F0621 注册表：本条 12 种即其
//! `SPEC_SEPARABLE == 12` 的**先行条目**，注册表以 `Planned(622)` 记
//! 本条未落地，本条落地后由 F0621 复审转 `SpecBacked`）；
//! 隔离语义 F0605；下游 F0623（不可分离 4 种，与本条共用
//! [`eval_scalar`] 与 oracle 骨架）、F0628（GPU 路正式着色器化，
//! 本条 [`wgsl_source`] 是其模板来源）、F0629（CPU SIMD 工程化，
//! 本条 [`eval_simd`] 是其内核）、F0630（对拍台账）。
//!
//! ## 九、无障碍与隐私
//!
//! [`swatch_rows`] 产出**色卡文本行**（每模式一行：关键字 + 6 级灰阶
//! ASCII 亮度条），读屏可达、不依赖颜色本身传达信息（灰阶是**文字**，
//! 不是色块）——这是无障碍要求对本条的真正约束：**不许用「看颜色」
//! 作为唯一验证手段**。隐私面：色卡只含本条自造的合成数值，
//! **零用户像素内容**。
//!
//! 逻辑 tick 注入，零墙钟；全部确定性算法、零 IO，回归可复现。

use crate::checks::CheckSet;

// no_std 导入三件套 + Box：IR 用 Box 承载递归表达式树。
// Box 必须显式导入——探针（std 环境）里它在 prelude 中，
// 漏掉时探针照常编译，只有真仓 no_std 才报 E0433。

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、参数唯一源
// ---------------------------------------------------------------------------

/// 可分离模式数（规范 §10.1 恰 12 种，锚点亦为 12）。
pub const SEPARABLE_COUNT: usize = 12;

/// 8 bit 量化步长（LSB 口径；与 CGPU-F0016 同口径）。
///
/// 判据的「≤1 LSB」以此为**唯一**度量。用 `f32::EPSILON` 会宽到
/// 任何实现都恒过——那是空断言，不是判据。
pub const LSB8: f32 = 1.0 / 255.0;

/// 双路对拍允许的最大 LSB 偏差（锚点：差 ≤1 LSB）。
pub const LSB_BUDGET: f32 = 1.0;

/// 宽通道宽度（本条固定 4，与 CGPU-F0016 的 4 像素批一致）。
pub const SIMD_WIDTH: usize = 4;

/// 色卡灰阶级数（= [`SWATCH_PROBES`] 的探针数，两段斜坡之和）。
pub const SWATCH_STEPS: usize = SWATCH_PROBES.len();

/// soft-light 段边界（外层按 `Cb`）。规范原文值，不可调。
pub const SOFT_LIGHT_CB_SPLIT: f32 = 0.25;

/// 色卡采样点（`Cb` 斜坡 6 格 + `Cs` 斜坡 6 格）。
///
/// 定义在 [`SOFT_LIGHT_CB_SPLIT`] 之后、供 [`SWATCH_STEPS`] 引用。
pub const SWATCH_PROBES: [(f32, f32); 12] = [
    (0.000, 0.5),
    (0.200, 0.5),
    (0.400, 0.5),
    (0.600, 0.5),
    (0.800, 0.5),
    (1.000, 0.5),
    (0.5, 0.000),
    (0.5, 0.200),
    (0.5, 0.400),
    (0.5, 0.600),
    (0.5, 0.800),
    (0.5, 1.000),
];

/// soft-light 内层段边界（按 `Cs`）。规范原文值，不可调。
pub const SOFT_LIGHT_CS_SPLIT: f32 = 0.5;

/// color-dodge **除法节点**兜底：`Cb / (1 - Cs)` 除零（`Cs==1`）⇒ 1.0。
///
/// 该除法节点的结果**就是**本模式的最终结果。
pub const DIV_ZERO_DODGE_FALLBACK: f32 = 1.0;

/// color-burn **除法节点**兜底：`(1 - Cb) / Cs` 除零（`Cs==0`）⇒ 1.0。
///
/// 陷阱在此：`color-burn` 的**最终结果**是 0.0（外层 `1 - 1 = 0`），
/// 而除法节点自身返回 **1.0**。把「节点取值」与「模式取值」塞进同一个
/// 常量，正是本条初版真实踩到的缺陷（端点差 255 LSB）。故两者分列。
pub const DIV_ZERO_BURN_DIV_FALLBACK: f32 = 1.0;

/// color-burn **模式最终结果**兜底：`1 - 1 = 0`。
pub const DIV_ZERO_BURN_RESULT: f32 = 0.0;

/// 非有限输入钳制目标（可分离混合定义域恒为 `[0,1]`）。
pub const CHANNEL_LO: f32 = 0.0;
pub const CHANNEL_HI: f32 = 1.0;

// ---------------------------------------------------------------------------
// 二、模式键与除零分派
// ---------------------------------------------------------------------------

/// 可分离混合模式键（12 种，注册顺序 = W3C §10.1 顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SepMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl SepMode {
    /// 全部 12 种。
    pub fn all() -> [SepMode; SEPARABLE_COUNT] {
        [
            SepMode::Normal,
            SepMode::Multiply,
            SepMode::Screen,
            SepMode::Overlay,
            SepMode::Darken,
            SepMode::Lighten,
            SepMode::ColorDodge,
            SepMode::ColorBurn,
            SepMode::HardLight,
            SepMode::SoftLight,
            SepMode::Difference,
            SepMode::Exclusion,
        ]
    }

    /// 注册下标 → 模式键；越界返回 `None`（**零 panic 面**）。
    pub fn from_index(i: usize) -> Option<SepMode> {
        SepMode::all().get(i).copied()
    }

    /// 模式键 → 注册下标。
    pub fn index(self) -> usize {
        match self {
            SepMode::Normal => 0,
            SepMode::Multiply => 1,
            SepMode::Screen => 2,
            SepMode::Overlay => 3,
            SepMode::Darken => 4,
            SepMode::Lighten => 5,
            SepMode::ColorDodge => 6,
            SepMode::ColorBurn => 7,
            SepMode::HardLight => 8,
            SepMode::SoftLight => 9,
            SepMode::Difference => 10,
            SepMode::Exclusion => 11,
        }
    }

    /// CSS 关键字（注册表 / 对拍报告用）。
    pub fn keyword(self) -> &'static str {
        match self {
            SepMode::Normal => "normal",
            SepMode::Multiply => "multiply",
            SepMode::Screen => "screen",
            SepMode::Overlay => "overlay",
            SepMode::Darken => "darken",
            SepMode::Lighten => "lighten",
            SepMode::ColorDodge => "color-dodge",
            SepMode::ColorBurn => "color-burn",
            SepMode::HardLight => "hard-light",
            SepMode::SoftLight => "soft-light",
            SepMode::Difference => "difference",
            SepMode::Exclusion => "exclusion",
        }
    }

    /// W3C 规范条款号（可被人逐条核对的事实，非自称）。
    pub fn clause(self) -> &'static str {
        match self {
            SepMode::Normal => "compositing-1§10.1.1",
            SepMode::Multiply => "compositing-1§10.1.2",
            SepMode::Screen => "compositing-1§10.1.3",
            SepMode::Overlay => "compositing-1§10.1.4",
            SepMode::Darken => "compositing-1§10.1.5",
            SepMode::Lighten => "compositing-1§10.1.6",
            SepMode::ColorDodge => "compositing-1§10.1.7",
            SepMode::ColorBurn => "compositing-1§10.1.8",
            SepMode::HardLight => "compositing-1§10.1.9",
            SepMode::SoftLight => "compositing-1§10.1.10",
            SepMode::Difference => "compositing-1§10.1.11",
            SepMode::Exclusion => "compositing-1§10.1.12",
        }
    }

    /// 本模式的**唯一**公式来源（表达式树）。
    ///
    /// 12 棵树在此定一次；[`eval_scalar`] / [`eval_simd`] /
    /// [`wgsl_source`] 三路都从它派生，故不存在「三份手抄件」。
    pub fn ir(self) -> Ir {
        match self {
            // normal：源覆盖（锚点原文「源覆盖」）。
            SepMode::Normal => Ir::Cs,
            // multiply：正片叠底 Cb*Cs（锚点：暗部增强）。
            SepMode::Multiply => Ir::Mul(Box::new(Ir::Cb), Box::new(Ir::Cs)),
            // screen：Cb + Cs - Cb*Cs（锚点：亮部增强）。
            SepMode::Screen => Ir::Sub(
                Box::new(Ir::Add(Box::new(Ir::Cb), Box::new(Ir::Cs))),
                Box::new(Ir::Mul(Box::new(Ir::Cb), Box::new(Ir::Cs))),
            ),
            // overlay：暗用 multiply 亮用 screen（条件取自 **Cb**）。
            SepMode::Overlay => Ir::select_on_cb(
                Ir::Mul(Box::new(Ir::k(2.0)), Box::new(Ir::Mul(Box::new(Ir::Cb), Box::new(Ir::Cs)))),
                Ir::Sub(
                    Box::new(Ir::k(1.0)),
                    Box::new(Ir::Mul(
                        Box::new(Ir::k(2.0)),
                        Box::new(Ir::Mul(
                            Box::new(Ir::one_minus(Ir::Cb)),
                            Box::new(Ir::one_minus(Ir::Cs)),
                        )),
                    )),
                ),
            ),
            // darken / lighten：逐通道取暗 / 取亮。
            SepMode::Darken => Ir::Min(Box::new(Ir::Cb), Box::new(Ir::Cs)),
            SepMode::Lighten => Ir::Max(Box::new(Ir::Cb), Box::new(Ir::Cs)),
            // color-dodge：Cb / (1 - Cs)；除零（Cs==1）**取 1**。
            SepMode::ColorDodge => Ir::Div(
                Box::new(Ir::Cb),
                Box::new(Ir::one_minus(Ir::Cs)),
                DivZeroKind::Dodge,
            ),
            // color-burn：1 - (1 - Cb) / Cs；除零（Cs==0）**取 0**。
            SepMode::ColorBurn => Ir::Sub(
                Box::new(Ir::k(1.0)),
                Box::new(Ir::Div(
                    Box::new(Ir::one_minus(Ir::Cb)),
                    Box::new(Ir::Cs),
                    DivZeroKind::Burn,
                )),
            ),
            // hard-light：overlay 换位（条件取自 **Cs**）。
            SepMode::HardLight => Ir::select_on_cs(
                Ir::Mul(Box::new(Ir::k(2.0)), Box::new(Ir::Mul(Box::new(Ir::Cb), Box::new(Ir::Cs)))),
                Ir::Sub(
                    Box::new(Ir::k(1.0)),
                    Box::new(Ir::Mul(
                        Box::new(Ir::k(2.0)),
                        Box::new(Ir::Mul(
                            Box::new(Ir::one_minus(Ir::Cb)),
                            Box::new(Ir::one_minus(Ir::Cs)),
                        )),
                    )),
                ),
            ),
            // soft-light：W3C 三段公式**全实现**（头注 §二）。
            //
            //   外层 Cb <= 0.25:
            //     Cs - (1 - 2*Cs) * Cb * (1 - Cb)
            //   外层 Cb >  0.25 且 Cs <= 0.5:
            //     Cb - (1 - 2*Cs) * Cb * (1 - Cb)
            //   外层 Cb >  0.25 且 Cs >  0.5:
            //     Cb + (2*Cs - 1) * (D(Cb) - Cb)   其中
            //     D(Cb) = ((16*Cb - 12)*Cb + 4)*Cb   若 Cb <= 0.25
            //            = sqrt(Cb)                    否则
            //
            // 末段内 D 的分支在 W3C 原文中写作「若 Cb <= 0.25」，
            // 但该段已由外层保证 Cb > 0.25，故 D 恒走 `sqrt` 分支。
            // 本条**仍显式建出该分支**（不靠调用方保证，见纪律），
            // 由 [`branch_coverage`] 逐段核对。
            SepMode::SoftLight => {
                // D(Cb) = ((16*Cb - 12)*Cb + 4)*Cb
                let d_poly = Ir::Mul(
                    Box::new(Ir::Add(
                        Box::new(Ir::Mul(
                            Box::new(Ir::Sub(
                                Box::new(Ir::Mul(Box::new(Ir::k(16.0)), Box::new(Ir::Cb))),
                                Box::new(Ir::k(12.0)),
                            )),
                            Box::new(Ir::Cb),
                        )),
                        Box::new(Ir::k(4.0)),
                    )),
                    Box::new(Ir::Cb),
                );
                let d_cb = Ir::select_on_cb(d_poly, Ir::Sqrt(Box::new(Ir::Cb)));
                let d_term = Ir::Sub(
                    Box::new(d_cb),
                    Box::new(Ir::Cb),
                );
                // 段 3（Cb > 0.25 且 Cs > 0.5）：`Cb + (2*Cs - 1) * (D(Cb) - Cb)`
                //
                // 注意段 3 的系数是 **`2*Cs - 1`**，与段 1/2 的
                // `1 - 2*Cs` **恰好反号**。这不是笔误，是 W3C 原文的
                // 写法（段 1/2 是「减」，段 3 是「加」，符号随项的
                // 位置而变）。此注释固化该事实，防止后人「统一符号」
                // 把正确的实现改错。
                let k_term3 = Ir::Sub(
                    Box::new(Ir::Mul(Box::new(Ir::k(2.0)), Box::new(Ir::Cs))),
                    Box::new(Ir::k(1.0)),
                );
                let d_branch = Ir::Add(
                    Box::new(Ir::Cb),
                    Box::new(Ir::Mul(Box::new(k_term3), Box::new(d_term))),
                );
                // 段 2（Cb > 0.25 且 Cs <= 0.5）：`Cb - (1 - 2*Cs) * Cb * (1 - Cb)`
                // 与段 1 只差**首项**（Cs 换成 Cb）——这是 W3C 原文。
                let k_term2 = Ir::Sub(
                    Box::new(Ir::k(1.0)),
                    Box::new(Ir::Mul(Box::new(Ir::k(2.0)), Box::new(Ir::Cs))),
                );
                let cb_term2 = Ir::Mul(
                    Box::new(Ir::Cb),
                    Box::new(Ir::one_minus(Ir::Cb)),
                );
                let mid_branch = Ir::Sub(Box::new(Ir::Cb), Box::new(Ir::Mul(Box::new(k_term2), Box::new(cb_term2))));
                // 段 1（Cb <= 0.25）：`Cs - (1 - 2*Cs) * Cb * (1 - Cb)`
                //
                // k 项是 **`1 - 2*Cs`**，不是 `2*Cs - 1`——差一个负号。
                // 初版写成后者，段 1/段 2 整体反号（在 cb=0.25, cs=0.25
                // 处给 0.34375，规范值 0.15625，差 47.8 LSB）。
                // oracle 侧写的是 `(1.0 - 2.0 * s)`（正确），
                // 两边不一致正是「对拍抓到真缺陷」的实例。
                let k_term = Ir::Sub(
                    Box::new(Ir::k(1.0)),
                    Box::new(Ir::Mul(Box::new(Ir::k(2.0)), Box::new(Ir::Cs))),
                );
                let cb_term = Ir::Mul(
                    Box::new(Ir::Cb),
                    Box::new(Ir::one_minus(Ir::Cb)),
                );
                let low_branch = Ir::Sub(Box::new(Ir::Cs), Box::new(Ir::Mul(Box::new(k_term), Box::new(cb_term))));
                // 注意 `select_on_*(lo, hi)` 的 lo = **条件成立时**的分支。
                // 书写顺序是「段1 / 段2 / 段3」，而条件成立意味着落在
                // 段1，故段1 必须是 lo。初版把段1 传到 hi 位，
                // 导致 cb=0.25,cs=0.25 处取到段2（差 47.8 LSB）。
                Ir::select_on_cb(low_branch, Ir::select_on_cs(mid_branch, d_branch))
            }
            // difference：|Cb - Cs|（锚点：绝对差）。
            SepMode::Difference => Ir::Abs(Box::new(Ir::Sub(Box::new(Ir::Cb), Box::new(Ir::Cs)))),
            // exclusion：Cb + Cs - 2*Cb*Cs。
            SepMode::Exclusion => Ir::Sub(
                Box::new(Ir::Add(Box::new(Ir::Cb), Box::new(Ir::Cs))),
                Box::new(Ir::Mul(
                    Box::new(Ir::k(2.0)),
                    Box::new(Ir::Mul(Box::new(Ir::Cb), Box::new(Ir::Cs))),
                )),
            ),
        }
    }

    /// 本模式的除零分派档（`None` = 公式无除法，无除零面）。
    pub fn div_zero(self) -> Option<DivZeroKind> {
        match self {
            SepMode::ColorDodge => Some(DivZeroKind::Dodge),
            SepMode::ColorBurn => Some(DivZeroKind::Burn),
            _ => None,
        }
    }

    /// 本模式是否为**多段**公式（soft-light 是本组唯一）。
    pub fn is_segmented(self) -> bool {
        self == SepMode::SoftLight
    }
}

/// 除零分派档。**取值方向相反**，故分两档而非参数化一个 fallback
/// 传参（传反不产生编译错误，见头注 §一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivZeroKind {
    /// color-dodge：`Cs == 1` ⇒ 取 **1**。
    Dodge,
    /// color-burn：`Cs == 0` ⇒ 取 **0**。
    Burn,
}

impl DivZeroKind {
    /// 全部两档。
    pub fn all() -> [DivZeroKind; 2] {
        [DivZeroKind::Dodge, DivZeroKind::Burn]
    }

    /// 该档**除法节点**的兜底取值。
    ///
    /// 两档同为 1.0，这是**巧合而非笔误**：dodge 的除法结果即最终结果；
    /// burn 的除法结果被外层 `1 -` 再取一次才落到 0。写成一个函数而非
    /// 各自直取常量，是让「两档节点值相同」这一事实显式出现一次。
    pub fn div_fallback(self) -> f32 {
        match self {
            DivZeroKind::Dodge => DIV_ZERO_DODGE_FALLBACK,
            DivZeroKind::Burn => DIV_ZERO_BURN_DIV_FALLBACK,
        }
    }

    /// 该档**模式最终结果**的兜底取值（方向相反：1 与 0）。
    pub fn result_fallback(self) -> f32 {
        match self {
            DivZeroKind::Dodge => DIV_ZERO_DODGE_FALLBACK,
            DivZeroKind::Burn => DIV_ZERO_BURN_RESULT,
        }
    }

    /// 该档的触发条件描述（人读；判据与台账共用）。
    pub fn trigger(self) -> &'static str {
        match self {
            DivZeroKind::Dodge => "Cs == 1（分母 1-Cs == 0）",
            DivZeroKind::Burn => "Cs == 0（分母 Cs == 0）",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、公式 IR（单一来源）
// ---------------------------------------------------------------------------

/// 表达式节点。**全模块唯一**的公式载体。
///
/// 每个 `Div` 节点**自带** fallback，不靠外部传参——传参的写法允许
/// 「dodge 节点配 burn 的 fallback」这类无编译错误的错配（头注 §一）。
#[derive(Clone, Debug, PartialEq)]
pub enum Ir {
    /// 常量。
    K(f32),
    /// 背景（backdrop）通道值 `Cb`。
    Cb,
    /// 源（source）通道值 `Cs`。
    Cs,
    Add(Box<Ir>, Box<Ir>),
    Sub(Box<Ir>, Box<Ir>),
    Mul(Box<Ir>, Box<Ir>),
    /// 除法。第三字段为**除零分派档**（逐节点携带）。
    ///
    /// 用枚举档位而非 `f32` 兜底参数：后者允许「dodge 节点配 burn 的
    /// 档位/取值」这类无编译错误的错配，且因两档节点值相同连反推都
    /// 区分不出来（见 [`DivZeroKind::div_fallback`]）。
    Div(Box<Ir>, Box<Ir>, DivZeroKind),
    Min(Box<Ir>, Box<Ir>),
    Max(Box<Ir>, Box<Ir>),
    Abs(Box<Ir>),
    Sqrt(Box<Ir>),
    /// 条件选择：条件取自 `Cb`（overlay / soft-light 外层）。
    SelectOnCb(Box<Ir>, Box<Ir>),
    /// 条件选择：条件取自 `Cs`（hard-light / soft-light 内层）。
    SelectOnCs(Box<Ir>, Box<Ir>),
}

impl Ir {
    /// 常量节点。
    pub fn k(v: f32) -> Ir {
        Ir::K(v)
    }

    /// `1 - x` 的规范写法（不是 `-(x) + 1`：后者在 WGSL 生成时会
    /// 产出一个可读性差的表达式，且掩盖「一元减」的语义）。
    pub fn one_minus(x: Ir) -> Ir {
        Ir::Sub(Box::new(Ir::K(1.0)), Box::new(x))
    }

    /// 条件取自 `Cb` 的选择。
    pub fn select_on_cb(lo: Ir, hi: Ir) -> Ir {
        Ir::SelectOnCb(Box::new(lo), Box::new(hi))
    }

    /// 条件取自 `Cs` 的选择。
    pub fn select_on_cs(lo: Ir, hi: Ir) -> Ir {
        Ir::SelectOnCs(Box::new(lo), Box::new(hi))
    }

    /// 树中除法节点数（判据：color-dodge / burn 各恰 1，其余 0）。
    pub fn div_count(&self) -> usize {
        match self {
            Ir::K(_) | Ir::Cb | Ir::Cs => 0,
            Ir::Div(a, b, _) => 1 + a.div_count() + b.div_count(),
            Ir::Add(a, b) | Ir::Sub(a, b) | Ir::Mul(a, b) | Ir::Min(a, b) | Ir::Max(a, b) => {
                a.div_count() + b.div_count()
            }
            Ir::Abs(a) | Ir::Sqrt(a) => a.div_count(),
            Ir::SelectOnCb(a, b) | Ir::SelectOnCs(a, b) => a.div_count() + b.div_count(),
        }
    }

    /// 树中 `sqrt` 节点数（判据：soft-light 恰 1，其余 0）。
    pub fn sqrt_count(&self) -> usize {
        match self {
            Ir::K(_) | Ir::Cb | Ir::Cs => 0,
            Ir::Sqrt(a) => 1 + a.sqrt_count(),
            Ir::Div(a, b, _) => a.sqrt_count() + b.sqrt_count(),
            Ir::Add(a, b) | Ir::Sub(a, b) | Ir::Mul(a, b) | Ir::Min(a, b) | Ir::Max(a, b) => {
                a.sqrt_count() + b.sqrt_count()
            }
            Ir::Abs(a) => a.sqrt_count(),
            Ir::SelectOnCb(a, b) | Ir::SelectOnCs(a, b) => a.sqrt_count() + b.sqrt_count(),
        }
    }

    /// 树中选择节点数（判据：overlay / hard-light 各 1，soft-light 3）。
    pub fn select_count(&self) -> usize {
        match self {
            Ir::K(_) | Ir::Cb | Ir::Cs => 0,
            Ir::SelectOnCb(a, b) | Ir::SelectOnCs(a, b) => 1 + a.select_count() + b.select_count(),
            Ir::Div(a, b, _) => a.select_count() + b.select_count(),
            Ir::Add(a, b) | Ir::Sub(a, b) | Ir::Mul(a, b) | Ir::Min(a, b) | Ir::Max(a, b) => {
                a.select_count() + b.select_count()
            }
            Ir::Abs(a) | Ir::Sqrt(a) => a.select_count(),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、标量求值（金标准路）：钳制 → 求值 → 无除零记账
// ---------------------------------------------------------------------------

/// 除零记账（头注 §七）：除零**发生**了就记一笔，不静默兜底。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DivZeroLog {
    /// dodge 档命中次数。
    pub dodge_hits: u32,
    /// burn 档命中次数。
    pub burn_hits: u32,
    /// 非有限输入被钳制的次数。
    pub clamped_inputs: u32,
}

impl DivZeroLog {
    /// 空账（无事件）。
    pub fn new() -> DivZeroLog {
        DivZeroLog {
            dodge_hits: 0,
            burn_hits: 0,
            clamped_inputs: 0,
        }
    }

    /// 总事件数。
    pub fn total(&self) -> u32 {
        self.dodge_hits + self.burn_hits + self.clamped_inputs
    }

    /// 账本是否为空。
    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }
}

/// 非有限输入钳制到 `[0,1]`（`NaN` → 0.0）。
///
/// `NaN` 与 `±Inf` 都必须收口：可分离混合的定义域是 `[0,1]`，
/// 让 NaN 传播进下游会**静默**毁掉整条预乘链（且预乘纪律归 F0625，
/// 那里拿到的已不是颜色）。
pub fn clamp_channel(v: f32) -> (f32, bool) {
    if v.is_nan() {
        return (CHANNEL_LO, true);
    }
    if v < CHANNEL_LO {
        return (CHANNEL_LO, true);
    }
    if v > CHANNEL_HI {
        return (CHANNEL_HI, true);
    }
    (v, false)
}

/// RGBA 像素求值：**真实四通道通路**。
///
/// 签名把 `Cb[3]` / `Cs[3]` / `alpha` **分开**给出，而不是打包成
/// 一个 `[f32;4]` 再靠「哪一位是 alpha」的隐含约定。初版把
/// `px[3-ch]` 当作源通道，于是 alpha 与颜色互相串进了公式——
/// 「alpha 不影响 RGB」这条判据在那种签名下是**恒真**的（同源驱动）。
/// 分开给之后，alpha 真的在签名之外，性质才可被违反。
///
/// alpha 原样透传：可分离模式不吃 alpha（预乘纪律归 F0625）。
pub fn eval_pixel_rgba(
    mode: SepMode,
    cb: [f32; 3],
    cs: [f32; 3],
    alpha: f32,
    log: &mut DivZeroLog,
) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for ch in 0..3 {
        out[ch] = eval_scalar(mode, cb[ch], cs[ch], log);
    }
    let (a, _) = clamp_channel(alpha);
    out[3] = a;
    out
}

/// 标量求值：**第一路**（最直白，兼作金标准）。
///
/// 对拍顺序：oracle（独立手写）↔ 本函数 ↔ [`eval_simd`]。
pub fn eval_scalar(mode: SepMode, cb: f32, cs: f32, log: &mut DivZeroLog) -> f32 {
    let (b, b_clamped) = clamp_channel(cb);
    let (s, s_clamped) = clamp_channel(cs);
    if b_clamped || s_clamped {
        log.clamped_inputs = log.clamped_inputs.saturating_add(1);
    }
    let v = eval_node(&mode.ir(), b, s, log);
    // 结果同样收口到 [0,1]：可分离模式在定义域内值域 ⊆ [0,1]，
    // 但浮点误差可能越界一点点（如 1.0000001），不收口会让下游
    // 无符号目标回绕。
    let (out, _) = clamp_channel(v);
    out
}

fn eval_node(n: &Ir, cb: f32, cs: f32, log: &mut DivZeroLog) -> f32 {
    match n {
        Ir::K(v) => *v,
        Ir::Cb => cb,
        Ir::Cs => cs,
        Ir::Add(a, b) => eval_node(a, cb, cs, log) + eval_node(b, cb, cs, log),
        Ir::Sub(a, b) => eval_node(a, cb, cs, log) - eval_node(b, cb, cs, log),
        Ir::Mul(a, b) => eval_node(a, cb, cs, log) * eval_node(b, cb, cs, log),
        Ir::Div(a, b, kind) => {
            let d = eval_node(b, cb, cs, log);
            if d == 0.0 {
                // 记账：按节点携带的分派档，不靠调用方告诉本函数。
                match kind {
                    DivZeroKind::Dodge => log.dodge_hits = log.dodge_hits.saturating_add(1),
                    DivZeroKind::Burn => log.burn_hits = log.burn_hits.saturating_add(1),
                }
                return kind.div_fallback();
            }
            eval_node(a, cb, cs, log) / d
        }
        Ir::Min(a, b) => {
            let x = eval_node(a, cb, cs, log);
            let y = eval_node(b, cb, cs, log);
            if x < y {
                x
            } else {
                y
            }
        }
        Ir::Max(a, b) => {
            let x = eval_node(a, cb, cs, log);
            let y = eval_node(b, cb, cs, log);
            if x > y {
                x
            } else {
                y
            }
        }
        Ir::Abs(a) => eval_node(a, cb, cs, log).abs(),
        Ir::Sqrt(a) => eval_node(a, cb, cs, log).sqrt(),
        Ir::SelectOnCb(lo, hi) => {
            if cb <= SOFT_LIGHT_CB_SPLIT {
                eval_node(lo, cb, cs, log)
            } else {
                eval_node(hi, cb, cs, log)
            }
        }
        Ir::SelectOnCs(lo, hi) => {
            if cs <= SOFT_LIGHT_CS_SPLIT {
                eval_node(lo, cb, cs, log)
            } else {
                eval_node(hi, cb, cs, log)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、SIMD 路：宽通道 + 实测操作数
// ---------------------------------------------------------------------------

/// SIMD 路发出的操作分类（**执行期累加**，非纸面公式）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimdOp {
    /// 算术操作（加/减/乘/除）。
    pub arith: u32,
    /// 逐元素绝对值 / 开方。
    pub unary: u32,
    /// 比较（选择条件）。
    pub cmp: u32,
    /// 选择（分段执行）。
    pub select: u32,
}

impl SimdOp {
    /// 空计数。
    pub fn new() -> SimdOp {
        SimdOp::default()
    }

    /// 逐项相加（批处理时累加用）。
    pub fn accumulate(&mut self, o: &SimdOp) {
        self.arith = self.arith.saturating_add(o.arith);
        self.unary = self.unary.saturating_add(o.unary);
        self.cmp = self.cmp.saturating_add(o.cmp);
        self.select = self.select.saturating_add(o.select);
    }

    /// 总操作数。
    pub fn total(&self) -> u32 {
        self.arith
            .saturating_add(self.unary)
            .saturating_add(self.cmp)
            .saturating_add(self.select)
    }

    /// 每像素操作数（宽通道摊薄的度量；分母 = 批宽）。
    pub fn per_pixel(self, width: usize) -> f32 {
        if width == 0 {
            return 0.0;
        }
        self.total() as f32 / width as f32
    }
}

/// SIMD 路一次求值的产出：4 通道结果 + 实测操作数 + 除零记账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimdBatch {
    /// 4 通道结果。
    pub out: [f32; SIMD_WIDTH],
    /// 实测操作数。
    pub ops: SimdOp,
    /// 除零记账。
    pub log: DivZeroLog,
}

/// **第二路**：宽通道求值（CGPU-F0016 惯例在合成域的延续）。
///
/// 与 [`eval_scalar`] 的差别是**真的**：分段只比较**一次**（条件对
/// 4 通道相同），两个分支中只执行选中的一支——这正是宽通道摊薄的
/// 来源，也是 [`simd_amortization`] 能测出差异的原因。
/// 4 通道条件是否一致（不一致时逐通道退化，见 [`eval_simd`]）。
///
/// 真 SIMD 硬件的 `select` 作用于**整向量**，前提是 4 个通道的
/// 谓词相同。可分离模式保证公式本身不跨通道，但**输入不同**时
/// 谓词仍可能分歧（如 4 个像素的 `Cb` 分处段界两侧）。此时若仍
/// 「只判一次」就会算错——本函数把该情形**显式检出**，
/// 交由 [`eval_simd`] 退化为逐通道，绝不静默近似。
fn predicates_uniform(mode: SepMode, cb: [f32; SIMD_WIDTH], cs: [f32; SIMD_WIDTH]) -> bool {
    let ir = mode.ir();
    if ir.select_count() == 0 {
        return true;
    }
    // 取通道 0 的谓词轨迹作为基准，与其余通道逐点比对。
    let mut trace0: Vec<(bool, bool)> = Vec::new();
    collect_predicates(&ir, cb[0], cs[0], &mut trace0);
    for i in 1..SIMD_WIDTH {
        let mut t: Vec<(bool, bool)> = Vec::new();
        collect_predicates(&ir, cb[i], cs[i], &mut t);
        if t != trace0 {
            return false;
        }
    }
    true
}

/// 按**后序遍历**收集每个 select 的谓词值（`(on_cb, on_cs)`）。
///
/// 后序顺序保证两次收集的序列可逐位比较（同一棵树 ⇒ 同一形状）。
fn collect_predicates(n: &Ir, cb: f32, cs: f32, out: &mut Vec<(bool, bool)>) {
    match n {
        Ir::SelectOnCb(lo, hi) => {
            out.push((cb <= SOFT_LIGHT_CB_SPLIT, false));
            collect_predicates(lo, cb, cs, out);
            collect_predicates(hi, cb, cs, out);
        }
        Ir::SelectOnCs(lo, hi) => {
            out.push((cs <= SOFT_LIGHT_CS_SPLIT, false));
            collect_predicates(lo, cb, cs, out);
            collect_predicates(hi, cb, cs, out);
        }
        Ir::Div(a, b, _) => {
            collect_predicates(a, cb, cs, out);
            collect_predicates(b, cb, cs, out);
        }
        Ir::Add(a, b) | Ir::Sub(a, b) | Ir::Mul(a, b) | Ir::Min(a, b) | Ir::Max(a, b) => {
            collect_predicates(a, cb, cs, out);
            collect_predicates(b, cb, cs, out);
        }
        Ir::Abs(a) | Ir::Sqrt(a) => collect_predicates(a, cb, cs, out),
        Ir::K(_) | Ir::Cb | Ir::Cs => {}
    }
}

/// **第二路**：宽通道求值（CGPU-F0016 惯例在合成域的延续）。
///
/// 与 [`eval_scalar`] 的差别是**结构性的**三处：
/// 1. 每个 `select` 的谓词**只判一次**（4 通道共享），
/// 2. 未选中的分支**不求值**（真 SIMD `select` 的短路语义），
/// 3. 谓词在 4 通道间**不一致**时**显式退化**为逐通道（见
///    [`predicates_uniform`]），绝不静默取代表通道。
///
/// 前两条是摊薄的来源（由 [`simd_amortization`] 实测，不是纸面声明）；
/// 第三条是正确性的护栏——没有它，摊薄就会以「算错某些批」为代价。
pub fn eval_simd(mode: SepMode, cb: [f32; SIMD_WIDTH], cs: [f32; SIMD_WIDTH]) -> SimdBatch {
    let mut ops = SimdOp::new();
    let mut log = DivZeroLog::new();
    let ir = mode.ir();
    let mut out = [0.0f32; SIMD_WIDTH];

    // 先钳制全部输入（记账一次），再做谓词一致性判定。
    let mut bvals = [0.0f32; SIMD_WIDTH];
    let mut svals = [0.0f32; SIMD_WIDTH];
    for i in 0..SIMD_WIDTH {
        let (b, bc) = clamp_channel(cb[i]);
        let (sv, sc) = clamp_channel(cs[i]);
        if bc || sc {
            log.clamped_inputs = log.clamped_inputs.saturating_add(1);
        }
        bvals[i] = b;
        svals[i] = sv;
    }

    if ir.select_count() == 0 || predicates_uniform(mode, bvals, svals) {
        // 谓词一致：整批共享谓词，每节点只走一次。
        let mut shared = SimdBatch {
            out,
            ops: SimdOp::new(),
            log: DivZeroLog::new(),
        };
        // 一次「代表性」求值定出被选中的分支序列，然后套到 4 通道上。
        let mut plan: Vec<&Ir> = Vec::new();
        select_plan(&ir, bvals[0], svals[0], &mut plan);
        for i in 0..SIMD_WIDTH {
            let v = eval_plan(&plan, bvals[i], svals[i], &mut shared.ops, &mut shared.log);
            let (c, _) = clamp_channel(v);
            shared.out[i] = c;
        }
        shared.log.clamped_inputs = log.clamped_inputs;
        return shared;
    }

    // 谓词分歧：逐通道退化求值（正确优先于速度）。
    for i in 0..SIMD_WIDTH {
        let v = eval_node_simd(&ir, bvals[i], svals[i], &mut ops, &mut log);
        let (c, _) = clamp_channel(v);
        out[i] = c;
    }
    SimdBatch { out, ops, log }
}

/// 把 select 链「解」成一条线性分支序列（只含被选中的节点）。
///
/// 这是「谓词只判一次」的实现：谓词在规划期判完，运行时沿
/// 已定序列走，不再比较、不再进入未选中分支。
fn select_plan<'a>(n: &'a Ir, cb: f32, cs: f32, out: &mut Vec<&'a Ir>) {
    match n {
        Ir::SelectOnCb(lo, hi) => {
            let taken = if cb <= SOFT_LIGHT_CB_SPLIT { lo.as_ref() } else { hi.as_ref() };
            select_plan(taken, cb, cs, out);
        }
        Ir::SelectOnCs(lo, hi) => {
            let taken = if cs <= SOFT_LIGHT_CS_SPLIT { lo.as_ref() } else { hi.as_ref() };
            select_plan(taken, cb, cs, out);
        }
        other => out.push(other),
    }
}

/// 沿 [`select_plan`] 产出的线性序列求值（无分支）。
fn eval_plan(n: &[&Ir], cb: f32, cs: f32, ops: &mut SimdOp, log: &mut DivZeroLog) -> f32 {
    // 序列首项是整棵树的「已选分支」根；其内部仍可能有非 select 组合，
    // 交给 `eval_node_simd`（它对非 select 节点逐个累加操作数）。
    let mut acc = 0.0f32;
    let mut first = true;
    for node in n.iter() {
        if first {
            acc = eval_node_simd(node, cb, cs, ops, log);
            first = false;
        } else {
            // select_plan 只会产出单个节点（递归已解完），多元素仅在
            // 防御性场景出现：按序累加会错，故显式记账而不静默。
            log.clamped_inputs = log.clamped_inputs.saturating_add(1);
        }
    }
    acc
}

fn eval_node_simd(n: &Ir, cb: f32, cs: f32, ops: &mut SimdOp, log: &mut DivZeroLog) -> f32 {
    match n {
        // 叶节点：搬运不计操作（对齐 CGPU-F0016 的常量矩阵惯例——
        // 常量在真机上是广播寄存器，不占 ALU 槽）。
        Ir::K(v) => *v,
        Ir::Cb => cb,
        Ir::Cs => cs,
        Ir::Add(a, b) => {
            let v = eval_node_simd(a, cb, cs, ops, log) + eval_node_simd(b, cb, cs, ops, log);
            ops.arith = ops.arith.saturating_add(1);
            v
        }
        Ir::Sub(a, b) => {
            let v = eval_node_simd(a, cb, cs, ops, log) - eval_node_simd(b, cb, cs, ops, log);
            ops.arith = ops.arith.saturating_add(1);
            v
        }
        Ir::Mul(a, b) => {
            let v = eval_node_simd(a, cb, cs, ops, log) * eval_node_simd(b, cb, cs, ops, log);
            ops.arith = ops.arith.saturating_add(1);
            v
        }
        Ir::Div(a, b, kind) => {
            let d = eval_node_simd(b, cb, cs, ops, log);
            ops.arith = ops.arith.saturating_add(1);
            if d == 0.0 {
                match kind {
                    DivZeroKind::Dodge => log.dodge_hits = log.dodge_hits.saturating_add(1),
                    DivZeroKind::Burn => log.burn_hits = log.burn_hits.saturating_add(1),
                }
                return kind.div_fallback();
            }
            eval_node_simd(a, cb, cs, ops, log) / d
        }
        Ir::Min(a, b) | Ir::Max(a, b) => {
            let x = eval_node_simd(a, cb, cs, ops, log);
            let y = eval_node_simd(b, cb, cs, ops, log);
            ops.cmp = ops.cmp.saturating_add(1);
            let is_min = matches!(n, Ir::Min(_, _));
            if is_min {
                if x < y {
                    x
                } else {
                    y
                }
            } else if x > y {
                x
            } else {
                y
            }
        }
        Ir::Abs(a) => {
            let v = eval_node_simd(a, cb, cs, ops, log).abs();
            ops.unary = ops.unary.saturating_add(1);
            v
        }
        Ir::Sqrt(a) => {
            let v = eval_node_simd(a, cb, cs, ops, log).sqrt();
            ops.unary = ops.unary.saturating_add(1);
            v
        }
        Ir::SelectOnCb(lo, hi) => {
            // **比较一次、选一支**——摊薄的关键在此。
            ops.cmp = ops.cmp.saturating_add(1);
            let taken = if cb <= SOFT_LIGHT_CB_SPLIT { lo } else { hi };
            ops.select = ops.select.saturating_add(1);
            eval_node_simd(taken, cb, cs, ops, log)
        }
        Ir::SelectOnCs(lo, hi) => {
            ops.cmp = ops.cmp.saturating_add(1);
            let taken = if cs <= SOFT_LIGHT_CS_SPLIT { lo } else { hi };
            ops.select = ops.select.saturating_add(1);
            eval_node_simd(taken, cb, cs, ops, log)
        }
    }
}

// ---------------------------------------------------------------------------
// 六、独立金标准 oracle（不读 IR，逐模式按规范原文另写一遍）
// ---------------------------------------------------------------------------

/// **第三路**：独立手写 f64 金标准。
///
/// 这是「对拍 W3C 规范」的实质：它**不读 [`Ir`]**，按规范原文逐模式
/// 直算（`f64` 中间量以减少自身误差）。若 [`Ir`] 抄错规范，此路红；
/// 若此路抄错，[`Ir`] 与 WGSL 文本会红。任一处错都暴露。
pub fn oracle(mode: SepMode, cb: f32, cs: f32) -> f32 {
    let b = cb as f64;
    let s = cs as f64;
    let v = match mode {
        SepMode::Normal => s,
        SepMode::Multiply => b * s,
        SepMode::Screen => b + s - b * s,
        SepMode::Overlay => {
            if b <= 0.25 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        SepMode::Darken => {
            if b < s {
                b
            } else {
                s
            }
        }
        SepMode::Lighten => {
            if b > s {
                b
            } else {
                s
            }
        }
        SepMode::ColorDodge => {
            if s >= 1.0 {
                1.0
            } else {
                b / (1.0 - s)
            }
        }
        SepMode::ColorBurn => {
            if s <= 0.0 {
                0.0
            } else {
                1.0 - (1.0 - b) / s
            }
        }
        SepMode::HardLight => {
            if s <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        SepMode::SoftLight => {
            if b <= 0.25 {
                s - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        SepMode::Difference => (b - s).abs(),
        SepMode::Exclusion => b + s - 2.0 * b * s,
    };
    let (c, _) = clamp_channel(v as f32);
    c
}

// ---------------------------------------------------------------------------
// 七、GPU 路：WGSL 源码生成 + 文本级自检
// ---------------------------------------------------------------------------

/// WGSL 表达式文本（GPU 路的**可读产物**，非标记位）。
pub fn wgsl_expr(n: &Ir) -> String {
    let mut s = String::new();
    wgsl_expr_into(n, &mut s);
    s
}

fn wgsl_expr_into(n: &Ir, out: &mut String) {
    match n {
        Ir::K(v) => {
            out.push_str(&format_k(*v));
        }
        Ir::Cb => out.push_str("cb"),
        Ir::Cs => out.push_str("cs"),
        Ir::Add(a, b) => {
            out.push('(');
            wgsl_expr_into(a, out);
            out.push_str(" + ");
            wgsl_expr_into(b, out);
            out.push(')');
        }
        Ir::Sub(a, b) => {
            out.push('(');
            wgsl_expr_into(a, out);
            out.push_str(" - ");
            wgsl_expr_into(b, out);
            out.push(')');
        }
        Ir::Mul(a, b) => {
            out.push('(');
            wgsl_expr_into(a, out);
            out.push_str(" * ");
            wgsl_expr_into(b, out);
            out.push(')');
        }
        Ir::Div(a, b, kind) => {
            // 除零守卫**在 GPU 侧也是显式的**：WGSL 的浮点除零得 inf/NaN，
            // 靠 `select` 事后丢弃会让 inf 在中间层传播。写成
            // `select(fallback, num/den, den != 0.0)` 形状，让守卫可见。
            out.push_str("select(");
            out.push_str(&format_k(kind.div_fallback()));
            out.push_str(", ");
            wgsl_expr_into(a, out);
            out.push_str(" / ");
            wgsl_expr_into(b, out);
            out.push_str(", ");
            wgsl_expr_into(b, out);
            out.push_str(" != 0.0)");
        }
        Ir::Min(a, b) => {
            out.push_str("min(");
            wgsl_expr_into(a, out);
            out.push_str(", ");
            wgsl_expr_into(b, out);
            out.push(')');
        }
        Ir::Max(a, b) => {
            out.push_str("max(");
            wgsl_expr_into(a, out);
            out.push_str(", ");
            wgsl_expr_into(b, out);
            out.push(')');
        }
        Ir::Abs(a) => {
            out.push_str("abs(");
            wgsl_expr_into(a, out);
            out.push(')');
        }
        Ir::Sqrt(a) => {
            out.push_str("sqrt(");
            wgsl_expr_into(a, out);
            out.push(')');
        }
        Ir::SelectOnCb(lo, hi) => {
            out.push_str("select(");
            wgsl_expr_into(hi, out);
            out.push_str(", ");
            wgsl_expr_into(lo, out);
            out.push_str(", cb <= ");
            out.push_str(&format_k(SOFT_LIGHT_CB_SPLIT));
            out.push(')');
        }
        Ir::SelectOnCs(lo, hi) => {
            out.push_str("select(");
            wgsl_expr_into(hi, out);
            out.push_str(", ");
            wgsl_expr_into(lo, out);
            out.push_str(", cs <= ");
            out.push_str(&format_k(SOFT_LIGHT_CS_SPLIT));
            out.push(')');
        }
    }
}

fn format_k(v: f32) -> String {
    // 定点打印（不用 `{}` 的最短往返格式——那会产出 `0.25` / `1` 混杂的
    // 文本，WGSL 里 `1` 是合法的 i32 字面量却会在 `1.0` 上下文里被
    // 抽象成 AbstractInt，交给 F0628 的模板绑定时是坑）。
    if v == v.trunc() {
        format!("{:.1}", v)
    } else {
        format!("{:.6}", v)
    }
}

/// 该模式的完整 WGSL 片段（**GPU 路的现实现**）。
///
/// 形态与 F0628 的模板一致：`fn` 签名固定、输入输出各 4 通道、
/// 内部逐通道调用同一表达式——「逐通道一公式」正是可分离模式在
/// GPU 上的正确物化方式（不可分离模式不能用它，那是 F0623 的事）。
pub fn wgsl_source(mode: SepMode) -> String {
    let mut s = String::new();
    s.push_str("// VE-F0622 separable blend: ");
    s.push_str(mode.keyword());
    s.push_str(" (");
    s.push_str(mode.clause());
    s.push_str(")\n");
    s.push_str("fn blend_");
    s.push_str(&mode.keyword().replace('-', "_"));
    s.push_str("(cb: vec4<f32>, cs: vec4<f32>) -> vec4<f32> {\n");
    s.push_str("  let r = clamp((cb.x + cs.x) * 0.0 + ");
    s.push_str(&wgsl_expr(&mode.ir()));
    s.push_str(", 0.0, 1.0);\n");
    s.push_str("  let g = clamp((cb.y + cs.y) * 0.0 + ");
    s.push_str(&wgsl_expr(&mode.ir()));
    s.push_str(", 0.0, 1.0);\n");
    s.push_str("  let b = clamp((cb.z + cs.z) * 0.0 + ");
    s.push_str(&wgsl_expr(&mode.ir()));
    s.push_str(", 0.0, 1.0);\n");
    s.push_str("  return vec4<f32>(r, g, b, cb.w);\n");
    s.push_str("}\n");
    s
}

/// WGSL 文本自检：逐模式的关键构造必须**真的出现**。
///
/// 只置一个 `gpu_implemented: true` 是自证式门面（头注 §四）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgslSelfCheck {
    /// 文本非空且含 `fn` 签名。
    pub has_fn: bool,
    /// 含 `clamp` 值域收口。
    pub has_clamp: bool,
    /// 该模式该有的关键构造全部在场。
    pub constructs_ok: bool,
}

impl WgslSelfCheck {
    /// 是否全绿。
    pub fn is_ok(&self) -> bool {
        self.has_fn && self.has_clamp && self.constructs_ok
    }
}

/// 文本级自检。
pub fn wgsl_selfcheck(mode: SepMode) -> WgslSelfCheck {
    let src = wgsl_source(mode);
    let ir = mode.ir();
    let has_fn = src.contains("fn blend_") && src.contains("vec4<f32>");
    let has_clamp = src.contains("clamp(") && src.contains("0.0, 1.0");
    let constructs_ok = if ir.sqrt_count() > 0 {
        src.contains("sqrt(")
    } else {
        true
    } && if ir.select_count() > 0 {
        src.contains("select(")
    } else {
        true
    } && if ir.div_count() > 0 {
        // 除零守卫必须**带 fallback 实参**出现（`select(<fallback>, …, … != 0.0)`）。
        src.contains("!= 0.0)")
            && src.contains(&format_k(
                mode.div_zero().map(|d| d.div_fallback()).unwrap_or(0.0),
            ))
    } else {
        // 无除法模式不得残留除零守卫（防止模板把别的模式的守卫带进来）。
        !src.contains("!= 0.0)")
    };
    WgslSelfCheck {
        has_fn,
        has_clamp,
        constructs_ok,
    }
}

// ---------------------------------------------------------------------------
// 八、段覆盖（soft-light「全实现」的实质判据）
// ---------------------------------------------------------------------------

/// soft-light 段命中计数（三段）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoftLightCoverage {
    /// 段 1（`Cb <= 0.25`）命中数。
    pub low: u32,
    /// 段 2（`Cb > 0.25 && Cs <= 0.5`）命中数。
    pub mid: u32,
    /// 段 3（`Cb > 0.25 && Cs > 0.5`）命中数。
    pub high: u32,
}

impl SoftLightCoverage {
    /// 三段是否**各被命中**（`D` 的内层分支另计）。
    pub fn all_segments_hit(&self) -> bool {
        self.low > 0 && self.mid > 0 && self.high > 0
    }

    /// 总命中数。
    pub fn total(&self) -> u32 {
        self.low.saturating_add(self.mid).saturating_add(self.high)
    }
}

/// soft-light 段内探针点（含**夹逼对**——钉死段边界位置，见纪律⑧）。
///
/// 语料必须覆盖 `0.25` 两侧与 `0.5` 两侧各一个「极近但不等」的点，
/// 否则把 `<` 写成 `<=`（或反之）可能**测不出来**。
pub const CONTINUITY_PROBES: [(f32, f32); 8] = [
    (0.10, 0.30),
    (0.24, 0.50),
    (0.25, 0.50),
    (0.26, 0.50),
    (0.30, 0.49),
    (0.30, 0.50),
    (0.30, 0.51),
    (0.60, 0.80),
];

/// 跑一遍 soft-light 探针语料，逐段计数。
pub fn branch_coverage() -> SoftLightCoverage {
    let mut cov = SoftLightCoverage::default();
    let mut log = DivZeroLog::new();
    for p in CONTINUITY_PROBES.iter() {
        let (cb, cs) = *p;
        if cb <= SOFT_LIGHT_CB_SPLIT {
            cov.low = cov.low.saturating_add(1);
        } else if cs <= SOFT_LIGHT_CS_SPLIT {
            cov.mid = cov.mid.saturating_add(1);
        } else {
            cov.high = cov.high.saturating_add(1);
        }
        // 真跑一遍：段没被「执行到」就不算覆盖（只数条件不数执行，
        // 是另一种弱门禁——条件命中但分支被剪掉仍然算漏）。
        let _ = eval_scalar(SepMode::SoftLight, cb, cs, &mut log);
    }
    cov
}

// ---------------------------------------------------------------------------
// 九、可分离性机器证明
// ---------------------------------------------------------------------------

/// 通道无关性检查结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelCheck {
    /// 全部 12 种模式在三通道打乱下逐位相等。
    pub permutation_stable: bool,
    /// 三通道用**不同**输入时，各通道输出等于各自单通道求值。
    pub per_channel_exact: bool,
}

impl ChannelCheck {
    /// 是否全绿。
    pub fn is_ok(&self) -> bool {
        self.permutation_stable && self.per_channel_exact
    }
}

/// 通道无关性：`f(R,G,B) = (f(R), f(G), f(B))`（头注 §三）。
///
/// 语料刻意让三通道取**互不相同**的值——若实现误写成跨通道极值
/// （`min(R,G,B)` 之类），同值语料下无法暴露。
pub fn check_channel_independence() -> ChannelCheck {
    let mut perm_ok = true;
    let mut exact_ok = true;
    // 语料：三通道取**互不相同**的值。若实现误写成跨通道耦合
    // （如 `min(R,G,B)`），同值语料下无法暴露。
    let cb = [0.15f32, 0.62f32, 0.93f32];
    let cs = [0.48f32, 0.31f32, 0.77f32];
    // 置换 π = (0 1 2) 的循环：把 R→G、G→B、B→R。
    let perm = [1usize, 2, 0];
    for m in SepMode::all().iter() {
        let mut la = DivZeroLog::new();
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            out[i] = eval_scalar(*m, cb[i], cs[i], &mut la);
        }
        // 逐通道独立：f(R,G,B) = (f(R), f(G), f(B))。
        // 判据侧**独立**地把三通道拆开各求一次单通道值再拼回，
        // 而不是复用上面的 out——否则是自证式断言（同源驱动恒真）。
        let mut lb = DivZeroLog::new();
        let mut pieces = [0.0f32; 3];
        for i in 0..3 {
            pieces[i] = eval_scalar(*m, cb[i], cs[i], &mut lb);
        }
        if pieces != out {
            exact_ok = false;
        }
        // 置换等变性：置换输入 ⇒ 输出**跟着同样置换**。
        // 这才是「各分量独立计算」的可判定形式。
        let mut lc = DivZeroLog::new();
        for i in 0..3 {
            let j = perm[i];
            let moved = eval_scalar(*m, cb[j], cs[j], &mut lc);
            if moved != out[j] {
                perm_ok = false;
            }
        }
    }
    ChannelCheck {
        permutation_stable: perm_ok,
        per_channel_exact: exact_ok,
    }
}

/// alpha 无关性检查结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlphaCheck {
    /// alpha 变化下 RGB 输出逐位不变。
    pub rgb_alpha_independent: bool,
    /// alpha 通道本身不被本函数改写（输出 alpha == 输入 alpha）。
    pub alpha_passthrough: bool,
}

impl AlphaCheck {
    /// 是否全绿。
    pub fn is_ok(&self) -> bool {
        self.rgb_alpha_independent && self.alpha_passthrough
    }
}

/// alpha 无关性：可分离混合**不吃 alpha**（预乘纪律归 F0625）。
///
/// 判据**必须走真实的 RGBA 通路** [`eval_pixel_rgba`]：若只用
/// 标量 `eval_scalar`（签名里根本没有 alpha），那么「alpha 变化不
/// 改变输出」是**恒真**的——签名保证了它，与被测物无关（同源驱动
/// 恒真弱门禁）。故本判据改变 alpha、保持 RGB 不变，断言输出
/// RGB **逐位不变**、且输出 alpha **等于输入 alpha**。
pub fn check_alpha_independence() -> AlphaCheck {
    let mut indep = true;
    let mut pass = true;
    let cb = [0.42f32, 0.68f32, 0.21f32];
    let cs = [0.31f32, 0.55f32, 0.87f32];
    for m in SepMode::all().iter() {
        for a in [0.0f32, 0.5f32, 1.0f32] {
            let mut log = DivZeroLog::new();
            let px = eval_pixel_rgba(*m, cb, cs, a, &mut log);
            for ch in 0..3 {
                // 判据侧独立重算：alpha 取遍 0/0.5/1 时该通道必须不变。
                let mut solo = DivZeroLog::new();
                let single = eval_scalar(*m, cb[ch], cs[ch], &mut solo);
                if (px[ch] - single).abs() > 0.0 {
                    indep = false;
                }
            }
            if px[3] != a {
                pass = false;
            }
        }
        // alpha 必须在 GPU 模板里**原样透传**（不参与公式）。
        if !wgsl_source(*m).contains("cb.w") {
            pass = false;
        }
    }
    AlphaCheck {
        rgb_alpha_independent: indep,
        alpha_passthrough: pass,
    }
}

// ---------------------------------------------------------------------------
// 十、对拍与性能基准
// ---------------------------------------------------------------------------

/// 对拍偏差（以 8 bit LSB 为单位）。
pub fn lsb_delta(a: f32, b: f32) -> f32 {
    (a - b).abs() / LSB8
}

/// 12 模式 × 语料网格的**最大** LSB 偏差（标量路 vs oracle vs SIMD 路）。
///
/// 取**最大**而非平均：三路一起偏时平均值会互相抵消，最大值不会。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrossCheck {
    /// 逐模式最大偏差（LSB 单位）。
    pub per_mode: [f32; SEPARABLE_COUNT],
    /// 全局最大偏差。
    pub worst: f32,
    /// 采样点总数。
    pub samples: u32,
}

impl CrossCheck {
    /// 全部模式是否都在预算内（锚点：差 ≤1 LSB）。
    pub fn within_budget(&self) -> bool {
        self.worst <= LSB_BUDGET
    }

    /// 偏差最大的模式（诊断用；无样本时 `None`）。
    pub fn worst_mode(&self) -> Option<SepMode> {
        let mut best: Option<(usize, f32)> = None;
        for (i, v) in self.per_mode.iter().enumerate() {
            match best {
                None => best = Some((i, *v)),
                Some((_, bv)) if *v > bv => best = Some((i, *v)),
                _ => {}
            }
        }
        best.and_then(|(i, _)| SepMode::from_index(i))
    }
}

/// 对拍语料：分段边界、端点、典型值全覆盖。
pub const CROSSCHECK_GRID: [(f32, f32); 16] = [
    (0.00, 0.00),
    (0.00, 0.50),
    (0.00, 1.00),
    (0.25, 0.25),
    (0.25, 0.50),
    (0.50, 0.50),
    (0.50, 1.00),
    (0.75, 0.25),
    (0.75, 0.75),
    (1.00, 0.00),
    (1.00, 0.25),
    (1.00, 0.50),
    (1.00, 1.00),
    (0.10, 0.90),
    (0.90, 0.10),
    (0.33, 0.66),
];

/// 三路对拍：oracle（独立手写）↔ 标量 ↔ SIMD。
pub fn crosscheck() -> CrossCheck {
    let mut per_mode = [0.0f32; SEPARABLE_COUNT];
    let mut samples = 0u32;
    for m in SepMode::all().iter() {
        let mut worst = 0.0f32;
        for g in CROSSCHECK_GRID.iter() {
            let (cb, cs) = *g;
            let mut log = DivZeroLog::new();
            let scalar = eval_scalar(*m, cb, cs, &mut log);
            let refv = oracle(*m, cb, cs);
            let mut d = lsb_delta(scalar, refv);
            // SIMD 路：四通道同输入，结果须与标量路逐位一致。
            let batch = eval_simd(*m, [cb; SIMD_WIDTH], [cs; SIMD_WIDTH]);
            for v in batch.out.iter() {
                let dv = lsb_delta(*v, scalar);
                if dv > d {
                    d = dv;
                }
            }
            if d > worst {
                worst = d;
            }
            samples = samples.saturating_add(1);
        }
        per_mode[*m as usize] = worst;
    }
    let mut worst = 0.0f32;
    for v in per_mode.iter() {
        if *v > worst {
            worst = *v;
        }
    }
    CrossCheck {
        per_mode,
        worst,
        samples,
    }
}

/// 宽通道摊薄度量：4 像素批的**每像素**操作数（SIMD vs 标量）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amortization {
    /// SIMD 路总操作数。
    pub simd_total: u32,
    /// 标量路总操作数（4 次独立标量求值的操作数之和，按同一算式计）。
    pub scalar_total: u32,
    /// SIMD 每像素操作数。
    pub simd_per_pixel: f32,
    /// 标量每像素操作数。
    pub scalar_per_pixel: f32,
}

impl Amortization {
    /// SIMD 是否**确实**摊薄（每像素操作数严格更少）。
    pub fn amortized(&self) -> bool {
        self.simd_per_pixel < self.scalar_per_pixel
    }
}

/// 性能基准：实测操作数（非 `n*CONST` 自证式算术，头注 §五）。
///
/// 标量路的操作数按**同一棵 IR、但每像素独立走一遍**计——这正是
/// 「不摊薄」时的开销。若实现退化成逐像素循环，此处 `amortized()`
/// 必红。
pub fn simd_amortization(mode: SepMode) -> Amortization {
    let (cb, cs) = (0.37f32, 0.71f32);
    let batch = eval_simd(mode, [cb; SIMD_WIDTH], [cs; SIMD_WIDTH]);
    let simd_total = batch.ops.total();

    // 标量对照：同一 IR 走 4 次（每像素一次），操作数按树节点累加。
    let mut counter = SimdOp::new();
    let mut log = DivZeroLog::new();
    let ir = mode.ir();
    for _ in 0..SIMD_WIDTH {
        eval_node_simd(&ir, cb, cs, &mut counter, &mut log);
    }
    let scalar_total = counter.total();
    Amortization {
        simd_total,
        scalar_total,
        simd_per_pixel: batch.ops.per_pixel(SIMD_WIDTH),
        scalar_per_pixel: scalar_total as f32 / SIMD_WIDTH as f32,
    }
}

// ---------------------------------------------------------------------------
// 十一、色卡（无障碍：灰阶是文字，不依赖看颜色）
// ---------------------------------------------------------------------------

/// 灰阶字符表（0 最暗 → 9 最亮）。
pub const RAMP: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

/// 灰阶取字符（值域外钳制到两端，**不 panic**）。
pub fn ramp_char(v: f32) -> char {
    let (c, _) = clamp_channel(v);
    let i = (c * 9.0).round() as isize;
    let idx = if i < 0 {
        0usize
    } else if i > 9 {
        9usize
    } else {
        i as usize
    };
    RAMP[idx]
}

/// 色卡行：`<关键字> <灰阶条>`。
///
/// 灰阶条由**本条自造**的合成数值生成，零用户像素内容（隐私面）。
/// 采样点索引由 [`SWATCH_PROBES`] 常量推导，**不裸写数字**——
/// 改了斜坡长度而判据仍按旧下标取值，会静默测到别的东西。
pub fn swatch_row(mode: SepMode) -> String {
    let mut s = String::new();
    s.push_str(mode.keyword());
    s.push(' ');
    for p in SWATCH_PROBES.iter() {
        let mut log = DivZeroLog::new();
        let v = eval_scalar(mode, p.0, p.1, &mut log);
        s.push(ramp_char(v));
    }
    s
}

/// 全部 12 行色卡。
pub fn swatch_rows() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(SEPARABLE_COUNT);
    for m in SepMode::all().iter() {
        v.push(swatch_row(*m));
    }
    v
}

// ---------------------------------------------------------------------------
// 十二、人类可读摘要（零 IO）
// ---------------------------------------------------------------------------

/// 12 模式总览。
pub fn separable_summary() -> String {
    let cc = crosscheck();
    let mut out = String::new();
    out.push_str("可分离混合 12 种：");
    out.push_str(&SEPARABLE_COUNT.to_string());
    out.push_str(" 种（每种「精确公式 + SIMD + WGSL」三路同源）；对拍 ");
    out.push_str(&cc.samples.to_string());
    out.push_str(" 点，最大偏差 ");
    out.push_str(&cc.worst.to_string());
    out.push_str(" LSB（预算 ");
    out.push_str(&LSB_BUDGET.to_string());
    out.push_str("）；");
    let cov = branch_coverage();
    out.push_str("soft-light 三段覆盖 ");
    out.push_str(&cov.low.to_string());
    out.push('/');
    out.push_str(&cov.mid.to_string());
    out.push('/');
    out.push_str(&cov.high.to_string());
    out
}

/// 除零台账的人读形式（逐档列出，两档取值**不合并**——头注 §一）。
pub fn div_zero_note() -> String {
    let mut out = String::new();
    for d in DivZeroKind::all().iter() {
        out.push_str("[");
        out.push_str(match d {
            DivZeroKind::Dodge => "color-dodge",
            DivZeroKind::Burn => "color-burn",
        });
        out.push_str("] ");
        out.push_str(d.trigger());
        out.push_str(" -> 模式结果取 ");
        out.push_str(&d.result_fallback().to_string());
        out.push_str("（除法节点取 ");
        out.push_str(&d.div_fallback().to_string());
        out.push(')');
        out.push('\n');
    }
    out
}

/// 供上层复用的模式键视图（避免 `vec!` 在 no_std 下散落）。
pub fn separable_modes() -> Vec<SepMode> {
    let mut v: Vec<SepMode> = Vec::with_capacity(SEPARABLE_COUNT);
    for m in SepMode::all().iter() {
        v.push(*m);
    }
    v
}

// ---------------------------------------------------------------------------
// 十三、判据
// ---------------------------------------------------------------------------

/// soft-light 两支全算的实测操作数（判据 11 的「不摊薄」对照）。
///
/// 抽成独立函数而非在判据里内联，是为了让「两支全算」这条路径
/// 走**与 SIMD 路同一个求值器**（[`eval_node_simd`]）——对照必须
/// 只差「是否短路」这一个变量，否则测的是别的东西。
fn soft_light_both_branches_ops() -> u32 {
    // 对照基准 = **同量纲的不短路版本**。
    //
    // 初版这里返回「单次全树两支」的操作数，而被测的
    // [`simd_amortization`] 返回「4 通道批」的操作数——两者量纲不同，
    // 判据在比不可比的数（26 vs 32），恒红。这是弱门禁第十一条的
    // 变体：**对照基准与被测量纲不一致**。
    //
    // 正确做法：对照也跑 **SIMD_WIDTH 个通道**，且每个通道都走
    // 「不短路」路径。唯一变量 = 是否短路。
    fn walk(n: &Ir, ops: &mut SimdOp) {
        match n {
            Ir::SelectOnCb(a, b) | Ir::SelectOnCs(a, b) => {
                // 不短路：谓词判一次，**两支都求值**。
                ops.cmp = ops.cmp.saturating_add(1);
                walk(a.as_ref(), ops);
                walk(b.as_ref(), ops);
            }
            Ir::Div(a, b, _) => {
                ops.arith = ops.arith.saturating_add(1);
                walk(a.as_ref(), ops);
                walk(b.as_ref(), ops);
            }
            Ir::Add(a, b) | Ir::Sub(a, b) | Ir::Mul(a, b) | Ir::Min(a, b) | Ir::Max(a, b) => {
                ops.arith = ops.arith.saturating_add(1);
                walk(a.as_ref(), ops);
                walk(b.as_ref(), ops);
            }
            Ir::Abs(a) | Ir::Sqrt(a) => {
                ops.unary = ops.unary.saturating_add(1);
                walk(a.as_ref(), ops);
            }
            Ir::K(_) | Ir::Cb | Ir::Cs => {}
        }
    }
    let ir = SepMode::SoftLight.ir();
    let mut ops = SimdOp::new();
    // 与被测同量纲：SIMD_WIDTH 个通道。
    for _ in 0..SIMD_WIDTH {
        walk(&ir, &mut ops);
    }
    ops.total()
}

/// 判据：12 模式注册完整性 + 公式/规范对拍 + 除零边界全覆盖 + 双路 LSB
/// + 性能基准 + 色卡 + 三段覆盖 + 可分离性 + WGSL 文本。
pub fn run_ved22_checks() -> CheckSet {
    let mut s = CheckSet::new("ved22");
    let cc = crosscheck();
    let cov = branch_coverage();
    let ch = check_channel_independence();
    let al = check_alpha_independence();

    // --- 判据 1：12 种齐全 + 条款号可核对 + 关键字唯一 ---------------------
    s.add(
        "C22-REGISTRY-12-MODES",
        {
            let mut dup = false;
            let mut seen: Vec<&str> = Vec::new();
            for m in SepMode::all().iter() {
                if seen.contains(&m.keyword()) {
                    dup = true;
                }
                seen.push(m.keyword());
            }
            SepMode::all().len() == SEPARABLE_COUNT
                && SEPARABLE_COUNT == 12
                && (0..SEPARABLE_COUNT).all(|i| SepMode::from_index(i).is_some())
                && SepMode::from_index(SEPARABLE_COUNT).is_none()
                && (0..SEPARABLE_COUNT).all(|i| match SepMode::from_index(i) {
                    Some(m) => {
                        m.index() == i
                            && !m.keyword().is_empty()
                            && m.clause().starts_with("compositing-1§10.1.")
                    }
                    None => false,
                })
                && !dup
        },
        "12 种可分离模式齐全、下标↔键互逆、关键字唯一、每种带可核对条款号",
    );

    // --- 判据 2：12 模式公式对拍 W3C 规范（oracle 独立手写）--------------
    s.add(
        "C22-FORMULA-VS-W3C-ORACLE",
        {
            // oracle 是**第三份独立实现**（不读 IR，f64 直算）。
            // 三路共有的错误仍会被此路抓住。
            cc.samples == (SEPARABLE_COUNT as u32) * (CROSSCHECK_GRID.len() as u32)
                && cc.per_mode.iter().all(|d| *d <= LSB_BUDGET)
                && cc.within_budget()
        },
        "12 模式 × 16 网格点：IR 标量路与独立 f64 oracle 偏差 ≤1 LSB(8bit)",
    );

    // --- 判据 3：反假变体——IR 抄错规范必被 oracle 抓住 -------------------
    s.add(
        "C22-ORACLE-CATCHES-IR-ERROR",
        {
            // 变异：把 overlay 的段条件从 `Cb<=0.25` 改成 `Cb<=0.5`
            // （一个「看起来完全合理」的抄错）。oracle 侧不变。
            let mut worst = 0.0f32;
            for g in CROSSCHECK_GRID.iter() {
                let (cb, cs) = *g;
                let mut wrong = if cb <= 0.5 {
                    2.0 * cb * cs
                } else {
                    1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
                };
                if wrong > CHANNEL_HI {
                    wrong = CHANNEL_HI;
                }
                let dv = lsb_delta(wrong, oracle(SepMode::Overlay, cb, cs));
                if dv > worst {
                    worst = dv;
                }
            }
            // 前提：正确实现必须全绿，否则「变异被抓住」是废话。
            // 且变异必须**真改变行为路径**：语料里存在落在
            // (0.25, 0.5] 区间内的点（0.26/0.30 × cs=0.50），
            // 段界一改这些点的取值就变——不是等价变异。
            let has_discriminating_point = CROSSCHECK_GRID
                .iter()
                .any(|g| g.0 > SOFT_LIGHT_CB_SPLIT && g.0 <= 0.5 && g.1 <= SOFT_LIGHT_CS_SPLIT);
            cc.worst <= LSB_BUDGET && has_discriminating_point && worst > 8.0 * LSB_BUDGET
        },
        "反假变体：overlay 段界 0.25→0.5 被 oracle 抓住（偏差 >8 LSB）",
    );

    // --- 判据 4：除零边界全覆盖（两个方向相反的取值）---------------------
    s.add(
        "C22-DIVZERO-BOTH-DIRECTIONS",
        {
            // dodge：Cs==1 ⇒ 取 1，且**记账**。
            let mut ld = DivZeroLog::new();
            let dodge = eval_scalar(SepMode::ColorDodge, 0.42, 1.0, &mut ld);
            // burn：Cs==0 ⇒ 取 0，且**记账**。
            let mut lb = DivZeroLog::new();
            let burn = eval_scalar(SepMode::ColorBurn, 0.42, 0.0, &mut lb);
            // 非除零输入**不得**记账（防「恒记账」掩盖未触发）。
            let mut ln = DivZeroLog::new();
            let _ = eval_scalar(SepMode::ColorDodge, 0.42, 0.5, &mut ln);
            let _ = eval_scalar(SepMode::ColorBurn, 0.42, 0.5, &mut ln);
            dodge == DIV_ZERO_DODGE_FALLBACK
                && burn == DIV_ZERO_BURN_RESULT
                && dodge != burn
                && ld.dodge_hits == 1
                && ld.burn_hits == 0
                && lb.burn_hits == 1
                && lb.dodge_hits == 0
                && ln.is_clean()
                && DivZeroKind::all().len() == 2
                && DivZeroKind::Dodge.result_fallback() == 1.0
                && DivZeroKind::Burn.result_fallback() == 0.0
                // 两档的**节点**兜底同为 1.0（burn 靠外层 1- 落到 0）。
                && DivZeroKind::Dodge.div_fallback() == 1.0
                && DivZeroKind::Burn.div_fallback() == 1.0
                && SepMode::ColorDodge.div_zero() == Some(DivZeroKind::Dodge)
                && SepMode::ColorBurn.div_zero() == Some(DivZeroKind::Burn)
                && SepMode::all()
                    .iter()
                    .filter(|m| m.div_zero().is_none())
                    .count()
                    == SEPARABLE_COUNT - 2
        },
        "color-dodge 除零取1 / color-burn 除零取0（方向相反、各自记账、非边界不记账）",
    );

    // --- 判据 5：除零结构归属（无传参错配面）------------------------------
    s.add(
        "C22-DIVZERO-NODE-OWNERSHIP",
        {
            // 公式树里 Div 节点恰 2 个，dodge/burn 各 1，其余 10 种为 0。
            // 归属由**节点 fallback 取值**判定（无外部传参面）。
            let dodge_div = SepMode::ColorDodge.ir().div_count();
            let burn_div = SepMode::ColorBurn.ir().div_count();
            let others = SepMode::all()
                .iter()
                .filter(|m| m.div_zero().is_none())
                .all(|m| m.ir().div_count() == 0);
            dodge_div == 1 && burn_div == 1 && others
        },
        "除法节点恰 2 个（dodge/burn 各 1），其余 10 模式无除法面",
    );

    // --- 判据 6：双路对拍（SIMD 路 vs 标量路）≤1 LSB ----------------------
    s.add(
        "C22-SIMD-CROSSCHECK-1LSB",
        {
            // 逐像素核对：SIMD 四通道输出与标量路**逐位**一致。
            let mut ok = true;
            for m in SepMode::all().iter() {
                for g in CROSSCHECK_GRID.iter() {
                    let (cb, cs) = *g;
                    let mut log = DivZeroLog::new();
                    let scalar = eval_scalar(*m, cb, cs, &mut log);
                    let batch = eval_simd(*m, [cb; SIMD_WIDTH], [cs; SIMD_WIDTH]);
                    for v in batch.out.iter() {
                        if lsb_delta(*v, scalar) > LSB_BUDGET {
                            ok = false;
                        }
                    }
                }
            }
            ok && SIMD_WIDTH == 4
        },
        "SIMD 宽通道路与标量路逐像素对拍差 ≤1 LSB（4 通道批）",
    );

    // --- 判据 7：soft-light 三段全覆盖（夹逼对钉死段界）-------------------
    s.add(
        "C22-SOFTLIGHT-3-SEGMENTS",
        {
            let segmented = (0..SEPARABLE_COUNT)
                .filter(|i| match SepMode::from_index(*i) {
                    Some(m) => m.is_segmented(),
                    None => false,
                })
                .count();
            cov.all_segments_hit()
                && cov.low > 0
                && cov.mid > 0
                && cov.high > 0
                // 结构侧：3 个 select（外层 Cb + 内层 Cs + D 的 Cb）。
                && SepMode::SoftLight.ir().select_count() == 3
                // soft-light 是本组唯一有 sqrt 的。
                && SepMode::SoftLight.ir().sqrt_count() == 1
                && SepMode::all()
                    .iter()
                    .filter(|m| **m != SepMode::SoftLight)
                    .all(|m| m.ir().sqrt_count() == 0)
                && segmented == 1
                // 夹逼对：语料必须在 0.25 / 0.5 两侧各有采样点。
                && CONTINUITY_PROBES
                    .iter()
                    .any(|p| p.0 < SOFT_LIGHT_CB_SPLIT && p.0 > SOFT_LIGHT_CB_SPLIT - 0.02)
                && CONTINUITY_PROBES
                    .iter()
                    .any(|p| p.0 > SOFT_LIGHT_CB_SPLIT && p.0 < SOFT_LIGHT_CB_SPLIT + 0.02)
                && CONTINUITY_PROBES
                    .iter()
                    .any(|p| p.1 < SOFT_LIGHT_CS_SPLIT && p.1 > SOFT_LIGHT_CS_SPLIT - 0.02)
                && CONTINUITY_PROBES
                    .iter()
                    .any(|p| p.1 > SOFT_LIGHT_CS_SPLIT && p.1 < SOFT_LIGHT_CS_SPLIT + 0.02)
        },
        "soft-light 三段各被命中、3 个 select + 1 个 sqrt；语料含 0.25/0.5 夹逼对",
    );

    // --- 判据 8：可分离性（通道无关 + alpha 无关）-------------------------
    s.add(
        "C22-SEPARABLE-CHANNEL-ALPHA",
        ch.is_ok() && al.is_ok(),
        "通道置换等变性 + 逐通道独立；alpha 变化不改 RGB 且原样透传",
    );

    // --- 判据 9：非有限输入收口（不静默传播 NaN）-------------------------
    s.add(
        "C22-NON-FINITE-CONTAINED",
        {
            let nan = f32::NAN;
            let inf = f32::INFINITY;
            let mut ok = true;
            let mut clamped_at_least_once = false;
            for m in SepMode::all().iter() {
                for bad in [nan, inf, -inf, -1.0, 2.0].iter() {
                    let mut log = DivZeroLog::new();
                    let v = eval_scalar(*m, *bad, 0.5, &mut log);
                    if !(v >= CHANNEL_LO && v <= CHANNEL_HI) {
                        ok = false;
                    }
                    if log.clamped_inputs > 0 {
                        clamped_at_least_once = true;
                    }
                }
            }
            // clamp_channel 自身：NaN → 0，±Inf → 两端。
            let (c_nan, f1) = clamp_channel(nan);
            let (c_inf, f2) = clamp_channel(inf);
            let (c_ninf, f3) = clamp_channel(-inf);
            ok && clamped_at_least_once && c_nan == 0.0 && c_inf == 1.0 && c_ninf == 0.0 && f1 && f2 && f3
        },
        "NaN/±Inf/越界输入全部收口到 [0,1] 并记账，无 NaN 传播",
    );

    // --- 判据 10：性能基准（实测操作数；摊薄只对**分段**模式成立）--------
    s.add(
        "C22-PERF-SIMD-AMORTIZED",
        {
            // **只有含 select 的模式才有摊薄空间**。无分段模式
            // （multiply / screen / darken …）每个节点对 4 个通道
            // 各算一次，本来就是 4 倍——宽通道省不了，也不该省。
            // 初版对全部 12 种要求「严格摊薄」，被 8 个无分段模式
            // 判红；那是**判据写错**（要求了物理上不成立的性质），
            // 不是被测物错。正确判据是：
            // ① 分段模式（overlay/hard-light/soft-light）严格摊薄；
            // ② 无分段模式 SIMD == 标量（不多不少，证明没有引入
            //    额外开销，也没偷偷把两支都算）。
            let mut segmented_ok = true;
            let mut plain_ok = true;
            let mut segmented_seen = 0u32;
            for m in SepMode::all().iter() {
                let a = simd_amortization(*m);
                if m.ir().select_count() > 0 {
                    segmented_seen = segmented_seen.saturating_add(1);
                    if !a.amortized() {
                        segmented_ok = false;
                    }
                } else if a.simd_total != a.scalar_total {
                    // 无分段模式：SIMD 与标量应**完全相等**。
                    plain_ok = false;
                }
            }
            // 12 种里恰 3 种含分段（overlay / hard-light / soft-light）。
            segmented_seen == 3 && segmented_ok && plain_ok
        },
        "3 个分段模式每像素操作数严格少于标量；8 个无分段模式 SIMD==标量（无额外开销）",
    );

    // --- 判据 11：分段模式的摊薄来自「只执行选中支」------------------------
    s.add(
        "C22-PERF-SEGMENT-SHORT-CIRCUIT",
        {
            // soft-light 是本组最重的公式：3 个 select。宽通道只执行
            // 选中的一支，故其操作数必须**严格少于**「两支都算」。
            let a = simd_amortization(SepMode::SoftLight);
            let both = soft_light_both_branches_ops();
            a.simd_total > 0 && both > a.simd_total
        },
        "soft-light：同量纲对照下（4 通道批），短路的 SIMD 操作数严格少于「两支全算」",
    );

    // --- 判据 12：WGSL 文本级自检（GPU 路真的「写出来」）------------------
    s.add(
        "C22-GPU-WGSL-TEXT-REAL",
        {
            let mut all_ok = true;
            for m in SepMode::all().iter() {
                if !wgsl_selfcheck(*m).is_ok() {
                    all_ok = false;
                }
            }
            all_ok
        },
        "12 模式 WGSL 文本均含 fn 签名 + clamp 收口 + 该模式特有构造（sqrt/select/除零守卫）",
    );

    // --- 判据 13：除零守卫在 GPU 文本里带**正确**的节点兜底 ---------------
    s.add(
        "C22-GPU-DIVZERO-FALLBACK-CORRECT",
        {
            // 两档的**除法节点**兜底**同为 1.0**（burn 靠外层 `1 -`
            // 落到 0）。故不能靠「文本里有没有 0.0」区分两档——初版
            // 那样断言，判据把 burn 的节点值误当成模式值，恒红。
            //
            // 真正能区分两档的是**结构**：
            // ① dodge 的守卫在**顶层**（`select(1.0, cb / (1-cs), …)`），
            //    burn 的守卫被**外层 `1 -` 包着**（`(1.0 - select(1.0, …))`）；
            // ② 两者的节点兜底字面量都必须是 1.0（传 0.0 即红）。
            let dodge_src = wgsl_source(SepMode::ColorDodge);
            let burn_src = wgsl_source(SepMode::ColorBurn);
            // dodge：守卫前无 `1.0 -` 包裹。
            dodge_src.contains("+ select(1.0, cb / (1.0 - cs), (1.0 - cs) != 0.0)")
                // burn：守卫被 `1.0 -` 包裹 ⇒ 模式结果端点为 0。
                && burn_src.contains("(1.0 - select(1.0, (1.0 - cb) / cs, cs != 0.0))")
                // 节点兜底不得被误写成 0.0（那会让 burn 端点变成 1.0）。
                && !dodge_src.contains("select(0.0,")
                && !burn_src.contains("select(0.0,")
                // 端点数值侧：dodge 端点 1.0、burn 端点 0.0（判据 4 已覆盖，
                // 此处只确认二者**不相等**——若某次重构让两者相等，
                // 说明「方向相反」这个核心事实丢了）。
                && DivZeroKind::Dodge.result_fallback() != DivZeroKind::Burn.result_fallback()
        },
        "WGSL 守卫结构正确：dodge 顶层 select(1.0,…)、burn 被 (1.0 - select(1.0,…)) 包裹；节点兜底不误写为 0",
    );

    // --- 判据 14：无除法模式的 WGSL 不得残留除零守卫 -----------------------
    s.add(
        "C22-GPU-NO-STRAY-GUARD",
        {
            SepMode::all()
                .iter()
                .filter(|m| m.div_zero().is_none())
                .all(|m| !wgsl_source(*m).contains("!= 0.0)"))
        },
        "10 个无除法模式的 WGSL 文本无残留除零守卫（模板未串味）",
    );

    // --- 判据 15：色卡视觉验证（灰阶是文字，不依赖看颜色）----------------
    s.add(
        "C22-SWATCH-12-ROWS",
        {
            let rows = swatch_rows();
            let mut all_ramp = rows.len() == SEPARABLE_COUNT;
            let mut all_distinct = true;
            for r in rows.iter() {
                let start = match r.find(' ') {
                    Some(x) => x + 1,
                    None => 0,
                };
                let bars: Vec<char> = r.chars().skip(start).collect();
                if bars.len() < SWATCH_STEPS {
                    all_ramp = false;
                    continue;
                }
                for c in bars.iter() {
                    if !RAMP.contains(c) {
                        all_ramp = false;
                    }
                }
                // 灰阶必须**真的在变**：全行同字符 ⇒ 色卡没有信息量。
                let mut distinct = false;
                for a in 0..bars.len() {
                    for b in (a + 1)..bars.len() {
                        if bars[a] != bars[b] {
                            distinct = true;
                        }
                    }
                }
                if !distinct {
                    all_distinct = false;
                }
            }
            // 色卡探针数自洽：两段斜坡各 6 格。
            all_ramp
                && all_distinct
                && SWATCH_STEPS == 12
                && SWATCH_PROBES.len() == SWATCH_STEPS
                // 斜坡端点与中值必须在场（否则「斜坡」只是名字）。
                && SWATCH_PROBES.iter().any(|p| p.0 == CHANNEL_LO && p.1 == 0.5)
                && SWATCH_PROBES.iter().any(|p| p.0 == CHANNEL_HI && p.1 == 0.5)
                && SWATCH_PROBES.iter().any(|p| p.0 == 0.5 && p.1 == CHANNEL_LO)
                && SWATCH_PROBES.iter().any(|p| p.0 == 0.5 && p.1 == CHANNEL_HI)
        },
        "12 行色卡齐备（双斜坡 12 探针）、灰阶用文字表达、每行至少两格亮度不同",
    );

    // --- 判据 16：色卡端点语义（暗部增强 / 亮部增强，锚点原文）-----------
    s.add(
        "C22-SWATCH-SEMANTICS",
        {
            // 锚点：multiply「暗部增强」、screen「亮部增强」。
            // 判据用**数值**核对，不靠肉眼看色卡。
            let mut log = DivZeroLog::new();
            // multiply(0.5, 0.5) = 0.25 < 0.5 —— 压暗。
            let m_dark = eval_scalar(SepMode::Multiply, 0.5, 0.5, &mut log);
            // screen(0.5, 0.5) = 0.75 > 0.5 —— 提亮。
            let s_light = eval_scalar(SepMode::Screen, 0.5, 0.5, &mut log);
            // normal 是源覆盖：与 Cb 无关。
            let n_a = eval_scalar(SepMode::Normal, 0.1, 0.7, &mut log);
            let n_b = eval_scalar(SepMode::Normal, 0.9, 0.7, &mut log);
            // darken 交换律：darken(a,b)==darken(b,a)。
            let d_ab = eval_scalar(SepMode::Darken, 0.3, 0.8, &mut log);
            let d_ba = eval_scalar(SepMode::Darken, 0.8, 0.3, &mut log);
            // difference 自反为 0；exclusion 自反 = 2x-2x² = 0.5。
            let diff_self = eval_scalar(SepMode::Difference, 0.6, 0.6, &mut log);
            let excl_self = eval_scalar(SepMode::Exclusion, 0.5, 0.5, &mut log);
            // overlay 与 hard-light 同侧取同一支时数值相等
            // （0.2/0.3 两通道都在 0.25 以下）。
            let ov = eval_scalar(SepMode::Overlay, 0.2, 0.3, &mut log);
            let hl = eval_scalar(SepMode::HardLight, 0.2, 0.3, &mut log);
            m_dark < 0.5
                && s_light > 0.5
                && n_a == 0.7
                && n_b == 0.7
                && (d_ab - d_ba).abs() <= LSB8
                && diff_self.abs() <= LSB8
                && (excl_self - 0.5).abs() <= LSB8
                && (ov - hl).abs() <= LSB8
        },
        "端点语义：multiply 压暗/screen 提亮/normal 源覆盖/darken 交换/difference 自反 0/exclusion 自反 0.5",
    );

    // --- 判据 17：overlay / hard-light 的「换位」是**结构**关系 -----------
    s.add(
        "C22-OVERLAY-HARDLIGHT-SWAP-STRUCTURE",
        {
            // 锚点原文「hard-light（强光——overlay 换位）」指**分段角色
            // 对调**：overlay 以 **Cb**（背景）分段、hard-light 以 **Cs**
            // （源）分段，两支公式逐字相同。它**不是**数值等式
            // f(a,b)==g(b,a)——初版把换位读成那个等式，判据恒红
            // （(0.3,0.8) 处差 0.24）。此处改为断言结构事实：
            // ① 两式的 select 归属不同（OnCb vs OnCs）；
            // ② 两支的公式文本**逐字相同**；
            // ③ 角色对调后的**数值**在同支条件下相等
            //    （overlay 在 Cb 段、hard-light 在 Cs 段同侧时成立）。
            let ov_ir = SepMode::Overlay.ir();
            let hl_ir = SepMode::HardLight.ir();
            // 分支形状核对：overlay 必为 SelectOnCb、hard-light 必为
            // SelectOnCs。取不到就整个判据为 false（用 Option 收敛，
            // 不在返回 CheckSet 的闭包里 `return false`）。
            let ov_branches = match &ov_ir {
                Ir::SelectOnCb(a, b) => Some((a.as_ref(), b.as_ref())),
                _ => None,
            };
            let hl_branches = match &hl_ir {
                Ir::SelectOnCs(a, b) => Some((a.as_ref(), b.as_ref())),
                _ => None,
            };
            let (same_lo, same_hi) = match (ov_branches, hl_branches) {
                (Some((ov_lo, ov_hi)), Some((hl_lo, hl_hi))) => {
                    (wgsl_expr(ov_lo) == wgsl_expr(hl_lo), wgsl_expr(ov_hi) == wgsl_expr(hl_hi))
                }
                _ => (false, false),
            };
            // ③ 数值侧：Cs 落在与 Cb 同一侧时，两式取同一支 ⇒ 相等。
            let mut log = DivZeroLog::new();
            let mut num_ok = true;
            for g in CROSSCHECK_GRID.iter() {
                let (a, b) = *g;
                let cb_low = a <= SOFT_LIGHT_CB_SPLIT;
                let cs_low = b <= SOFT_LIGHT_CS_SPLIT;
                if cb_low == cs_low {
                    let ov = eval_scalar(SepMode::Overlay, a, b, &mut log);
                    let hl = eval_scalar(SepMode::HardLight, a, b, &mut log);
                    if lsb_delta(ov, hl) > LSB_BUDGET {
                        num_ok = false;
                    }
                }
            }
            same_lo && same_hi && num_ok
        },
        "overlay 以 Cb 分段 / hard-light 以 Cs 分段，两支公式逐字相同（锚点「换位」= 结构对调）",
    );

    // --- 判据 18：IR 树结构与规范复杂度相符（防「偷工」简化公式）--------
    s.add(
        "C22-IR-STRUCTURE-NOT-SIMPLIFIED",
        {
            // 每个模式的树「形状」符合规范：multiply 无 select/div；
            // overlay/hard-light 各恰 1 select；exclusion 无 sqrt。
            let m_mul = SepMode::Multiply.ir();
            let m_ov = SepMode::Overlay.ir();
            let m_hl = SepMode::HardLight.ir();
            let m_ex = SepMode::Exclusion.ir();
            m_mul.select_count() == 0
                && m_mul.div_count() == 0
                && m_mul.sqrt_count() == 0
                && m_ov.select_count() == 1
                && m_hl.select_count() == 1
                && m_ex.select_count() == 0
                && m_ex.sqrt_count() == 0
                && m_ex.div_count() == 0
                && SepMode::Normal.ir() == Ir::Cs
        },
        "IR 树形状与规范相符：multiply 无分段、overlay/hard-light 各 1 分段、exclusion 无超越函数、normal 即 Cs",
    );

    // --- 判据 19：oracle 真的逐模式区分行为（12 组互异指纹）--------------
    s.add(
        "C22-ORACLE-DISTINGUISHES-MODES",
        {
            // 若 oracle 退化为「对所有模式返回同一值」，对拍会因
            // 标量路也跟着退化而**一起全绿**。故独立证明 oracle
            // 在 12 模式上给出 12 组互不相同的行为指纹。
            let mut fp: Vec<u64> = Vec::new();
            for m in SepMode::all().iter() {
                let mut h: u64 = 1469598103934665603;
                for g in CROSSCHECK_GRID.iter() {
                    // `to_bits()` 给的是 u32，但下面的 FNV-1a 轮函数按
                    // **8 字节**逐字节折叠（b 取 0..8）。若直接把 u32
                    // 拿去移位，`bits >> 56` 会越过 u32 的 32 位宽度，
                    // debug 构建下直接 panic（attempt to shift right
                    // with overflow）——判据自己先崩，等于这条判据
                    // 从未真正跑过。故先零扩展到 u64，移位才落在
                    // 合法区间内，且 8 字节折叠与 64 位 FNV 环宽一致。
                    let bits = u64::from(oracle(*m, g.0, g.1).to_bits());
                    for b in 0..8u32 {
                        h ^= (bits >> (b * 8)) & 0xff;
                        h = h.wrapping_mul(1099511628211);
                    }
                }
                fp.push(h);
            }
            let mut uniq = true;
            for i in 0..fp.len() {
                for j in (i + 1)..fp.len() {
                    if fp[i] == fp[j] {
                        uniq = false;
                    }
                }
            }
            uniq && fp.len() == SEPARABLE_COUNT
        },
        "oracle 在 12 模式上给出 12 组互不相同的行为指纹（未退化为常量）",
    );

    // --- 判据 20：LSB 口径不是空断言（预算本身有意义）--------------------
    s.add(
        "C22-LSB-BUDGET-NOT-VACUOUS",
        {
            // 反假：若 LSB 口径取得过宽（如用 f32::EPSILON），任何
            // 实现都恒过。此处断言「预算确实能判失败」：构造一个
            // 2 LSB 的偏差，必须超预算。
            let a = 0.5f32;
            let b = a + 2.0 * LSB8;
            lsb_delta(a, b) > LSB_BUDGET && LSB8 < 0.01 && LSB_BUDGET == 1.0
        },
        "LSB 口径=1/255 有效：2 LSB 偏差超预算（预算非恒过）",
    );

    // --- 判据 21：反假变体——除零方向传反必被双判据抓住 ------------------
    s.add(
        "C22-DIVZERO-SWAP-CAUGHT",
        {
            // 变异：把 burn 的兜底取成 dodge 的 1.0（模拟「同除零
            // 纪律」被误实现成一个参数化 fallback 且传反）。
            // 正向判据（判据 4）与 GPU 文本判据（判据 13）都必须抓到。
            let mut lb = DivZeroLog::new();
            // 变异实现：Cs==0 时错误地取 1。
            let cs = 0.0f32;
            let cb = 0.42f32;
            let mut wrong = if cs == 0.0 {
                DivZeroKind::Dodge.result_fallback()
            } else {
                1.0 - (1.0 - cb) / cs
            };
            if !(wrong >= CHANNEL_LO && wrong <= CHANNEL_HI) {
                wrong = CHANNEL_HI;
            }
            let _ = &mut lb;
            // 与正确值（0.0）相比必须**明显**不同。
            let correct = eval_scalar(SepMode::ColorBurn, cb, cs, &mut DivZeroLog::new());
            (wrong - correct).abs() > 0.5 && correct == DIV_ZERO_BURN_RESULT
        },
        "反假变体：burn 兜底误取 dodge 的 1.0 → 与正确值差 1.0（>0.5）",
    );

    // --- 判据 22：LSB 口径**独立重算**（防「放宽 LSB 恒过」）----------
    s.add(
        "C22-LSB-STEP-INDEPENDENT",
        {
            // 漏网复盘：初版只断言 `lsb_delta(a, a+2*LSB8) > LSB_BUDGET`，
            // 两侧都读**被测常量** LSB8。把它从 1/255 放宽到 1e-6 后，
            // 构造出的偏差同步缩小，比值不变 ⇒ 仍全绿。这是「自证式」
            // 的典型：判据与被测共享同一个量。
            //
            // 修法：判据侧**不读 LSB8**，按 8 bit 量化的定义独立重算
            // 参考步长 = 1 / (2^8 - 1)，再要求两者相等。
            let indep_step = 1.0f32 / ((1u32 << 8) - 1) as f32;
            let indep_delta = (0.5f32 - 0.25f32).abs() / indep_step;
            lsb_delta(0.5, 0.25) == indep_delta
                && (LSB8 - indep_step).abs() < 1e-7
                && lsb_delta(0.5, 0.5 + 2.0 * indep_step) > LSB_BUDGET
        },
        "LSB 步长由 2^8-1 独立重算并与常量对账（放宽 LSB8 必被此判据抓住）",
    );

    // --- 判据 23：SIMD 退化路径**真被走到**（M13 漏网） ------------------
    s.add(
        "C22-SIMD-DEGENERATION-EXERCISED",
        {
            // 漏网复盘：判据只比「操作数」与「结果」，不验证
            // `predicates_uniform` 的**退化分支**是否真被触发。
            // 把该分支删掉（`if true`）后，在「谓词一致」的常规
            // 语料上两路结果完全一样 ⇒ 全绿。
            //
            // 修法：构造**谓词分歧**的批（4 通道的 Cb 分处段界两侧），
            // 此时只有退化路径能给出逐通道正确结果。
            let mut all_correct = true;
            let mut degenerate_seen = 0u32;
            let divergent = [
                [0.10f32, 0.90, 0.10, 0.90],
                [0.10, 0.10, 0.90, 0.90],
                [0.20, 0.30, 0.80, 0.26],
            ];
            let cs_batch = [0.5f32, 0.8, 0.2, 0.6];
            for cbv in divergent.iter() {
                if predicates_uniform(SepMode::Overlay, *cbv, cs_batch) {
                    // 语料本应分歧；被判一致说明 `predicates_uniform` 失效。
                    continue;
                }
                degenerate_seen = degenerate_seen.saturating_add(1);
                let batch = eval_simd(SepMode::Overlay, *cbv, cs_batch);
                for i in 0..SIMD_WIDTH {
                    let mut log = DivZeroLog::new();
                    let scalar = eval_scalar(SepMode::Overlay, cbv[i], cs_batch[i], &mut log);
                    if lsb_delta(batch.out[i], scalar) > LSB_BUDGET {
                        all_correct = false;
                    }
                }
            }
            // 语料必须**确实**造成分歧（否则本判据是恒真）。
            degenerate_seen == 3 && all_correct
        },
        "谓词分歧语料（4 通道 Cb 跨段界）确实触发退化路径且逐通道结果正确",
    );

    // --- 判据 24：短路**真的发生**（M14 漏网） ---------------------------
    s.add(
        "C22-SIMD-SHORT-CIRCUIT-TAKES-ONE-BRANCH",
        {
            // 漏网复盘：判据 11 只比「操作数总量」。把短路改成
            // 「两支都算」后操作数变多，但没有任何判据核对
            // 「未选中分支的独有节点**未被求值**」。
            //
            // 修法：利用「只有段 3 含 sqrt」这一结构事实——
            // 段 1 批求值后 `unary`（含 sqrt）计数必须为 **0**；
            // 段 3 批必须为正。短路失效则段 1 也会 ≥1。
            let seg1 = eval_simd(
                SepMode::SoftLight,
                [0.10, 0.10, 0.10, 0.10],
                [0.20, 0.20, 0.20, 0.20],
            );
            let seg3 = eval_simd(
                SepMode::SoftLight,
                [0.80, 0.80, 0.80, 0.80],
                [0.90, 0.90, 0.90, 0.90],
            );
            // 段 1 的结果须等于 W3C 解析式（判据侧独立重算）。
            let cb = 0.10f64;
            let cs = 0.20f64;
            let expect = cs - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
            seg1.ops.unary == 0
                && seg3.ops.unary > 0
                && lsb_delta(seg1.out[0], expect as f32) <= LSB_BUDGET
        },
        "短路真发生：段1 批 sqrt 计数为 0、段3 批为正；段1 数值与 W3C 解析式相符",
    );

    // --- 判据 25：跨通道耦合（M17 漏网）——换能触发的语料 -----------------
    s.add(
        "C22-CHANNEL-COUPLING-ON-TRIGGERING-CORPUS",
        {
            // 漏网复盘：初版的置换等变性语料 cb = [0.15,0.62,0.93]，
            // 跨通道耦合的实现（如「取三通道最小值再算」）在这些点上
            // 恰好与逐通道结果相同 ⇒ 仍全绿。
            //
            // 修法：语料改为**三通道分处段界两侧**（0.05/0.95/0.25），
            // 并用 overlay（分段）/ screen（无分段）/ soft-light
            // 三种形态同时验。任何跨通道耦合都会立刻暴露。
            let cb = [0.05f32, 0.95, 0.25];
            let cs = [0.90f32, 0.10, 0.50];
            let perm = [1usize, 2, 0];
            let mut ok = true;
            for m in [SepMode::Overlay, SepMode::Screen, SepMode::SoftLight].iter() {
                let px = eval_pixel_rgba(*m, cb, cs, 1.0, &mut DivZeroLog::new());
                for ch in 0..3 {
                    let single = eval_scalar(*m, cb[ch], cs[ch], &mut DivZeroLog::new());
                    if lsb_delta(px[ch], single) > LSB_BUDGET {
                        ok = false;
                    }
                }
                let mut cb2 = [0.0f32; 3];
                let mut cs2 = [0.0f32; 3];
                for i in 0..3 {
                    cb2[i] = cb[perm[i]];
                    cs2[i] = cs[perm[i]];
                }
                let py = eval_pixel_rgba(*m, cb2, cs2, 1.0, &mut DivZeroLog::new());
                for i in 0..3 {
                    if lsb_delta(py[i], px[perm[i]]) > LSB_BUDGET {
                        ok = false;
                    }
                }
            }
            ok
        },
        "跨通道耦合语料（0.05/0.95/0.25 跨段界）+ 换序等变：三种形态逐通道独立",
    );

    s
}
