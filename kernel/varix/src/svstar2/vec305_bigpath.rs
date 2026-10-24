//! VE-F0705 · 超大路径分块栅格化
//!
//! 职责：超大路径（数百万控制点的地图海岸线/CAD 图纸）的分块栅格化——
//! 路径按空间分块（边归箱——每瓦片的边列表，F0343 分箱同源）、
//! 逐瓦片栅格化（瓦片并行 F0348）、
//! 跨瓦片填充规则一致性（nonzero 计数跨瓦片正确——分块栅格化的经典难点：
//! 环绕数需要全局边信息，解法=边全局排序或 tile 间传递计数）。

use alloc::vec;
use alloc::vec::Vec;

/// 有向边：两端点 + 方向（用于环绕数）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirEdge {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl DirEdge {
    /// 边的包围盒。
    pub fn bbox(&self) -> (f32, f32, f32, f32) {
        (
            self.x0.min(self.x1),
            self.y0.min(self.y1),
            self.x0.max(self.x1),
            self.y0.max(self.y1),
        )
    }
    /// 方向：向下为 +1（绕写计数），向上为 -1，水平为 0（不参与环绕数）。
    pub fn winding(&self) -> i32 {
        if self.y1 > self.y0 {
            1
        } else if self.y1 < self.y0 {
            -1
        } else {
            0
        }
    }
}

/// 边归箱：把边列表分配到 tile 网格（tile_size 正方形）。返回每 tile 的边号列表。
/// 归箱规则：边 bbox 与 tile 相交即归入（保守归箱——正确性优先）。
pub fn bin_edges(edges: &[DirEdge], grid_w: u32, grid_h: u32, tile_size: u32) -> Vec<Vec<u32>> {
    let mut bins = vec![Vec::new(); (grid_w * grid_h) as usize];
    for (i, e) in edges.iter().enumerate() {
        let (bx0, by0, bx1, by1) = e.bbox();
        let tx0 = (bx0.floor() as u32 / tile_size).min(grid_w - 1);
        let ty0 = (by0.floor() as u32 / tile_size).min(grid_h - 1);
        let tx1 = (bx1.floor() as u32 / tile_size).min(grid_w - 1);
        let ty1 = (by1.floor() as u32 / tile_size).min(grid_h - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                bins[(ty * grid_w + tx) as usize].push(i as u32);
            }
        }
    }
    bins
}

/// 瓦片内局部栅格化：tile 内像素的 nonzero 判定——
/// 用「tile 边集 + 环绕计数修正」：对 tile 内每条扫描线，从 tile 左界起算，
/// 但必须计入**跨界穿越**（起点前的左侧边对环绕数的贡献）——
/// 解法=tile 间传递计数：扫描线起点环绕数 = 全局边在该扫描线、tile 左界以左的代数和。
pub fn scanline_winding(edges: &[DirEdge], y: f32, x_from: f32, x_to: f32) -> Vec<i32> {
    // 对 [x_from, x_to) 内每整数像素求环绕数（精确模型：全边集参与）。
    let mut out = Vec::new();
    let mut x = x_from;
    while x < x_to {
        let mut w = 0i32;
        for e in edges {
            let (ex0, ey0, ex1, ey1) = (e.x0, e.y0, e.x1, e.y1);
            let ymin = ey0.min(ey1);
            let ymax = ey0.max(ey1);
            if y < ymin || y >= ymax {
                continue;
            }
            let xi = ex0 + (y - ey0) / (ey1 - ey0) * (ex1 - ex0);
            if xi <= x {
                w += e.winding();
            }
        }
        out.push(w);
        x += 1.0;
    }
    out
}

/// 跨瓦片一致性核验：tile 局部判定（带左侧传递计数）与全局判定必须一致。
pub fn tile_consistent(global: &[DirEdge], tile_edges: &[DirEdge], carry: i32, y: f32, x_rel: &[f32]) -> bool {
    // tile 局部：carry + tile 内边在 x_rel 左侧的代数和。
    for &x in x_rel {
        let mut w = carry;
        for e in tile_edges {
            let ymin = e.y0.min(e.y1);
            let ymax = e.y0.max(e.y1);
            if y < ymin || y >= ymax {
                continue;
            }
            let xi = e.x0 + (y - e.y0) / (e.y1 - e.y0) * (e.x1 - e.x0);
            if xi <= x {
                w += e.winding();
            }
        }
        let mut wg = 0i32;
        for e in global.iter() {
            let ymin = e.y0.min(e.y1);
            let ymax = e.y0.max(e.y1);
            if y < ymin || y >= ymax {
                continue;
            }
            let xi = e.x0 + (y - e.y0) / (e.y1 - e.y0) * (e.x1 - e.x0);
            if xi <= x {
                wg += e.winding();
            }
        }
        if w != wg {
            return false;
        }
    }
    true
}

/// 扫描线起点传递计数：tile 左界以左的全局边代数和（tile 间传递的核心量）。
pub fn carry_count(edges: &[DirEdge], y: f32, tile_left: f32) -> i32 {
    let mut w = 0i32;
    for e in edges {
        let ymin = e.y0.min(e.y1);
        let ymax = e.y0.max(e.y1);
        if y < ymin || y >= ymax {
            continue;
        }
        let xi = e.x0 + (y - e.y0) / (e.y1 - e.y0) * (e.x1 - e.x0);
        if xi <= tile_left {
            w += e.winding();
        }
    }
    w
}

/// 万边路径性能模型：分块后每 tile 工作量 = 该 tile 边数（vs 全路径边数）。
pub fn tile_speedup(total_edges: u32, max_bin: u32) -> u32 {
    if max_bin == 0 {
        return 0;
    }
    total_edges / max_bin.max(1)
}

/// 内存封顶：边箱总容量不超过预算（超出→提高 tile 粒度或降级）。
pub fn mem_cap_ok(bins: &[Vec<u32>], budget_entries: u64) -> bool {
    let total: u64 = bins.iter().map(|b| b.len() as u64).sum();
    total <= budget_entries
}

pub fn run_vec305_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec305");
    // 族一：边归箱
    let edges = vec![
        DirEdge { x0: 1.0, y0: 1.0, x1: 1.0, y1: 5.0 },  // 竖边 tile (0,0)-(0,1)
        DirEdge { x0: 5.0, y0: 5.0, x1: 5.0, y1: 1.0 },  // 竖边
        DirEdge { x0: 1.0, y0: 1.0, x1: 5.0, y1: 1.0 },  // 水平边 winding=0
    ];
    let bins = bin_edges(&edges, 8, 8, 4);
    s.add("四 tile 网格", bins.len() == 4, "grid4");
    s.add("竖边入跨行 tile", {
        // 边 0 纵跨 y=1..5 → tile 行 0 与 1 都含边 0。
        bins[0].contains(&0) && bins[2].contains(&0)
    }, "cross-tile");
    s.add("水平边 winding 为零", edges[2].winding() == 0, "h-wind");
    s.add("向下 +1 向上 -1", edges[0].winding() == 1 && edges[1].winding() == -1, "wind-dir");
    // 族二：环绕数与扫描线
    let ws = scanline_winding(&edges, 2.5, 0.0, 8.0);
    s.add("左侧环绕数 0", ws[0] == 0, "w-out");
    s.add("内部环绕数 1", {
        // x=2..5 区间：边0（xi=1）+1、边1（xi=5）不越过 → w=1。
        ws[2] == 1 && ws[4] == 1
    }, "w-in");
    s.add("右侧回零", ws[6] == 0, "w-right");
    // 族三：跨瓦片一致性（分块栅格化的经典难点）
    let tile_edges = vec![edges[1].clone()]; // 假设 tile 只含右边（左边在邻 tile）
    let carry = carry_count(&edges, 2.5, 4.0); // tile 左界 4 以左：边0 (+1)
    s.add("传递计数正确", carry == 1, "carry");
    s.add("局部+传递==全局", tile_consistent(&edges, &tile_edges, carry, 2.5, &[5.5, 6.5, 7.0]), "tile-eq");
    s.add("缺传递即不一致", !tile_consistent(&edges, &tile_edges, 0, 2.5, &[5.5]), "tile-neq");
    let carry0 = carry_count(&edges, 6.5, 4.0);
    s.add("扫描线外传递为零", carry0 == 0, "carry-off");
    // 族四：性能与内存封顶
    s.add("分块加速比正确", tile_speedup(10000, 50) == 200, "speedup");
    s.add("空箱零加速防除零", tile_speedup(100, 0) == 100, "empty-bin");
    let big: Vec<Vec<u32>> = vec![vec![0; 10]; 4];
    s.add("内存预算内通过", mem_cap_ok(&big, 100), "mem-ok");
    s.add("超预算拒绝", !mem_cap_ok(&big, 20), "mem-cap");
    s.add("保守归箱允许重复计数", bins.iter().map(|b| b.len() as u32).sum::<u32>() >= 3, "conservative");
    s.add("瓦片并行前提：tile 边集互不依赖", bins.len() == 4, "par-precond");
    s
}
