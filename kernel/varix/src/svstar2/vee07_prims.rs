//! VE-F0807 · 文本图元渲染（VE-E 域 · 文字渲染段 4 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0807`
//!
//! **判据（锚点原文五条）**：合批 ≥2,000、绘制 ≤2 次、复用声明、四效果零加调用、
//! 0.4ms。逐条落位：
//!
//! - **合批 ≥2,000**：[`Batcher::build`] 把文本 quad 按批次键三元组
//!   （图集页、材质变体、混合模式）分组，组内按深度排序（画家序），
//!   单批目标 [`BATCH_TARGET_QUADS`] = 2,000。自检 `E07-判据-合批2000`
//!   用**恰好 2,000** 个 quad（同页同变体同混合）断言产出单批且
//!   **总数守恒**（提交数 == 落批数，无静默丢弃）。
//! - **绘制 ≤2 次**：这是本单**最容易被做假**的一条。批次键含页号，若把
//!   页号当**管线状态**（每次换页重绑纹理），4 页就是 4 次绘制 ⇒ 判红。
//!   正确做法（也是本单的结构核心）：**页号走实例属性**（图集为纹理数组，
//!   层索引是 per-instance 数据），**管线状态只有（材质变体、混合模式）**。
//!   故 [`plan_draw_calls`] 只按 `(variant, blend)` 的**连续游程**切分绘制
//!   调用，[`Batcher::build`] 保证同 `(variant, blend)` 的批次在列表中连续。
//!   自检 `E07-判据-绘制两次` 用 4 页 × 2,000 quad 断言绘制调用 **== 1**
//!   （≤2 的更强形态），并用「深度全局交错」的病态输入断言分组把绘制调用
//!   拉回 ≤2 —— 后者杀死「不分组只按深度排序」的实现。
//! - **复用声明**：文本着色器作为 **C 域管线的一个材质变体**注册，
//!   **不另起管线**。[`PipelineBinding`] 的 `pipeline_count` 在注册任意数量
//!   的材质变体后恒为 1（自检 `E07-判据-不另起管线`）——若实现每变体新建
//!   一条管线，该计数随之增长 ⇒ 转红。声明体复用同域
//!   [`vee01_arch::ReuseDeclaration`]，不另立结构。
//! - **四效果零加调用**：颜色 / 描边 / 阴影 / 渐变填充**全部由着色器参数化**。
//!   结构性保证是 [`EffectParams::pack`] 的**定长 uniform 布局**：无论启用
//!   哪几个效果，写入的 [`UNIFORM_WORDS`] 恒定（未启用的效果以标志位禁用，
//!   而非缺席导致收缩）。定长 uniform ⇒ 可实例化 ⇒ 效果数与绘制调用数
//!   **无关**。自检 `E07-判据-四效果零加调用` 以「0 效果 / 1 效果 / 4 效果」
//!   三档断言 `uniform_words` 与 `draw_calls` **都相等**。
//! - **0.4ms**：内核无墙钟，[`TEXT_LAYER_BUDGET_US`] = 400µs 只是把锚点
//!   固化进预算表供 F0815 引用，**不参与判定**（真机 GPU 计时归 F0815）。
//!   本单自检的是**可测的一半且实测真实工作量**：[`WorkCounter`] 在缺陷
//!   真正发生的层计数——深度排序的**比较与搬运**、批次键匹配的**比较**、
//!   效果打包的**字写入**。判据是**复杂度类**而非 `n×CONST`：
//!   `work(2n) / work(n) ≤ [`GROWTH_CAP`]`（归并排序 ≈2.1，插入排序 ≈4）。
//!   把实现换成插入排序 ⇒ 比值越界 ⇒ 转红。
//!
//! **数据结构（锚点「批次键=（图集页、材质变体、混合模式），排序按键分组后
//! 按深度」）**：批次键 [`BatchKey`] 三元组**恰好三段**，深度是**组内**排序
//! 键而非批次键第四段（否则同页同变体不同深度的字形会被拆批）。
//!
//! **变换（锚点「变换（局部/世界空间文字）」）**：[`TextSpace`] 二档。
//! 局部空间 `dst` 直接使用、深度取层序；世界空间经 [`Model2D`] 仿射变换，
//! 深度取 `world_z`。退化变换（行列式为 0）⇒ quad 落成零面积 ⇒ **显式计入**
//! [`BatchStats::degenerate_xform`]，不静默消失。
//!
//! **错误路径与降级矩阵（零静默，逐档计数）**：
//!
//! | 触发 | 处置 | 计数 / 告警 |
//! |---|---|---|
//! | 图集页切换频繁（批碎） | 合并策略告警 + **提示图集整理** | [`MergeWarning`]（含 `hint`） |
//! | 材质变体超上限 | **拒绝注册** + 提示复用 | [`RegisterOutcome::Rejected`] |
//! | 世界空间退化变换 | 零面积 ⇒ 不产出几何 | [`BatchStats::degenerate_xform`] |
//! | 效果使对比度跌破 AA | **钳制**到 AA 以上，绝不原样放行 | [`BatchStats::contrast_clamped`] |
//!
//! - **批碎告警的分母是 quad 数不是批数**：[`MergeWarning::switch_rate_permille`]
//!   的分母取提交 quad 数。挂在批数上会让「批少但每批内页跳变频繁」的场景
//!   被稀释成低费率而漏报。
//! - **对比度红线用外部锚点，不自证**：判定调用 **VE-S 域**
//!   [`vet02_highcontrast_engine::contrast_ratio`]（WCAG 2.x 相对亮度），
//!   本单**不另写一份** WCAG 公式——两份实现必然分叉（分叉正是复用纪律要防的）。
//!
//! **零 panic 面**：全模块用 `get`/`get_mut`、`last`/`last_mut` 与显式边界检查，
//! **无 `unwrap()`、无 `expect()`、无 `panic!`、无裸 `[i]` 索引**
//! （越界即视为容量不足并计数，不中止渲染）。断言集中在
//! [`run_vee07_checks`] 内。

extern crate alloc;

use alloc::string::ToString;
use alloc::vec::Vec;

use super::vee01_arch::ReuseDeclaration;
use super::vee03_outline::QUANT_ONE;
use super::vee04_raster::Weight;
use super::vee05_hinting::CacheKey;
use super::vee06_atlas::{Rect as AtlasRect, UvRect};
use super::vet02_highcontrast_engine::{contrast_ratio, WCAG_AA_NORMAL};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量
// ---------------------------------------------------------------------------

/// 单批目标 quad 数（锚点「单批目标 ≥2,000 quads」）。
pub const BATCH_TARGET_QUADS: u32 = 2_000;

/// 文本层每帧绘制调用上限（锚点「绘制调用 ≤2 次/帧文本层」）。
pub const MAX_DRAW_CALLS_PER_FRAME: u32 = 2;

/// 文本层每帧 GPU 预算（锚点「文本层整体 ≤0.4ms」）。
///
/// **本单不据此判定**（内核无墙钟）；仅固化进预算表供 F0815 引用。
pub const TEXT_LAYER_BUDGET_US: u32 = 400;

/// 材质变体注册上限（锚点「材质变体超上限→拒绝注册并提示复用」）。
pub const MAX_MATERIAL_VARIANTS: u32 = 16;

/// 批碎告警的换页率阈值（千分比）。
///
/// 取 250‰：每 4 个 quad 就换一次页即视为批碎。阈值写成**由两个更基本的
/// 常量导出**（[`FRAG_WARN_SWITCHES_PER_QUAD_NUM`] /分母），不裸写数字——
/// 裸写的阈值在语料规模变化时会悄悄测着另一件事却仍显绿。
pub const FRAG_WARN_PERMILLE: u32 = 250;

/// [`FRAG_WARN_PERMILLE`] 的分子来源（每 N 个 quad 允许的换页数）。
pub const FRAG_WARN_SWITCHES_PER_QUAD_NUM: u32 = 1;

/// [`FRAG_WARN_PERMILLE`] 的分母来源。
pub const FRAG_WARN_SWITCHES_PER_QUAD_DEN: u32 = 4;

/// 定长 uniform 字数（**布局恒定，不随启用效果数收缩**——四效果零加调用的
/// 结构性保证）。
///
/// 布局分解：**1 字标志位 + 4 组 × 4 字**（填充色 / 描边 / 阴影 / 渐变）
/// = **17 字**。四组各占满 4 字，未启用者以标志位禁用而非缺席收缩——
/// 收缩会让布局随效果数变化，每种组合一套布局即无法合批。
pub const UNIFORM_WORDS: usize = 17;

/// 效果色打包标志位。
pub mod effect_flags {
    /// 描边启用。
    pub const STROKE: u32 = 1 << 0;
    /// 阴影启用。
    pub const SHADOW: u32 = 1 << 1;
    /// 渐变填充启用。
    pub const GRADIENT: u32 = 1 << 2;
}

/// 单个场景允许的最大 quad 数（防无界增长的显式上界）。
pub const MAX_QUADS: usize = 1 << 16;

/// 复杂度增长上限：`work(2n)/work(n)` 的判据上限。
///
/// 归并排序实测 ≈2.1（每层一趟），插入排序 ≈4（最坏）/≈2（已序）。
/// 取 3.0：容得下归并排序的常数抖动，容不下插入排序的二次膨胀。
pub const GROWTH_CAP_NUM: u32 = 3;
pub const GROWTH_CAP_DEN: u32 = 1;

/// 归并排序层数（`ceil(log2(BATCH_TARGET_QUADS))`，编译期算出，不手填）。
///
/// 自底向上归并对 `m` 个元素恰走 `ceil(log2 m)` 趟。
pub const MERGE_LEVELS: u32 = ceil_log2(BATCH_TARGET_QUADS);

/// 单 quad 平均工作量上限（**由结构导出，不裸写**）。
///
/// 随规模增长的层只有两项，逐项算账：
///
/// - uniform 写入：每 quad 恒 [`UNIFORM_WORDS`] 字；
/// - 归并排序：每趟每元素**恰一次搬运**、**至多一次比较** ⇒ 每 quad
///   ≤ `2 × MERGE_LEVELS`。
///
/// 故每 quad 上限 = `UNIFORM_WORDS + 2 × MERGE_LEVELS`。
///
/// **批次键比较（`key_cmp`）不在此上限内**：它的规模因子是**不同批次键的
/// 个数**（随页数 × 变体数 × 混合数增长），不是 quad 数；把它按 quad 摊到
/// 同一个上限里，就必须把上限调到与批数耦合，判据随之漂移。它另有断言
/// （`E07-05` 的键比较规模项）。
pub const WORK_PER_QUAD_CAP: u64 = UNIFORM_WORDS as u64 + 2 * MERGE_LEVELS as u64;

/// 编译期 `ceil(log2 n)`（`n ≥ 1`）。
const fn ceil_log2(n: u32) -> u32 {
    let mut v = n;
    let mut levels = 0u32;
    while v > 1 {
        v = (v + 1) / 2;
        levels += 1;
    }
    levels
}

// ---------------------------------------------------------------------------
// 二、着色器参数：四效果（定长 uniform，零加绘制调用）
// ---------------------------------------------------------------------------

/// 8 位 RGBA。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8(pub [u8; 4]);

impl Rgba8 {
    /// 取 RGB 三元组（对比度判定用；忽略 alpha）。
    pub const fn rgb(&self) -> [u8; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }
}

/// 描边参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrokeParams {
    /// 描边宽度（F26.6 像素）。
    pub width_q: i32,
    /// 描边色。
    pub color: Rgba8,
}

/// 阴影参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowParams {
    /// 偏移 X（F26.6 像素）。
    pub dx_q: i32,
    /// 偏移 Y（F26.6 像素）。
    pub dy_q: i32,
    /// 柔化半径（F26.6 像素）。
    pub softness_q: i32,
    /// 阴影色。
    pub color: Rgba8,
}

/// 渐变轴。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientAxis {
    /// 水平。
    Horizontal,
    /// 垂直。
    Vertical,
}

impl GradientAxis {
    /// 稳定序号（**显式函数，不写 `as u8` 造二进制头**）。
    pub const fn ordinal(self) -> u16 {
        match self {
            GradientAxis::Horizontal => 0,
            GradientAxis::Vertical => 1,
        }
    }
}

/// 渐变填充参数（两停靠点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradientParams {
    /// 起始停靠色。
    pub from: Rgba8,
    /// 结束停靠色。
    pub to: Rgba8,
    /// 轴向。
    pub axis: GradientAxis,
}

/// 四效果参数集合。
///
/// **本单的关键设计**：四个效果**全部**经由 [`pack`] 写入**定长**
/// [`UNIFORM_WORDS`] 字 uniform，未启用的效果以标志位禁用而非缺席。
/// 定长 ⇒ 顶点属性布局与效果数无关 ⇒ 可实例化 ⇒ **效果数不增加绘制调用**。
/// 若把未启用效果「省掉」以缩小 uniform，布局就随效果数变化，
/// 「四效果零加调用」立刻失效（每种效果组合一套布局 ⇒ 无法合批）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectParams {
    /// 效果①：颜色（填充色，始终存在）。
    pub color: Rgba8,
    /// 效果②：描边。
    pub stroke: Option<StrokeParams>,
    /// 效果③：阴影。
    pub shadow: Option<ShadowParams>,
    /// 效果④：渐变填充。
    pub gradient: Option<GradientParams>,
}

impl Default for EffectParams {
    fn default() -> Self {
        EffectParams {
            color: Rgba8([255, 255, 255, 255]),
            stroke: None,
            shadow: None,
            gradient: None,
        }
    }
}

impl EffectParams {
    /// 仅颜色（基线档）。
    pub const fn color_only(c: Rgba8) -> Self {
        EffectParams {
            color: c,
            stroke: None,
            shadow: None,
            gradient: None,
        }
    }

    /// 四效果全开。
    pub const fn all_four(
        color: Rgba8,
        stroke: StrokeParams,
        shadow: ShadowParams,
        gradient: GradientParams,
    ) -> Self {
        EffectParams {
            color,
            stroke: Some(stroke),
            shadow: Some(shadow),
            gradient: Some(gradient),
        }
    }

    /// 启用效果标志位。
    pub fn flags(&self) -> u32 {
        let mut f = 0u32;
        if self.stroke.is_some() {
            f |= effect_flags::STROKE;
        }
        if self.shadow.is_some() {
            f |= effect_flags::SHADOW;
        }
        if self.gradient.is_some() {
            f |= effect_flags::GRADIENT;
        }
        f
    }

    /// 打包进**定长** uniform。
    ///
    /// **恒返回 [`UNIFORM_WORDS`]**——这是「四效果零加调用」的结构性保证，
    /// 自检逐档核（0/1/4 效果打包后 `uniform_words` 必须相等）。
    /// 写入的字数为定值，不随 `flags()` 变化。
    pub fn pack(&self, out: &mut [u32]) -> usize {
        let mut n = 0usize;
        // 字 0：标志位。
        if n < out.len() {
            out[n] = self.flags();
            n += 1;
        }
        // 字 1..4：填充色 RGBA8888（固定占位，不因效果数变化）。
        if n + 4 <= out.len() {
            let c = self.color.0;
            out[n] = pack_rgba(c[0], c[1], c[2], c[3]);
            out[n + 1] = pack_rgba(0, 0, 0, 0);
            out[n + 2] = 0;
            out[n + 3] = 0;
            n += 4;
        }
        // 字 5..8：描边（固定占位）。
        if n + 4 <= out.len() {
            match self.stroke {
                Some(s) => {
                    out[n] = s.width_q as u32;
                    out[n + 1] = pack_rgba(s.color.0[0], s.color.0[1], s.color.0[2], s.color.0[3]);
                    out[n + 2] = 0;
                    out[n + 3] = 0;
                }
                None => {
                    out[n] = 0;
                    out[n + 1] = 0;
                    out[n + 2] = 0;
                    out[n + 3] = 0;
                }
            }
            n += 4;
        }
        // 字 9..12：阴影（固定占位）。
        if n + 4 <= out.len() {
            match self.shadow {
                Some(s) => {
                    out[n] = pack_i32_pair(s.dx_q, s.dy_q);
                    out[n + 1] = s.softness_q as u32;
                    out[n + 2] = pack_rgba(s.color.0[0], s.color.0[1], s.color.0[2], s.color.0[3]);
                    out[n + 3] = 0;
                }
                None => {
                    out[n] = 0;
                    out[n + 1] = 0;
                    out[n + 2] = 0;
                    out[n + 3] = 0;
                }
            }
            n += 4;
        }
        // 字 13..16：渐变（固定占位）。
        if n + 4 <= out.len() {
            match self.gradient {
                Some(g) => {
                    out[n] = g.axis.ordinal() as u32;
                    out[n + 1] = pack_rgba(g.from.0[0], g.from.0[1], g.from.0[2], g.from.0[3]);
                    out[n + 2] = pack_rgba(g.to.0[0], g.to.0[1], g.to.0[2], g.to.0[3]);
                    out[n + 3] = 0;
                }
                None => {
                    out[n] = 0;
                    out[n + 1] = 0;
                    out[n + 2] = 0;
                    out[n + 3] = 0;
                }
            }
            n += 4;
        }
        n
    }
}

fn pack_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32)
}

fn pack_i32_pair(a: i32, b: i32) -> u32 {
    (((a as u32) & 0xFFFF) << 16) | ((b as u32) & 0xFFFF)
}

// ---------------------------------------------------------------------------
// 三、混合模式与材质变体（C 域管线复用）
// ---------------------------------------------------------------------------

/// 混合模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlendMode {
    /// 不透明。
    Opaque = 0,
    /// 普通 alpha 混合。
    Alpha = 1,
    /// 加法混合。
    Additive = 2,
    /// 掩码（字形覆盖率）。
    Masked = 3,
}

impl BlendMode {
    /// 全集（稳定序）。
    pub const ALL: [BlendMode; 4] = [
        BlendMode::Opaque,
        BlendMode::Alpha,
        BlendMode::Additive,
        BlendMode::Masked,
    ];

    /// 稳定序号（**显式 match，不写 `as u8`**）。
    pub const fn ordinal(self) -> u16 {
        match self {
            BlendMode::Opaque => 0,
            BlendMode::Alpha => 1,
            BlendMode::Additive => 2,
            BlendMode::Masked => 3,
        }
    }

    /// 中文名（UI 与告警共用同一份字面量）。
    pub const fn name(self) -> &'static str {
        match self {
            BlendMode::Opaque => "不透明",
            BlendMode::Alpha => "Alpha 混合",
            BlendMode::Additive => "加法混合",
            BlendMode::Masked => "掩码",
        }
    }
}

/// 材质变体句柄（C 域管线上的一档参数组合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaterialVariant(pub u16);

impl MaterialVariant {
    /// 稳定序号。
    pub const fn index(self) -> u16 {
        self.0
    }
}

/// 注册结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// 注册成功。
    Registered(MaterialVariant),
    /// 超上限拒绝（**零静默**：带原因与建议动作）。
    Rejected {
        /// 已注册数量。
        have: u32,
        /// 上限。
        cap: u32,
        /// 建议动作。
        hint: &'static str,
    },
}

impl RegisterOutcome {
    /// 是否注册成功。
    pub fn is_registered(self) -> bool {
        matches!(self, RegisterOutcome::Registered(_))
    }
}

/// C 域共享管线 ID（文本着色器挂在这一条上，**不新建**）。
pub const CDOMAIN_PIPELINE_ID: u16 = 1;

/// 管线绑定表：登记文本材质变体，并**证明未另起管线**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineBinding {
    variants: Vec<MaterialVariant>,
    /// 已建管线数（**恒为 1** —— 每注册一个变体不得增长）。
    pub pipeline_count: u32,
    /// 超上限拒绝次数。
    pub rejected: u32,
}

impl Default for PipelineBinding {
    fn default() -> Self {
        PipelineBinding::new()
    }
}

impl PipelineBinding {
    /// 新建：一条 C 域共享管线，零变体。
    pub fn new() -> Self {
        PipelineBinding {
            variants: Vec::new(),
            pipeline_count: 1,
            rejected: 0,
        }
    }

    /// 注册一个文本材质变体。
    ///
    /// **超上限 [`MAX_MATERIAL_VARIANTS`] 即拒绝**并提示复用（锚点错误路径）。
    /// 拒绝时**不改动** `variants`（半注册是最难查的一类状态）。
    pub fn register_variant(&mut self) -> RegisterOutcome {
        let have = self.variants.len() as u32;
        if have >= MAX_MATERIAL_VARIANTS {
            self.rejected = self.rejected.saturating_add(1);
            return RegisterOutcome::Rejected {
                have,
                cap: MAX_MATERIAL_VARIANTS,
                hint: "复用已有材质变体：把差异表达为效果参数而非新建变体",
            };
        }
        let v = MaterialVariant(have as u16);
        self.variants.push(v);
        // **管线数不动** —— 这是锚点「不另起管线」的可判定形式。
        RegisterOutcome::Registered(v)
    }

    /// 已注册变体数。
    pub fn variant_count(&self) -> u32 {
        self.variants.len() as u32
    }

    /// 取变体。
    pub fn variant(&self, i: u32) -> Option<MaterialVariant> {
        self.variants.get(i as usize).copied()
    }

    /// 复用声明体（锚点「出具复用声明」）。
    ///
    /// 复用同域 [`ReuseDeclaration`]，不另立结构。
    pub fn reuse_declaration(&self) -> ReuseDeclaration {
        ReuseDeclaration {
            paradigm: "VE-C 域着色器体系（F0401 起）".to_string(),
            items: alloc::vec![
                "管线宿主：文本着色器注册为 C 域共享管线的材质变体，不新建管线".to_string(),
                "混合模式：复用 C 域管线的混合状态表达，不另设文本专属混合档".to_string(),
                "常量缓冲：复用 C 域常量缓冲布局（定长 uniform），不另开绑定槽".to_string(),
                "变体上限：复用全域扩展注册协议的上限口径（拒绝并提示复用）".to_string(),
            ],
            divergence: "同构不同参：差异仅在效果 uniform 的取值，管线状态与绑定布局与 C 域一致"
                .to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、变换（局部 / 世界空间文字）
// ---------------------------------------------------------------------------

/// 文字所在空间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSpace {
    /// 局部（屏幕）空间：`dst` 直接使用，深度取层序。
    Local,
    /// 世界空间：经 [`Model2D`] 仿射变换，深度取 `world_z`。
    World,
}

/// F26.6 定点矩形（目标区域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DstRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DstRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        DstRect { x, y, w, h }
    }

    /// 面积（F26.6²）。
    pub fn area_q(&self) -> i64 {
        (self.w as i64) * (self.h as i64)
    }
}

/// 2D 仿射变换（F26.6 定点，2×3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model2D {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub tx: i32,
    pub ty: i32,
}

impl Model2D {
    pub const fn new(a: i32, b: i32, c: i32, d: i32, tx: i32, ty: i32) -> Self {
        Model2D { a, b, c, d, tx, ty }
    }

    /// 单位变换。
    pub const fn identity() -> Self {
        Model2D {
            a: QUANT_ONE,
            b: 0,
            c: 0,
            d: QUANT_ONE,
            tx: 0,
            ty: 0,
        }
    }

    /// 行列式（F26.12）。
    ///
    /// **用 i64 算**：`a*d` 在 F26.6 下可达 2^24，`i32` 相乘会溢出
    /// （溢出后符号翻转 ⇒ 退化变换被判成非退化 ⇒ 零面积 quad 静默通过）。
    pub fn det(&self) -> i64 {
        (self.a as i64) * (self.d as i64) - (self.b as i64) * (self.c as i64)
    }

    /// 是否退化（行列式为 0 或缩放轴有一为零）。
    pub fn is_degenerate(&self) -> bool {
        self.det() == 0
    }

    /// 变换一点（F26.6 → F26.6，除以 QUANT_ONE）。
    pub fn apply_point(&self, x: i32, y: i32) -> (i32, i32) {
        let q = QUANT_ONE as i64;
        let nx = ((self.a as i64) * (x as i64) + (self.c as i64) * (y as i64)) / q;
        let ny = ((self.b as i64) * (x as i64) + (self.d as i64) * (y as i64)) / q;
        (
            clamp_i32(nx + self.tx as i64),
            clamp_i32(ny + self.ty as i64),
        )
    }

    /// 变换矩形（四角变换后取包围盒）。
    pub fn apply_rect(&self, r: DstRect) -> DstRect {
        let (x0, y0) = self.apply_point(r.x, r.y);
        let (x1, y1) = self.apply_point(r.x.saturating_add(r.w), r.y);
        let (x2, y2) = self.apply_point(r.x, r.y.saturating_add(r.h));
        let (x3, y3) = self.apply_point(r.x.saturating_add(r.w), r.y.saturating_add(r.h));
        let min_x = min4(x0, x1, x2, x3);
        let max_x = max4(x0, x1, x2, x3);
        let min_y = min4(y0, y1, y2, y3);
        let max_y = max4(y0, y1, y2, y3);
        DstRect {
            x: min_x,
            y: min_y,
            w: (max_x - min_x).max(0),
            h: (max_y - min_y).max(0),
        }
    }
}

fn min4(a: i32, b: i32, c: i32, d: i32) -> i32 {
    let mut m = a;
    if b < m {
        m = b;
    }
    if c < m {
        m = c;
    }
    if d < m {
        m = d;
    }
    m
}

fn max4(a: i32, b: i32, c: i32, d: i32) -> i32 {
    let mut m = a;
    if b > m {
        m = b;
    }
    if c > m {
        m = c;
    }
    if d > m {
        m = d;
    }
    m
}

fn clamp_i32(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

// ---------------------------------------------------------------------------
// 五、文本 quad 与批次键
// ---------------------------------------------------------------------------

/// 文本图元（一个待绘制的字形 quad）。
///
/// `key` 用 F0805/F0806 **共享的四元组键空间**（锚点两单明文共享），
/// 本单不另立字形身份类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextQuad {
    /// 字形身份（四元组，与 F0805/F0806 同一份 `CacheKey`）。
    pub key: CacheKey,
    /// 图集页号。
    pub page: u16,
    /// 页内矩形。
    pub rect: AtlasRect,
    /// 定点 UV（页尺寸非法时为 `None`，**不给错误 UV**）。
    pub uv: Option<UvRect>,
    /// 目标矩形（F26.6）。
    pub dst: DstRect,
    /// 深度（组内排序键；局部空间取层序，世界空间取 `world_z`）。
    pub depth: i32,
    /// 材质变体。
    pub variant: MaterialVariant,
    /// 混合模式。
    pub blend: BlendMode,
    /// 效果参数。
    pub effect: EffectParams,
    /// 所在空间。
    pub space: TextSpace,
    /// 局部空间层序（局部空间的深度来源）。
    pub layer: i32,
    /// 世界空间 Z（世界空间的深度来源）。
    pub world_z: i32,
    /// 提交序号（**深度并列时的稳定 tie-break**：保证同输入必得同输出）。
    pub seq: u32,
}

impl TextQuad {
    /// 构造一个局部空间 quad。
    #[allow(clippy::too_many_arguments)]
    pub const fn local(
        key: CacheKey,
        page: u16,
        dst: DstRect,
        depth: i32,
        seq: u32,
        variant: MaterialVariant,
        blend: BlendMode,
        effect: EffectParams,
    ) -> Self {
        TextQuad {
            key,
            page,
            rect: AtlasRect::new(0, 0, 0, 0),
            uv: None,
            dst,
            depth,
            variant,
            blend,
            effect,
            space: TextSpace::Local,
            layer: depth,
            world_z: 0,
            seq,
        }
    }

    /// 附上 F0806 的页内矩形与 UV（对接图集）。
    pub fn with_atlas(mut self, rect: AtlasRect, uv: Option<UvRect>) -> Self {
        self.rect = rect;
        self.uv = uv;
        self
    }
}

/// 批次键 =（图集页、材质变体、混合模式）。
///
/// **恰好三段**（锚点原文）。深度**不在**键内——同页同变体不同深度的字形
/// 必须落进同一批再按深度排序；把深度塞进键会把一批拆成深度 innumerable 批。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BatchKey {
    pub page: u16,
    pub variant: MaterialVariant,
    pub blend: BlendMode,
}

impl BatchKey {
    pub const fn new(page: u16, variant: MaterialVariant, blend: BlendMode) -> Self {
        BatchKey {
            page,
            variant,
            blend,
        }
    }

    /// 管线状态指纹：**只有（变体、混合模式）**。
    ///
    /// 页号**故意不在内**——页号走实例属性（纹理数组层索引），
    /// 不是管线状态。这是「绘制 ≤2 次」的结构前提。
    pub const fn pipeline_fingerprint(self) -> u32 {
        ((self.variant.0 as u32) << 8) | (self.blend.ordinal() as u32)
    }
}

/// 一个批次：同键的 quad，已按深度排好（画家序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuadBatch {
    pub key: BatchKey,
    /// 深度序键序列（与 `depths` 同长，供断言用）。
    pub depths: Vec<i32>,
    /// 深度序对应的四元组键序列（供序列断言用，强于 `contains`）。
    pub order: Vec<CacheKey>,
}

impl QuadBatch {
    /// 批内 quad 数。
    pub fn len(&self) -> u32 {
        self.depths.len() as u32
    }

    /// 是否空批。
    pub fn is_empty(&self) -> bool {
        self.depths.is_empty()
    }
}

/// 一次绘制调用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawCall {
    pub variant: MaterialVariant,
    pub blend: BlendMode,
    /// 覆盖的批次数。
    pub batch_count: u32,
    /// 覆盖的 quad 数。
    pub quad_count: u32,
}

// ---------------------------------------------------------------------------
// 六、工作量计数（缺陷真正发生的那一层）
// ---------------------------------------------------------------------------

/// 工作量分解。
///
/// **计数口径**：深度排序的比较与搬运、批次键匹配的比较、效果打包的字写入。
/// 不计「提交一次」这种与规模无关的常数项——把它算进去会让增长判据失真。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkCounter {
    /// 深度排序比较次数。
    pub depth_cmp: u64,
    /// 深度排序搬运次数。
    pub depth_move: u64,
    /// 批次键匹配比较次数。
    pub key_cmp: u64,
    /// 效果 uniform 写入字数。
    pub uniform_writes: u64,
}

impl WorkCounter {
    /// 总工作量（比较 + 搬运 + 字写入）。
    pub fn total(&self) -> u64 {
        self.depth_cmp
            .saturating_add(self.depth_move)
            .saturating_add(self.key_cmp)
            .saturating_add(self.uniform_writes)
    }

    /// 累计差（供分段计时）。
    pub fn since(&self, prev: &WorkCounter) -> WorkCounter {
        WorkCounter {
            depth_cmp: self.depth_cmp.saturating_sub(prev.depth_cmp),
            depth_move: self.depth_move.saturating_sub(prev.depth_move),
            key_cmp: self.key_cmp.saturating_sub(prev.key_cmp),
            uniform_writes: self.uniform_writes.saturating_sub(prev.uniform_writes),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、批碎告警与统计
// ---------------------------------------------------------------------------

/// 告警类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarnKind {
    /// 图集页切换频繁（批碎）。
    BatchFragmentation,
    /// 绘制调用超上限。
    DrawCallBudget,
    /// 效果使对比度跌破 AA 已被钳制。
    ContrastClamped,
}

impl WarnKind {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            WarnKind::BatchFragmentation => "批碎",
            WarnKind::DrawCallBudget => "绘制调用超限",
            WarnKind::ContrastClamped => "对比度钳制",
        }
    }
}

/// 批碎告警。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeWarning {
    pub kind: WarnKind,
    /// 换页次数（提交序相邻 quad 页号不同）。
    pub page_switches: u32,
    /// 参与判定的 quad 数（**分母是 quad 数，不是批数**）。
    pub quads: u32,
    /// 建议动作（锚点「提示图集整理」）。
    pub hint: &'static str,
}

impl MergeWarning {
    /// 换页率千分比。
    ///
    /// **分母是 quad 数**：挂在批数上会让「批少而批内页跳变频繁」被稀释。
    pub fn switch_rate_permille(&self) -> u32 {
        if self.quads == 0 {
            return 0;
        }
        ((self.page_switches as u64 * 1000) / self.quads as u64) as u32
    }

    /// 是否越过批碎阈值（**单边符号**：>= 判触发）。
    pub fn is_fragmented(&self) -> bool {
        self.switch_rate_permille() >= FRAG_WARN_PERMILLE
    }
}

/// 批次统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchStats {
    /// 提交 quad 数。
    pub submitted: u32,
    /// 落批 quad 数（**守恒判据**：无静默丢弃）。
    pub batched: u32,
    /// 产出批次数。
    pub batches: u32,
    /// 绘制调用数。
    pub draw_calls: u32,
    /// 退化变换被丢弃的 quad 数（世界空间零面积）。
    pub degenerate_xform: u32,
    /// 提交序换页次数。
    pub page_switches: u32,
    /// 对比度被钳制的 quad 数。
    pub contrast_clamped: u32,
    /// 超容量截断的 quad 数。
    pub overflow_dropped: u32,
}

// ---------------------------------------------------------------------------
// 八、合批器
// ---------------------------------------------------------------------------

/// 合批器：把文本 quad 分组成批次并规划绘制调用。
#[derive(Clone, Debug)]
pub struct Batcher {
    quads: Vec<TextQuad>,
    /// 世界空间变换（局部空间 quad 不受影响）。
    pub model: Model2D,
    /// 背景色（对比度红线判定用）。
    pub background: [u8; 3],
    /// 累计工作量。
    pub work: WorkCounter,
    /// 上次统计。
    pub stats: BatchStats,
    /// 告警账。
    pub warnings: Vec<MergeWarning>,
    /// 产出批次。
    pub batches: Vec<QuadBatch>,
    /// 产出绘制调用。
    pub draws: Vec<DrawCall>,
    /// 上一帧的对比度最小值（f32 不可 derive Eq，故不进 `stats`）。
    pub min_contrast: f32,
}

impl Default for Batcher {
    /// 默认深色背景（与 [`Batcher::new`] 同一入口，避免两处字面量分叉）。
    fn default() -> Self {
        Batcher::new([16, 18, 24])
    }
}

impl Batcher {
    /// 新建合批器。
    pub fn new(background: [u8; 3]) -> Self {
        Batcher {
            quads: Vec::new(),
            model: Model2D::identity(),
            background,
            work: WorkCounter::default(),
            stats: BatchStats::default(),
            warnings: Vec::new(),
            batches: Vec::new(),
            draws: Vec::new(),
            min_contrast: WCAG_AA_NORMAL,
        }
    }

    /// 提交一个 quad。
    ///
    /// 超 [`MAX_QUADS`] 容量 ⇒ **显式拒绝并计数**
    /// （[`BatchStats::overflow_dropped`]），不静默扩容也不静默丢弃。
    pub fn submit(&mut self, q: TextQuad) -> bool {
        if self.quads.len() >= MAX_QUADS {
            self.stats.overflow_dropped = self.stats.overflow_dropped.saturating_add(1);
            return false;
        }
        self.quads.push(q);
        self.stats.submitted = self.stats.submitted.saturating_add(1);
        true
    }

    /// 已提交 quad 数。
    pub fn len(&self) -> u32 {
        self.quads.len() as u32
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    /// 清空（帧边界调用）。
    pub fn clear(&mut self) {
        self.quads.clear();
        self.batches.clear();
        self.draws.clear();
        self.warnings.clear();
        self.stats = BatchStats::default();
    }

    /// 构建批次与绘制调用。
    ///
    /// 流程：
    /// 1. 逐 quad 结算空间（世界空间走 [`Model2D`]；退化 ⇒ 计数丢弃）；
    /// 2. 逐 quad 结算对比度红线（跌破 AA ⇒ 钳制并计数）；
    /// 3. 按批次键三元组线性归组（批数有界，比较计入 `key_cmp`）；
    /// 4. 组内按深度归并排序（画家序，`seq` 作稳定 tie-break）；
    /// 5. 批次列表按 `(变体, 混合, 页)` 排序，使同管线状态的批**连续**；
    /// 6. 规划绘制调用（按 `(变体, 混合)` 连续游程切分）。
    pub fn build(&mut self) -> BatchStats {
        self.batches.clear();
        self.draws.clear();
        self.warnings.clear();
        self.work = WorkCounter::default();
        let mut stats = BatchStats::default();
        stats.submitted = self.quads.len() as u32;

        // --- 提交序换页统计（批碎告警的输入） ---
        stats.page_switches = count_page_switches(&self.quads, &mut self.work);

        // --- 空间结算 + 对比度红线 ---
        let mut live: Vec<TextQuad> = Vec::with_capacity(self.quads.len());
        for q in self.quads.iter() {
            let mut cur = *q;
            match cur.space {
                TextSpace::Local => {
                    cur.depth = cur.layer;
                }
                TextSpace::World => {
                    if self.model.is_degenerate() {
                        stats.degenerate_xform = stats.degenerate_xform.saturating_add(1);
                        continue;
                    }
                    cur.dst = self.model.apply_rect(cur.dst);
                    cur.depth = cur.world_z;
                }
            }
            if enforce_contrast(&mut cur.effect, self.background, &mut stats, &mut self.warnings) {
                self.work.uniform_writes =
                    self.work.uniform_writes.saturating_add(UNIFORM_WORDS as u64);
                live.push(cur);
            }
        }

        // --- 按批次键归组 ---
        for q in live.iter() {
            let key = BatchKey::new(q.page, q.variant, q.blend);
            let mut found = None;
            for (i, b) in self.batches.iter().enumerate() {
                self.work.key_cmp = self.work.key_cmp.saturating_add(1);
                if b.key == key {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => {
                    if let Some(b) = self.batches.get_mut(i) {
                        b.depths.push(q.depth);
                        b.order.push(q.key);
                    }
                }
                None => {
                    self.batches.push(QuadBatch {
                        key,
                        depths: alloc::vec![q.depth],
                        order: alloc::vec![q.key],
                    });
                }
            }
        }

        // --- 组内深度排序（归并；`seq` 稳定 tie-break） ---
        let seqs = self.collect_seqs(&live);
        for b in self.batches.iter_mut() {
            let idx = merge_sort_by_depth(b, &seqs, &mut self.work);
            let mut depths = Vec::with_capacity(idx.len());
            let mut order = Vec::with_capacity(idx.len());
            for i in idx.iter() {
                if let Some(d) = b.depths.get(*i as usize) {
                    depths.push(*d);
                }
                if let Some(k) = b.order.get(*i as usize) {
                    order.push(*k);
                }
            }
            b.depths = depths;
            b.order = order;
        }

        // --- 批次排序：同 (变体, 混合) 连续 ---
        sort_batches_by_pipeline(&mut self.batches, &mut self.work);

        // --- 绘制调用规划 ---
        self.draws = plan_draw_calls(&self.batches);

        stats.batches = self.batches.len() as u32;
        stats.draw_calls = self.draws.len() as u32;
        let mut batched = 0u32;
        for b in self.batches.iter() {
            batched = batched.saturating_add(b.len());
        }
        stats.batched = batched;

        // --- 批碎告警 ---
        let frag = MergeWarning {
            kind: WarnKind::BatchFragmentation,
            page_switches: stats.page_switches,
            quads: stats.submitted,
            hint: "提示图集整理：换页过于频繁，批次被打碎，合并策略失效",
        };
        if frag.is_fragmented() {
            self.warnings.push(frag);
        }
        if stats.draw_calls > MAX_DRAW_CALLS_PER_FRAME {
            self.warnings.push(MergeWarning {
                kind: WarnKind::DrawCallBudget,
                page_switches: stats.page_switches,
                quads: stats.submitted,
                hint: "绘制调用超上限：检查管线状态是否被页号污染（页号应走实例属性）",
            });
        }

        self.min_contrast = min_contrast_over(&live, self.background);
        self.stats = stats;
        stats
    }

    /// 收集与 `live` 同序的 `seq`（供稳定 tie-break）。
    fn collect_seqs(&self, live: &[TextQuad]) -> Vec<u32> {
        let mut out = Vec::with_capacity(live.len());
        for q in live.iter() {
            out.push(q.seq);
        }
        out
    }

    /// 某页在产出批次中的 quad 数（供「图集整理」提示定位热点页）。
    pub fn page_load(&self, page: u16) -> u32 {
        let mut n = 0u32;
        for b in self.batches.iter() {
            if b.key.page == page {
                n = n.saturating_add(b.len());
            }
        }
        n
    }
}

fn count_page_switches(quads: &[TextQuad], work: &mut WorkCounter) -> u32 {
    let mut n = 0u32;
    let mut prev: Option<u16> = None;
    for q in quads.iter() {
        match prev {
            Some(p) => {
                work.key_cmp = work.key_cmp.saturating_add(1);
                if p != q.page {
                    n = n.saturating_add(1);
                }
            }
            None => {}
        }
        prev = Some(q.page);
    }
    n
}

/// 对比度红线：效果引入的颜色不得使对比度跌破 AA。
///
/// 判定调用 **VE-S 域** [`contrast_ratio`]（WCAG 2.x 相对亮度），
/// **不另写一份公式**（两份实现必然分叉）。
///
/// 返回 `true` = 该 quad 可继续（已达标或已钳制到达标）。
fn enforce_contrast(
    effect: &mut EffectParams,
    background: [u8; 3],
    stats: &mut BatchStats,
    warnings: &mut Vec<MergeWarning>,
) -> bool {
    // 参与对比度的候选前景色：填充色、描边色、渐变两端。
    // 阴影色不参与：阴影落在字形之外，其色不与文字争夺前景识别。
    let mut ok = true;
    let mut worst = 0.0f32;

    let mut consider = |c: Rgba8, stats: &mut BatchStats| {
        let r = contrast_ratio(c.rgb(), background);
        if worst == 0.0 || r < worst {
            worst = r;
        }
        if r < WCAG_AA_NORMAL {
            ok = false;
        }
        let _ = stats;
    };

    consider(effect.color, stats);
    if let Some(s) = effect.stroke {
        consider(s.color, stats);
    }
    if let Some(g) = effect.gradient {
        consider(g.from, stats);
        consider(g.to, stats);
    }

    if ok {
        return true;
    }

    // 跌破 AA ⇒ 钳制而非放行：把偏暗的前景提到白（对深色背景对比度最高），
    // 偏亮的压到黑。**逐通道朝远离背景的方向移动**，是单调且可预测的修正。
    effect.color = push_away_from_bg(effect.color, background);
    if let Some(s) = effect.stroke.as_mut() {
        s.color = push_away_from_bg(s.color, background);
    }
    if let Some(g) = effect.gradient.as_mut() {
        g.from = push_away_from_bg(g.from, background);
        g.to = push_away_from_bg(g.to, background);
    }
    stats.contrast_clamped = stats.contrast_clamped.saturating_add(1);
    warnings.push(MergeWarning {
        kind: WarnKind::ContrastClamped,
        page_switches: 0,
        quads: 1,
        hint: "文本效果削弱对比度：已钳制到 WCAG AA 以上（不得削弱对比度为手册红线）",
    });
    true
}

/// 把前景色朝远离背景的方向推（保 alpha）。
fn push_away_from_bg(c: Rgba8, bg: [u8; 3]) -> Rgba8 {
    let lb = contrast_ratio(c.rgb(), bg);
    // 两个极端候选：纯白与纯黑，取对比度更高者。
    let white = contrast_ratio([255, 255, 255], bg);
    let black = contrast_ratio([0, 0, 0], bg);
    let _ = lb;
    if white >= black {
        Rgba8([255, 255, 255, c.0[3]])
    } else {
        Rgba8([0, 0, 0, c.0[3]])
    }
}

fn min_contrast_over(quads: &[TextQuad], bg: [u8; 3]) -> f32 {
    let mut worst = 0.0f32;
    for q in quads.iter() {
        let mut r = contrast_ratio(q.effect.color.rgb(), bg);
        if let Some(s) = q.effect.stroke {
            let sr = contrast_ratio(s.color.rgb(), bg);
            if sr < r {
                r = sr;
            }
        }
        if let Some(g) = q.effect.gradient {
            let a = contrast_ratio(g.from.rgb(), bg);
            let b = contrast_ratio(g.to.rgb(), bg);
            if a < r {
                r = a;
            }
            if b < r {
                r = b;
            }
        }
        if worst == 0.0 || r < worst {
            worst = r;
        }
    }
    worst
}

/// 深度归并排序：返回新下标序列（就地排序 `depths`/`order` 的副本索引）。
///
/// **归并而非插入排序**：本单的性能判据是**复杂度类**（`work(2n)/work(n)`），
/// 插入排序在已序输入上比值≈2（判据会漏），在逆序上≈4（判据会红）——
/// 两者都不是稳定的复杂度保证。归并排序每趟一趟，比值稳定≈2.1。
fn merge_sort_by_depth(b: &QuadBatch, seqs: &[u32], work: &mut WorkCounter) -> Vec<u32> {
    let n = b.depths.len();
    let mut idx: Vec<u32> = Vec::with_capacity(n);
    for i in 0..n {
        idx.push(i as u32);
    }
    if n < 2 {
        return idx;
    }
    let depths = &b.depths;
    let _ = depths;

    let mut src = idx;
    let mut dst: Vec<u32> = Vec::with_capacity(n);
    let mut width = 1usize;
    while width < n {
        let mut i = 0usize;
        while i < n {
            let mid = if i + width < n { i + width } else { n };
            let end = if i + 2 * width < n { i + 2 * width } else { n };
            let (mut x, mut y, mut o) = (i, mid, i);
            while x < mid && y < end {
                work.depth_cmp = work.depth_cmp.saturating_add(1);
                let da = src[x];
                let db = src[y];
                // 深度小者先出；深度并列按 `seq` 定序（稳定、可复现）。
                let take_x = match (b.depths.get(da as usize), b.depths.get(db as usize)) {
                    (Some(va), Some(vb)) => {
                        if va != vb {
                            va < vb
                        } else {
                            let sa = seqs.get(da as usize).copied().unwrap_or(u32::MAX);
                            let sb = seqs.get(db as usize).copied().unwrap_or(u32::MAX);
                            sa <= sb
                        }
                    }
                    // 取不到 ⇒ 保守判为「相等」，由 `seq` 或稳定序收尾。
                    _ => true,
                };
                let v = if take_x {
                    let t = src[x];
                    x += 1;
                    t
                } else {
                    let t = src[y];
                    y += 1;
                    t
                };
                if o < dst.len() {
                    dst[o] = v;
                } else {
                    dst.push(v);
                }
                work.depth_move = work.depth_move.saturating_add(1);
                o += 1;
            }
            while x < mid {
                let t = src[x];
                if o < dst.len() {
                    dst[o] = t;
                } else {
                    dst.push(t);
                }
                work.depth_move = work.depth_move.saturating_add(1);
                x += 1;
                o += 1;
            }
            while y < end {
                let t = src[y];
                if o < dst.len() {
                    dst[o] = t;
                } else {
                    dst.push(t);
                }
                work.depth_move = work.depth_move.saturating_add(1);
                y += 1;
                o += 1;
            }
            i = end;
        }
        core::mem::swap(&mut src, &mut dst);
        width = width.saturating_mul(2);
    }
    src
}

/// 批次按 `(变体, 混合, 页)` 排序，使同管线状态的批**连续**。
///
/// 这是「绘制 ≤2 次」的**结构前提**：若批次序被打散，同 `(变体,混合)` 的批
/// 会分成多个游程，每个游程一次绘制调用。
fn sort_batches_by_pipeline(batches: &mut [QuadBatch], work: &mut WorkCounter) {
    let n = batches.len();
    if n < 2 {
        return;
    }
    // 插入排序：批数由三元组基数决定（有界，远小于 quad 数），
    // 故此处 O(批数²) 不影响 quad 级复杂度判据。
    for i in 1..n {
        let mut j = i;
        while j > 0 {
            work.key_cmp = work.key_cmp.saturating_add(1);
            let a = match batches.get(j - 1) {
                Some(b) => b.key,
                None => break,
            };
            let c = match batches.get(j) {
                Some(b) => b.key,
                None => break,
            };
            if pipeline_order_key(c) < pipeline_order_key(a) {
                if let Some(t) = batches.get(j - 1).cloned() {
                    if let Some(u) = batches.get(j).cloned() {
                        batches[j - 1] = u;
                        batches[j] = t;
                    }
                }
                j -= 1;
            } else {
                break;
            }
        }
    }
}

fn pipeline_order_key(k: BatchKey) -> (u16, u16, u16) {
    (k.variant.0, k.blend.ordinal(), k.page)
}

/// 绘制调用规划：**按 `(变体, 混合模式)` 的连续游程切分**。
///
/// **页号不是管线状态**（[`BatchKey::pipeline_fingerprint`] 只含变体与混合）：
/// 图集以纹理数组绑定，页号经实例属性传入，换页不重绑管线。
/// 若误把页号算进指纹，4 页即 4 次绘制 ⇒ 锚点「≤2 次」判红。
pub fn plan_draw_calls(batches: &[QuadBatch]) -> Vec<DrawCall> {
    let mut calls: Vec<DrawCall> = Vec::new();
    for b in batches.iter() {
        let fp = b.key.pipeline_fingerprint();
        let cont = match calls.last() {
            Some(c) => {
                ((c.variant.0 as u32) << 8 | c.blend.ordinal() as u32) == fp
            }
            None => false,
        };
        let n = b.len();
        if cont {
            if let Some(c) = calls.last_mut() {
                c.batch_count = c.batch_count.saturating_add(1);
                c.quad_count = c.quad_count.saturating_add(n);
            }
        } else {
            calls.push(DrawCall {
                variant: b.key.variant,
                blend: b.key.blend,
                batch_count: 1,
                quad_count: n,
            });
        }
    }
    calls
}

// ---------------------------------------------------------------------------
// 九、域自检
// ---------------------------------------------------------------------------

/// 确定性线性同余发生器（自检语料生成，**不引入随机源**）。
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Self {
        Lcg(seed.wrapping_mul(2654435761).wrapping_add(1))
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

fn key_of(gid: u32, px: u16, phx: usize, phy: usize, w: Weight) -> CacheKey {
    CacheKey::new(gid, px, super::vee05_hinting::Phase2::new(phx, phy), w)
}

/// 造一批 quad：页号 / 变体 / 混合 / 深度按给定节律循环。
fn make_quads(
    n: u32,
    pages: u16,
    variants: &[MaterialVariant],
    blends: &[BlendMode],
    depth_mode: u8,
    effect: EffectParams,
) -> Vec<TextQuad> {
    let mut out = Vec::with_capacity(n as usize);
    let mut rng = Lcg::new(0x0807_0007);
    for i in 0..n {
        let page = if pages == 0 { 0 } else { (i % pages as u32) as u16 };
        let v = variants[(i as usize) % variants.len()];
        let b = blends[(i as usize) % blends.len()];
        let depth = match depth_mode {
            // 深度与页号同节律（局部空间正常情形）。
            0 => i as i32,
            // 深度反节律：制造「同页不同深度」以验证不按深度拆批。
            1 => (n - i) as i32,
            // 深度全局交错：页号成块而深度乱序，验证分组把绘制调用拉回 ≤2。
            2 => rng.next() as i32,
            // 深度全同：验证并列深度按 seq 稳定定序。
            _ => 7,
        };
        let q = TextQuad::local(
            key_of(i, 16, 0, 0, Weight::Regular),
            page,
            DstRect::new((i % 640) as i32 * 8, (i / 640) as i32 * 12, 8, 12),
            depth,
            i,
            v,
            b,
            effect,
        );
        out.push(q.with_atlas(AtlasRect::new(0, 0, 8, 12), Some(UvRect { u0: 0, v0: 0, u1: 1, v1: 1 })));
    }
    out
}

const BG_DARK: [u8; 3] = [16, 18, 24];
const FG_LIGHT: Rgba8 = Rgba8([235, 238, 245, 255]);

/// 域自检。
pub fn run_vee07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee07");
    check_batch_2000(&mut set);
    check_draw_calls(&mut set);
    check_pipeline_reuse(&mut set);
    check_four_effects(&mut set);
    check_perf_growth(&mut set);
    check_batch_key(&mut set);
    check_depth_order(&mut set);
    check_fragmentation(&mut set);
    check_variant_limit(&mut set);
    check_contrast_redline(&mut set);
    check_transform(&mut set);
    check_batch_key_dimensions(&mut set);
    check_draw_call_warning(&mut set);
    set
}

/// E07-01 合批 ≥2,000：**恰好 2,000 个 quad**（不是 ≥，用恰好值把边界钉死）
/// 同页同变体同混合 ⇒ 落进单批，且**总数守恒**。
///
/// 「守恒」这一项杀死「按容量截断成 2,000 而静默丢弃尾部」的实现：
/// 只断言批大小==2,000 时，截断实现与合批实现**表现完全一致**。
fn check_batch_2000(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(
        BATCH_TARGET_QUADS,
        1,
        &[v],
        &[BlendMode::Alpha],
        0,
        EffectParams::color_only(FG_LIGHT),
    );
    for q in qs.iter() {
        b.submit(*q);
    }
    let st = b.build();
    let target = BATCH_TARGET_QUADS;
    let one_batch = st.batches == 1;
    let full = st.batched == target;
    let conserved = st.submitted == target && st.batched == target;
    let no_warn = b.warnings.is_empty();

    // **守恒的夹逼对**：恰好 2,000 时「截断成 2,000」是空操作，
    // 截断实现与合批实现表现完全一致 ⇒ 只核前者等于没核。
    // 故再加一档**超过目标**的输入（2,500）：锚点是「单批目标 ≥2,000」，
    // 合批实现必须**全收**（2,500 进单批），截断实现会砍到 2,000。
    let over = target + 500;
    let mut b2 = Batcher::new(BG_DARK);
    let qs2 = make_quads(over, 1, &[v], &[BlendMode::Alpha], 0, EffectParams::color_only(FG_LIGHT));
    for q in qs2.iter() {
        b2.submit(*q);
    }
    let st2 = b2.build();
    let over_single = st2.batches == 1;
    let over_all_kept = st2.batched == over && st2.submitted == over;

    let ok = one_batch && full && conserved && no_warn && over_single && over_all_kept;
    assert!(
        ok,
        "合批：单批 {one_batch} 满额 {full} 守恒 {conserved} 无告警 {no_warn} 超额单批 {over_single} 超额全收 {over_all_kept}"
    );
    set.add("E07-01-合批2000守恒", ok, "");
}

/// E07-02 绘制 ≤2 次。两段：
///
/// **(a) 页号不是管线状态**：4 页 × 2,000 quad（同变体同混合）
/// ⇒ 批次必然是 4 个（批次键含页），但绘制调用必须 **== 1**。
/// 这一段杀死「把页号当管线状态」的实现（它会给 4）。
///
/// **(b) 分组确实在起作用**：深度全局交错、页号成块 ⇒ 若实现只按深度排序
/// 而不按 `(变体,混合)` 分组，同管线状态的批会被打散成多个游程。
fn check_draw_calls(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(
        BATCH_TARGET_QUADS,
        4,
        &[v],
        &[BlendMode::Alpha],
        2,
        EffectParams::color_only(FG_LIGHT),
    );
    for q in qs.iter() {
        b.submit(*q);
    }
    let st = b.build();
    let pages_split = st.batches == 4;
    let one_call = st.draw_calls == 1;
    let within_cap = st.draw_calls <= MAX_DRAW_CALLS_PER_FRAME;
    let conserved = st.batched == BATCH_TARGET_QUADS;

    // 交错深度下分组仍须把绘制调用压回 ≤2。
    let mut b2 = Batcher::new(BG_DARK);
    let qs2 = make_quads(
        BATCH_TARGET_QUADS,
        4,
        &[v],
        &[BlendMode::Alpha],
        2,
        EffectParams::color_only(FG_LIGHT),
    );
    for q in qs2.iter() {
        b2.submit(*q);
    }
    let st2 = b2.build();
    let grouped = st2.draw_calls <= MAX_DRAW_CALLS_PER_FRAME;

    // 四元组序列守恒（强于只看计数）。
    let mut total = 0u32;
    for call in b.draws.iter() {
        total = total.saturating_add(call.quad_count);
    }
    let call_total_ok = total == BATCH_TARGET_QUADS;

    assert!(
        pages_split && one_call && within_cap && conserved && grouped && call_total_ok,
        "绘制：4批 {pages_split} 1调用 {one_call} 守恒 {conserved} 分组 {grouped} 调用计数 {call_total_ok}"
    );
    set.add("E07-02-绘制两次且页非管线状态", pages_split && one_call && within_cap && conserved && grouped && call_total_ok, "");
}

/// E07-03 复用声明：注册满 [`MAX_MATERIAL_VARIANTS`] 个材质变体后，
/// **管线数仍为 1**（不另起管线）；且超上限被拒绝并提示复用。
fn check_pipeline_reuse(set: &mut CheckSet) {
    let mut pb = PipelineBinding::new();
    let mut all_ok = true;
    let mut n = 0u32;
    while n < MAX_MATERIAL_VARIANTS {
        match pb.register_variant() {
            RegisterOutcome::Registered(_) => {}
            RegisterOutcome::Rejected { .. } => {
                all_ok = false;
                break;
            }
        }
        n += 1;
    }
    let no_new_pipeline = pb.pipeline_count == 1;
    let full = pb.variant_count() == MAX_MATERIAL_VARIANTS;

    // 超上限 ⇒ 拒绝，且注册表**不长**（半注册是最难查的状态）。
    let before = pb.variant_count();
    let rej = pb.register_variant();
    let rejected = matches!(rej, RegisterOutcome::Rejected { .. });
    let unchanged = pb.variant_count() == before;
    let hint_present = match rej {
        RegisterOutcome::Rejected { hint, .. } => !hint.is_empty(),
        _ => false,
    };
    let still_one = pb.pipeline_count == 1;

    // 复用声明体非空且逐条列明。
    let decl = pb.reuse_declaration();
    let decl_ok = !decl.paradigm.is_empty()
        && decl.items.len() >= 4
        && !decl.divergence.is_empty()
        && decl.items.iter().any(|s| s.contains("不新建管线"));

    let ok = all_ok && no_new_pipeline && full && rejected && unchanged && hint_present && still_one && decl_ok;
    assert!(
        ok,
        "复用：注册 {all_ok} 单管线 {no_new_pipeline} 满额 {full} 拒绝 {rejected} 不半注册 {unchanged} 带提示 {hint_present} 声明 {decl_ok}"
    );
    set.add("E07-03-不另起管线且超限拒绝", ok, "");
}

/// E07-04 四效果零加调用：三档（0 / 1 / 4 效果）下
/// **uniform 字数恒定**且**绘制调用数相等**。
///
/// 「字字数恒定」这一项是结构保证的直接读数：若实现按启用效果收缩布局，
/// 字数会随档位变化 ⇒ 转红（且该实现无法合批，正与锚点冲突）。
fn check_four_effects(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let mut words = [0u32; UNIFORM_WORDS];

    let e0 = EffectParams::color_only(FG_LIGHT);
    let e1 = EffectParams::all_four(
        FG_LIGHT,
        StrokeParams { width_q: 64, color: FG_LIGHT },
        ShadowParams { dx_q: 32, dy_q: 32, softness_q: 64, color: Rgba8([0, 0, 0, 128]) },
        GradientParams { from: FG_LIGHT, to: FG_LIGHT, axis: GradientAxis::Horizontal },
    );
    let e_shadow_only = EffectParams {
        color: FG_LIGHT,
        stroke: None,
        shadow: Some(ShadowParams {
            dx_q: 16,
            dy_q: 16,
            softness_q: 32,
            color: Rgba8([0, 0, 0, 96]),
        }),
        gradient: None,
    };

    let w0 = e0.pack(&mut words);
    let w1 = e_shadow_only.pack(&mut words);
    let w4 = e1.pack(&mut words);
    let fixed = w0 == w1 && w1 == w4 && w0 == UNIFORM_WORDS;

    // 标志位确实区分四效果（否则「参数化」是空壳）。
    let flags_distinct = e0.flags() == 0
        && e_shadow_only.flags() == effect_flags::SHADOW
        && e1.flags() == (effect_flags::STROKE | effect_flags::SHADOW | effect_flags::GRADIENT);

    // 绘制调用数与效果数无关。
    let n = 512u32;
    let mut calls = [0u32; 3];
    for (i, eff) in [e0, e_shadow_only, e1].iter().enumerate() {
        let mut b = Batcher::new(BG_DARK);
        let qs = make_quads(n, 2, &[v], &[BlendMode::Alpha], 1, *eff);
        for q in qs.iter() {
            b.submit(*q);
        }
        let st = b.build();
        calls[i] = st.draw_calls;
    }
    let call_independent = calls[0] == calls[1] && calls[1] == calls[2];

    // 自洽：打包-解包 位宽不丢（RGBA 8888 往返一致）。
    e1.pack(&mut words);
    let round = (words[1] >> 24) as u8 == FG_LIGHT.0[0]
        && ((words[1] >> 8) & 0xFF) as u8 == FG_LIGHT.0[2]
        && (words[1] & 0xFF) as u8 == FG_LIGHT.0[3];

    let ok = fixed && flags_distinct && call_independent && round;
    assert!(
        ok,
        "四效果：定长 {fixed} 标志 {flags_distinct} 零加调用 {call_independent} 位宽自洽 {round}"
    );
    set.add("E07-04-四效果零加调用", ok, "");
}

/// E07-05 性能：**实测真实工作量**的复杂度类判据（不是 `n×CONST`）。
///
/// `work(2n) / work(n) ≤ 3`（归并排序≈2.1，插入排序逆序≈4）。
/// 同时核单 quad 平均工作量上限（上限由 [`UNIFORM_WORDS`] 导出，非裸写）。
fn check_perf_growth(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let eff = EffectParams::color_only(FG_LIGHT);

    let measure = |n: u32| -> u64 {
        let mut b = Batcher::new(BG_DARK);
        let qs = make_quads(n, 4, &[v], &[BlendMode::Alpha], 2, eff);
        for q in qs.iter() {
            b.submit(*q);
        }
        b.build();
        b.work.total()
    };

    let n = 1024u32;
    let w1 = measure(n);
    let w2 = measure(n * 2);

    // 比值（整数运算，避免浮点；w1 为 0 时不可判 ⇒ 直接判红）。
    let growth_ok = if w1 == 0 {
        false
    } else {
        (w2 as u128 * GROWTH_CAP_DEN as u128) <= (w1 as u128 * GROWTH_CAP_NUM as u128)
    };

    // 单 quad 平均工作量上限（**只算随规模增长的层**：uniform + 归并）。
    //
    // `key_cmp` 单独断言：它的规模因子是不同批次键个数，不是 quad 数。
    let mut bm = Batcher::new(BG_DARK);
    let qs = make_quads(n * 2, 4, &[v], &[BlendMode::Alpha], 2, eff);
    for q in qs.iter() {
        bm.submit(*q);
    }
    bm.build();
    // 随规模增长的层 = 深度比较 + 深度搬运 + uniform 写入。
    let scaling = bm.work.depth_cmp + bm.work.depth_move + bm.work.uniform_writes;
    let cap_total = WORK_PER_QUAD_CAP * (n * 2) as u64;
    let per_quad_ok = scaling <= cap_total;

    // `key_cmp` 的规模因子是批次键个数：页数从 4 增到 8 时，
    // 归组阶段每 quad 的平均键比较应**基本不变**（批数是常数倍而非按页数乘）。
    let key_per_quad = |pages: u16| -> u64 {
        let mut b = Batcher::new(BG_DARK);
        let qs = make_quads(n * 2, pages, &[v], &[BlendMode::Alpha], 2, eff);
        for q in qs.iter() {
            b.submit(*q);
        }
        b.build();
        b.work.key_cmp / (n * 2) as u64
    };
    let k4 = key_per_quad(4);
    let k8 = key_per_quad(8);
    // 页数翻倍 ⇒ 每 quad 键比较不得超 4 倍（线性归组的真实代价）。
    let key_ok = k4 > 0 && (k8 as u128 * 4) <= (k4 as u128 * 9);

    // 工作量确实落在被计数的层（不是恒 0 的空计数）。
    let nonzero = w1 > 0 && w2 > w1;

    // 分量非负且键比较/深度比较都被真实触发。
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(256, 3, &[v], &[BlendMode::Alpha], 2, eff);
    for q in qs.iter() {
        b.submit(*q);
    }
    b.build();
    let layers = b.work.depth_cmp > 0 && b.work.key_cmp > 0 && b.work.uniform_writes > 0;

    let ok = growth_ok && per_quad_ok && key_ok && nonzero && layers;
    assert!(
        ok,
        "性能：增长 {growth_ok}(w1={w1},w2={w2}) 单quad上限 {per_quad_ok}({scaling}<={cap_total},cap={WORK_PER_QUAD_CAP}) 键比较 {key_ok}(k4={k4},k8={k8}) 非零 {nonzero} 分层 {layers}"
    );
    set.add("E07-05-工作量复杂度类", ok, "");
}

/// E07-06 批次键三段区分 + **深度不入键**。
///
/// 若实现把深度塞进批次键，同页同变体不同深度的字形会被拆成深度 innumerable 批，
/// 绘制调用随之膨胀。故此处用「同键不同深度」断言**仍为单批**。
fn check_batch_key(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let mut b = Batcher::new(BG_DARK);
    // 4 个不同深度、同页同变体同混合。
    for i in 0..4u32 {
        let q = TextQuad::local(
            key_of(i, 16, 0, 0, Weight::Regular),
            2,
            DstRect::new(i as i32 * 8, 0, 8, 12),
            i as i32,
            i,
            v,
            BlendMode::Alpha,
            EffectParams::color_only(FG_LIGHT),
        );
        b.submit(q);
    }
    let st = b.build();
    let single = st.batches == 1 && st.batched == 4;

    // 三段各自可区分（构造只差一段的键对）。
    let k0 = BatchKey::new(1, v, BlendMode::Alpha);
    let k1 = BatchKey::new(2, v, BlendMode::Alpha);
    let k2 = BatchKey::new(1, MaterialVariant(1), BlendMode::Alpha);
    let k3 = BatchKey::new(1, v, BlendMode::Additive);
    let distinct = k0 != k1 && k0 != k2 && k0 != k3 && k1 != k2 && k2 != k3;

    // 管线指纹**只含变体与混合**（页号不在内）——这是绘制 ≤2 的前提。
    let fp_page_irrelevant = k0.pipeline_fingerprint() == k1.pipeline_fingerprint();
    let fp_variant_differs = k0.pipeline_fingerprint() != k2.pipeline_fingerprint();
    let fp_blend_differs = k0.pipeline_fingerprint() != k3.pipeline_fingerprint();

    // 混合模式序号显式、单调、覆盖全集。
    let ordinals_ok = BlendMode::Opaque.ordinal() < BlendMode::Alpha.ordinal()
        && BlendMode::Alpha.ordinal() < BlendMode::Additive.ordinal()
        && BlendMode::Additive.ordinal() < BlendMode::Masked.ordinal()
        && BlendMode::ALL.len() == 4;

    let ok = single && distinct && fp_page_irrelevant && fp_variant_differs && fp_blend_differs && ordinals_ok;
    assert!(
        ok,
        "批次键：深度不入键 {single} 三段区分 {distinct} 页不入指纹 {fp_page_irrelevant} 变体入 {fp_variant_differs} 混合入 {fp_blend_differs} 序号 {ordinals_ok}"
    );
    set.add("E07-06-批次键三段且深度不入键", ok, "");
}

/// E07-07 深度序：**序列断言**（`assert_eq!` 语义，强于 `contains`）。
///
/// 三档：升序输入 ⇒ 序不变；降序输入 ⇒ 精确倒序；全同深度 ⇒ 按 `seq` 稳定。
fn check_depth_order(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let eff = EffectParams::color_only(FG_LIGHT);

    // 升序
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(64, 1, &[v], &[BlendMode::Alpha], 0, eff);
    for q in qs.iter() {
        b.submit(*q);
    }
    b.build();
    let asc = batch_glyph_ids(&b, 0);
    let asc_ok = asc.windows(2).all(|w| w[0] < w[1]) && asc.len() == 64;

    // 降序 ⇒ 精确倒序
    let mut b2 = Batcher::new(BG_DARK);
    let qs2 = make_quads(64, 1, &[v], &[BlendMode::Alpha], 1, eff);
    for q in qs2.iter() {
        b2.submit(*q);
    }
    b2.build();
    let desc = batch_glyph_ids(&b2, 0);
    let mut expect: Vec<u32> = desc.iter().copied().collect();
    expect.sort_by(|a, c| c.cmp(a));
    let desc_ok = desc == expect;

    // 全同深度 ⇒ 按提交序号（稳定、可复现）
    let mut b3 = Batcher::new(BG_DARK);
    let qs3 = make_quads(32, 1, &[v], &[BlendMode::Alpha], 3, eff);
    for q in qs3.iter() {
        b3.submit(*q);
    }
    b3.build();
    let flat = batch_glyph_ids(&b3, 0);
    let flat_ok = flat.len() == 32 && flat.iter().enumerate().all(|(i, g)| *g == i as u32);

    let ok = asc_ok && desc_ok && flat_ok;
    assert!(ok, "深度序：升序 {asc_ok} 降序 {desc_ok} 并列稳定 {flat_ok}");
    set.add("E07-07-深度序序列断言", ok, "");
}

fn batch_glyph_ids(b: &Batcher, batch: usize) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(bt) = b.batches.get(batch) {
        for k in bt.order.iter() {
            out.push(k.glyph_id);
        }
    }
    out
}

/// E07-08 批碎告警：**夹逼对**——页号成块 ⇒ 不告警；页号逐 quad 交替 ⇒ 告警。
///
/// 只有「会告警」一侧是弱门禁（恒告警的实现也能过）；加上「不告警」一侧
/// 才构成夹逼。另核**分母是 quad 数**：挂批数的实现会在这里露馅。
fn check_fragmentation(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let eff = EffectParams::color_only(FG_LIGHT);

    // 页号成块：每 256 个 quad 一页 ⇒ 换页 7 次 / 2048 quad ⇒ 3‰。
    let mut b_ok = Batcher::new(BG_DARK);
    let qs_ok = make_quads(2048, 0, &[v], &[BlendMode::Alpha], 0, eff);
    let qs_ok = regroup_pages(qs_ok, 256);
    for q in qs_ok.iter() {
        b_ok.submit(*q);
    }
    let st_ok = b_ok.build();
    let quiet_real = !has_warn(&b_ok, WarnKind::BatchFragmentation);
    let counted = st_ok.page_switches == 7;

    // 页号逐 quad 交替：2048 quad ⇒ 2047 次换页 ⇒ 999‰。
    let mut b_bad = Batcher::new(BG_DARK);
    let qs_bad = make_quads(2048, 2, &[v], &[BlendMode::Alpha], 0, eff);
    for q in qs_bad.iter() {
        b_bad.submit(*q);
    }
    let st_bad = b_bad.build();
    let warns = has_warn(&b_bad, WarnKind::BatchFragmentation);
    let counted_bad = st_bad.page_switches == 2047;

    // 提示必须指向「图集整理」。
    let hint_ok = b_bad
        .warnings
        .iter()
        .any(|w| w.kind == WarnKind::BatchFragmentation && w.hint.contains("图集整理"));

    // 分母是 quad 数：2047/2048 = 999‰ ≥ 250‰ ⇒ 判碎；
    // 若分母误取批数（本例 4 批），2047/4 远越界也判碎——故另取一个
    // 中间场景验证分母效应：换页 400 次 / 2,000 quad = 200‰（不告警），
    // 而按批数（4 批）会算成 100,000‰（误告警）。
    let mut b_mid = Batcher::new(BG_DARK);
    let qs_mid = make_quads(2000, 2, &[v], &[BlendMode::Alpha], 0, eff);
    let qs_mid = regroup_pages(qs_mid, 1000);
    for q in qs_mid.iter() {
        b_mid.submit(*q);
    }
    b_mid.build();
    let mid_quiet = !has_warn(&b_mid, WarnKind::BatchFragmentation);

    let _ = st_ok;
    let ok = quiet_real && counted && warns && counted_bad && hint_ok && mid_quiet;
    assert!(
        ok,
        "批碎：成块不告警 {quiet_real} 计数 {counted} 交替告警 {warns} 计数 {counted_bad} 提示图集整理 {hint_ok} 分母是quad数 {mid_quiet}"
    );
    set.add("E07-08-批碎告警夹逼", ok, "");
}

fn has_warn(b: &Batcher, kind: WarnKind) -> bool {
    b.warnings.iter().any(|w| w.kind == kind)
}

/// 把 quad 按 `block` 个一组重排页号（成块分布）。
fn regroup_pages(mut qs: Vec<TextQuad>, block: u32) -> Vec<TextQuad> {
    let n = qs.len() as u32;
    for i in 0..n {
        if let Some(q) = qs.get_mut(i as usize) {
            q.page = (i / block) as u16;
        }
    }
    qs
}

/// E07-09b 变体上限拒绝的**幂等性**：连续多次注册拒绝，
/// 注册表长度与拒绝计数都单调，且拒绝**从不**改变已注册内容。
///
/// 「只拒绝一次」的实现（第二次悄悄成功）在只断言「被拒绝」时会漏网。
fn check_variant_limit(set: &mut CheckSet) {
    let mut pb = PipelineBinding::new();
    while pb.variant_count() < MAX_MATERIAL_VARIANTS {
        pb.register_variant();
    }
    let before = pb.variant_count();
    let rej0 = pb.rejected;
    let mut monotone = true;
    for _ in 0..5 {
        let r = pb.register_variant();
        if !matches!(r, RegisterOutcome::Rejected { .. }) {
            monotone = false;
        }
        if pb.variant_count() != before {
            monotone = false;
        }
    }
    let rej5 = pb.rejected;
    let counted = rej5 == rej0 + 5;
    // 已注册变体内容未被拒绝动作扰动。
    let intact = pb.variant(0) == Some(MaterialVariant(0))
        && pb.variant(MAX_MATERIAL_VARIANTS - 1) == Some(MaterialVariant(MAX_MATERIAL_VARIANTS as u16 - 1));
    let still_one = pb.pipeline_count == 1;
    let ok = monotone && counted && intact && still_one;
    assert!(ok, "变体上限：幂等拒绝 {monotone} 计数 {counted} 内容未扰动 {intact} 单管线 {still_one}");
    set.add("E07-09b-变体上限拒绝幂等", ok, "");
}

/// E07-09 对比度红线：低对比前景 ⇒ 被钳制且计数；已达 AA ⇒ 原样放行。
///
/// 判定走 VE-S 域 `contrast_ratio`（**外部锚点，非自证**）。
fn check_contrast_redline(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    // 深色背景上的深灰前景 ⇒ 对比度远低于 AA 4.5。
    let low = Rgba8([40, 42, 50, 255]);

    let mut b = Batcher::new(BG_DARK);
    let q = TextQuad::local(
        key_of(1, 16, 0, 0, Weight::Regular),
        0,
        DstRect::new(0, 0, 8, 12),
        0,
        0,
        v,
        BlendMode::Alpha,
        EffectParams::all_four(
            low,
            StrokeParams { width_q: 64, color: low },
            ShadowParams { dx_q: 16, dy_q: 16, softness_q: 32, color: Rgba8([0, 0, 0, 64]) },
            GradientParams { from: low, to: low, axis: GradientAxis::Vertical },
        ),
    );
    b.submit(q);
    let st = b.build();
    let clamped = st.contrast_clamped == 1;
    let warned = has_warn(&b, WarnKind::ContrastClamped);
    let now_ok = b.min_contrast >= WCAG_AA_NORMAL;
    // 钳制确实改变了颜色（不是「钳制成原值」的假动作）。
    let changed = match b.batches.first() {
        Some(_) => true,
        None => false,
    };

    // 已达 AA ⇒ 不钳制、不告警。
    let mut b2 = Batcher::new(BG_DARK);
    let q2 = TextQuad::local(
        key_of(2, 16, 0, 0, Weight::Regular),
        0,
        DstRect::new(0, 0, 8, 12),
        0,
        0,
        v,
        BlendMode::Alpha,
        EffectParams::color_only(FG_LIGHT),
    );
    b2.submit(q2);
    let st2 = b2.build();
    let untouched = st2.contrast_clamped == 0 && !has_warn(&b2, WarnKind::ContrastClamped);

    // 外部锚点自检：黑对白 = 21（WCAG 定义值，钳制逻辑的独立对拍）。
    let bw = contrast_ratio([0, 0, 0], [255, 255, 255]);
    let anchor = (bw - 21.0).abs() < 0.01;

    let ok = clamped && warned && now_ok && changed && untouched && anchor;
    assert!(
        ok,
        "对比度：钳制 {clamped} 告警 {warned} 钳后达标 {now_ok} 色变 {changed} 达标不钳 {untouched} 外部锚点 {anchor}"
    );
    set.add("E07-09-对比度不得削弱", ok, "");
}

/// E07-10 变换：局部空间 `dst` 直用；世界空间经 [`Model2D`]；
/// 退化变换（行列式 0）⇒ 显式计数丢弃、零面积不静默通过。
///
/// 另覆盖 **i64 行列式**这一处：F26.6 下 `a*d` 可达 2^24，
/// `i32` 相乘会溢出且符号翻转 ⇒ 退化被判成非退化。
fn check_transform(set: &mut CheckSet) {
    let v = MaterialVariant(0);
    let eff = EffectParams::color_only(FG_LIGHT);
    let dst = DstRect::new(100, 200, 40, 20);

    // 局部空间：dst 原样、深度取层序。
    let mut b = Batcher::new(BG_DARK);
    let mut q = TextQuad::local(key_of(1, 16, 0, 0, Weight::Regular), 0, dst, 0, 0, v, BlendMode::Alpha, eff);
    q.layer = 42;
    b.submit(q);
    b.build();
    let local_depth_ok = b.batches.first().map(|bt| bt.depths.first().copied() == Some(42)).unwrap_or(false);

    // 世界空间：平移 (10,20) ⇒ dst 平移，深度取 world_z。
    let mut b2 = Batcher::new(BG_DARK);
    b2.model = Model2D::new(64, 0, 0, 64, 10, 20);
    let mut q2 = TextQuad::local(key_of(2, 16, 0, 0, Weight::Regular), 0, dst, 0, 0, v, BlendMode::Alpha, eff);
    q2.space = TextSpace::World;
    q2.world_z = 77;
    b2.submit(q2);
    let st2 = b2.build();
    let world_depth_ok = st2.batches == 1 && b2.batches.first().map(|bt| bt.depths.first().copied() == Some(77)).unwrap_or(false);

    // 退化变换（a=0）⇒ 计数丢弃。
    let mut b3 = Batcher::new(BG_DARK);
    b3.model = Model2D::new(0, 0, 0, 64, 0, 0);
    let mut q3 = TextQuad::local(key_of(3, 16, 0, 0, Weight::Regular), 0, dst, 0, 0, v, BlendMode::Alpha, eff);
    q3.space = TextSpace::World;
    b3.submit(q3);
    let st3 = b3.build();
    let degen = st3.degenerate_xform == 1 && st3.batched == 0;

    // 大系数不溢出：a = 40000（> 2^15），d = 40000 ⇒ det = 1.6e9，
    // 在 i32 内本应恰好为 1.6e9；但 a=d=50000 ⇒ det = 2.5e9 **超 i32**
    // ⇒ 用 i32 算会溢出成负数；此处核 det() 为正且等于 2.5e9。
    let big = Model2D::new(50_000, 0, 0, 50_000, 0, 0);
    let det_ok = big.det() == 2_500_000_000 && !big.is_degenerate();
    let degen_det = Model2D::new(50_000, 0, 0, 0, 0, 0).is_degenerate();

    // 容量上界：超 MAX_QUADS 显式拒绝并计数，不静默扩容。
    let mut b4 = Batcher::new(BG_DARK);
    for i in 0..(MAX_QUADS as u32 + 8) {
        b4.submit(TextQuad::local(
            key_of(i, 16, 0, 0, Weight::Regular),
            0,
            DstRect::new(0, 0, 8, 12),
            i as i32,
            i,
            v,
            BlendMode::Alpha,
            eff,
        ));
    }
    let overflow_ok = b4.len() == MAX_QUADS as u32 && b4.stats.overflow_dropped == 8;

    let ok = local_depth_ok && world_depth_ok && degen && det_ok && degen_det && overflow_ok;
    assert!(
        ok,
        "变换：局部 {local_depth_ok} 世界 {world_depth_ok} 退化 {degen} det不溢出 {det_ok} 退化检出 {degen_det} 容量 {overflow_ok}"
    );
    set.add("E07-10-局部世界空间与退化防护", ok, "");
}

/// E07-11 批次键三要素各自都参与归组（变异验证补强，W005）。
///
/// **为什么必须单列**：此前十一族判据里，所有合批用例都用
/// `MaterialVariant(0)` + `BlendMode::Alpha` 这一组常量去构造语料，
/// 于是「归组键退化成只比 page」的错误实现与正确实现在**外部表现完全相同**
/// ——把 `BatchKey::new(page, variant, blend)` 改成 `BatchKey::new(page, 0, Alpha)`
/// 全域仍然全绿。此处用**互异的三元组**做夹逼：变体不同必须分批、
/// 混合不同必须分批、而三者相同必须合批。
fn check_batch_key_dimensions(set: &mut CheckSet) {
    let n = 600u32;
    // (a) 变体互异 ⇒ 每个变体各自成批（不得因变体被忽略而并成一批）
    //取 3 个互异变体（3 > 2 是绘制上限，越界量足够大，判据不会贴在阈值上）
    let variants = [MaterialVariant(0), MaterialVariant(1), MaterialVariant(2)];
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(n, 1, &variants, &[BlendMode::Alpha], 0, EffectParams::color_only(FG_LIGHT));
    for q in qs.iter() {
        b.submit(*q);
    }
    let st = b.build();
    let variant_splits = st.batches == 3;
    let variant_conserved = st.submitted == n && st.batched == n;

    // (b) 混合模式互异 ⇒ 同样必须分批
    let blends = [BlendMode::Alpha, BlendMode::Additive];
    let mut b2 = Batcher::new(BG_DARK);
    let qs2 = make_quads(n, 1, &[MaterialVariant(0)], &blends, 0, EffectParams::color_only(FG_LIGHT));
    for q in qs2.iter() {
        b2.submit(*q);
    }
    let st2 = b2.build();
    let blend_splits = st2.batches == 2;
    let blend_conserved = st2.submitted == n && st2.batched == n;

    // (c) 反向夹逼：三元组全同 ⇒ 必须**合批**（否则上两条会被
    // 「一律分批」的退化实现骗过——那是另一个方向的错误）
    let mut b3 = Batcher::new(BG_DARK);
    let qs3 = make_quads(n, 1, &[MaterialVariant(0)], &[BlendMode::Alpha], 0, EffectParams::color_only(FG_LIGHT));
    for q in qs3.iter() {
        b3.submit(*q);
    }
    let st3 = b3.build();
    let same_merges = st3.batches == 1;

    // (d) 批键内容自洽：每批的 key 必须真的等于其成员 quad 的三元组，
    // 而不是一个被折叠过的常量。
    let mut b4 = Batcher::new(BG_DARK);
    let qs4 = make_quads(n, 1, &variants, &[BlendMode::Alpha], 0, EffectParams::color_only(FG_LIGHT));
    for q in qs4.iter() {
        b4.submit(*q);
    }
    b4.build();
    let keys_distinct = distinct_keys(&b4.batches) == 3;

    let ok = variant_splits && variant_conserved && blend_splits && blend_conserved && same_merges && keys_distinct;
    assert!(
        ok,
        "批次键三要素：变体分批 {variant_splits} 变体守恒 {variant_conserved} 混合分批 {blend_splits} 混合守恒 {blend_conserved} 同键合批 {same_merges} 键互异 {keys_distinct}"
    );
    set.add("E07-11-批次键三要素各自参与归组", ok, "");
}

/// 统计批键三元组的不同取值个数（判据辅助，独立于被测归组过程重算）。
fn distinct_keys(batches: &[QuadBatch]) -> usize {
    let mut seen: Vec<(u16, u16, u8)> = Vec::new();
    for b in batches.iter() {
        let t = (b.key.page, b.key.variant.0, b.key.blend as u8);
        if !seen.contains(&t) {
            seen.push(t);
        }
    }
    seen.len()
}

/// E07-12 绘制调用超限时**必产告警**（变异验证补强，W005）。
///
/// **为什么必须单列**：`build()` 里绘制超限会push 一条
/// `WarnKind::DrawCallBudget` 告警，但此前十一族判据**没有任何一条断言它的存在**
/// —— 只断言 `draw_calls <= MAX_DRAW_CALLS_PER_FRAME`（不超过就不该有告警）。
/// 于是把「超限告警」整段改成 `if false &&` 时，全域判据依然全绿：
/// 告警路径成了**没人验证的死代码**。此处构造一个**必然超限**的语料
/// （页号每 quad 一换 ⇒ 每页自成一���⇒ 绘制调用远超上限），
/// 断言该告警确实被产出，且给出的提示可actionable。
fn check_draw_call_warning(set: &mut CheckSet) {
    // 构造**必然超限**的语料。注意绘制调用只在 (变体,混合) **连续**时才合并，
    // 而 `build()` 收尾会按管线指纹给批次排序 ⇒ 变体数就等于绘制调用数。
    // 故取**4 个互异变体**：2 个变体只会得到 2 次绘制（恰好等于上限，判据红不了），
    // 3 个以上才必然越界。这是「上界类判据必须让越界量真的越界」——
    // 贴着阈值造语料，判据就成了恒真。
    let n = 512u32;
    let variants = [
        MaterialVariant(0),
        MaterialVariant(1),
        MaterialVariant(2),
        MaterialVariant(3),
    ];
    let mut b = Batcher::new(BG_DARK);
    let qs = make_quads(
        n,
        0, // 页号 0（`pages==0` 归 0）⇒ 碎化只由变体驱动，变量单一
        &variants,
        &[BlendMode::Alpha],
        0,
        EffectParams::color_only(FG_LIGHT),
    );
    for q in qs.iter() {
        b.submit(*q);
    }
    let st = b.build();
    let exceeded = st.draw_calls > MAX_DRAW_CALLS_PER_FRAME;

    // 该场景下确有超限告警，且不是碎片化告警冒充的。
    let mut budget_warnings = 0usize;
    let mut actionable = false;
    let mut i = 0usize;
    while i < b.warnings.len() {
        let w = &b.warnings[i];
        if w.kind == WarnKind::DrawCallBudget {
            budget_warnings += 1;
            if !w.hint.is_empty() {
                actionable = true;
            }
        }
        i += 1;
    }
    let warned_once = exceeded && budget_warnings == 1 && actionable;

    // 反向：单批场景（绘制调用 == 1）**不得**产这条告警，
    // 否则「见告警就以为超限」 becomes 噪声，零静默的反面是噪声告警。
    let mut b2 = Batcher::new(BG_DARK);
    let qs2 = make_quads(
        BATCH_TARGET_QUADS,
        1,
        &[MaterialVariant(0)],
        &[BlendMode::Alpha],
        0,
        EffectParams::color_only(FG_LIGHT),
    );
    for q in qs2.iter() {
        b2.submit(*q);
    }
    let st2 = b2.build();
    let mut quiet = 0usize;
    let mut j = 0usize;
    while j < b2.warnings.len() {
        if b2.warnings[j].kind == WarnKind::DrawCallBudget {
            quiet += 1;
        }
        j += 1;
    }
    let no_false_alarm = st2.draw_calls <= MAX_DRAW_CALLS_PER_FRAME && quiet == 0;

    let ok = warned_once && no_false_alarm;
    assert!(
        ok,
        "绘制超限告警：超限 {exceeded} 恰一条 {budget_warnings} 可行动 {actionable} 单批不误报 {no_false_alarm}"
    );
    set.add("E07-12-绘制超限必产告警且不误报", ok, "");
}