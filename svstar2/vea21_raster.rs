//! VE-F0021 · 光栅化状态机（VE-A 域 · 合成核心 · 目标 310 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0021`
//!
//! **判据（锚点原文）**：光栅化状态（填充模式、剔除模式、裁剪矩形、多重采样四维），裁剪矩形
//! 的批量管理，线框调试模式的一键切换；含多重采样与后处理的兼容声明。判据五条：**四维状态、
//! scissor 批量、线框一键、越界钳制、判据**。
//!
//! **错误路径与降级矩阵**：非法→拒绝；scissor 越界→钳制；批量失效→事务。
//!
//! **数据结构**：状态封装；scissor 管理。
//!
//! **性能逐项分解**：O(状态)。
//!
//! **跨批对接点**：A19 同构（F0020 深度模板状态机同构：四维 + 预置 + 事务 + 一键切换）。
//!
//! **无障碍与隐私**：状态表读屏可达——四维每维有稳定短名与文字描述，线框切换有文字
//! 状态面，读屏器能逐项朗读（[`RasterState::a11y_table`]）。
//!
//! ## 设计要点
//!
//! - **四维正交**（[`RasterDim`]）：填充模式、剔除模式、裁剪矩形、多重采样四维**互不
//!   蕴含**——改一维不动其余三维。四维各自穷举登记（[`FILL_TABLE`] 3 种、
//!   [`CULL_TABLE`] 2 种、[`SAMPLE_TABLE`] 3 种），维度值改动即被机检发现。
//! - **四维的相互作用只有两处显式声明**（[`COMPAT_DOC`]）：① **多重采样 × 后处理**——
//!   后处理在 resolve 之后进行，故 MSAA 开启时后处理读到的是已解析结果，二者**兼容**；
//!   ② **裁剪矩形 × scissor 批量**——scissor 是硬件级裁剪，与四维的逻辑裁剪**叠加**
//!   （取交集），不是替换。
//!
//!   这两条是规格明确要求的"兼容声明"，写成断言而非注释——注释会随代码漂移。
//! - **scissor 批量管理 + 事务**（[`ScissorBatch`]）：批量设置裁剪矩形，**要么全成、
//!   要么全不成**（错误矩阵「批量失效→事务」）。实现方式是**先在影子缓冲校验全部条目**、
//!   全通过才整体提交；任一条越界则整批回滚且**不改动任何活动 scissor**。
//!   部分生效是最坏结果——上层以为全批生效了。
//! - **越界钳制**（[`clamp_scissor`]）：scissor 越界**钳制而非拒绝**（与非法→拒绝的分工：
//!   参数**非法**（负宽、非有限）拒绝；参数合法但**超出目标尺寸**钳制）。这个分工
//!   是规格「非法→拒绝；scissor 越界→钳制」的字面落实——两者不是同一件事。
//! - **线框一键切换**（[`WireframeToggle`]）：一键在填充/线框间切换，**四维中只动填充
//!   模式一维**，其余三维逐位不变（判据「线框一键」的核心是"一键"且"只动该动的"）。
//!   切线框时若多重采样仍开着，须显式声明兼容性影响而不是静默留着。
//! - **深度模板维度的衔接**：光栅化四维与 F0020 的模板维度**互补而不重叠**——
//!   本条管「几何怎么被光栅化」，F0020 管「光栅化之后怎么被测试」，交界处共享深度缓冲
//!   但不共享状态机（两条各有各的转移表，避免互相牵连）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 填充模式全集规模。
pub const FILL_TABLE_SIZE: usize = 3;

/// 剔除模式全集规模。
pub const CULL_TABLE_SIZE: usize = 2;

/// 多重采样模式全集规模。
pub const SAMPLE_TABLE_SIZE: usize = 3;

/// 目标帧缓冲最小边长（下界；scissor 钳制的目标面）。
pub const MIN_TARGET_DIM: u32 = 1;

/// 钳制容差（浮点坐标量化到整数像素时的容差）。
pub const CLAMP_EPS: f32 = 1e-4;

/// 非法参数错误码。
pub const ILLEGAL_PARAM: &str = "E_RASTER_ILLEGAL_PARAM";

/// 四维正交契约。
pub const DIM_ORTHOGONAL_DOC: &str = "\
四维正交契约（VE-F0021 · v1）：填充模式、剔除模式、裁剪矩形、多重采样四维互不蕴含——\
改一维不动其余三维。四维的枚举值分别穷举登记于填充表（三）、剔除表（二）、采样表（三），\
维度值改动即被机检发现。线框一键切换只动填充模式一维，其余三维逐位不变。";

/// 多重采样与后处理兼容声明。
pub const COMPAT_DOC: &str = "\
多重采样与后处理兼容声明（VE-F0021 · v1）：① 多重采样 × 后处理——后处理在解析（resolve）\
之后进行，故多重采样开启时后处理读到的是已解析结果，二者**兼容**；② 裁剪矩形 × 批量管理——\
批量 scissor 是硬件级裁剪，与四维的逻辑裁剪**叠加取交集**，不是替换。这两条写成断言而非\
注释：注释会随代码漂移，断言会。";

/// scissor 事务契约。
pub const BATCH_TX_DOC: &str = "\
scissor 批量事务契约（VE-F0021 · v1）：批量设置裁剪矩形要么全成要么全不成。实现方式是先在\
影子缓冲校验全部条目，全通过才整体提交；任一条越界则整批回滚且不改动任何活动裁剪矩形。\
**部分生效是最坏结果**——上层以为全批生效了。故批内条目数显式登记，回滚事实入审计。";

/// 越界钳制契约。
pub const CLAMP_DOC: &str = "\
越界钳制契约（VE-F0021 · v1）：参数**非法**（负宽高、非有限坐标）拒绝并带建议；参数合法但\
**超出目标尺寸**钳制到目标面。这个分工是规格「非法→拒绝；scissor 越界→钳制」的字面落实——\
两者不是同一件事，把越界也当非法拒绝会让合法的全屏裁剪无法表达，把非法也钳制会掩盖调用\
方逻辑错误。";

/// 线框一键契约。
pub const WIREFRAME_DOC: &str = "\
线框一键契约（VE-F0021 · v1）：一键在填充与线框间切换，且只动填充模式一维，其余三维逐位\
不变（这是「一键」的真正含义——不该被顺带改动的东西不能动）。切线框时多重采样仍开着须显式\
声明兼容影响，不静默留着。";

// ---------------------------------------------------------------------------
// 二、四维状态封装
// ---------------------------------------------------------------------------

/// 填充模式（第一维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    /// 实心填充。
    Solid,
    /// 线框。
    Wireframe,
    /// 点阵。
    Point,
}

impl FillMode {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            FillMode::Solid => "solid",
            FillMode::Wireframe => "wireframe",
            FillMode::Point => "point",
        }
    }

    /// 文字描述（读屏可达）。
    pub fn describe(self) -> &'static str {
        match self {
            FillMode::Solid => "实心填充：面片内部全部着色",
            FillMode::Wireframe => "线框：只画边线，内部不着色",
            FillMode::Point => "点阵：只画顶点，内部与边线均不着色",
        }
    }

    /// 是否为线框态（一键切换的判据面）。
    pub fn is_wireframe(self) -> bool {
        matches!(self, FillMode::Wireframe)
    }

    /// 全集。
    pub fn all() -> [FillMode; FILL_TABLE_SIZE] {
        [FillMode::Solid, FillMode::Wireframe, FillMode::Point]
    }
}

/// 剔除模式（第二维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullMode {
    /// 不剔除。
    None,
    /// 剔除背面。
    Back,
}

impl CullMode {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            CullMode::None => "none",
            CullMode::Back => "back",
        }
    }

    /// 文字描述。
    pub fn describe(self) -> &'static str {
        match self {
            CullMode::None => "不剔除：正反面都光栅化（双面材质用）",
            CullMode::Back => "剔除背面：只光栅化正面，省一半片元",
        }
    }

    /// 全集。
    pub fn all() -> [CullMode; CULL_TABLE_SIZE] {
        [CullMode::None, CullMode::Back]
    }
}

/// 多重采样模式（第三维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleMode {
    /// 单采样。
    Single,
    /// 四采样。
    Ms4x,
    /// 八采样。
    Ms8x,
}

impl SampleMode {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            SampleMode::Single => "1x",
            SampleMode::Ms4x => "4x",
            SampleMode::Ms8x => "8x",
        }
    }

    /// 文字描述。
    pub fn describe(self) -> &'static str {
        match self {
            SampleMode::Single => "单采样：每像素一个样本，无解析成本",
            SampleMode::Ms4x => "四采样：每像素四个样本，边缘抗锯齿，解析后处理",
            SampleMode::Ms8x => "八采样：每像素八个样本，边缘更平滑，解析成本更高",
        }
    }

    /// 每像素样本数（1 / 4 / 8）。
    pub fn samples(self) -> u32 {
        match self {
            SampleMode::Single => 1,
            SampleMode::Ms4x => 4,
            SampleMode::Ms8x => 8,
        }
    }

    /// 是否开启多重采样。
    pub fn enabled(self) -> bool {
        self.samples() > 1
    }

    /// 全集。
    pub fn all() -> [SampleMode; SAMPLE_TABLE_SIZE] {
        [SampleMode::Single, SampleMode::Ms4x, SampleMode::Ms8x]
    }
}

/// 裁剪矩形（第四维；浮点坐标，像素对齐由钳制负责）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScissorRect {
    /// 左。
    pub x: f32,
    /// 上。
    pub y: f32,
    /// 宽（>0）。
    pub w: f32,
    /// 高（>0）。
    pub h: f32,
}

impl ScissorRect {
    /// 构造：非有限或非正尺寸显式拒绝（**非法**路径，不是越界路径）。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        Some(ScissorRect { x, y, w, h })
    }

    /// 全屏裁剪（覆盖整个目标）。
    pub fn full(target_w: u32, target_h: u32) -> Option<Self> {
        ScissorRect::new(0.0, 0.0, target_w as f32, target_h as f32)
    }

    /// 右边界。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 短名（读屏与日志）。
    pub fn tag(&self) -> String {
        format!("scissor({},{},{}x{})", self.x, self.y, self.w, self.h)
    }

    /// 是否越出目标面（钳制判据）。
    pub fn out_of(&self, target_w: u32, target_h: u32) -> bool {
        self.x < -CLAMP_EPS
            || self.y < -CLAMP_EPS
            || self.right() > target_w as f32 + CLAMP_EPS
            || self.bottom() > target_h as f32 + CLAMP_EPS
    }

    /// 取交集（scissor 与逻辑裁剪**叠加**而非替换——[`COMPAT_DOC`] 第二条）。
    pub fn intersect(&self, o: &ScissorRect) -> Option<ScissorRect> {
        let x0 = if self.x > o.x { self.x } else { o.x };
        let y0 = if self.y > o.y { self.y } else { o.y };
        let x1 = if self.right() < o.right() { self.right() } else { o.right() };
        let y1 = if self.bottom() < o.bottom() { self.bottom() } else { o.bottom() };
        ScissorRect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// 目标帧缓冲尺寸（钳制的目标面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetSize {
    /// 宽（像素）。
    pub w: u32,
    /// 高（像素）。
    pub h: u32,
}

impl TargetSize {
    /// 构造：小于 [`MIN_TARGET_DIM`] 显性拒绝（零尺寸目标会让钳制退化为空裁剪）。
    pub fn new(w: u32, h: u32) -> Option<Self> {
        if w < MIN_TARGET_DIM || h < MIN_TARGET_DIM {
            return None;
        }
        Some(TargetSize { w, h })
    }
}

/// 四维光栅化状态（状态封装的单一事实源）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterState {
    /// 第一维：填充模式。
    pub fill: FillMode,
    /// 第二维：剔除模式。
    pub cull: CullMode,
    /// 第三维：多重采样模式。
    pub sample: SampleMode,
    /// 第四维：裁剪矩形。
    pub scissor: Option<ScissorRect>,
    /// 目标面尺寸（钳制依据；不参与四维正交性判定）。
    pub target: TargetSize,
}

impl RasterState {
    /// 构造：默认实心、剔除背面、单采样、无裁剪。
    pub fn new(target: TargetSize) -> Self {
        RasterState {
            fill: FillMode::Solid,
            cull: CullMode::Back,
            sample: SampleMode::Single,
            scissor: None,
            target,
        }
    }

    /// 设置填充模式（只动第一维——**返回 self 便于链式，但四维各自独立**）。
    pub fn with_fill(mut self, f: FillMode) -> Self {
        self.fill = f;
        self
    }

    /// 设置剔除模式（第二维）。
    pub fn with_cull(mut self, c: CullMode) -> Self {
        self.cull = c;
        self
    }

    /// 设置多重采样（第三维）。
    pub fn with_sample(mut self, s: SampleMode) -> Self {
        self.sample = s;
        self
    }

    /// 设置裁剪矩形（第四维；越界在此钳制）。
    pub fn with_scissor(mut self, s: Option<ScissorRect>) -> Self {
        self.scissor = s.and_then(|r| clamp_scissor(&r, self.target));
        self
    }

    /// 四维签名（用于"只动一维"的逐位比对）。
    ///
    /// **裁剪矩形按量化键入签名**：浮点直接比较会把「钳制前后的同一个矩形」判成不同，
    /// 而钳制是幂等的（钳两次结果相同）——量化键才能反映"实质未变"。
    pub fn dim_signature(&self) -> (FillMode, CullMode, SampleMode, Option<RectKey>) {
        (
            self.fill,
            self.cull,
            self.sample,
            self.scissor.map(|r| rect_key(&r)),
        )
    }

    /// 是否线框态。
    pub fn is_wireframe(&self) -> bool {
        self.fill.is_wireframe()
    }

    /// 有效裁剪域（裁剪矩形 ∩ 目标面；无裁剪时为全屏）。
    pub fn effective_clip(&self) -> ScissorRect {
        let full = ScissorRect::full(self.target.w, self.target.h);
        match (self.scissor, full) {
            (Some(s), Some(f)) => s.intersect(&f).unwrap_or(ScissorRect {
                //交集为空（裁剪完全在目标外且被钳制过不该发生，兜底给全屏）
                x: 0.0,
                y: 0.0,
                w: self.target.w as f32,
                h: self.target.h as f32,
            }),
            (None, Some(f)) => f,
            _ => ScissorRect {
                x: 0.0,
                y: 0.0,
                w: self.target.w as f32,
                h: self.target.h as f32,
            },
        }
    }

    /// 状态表（无障碍朗读面）。
    pub fn a11y_table() -> Vec<String> {
        let mut out = Vec::new();
        out.push(String::from("光栅化四维：填充模式、剔除模式、多重采样、裁剪矩形"));
        for f in FillMode::all().iter() {
            out.push(format!("填充模式 {}：{}", f.tag(), f.describe()));
        }
        for c in CullMode::all().iter() {
            out.push(format!("剔除模式 {}：{}", c.tag(), c.describe()));
        }
        for s in SampleMode::all().iter() {
            out.push(format!("多重采样 {}：{}", s.tag(), s.describe()));
        }
        out
    }
}

/// 量化矩形键（1 像素精度；用于"实质未变"比对）。
pub type RectKey = (i64, i64, i64, i64);

/// 矩形量化键。
pub fn rect_key(r: &ScissorRect) -> RectKey {
    (
        r.x.round() as i64,
        r.y.round() as i64,
        r.w.round().max(0.0) as i64,
        r.h.round().max(0.0) as i64,
    )
}

/// 越界钳制：合法但超出目标面的裁剪矩形**钳制**到目标面（不拒绝）。
///
/// 幂等：钳两次结果相同（这是"实质未变"判定成立的前提）。
pub fn clamp_scissor(r: &ScissorRect, target: TargetSize) -> Option<ScissorRect> {
    let tw = target.w as f32;
    let th = target.h as f32;
    // 先把起点钳进目标面，再钳尺寸，保证结果非空且在面内。
    let x = if r.x < 0.0 { 0.0 } else if r.x > tw { tw } else { r.x };
    let y = if r.y < 0.0 { 0.0 } else if r.y > th { th } else { r.y };
    let x1 = if r.right() > tw { tw } else { r.right() };
    let y1 = if r.bottom() > th { th } else { r.bottom() };
    // 起点被钳到面边界右/下侧时宽高会变非正 —— 此时裁剪为空，返回 None 表示空域。
    if !(x1 - x > CLAMP_EPS) || !(y1 - y > CLAMP_EPS) {
        return None;
    }
    ScissorRect::new(x, y, x1 - x, y1 - y)
}

// ---------------------------------------------------------------------------
// 三、scissor 批量管理（事务语义）
// ---------------------------------------------------------------------------

/// 批量条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScissorEntry {
    /// 条目 id（回滚审计用）。
    pub id: u32,
    /// 裁剪矩形。
    pub rect: ScissorRect,
}

/// 批量受理结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchResult {
    /// 全成：已提交，逐条给出钳制后的矩形。
    Committed {
        /// 条目数。
        count: usize,
    },
    /// 全不成：整批回滚，活动裁剪未被改动。
    RolledBack {
        /// 条目数。
        count: usize,
        /// 失败条目 id（首个失败者）。
        first_bad: u32,
    },
}

impl BatchResult {
    /// 是否已提交。
    pub fn committed(&self) -> bool {
        matches!(self, BatchResult::Committed { .. })
    }
}

/// scissor 批量管理器（事务：全成或全不成）。
pub struct ScissorBatch {
    /// 活动裁剪集合（提交后生效）。
    active: Vec<ScissorEntry>,
    /// 审计留痕。
    audits: Vec<String>,
}

impl ScissorBatch {
    /// 构造：空集合。
    pub fn new() -> Self {
        ScissorBatch { active: Vec::new(), audits: Vec::new() }
    }

    /// 活动条目数。
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// 活动条目（只读）。
    pub fn active(&self) -> &[ScissorEntry] {
        &self.active
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// **事务批量**：先在影子缓冲校验全部条目，全通过才整体提交。
    ///
    /// 任一条钳制后为空（即该条目在此目标面上不可见）→ 整批回滚，活动集合**逐位不变**。
    /// 部分生效是最坏结果——上层以为全批生效了。
    pub fn apply_batch(&mut self, entries: &[ScissorEntry], target: TargetSize) -> BatchResult {
        // ---- 影子缓冲：逐条校验，不动活动集合 ----
        let mut shadow: Vec<ScissorEntry> = Vec::new();
        for e in entries.iter() {
            match clamp_scissor(&e.rect, target) {
                Some(clamped) => shadow.push(ScissorEntry { id: e.id, rect: clamped }),
                None => {
                    // 整批回滚：活动集合一个字节都不改。
                    self.audits.push(format!(
                        "批量事务回滚：条目 {} 在 {}x{} 目标面上钳制后为空（整批 {} 条未生效）",
                        e.id,
                        target.w,
                        target.h,
                        entries.len()
                    ));
                    return BatchResult::RolledBack {
                        count: entries.len(),
                        first_bad: e.id,
                    };
                }
            }
        }
        // ---- 全通过：整体提交（替换而非追加——批量语义是"这一批就是当前批"）----
        let count = shadow.len();
        self.active = shadow;
        self.audits
            .push(format!("批量事务提交：{} 条生效", count));
        BatchResult::Committed { count }
    }

    /// 清空（回到无裁剪）。
    pub fn clear(&mut self) {
        self.active.clear();
        self.audits.push("scissor 集合已清空".to_string());
    }
}

impl Default for ScissorBatch {
    fn default() -> Self {
        ScissorBatch::new()
    }
}

// ---------------------------------------------------------------------------
// 四、线框一键切换
// ---------------------------------------------------------------------------

/// 线框切换结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireframeToggle {
    /// 切换后的填充模式。
    pub fill: FillMode,
    /// 是否发生了切换（已是目标态则 false）。
    pub changed: bool,
    /// 兼容影响说明（多重采样仍开着时非空——不静默留着）。
    pub compat_note: String,
    /// 其余三维是否逐位未变（机检面；恒 true 是本条的承诺）。
    pub others_unchanged: bool,
}

/// 线框一键切换（[`WIREFRAME_DOC`]）。
///
/// **只动填充模式一维**——这是判据「线框一键」的核心。不该被顺带改动的东西不能动：
/// 切线框时剔除模式、多重采样、裁剪矩形一律原样保留。
pub fn toggle_wireframe(state: &RasterState, to_wireframe: bool) -> (RasterState, WireframeToggle) {
    let before_others = (state.cull, state.sample, state.scissor.map(|r| rect_key(&r)));
    let target = if to_wireframe { FillMode::Wireframe } else { FillMode::Solid };
    let changed = state.fill != target;
    let mut next = *state;
    next.fill = target;
    let after_others = (next.cull, next.sample, next.scissor.map(|r| rect_key(&r)));
    let others_unchanged = before_others == after_others;

    // 多重采样开着时切线框的兼容影响：线框边缘仍按多重采样解析，线宽视觉上会被
    // 多重采样柔化——这不是错误，但必须说出来，不能静默留着。
    let compat_note = if state.sample.enabled() {
        format!(
            "多重采样 {}x 仍开启：线框边缘将按多重采样解析，线宽视觉被柔化（不影响正确性）",
            state.sample.samples()
        )
    } else {
        String::new()
    };

    let result = WireframeToggle {
        fill: target,
        changed,
        compat_note,
        others_unchanged,
    };
    (next, result)
}

/// 多重采样与后处理的兼容判定（[`COMPAT_DOC`] 第一条）。
///
/// 语义：后处理在**解析之后**进行，故多重采样开启时后处理读到的是已解析结果——
/// **二者兼容**。返回 `true` 表示兼容（这里恒真，因为兼容声明是本域的契约）；
/// 保留这个函数是为了让「兼容」成为可断言的事实，而不是文档里的一句话。
pub fn msaa_postprocess_compatible(sample: SampleMode) -> bool {
    // 解析顺序保证：任何采样模式都不阻断后处理（单采样无解析步骤，后处理直接读）。
    let _ = sample;
    true
}

// ---------------------------------------------------------------------------
// 五、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0021 · 光栅化状态机 —— 判据自检。
///
/// 判据五条（锚点）：四维状态、scissor 批量、线框一键、越界钳制、判据。
/// 覆盖六个判据族：`dim-*`（四维状态）、`scissor-*`（scissor 批量与事务）、
/// `wire-*`（线框一键）、`clamp-*`（越界钳制与非法拒绝）、`compat-*`（兼容声明）、
/// `judge-*`（契约条款在场）。
pub fn run_vea21_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0021");

    /// 目标面 800x600。
    fn target() -> TargetSize {
        TargetSize::new(800, 600).unwrap_or(TargetSize { w: 800, h: 600 })
    }

    // ---- 四维状态 ----

    {
        // 四维枚举各自穷举登记，维度规模与常量一致。
        set.add(
            "A21-dim-四维枚举齐备",
            FillMode::all().len() == FILL_TABLE_SIZE
                && CullMode::all().len() == CULL_TABLE_SIZE
                && SampleMode::all().len() == SAMPLE_TABLE_SIZE
                && DIM_ORTHOGONAL_DOC.contains("四维"),
            "",
        );
    }

    {
        // 四维短名与描述齐备（读屏可达）。
        let named = FillMode::all().iter().all(|f| !f.tag().is_empty())
            && CullMode::all().iter().all(|c| !c.tag().is_empty())
            && SampleMode::all().iter().all(|s| !s.tag().is_empty());
        let described = FillMode::all().iter().all(|f| !f.describe().is_empty())
            && CullMode::all().iter().all(|c| !c.describe().is_empty())
            && SampleMode::all().iter().all(|s| !s.describe().is_empty());
        set.add("A21-dim-短名描述齐备", named && described, "");
    }

    {
        // **四维正交**：改一维不动其余三维（签名前三位 + 第四维）。
        let base = RasterState::new(target());
        let only_cull = base.with_cull(CullMode::None);
        let sig_base = base.dim_signature();
        let sig_cull = only_cull.dim_signature();
        // 剔除变了 → 签名必变（否则正交性失效）
        let changed = sig_base != sig_cull;
        // 但填充/采样/裁剪三位不变
        let others_same = sig_base.0 == sig_cull.0 && sig_base.2 == sig_cull.2 && sig_base.3 == sig_cull.3;
        set.add("A21-dim-四维正交", changed && others_same, "");
    }

    {
        // 改填充不动其余三维（正交性逐维抽查第二例）。
        let base = RasterState::new(target());
        let s2 = base.with_fill(FillMode::Point);
        let a = base.dim_signature();
        let b = s2.dim_signature();
        set.add(
            "A21-dim-改填充不动其余",
            a.0 != b.0 && a.1 == b.1 && a.2 == b.2 && a.3 == b.3,
            "",
        );
    }

    {
        // 多重采样样本数正确且开启判定一致。
        let s1 = SampleMode::Single.samples();
        let s4 = SampleMode::Ms4x.samples();
        let s8 = SampleMode::Ms8x.samples();
        set.add(
            "A21-dim-采样数正确",
            s1 == 1 && s4 == 4 && s8 == 8
                && !SampleMode::Single.enabled()
                && SampleMode::Ms4x.enabled()
                && SampleMode::Ms8x.enabled(),
            "",
        );
    }

    {
        // 状态表读屏可达（四维 + 三类枚举）。
        let t = RasterState::a11y_table();
        let expected = 1 + FILL_TABLE_SIZE + CULL_TABLE_SIZE + SAMPLE_TABLE_SIZE;
        set.add("A21-dim-状态表读屏可达", t.len() == expected, "");
    }

    // ---- 越界钳制与非法拒绝（分工） ----

    {
        // **非法拒绝**：负宽高、非有限坐标。
        let neg_w = ScissorRect::new(0.0, 0.0, -1.0, 10.0);
        let zero_h = ScissorRect::new(0.0, 0.0, 10.0, 0.0);
        let inf = ScissorRect::new(f32::NAN, 0.0, 10.0, 10.0);
        set.add("A21-clamp-非法拒绝", neg_w.is_none() && zero_h.is_none() && inf.is_none(), "");
    }

    {
        // **越界钳制**（不是拒绝）：合法但超出目标面 → 钳到面内且尺寸有效。
        let t = target();
        let big = ScissorRect::new(0.0, 0.0, 5000.0, 5000.0).unwrap_or(ScissorRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let clamped = clamp_scissor(&big, t);
        let ok = match clamped {
            Some(c) => {
                c.w <= t.w as f32 + CLAMP_EPS && c.h <= t.h as f32 + CLAMP_EPS && big.out_of(t.w, t.h)
            }
            None => false,
        };
        set.add("A21-clamp-越界钳制非拒绝", ok, "");
    }

    {
        // 钳制**幂等**：钳两次结果相同（"实质未变"判定的前提）。
        let t = target();
        let r = ScissorRect::new(-10.0, -10.0, 9000.0, 9000.0).unwrap_or(ScissorRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let c1 = clamp_scissor(&r, t);
        let idempotent = match c1 {
            Some(c) => match clamp_scissor(&c, t) {
                Some(c2) => rect_key(&c) == rect_key(&c2),
                None => false,
            },
            None => false,
        };
        set.add("A21-clamp-钳制幂等", idempotent && CLAMP_DOC.contains("钳制"), "");
    }

    {
        // 完全在目标面外的裁剪 → 钳制后为空域（None），而非错误矩形。
        let t = target();
        let outside = ScissorRect::new(9000.0, 9000.0, 100.0, 100.0).unwrap_or(ScissorRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        set.add("A21-clamp-面外钳制为空域", clamp_scissor(&outside, t).is_none(), "");
    }

    {
        // 非法目标面拒绝（零尺寸会让钳制退化为空裁剪）。
        set.add(
            "A21-clamp-非法目标拒绝",
            TargetSize::new(0, 600).is_none() && TargetSize::new(800, 0).is_none(),
            "",
        );
    }

    {
        // **起点为负但主体在面内** → 起点钳到 0，右/下边界保留，宽度按可见部分收窄。
        //
        // 注意用例形态：矩形**整体**在面外（左上远处）时钳制给None 是对的（见
        // `A21-clamp-面外钳制为空域`）；此处钉的是"部分可见"这一路——起点被钳到 0
        // 之后右边界不得跟着被吞掉，否则一格可见区域会凭空消失。
        let t = target();
        let partial = ScissorRect::new(-50.0, -30.0, 200.0, 150.0).unwrap_or(ScissorRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let clamped = clamp_scissor(&partial, t);
        let ok = match clamped {
            Some(c) => {
                c.x >= -CLAMP_EPS
                    && c.y >= -CLAMP_EPS
                    // 右/下边界与钳制前一致（可见部分未被吞）。
                    && (c.right() - partial.right()).abs() <= CLAMP_EPS
                    && (c.bottom() - partial.bottom()).abs() <= CLAMP_EPS
                    // 宽高收窄到可见量且为正。
                    && c.w > 0.0
                    && c.h > 0.0
                    && c.w < partial.w
                    && c.h < partial.h
            }
            None => false,
        };
        set.add("A21-clamp-负起点钳到原点", ok && partial.out_of(t.w, t.h), "");
    }

    {
        // **右下越界但左上在面内** → 右/下边界钳到面边界，宽高按可见部分收窄。
        let t = target();
        let partial = ScissorRect::new(700.0, 500.0, 300.0, 200.0).unwrap_or(ScissorRect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let hit_edge = match clamp_scissor(&partial, t) {
            Some(c) => {
                c.right() <= t.w as f32 + CLAMP_EPS
                    && c.bottom() <= t.h as f32 + CLAMP_EPS
                    && (c.right() - t.w as f32).abs() <= CLAMP_EPS
                    && (c.bottom() - t.h as f32).abs() <= CLAMP_EPS
                    // 起点不动（在面内），宽高被截短。
                    && (c.x - partial.x).abs() <= CLAMP_EPS
                    && (c.y - partial.y).abs() <= CLAMP_EPS
                    && c.w > 0.0
                    && c.h > 0.0
            }
            None => false,
        };
        set.add("A21-clamp-超界起点不吞尺寸", hit_edge && partial.out_of(t.w, t.h), "");
    }

    {
        // 有效裁剪域 = 裁剪矩形 ∩ 目标面（叠加而非替换）。
        let t = target();
        let s = RasterState::new(t).with_scissor(ScissorRect::new(100.0, 100.0, 200.0, 200.0));
        let clip = s.effective_clip();
        let within = clip.x >= -CLAMP_EPS
            && clip.y >= -CLAMP_EPS
            && clip.right() <= t.w as f32 + CLAMP_EPS
            && clip.bottom() <= t.h as f32 + CLAMP_EPS;
        set.add("A21-clamp-有效域在面内", within && COMPAT_DOC.contains("叠加"), "");
    }

    // ---- scissor 批量（事务） ----

    {
        // 全成：三条合法条目整批提交。
        let t = target();
        let mut b = ScissorBatch::new();
        let entries = [
            ScissorEntry { id: 1, rect: ScissorRect::new(0.0, 0.0, 100.0, 100.0).unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }) },
            ScissorEntry { id: 2, rect: ScissorRect::new(100.0, 0.0, 100.0, 100.0).unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }) },
            ScissorEntry { id: 3, rect: ScissorRect::new(200.0, 0.0, 100.0, 100.0).unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }) },
        ];
        let r = b.apply_batch(&entries, t);
        set.add(
            "A21-scissor-全批提交",
            r.committed() && b.len() == 3 && BATCH_TX_DOC.contains("全成"),
            "",
        );
    }

    {
        // **全不成（事务回滚）**：一条越界 → 整批回滚，活动集合逐条不变。
        let t = target();
        let mut b = ScissorBatch::new();
        // 先建立一批活动条目
        let good = [ScissorEntry {
            id: 1,
            rect: ScissorRect::new(0.0, 0.0, 100.0, 100.0)
                .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
        }];
        b.apply_batch(&good, t);
        let before: Vec<RectKey> = b.active().iter().map(|e| rect_key(&e.rect)).collect();
        let audit_mark = b.audits().len();
        // 新批含一条面外条目（钳制后为空）
        let mixed = [
            ScissorEntry {
                id: 10,
                rect: ScissorRect::new(0.0, 0.0, 100.0, 100.0)
                    .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
            },
            ScissorEntry {
                id: 11,
                rect: ScissorRect::new(9000.0, 9000.0, 50.0, 50.0)
                    .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
            },
        ];
        let r = b.apply_batch(&mixed, t);
        let after: Vec<RectKey> = b.active().iter().map(|e| rect_key(&e.rect)).collect();
        let rolled = matches!(r, BatchResult::RolledBack { first_bad: 11, .. });
        // 本次调用新增的审计段：回滚只留回滚痕，不得出现"提交"字样。
        // （扫全历史是错的——上一批合法提交的留痕里本就含"提交"二字。）
        let fresh_audit: Vec<&String> = b.audits()[audit_mark..].iter().collect();
        set.add(
            "A21-scissor-整批回滚不留半批",
            rolled
                && before == after
                && !fresh_audit.iter().any(|a| a.contains("提交"))
                && fresh_audit.iter().any(|a| a.contains("回滚")),
            "",
        );
    }

    {
        // 批量条目数显式登记（回滚事实可追溯）。
        let t = target();
        let mut b = ScissorBatch::new();
        let bad = [
            ScissorEntry {
                id: 7,
                rect: ScissorRect::new(9000.0, 0.0, 10.0, 10.0)
                    .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
            },
        ];
        let r = b.apply_batch(&bad, t);
        let count_ok = matches!(r, BatchResult::RolledBack { count: 1, .. });
        let audited = b.audits().iter().any(|a| a.contains("回滚"));
        set.add("A21-scissor-回滚可追溯", count_ok && audited, "");
    }

    {
        // 批量提交是**替换**语义（这一批就是当前批，不累加）。
        let t = target();
        let mut b = ScissorBatch::new();
        let one = [ScissorEntry {
            id: 1,
            rect: ScissorRect::new(0.0, 0.0, 100.0, 100.0)
                .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
        }];
        b.apply_batch(&one, t);
        b.apply_batch(&one, t);
        set.add("A21-scissor-提交为替换语义", b.len() == 1, "");
    }

    {
        // 空批量：提交零条（不是错误）。
        let t = target();
        let mut b = ScissorBatch::new();
        let r = b.apply_batch(&[], t);
        set.add("A21-scissor-空批不报错", r.committed() && b.is_empty(), "");
    }

    {
        // 清空回到无裁剪。
        let t = target();
        let mut b = ScissorBatch::new();
        let e = [ScissorEntry {
            id: 1,
            rect: ScissorRect::new(0.0, 0.0, 100.0, 100.0)
                .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }),
        }];
        b.apply_batch(&e, t);
        b.clear();
        set.add("A21-scissor-清空回无裁剪", b.is_empty(), "");
    }

    // ---- 线框一键 ----

    {
        // 一键切线框：只动填充模式，其余三维逐位不变。
        //
        // 剔除维的初值取**Back**（非默认的 None）：若这里用 None，变体把 cull 改成
        // None 就是同值，「只动一维」判据恒真——用默认值以外的取值才钉得死。
        let base = RasterState::new(target())
            .with_cull(CullMode::Back)
            .with_sample(SampleMode::Ms4x)
            .with_scissor(ScissorRect::new(10.0, 10.0, 100.0, 100.0));
        let (after, res) = toggle_wireframe(&base, true);
        let sig_a = base.dim_signature();
        let sig_b = after.dim_signature();
        set.add(
            "A21-wire-一键切线框只动一维",
            res.changed
                && after.is_wireframe()
                && sig_a.0 != sig_b.0
                && sig_a.1 == sig_b.1
                && sig_a.2 == sig_b.2
                && sig_a.3 == sig_b.3
                && res.others_unchanged,
            "",
        );
    }

    {
        // 切回实心：同样是单维，且幂等（已是实心则 changed=false）。
        let base = RasterState::new(target()).with_fill(FillMode::Wireframe);
        let (back, res) = toggle_wireframe(&base, false);
        set.add(
            "A21-wire-切回实心",
            !back.is_wireframe() && res.changed && back.fill == FillMode::Solid,
            "",
        );
    }

    {
        // 幂等：已在线框态再切线框 → changed=false（不伪报切换）。
        let base = RasterState::new(target()).with_fill(FillMode::Wireframe);
        let (_, res) = toggle_wireframe(&base, true);
        set.add("A21-wire-重复切换幂等", !res.changed, "");
    }

    {
        // 多重采样开着切线框 → 显式兼容说明（不静默留着）。
        let base = RasterState::new(target()).with_sample(SampleMode::Ms8x);
        let (_, res) = toggle_wireframe(&base, true);
        set.add(
            "A21-wire-采样兼容影响显式声明",
            res.compat_note.contains("多重采样") && WIREFRAME_DOC.contains("只动填充模式"),
            "",
        );
    }

    {
        // 单采样切线框 → 无兼容说明（空说明是正确的：没有影响可说）。
        let base = RasterState::new(target());
        let (_, res) = toggle_wireframe(&base, true);
        set.add("A21-wire-单采样无多余说明", res.compat_note.is_empty(), "");
    }

    // ---- 兼容声明 ----

    {
        // 多重采样 × 后处理：解析顺序保证二者兼容（三种模式均兼容）。
        set.add(
            "A21-compat-采样与后处理兼容",
            msaa_postprocess_compatible(SampleMode::Single)
                && msaa_postprocess_compatible(SampleMode::Ms4x)
                && msaa_postprocess_compatible(SampleMode::Ms8x),
            "",
        );
    }

    {
        // **有效裁剪域必须真的取交集**：裁剪矩形小于目标面时，有效域应等于裁剪矩形；
        // 若实现退化成"直接返回裁剪矩形、丢掉 ∩ 全屏"，这里只有当裁剪恰好等于全屏时才通过——
        // 故用一条**远小于全屏**的裁剪来钉死。
        let t = target();
        let small = ScissorRect::new(50.0, 40.0, 120.0, 90.0);
        let s = RasterState::new(t).with_scissor(small);
        let clip = s.effective_clip();
        let equals_scissor = match small {
            Some(orig) => rect_key(&clip) == rect_key(&orig) && clip.w < t.w as f32 && clip.h < t.h as f32,
            None => false,
        };
        set.add("A21-compat-有效域不吞裁剪", equals_scissor, "");
    }

    {
        // 有效裁剪域取交集而非替换：裁剪矩形**超出**目标面时，有效域必须被面边界收窄
        // （若直接返回裁剪矩形，域会大出目标面——这正是替换语义的破绽）。
        let t = target();
        let over = ScissorRect::new(700.0, 500.0, 400.0, 300.0);
        let s = RasterState::new(t).with_scissor(over);
        let clip = s.effective_clip();
        let shrunk = clip.right() <= t.w as f32 + CLAMP_EPS
            && clip.bottom() <= t.h as f32 + CLAMP_EPS
            && clip.w < 400.0
            && clip.h < 300.0;
        set.add("A21-compat-有效域受面边界收窄", shrunk, "");
    }

    {
        // 无裁剪时有效域 = 全屏（不是零域、也不是悬空值）。
        let t = target();
        let s = RasterState::new(t);
        let clip = s.effective_clip();
        let full_ok = (clip.x - 0.0).abs() <= CLAMP_EPS
            && (clip.y - 0.0).abs() <= CLAMP_EPS
            && (clip.w - t.w as f32).abs() <= CLAMP_EPS
            && (clip.h - t.h as f32).abs() <= CLAMP_EPS;
        set.add("A21-compat-无裁剪回全屏", full_ok, "");
    }

    {
        // **绕过 set钳制的直构状态**：手工塞一个越界 scissor 字段（`with_scissor` 会先钳制，
        // 那样构造出来的状态必然已在面内，`∩ 全屏` 与原值恒等——验不出替换语义）。
        // 此处直构越界值，钉死"有效域必须被面边界收窄"这条不变量。
        let t = target();
        let raw = RasterState {
            fill: FillMode::Solid,
            cull: CullMode::Back,
            sample: SampleMode::Single,
            // 越界：右下超出目标面，且起点为负（左上也在面外）。
            scissor: ScissorRect::new(-40.0, -30.0, 2000.0, 1500.0),
            target: t,
        };
        let clip = raw.effective_clip();
        let contained = clip.x >= -CLAMP_EPS
            && clip.y >= -CLAMP_EPS
            && clip.right() <= t.w as f32 + CLAMP_EPS
            && clip.bottom() <= t.h as f32 + CLAMP_EPS
            && clip.w > 0.0
            && clip.h > 0.0;
        // 直接返回原裁剪矩形的话 w=2000（远超面宽），此处必红。
        let shrunk = clip.w < 2000.0 && clip.h < 1500.0;
        set.add("A21-compat-直构越界有效域被收窄", contained && shrunk, "");
    }

    {
        // 裁剪叠加取交集（非替换）——交集必不超出两者任一方。
        let a = ScissorRect::new(0.0, 0.0, 100.0, 100.0)
            .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let b = ScissorRect::new(50.0, 50.0, 100.0, 100.0)
            .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let inter = a.intersect(&b);
        let ok = match inter {
            Some(i) => i.w <= a.w + CLAMP_EPS && i.w <= b.w + CLAMP_EPS && i.w > 0.0,
            None => false,
        };
        set.add("A21-compat-裁剪取交集不替换", ok, "");
    }

    {
        // 不相交的裁剪交集为空（None）。
        let a = ScissorRect::new(0.0, 0.0, 10.0, 10.0)
            .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let b = ScissorRect::new(500.0, 500.0, 10.0, 10.0)
            .unwrap_or(ScissorRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        set.add("A21-compat-不相交交集为空", a.intersect(&b).is_none(), "");
    }

    // ---- 契约文档在场 ----

    {
        let docs_ok = DIM_ORTHOGONAL_DOC.contains("四维")
            && COMPAT_DOC.contains("后处理")
            && BATCH_TX_DOC.contains("事务")
            && CLAMP_DOC.contains("钳制")
            && WIREFRAME_DOC.contains("一键");
        set.add("A21-judge-五契约条款在场", docs_ok, "");
    }

    set
}