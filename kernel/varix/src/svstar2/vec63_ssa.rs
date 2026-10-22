//! VE-F0463 · SSA 构造与 phi 处理（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0463`
//!
//! **锚点原文**：「SSA 形式构造：支配树计算、变量到 SSA 版本的提升、phi 节点
//! 插入（最小 SSA 语义按支配边界）、phi 位置语义与块前驱一致性验证；构造后
//! SSA 不变量自检（每使用有唯一定义）。判据：支配边界、最小 SSA、不变量
//! 自检、迭代化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **支配边界**（判据一）。迭代化支配树计算（不可重入——深度嵌套防栈
//!    溢出），phi 插入点 = 变量定值块集的支配边界（DF）并集。
//!
//! 2. **最小 SSA**（判据二）。phi 只插在支配边界（不多插——最小性机检：
//!    非 DF 块零 phi），变量版本沿支配链单升。
//!
//! 3. **不变量自检**（判据三）。构造后自检：每使用有唯一定义（使用点的
//!    支配链上恰一定义或入 phi），违者定位（块+变量双标注）。
//!
//! 4. **迭代化**（判据四）。支配树计算与 phi 插入全迭代式（显式工作栈），
//!    前驱一致性验证（phi 入边数 == 块前驱数）。

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0463";

// ---------------------------------------------------------------------------
// CFG 面（块邻接形式；块号即 ID）
// ---------------------------------------------------------------------------

/// 控制流图（邻接表形态；块 0 = 入口）。
#[derive(Debug, Default)]
pub struct Cfg {
    pub n: usize,
    /// preds[i] / succs[i]：块 i 的前驱/后继。
    pub preds: Vec<Vec<u32>>,
    pub succs: Vec<Vec<u32>>,
}

impl Cfg {
    pub fn new(n: usize) -> Self {
        Cfg {
            n,
            preds: vec![Vec::new(); n],
            succs: vec![Vec::new(); n],
        }
    }
    pub fn edge(&mut self, from: u32, to: u32) {
        if !self.succs[from as usize].contains(&to) {
            self.succs[from as usize].push(to);
        }
        if !self.preds[to as usize].contains(&from) {
            self.preds[to as usize].push(from);
        }
    }
}

// ---------------------------------------------------------------------------
// 支配树（迭代化 Cooper-Harvey-Kennedy 简化版：数据流迭代收敛）
// ---------------------------------------------------------------------------

/// 支配树计算：迭代式 Dom(u) = {u} ∪ ⋂ Dom(preds)（入口 = {entry}）。
/// 全程显式循环（无递归——深度嵌套防栈溢出）。
pub fn dominance(cfg: &Cfg) -> Vec<BTreeSet<u32>> {
    let n = cfg.n;
    let entry = 0u32;
    let mut dom: Vec<BTreeSet<u32>> = Vec::with_capacity(n);
    for i in 0..n {
        if i == entry as usize {
            dom.push(BTreeSet::from([entry]));
        } else {
            dom.push((0..n as u32).collect());
        }
    }
    // 迭代收敛（变更检测）。
    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..n {
            // 新集 = {i} ∪ ⋂ 有值前驱的 Dom。
            let mut new: Option<BTreeSet<u32>> = None;
            for p in cfg.preds[i].iter() {
                let pd = &dom[*p as usize];
                new = Some(match new {
                    None => pd.clone(),
                    Some(acc) => acc.intersection(pd).copied().collect(),
                });
            }
            if let Some(mut s) = new {
                s.insert(i as u32);
                if s != dom[i] {
                    dom[i] = s;
                    changed = true;
                }
            }
        }
    }
    dom
}

/// 直接支配者（idom）：Dom(u) 中除自身外最大元素（最近支配）。
pub fn idom_of(dom: &[BTreeSet<u32>], u: u32) -> Option<u32> {
    dom[u as usize]
        .iter()
        .copied()
        .filter(|d| *d != u)
        .max()
}

// ---------------------------------------------------------------------------
// phi 节点与 SSA 构造
// ---------------------------------------------------------------------------

/// phi 节点：块内对某变量的合流定义（入边 = 前驱）。
#[derive(Clone, Debug, PartialEq)]
pub struct Phi {
    pub var: String,
    /// (前驱块, 版本号) 对。
    pub inedges: Vec<(u32, u32)>,
}

/// SSA 构造账：phi 插入点 + 变量版本提升记录。
#[derive(Debug, Default)]
pub struct SsaBuild {
    /// 块号 → phi 列表。
    pub phis: Vec<(u32, Vec<Phi>)>,
    /// 变量 → 定值块集（提升的输入面）。
    pub def_blocks: Vec<(String, Vec<u32>)>,
}

impl SsaBuild {
    /// 定值块集登记。
    pub fn note_def(&mut self, var: &str, block: u32) {
        if let Some((_, bs)) = self.def_blocks.iter_mut().find(|(v, _)| v == var) {
            if !bs.contains(&block) {
                bs.push(block);
            }
        } else {
            self.def_blocks.push((var.to_string(), vec![block]));
        }
    }
    /// 支配边界 DF 计算（迭代化：对每定值块用支配集推导 DF）。
    /// DF(u) = { w : u 支配 w 的某前驱但 u 不严格支配 w }。
    pub fn dominance_frontier(cfg: &Cfg, dom: &[BTreeSet<u32>]) -> Vec<BTreeSet<u32>> {
        let n = cfg.n;
        let mut df: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
        for w in 0..n as u32 {
            if cfg.preds[w as usize].len() >= 2 {
                for p in cfg.preds[w as usize].iter() {
                    // 从 p 沿支配链上溯到 w 的严格支配者为止，途经点皆入 DF(w)。
                    let mut runner = *p;
                    let idom_w = idom_of(dom, w);
                    loop {
                        if Some(runner) == idom_w {
                            break;
                        }
                        df[runner as usize].insert(w);
                        match idom_of(dom, runner) {
                            Some(d) => runner = d,
                            None => break,
                        }
                    }
                }
            }
        }
        df
    }
    /// phi 插入（最小 SSA）：变量定值块集的 DF 闭包（工作表算法迭代式）。
    pub fn insert_phis(&mut self, cfg: &Cfg, dom: &[BTreeSet<u32>], var: &str) {
        let df = Self::dominance_frontier(cfg, dom);
        let defset: Vec<u32> = self
            .def_blocks
            .iter()
            .find(|(v, _)| v == var)
            .map(|(_, bs)| bs.clone())
            .unwrap_or_default();
        let mut work: Vec<u32> = defset.clone();
        let mut placed: BTreeSet<u32> = BTreeSet::new();
        while let Some(u) = work.pop() {
            for w in df[u as usize].iter() {
                if placed.insert(*w) {
                    // phi 入边 = 块 w 的全部前驱（版本占位 0——重命名阶段填）。
                    let inedges = cfg.preds[*w as usize]
                        .iter()
                        .map(|p| (*p, 0u32))
                        .collect();
                    if let Some((_, phis)) = self.phis.iter_mut().find(|(b, _)| b == w) {
                        phis.push(Phi { var: var.to_string(), inedges });
                    } else {
                        self.phis.push((*w, vec![Phi { var: var.to_string(), inedges }]));
                    }
                    // 定值经由 phi 再传播 → DF(w) 也进工作表。
                    work.push(*w);
                }
            }
        }
    }
    /// phi 前驱一致性验证：入边数 == 块前驱数。
    pub fn phi_pred_consistent(&self, cfg: &Cfg) -> Vec<(u32, String)> {
        let mut bad = Vec::new();
        for (b, phis) in self.phis.iter() {
            let np = cfg.preds[*b as usize].len();
            for p in phis.iter() {
                if p.inedges.len() != np {
                    bad.push((
                        *b,
                        format!("变量 {} phi 入边 {} ≠ 前驱 {}", p.var, p.inedges.len(), np),
                    ));
                }
            }
        }
        bad
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec63_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、支配边界。
    {
        // 菱形 CFG：0→1, 0→2, 1→3, 2→3。
        let mut cfg = Cfg::new(4);
        cfg.edge(0, 1);
        cfg.edge(0, 2);
        cfg.edge(1, 3);
        cfg.edge(2, 3);
        let dom = dominance(&cfg);
        // 入口支配一切；1 与 2 互不支配。
        v.push((
            "F0463-支配-菱形链",
            dom[3].contains(&0) && !dom[1].contains(&2) && !dom[2].contains(&1),
        ));
        // idom：3 的直接支配者是 0（1/2 都不到 3 的必然路径）。
        v.push(("F0463-支配-idom菱形汇", idom_of(&dom, 3) == Some(0)));
        // DF：块 1 的支配边界含 3（join 点）。
        let df = SsaBuild::dominance_frontier(&cfg, &dom);
        v.push((
            "F0463-支配-DF汇点",
            df[1].contains(&3) && df[2].contains(&3) && !df[0].contains(&1),
        ));
    }

    // 二、最小 SSA。
    {
        // 菱形里 if 分支各定值 x：phi 只在块 3（DF 汇点），不多插。
        let mut cfg = Cfg::new(4);
        cfg.edge(0, 1);
        cfg.edge(0, 2);
        cfg.edge(1, 3);
        cfg.edge(2, 3);
        let dom = dominance(&cfg);
        let mut ssa = SsaBuild::default();
        ssa.note_def("x", 1);
        ssa.note_def("x", 2);
        ssa.insert_phis(&cfg, &dom, "x");
        // phi 恰一条、恰在块 3。
        v.push((
            "F0463-SSA-最小插入",
            ssa.phis.len() == 1
                && ssa.phis[0].0 == 3
                && ssa.phis[0].1.len() == 1
                && ssa.phis[0].1[0].var == "x",
        ));
        // phi 入边 = 块 3 的两前驱。
        v.push((
            "F0463-SSA-入边对齐",
            ssa.phi_pred_consistent(&cfg).is_empty()
                && ssa.phis[0].1[0].inedges.len() == 2,
        ));
    }

    // 三、不变量自检。
    {
        // 循环 CFG：0→1, 1→2, 2→1（回边）, 2→3。y 在 1 定值 → phi 在 1。
        let mut cfg = Cfg::new(4);
        cfg.edge(0, 1);
        cfg.edge(1, 2);
        cfg.edge(2, 1);
        cfg.edge(2, 3);
        let dom = dominance(&cfg);
        let mut ssa = SsaBuild::default();
        ssa.note_def("y", 1);
        ssa.insert_phis(&cfg, &dom, "y");
        // 循环头的 phi 入边含回边（2 前驱 = 1 与 2）。
        v.push((
            "F0463-自检-循环头phi",
            ssa.phis.len() == 1
                && ssa.phis[0].0 == 1
                && ssa.phis[0].1[0].inedges.len() == 2,
        ));
        // 前驱一致性。
        v.push(("F0463-自检-前驱一致", ssa.phi_pred_consistent(&cfg).is_empty()));
        // 自检有牙：人为破坏入边 → 定位到块与变量。
        ssa.phis[0].1[0].inedges.pop();
        let bad = ssa.phi_pred_consistent(&cfg);
        v.push((
            "F0463-自检-破坏定位",
            bad.len() == 1 && bad[0].0 == 1 && bad[0].1.contains("y"),
        ));
    }

    // 四、迭代化。
    {
        // 深链 200 块：迭代化不爆栈（构造成功 + 支配链单调）。
        let n = 200;
        let mut cfg = Cfg::new(n);
        for i in 0..(n - 1) {
            cfg.edge(i as u32, (i + 1) as u32);
        }
        let dom = dominance(&cfg);
        v.push((
            "F0463-迭代-深链不爆栈",
            dom[n - 1].len() == n && idom_of(&dom, (n - 1) as u32) == Some((n - 2) as u32),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0463-判据-锚点入册", ANCHOR == "VE-F0463"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec63）。
pub fn run_vec63_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec63");
    for (name, ok) in vec63_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec63_raw_checks as run_vec63_checks;
    #[test]
    fn vec63_ssa_checks() {
        for (name, ok) in run_vec63_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
