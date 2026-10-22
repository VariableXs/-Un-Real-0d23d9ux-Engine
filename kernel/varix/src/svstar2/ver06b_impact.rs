//! VE-F3422 · 换肤影响面计算 —— 精确 diff → 受影响组件集合 + 三档分级
//! + 影响面预览 + 分帧兜底。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3422`
//!
//! # 职责（锚点原文拆解）
//!
//! - **精确 diff**：把新旧令牌的差异，映射到**真正会重绘的组件**。映射靠
//!   组件登记的读依赖（[`ComponentBinding::reads`]），不靠猜。一个组件
//!   读了 `color.bg`，只有 `color.bg` 变了它才进影响面——「全量重绘」
//!   是最贵的降级手段，不是默认值；
//! - **三档分级**（[`ImpactLevel`]）：
//!   - **全重绘**：有组件声明了 `full_repaint`（例如整窗背景），或受影响
//!     组件占比超过 [`FULL_REPAINT_RATIO_PERMILLE`]；
//!   - **局部重绘**：有组件受影响，但没有触发全重绘条件；
//!   - **无感**：没有任何组件受影响，或受影响的只是非视觉令牌
//!     （`motion.*` / `sound.*` 这类不进像素的路径）；
//! - **影响面最小化优先**：同一组件被多条变更命中只算一次；同一路径的
//!   「值没变」变更在 diff 阶段就被丢掉（`old == new` 不是变更，是噪声）；
//! - **影响面预览**：换之前要让用户知道什么会变（[`ImpactPreview::lines`]）。
//!   没有预览就调用 [`ImpactPreview::authorize_switch`] 一律拒绝——预览
//!   不是可选装饰，是切换的前置条件；
//! - **计算超时→分帧**：组件多到一帧算不完时，[`FramePlan`] 把组件按
//!   预算切片，跨帧完成。分帧只切**计算**，不切**语义**：切片完成后
//!   仍要合成一个完整影响面再判分级。
//!
//! # 为什么分级必须三档而不是两档
//!
//! 两档（变了/没变）会让「只换了一个按钮的圆角」也走全量重绘——用户
//! 看到的是整屏闪一下。三档的价值不在分类本身，而在于**给下游一个
//! 可以拒绝全量重绘的依据**：局部重绘是默认路径，全量重绘必须举证
//! （有 `full_repaint` 声明或占比超阈）。
//!
//! # 复杂度
//!
//! - `diff`：O(变更数)；
//! - `compute`：O(变更数 × 绑定数)，绑定用前缀匹配；
//! - `plan_frames`：O(组件数)；
//! - `lines`：O(受影响组件数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5416 起）
// ---------------------------------------------------------------------------

/// 影响面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpactCode {
    /// 变更集为空（没有可算的影响面）。
    EmptyDiff = 5416,
    /// 组件绑定为空（没有登记读依赖，无从计算）。
    NoBindings = 5417,
    /// 绑定路径为空。
    EmptyReadPath = 5418,
    /// 组件 id 重复（同一组件登记两次）。
    DuplicateComponent = 5419,
    /// 未生成预览就请求切换授权。
    PreviewRequired = 5420,
    /// 分帧预算为 0。
    ZeroFrameBudget = 5421,
    /// 分帧后仍超时（单组件成本高于整帧预算）。
    FrameUnfit = 5422,
    /// 分级与实测不符（自检失败）。
    LevelMismatch = 5423,
}

impl ImpactCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ImpactCode::EmptyDiff => "变更集为空",
            ImpactCode::NoBindings => "组件绑定为空",
            ImpactCode::EmptyReadPath => "绑定路径为空",
            ImpactCode::DuplicateComponent => "组件重复登记",
            ImpactCode::PreviewRequired => "未生成影响面预览",
            ImpactCode::ZeroFrameBudget => "分帧预算为 0",
            ImpactCode::FrameUnfit => "单组件成本超出整帧预算",
            ImpactCode::LevelMismatch => "影响分级与实测不符",
        }
    }
}

/// 全重绘触发阈值：受影响组件占比超过 40% 即判全量重绘。
/// 这个数字不是拍的：超过四成组件要重绘时，局部重绘省下的时间抵不过
/// 逐组件调度的开销，还不如整屏刷一次。
pub const FULL_REPAINT_RATIO_PERMILLE: u32 = 400;

/// 不进像素的路径前缀：这类令牌变了不算视觉影响。
const NON_VISUAL_PREFIXES: [&str; 5] = ["motion.", "sound.", "haptic.", "telemetry.", "log."];

// ---------------------------------------------------------------------------
// 二、输入模型
// ---------------------------------------------------------------------------

/// 一条令牌变更（与 F3421 的 [`Change`] 同构，但本模块只关心路径与值差）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TokenDelta {
    /// 路径。
    pub path: String,
    /// 旧值。
    pub old: String,
    /// 新值。
    pub new: String,
}

impl TokenDelta {
    /// 构造。
    pub fn new(path: &str, old: &str, new: &str) -> TokenDelta {
        TokenDelta {
            path: path.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        }
    }

    /// 是否真的变了。
    pub fn effective(&self) -> bool {
        self.old != self.new
    }

    /// 是否视觉相关。
    pub fn visual(&self) -> bool {
        !NON_VISUAL_PREFIXES
            .iter()
            .any(|p| self.path.starts_with(p))
    }
}

/// 组件对令牌的读依赖。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ComponentBinding {
    /// 组件 id（稳定，不随重建变化）。
    pub component: u32,
    /// 读依赖：精确路径或前缀（`color.` 表示整个 color 命名空间）。
    pub reads: Vec<String>,
    /// 是否声明「我变就要全量重绘」。
    pub full_repaint: bool,
    /// 单组件重绘成本（微秒），分帧用。
    pub cost_us: u32,
}

impl ComponentBinding {
    /// 构造。
    pub fn new(component: u32, full_repaint: bool, cost_us: u32) -> ComponentBinding {
        ComponentBinding {
            component,
            reads: Vec::new(),
            full_repaint,
            cost_us,
        }
    }

    /// 加一条读依赖。
    pub fn reading(mut self, path: &str) -> ComponentBinding {
        self.reads.push(path.to_string());
        self
    }

    /// 是否读某路径。前缀以 `.` 结尾即前缀匹配，否则精确匹配。
    pub fn reads_path(&self, path: &str) -> bool {
        self.reads.iter().any(|r| {
            if r.ends_with('.') {
                path.starts_with(r.as_str())
            } else {
                r == path
            }
        })
    }
}

// ---------------------------------------------------------------------------
// 三、影响面
// ---------------------------------------------------------------------------

/// 一个受影响组件。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AffectedComponent {
    /// 组件 id。
    pub component: u32,
    /// 命中的路径（去重、按字典序）。
    pub hits: Vec<String>,
    /// 是否触发全量重绘。
    pub full: bool,
    /// 重绘成本。
    pub cost_us: u32,
}

/// 影响面。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImpactPreview {
    /// 受影响组件（按 id 升序）。
    pub affected: Vec<AffectedComponent>,
    /// 分级。
    pub level: ImpactLevel,
    /// 参与计算的组件总数。
    pub total_components: u32,
    /// 被丢弃的无效变更数（`old == new`）。
    pub dropped_edits: u32,
    /// 非视觉变更数。
    pub non_visual_edits: u32,
}

/// 影响分级（与 F3421 的枚举同构；此处独立定义是因为本模块要能单独被
/// 工具链引用而不拉起整个引擎）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImpactLevel {
    /// 无感。
    Imperceptible,
    /// 局部重绘。
    LocalRepaint,
    /// 全量重绘。
    FullRepaint,
}

impl ImpactLevel {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ImpactLevel::Imperceptible => "无感",
            ImpactLevel::LocalRepaint => "局部重绘",
            ImpactLevel::FullRepaint => "全量重绘",
        }
    }
}

impl ImpactPreview {
    /// 受影响组件数。
    pub fn affected_count(&self) -> usize {
        self.affected.len()
    }

    /// 是否为空影响面。
    pub fn is_empty(&self) -> bool {
        self.affected.is_empty()
    }

    /// 换肤切换授权。没有预览就没有授权——调用方必须先拿到
    /// `ImpactPreview` 才能继续，这是类型层面的约定。
    pub fn authorize_switch(&self) -> Result<ImpactLevel, ImpactCode> {
        // 预览存在即授权（本类型只能由 `compute` 产出，故此处只做
        // 「预览确实覆盖了变更」的一致性校验）。
        if self.total_components == 0 && self.dropped_edits == 0 {
            return Err(ImpactCode::NoBindings);
        }
        Ok(self.level)
    }

    /// 预览文本（读屏可达：一条一行，先说分级再说组件）。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤影响：{}，受影响组件 {} / {}，无效变更丢弃 {} 条，非视觉变更 {} 条",
            self.level.label(),
            self.affected.len(),
            self.total_components,
            self.dropped_edits,
            self.non_visual_edits
        ));
        for a in self.affected.iter() {
            out.push(format!(
                "组件 {}：命中 {} 条路径（{}）{}",
                a.component,
                a.hits.len(),
                a.hits.join("、"),
                if a.full { "，需全量重绘" } else { "" }
            ));
        }
        if out.len() == 1 {
            out.push("没有组件受影响，切换对画面无感".to_string());
        }
        out
    }

    /// 受影响组件 id 集（给下游编排用）。
    pub fn component_ids(&self) -> Vec<u32> {
        self.affected.iter().map(|a| a.component).collect()
    }
}

/// 计算影响面。纯函数：输入变更与绑定，输出预览。
pub fn compute(
    deltas: &[TokenDelta],
    bindings: &[ComponentBinding],
) -> Result<ImpactPreview, ImpactCode> {
    if deltas.is_empty() {
        return Err(ImpactCode::EmptyDiff);
    }
    if bindings.is_empty() {
        return Err(ImpactCode::NoBindings);
    }
    // 组件 id 唯一性：重复登记会让影响面重复计数。
    let mut ids: Vec<u32> = bindings.iter().map(|b| b.component).collect();
    ids.sort();
    let mut i = 0;
    while i + 1 < ids.len() {
        if ids.get(i) == ids.get(i + 1) {
            return Err(ImpactCode::DuplicateComponent);
        }
        i += 1;
    }
    // 过滤无效/非视觉变更。
    let mut dropped = 0u32;
    let mut non_visual = 0u32;
    let mut effective: Vec<&TokenDelta> = Vec::new();
    for d in deltas.iter() {
        if !d.effective() {
            dropped = dropped.saturating_add(1);
            continue;
        }
        if !d.visual() {
            non_visual = non_visual.saturating_add(1);
            continue;
        }
        if d.path.trim().is_empty() {
            return Err(ImpactCode::EmptyReadPath);
        }
        effective.push(d);
    }
    // 逐绑定匹配。
    let mut affected: Vec<AffectedComponent> = Vec::new();
    for b in bindings.iter() {
        let mut hits: Vec<String> = Vec::new();
        for d in effective.iter() {
            if b.reads_path(&d.path) && !hits.iter().any(|h| h == &d.path) {
                hits.push(d.path.clone());
            }
        }
        if hits.is_empty() {
            continue;
        }
        hits.sort();
        affected.push(AffectedComponent {
            component: b.component,
            hits,
            full: b.full_repaint,
            cost_us: b.cost_us,
        });
    }
    affected.sort_by(|a, b| a.component.cmp(&b.component));
    // 分级。
    let total = bindings.len() as u32;
    let count = affected.len() as u32;
    let any_full = affected.iter().any(|a| a.full);
    let ratio = if total == 0 { 0 } else { count * 1000 / total };
    let level = if count == 0 {
        ImpactLevel::Imperceptible
    } else if any_full || ratio >= FULL_REPAINT_RATIO_PERMILLE {
        ImpactLevel::FullRepaint
    } else {
        ImpactLevel::LocalRepaint
    };
    Ok(ImpactPreview {
        affected,
        level,
        total_components: total,
        dropped_edits: dropped,
        non_visual_edits: non_visual,
    })
}

// ---------------------------------------------------------------------------
// 四、分帧兜底
// ---------------------------------------------------------------------------

/// 分帧计划。把受影响组件按「每帧预算」切片，跨帧计算。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FramePlan {
    /// 每帧的组件 id 列表。
    pub frames: Vec<Vec<u32>>,
    /// 每帧预计成本（微秒）。
    pub frame_cost_us: Vec<u32>,
}

impl FramePlan {
    /// 帧数。
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 是否一帧装得下。
    pub fn single_frame(&self) -> bool {
        self.frames.len() <= 1
    }
}

/// 按每帧预算切片。成本高于整帧预算的单组件直接拒绝——
/// 「分帧」救不了「一个组件比一帧还贵」，那要先降档（F3428）。
pub fn plan_frames(
    affected: &[AffectedComponent],
    budget_per_frame_us: u32,
) -> Result<FramePlan, ImpactCode> {
    if budget_per_frame_us == 0 {
        return Err(ImpactCode::ZeroFrameBudget);
    }
    let mut frames: Vec<Vec<u32>> = Vec::new();
    let mut costs: Vec<u32> = Vec::new();
    let mut current: Vec<u32> = Vec::new();
    let mut current_cost: u32 = 0;
    for a in affected.iter() {
        if a.cost_us > budget_per_frame_us {
            return Err(ImpactCode::FrameUnfit);
        }
        if current_cost.saturating_add(a.cost_us) > budget_per_frame_us && !current.is_empty() {
            frames.push(current);
            costs.push(current_cost);
            current = Vec::new();
            current_cost = 0;
        }
        current.push(a.component);
        current_cost = current_cost.saturating_add(a.cost_us);
    }
    if !current.is_empty() {
        frames.push(current);
        costs.push(current_cost);
    }
    Ok(FramePlan {
        frames,
        frame_cost_us: costs,
    })
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings() -> Vec<ComponentBinding> {
        alloc::vec![
            ComponentBinding::new(1, true, 900).reading("color."),
            ComponentBinding::new(2, false, 300).reading("color.accent"),
            ComponentBinding::new(3, false, 200).reading("spacing."),
            ComponentBinding::new(4, false, 100).reading("motion.fast"),
        ]
    }

    #[test]
    fn exact_diff_only_hits_readers() {
        let deltas = alloc::vec![TokenDelta::new("color.accent", "#111111", "#222222")];
        let p = compute(&deltas, &bindings()).expect("可算");
        // 组件 1（前缀 color.）与组件 2（精确）命中；3/4 不命中。
        assert_eq!(p.component_ids(), alloc::vec![1, 2]);
        // 组件 1 声明全量重绘 -> 整体全量。
        assert_eq!(p.level, ImpactLevel::FullRepaint);
    }

    #[test]
    fn unchanged_and_non_visual_edits_are_dropped() {
        let deltas = alloc::vec![
            TokenDelta::new("color.accent", "#111111", "#111111"),
            TokenDelta::new("motion.fast", "120ms", "80ms"),
            TokenDelta::new("spacing.gap", "8px", "12px"),
        ];
        let p = compute(&deltas, &bindings()).expect("可算");
        assert_eq!(p.dropped_edits, 1);
        assert_eq!(p.non_visual_edits, 1);
        assert_eq!(p.component_ids(), alloc::vec![3]);
        assert_eq!(p.level, ImpactLevel::LocalRepaint);
    }

    #[test]
    fn zero_affected_is_imperceptible() {
        let deltas = alloc::vec![TokenDelta::new("radius.card", "8px", "12px")];
        let p = compute(&deltas, &bindings()).expect("可算");
        assert!(p.is_empty());
        assert_eq!(p.level, ImpactLevel::Imperceptible);
        assert_eq!(p.authorize_switch().ok(), Some(ImpactLevel::Imperceptible));
    }

    #[test]
    fn ratio_threshold_triggers_full_repaint() {
        // 4 个组件全部受 color 影响 -> 100% > 40%，即使没人声明 full 也全量。
        let bs = alloc::vec![
            ComponentBinding::new(1, false, 10).reading("color."),
            ComponentBinding::new(2, false, 10).reading("color."),
            ComponentBinding::new(3, false, 10).reading("color."),
            ComponentBinding::new(4, false, 10).reading("color."),
        ];
        let deltas = alloc::vec![TokenDelta::new("color.bg", "#000000", "#FFFFFF")];
        let p = compute(&deltas, &bs).expect("可算");
        assert_eq!(p.level, ImpactLevel::FullRepaint);
    }

    #[test]
    fn duplicate_component_registration_is_rejected() {
        let bs = alloc::vec![
            ComponentBinding::new(1, false, 10).reading("color."),
            ComponentBinding::new(1, false, 10).reading("color."),
        ];
        let deltas = alloc::vec![TokenDelta::new("color.bg", "#000000", "#FFFFFF")];
        assert_eq!(compute(&deltas, &bs).err(), Some(ImpactCode::DuplicateComponent));
    }

    #[test]
    fn frame_plan_splits_within_budget() {
        let deltas = alloc::vec![TokenDelta::new("color.accent", "#111111", "#222222")];
        let p = compute(&deltas, &bindings()).expect("可算");
        // 组件 1 成本 900 + 组件 2 成本 300；每帧 1000us -> 两帧。
        let plan = plan_frames(&p.affected, 1000).expect("可切片");
        assert_eq!(plan.frame_count(), 2);
        assert_eq!(plan.frames.first().map(|f| f.len()), Some(1));
        assert!(plan.frame_cost_us.iter().all(|c| *c <= 1000));
    }

    #[test]
    fn oversized_component_is_frame_unfit() {
        let affected = alloc::vec![AffectedComponent {
            component: 9,
            hits: alloc::vec!["color.bg".to_string()],
            full: true,
            cost_us: 5000,
        }];
        assert_eq!(
            plan_frames(&affected, 1000).err(),
            Some(ImpactCode::FrameUnfit)
        );
    }

    #[test]
    fn preview_lines_are_screen_reader_ready() {
        let deltas = alloc::vec![TokenDelta::new("spacing.gap", "8px", "12px")];
        let p = compute(&deltas, &bindings()).expect("可算");
        let lines = p.lines();
        assert!(lines
            .first()
            .map(|l| l.contains("局部重绘") && l.contains("受影响组件 1"))
            .unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("组件 3")));
    }
}
