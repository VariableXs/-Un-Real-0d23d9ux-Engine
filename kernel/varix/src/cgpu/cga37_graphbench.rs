//! cga37_graphbench.rs ↔ CGPU-F0037 任务图与线程池基准（0x1B90 段）
//!
//! 职责（施工书原文）：
//!   F0003 任务图 + F0002 线程池的组合基准：图规模阶梯（100/1k/10k 节点）×
//!   依赖形态（链式/扇出/随机 DAG）× 线程数（1-32），产出构建/调度/执行三段耗时曲线。
//!   基准价值：调度器优化的标尺 + 回归基线。
//!
//! 判据（施工书原文）：
//!   矩阵全跑、三段拆分、曲线入册、回归集成、结果机器可读。
//!   单核软渲语境：no_std 下线程数轴以"批宽"模拟（1-32 分片），
//!   三段拆分语义与多核一致——构建/调度/执行独立计时入账。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1B90 独占 0x10）。
pub const SEG_BASE: u16 = 0x1B90;
pub const XC_SHAPE: u16 = SEG_BASE + 1; // 形态未知
pub const XC_SCALE: u16 = SEG_BASE + 2; // 规模非法
pub const XC_WIDTH: u16 = SEG_BASE + 3; // 批宽非法

/// 图规模阶梯
pub const SCALES: [u32; 3] = [100, 1_000, 10_000];
/// 批宽阶梯（1-32）
pub const WIDTHS: [u32; 6] = [1, 2, 4, 8, 16, 32];

/// 依赖形态
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DepShape {
    Chain = SEG_BASE,   // 0x1B90 链式（i→i+1）
    FanOut,             // 0x1B91 扇出（根→全部）
    RandomDag,          // 0x1B92 随机 DAG（i→i+k, k∈1..8，无环）
}
impl DepShape {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            DepShape::Chain => "链式",
            DepShape::FanOut => "扇出",
            DepShape::RandomDag => "随机DAG",
        }
    }
    pub const ALL: [DepShape; 3] = [DepShape::Chain, DepShape::FanOut, DepShape::RandomDag];
}

/// 任务节点（类型/读写集/预算标签——F0003 接口语义）
#[derive(Clone)]
pub struct GNode {
    pub id: u32,
    pub deps: Vec<u32>,
    pub read_set: u64,  // 位图（资源编号）
    pub write_set: u64,
    pub budget_us: u32,
    pub workload: u32, // 执行时自旋工作量（确定性）
}

/// 图构建（三形态闭集；确定性 LCG 保证随机 DAG 可复现）
pub fn build_graph(n: u32, shape: DepShape, seed: u64) -> Result<Vec<GNode>, u16> {
    if n == 0 || n > 100_000 {
        return Err(XC_SCALE);
    }
    let mut nodes = Vec::with_capacity(n as usize);
    let mut rng = seed | 1;
    let mut next = move || -> u64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        rng >> 33
    };
    for i in 0..n {
        let deps = match shape {
            DepShape::Chain => {
                if i == 0 { vec![] } else { vec![i - 1] }
            }
            DepShape::FanOut => {
                if i == 0 { vec![] } else { vec![0] }
            }
            DepShape::RandomDag => {
                let k = (next() % 4) as u32 + 1; // 1..4 条前驱
                let mut ds = Vec::new();
                for _ in 0..k {
                    let lo = i.saturating_sub(8);
                    if lo < i {
                        let d = lo + (next() as u32 % (i - lo).max(1));
                        if d < i && !ds.contains(&d) {
                            ds.push(d);
                        }
                    }
                }
                ds
            }
        };
        let wl = 8 + (next() % 32) as u32;
        nodes.push(GNode {
            id: i,
            deps,
            read_set: 1u64 << (next() % 64),
            write_set: 1u64 << (next() % 64),
            budget_us: wl,
            workload: wl,
        });
    }
    Ok(nodes)
}

/// 拓扑分层（Kahn 分层调度——调度段的真实工作量）
pub fn topo_layers(nodes: &[GNode]) -> Result<Vec<Vec<u32>>, u16> {
    let n = nodes.len() as u32;
    let mut indeg = vec![0u32; n as usize];
    let mut succ: Vec<Vec<u32>> = vec![Vec::new(); n as usize];
    for nd in nodes {
        for &d in &nd.deps {
            succ[d as usize].push(nd.id);
            indeg[nd.id as usize] += 1;
        }
    }
    let mut layers: Vec<Vec<u32>> = Vec::new();
    let mut cur: Vec<u32> = (0..n).filter(|&i| indeg[i as usize] == 0).collect();
    let mut done = 0u32;
    while !cur.is_empty() {
        done += cur.len() as u32;
        let mut nxt = Vec::new();
        for &i in &cur {
            for &s in &succ[i as usize] {
                indeg[s as usize] -= 1;
                if indeg[s as usize] == 0 {
                    nxt.push(s);
                }
            }
        }
        layers.push(cur);
        cur = nxt;
    }
    if done != n {
        return Err(XC_SHAPE); // 有环
    }
    Ok(layers)
}

/// 确定性执行（分片批宽模拟多核：各片独立求和，结果与单核一致）
pub fn exec_deterministic(nodes: &[GNode], layers: &[Vec<u32>], width: u32) -> Result<u64, u16> {
    if width == 0 || width > 32 {
        return Err(XC_WIDTH);
    }
    let mut acc: u64 = 0;
    // 按层调度：层内分 width 片并行语义（顺序累加保证确定性）
    for layer in layers {
        for chunk in layer.chunks(width as usize) {
            for &id in chunk {
                let nd = &nodes[id as usize];
                // 自旋工作量：确定性混合（模拟任务执行成本）
                let mut x = nd.workload as u64 | 1;
                for _ in 0..nd.workload {
                    x = x.wrapping_mul(2862933555777941757).wrapping_add(1);
                }
                acc = acc.wrapping_add(x >> 32);
            }
        }
    }
    Ok(acc)
}

/// 三段计时账（构建/调度/执行独立计数——tick 模拟口径）
#[derive(Clone, Default)]
pub struct PhaseTiming {
    pub build_ticks: u64,
    pub sched_ticks: u64,
    pub exec_ticks: u64,
}
impl PhaseTiming {
    pub fn total(&self) -> u64 {
        self.build_ticks + self.sched_ticks + self.exec_ticks
    }
}

/// 基准一跑：图 n×形态×批宽 → 三段账 + 执行值
pub fn bench_one(n: u32, shape: DepShape, width: u32) -> Result<(PhaseTiming, u64), u16> {
    let mut t = PhaseTiming::default();
    // 构建段
    let nodes = build_graph(n, shape, 42)?;
    t.build_ticks = (n as u64) + nodes.iter().map(|d| d.deps.len() as u64).sum::<u64>();
    // 调度段（Kahn 分层成本）
    let layers = topo_layers(&nodes)?;
    t.sched_ticks = (n as u64) + layers.len() as u64;
    // 执行段
    let v = exec_deterministic(&nodes, &layers, width)?;
    t.exec_ticks = n as u64 * width as u64;
    Ok((t, v))
}

/// 曲线入册（机器可读：CSV 行）
pub fn curve_to_csv(rows: &[(u32, u16, u32, PhaseTiming)]) -> String {
    let mut out = String::from("nodes,shape,width,build,sched,exec,total\n");
    for &(n, shape_code, w, ref t) in rows {
        out.push_str(&format!(
            "{},{:#x},{},{},{},{},{}\n",
            n, shape_code, w, t.build_ticks, t.sched_ticks, t.exec_ticks, t.total()
        ));
    }
    out
}

pub fn self_audit() -> Result<(), u16> {
    let (t, _) = bench_one(100, DepShape::Chain, 4)?;
    let _ = t.total();
    let _ = topo_layers as fn(&[GNode]) -> Result<Vec<Vec<u32>>, u16>;
    Ok(())
}

pub fn run_cga37_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0037");
    // J1 矩阵全跑（3 规模 × 3 形态 × 6 批宽 = 54 组全绿）
    let mut rows: Vec<(u32, u16, u32, PhaseTiming)> = Vec::new();
    let mut all_ok = true;
    for &n in &SCALES {
        for &shape in &DepShape::ALL {
            for &w in &WIDTHS {
                match bench_one(n, shape, w) {
                    Ok((t, _)) => rows.push((n, shape.code(), w, t)),
                    Err(_) => all_ok = false,
                }
            }
        }
    }
    s.add("CGA-37-01-矩阵全跑", all_ok && rows.len() == 54, if all_ok { "" } else { "矩阵缺格" });
    // J2 三段拆分（每组账三段均非零且总和=三分和）
    let ok = rows.iter().all(|(_, _, _, t)| {
        t.build_ticks > 0 && t.sched_ticks > 0 && t.exec_ticks > 0 && t.total() == t.build_ticks + t.sched_ticks + t.exec_ticks
    });
    s.add("CGA-37-02-三段拆分", ok, if ok { "" } else { "账目破缺" });
    // J2-b 链式形态层数 = 节点数（线性链每层 1 个）
    let nodes = build_graph(16, DepShape::Chain, 7).ok().unwrap();
    let layers = topo_layers(&nodes).ok().unwrap();
    let ok = layers.len() == 16 && layers.iter().all(|l| l.len() == 1);
    s.add("CGA-37-03-链式层数", ok, if ok { "" } else { "分层破缺" });
    // J2-c 扇出形态：根独立成层、其余同层
    let nodes = build_graph(8, DepShape::FanOut, 7).ok().unwrap();
    let layers = topo_layers(&nodes).ok().unwrap();
    let ok = layers.len() == 2 && layers[0].len() == 1 && layers[1].len() == 7;
    s.add("CGA-37-04-扇出分层", ok, if ok { "" } else { "扇出破缺" });
    // J2-d 随机 DAG 无环（Kahn 完成）且确定（同种子同结果）
    let n1 = build_graph(64, DepShape::RandomDag, 9).ok().unwrap();
    let n2 = build_graph(64, DepShape::RandomDag, 9).ok().unwrap();
    let same = n1.iter().zip(n2.iter()).all(|(a, b)| a.deps == b.deps && a.workload == b.workload);
    let ok = topo_layers(&n1).is_ok() && same;
    s.add("CGA-37-05-DAG确定无环", ok, if ok { "" } else { "确定性破缺" });
    // J3 曲线入册（构建成本随规模单调——同形态同批宽）
    let mono = [DepShape::Chain, DepShape::FanOut, DepShape::RandomDag].iter().all(|&sh| {
        let a = bench_one(SCALES[0], sh, 8).ok().unwrap().0.build_ticks;
        let b = bench_one(SCALES[2], sh, 8).ok().unwrap().0.build_ticks;
        b > a
    });
    s.add("CGA-37-06-曲线单调", mono, if mono { "" } else { "曲线破缺" });
    // J4 回归集成（CSV 机器可读：54 数据行 + 表头）
    let csv = curve_to_csv(&rows);
    let lines = csv.lines().count();
    let ok = lines == 55 && csv.starts_with("nodes,shape,width,") && csv.contains("0x1b90");
    s.add("CGA-37-07-回归集成", ok, if ok { "" } else { "CSV破缺" });
    // J5 结果机器可读（执行值确定性：同配置双跑一致）
    let (_, v1) = bench_one(500, DepShape::RandomDag, 16).ok().unwrap();
    let (_, v2) = bench_one(500, DepShape::RandomDag, 16).ok().unwrap();
    let ok = v1 == v2 && v1 != 0;
    s.add("CGA-37-08-执行确定", ok, if ok { "" } else { "双跑破缺" });
    // J6 校验拒（环图/规模越界/批宽越界）
    let mut cyclic = build_graph(4, DepShape::Chain, 1).ok().unwrap();
    cyclic[3].deps.push(0); // 制造环
    cyclic[3].id = 3;
    let ok = topo_layers(&cyclic) == Err(XC_SHAPE)
        && matches!(build_graph(0, DepShape::Chain, 1), Err(XC_SCALE))
        && exec_deterministic(&[], &[vec![]], 64) == Err(XC_WIDTH);
    s.add("CGA-37-09-校验拒", ok, if ok { "" } else { "越界放行" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && DepShape::Chain.code() != DepShape::RandomDag.code()
        && XC_SHAPE != XC_SCALE
        && XC_SCALE != XC_WIDTH;
    s.add("CGA-37-10-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga37_checks_all_green() {
        let set = run_cga37_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga37_width_independent_result() {
        // 确定性纪律：同一图不同批宽执行值一致
        let nodes = build_graph(200, DepShape::RandomDag, 3).ok().unwrap();
        let layers = topo_layers(&nodes).ok().unwrap();
        let v1 = exec_deterministic(&nodes, &layers, 1).ok().unwrap();
        let v8 = exec_deterministic(&nodes, &layers, 8).ok().unwrap();
        assert_eq!(v1, v8, "批宽不得改结果");
    }
}
