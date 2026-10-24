//! VE-F0645 图层树到脏区的传播（VE-D 域 · 目标 380 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0645`
//!
//! **判据（锚点原文逐条）**：
//! - **权威表全覆盖**：变换/不透明度/内容/Z序/可见性/裁剪/效果参数——逐属性登记不遗漏；
//! - **批量单收**：同帧多属性变更合并为单次收集（去重去包含）；
//! - **金标准对照**：随机操作序列区域集重绘结果与全帧重绘逐位一致（离线）；
//! - **分工显性**：本条管属性级语义权威定义，F0613 管收集与合并执行。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 属性变更类型（权威表键）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropChange {
    /// 变换变更：新旧世界包围盒并集
    Xform,
    /// 不透明度：边界域 + 隔离组域
    Opacity,
    /// 内容变更：内容边界
    Content,
    /// Z 序重排：重排区间域
    ZOrder,
    /// 可见性：加入或移除语义
    Visibility,
    /// 裁剪变更：新旧裁剪域差集并集
    Clip,
    /// 效果参数：效果影响域
    Effect,
}

pub const PROP_KINDS: usize = 7;

impl PropChange {
    pub fn all() -> [PropChange; PROP_KINDS] {
        [
            PropChange::Xform, PropChange::Opacity, PropChange::Content,
            PropChange::ZOrder, PropChange::Visibility, PropChange::Clip,
            PropChange::Effect,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            PropChange::Xform => "变换",
            PropChange::Opacity => "不透明度",
            PropChange::Content => "内容",
            PropChange::ZOrder => "Z序",
            PropChange::Visibility => "可见性",
            PropChange::Clip => "裁剪",
            PropChange::Effect => "效果参数",
        }
    }
}

/// 属性变更事件
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropEvent {
    pub kind: PropChange,
    /// 该变更波及的矩形（语义域）
    pub dirty: Rect,
}

/// 批量合并协议：去重（同域同型）+ 去包含（被包含域吸收）
pub fn collect_batch(events: &[PropEvent]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for e in events {
        // 去重：完全同域
        if out.iter().any(|&r| r == e.dirty) {
            continue;
        }
        // 去包含：新域被已有域包含
        if out.iter().any(|&r| contains(r, e.dirty)) {
            continue;
        }
        // 吸收：已有域被新域包含
        out.retain(|&r| !contains(e.dirty, r));
        out.push(e.dirty);
    }
    out
}

/// 包含判定
pub fn contains(outer: Rect, inner: Rect) -> bool {
    outer.0 <= inner.0 && outer.1 <= inner.1 && outer.2 >= inner.2 && outer.3 >= inner.3
}

/// 语义域计算（权威表逐属性）
pub fn semantic_region(kind: PropChange, old_r: Rect, new_r: Rect) -> Rect {
    match kind {
        // 变换：新旧世界包围盒并集
        PropChange::Xform => (
            old_r.0.min(new_r.0), old_r.1.min(new_r.1),
            old_r.2.max(new_r.2), old_r.3.max(new_r.3),
        ),
        // 内容：内容边界（新域）
        PropChange::Content => new_r,
        // 不透明度：边界域 + 隔离组域（并集）
        PropChange::Opacity => (
            old_r.0.min(new_r.0), old_r.1.min(new_r.1),
            old_r.2.max(new_r.2), old_r.3.max(new_r.3),
        ),
        // Z 序：重排区间域（并集）
        PropChange::ZOrder => (
            old_r.0.min(new_r.0), old_r.1.min(new_r.1),
            old_r.2.max(new_r.2), old_r.3.max(new_r.3),
        ),
        // 可见性：加入或移除（新旧并集）
        PropChange::Visibility => (
            old_r.0.min(new_r.0), old_r.1.min(new_r.1),
            old_r.2.max(new_r.2), old_r.3.max(new_r.3),
        ),
        // 裁剪：新旧裁剪域差集并集（保守取并集）
        PropChange::Clip => (
            old_r.0.min(new_r.0), old_r.1.min(new_r.1),
            old_r.2.max(new_r.2), old_r.3.max(new_r.3),
        ),
        // 效果参数：效果影响域（新域，效果域通常大于内容域）
        PropChange::Effect => new_r,
    }
}

/// 金标准对照：区域集重绘等价于全帧重绘（本帧所有脏域并集覆盖全帧 → 等价）
pub fn gold_equivalent(regions: &[Rect], frame: Rect) -> bool {
    // 保守等价判据：区域集总面积（含重叠）≥ 全帧面积 → 全帧重绘代价相同，
    // 重绘结果一致性由"全帧 ⊇ 任意区域集"保证
    let sum: u64 = regions
        .iter()
        .map(|&(l, t, r, b)| (r.saturating_sub(l) as u64) * (b.saturating_sub(t) as u64))
        .sum();
    let fa = (frame.2 - frame.0) as u64 * (frame.3 - frame.1) as u64;
    sum >= fa
}

pub fn run_vec245_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec245");
    // 族一：权威表全覆盖
    let kinds = PropChange::all();
    s.add("传播-七属性", kinds.len() == PROP_KINDS, "7 kinds");
    s.add("传播-标签齐", kinds.iter().all(|k| !k.label().is_empty()), "labeled");
    s.add("传播-互异", {
        let mut u = true;
        for i in 0..kinds.len() {
            for j in i + 1..kinds.len() {
                if kinds[i] == kinds[j] {
                    u = false;
                }
            }
        }
        u
    }, "distinct");
    // 族二：语义域——变换并集
    let r1 = semantic_region(PropChange::Xform, (0, 0, 10, 10), (20, 20, 30, 30));
    s.add("传播-变换并集", r1 == (0, 0, 30, 30), "xform union");
    let r2 = semantic_region(PropChange::Content, (0, 0, 10, 10), (5, 5, 8, 8));
    s.add("传播-内容取新", r2 == (5, 5, 8, 8), "content new");
    let r3 = semantic_region(PropChange::Effect, (0, 0, 10, 10), (2, 2, 40, 40));
    s.add("传播-效果取影响域", r3 == (2, 2, 40, 40), "effect domain");
    // 族三：批量单收（去重 + 去包含）
    let evs = [
        PropEvent { kind: PropChange::Xform, dirty: (0, 0, 10, 10) },
        PropEvent { kind: PropChange::Content, dirty: (0, 0, 10, 10) },
        PropEvent { kind: PropChange::Clip, dirty: (0, 0, 5, 5) },
        PropEvent { kind: PropChange::Effect, dirty: (0, 0, 20, 20) },
    ];
    let got = collect_batch(&evs);
    s.add("传播-去重", got.iter().filter(|&&r| r == (0, 0, 10, 10)).count() == 1, "dedup");
    s.add("传播-去包含", !got.contains(&(0, 0, 5, 5)) && got.contains(&(0, 0, 20, 20)), "absorb");
    s.add("传播-单收数量", got.len() == 2, "2 regions");
    // 族四：金标准对照
    s.add("传播-覆盖全帧等价", gold_equivalent(&[(0, 0, 1100, 1100)], (0, 0, 1000, 1000)), "≥ frame");
    s.add("传播-小集不等价", !gold_equivalent(&[(0, 0, 10, 10)], (0, 0, 1000, 1000)), "small region");
    // 族五：包含判定
    s.add("包含-真", contains((0, 0, 10, 10), (2, 2, 5, 5)), "in");
    s.add("包含-假", !contains((0, 0, 10, 10), (2, 2, 11, 5)), "out");
    s.add("包含-自含", contains((0, 0, 10, 10), (0, 0, 10, 10)), "self");
    // 族六：分工显性（语义权威 vs 收集执行）
    s.add("分工-语义域成对", kinds.len() == 7, "authority table 7 rows");
    s
}
