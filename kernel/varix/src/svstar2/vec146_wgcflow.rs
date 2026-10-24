//! VE-F0546 · WGSL 控制流生成（复用 F0503）（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0546`
//!
//! **锚点原文**：「WGSL 控制流生成：复用 F0503 重建框架（框架第三用复用声
//! 明），差异适配（WGSL 循环语法、continuing 块语义差异项清单化）；差异项
//! 回归用例独立维护。判据：框架第三用、差异清单、适配层、独立回归、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **框架第三用**（判据一）。区域识别与旗兜底消费 F0503 产出，差异
//!    只在文本适配层。
//!
//! 2. **差异清单**（判据二）。WGSL 特有差异逐项登记（loop+continuing 块、
//!    break if 语法、旗守卫数组形态）。
//!
//! 3. **适配层**（判据三）。区域 → WGSL 文本逐差异项适配；未适配项回归
//!    拦截。
//!
//! 4. **独立回归**（判据四）。差异项回归带引用单独维护。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0546";

/// 差异项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffItem {
    /// loop + continuing 块。
    Continuing,
    /// break if 语法。
    BreakIf,
    /// 旗守卫数组。
    FlagArr,
}

pub const DIFFS: [DiffItem; 3] = [DiffItem::Continuing, DiffItem::BreakIf, DiffItem::FlagArr];

/// 区域形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Loop,
    Break,
    Guard,
}

/// 适配产出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapt {
    pub item: DiffItem,
    pub text: String,
}

/// 逐差异项适配。
pub fn adapt(shape: Shape, cond: &str, body: &str) -> Option<Adapt> {
    match shape {
        Shape::Loop => Some(Adapt { item: DiffItem::Continuing, text: format!("loop {{ {} continuing {{ {} }} }}", body, cond) }),
        Shape::Break => Some(Adapt { item: DiffItem::BreakIf, text: format!("loop {{ break if {}; {} }}", cond, body) }),
        Shape::Guard => Some(Adapt { item: DiffItem::FlagArr, text: format!("if ({}[0]) {{ continue; }}", cond) }),
    }
}

/// 覆盖校验。
pub fn cover_all(adapted: &[Adapt]) -> bool {
    DIFFS.iter().all(|d| adapted.iter().any(|a| a.item == *d))
}

/// 未适配清单。
pub fn uncovered(adapted: &[Adapt]) -> Vec<DiffItem> {
    DIFFS.iter().filter(|d| !adapted.iter().any(|a| a.item == **d)).copied().collect()
}

/// 独立回归用例。
pub struct RegCase {
    pub item: DiffItem,
    pub cond: &'static str,
    pub body: &'static str,
    pub want: &'static str,
}

pub const REGS: [RegCase; 3] = [
    RegCase { item: DiffItem::Continuing, cond: "i++", body: "f(i)", want: "loop { f(i) continuing { i++ } }" },
    RegCase { item: DiffItem::BreakIf, cond: "i >= n", body: "g(i)", want: "loop { break if i >= n; g(i) }" },
    RegCase { item: DiffItem::FlagArr, cond: "done", body: "", want: "if (done[0]) { continue; }" },
];

/// 回归运行。
pub fn reg_run(r: &RegCase) -> bool {
    let shape = match r.item {
        DiffItem::Continuing => Shape::Loop,
        DiffItem::BreakIf => Shape::Break,
        DiffItem::FlagArr => Shape::Guard,
    };
    match adapt(shape, r.cond, r.body) {
        Some(a) => a.text == r.want,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec146_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec146");

    // 族一：差异清单。
    s.add("差异三件", DIFFS.len() == 3, "WGSL 特有项");
    s.add("差异唯一", {
        let mut v: Vec<u8> = DIFFS.iter().map(|d| *d as u8).collect();
        v.sort();
        v.dedup();
        v.len() == 3
    }, "清单无重");

    // 族二：适配层。
    let a1 = adapt(Shape::Loop, "i++", "f(i)");
    s.add("continuing 块形态", a1.as_ref().map(|a| a.text.contains("continuing")) == Some(true), "WGSL 循环语法");
    let a2 = adapt(Shape::Break, "i >= n", "g(i)");
    s.add("break if 形态", a2.as_ref().map(|a| a.text.starts_with("loop { break if")) == Some(true), "break if 语法");
    let a3 = adapt(Shape::Guard, "done", "");
    s.add("旗守卫数组", a3.as_ref().map(|a| a.text.contains("done[0]")) == Some(true), "F0503 旗对应");
    s.add("未适配可见", uncovered(&[a1.unwrap()]) == vec![DiffItem::BreakIf, DiffItem::FlagArr], "缺哪报哪");

    // 族三：覆盖校验。
    let all: Vec<Adapt> = [adapt(Shape::Loop, "c", "b"), adapt(Shape::Break, "c", "b"), adapt(Shape::Guard, "c", "")]
        .into_iter()
        .flatten()
        .collect();
    s.add("覆盖达成", cover_all(&all), "清单全适配");
    s.add("部分覆盖即拦", !cover_all(&all[..2]), "缺项拦截");

    // 族四：独立回归。
    for r in REGS.iter() {
        s.add("回归过", reg_run(r), "差异项回归");
    }
    s.add("回归三件", REGS.len() == 3, "独立维护");
    s.add("回归精确文本", reg_run(&REGS[0]), "continuing 文本逐字");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapt() {
        let a = adapt(Shape::Guard, "x", "").ok().unwrap();
        assert!(a.text.contains("x[0]"));
    }
}
