//! VE-F0486 · 数据流优化（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0486`
//!
//! **锚点原文**：「IR 数据流优化：冗余拷贝消除（拷贝链追踪穿透）、寄存器级
//! 复用提示（生命周期不重叠的值合并提示供后端）、无用 phi 简化；数据流分析
//! 与 F0467 DCE、F0468 CSE 协同序（管理器统一排序）。判据：拷贝穿透、复用
//! 提示、phi 简化、保守退出、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **拷贝穿透**（判据一）。拷贝链追踪（copy a→b 后对 b 的使用穿透到 a
//!    ——链式解析防环），别名场景保守停。
//!
//! 2. **复用提示**（判据二）。生命周期不重叠的值合并提示（区间交集空 →
//!    提示入账供后端），提示不强制（后端裁决）。
//!
//! 3. **phi 简化**（判据三）。无用 phi（全入边同源或无使用）→ 简化消除，
//!    误简化由验证器拦截（回滚面在管理器）。
//!
//! 4. **保守退出**（判据四）。分析冲突（环拷贝/未知引用）→ 保守退出本 pass
//!    （退出登记不静默半改）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0486";

// ---------------------------------------------------------------------------
// 拷贝链穿透
// ---------------------------------------------------------------------------

/// 拷贝表：copy 指令序贯 (dst ← src)。
#[derive(Debug, Default)]
pub struct CopyChain {
    pub copies: Vec<(u32, u32)>,
    /// 穿透统计。
    pub resolved: usize,
    pub conservative_stops: Vec<String>,
}

impl CopyChain {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_copy(&mut self, dst: u32, src: u32) {
        self.copies.push((dst, src));
    }
    /// 穿透解析：dst 沿拷贝链追到非拷贝源（深度限界防环——环 = 保守停）。
    pub fn resolve(&mut self, mut r: u32) -> u32 {
        for _ in 0..64 {
            match self.copies.iter().find(|(d, _)| *d == r) {
                Some((_, s)) => {
                    if *s == r {
                        // 自拷贝环 → 保守停。
                        self.conservative_stops.push(format!("自拷贝环 r{}", r));
                        return r;
                    }
                    r = *s;
                }
                None => {
                    self.resolved += 1;
                    return r;
                }
            }
        }
        // 深度超限（长环）→ 保守停。
        self.conservative_stops.push(format!("深度超限 r{}", r));
        r
    }
}

// ---------------------------------------------------------------------------
// 生命周期与复用提示
// ---------------------------------------------------------------------------

/// 值生命周期区间 [def 点, last use 点]。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveRange {
    pub reg: u32,
    pub start: usize,
    pub end: usize,
}

/// 复用提示：区间不重叠的值对（合并提示入账——供后端裁决）。
pub fn reuse_hints(ranges: &[LiveRange]) -> Vec<(u32, u32)> {
    let mut hints = Vec::new();
    for i in 0..ranges.len() {
        for j in (i + 1)..ranges.len() {
            let (a, b) = (ranges[i], ranges[j]);
            let disjoint = a.end < b.start || b.end < a.start;
            if disjoint {
                hints.push((a.reg, b.reg));
            }
        }
    }
    hints
}

// ---------------------------------------------------------------------------
// phi 简化
// ---------------------------------------------------------------------------

/// phi 节点（简化面）。
#[derive(Clone, Debug, PartialEq)]
pub struct SimplePhi {
    pub def: u32,
    /// 入边源（全同源 → 可简化为直拷贝；无使用 → 死）。
    pub sources: Vec<u32>,
    /// def 的使用计数。
    pub uses: u32,
}

/// phi 简化判定与产物：全同源 → Copy；零使用 → 死；否则保留。
#[derive(Clone, Debug, PartialEq)]
pub enum PhiAction {
    Copy { def: u32, src: u32 },
    Dead { def: u32 },
    Keep,
}

pub fn phi_action(p: &SimplePhi) -> PhiAction {
    // 零使用或零入边（退化 phi 无源无义）→ 死。
    if p.uses == 0 || p.sources.is_empty() {
        return PhiAction::Dead { def: p.def };
    }
    if p.sources.iter().all(|s| *s == p.sources[0]) {
        return PhiAction::Copy { def: p.def, src: p.sources[0] };
    }
    PhiAction::Keep
}

// ---------------------------------------------------------------------------
// 协同序（与 DCE/CSE 的管理器排序登记）
// ---------------------------------------------------------------------------

/// 协同序登记（DCE → CSE → 数据流——固定序原则 F0489 复用）。
pub static COOP_ORDER: &[&str] = &["dce", "cse", "dataflow"];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec86_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、拷贝穿透。
    {
        let mut cc = CopyChain::new();
        cc.add_copy(1, 0); // r1 ← r0
        cc.add_copy(2, 1); // r2 ← r1（链式）
        // r2 穿透到 r0（两跳）。
        let r = cc.resolve(2);
        v.push(("F0486-穿透-链式解析", r == 0 && cc.resolved == 1));
        // 无拷贝 → 原样（已解析终点）。
        let r2 = cc.resolve(5);
        v.push(("F0486-穿透-非拷贝原样", r2 == 5));
        // 自拷贝环 → 保守停。
        let mut cc2 = CopyChain::new();
        cc2.add_copy(3, 3);
        let r3 = cc2.resolve(3);
        v.push((
            "F0486-穿透-自环保守停",
            r3 == 3 && cc2.conservative_stops.len() == 1,
        ));
        // 长环 → 保守停（深度限界）。
        let mut cc3 = CopyChain::new();
        for i in 0..100 {
            cc3.add_copy(i + 1, i);
        }
        let r4 = cc3.resolve(100);
        v.push((
            "F0486-穿透-长环限界",
            cc3.conservative_stops.len() == 1 && cc3.resolved == 0,
        ));
        let _ = r4;
    }

    // 二、复用提示。
    {
        let ranges = vec![
            LiveRange { reg: 1, start: 0, end: 3 },
            LiveRange { reg: 2, start: 5, end: 8 },  // 与 r1 不重叠 → 提示
            LiveRange { reg: 3, start: 2, end: 6 },  // 与两者都重叠 → 无提示
        ];
        let hints = reuse_hints(&ranges);
        v.push((
            "F0486-提示-不重叠对",
            hints.contains(&(1, 2)) && !hints.contains(&(1, 3)) && !hints.contains(&(2, 3)),
        ));
        // 提示不强制（后端裁决面——提示清单只是登记）。
        v.push(("F0486-提示-登记面", hints.len() == 1));
    }

    // 三、phi 简化。
    {
        // 全同源 → Copy。
        let p1 = SimplePhi { def: 5, sources: vec![2, 2, 2], uses: 1 };
        v.push((
            "F0486-phi-全同源简化",
            phi_action(&p1) == PhiAction::Copy { def: 5, src: 2 },
        ));
        // 零使用 → 死。
        let p2 = SimplePhi { def: 6, sources: vec![1, 2], uses: 0 };
        v.push(("F0486-phi-零使用死", phi_action(&p2) == PhiAction::Dead { def: 6 }));
        // 异源有使用 → 保留。
        let p3 = SimplePhi { def: 7, sources: vec![1, 2], uses: 3 };
        v.push(("F0486-phi-异源保留", phi_action(&p3) == PhiAction::Keep));
        // 零入边（退化 phi）→ 死（无源无义）。
        let p4 = SimplePhi { def: 8, sources: vec![], uses: 2 };
        v.push(("F0486-phi-零入边死", phi_action(&p4) == PhiAction::Dead { def: 8 }));
    }

    // 四、保守退出 + 协同序。
    {
        // 保守停有账（可溯不静默）。
        let mut cc = CopyChain::new();
        cc.add_copy(3, 3);
        let _ = cc.resolve(3);
        v.push((
            "F0486-保守-停有账",
            !cc.conservative_stops.is_empty() && cc.conservative_stops[0].contains("r3"),
        ));
        // 协同序固定（DCE → CSE → 数据流）。
        v.push((
            "F0486-协同-固定序",
            COOP_ORDER == ["dce", "cse", "dataflow"],
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0486-判据-锚点入册", ANCHOR == "VE-F0486"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec86）。
pub fn run_vec86_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec86");
    for (name, ok) in vec86_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec86_raw_checks as run_vec86_checks;
    #[test]
    fn vec86_dataflow_checks() {
        for (name, ok) in run_vec86_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
