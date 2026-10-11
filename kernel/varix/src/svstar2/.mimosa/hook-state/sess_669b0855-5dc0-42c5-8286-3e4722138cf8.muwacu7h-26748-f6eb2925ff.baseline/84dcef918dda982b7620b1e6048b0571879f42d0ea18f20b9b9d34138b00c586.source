//! VE-F0603 · 图层不透明度与淡入淡出（VE-D 域 · 2D 合成引擎 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0603`
//!
//! **判据（锚点原文）**：图层不透明度：标量作用于整层子树——子树先合成为
//! 独立组再整体乘 alpha（分组合成边界），隐式触发 F0605 隔离组语义（不透明度
//! 小于 1 即隐式隔离——条件清单成员之一）；与混合模式叠加语义：先组内混合后
//! 组 alpha 再与外部混合（顺序契约与 F0625 预乘纪律一致）；淡入淡出：与
//! F0608 插值接口联动（alpha 通道为可动画属性）；边界行为：0 跳过绘制但保留
//! 布局与命中豁免语义、1 走不透明快速路径（跳过分组合成）。
//!
//! **错误路径与降级矩阵**：值域钳制（0-1 外拒绝）；分组深度爆炸→隔离组复用
//! 合并；动画打断→当前值为起点续走。
//!
//! **设计要点**：
//! - **分组语义**：alpha 是子树级标量。嵌套组的有效 alpha 相乘
//!   （`group_alpha = 父组 × 层`）——这是"整组再乘"语义在嵌套下的唯一自洽解；
//! - **顺序契约**：组内混合 → 组 alpha → 外部混合，三段顺序以
//!   `ORDER_CONTRACT_DOC` 在册；`apply_group_alpha` 按 F0625 预乘纪律
//!   实现整体乘 alpha（rgb 与 a 同乘——预乘域的整体淡出就是各通道等比缩放）；
//! - **隐式隔离**：alpha < 1 即隐式隔离（F0605 四条件之一），本条提供
//!   该条件的判定与文档挂点——语义定义在 F0605，实现条不抢；
//! - **边界行为**：alpha=1 不透明快速路径（O(1) 判定，跳过分组合成）；
//!   alpha=0 跳过绘制但**保留布局**（结构仍在树里）且**命中豁免**
//!   （全透明层不拦截点击）；
//! - **值域纪律**：0-1 外拒绝（不静默钳制——静默钳制会让"看不见"和
//!   "设错了"混为一谈）；NaN/Inf 同样拒绝（零容忍）；
//! - **淡入淡出**：alpha 是可动画属性（F0608 联动挂点）。线性渐变按逻辑
//!   tick 步进；打断重定目标时**以当前值为起点续走**（不跳变、不重放）；
//! - **分组深度爆炸→复用合并**：连续两层纯 alpha 隔离组合并为单层
//!   （alpha 相乘）——深度上限 `MAX_ISOLATION_DEPTH` 兜底，超过即合并。
//!
//! **跨批对接点**：上游 F0601 结构、F0605 隔离语义；下游 F0608 动画、
//! F0628 GPU 实现。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 隔离组深度上限。超过即触发相邻纯 alpha 组合并（分组深度爆炸→复用合并）。
pub const MAX_ISOLATION_DEPTH: usize = 16;

// ---------------------------------------------------------------------------
// 二、顺序契约文档（判据点名：先组内混合后组 alpha 再与外部混合）
// ---------------------------------------------------------------------------

/// 顺序契约（人读文本，与 F0625 预乘纪律一致）。
pub const ORDER_CONTRACT_DOC: &str = "\
不透明度顺序契约（VE-F0603 · v1，与 F0625 预乘纪律一致）：
S1 组内混合：子树各层先在组内按各自混合模式混合（独立组合成）。
S2 组 alpha：合成结果整体乘组不透明度（预乘域：rgb 与 a 等比缩放）。
S3 外部混合：乘过 alpha 的组作为整体参与父级混合。
S4 顺序不可换：先乘 alpha 再组内混合 = 混合对象被错误衰减（半透明叠加
   的结果依赖顺序，两义即视觉错误）。";

/// 隐式隔离条件挂点（本条是 F0605 条件清单的"不透明度"成员）。
pub const IMPLICIT_ISOLATION_DOC: &str = "\
隐式隔离条件清单（VE-F0603 挂点 · 全集定义归 F0605）：
- 不透明度 < 1（本条贡献的条件：requires_isolation）；
- 混合模式非 normal（F0605/F0621）；
- 效果链非空（F0605/F0609）；
- 蒙版存在（F0605/F0650）。";

// ---------------------------------------------------------------------------
// 三、不透明度值（值域纪律 + 边界行为）
// ---------------------------------------------------------------------------

/// 合成路径三态（O(1) 快速路径判定——性能逐项分解第一条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositingPath {
    /// alpha = 0：跳过绘制，保留布局，命中豁免。
    SkipDraw,
    /// 0 < alpha < 1：分组合成（隐式隔离组）。
    GroupComposite,
    /// alpha = 1：不透明快速路径，跳过分组合成。
    OpaqueFastPath,
}

/// 不透明度值（构造即校验——0-1 外拒绝，NaN/Inf 拒绝）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opacity(f32);

impl Opacity {
    /// 构造并校验（值域钳制纪律：0-1 外拒绝，不静默钳制）。
    pub fn new(v: f32) -> Result<Self, AlphaError> {
        if !v.is_finite() {
            return Err(AlphaError::NonFinite(v));
        }
        if !(0.0..=1.0).contains(&v) {
            return Err(AlphaError::OutOfRange(v));
        }
        Ok(Opacity(v))
    }

    /// 不透明（1.0）便捷构造。
    pub const OPAQUE: Opacity = Opacity(1.0);
    /// 全透明（0.0）便捷构造。
    pub const TRANSPARENT: Opacity = Opacity(0.0);

    /// 取值。
    pub fn value(self) -> f32 {
        self.0
    }

    /// 合成路径判定（O(1)——快速路径判定的全部逻辑就是两次浮点比较）。
    pub fn path(self) -> CompositingPath {
        if self.0 == 0.0 {
            CompositingPath::SkipDraw
        } else if self.0 == 1.0 {
            CompositingPath::OpaqueFastPath
        } else {
            CompositingPath::GroupComposite
        }
    }

    /// 隐式隔离判定（F0605 条件清单成员：不透明度 < 1 即隐式隔离）。
    pub fn requires_isolation(self) -> bool {
        self.0 < 1.0
    }

    /// 命中判定（边界行为：alpha=0 命中豁免——全透明层不拦截点击）。
    pub fn hit_allowed(self) -> bool {
        self.0 > 0.0
    }

    /// 嵌套组的有效 alpha（分组语义：整组再乘在嵌套下 = 相乘）。
    pub fn nested(self, parent_group: Opacity) -> Opacity {
        Opacity(self.0 * parent_group.0)
    }

    /// 隔离组复用合并（分组深度爆炸的处置）：相邻两层纯 alpha 组合并为
    /// 单层，alpha 相乘——合并后语义等价（数学：乘法结合律）。
    pub fn merge(self, inner: Opacity) -> Result<Opacity, AlphaError> {
        Opacity::new(self.0 * inner.0)
    }
}

/// alpha 错误（错误路径：值域钳制 0-1 外拒绝 + 非有限拒绝）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaError {
    /// 0-1 外。
    OutOfRange(f32),
    /// NaN/Inf。
    NonFinite(f32),
}

impl AlphaError {
    /// 错误码。
    pub fn code(&self) -> &'static str {
        match self {
            AlphaError::OutOfRange(_) => "E_OUT_OF_RANGE",
            AlphaError::NonFinite(_) => "E_NON_FINITE",
        }
    }

    /// 人读建议（三要素：下一步）。
    pub fn next_hint(&self) -> &'static str {
        match self {
            AlphaError::OutOfRange(_) => "把值收进 [0,1] 再设置；如需超界表达请走上层效果，不是不透明度",
            AlphaError::NonFinite(_) => "上游计算产生了 NaN/Inf，先修计算路径再设置",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、组 alpha 的预乘应用（顺序契约 S2 的实现，F0625 纪律）
// ---------------------------------------------------------------------------

/// 预乘色（rgb 已预乘 a：F0625 纪律的输入形态）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PremulColor {
    /// 预乘 rgb。
    pub rgb: [f32; 3],
    /// alpha。
    pub a: f32,
}

/// 组 alpha 整体应用（顺序契约 S2）：预乘域内 rgb 与 a **等比缩放**——
/// "整组再乘 alpha"在预乘域的正确形态就是各通道同乘组系数。
///
/// group 系数 ≤ 1：组只能让组结果更透明，不能无中生有（防线断言）。
pub fn apply_group_alpha(c: &PremulColor, group: Opacity) -> Option<PremulColor> {
    let g = group.value();
    if !g.is_finite() {
        return None;
    }
    Some(PremulColor {
        rgb: [c.rgb[0] * g, c.rgb[1] * g, c.rgb[2] * g],
        a: c.a * g,
    })
}

// ---------------------------------------------------------------------------
// 五、淡入淡出（F0608 联动：alpha 通道是可动画属性）
// ---------------------------------------------------------------------------

/// 淡入淡出状态机：current → target，每 tick 步进 step。
///
/// 打断纪律：`retarget` 换目标时**以当前值为起点续走**——不跳变、不重放。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    /// 当前值。
    pub current: Opacity,
    /// 目标值。
    pub target: Opacity,
    /// 每 tick 步进量（>0）。
    pub step: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeStep {
    /// 渐变中：给出新当前值。
    Stepping(Opacity),
    /// 已到达目标（快速路径可接管）。
    Reached(Opacity),
}

impl Fade {
    /// 构造渐变（step 非有限或 ≤0 拒绝——零步进渐变是死循环）。
    pub fn new(current: Opacity, target: Opacity, step: f32) -> Result<Self, AlphaError> {
        if !step.is_finite() || step <= 0.0 {
            return Err(AlphaError::NonFinite(step));
        }
        Ok(Fade { current, target, step })
    }

    /// 推进一步（逻辑 tick 注入）。
    pub fn tick(&mut self) -> FadeStep {
        let cur = self.current.value();
        let tgt = self.target.value();
        if cur == tgt {
            return FadeStep::Reached(self.current);
        }
        let next = if cur < tgt {
            (cur + self.step).min(tgt)
        } else {
            (cur - self.step).max(tgt)
        };
        self.current = Opacity(next);
        if next == tgt {
            FadeStep::Reached(self.current)
        } else {
            FadeStep::Stepping(self.current)
        }
    }

    /// 打断重定目标（错误路径：动画打断→当前值为起点续走）。
    pub fn retarget(&mut self, new_target: Opacity) {
        self.target = new_target; // current 不动——起点就是当前值
    }

    /// 是否仍在渐变。
    pub fn animating(&self) -> bool {
        self.current.value() != self.target.value()
    }
}

// ---------------------------------------------------------------------------
// 六、动画标记查询挂点（无障碍：闪烁类动画由 UI 层遵循减弱动效偏好）
// ---------------------------------------------------------------------------

/// 动画标记查询挂点（树侧预留——F0601 语义标注挂点的 alpha 面消费端）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationMarker {
    /// 该层 alpha 是否正在动画。
    pub animating: bool,
    /// 动画是否属于闪烁类（UI 层按减弱动效偏好拦截的输入依据）。
    pub flicker_class: bool,
}

impl AnimationMarker {
    /// 读屏可读摘要。
    pub fn screen_text(self) -> String {
        if self.animating {
            if self.flicker_class {
                "该层 alpha 动画中（闪烁类，可被减弱动效偏好拦截）".to_string()
            } else {
                "该层 alpha 动画中".to_string()
            }
        } else {
            "该层 alpha 静止".to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0603 域自检（判据逐条映射见 `ved03_checks.rs`）。
pub fn run_ved03_checks() -> CheckSet {
    super::ved03_checks::run_ved03_checks()
}

/// 读屏摘要。
pub fn screen_text(o: Opacity) -> String {
    format!("不透明度：{:.3}", o.value())
}
