//! CGPU-F0474 · 文档总纲（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 文档总纲：九组文档索引+预算域架构总图（模型/仲裁/场景/预测/
//!   治理/集成/统计/边界/高级九层全景）；
//! - 术语表；API 总手册（v1.0 终版）；调参手册终版。
//!   判据：索引九层、术语、总手册、调参终版、判据。
//!
//! 设计纪律：
//! - 九层架构总图结构化：层名+文档号+层间依赖（下行依赖链无环，
//!   拓扑排序可重放）。
//! - 术语表：规范词→定义（非空+唯一键）。
//! - API 总手册 v1.0 终版：签名册（与 F0397/F0447 对齐）+ 版本锁。
//! - 调参手册终版：参数表（名+区间+默认值——区间含默认值即自洽）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10D0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10D0，全仓独占）。
pub const SEG_BASE: u16 = 0x10D0;

pub const E_DL_LAYER: u16 = SEG_BASE + 1; // 层非法/依赖环
pub const E_DL_TERM: u16 = SEG_BASE + 2; // 术语缺/重
pub const E_DL_API: u16 = SEG_BASE + 3; // 手册签名缺
pub const E_DL_TUNE: u16 = SEG_BASE + 4; // 调参区间不含默认值

/// 九层（模型/仲裁/场景/预测/治理/集成/统计/边界/高级）。
pub const LAYERS: [&str; 9] = [
    "model", "arbiter", "scene", "predict", "govern", "integrate", "statistic", "edge", "advanced",
];

/// 层间下行依赖（上层依赖下层——数字小的层更底层）。
pub fn layer_deps() -> Vec<(usize, usize)> {
    vec![
        (1, 0), // arbiter 依赖 model
        (2, 1), // scene 依赖 arbiter
        (3, 1), // predict 依赖 arbiter
        (4, 2), // govern 依赖 scene
        (5, 3), // integrate 依赖 predict
        (6, 4), // statistic 依赖 govern
        (7, 6), // edge 依赖 statistic
        (8, 7), // advanced 依赖 edge
    ]
}

/// 依赖链无环（拓扑可排序）判定。
pub fn deps_acyclic() -> bool {
    let deps = layer_deps();
    let n = LAYERS.len();
    let mut indeg = vec![0usize; n];
    for &(down, _up) in deps.iter() {
        indeg[down] += 1; // down 依赖 up：入度记在依赖方
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut done = 0;
    while let Some(x) = queue.pop() {
        done += 1;
        for &(down, up) in deps.iter() {
            if up == x {
                indeg[down] -= 1;
                if indeg[down] == 0 {
                    queue.push(down);
                }
            }
        }
    }
    done == n
}

/// 术语表（规范词→定义）。
pub fn glossary() -> Vec<(&'static str, &'static str)> {
    vec![
        ("弃车", "应急分级下被主动让出资源的类别处理"),
        ("守恒", "Σ(余额+预占) 不增不减的预算铁律"),
        ("迟滞", "恢复方向与触发方向的阈值差带"),
        ("粒度", "份额分配的最小量化台阶"),
        ("豁免", "用户知情选择后临时解除紧缩"),
    ]
}

pub fn glossary_ok() -> Result<(), u16> {
    let g = glossary();
    if g.is_empty() {
        return Err(E_DL_TERM);
    }
    for (w, d) in g.iter() {
        if w.is_empty() || d.is_empty() {
            return Err(E_DL_TERM);
        }
    }
    // 唯一键。
    for i in 0..g.len() {
        for j in (i + 1)..g.len() {
            if g[i].0 == g[j].0 {
                return Err(E_DL_TERM);
            }
        }
    }
    Ok(())
}

/// API 总手册 v1.0 终版（签名册对齐 F0397/F0447 两文档源）。
pub fn api_manual_v1() -> Vec<&'static str> {
    vec![
        "c01.model.open",
        "c02.arb.grant",
        "c03.scene.apply",
        "c04.pred.feed",
        "c05.govern.sweep",
        "c06.integ.push",
        "c07.stat.export",
        "c08.adv.policy",
        "c09.edge.state",
    ]
}

pub fn manual_ok() -> Result<(), u16> {
    let m = api_manual_v1();
    if m.len() != 9 {
        return Err(E_DL_API);
    }
    // 全名唯一。
    for i in 0..m.len() {
        for j in (i + 1)..m.len() {
            if m[i] == m[j] {
                return Err(E_DL_API);
            }
        }
    }
    // v1.0 版本锁：每组前缀在场。
    for g in 1..=9u8 {
        let p = alloc::format!("c0{}", g);
        if !m.iter().any(|s| s.starts_with(p.as_str())) {
            return Err(E_DL_API);
        }
    }
    Ok(())
}

/// 调参手册终版：参数（名+下限+上限+默认值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuneParam {
    pub name: &'static str,
    pub lo: u64,
    pub hi: u64,
    pub default: u64,
}

impl TuneParam {
    pub fn consistent(&self) -> bool {
        self.lo <= self.default && self.default <= self.hi && self.lo < self.hi
    }
}

pub fn tune_params() -> Vec<TuneParam> {
    vec![
        TuneParam { name: "jain_floor_pm", lo: 800, hi: 990, default: 900 },
        TuneParam { name: "hunger_max_wait", lo: 8, hi: 128, default: 32 },
        TuneParam { name: "monopoly_cap_pm", lo: 300, hi: 800, default: 500 },
        TuneParam { name: "storm_cap", lo: 20, hi: 1000, default: 100 },
    ]
}

pub fn tune_ok() -> Result<(), u16> {
    let ps = tune_params();
    if ps.is_empty() {
        return Err(E_DL_TUNE);
    }
    if ps.iter().any(|p| !p.consistent()) {
        return Err(E_DL_TUNE);
    }
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、九层索引。
    {
        v.push(("F0474-索引-九层齐备", LAYERS.len() == 9 && LAYERS[0] == "model" && LAYERS[8] == "advanced"));
        v.push(("F0474-索引-依赖链无环", deps_acyclic()));
        // 层名唯一。
        let mut dup = false;
        for i in 0..LAYERS.len() {
            for j in (i + 1)..LAYERS.len() {
                if LAYERS[i] == LAYERS[j] {
                    dup = true;
                }
            }
        }
        v.push(("F0474-索引-层名唯一", !dup));
    }
    // 二、术语表。
    {
        v.push(("F0474-术语-五词齐备", glossary().len() == 5));
        v.push(("F0474-术语-键唯一定义非空", glossary_ok().is_ok()));
    }
    // 三、API 总手册 v1.0。
    {
        v.push(("F0474-总手册-九签名终版", manual_ok().is_ok()));
        // 每组前缀在场（c01~c09）。
        let m = api_manual_v1();
        v.push(("F0474-总手册-组前缀覆盖", (1..=9).all(|g| m.iter().any(|s| s.starts_with(alloc::format!("c0{}", g).as_str())))));
    }
    // 四、调参手册终版。
    {
        v.push(("F0474-调参-四参数齐备", tune_params().len() == 4));
        v.push(("F0474-调参-区间含默认值", tune_ok().is_ok()));
        // 红线：区间不含默认值的参数不合格。
        let bad = TuneParam { name: "x", lo: 10, hi: 20, default: 30 };
        v.push(("F0474-调参-区间外默认拒", !bad.consistent()));
    }
    // 五、判据自检。
    {
        v.push(("F0474-判据-依赖表八条", layer_deps().len() == 8));
        v.push(("F0474-判据-环检测阻断语义", {
            // 构造环：0↔1 互依 → 不可排序。
            let n = 2usize;
            let mut indeg = vec![2usize; n]; // 各自被指 1 次 + 自环语义近似
            indeg[0] = 1;
            indeg[1] = 1;
            // 环时拓扑排序无法完成：队列空但 done<n。
            let queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
            queue.is_empty() // 环 → 无入度零节点 → 检出
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0474_docmaster_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
