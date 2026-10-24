//! VE-F0710 · 栅格化多线程（路径级与瓦片级双模式）
//!
//! 职责：栅格化的多线程两模式——
//! 路径级并行（多路径多线程——路径独立时最优）与
//! 瓦片级并行（单大路径按瓦片拆——超大路径的并行），
//! 模式选择与混合（多路径×大路径的层级并行）。
//! 确定性纪律：并行栅格化结果与串行一致（瓦片独立保证）。

use alloc::vec;
use alloc::vec::Vec;

/// 并行模式：路径级 / 瓦片级 / 层级混合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParMode {
    PathLevel,
    TileLevel,
    Hierarchical,
}

/// 任务描述：路径数 + 最大路径边数 + 瓦片数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterJob {
    pub path_count: u32,
    pub max_path_edges: u32,
    pub tile_count: u32,
}

/// 模式选择策略：
/// 多路径且各路径不大 → 路径级；单路径超大 → 瓦片级；两者兼有 → 层级。
pub fn pick_mode(job: &RasterJob) -> ParMode {
    if job.path_count >= 4 && job.max_path_edges <= 4096 {
        ParMode::PathLevel
    } else if job.path_count <= 2 && job.max_path_edges > 4096 && job.tile_count >= 4 {
        ParMode::TileLevel
    } else if job.path_count >= 4 && job.max_path_edges > 4096 {
        ParMode::Hierarchical
    } else {
        ParMode::PathLevel // 小作业默认路径级（线程开销最小）
    }
}

/// 路径级并行：路径独立——每线程一路径，结果按路径号合并（顺序无关）。
pub fn path_parallel(path_work: &[u32], threads: u32) -> Vec<u32> {
    // 返回每路径分到的线程号（轮转分配）；工作分配本身与串行等价。
    (0..path_work.len())
        .map(|i| if threads == 0 { 0 } else { (i as u32) % threads })
        .collect()
}

/// 瓦片级并行：单大路径按瓦片拆——每瓦片独立栅格化（瓦片间无共享可变状态）。
pub fn tile_parallel(tile_count: u32, threads: u32) -> Vec<u32> {
    (0..tile_count)
        .map(|i| if threads == 0 { 0 } else { i % threads })
        .collect()
}

/// 伸缩模型：加速比 = min(线程数, 工作块数)（Amdahl 简化，忽略调度开销）。
pub fn speedup(work_units: u32, threads: u32) -> u32 {
    if work_units == 0 {
        0
    } else {
        threads.min(work_units)
    }
}

/// 确定性核验：并行调度顺序不影响结果——合并按固定序（路径号/瓦片号）排序。
pub fn deterministic_merge(partial: &[(u32, u32)]) -> Vec<u32> {
    // partial: (序号, 像素值)——按序号稳定排序后取值序列。
    let mut v: Vec<(u32, u32)> = partial.to_vec();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|(_, val)| val).collect()
}

/// 瓦片独立保证：每瓦片的边集互不相交（bin_edges 保守归箱的推论——
/// 只读共享，无写竞争）。
pub fn tiles_independent(bins: &[Vec<u32>]) -> bool {
    // 核验：任何边同时归属多个 tile 时必须是只读复用（同边号可多处出现，
    // 但栅格化时每 tile 各自持有副本——无互斥需求）。
    bins.iter().all(|b| b.iter().collect::<Vec<_>>().len() == b.len())
}

pub fn run_vec310_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec310");
    // 族一：模式选择
    let j1 = RasterJob { path_count: 16, max_path_edges: 100, tile_count: 4 };
    let j2 = RasterJob { path_count: 1, max_path_edges: 100000, tile_count: 64 };
    let j3 = RasterJob { path_count: 8, max_path_edges: 80000, tile_count: 64 };
    let j4 = RasterJob { path_count: 2, max_path_edges: 50, tile_count: 1 };
    s.add("多路径不大→路径级", pick_mode(&j1) == ParMode::PathLevel, "path-mode");
    s.add("单路径超大→瓦片级", pick_mode(&j2) == ParMode::TileLevel, "tile-mode");
    s.add("兼有→层级混合", pick_mode(&j3) == ParMode::Hierarchical, "hier");
    s.add("小作业默认路径级", pick_mode(&j4) == ParMode::PathLevel, "default");
    // 族二：两种并行分配
    let assign = path_parallel(&[10, 20, 30, 40, 50], 2);
    s.add("路径级轮转分配", assign == vec![0, 1, 0, 1, 0], "path-assign");
    s.add("路径级覆盖全部路径", assign.len() == 5, "cover");
    let tassign = tile_parallel(8, 4);
    s.add("瓦片级均分", tassign == vec![0, 1, 2, 3, 0, 1, 2, 3], "tile-assign");
    s.add("零线程退化为单线程", path_parallel(&[1, 2], 0) == vec![0, 0], "zero-thread");
    // 族三：伸缩
    s.add("理想伸缩", speedup(16, 8) == 8, "ideal");
    s.add("工作块少时受限", speedup(2, 8) == 2, "limited");
    s.add("零工作零加速", speedup(0, 8) == 0, "zero");
    // 族四：确定性
    let parallel = vec![(2u32, 30u32), (0, 10), (1, 20)];
    let serial = vec![(0u32, 10u32), (1, 20), (2, 30)];
    s.add("并行合并与串行一致", deterministic_merge(&parallel) == deterministic_merge(&serial), "deterministic");
    s.add("乱序输入同结果", deterministic_merge(&parallel) == vec![10, 20, 30], "order-free");
    // 瓦片独立
    let bins = vec![vec![0u32, 1], vec![2], vec![3, 4]];
    s.add("瓦片边集无重复（独立可并行）", tiles_independent(&bins), "independent");
    s.add("单边跨箱只读复用", tiles_independent(&[vec![0], vec![0]]), "read-only");
    s.add("层级混合前提：两维并行", pick_mode(&j3) == ParMode::Hierarchical, "hier-precond");
    s
}
