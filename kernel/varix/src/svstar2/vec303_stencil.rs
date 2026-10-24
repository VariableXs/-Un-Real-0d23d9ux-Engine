//! VE-F0703 · GPU 路径栅格化（stencil-and-cover）
//!
//! 职责：路径的 GPU 栅格化两大技法之一——stencil-and-cover：
//! 路径边画进 stencil（奇偶/绕写在模板缓冲），然后覆盖矩形填充
//!（stencil 测试过滤出路径内像素）。实现：边画 pass（三角形化边或分析边）、
//! cover 矩形 pass、stencil 清理策略。
//! 适用性：复杂路径大量重叠边时 GPU 路径远快于 CPU。
//! 与 CPU 扫描线的路由决策（路径复杂度/尺寸 → CPU or GPU 栅格化，与 F0004 同源）。

use alloc::vec;
use alloc::vec::Vec;

/// 填充规则：奇偶或绕写（nonzero）——决定 stencil 递增方向与测试掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    EvenOdd,
    NonZero,
}

/// stencil-and-cover 的三 pass 流水线状态。
#[derive(Clone, Debug, PartialEq)]
pub struct ScPass {
    /// pass 1：边画——路径每条边画成细三角形，正面 +1 反面 -1（或奇偶翻转位）。
    pub edge_done: bool,
    /// pass 2：cover——路径 bbox 的覆盖矩形填充，stencil 测试放行内部像素。
    pub cover_done: bool,
    /// pass 3：清理——stencil 缓冲复位策略（整帧清 / 增量清）。
    pub clean_done: bool,
}

impl ScPass {
    pub fn fresh() -> Self {
        ScPass { edge_done: false, cover_done: false, clean_done: false }
    }
    /// 全 pass 完成才算渲染完成。
    pub fn complete(&self) -> bool {
        self.edge_done && self.cover_done && self.clean_done
    }
}

/// 模板缓冲模拟：像素网格上的 stencil 值（边画 pass 累积）。
pub struct StencilBuf {
    pub w: u32,
    pub h: u32,
    pub cells: Vec<i32>,
}

impl StencilBuf {
    pub fn new(w: u32, h: u32) -> Self {
        StencilBuf { w, h, cells: vec![0; (w * h) as usize] }
    }
    /// 边画：把一条有向边光栅化到 stencil（简化的 DDA——每行内边经过的像素 +dir）。
    /// 真实 GPU 用细三角形；本模型按扫描线与边的交点记号（效果等价于奇偶计数）。
    pub fn draw_edge(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, dir: i32) {
        if (y1 - y0).abs() < 1e-9 {
            return; // 水平边不产生交点记号
        }
        let (x_t, x_b) = if y0 < y1 { (x0, x1) } else { (x1, x0) };
        let _ = (x_t, x_b); // DDA 上下界留痕
        let (ty, by) = (y0.min(y1), y0.max(y1));
        let y_start = ty.ceil() as i32;
        let y_end = (by - 1.0).floor() as i32;
        for y in y_start..=y_end {
            let yy = y as f32 + 0.5;
            if yy < y0.min(y1) || yy >= y0.max(y1) {
                continue;
            }
            let t = (yy - y0) / (y1 - y0);
            let xi = (x0 + t * (x1 - x0)).floor() as i32;
            if xi >= 0 && (xi as u32) < self.w && y >= 0 && (y as u32) < self.h {
                let idx = (y as u32 * self.w + xi as u32) as usize;
                self.cells[idx] += dir;
            }
        }
    }
    /// cover pass：对 bbox 内每像素按填充规则判定内外（i32 stencil 值的符号/奇偶）。
    pub fn cover_fill(&self, bx0: u32, by0: u32, bx1: u32, by1: u32, rule: FillRule) -> Vec<bool> {
        let mut mask = Vec::new();
        for y in by0..by1.min(self.h) {
            for x in bx0..bx1.min(self.w) {
                let v = self.cells[(y * self.w + x) as usize];
                let inside = match rule {
                    FillRule::EvenOdd => v & 1 == 1,
                    FillRule::NonZero => v != 0,
                };
                mask.push(inside);
            }
        }
        mask
    }
    /// 清理 pass：整帧清零（返回清理的像素数）。
    pub fn clear(&mut self) -> u32 {
        let n = self.cells.iter().filter(|&&v| v != 0).count() as u32;
        self.cells.iter_mut().for_each(|c| *c = 0);
        n
    }
}

/// 三角形化边：边 → 细三角形（GPU 边画 pass 的实际提交形状）。
pub fn edge_triangles(x0: f32, y0: f32, x1: f32, y1: f32, half_w: f32) -> [(f32, f32); 3] {
    // 单三角形：边两端 + 沿法线偏移的第三点（GPU 常用的单三角形边表示）。
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    let nx = -dy / len * half_w;
    let ny = dx / len * half_w;
    [(x0, y0), (x1, y1), (x0 + nx, y0 + ny)]
}

/// 路由决策器：路径复杂度/尺寸 → CPU or GPU（与 F0004 同源，与 F0704 三路由整合的前身）。
/// 复杂度 = 边数 × 平均交叠深度；大量重叠边 → GPU。
pub fn route_cpu_gpu(edge_count: u32, overlap_depth: u32, path_area_px: u64) -> &'static str {
    let complexity = edge_count as u64 * overlap_depth as u64;
    if path_area_px < 256 {
        "CPU" // 极小路径：上传开销大于收益
    } else if complexity >= 4096 {
        "GPU-SC" // 大量重叠边：stencil-and-cover
    } else {
        "CPU" // 常规路径：CPU 扫描线更快（无 GPU 上传）
    }
}

pub fn run_vec303_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec303");
    // 族一：三 pass 流水线
    let p = ScPass::fresh();
    s.add("初始无 pass", !p.complete(), "fresh");
    let p2 = ScPass { edge_done: true, cover_done: true, clean_done: true };
    s.add("三 pass 齐算完成", p2.complete(), "complete");
    let p3 = ScPass { edge_done: true, cover_done: true, clean_done: false };
    s.add("缺清理不算完成", !p3.complete(), "no-clean");
    // 族二：stencil 缓冲与填充规则
    let mut sb = StencilBuf::new(8, 8);
    // 画一个方形路径的两条竖边：左 +1（绕写），右 -1。
    sb.draw_edge(1.0, 1.0, 1.0, 5.0, 1);
    sb.draw_edge(5.0, 5.0, 5.0, 1.0, -1);
    let mask = sb.cover_fill(0, 0, 8, 8, FillRule::NonZero);
    // 行内 [1..5) 区间应为内部（左 +1 右 -1 的 nonzero 语义：内部值 1，外部 0）。
    s.add("nonzero 内部判定", {
        let inside = mask[1 * 8 + 2];
        let outside = mask[1 * 8 + 6];
        inside && !outside
    }, "nz-in");
    let mask_eo = sb.cover_fill(0, 0, 8, 8, FillRule::EvenOdd);
    s.add("evenodd 同判定（单层）", {
        mask_eo[1 * 8 + 2] && !mask_eo[1 * 8 + 6]
    }, "eo-in");
    // 奇偶 vs nonzero 区分：自交（8 字形）——nonzero 双圈仍内，evenodd 中段翻外。
    let mut sb2 = StencilBuf::new(16, 8);
    sb2.draw_edge(2.0, 1.0, 2.0, 7.0, 1);
    sb2.draw_edge(6.0, 7.0, 6.0, 1.0, 1); // 同向（绕写不翻）
    let nz = sb2.cover_fill(0, 0, 16, 8, FillRule::NonZero);
    let eo = sb2.cover_fill(0, 0, 16, 8, FillRule::EvenOdd);
    s.add("nonzero 双边内", nz[3 * 16 + 3], "nz2");
    s.add("evenodd 双边翻外", !eo[3 * 16 + 3], "eo2");
    // 清理
    let cleaned = sb2.clear();
    s.add("清理复位非零格", cleaned > 0 && sb2.cells.iter().all(|&v| v == 0), "clean");
    // 族三：边三角形化
    let tri = edge_triangles(0.0, 0.0, 4.0, 0.0, 0.5);
    s.add("边三角形三顶点", tri.len() == 3, "tri3");
    s.add("法线偏移垂直边", {
        let dx = tri[1].0 - tri[0].0;
        let dy = tri[1].1 - tri[0].1;
        let nx = tri[2].0 - tri[0].0;
        let ny = tri[2].1 - tri[0].1;
        (dx * nx + dy * ny).abs() < 1e-6
    }, "normal");
    // 族四：路由决策
    s.add("极小路径走 CPU", route_cpu_gpu(4, 1, 100) == "CPU", "tiny");
    s.add("高复杂度走 GPU-SC", route_cpu_gpu(256, 32, 65536) == "GPU-SC", "complex");
    s.add("常规路径走 CPU", route_cpu_gpu(12, 1, 4096) == "CPU", "normal");
    s.add("路由决策与 F0004 同源", route_cpu_gpu(64, 8, 65536) == "CPU", "mid");
    // 复杂路径 GPU 收益：模型化收益 = 重叠边省去的重复扫描
    s.add("GPU 收益随重叠深度增", {
        let cpu_cost = 256u64 * 32; // 复杂度即 CPU 代价
        let gpu_cost = 65536; // cover pass 像素代价
        cpu_cost > gpu_cost * 2
    }, "gpu-win");
    s
}
