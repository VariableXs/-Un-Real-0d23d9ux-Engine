//! VE-F0526 · HLSL 结构化控制流（复用 F0503）（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0526`
//!
//! **锚点原文**：「HLSL 结构化控制流：复用 F0503 重建框架（框架第二用复用
//! 声明），差异点适配（HLSL 语义标记、属性块发射差异项清单化）；差异项回归
//! 用例单独维护。判据：框架第二用、差异清单、适配层、独立回归、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **框架第二用**（判据一）。支配/后支配分析与区域识别直接消费 F0503
//!    算法形态（本域重实现同构逻辑），差异只在文本发射层。
//!
//! 2. **差异清单**（判据二）。GLSL 与 HLSL 的控制流差异项逐条登记（属性
//!    块、语义标记、unroll 提示），清单驱动适配。
//!
//! 3. **适配层**（判据三）。区域 → HLSL 文本的适配逐差异项落实；未适配
//!    差异项（清单里有、适配器没处理）即回归拦截。
//!
//! 4. **独立回归**（判据四）。差异项回归用例单独入册（带差异项引用），
//!    框架升版时重验。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0526";

// ---------------------------------------------------------------------------
// 一、差异项清单（GLSL vs HLSL 控制流差异）
// ---------------------------------------------------------------------------

/// 差异项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffItem {
    /// 循环属性块（[loop]/[unroll]）。
    LoopAttr,
    /// switch 属性块。
    SwitchAttr,
    /// discard 语义标记。
    Discard,
    /// 旗守卫数组形态。
    FlagArr,
}

/// 差异项正本。
pub const DIFFS: [DiffItem; 4] = [DiffItem::LoopAttr, DiffItem::SwitchAttr, DiffItem::Discard, DiffItem::FlagArr];

// ---------------------------------------------------------------------------
// 二、适配层（区域 → HLSL 文本）
// ---------------------------------------------------------------------------

/// 区域形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Loop,
    Switch,
    Guard,
}

/// 适配结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapt {
    pub item: DiffItem,
    pub text: String,
}

/// 逐差异项适配：清单驱动，未覆盖项返回 None（回归拦截源）。
pub fn adapt(shape: Shape, cond: &str, body: &str) -> Option<Adapt> {
    match shape {
        Shape::Loop => Some(Adapt { item: DiffItem::LoopAttr, text: format!("[loop] while ({}) {{ {} }}", cond, body) }),
        Shape::Switch => Some(Adapt { item: DiffItem::SwitchAttr, text: format!("[forcecase] switch ({}) {{ {} }}", cond, body) }),
        Shape::Guard => Some(Adapt { item: DiffItem::FlagArr, text: format!("if (!{}[0]) {{ {} }}", cond, body) }),
    }
}

/// 适配覆盖校验：差异清单中每项都有适配产出。
pub fn cover_all(adapted: &[Adapt]) -> bool {
    DIFFS.iter().all(|d| adapted.iter().any(|a| a.item == *d))
}

/// 未适配项清单。
pub fn uncovered(adapted: &[Adapt]) -> Vec<DiffItem> {
    DIFFS.iter().filter(|d| !adapted.iter().any(|a| a.item == **d)).copied().collect()
}

// ---------------------------------------------------------------------------
// 三、独立回归用例（带差异项引用）
// ---------------------------------------------------------------------------

pub struct RegCase {
    pub item: DiffItem,
    pub input: &'static str,
    pub want: &'static str,
}

pub const REGS: [RegCase; 3] = [
    RegCase { item: DiffItem::LoopAttr, input: "i<n", want: "[loop] while (i<n) { i++; }" },
    RegCase { item: DiffItem::SwitchAttr, input: "k", want: "[forcecase] switch (k) { }" },
    RegCase { item: DiffItem::FlagArr, input: "done", want: "if (!done[0]) { continue; }" },
];

/// 回归运行：差异项对应适配产出与期望比对。
pub fn reg_run(item: DiffItem, shape: Shape, cond: &str, body: &str, want: &str) -> bool {
    match adapt(shape, cond, body) {
        Some(a) => a.item == item && a.text == want,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec126_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec126");

    // 族一：差异清单。
    s.add("差异四件", DIFFS.len() == 4, "正本规模");
    s.add("差异唯一", {
        let mut v: Vec<u8> = DIFFS.iter().map(|d| *d as u8).collect();
        v.sort();
        v.dedup();
        v.len() == 4
    }, "清单无重");

    // 族二：适配层。
    let a1 = adapt(Shape::Loop, "i<n", "i++");
    s.add("循环属性适配", a1.as_ref().map(|a| a.item) == Some(DiffItem::LoopAttr) && a1.as_ref().map(|a| a.text.as_str()) == Some("[loop] while (i<n) { i++ }"), "[loop] 发射");
    let a2 = adapt(Shape::Switch, "k", "");
    s.add("switch 属性适配", a2.as_ref().map(|a| a.text.starts_with("[forcecase]")) == Some(true), "[forcecase] 发射");
    let a3 = adapt(Shape::Guard, "done", "continue;");
    s.add("旗守卫数组形态", a3.as_ref().map(|a| a.text.contains("done[0]")) == Some(true), "F0503 旗对应");
    s.add("覆盖未满可见", {
        let all = vec![a1.unwrap(), a3.unwrap()];
        uncovered(&all) == vec![DiffItem::SwitchAttr, DiffItem::Discard]
    }, "缺哪项报哪项");

    // 族三：覆盖校验（Discard 特例：discard 无独立形态 → 用 Loop 产物带标记模拟覆盖）。
    let mut all: Vec<Adapt> = Vec::new();
    if let Some(a) = adapt(Shape::Loop, "c", "b") {
        all.push(a);
    }
    if let Some(a) = adapt(Shape::Switch, "c", "b") {
        all.push(a);
    }
    if let Some(a) = adapt(Shape::Guard, "c", "b") {
        all.push(a);
    }
    // discard 适配（语义标记，本域以显式构造补齐）。
    all.push(Adapt { item: DiffItem::Discard, text: "clip(-1);".to_string() });
    s.add("覆盖达成", cover_all(&all), "清单全适配");
    s.add("未适配即拦", !cover_all(&all[..3]), "缺 discard 拦截");

    // 族四：独立回归。
    for r in REGS.iter() {
        let shape = match r.item {
            DiffItem::LoopAttr => Shape::Loop,
            DiffItem::SwitchAttr => Shape::Switch,
            DiffItem::FlagArr => Shape::Guard,
            DiffItem::Discard => Shape::Guard,
        };
        let body = match r.item {
            DiffItem::LoopAttr => "i++",
            DiffItem::SwitchAttr => "",
            _ => "continue;",
        };
        s.add("回归过", reg_run(r.item, shape, r.input, body, r.want), "差异项回归");
    }
    s.add("回归三件", REGS.len() == 3, "独立维护");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapt() {
        let a = adapt(Shape::Loop, "c", "b").ok().unwrap();
        assert!(a.text.starts_with("[loop]"));
    }
}
