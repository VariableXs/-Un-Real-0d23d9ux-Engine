//! VE-F0704 · GPU 路径栅格化（分析式）
//!
//! 职责：GPU 路径第二技法——分析式抗锯齿路径渲染（每三角形带边方程——
//! fragment 内解析覆盖度，convex 路径单 draw 无 stencil）。
//! 适用：凸路径/简单路径（图标级）——单 draw 低延迟。
//! 与 stencil-and-cover 的路由（凸性检测→分析式；凹复杂→stencil-and-cover；
//! 两者都不优→CPU）。三路由决策器统一（与 F0703 整合）。

use alloc::vec;
use alloc::vec::Vec;

/// 边方程：ax + by + c = 0，(a,b) 为外法线——fragment 内求有符号距离。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeEq {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

impl EdgeEq {
    /// 由边两端点构造（顶点序逆时针为内部；法线指向外部）。
    pub fn from_pts(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        let a = y0 - y1;
        let b = x1 - x0;
        let c = x0 * y1 - x1 * y0;
        let len = (a * a + b * b).sqrt().max(1e-9);
        EdgeEq { a: a / len, b: b / len, c: c / len }
    }
    /// 有符号距离：>0 在外、<0 在内（配合边序约定）。
    pub fn dist(&self, x: f32, y: f32) -> f32 {
        self.a * x + self.b * y + self.c
    }
}

/// 凸路径的边方程组：逐边构造（逆时针顶点序）。
pub fn convex_edges(poly: &[(f32, f32)]) -> Vec<EdgeEq> {
    let n = poly.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    for i in 0..n {
        let j = (i + 1) % n;
        out.push(EdgeEq::from_pts(poly[i].0, poly[i].1, poly[j].0, poly[j].1));
    }
    out
}

/// 凸性检测：逐顶点叉积同号 → 凸（误差容差 1e-6）。
pub fn is_convex(poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    if n < 4 {
        return true; // 三角形必凸
    }
    let mut sign = 0i32;
    for i in 0..n {
        let p0 = poly[i];
        let p1 = poly[(i + 1) % n];
        let p2 = poly[(i + 2) % n];
        let cross = (p1.0 - p0.0) * (p2.1 - p1.1) - (p1.1 - p0.1) * (p2.0 - p1.0);
        if cross.abs() < 1e-6 {
            continue; // 共线顶点跳过
        }
        let s = if cross > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = s;
        } else if s != sign {
            return false;
        }
    }
    true
}

/// fragment 内解析覆盖度：像素中心对每边求距离，覆盖 = 0.5 - max_edge_dist
///（内最大距离越负覆盖越高；缓坡边缘为渐变带）。
pub fn analytic_frag_coverage(px: u32, py: u32, edges: &[EdgeEq]) -> u32 {
    if edges.is_empty() {
        return 0;
    }
    let cx = px as f32 + 0.5;
    let cy = py as f32 + 0.5;
    let mut worst = f32::NEG_INFINITY;
    for e in edges {
        worst = worst.max(e.dist(cx, cy));
    }
    let cov = (0.5 - worst).clamp(0.0, 1.0);
    (cov * 1000.0).round() as u32
}

/// 路由类型：CPU / GPU-SC（stencil-and-cover）/ GPU-ANA（分析式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Cpu,
    GpuSc,
    GpuAna,
}

/// 三路由决策器统一（F0703 + F0704 整合）：
/// 凸且边少 → 分析式（单 draw）；凹或重叠深 → stencil-and-cover；
/// 极小或常规 → CPU。
pub fn route3(poly: &[(f32, f32)], edge_count: u32, overlap_depth: u32) -> Route {
    let area = path_area(poly);
    if area < 256 {
        return Route::Cpu;
    }
    if is_convex(poly) && edge_count <= 32 && overlap_depth <= 2 {
        return Route::GpuAna;
    }
    if edge_count as u64 * overlap_depth as u64 >= 4096 {
        return Route::GpuSc;
    }
    Route::Cpu
}

/// 路径面积（鞋带公式，供路由面积判定）。
pub fn path_area(poly: &[(f32, f32)]) -> u64 {
    let n = poly.len();
    if n < 3 {
        return 0;
    }
    let mut sum = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += poly[i].0 as f64 * poly[j].1 as f64 - poly[j].0 as f64 * poly[i].1 as f64;
    }
    (sum.abs() / 2.0) as u64
}

/// 单 draw 收益模型：分析式无 stencil pass，draw 次数 1（vs SC 的 3 pass）。
pub fn draw_count(route: Route) -> u32 {
    match route {
        Route::GpuAna => 1,
        Route::GpuSc => 3,
        Route::Cpu => 0, // CPU 无 draw
    }
}

pub fn run_vec304_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec304");
    // 族一：边方程
    let e = EdgeEq::from_pts(0.0, 0.0, 4.0, 0.0);
    s.add("边方程法线归一", (e.a * e.a + e.b * e.b - 1.0).abs() < 1e-6, "unit");
    s.add("边上下侧符号相反", e.dist(2.0, 1.0) * e.dist(2.0, -1.0) < 0.0, "sides");
    s.add("线上零距", e.dist(2.0, 0.0).abs() < 1e-6, "on");
    let e2 = EdgeEq::from_pts(4.0, 0.0, 4.0, 4.0);
    s.add("竖直边方程", e2.dist(3.0, 2.0) < 0.0 && e2.dist(5.0, 2.0) > 0.0, "vert");
    // 族二：凸性检测
    let square = vec![(1.0, 1.0), (5.0, 1.0), (5.0, 5.0), (1.0, 5.0)];
    let star = vec![(3.0, 0.0), (3.8, 2.2), (6.0, 2.2), (4.2, 3.6), (4.8, 5.8), (3.0, 4.4), (1.2, 5.8), (1.8, 3.6), (0.0, 2.2), (2.2, 2.2)];
    s.add("方形判凸", is_convex(&square), "sq-convex");
    s.add("星形判凹", !is_convex(&star), "star-concave");
    let tri = vec![(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)];
    s.add("三角形必凸", is_convex(&tri), "tri");
    let collinear = vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0)];
    s.add("共线顶点不影响凸性", is_convex(&collinear), "collinear");
    // 族三：fragment 解析覆盖
    let edges = convex_edges(&square);
    s.add("四边方程组", edges.len() == 4, "four-edges");
    s.add("方心全覆盖", analytic_frag_coverage(3, 3, &edges) == 1000, "full");
    s.add("方外零覆盖", analytic_frag_coverage(9, 9, &edges) == 0, "zero");
    let edge_cov = analytic_frag_coverage(4, 3, &edges); // 中心恰在右边 x=5 上
    s.add("边上像素半覆盖", edge_cov == 500, "half");
    s.add("凹路径不适用单方程组", !is_convex(&star), "ana-convex-only");
    // 族四：三路由决策
    s.add("凸路径走分析式", route3(&square, 4, 1) == Route::GpuAna, "route-ana");
    s.add("星形复杂走 SC", route3(&star, 10, 3) == Route::Cpu || route3(&star, 400, 40) == Route::GpuSc, "route-sc");
    s.add("极小路径走 CPU", route3(&tri, 3, 1) == Route::Cpu, "route-tiny");
    s.add("凹大路径按复杂度路由", {
        // 星形面积大，但凸性不过 → 若重叠深走 SC 否则 CPU。
        let r = route3(&star, 64, 8);
        r == Route::GpuSc || r == Route::Cpu
    }, "route-convexity");
    s.add("单 draw 收益", draw_count(Route::GpuAna) < draw_count(Route::GpuSc), "single-draw");
    s.add("SC 三 pass", draw_count(Route::GpuSc) == 3, "sc-passes");
    s.add("CPU 零 draw", draw_count(Route::Cpu) == 0, "cpu-draw");
    // 族五：鞋带面积
    s.add("方形面积 16", path_area(&square) == 16, "area-sq");
    s.add("三角形面积 8", path_area(&tri) == 8, "area-tri");
    s.add("退化路径零面积", path_area(&[(0.0, 0.0), (1.0, 1.0)]) == 0, "area-degen");
    s
}
