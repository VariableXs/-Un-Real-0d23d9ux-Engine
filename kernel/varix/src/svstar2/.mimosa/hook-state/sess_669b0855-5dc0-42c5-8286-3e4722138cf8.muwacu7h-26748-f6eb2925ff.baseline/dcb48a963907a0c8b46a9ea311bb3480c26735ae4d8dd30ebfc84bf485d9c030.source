//! VE-F0604 · 图层裁剪系统（VE-D 域 · 2D 合成引擎 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0604`
//!
//! **判据（锚点原文）**：矩形裁剪、路径裁剪、圆角裁剪三形态（圆角的蒙版实现
//! 路径与 F0650 衔接——本条管语义与裁剪域计算，蒙版机制归蒙版组）；裁剪继承：
//! 父子裁剪取交集、包围盒沿树级联收窄（子树有效裁剪域=祖先链逐层求交）；裁剪
//! 与变换交互：裁剪区域随层变换（裁剪矩形定义在层局部坐标系，随世界矩阵变换）；
//! 剔除优化：子树包围盒完全落在裁剪域外即可整树剔除（剔除判定在树遍历 F0614
//! 的标准动作）。判据四条：**三形态、交集级联、随层变换、整树剔除**。
//!
//! **错误路径与降级矩阵**：退化裁剪（空域）→子树跳过；路径裁剪失效→降级包围
//! 盒矩形并登记；交集计算溢出→钳制。
//!
//! **设计要点**：
//! - **三形态**：[`ClipShape::Rect`] 矩形 / [`ClipShape::Path`] 路径（本层只管
//!   裁剪域=路径包围盒，光栅化归蒙版组）/ [`ClipShape::Rounded`] 圆角（域计算
//!   按其 AABB，圆角语义的蒙版实现路径归 F0650——分工显性不越权）；
//! - **交集级联**：子树有效裁剪域 = 祖先链（根→本层）逐层求交，求交 O(深度)；
//!   链上任何一层求交为空 ⇒ 退化空域 ⇒ 整子树跳过（不是裁到零再画，是直接
//!   不进遍历——F0614 遍历的标准动作）；
//! - **随层变换**：裁剪域声明在层局部坐标系，随世界矩阵（上游 F0602 的
//!   [`Mat2D`]）变换到世界域；轴对齐世界域取变换后四角 AABB（旋转层的圆角
//!   语义在蒙版组按局部域精化，世界域剔除用 AABB 足够——注记显性）；
//! - **整树剔除 O(1)**：子树包围盒 vs 有效裁剪域一次矩形判定分三案：域外
//!   （整树剔除）/ 域内（免裁剪直通）/ 部分相交（下发矩形裁剪域）；
//! - **溢出钳制**：求交结果超出 [`COORD_CLAMP`] 即钳制（交集计算溢出→钳制）；
//!   非有限输入（NaN/Inf）显性拒绝不静默；
//! - **命中一致性（F0611 对接）**：命中测试与渲染共用同一有效裁剪域判定
//!   [`EffectiveClip::allows`]——渲染被裁掉的点命中必拒，无双重标准；
//! - **路径失效降级**：路径声明无效（零段/无效包围盒）→ 降级为包围盒矩形
//!   裁剪并登记（降级可见，不静默）。
//!
//! **跨批对接点**：上游 F0601 图层树（稳定 id）/ F0602 变换（世界矩阵）；下游
//! F0614 树遍历（剔除标准动作）、F0650 圆角蒙版（实现路径）、F0611 命中测试
//! （域一致性）。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use super::ved02_xform::Mat2D;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 坐标钳制界：求交/变换结果超出即钳制（交集计算溢出→钳制判据）。
pub const COORD_CLAMP: f32 = 1.0e6;

/// 矩形判定的浮点容差。
pub const RECT_EPS: f32 = 1e-4;

/// 裁剪级联深度上限（防环/防病态深树的护栏；超限显性报错）。
pub const MAX_CLIP_DEPTH: usize = 64;

/// 分工注记：圆角蒙版实现路径归 F0650（本层只做域计算）。
pub const ROUNDED_MASK_DOC: &str = "圆角裁剪的域计算按 AABB 在本层完成；圆角的蒙版实现路径归 F0650 蒙版组";

// ---------------------------------------------------------------------------
// 二、矩形域（轴对齐裁剪域的载体）
// ---------------------------------------------------------------------------

/// 轴对齐矩形域（局部或世界坐标系）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    /// 左上 x。
    pub x: f32,
    /// 左上 y。
    pub y: f32,
    /// 宽（≥0）。
    pub w: f32,
    /// 高（≥0）。
    pub h: f32,
}

impl RectF {
    /// 构造：负宽高显性拒绝（不静默取绝对值）。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Result<Self, &'static str> {
        if !(w >= 0.0 && h >= 0.0) {
            return Err("矩形宽高必须 ≥0（负尺寸显性拒绝）");
        }
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return Err("矩形坐标含非有限值（NaN/Inf 显性拒绝）");
        }
        Ok(RectF { x, y, w, h })
    }

    /// 零面积退化矩形（合法但空——级联求交的退化终点）。
    pub fn empty_at(x: f32, y: f32) -> Self {
        RectF { x, y, w: 0.0, h: 0.0 }
    }

    /// 是否退化（零面积）。
    pub fn is_empty(&self) -> bool {
        self.w <= RECT_EPS || self.h <= RECT_EPS
    }

    /// 求交：不相交或退化返回 None（有效裁剪域的求交原语）。
    pub fn intersect(&self, o: &RectF) -> Option<RectF> {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        if x2 - x1 <= RECT_EPS || y2 - y1 <= RECT_EPS {
            return None;
        }
        RectF::new(x1, y1, x2 - x1, y2 - y1).ok()
    }

    /// 包含点（命中一致性判定用；容差内算含）。
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x - RECT_EPS
            && x <= self.x + self.w + RECT_EPS
            && y >= self.y - RECT_EPS
            && y <= self.y + self.h + RECT_EPS
    }

    /// 包含关系：o 完全落在 self 内（剔除"域内免裁剪"判定）。
    pub fn contains_rect(&self, o: &RectF) -> bool {
        o.x >= self.x - RECT_EPS
            && o.y >= self.y - RECT_EPS
            && o.x + o.w <= self.x + self.w + RECT_EPS
            && o.y + o.h <= self.y + self.h + RECT_EPS
    }

    /// 四角坐标（变换到世界域用）。
    fn corners(&self) -> [(f32, f32); 4] {
        [
            (self.x, self.y),
            (self.x + self.w, self.y),
            (self.x, self.y + self.h),
            (self.x + self.w, self.y + self.h),
        ]
    }

    /// 变换后 AABB：四角经矩阵变换取包围盒（随层变换判据的世界域载体）。
    pub fn transformed_aabb(&self, m: &Mat2D) -> Option<RectF> {
        let cs = self.corners();
        let mut minx = f32::INFINITY;
        let mut miny = f32::INFINITY;
        let mut maxx = f32::NEG_INFINITY;
        let mut maxy = f32::NEG_INFINITY;
        for &(cx, cy) in &cs {
            let (px, py) = m.apply(cx, cy);
            if !px.is_finite() || !py.is_finite() {
                return None; // 非有限变换显性拒绝。
            }
            minx = minx.min(px);
            miny = miny.min(py);
            maxx = maxx.max(px);
            maxy = maxy.max(py);
        }
        // 溢出钳制。
        let lo = -COORD_CLAMP;
        let hi = COORD_CLAMP;
        let (minx, miny) = (minx.clamp(lo, hi), miny.clamp(lo, hi));
        let (maxx, maxy) = (maxx.clamp(lo, hi), maxy.clamp(lo, hi));
        RectF::new(minx, miny, (maxx - minx).max(0.0), (maxy - miny).max(0.0)).ok()
    }
}

// ---------------------------------------------------------------------------
// 三、三形态裁剪声明
// ---------------------------------------------------------------------------

/// 圆角半径（四角独立；圆角语义的蒙版实现归 F0650）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radius4 {
    /// 左上。
    pub tl: f32,
    /// 右上。
    pub tr: f32,
    /// 右下。
    pub br: f32,
    /// 左下。
    pub bl: f32,
}

impl Radius4 {
    /// 全零半径（= 纯矩形语义）。
    pub const ZERO: Radius4 = Radius4 { tl: 0.0, tr: 0.0, br: 0.0, bl: 0.0 };

    /// 是否全零（全零时圆角裁剪按纯矩形域处理）。
    pub fn is_zero(&self) -> bool {
        self.tl <= 0.0 && self.tr <= 0.0 && self.br <= 0.0 && self.bl <= 0.0
    }

    /// 半径合法性：非负且 ≤ min(w,h)/2（超限显性拒绝——不静默钳到假圆角）。
    pub fn validate(&self, w: f32, h: f32) -> Result<(), &'static str> {
        let max_r = w.min(h) / 2.0;
        for r in [self.tl, self.tr, self.br, self.bl] {
            if !r.is_finite() || r < 0.0 {
                return Err("圆角半径必须为非负有限值");
            }
            if r > max_r + RECT_EPS {
                return Err("圆角半径超过 min(w,h)/2——请先修正尺寸或半径");
            }
        }
        Ok(())
    }
}

/// 路径裁剪的域载体（路径光栅化归蒙版组；本层只消费路径包围盒）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathDomain {
    /// 路径包围盒（局部坐标系）。
    pub bounds: RectF,
    /// 路径段数（0 = 无效路径 → 降级判定依据）。
    pub segments: u32,
}

/// 三形态裁剪声明（定义在层局部坐标系）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipShape {
    /// 矩形裁剪。
    Rect(RectF),
    /// 路径裁剪（域 = 路径包围盒；失效降级见 [`degrade_shape`]）。
    Path(PathDomain),
    /// 圆角裁剪（域按 AABB；圆角语义归 F0650）。
    Rounded { bounds: RectF, radius: Radius4 },
}

impl ClipShape {
    /// 本形态的局部域 AABB（三形态统一求交入口）。
    pub fn domain(&self) -> RectF {
        match self {
            ClipShape::Rect(r) => *r,
            ClipShape::Path(p) => p.bounds,
            ClipShape::Rounded { bounds, .. } => *bounds,
        }
    }

    /// 是否是圆角形态（蒙版组对接标记）。
    pub fn is_rounded(&self) -> bool {
        matches!(self, ClipShape::Rounded { .. })
    }
}

/// 路径失效降级判定：零段路径 → 降级为包围盒矩形（并登记）。
pub fn degrade_shape(shape: &ClipShape) -> Option<(ClipShape, &'static str)> {
    match shape {
        ClipShape::Path(p) if p.segments == 0 => Some((
            ClipShape::Rect(p.bounds),
            "路径裁剪失效（零段路径）→ 降级为包围盒矩形裁剪（已登记）",
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 四、有效裁剪域（交集级联的世界域结论）
// ---------------------------------------------------------------------------

/// 有效裁剪域（祖先链逐层求交后的世界域结论）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectiveClip {
    /// 无裁剪（祖先链无任何裁剪声明）。
    Unclipped,
    /// 退化空域（链上求交为空 ⇒ 子树跳过——F0614 标准动作）。
    Empty { /// 空域发生在链上第几层（0 基）。
            at_depth: usize },
    /// 矩形世界域（含圆角标记：命中/剔除按 AABB，蒙版精化归 F0650）。
    Rect {
        /// 世界域 AABB。
        bounds: RectF,
        /// 链上是否存在圆角形态（命中一致性需蒙版组精化的标记）。
        rounded: bool,
        /// 级联消耗的深度（求交 O(深度) 的实测账）。
        depth_used: usize,
    },
}

impl EffectiveClip {
    /// 点是否被本域放行（命中一致性 F0611：命中与渲染同域同判定）。
    pub fn allows(&self, x: f32, y: f32) -> bool {
        match self {
            EffectiveClip::Unclipped => true,
            EffectiveClip::Empty { .. } => false,
            EffectiveClip::Rect { bounds, .. } => bounds.contains_point(x, y),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、剔除判定（F0614 遍历的标准动作，O(1)）
// ---------------------------------------------------------------------------

/// 子树剔除判定结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullDecision {
    /// 子树包围盒完全在裁剪域外 → 整树剔除（不进遍历）。
    CullWholeSubtree,
    /// 子树包围盒完全在裁剪域内 → 免裁剪直通（省一层裁剪开销）。
    NoClipNeeded,
    /// 部分相交 → 下发矩形裁剪域继续遍历。
    ClipToRect,
}

impl CullDecision {
    /// 人话名（审计与读屏用）。
    pub fn name(self) -> &'static str {
        match self {
            CullDecision::CullWholeSubtree => "整树剔除",
            CullDecision::NoClipNeeded => "免裁剪直通",
            CullDecision::ClipToRect => "下发裁剪域",
        }
    }
}

/// O(1) 剔除判定：子树包围盒 vs 有效裁剪域。
pub fn cull_subtree(subtree_bounds: &RectF, clip: &EffectiveClip) -> CullDecision {
    match clip {
        EffectiveClip::Unclipped => CullDecision::NoClipNeeded,
        EffectiveClip::Empty { .. } => CullDecision::CullWholeSubtree,
        EffectiveClip::Rect { bounds, .. } => {
            if bounds.intersect(subtree_bounds).is_none() {
                CullDecision::CullWholeSubtree
            } else if bounds.contains_rect(subtree_bounds) {
                CullDecision::NoClipNeeded
            } else {
                CullDecision::ClipToRect
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、裁剪系统（声明表 + 级联解析 + 降级/错误账）
// ---------------------------------------------------------------------------

/// 单层裁剪声明（按图层稳定 id 登记）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipDecl {
    /// 图层稳定 id（上游 F0601 的 TreeNode.id）。
    pub node_id: u64,
    /// 裁剪形态（局部坐标系）。
    pub shape: ClipShape,
    /// 是否启用（停用 = 本层不产生裁剪，声明保留）。
    pub enabled: bool,
}

/// 图层裁剪系统：声明表 + 祖先链级联解析 + 降级登记 + 错误账（零静默）。
pub struct ClipSystem {
    decls: Vec<ClipDecl>,
    degraded: Vec<String>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl ClipSystem {
    /// 空系统。
    pub fn new() -> Self {
        ClipSystem {
            decls: Vec::new(),
            degraded: Vec::new(),
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    // -- 只读观测面 -----------------------------------------------------------

    /// 声明数。
    pub fn decls_len(&self) -> usize {
        self.decls.len()
    }

    /// 按 id 查声明。
    pub fn decl_of(&self, node_id: u64) -> Option<&ClipDecl> {
        self.decls.iter().find(|d| d.node_id == node_id)
    }

    /// 降级登记。
    pub fn degraded(&self) -> &[String] {
        &self.degraded
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    fn record_error(&mut self, who: String, code: &'static str, detail: String) {
        self.errors.push((who, code, detail));
    }

    // -- 声明管理 ---------------------------------------------------------------

    /// 登记裁剪声明：形态合法性当场校验（负尺寸/非法半径/非有限值显性拒绝）；
    /// 同 id 重复登记 = 覆盖（留审计）。
    pub fn declare(&mut self, node_id: u64, shape: ClipShape) -> Result<(), &'static str> {
        match &shape {
            ClipShape::Rect(r) => {
                if r.is_empty() {
                    self.record_error(
                        format!("declare(n={node_id})"),
                        "E_DEGENERATE_DECL",
                        "矩形裁剪域为零面积——语义上等于整层裁掉，请改用停用或修正尺寸".to_string(),
                    );
                }
            }
            ClipShape::Rounded { bounds, radius } => {
                radius.validate(bounds.w, bounds.h)?;
                if bounds.is_empty() {
                    self.record_error(
                        format!("declare(n={node_id})"),
                        "E_DEGENERATE_DECL",
                        "圆角裁剪域为零面积——语义上等于整层裁掉，请改用停用或修正尺寸".to_string(),
                    );
                }
            }
            ClipShape::Path(p) => {
                if !p.bounds.w.is_finite() || !p.bounds.h.is_finite() {
                    return Err("路径包围盒含非有限值");
                }
            }
        }
        // 路径失效降级：登记并替换为降级形态。
        if let Some((degraded, why)) = degrade_shape(&shape) {
            self.degraded.push(format!("tick{} n={node_id}：{why}", self.tick));
            self.audits.push(format!("tick{} n={node_id} 路径失效降级生效", self.tick));
            return self.declare_inner(node_id, degraded);
        }
        self.declare_inner(node_id, shape)
    }

    fn declare_inner(&mut self, node_id: u64, shape: ClipShape) -> Result<(), &'static str> {
        if let Some(d) = self.decls.iter_mut().find(|d| d.node_id == node_id) {
            d.shape = shape;
            d.enabled = true;
            self.audits.push(format!("tick{} n={node_id} 裁剪声明覆盖更新", self.tick));
            return Ok(());
        }
        self.decls.push(ClipDecl { node_id, shape, enabled: true });
        Ok(())
    }

    /// 停用/启用声明（不存在则显性报错）。
    pub fn set_enabled(&mut self, node_id: u64, enabled: bool) -> Result<(), &'static str> {
        match self.decls.iter_mut().find(|d| d.node_id == node_id) {
            Some(d) => {
                d.enabled = enabled;
                Ok(())
            }
            None => Err("该图层无裁剪声明，无法启停"),
        }
    }

    // -- 级联解析（求交 O(深度)） ---------------------------------------------

    /// 祖先链（根→本层）的有效裁剪域解析。
    ///
    /// `chain`：(图层稳定 id, 本层世界矩阵) 序列，根在前本层在后。每层的
    /// 裁剪域定义在其局部坐标系，先随世界矩阵变换到世界域 AABB，再逐层
    /// 求交——子树有效裁剪域=祖先链逐层求交（级联判据）。
    pub fn effective_clip(
        &mut self,
        chain: &[(u64, Mat2D)],
    ) -> Result<EffectiveClip, &'static str> {
        if chain.len() > MAX_CLIP_DEPTH {
            self.record_error(
                "effective_clip".to_string(),
                "E_CHAIN_TOO_DEEP",
                format!("祖先链深 {} 超上限 {}——疑环或病态树，显性拒绝", chain.len(), MAX_CLIP_DEPTH),
            );
            return Err("祖先链深度超限");
        }
        let mut acc: Option<RectF> = None;
        let mut rounded = false;
        for (depth, (node_id, world)) in chain.iter().enumerate() {
            let decl = match self.decl_of(*node_id) {
                Some(d) if d.enabled => d,
                _ => continue, // 无声明或停用 = 本层不产生裁剪。
            };
            // 路径失效降级复核（声明登记时已降级；此处兜底防御）。
            let shape = match &decl.shape {
                ClipShape::Path(p) if p.segments == 0 => ClipShape::Rect(p.bounds),
                s => *s,
            };
            let local = shape.domain();
            let world_rect = match local.transformed_aabb(world) {
                Some(r) => r,
                None => {
                    self.record_error(
                        format!("effective_clip(n={node_id})"),
                        "E_NONFINITE_WORLD",
                        "裁剪域随世界矩阵变换产生非有限值——该层裁剪按全域外处置（跳过子树）".to_string(),
                    );
                    return Ok(EffectiveClip::Empty { at_depth: depth });
                }
            };
            if let ClipShape::Rounded { radius, .. } = &shape {
                if !radius.is_zero() {
                    rounded = true;
                }
            }
            acc = Some(match acc {
                None => world_rect,
                Some(prev) => match prev.intersect(&world_rect) {
                    Some(x) => x,
                    None => {
                        // 退化空域：子树跳过（不是裁到零再画）。
                        self.audits.push(format!(
                            "tick{} 深度 {} 处（n={node_id}）级联求交为空 → 子树跳过",
                            self.tick, depth
                        ));
                        return Ok(EffectiveClip::Empty { at_depth: depth });
                    }
                },
            });
        }
        Ok(match acc {
            None => EffectiveClip::Unclipped,
            Some(bounds) => EffectiveClip::Rect { bounds, rounded, depth_used: chain.len() },
        })
    }

    // -- 帧驱动与读屏 ------------------------------------------------------------

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 状态面板（错误/降级账可读出——本域无直接无障碍面，面板供诊断中心）。
    pub fn panel_text(&self) -> String {
        format!(
            "图层裁剪面板：声明 {} 条，降级登记 {} 条，审计 {} 条，错误 {} 条",
            self.decls.len(),
            self.degraded.len(),
            self.audits.len(),
            self.errors.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0604 域自检（判据逐条映射见 `ved04_checks.rs`）。
pub fn run_ved04_checks() -> CheckSet {
    super::ved04_checks::run_ved04_checks()
}

// ---------------------------------------------------------------------------
// 八、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> RectF {
        RectF::new(x, y, w, h).unwrap()
    }

    #[test]
    fn ved04_three_shapes_domain_and_rounded_doc() {
        let r = ClipShape::Rect(rect(0.0, 0.0, 100.0, 80.0));
        let p = ClipShape::Path(PathDomain { bounds: rect(10.0, 10.0, 60.0, 60.0), segments: 8 });
        let rd = ClipShape::Rounded { bounds: rect(0.0, 0.0, 100.0, 80.0), radius: Radius4 { tl: 8.0, tr: 8.0, br: 0.0, bl: 0.0 } };
        assert_eq!(r.domain(), rect(0.0, 0.0, 100.0, 80.0));
        assert_eq!(p.domain(), rect(10.0, 10.0, 60.0, 60.0));
        assert_eq!(rd.domain(), rect(0.0, 0.0, 100.0, 80.0), "圆角域按 AABB");
        assert!(rd.is_rounded() && !r.is_rounded());
        assert!(!ROUNDED_MASK_DOC.is_empty(), "圆角蒙版分工显性");
    }

    #[test]
    fn ved04_intersect_cascade_narrows_and_empties() {
        // 父子取交集，沿树收窄。
        let a = rect(0.0, 0.0, 100.0, 100.0);
        let b = rect(20.0, 20.0, 100.0, 100.0);
        let ab = a.intersect(&b).unwrap();
        assert_eq!(ab, rect(20.0, 20.0, 80.0, 80.0), "交集=收窄");
        // 不相交 → None（空域）。
        let c = rect(500.0, 500.0, 10.0, 10.0);
        assert!(a.intersect(&c).is_none());
        // 退化矩形显性拒绝负尺寸。
        assert!(RectF::new(0.0, 0.0, -1.0, 5.0).is_err());
        assert!(RectF::new(f32::NAN, 0.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn ved04_clip_follows_world_transform() {
        let mut cs = ClipSystem::new();
        // 局部域 (0,0,100,50)，世界矩阵平移 (10,20)。
        let _ = cs.declare(1, ClipShape::Rect(rect(0.0, 0.0, 100.0, 50.0)));
        let m = Mat2D::translation(10.0, 20.0);
        let eff = cs.effective_clip(&[(1, m)]).unwrap();
        match eff {
            EffectiveClip::Rect { bounds, depth_used, .. } => {
                assert_eq!(bounds, rect(10.0, 20.0, 100.0, 50.0), "裁剪域随世界矩阵平移");
                assert_eq!(depth_used, 1, "求交 O(深度) 的深度账");
            }
            other => panic!("应为 Rect 域，实际 {other:?}"),
        }
        // 缩放：局部 100x50 放大 2x → 世界 200x100。
        let _ = cs.declare(2, ClipShape::Rect(rect(0.0, 0.0, 100.0, 50.0)));
        let s = Mat2D::scaling(2.0, 2.0);
        let eff = cs.effective_clip(&[(2, s)]).unwrap();
        assert_eq!(
            eff,
            EffectiveClip::Rect { bounds: rect(0.0, 0.0, 200.0, 100.0), rounded: false, depth_used: 1 }
        );
    }

    #[test]
    fn ved04_cascade_across_chain_with_disabled_layers() {
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(rect(0.0, 0.0, 200.0, 200.0)));
        let _ = cs.declare(2, ClipShape::Rect(rect(50.0, 50.0, 100.0, 100.0)));
        let _ = cs.declare(3, ClipShape::Rect(rect(80.0, 80.0, 100.0, 100.0)));
        let id = Mat2D::IDENTITY;
        // 三层链：0-200 ∩ 50-150 ∩ 80-180 = 80..150。
        let eff = cs.effective_clip(&[(1, id), (2, id), (3, id)]).unwrap();
        assert_eq!(
            eff,
            EffectiveClip::Rect { bounds: rect(80.0, 80.0, 70.0, 70.0), rounded: false, depth_used: 3 }
        );
        // 停用中间层 → 链上少一个约束。
        let _ = cs.set_enabled(2, false);
        let eff = cs.effective_clip(&[(1, id), (2, id), (3, id)]).unwrap();
        assert_eq!(
            eff,
            EffectiveClip::Rect { bounds: rect(80.0, 80.0, 100.0, 100.0), rounded: false, depth_used: 3 }
        );
        // 链上求交为空 → Empty（子树跳过）。
        let _ = cs.declare(4, ClipShape::Rect(rect(900.0, 900.0, 10.0, 10.0)));
        let eff = cs.effective_clip(&[(1, id), (4, id)]).unwrap();
        assert!(matches!(eff, EffectiveClip::Empty { at_depth: 1 }));
    }

    #[test]
    fn ved04_cull_three_decisions_o1() {
        let clip = EffectiveClip::Rect { bounds: rect(0.0, 0.0, 100.0, 100.0), rounded: false, depth_used: 1 };
        // 域外 → 整树剔除。
        assert_eq!(cull_subtree(&rect(200.0, 200.0, 10.0, 10.0), &clip), CullDecision::CullWholeSubtree);
        // 域内 → 免裁剪直通。
        assert_eq!(cull_subtree(&rect(10.0, 10.0, 20.0, 20.0), &clip), CullDecision::NoClipNeeded);
        // 部分相交 → 下发裁剪域。
        assert_eq!(cull_subtree(&rect(90.0, 90.0, 20.0, 20.0), &clip), CullDecision::ClipToRect);
        // 空域 → 整树剔除；无裁剪 → 直通。
        let empty = EffectiveClip::Empty { at_depth: 0 };
        let none = EffectiveClip::Unclipped;
        assert_eq!(cull_subtree(&rect(0.0, 0.0, 1.0, 1.0), &empty), CullDecision::CullWholeSubtree);
        assert_eq!(cull_subtree(&rect(0.0, 0.0, 1.0, 1.0), &none), CullDecision::NoClipNeeded);
    }

    #[test]
    fn ved04_path_invalid_degrades_and_registered() {
        let mut cs = ClipSystem::new();
        // 零段路径 → 降级为包围盒矩形并登记。
        let _ = cs.declare(
            7,
            ClipShape::Path(PathDomain { bounds: rect(0.0, 0.0, 40.0, 40.0), segments: 0 }),
        );
        assert_eq!(cs.degraded().len(), 1, "降级必须登记（零静默）");
        assert!(cs.degraded()[0].contains("降级"));
        match cs.decl_of(7).map(|d| &d.shape) {
            Some(ClipShape::Rect(r)) => assert_eq!(*r, rect(0.0, 0.0, 40.0, 40.0), "降级为包围盒矩形"),
            other => panic!("应为降级矩形，实际 {other:?}"),
        }
        // 有效路径不受影响。
        let _ = cs.declare(
            8,
            ClipShape::Path(PathDomain { bounds: rect(0.0, 0.0, 40.0, 40.0), segments: 12 }),
        );
        assert!(matches!(cs.decl_of(8).map(|d| &d.shape), Some(ClipShape::Path(_))));
    }

    #[test]
    fn ved04_overflow_clamped_and_nonfinite_rejected() {
        // 变换产生越界坐标 → 钳制到 ±COORD_CLAMP。
        let big = rect(0.0, 0.0, 10.0, 10.0);
        let m_huge = Mat2D::scaling(COORD_CLAMP, COORD_CLAMP);
        let t = big.transformed_aabb(&m_huge).unwrap();
        assert!(t.x + t.w <= COORD_CLAMP + 1.0, "溢出被钳制");
        // 非有限变换 → None（显性拒绝）。
        let m_nan = Mat2D { a: f32::NAN, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
        assert!(big.transformed_aabb(&m_nan).is_none());
        // 非法半径显性拒绝。
        let mut cs = ClipSystem::new();
        let bad = cs.declare(
            9,
            ClipShape::Rounded { bounds: rect(0.0, 0.0, 40.0, 40.0), radius: Radius4 { tl: 100.0, tr: 0.0, br: 0.0, bl: 0.0 } },
        );
        assert!(bad.is_err(), "半径超 min(w,h)/2 显性拒绝");
    }

    #[test]
    fn ved04_hit_consistency_with_render() {
        // 命中一致：渲染被裁掉的点，命中必拒（同域同判定）。
        let mut cs = ClipSystem::new();
        let _ = cs.declare(1, ClipShape::Rect(rect(0.0, 0.0, 100.0, 100.0)));
        let _ = cs.declare(2, ClipShape::Rect(rect(20.0, 20.0, 50.0, 50.0)));
        let id = Mat2D::IDENTITY;
        let eff = cs.effective_clip(&[(1, id), (2, id)]).unwrap();
        assert!(eff.allows(30.0, 30.0), "域内点放行");
        assert!(!eff.allows(10.0, 10.0), "域外点拒绝——与渲染裁剪一致");
        assert!(!eff.allows(75.0, 75.0), "级联收窄后的域外点拒绝");
        // 空域全拒。
        let empty = EffectiveClip::Empty { at_depth: 0 };
        assert!(!empty.allows(0.0, 0.0));
    }

    #[test]
    fn ved04_chain_depth_guard_and_panel() {
        let mut cs = ClipSystem::new();
        let id = Mat2D::IDENTITY;
        let mut chain = Vec::new();
        for i in 0..(MAX_CLIP_DEPTH + 1) {
            chain.push((i as u64, id));
        }
        assert!(cs.effective_clip(&chain).is_err(), "超深链显性拒绝");
        assert!(cs.errors().iter().any(|(_, c, _)| *c == "E_CHAIN_TOO_DEEP"));
        let p = cs.panel_text();
        assert!(p.contains("图层裁剪面板"));
    }

    #[test]
    fn ved04_checks_all_green() {
        let set = run_ved04_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0604 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
