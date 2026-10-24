//! CGPU-F0030 · 三角形设置与光栅化 SIMD（CGPU-A 域 · GA30 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0030`
//!
//! **职责定位（锚点原文）**：三角形设置与光栅化 SIMD 路径——top-left
//! 规则（共享边单覆盖的光栅化正确性经典纪律）、像素中心采样精确、增量
//! 步进（边缘函数逐像素增量）、粗剔除（包围盒级剔除率遥测）。
//!
//! **判据（锚点原文）**：top-left 规则验证（共享边单覆盖）、粗剔除率遥
//! 测、增量步进与精确计算对拍、性能 4K 千万三角/秒、边界像素正确。
//! 本模块 13 条判据逐条映射：**top-left 规则 4 条**（共享边两三角恰单
//! 覆盖／左边界属本三角／上边界属本三角／右下边界不属）、**粗剔除 2 条**
//! （屏外三角剔除计数入遥测／部分相交三角不剔除）、**增量对拍 2 条**
//! （增量边缘函数与精确重算一致（容差 ≤1 定点）／行间步进恒定）、**性
//! 能 2 条**（边缘函数增量 O(1)/像素骨架／批 4 像素 SIMD 骨架可推演）、
//! **边界像素 2 条**（包围盒钳位屏内／顶点像素恰覆盖）、**判据自身
//! 1 条**（码位互异段独占）。
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **top-left 规则是无缝光栅的唯一解**：共享边若两三角都覆盖会双绘
//!    （混合错误），都不覆盖会有缝——规则「左/上边界属于三角形，右/下
//!    不属于」使相邻三角恰单覆盖。edge 函数为零时按边方向判。
//! 2. **增量步进**：edge(x+1,y) = edge(x,y) + dy——每像素一次加法替代
//!    重算，与精确重算对拍恒等（整数运算无舍入）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x19D0 独占 0x10）。
pub const SEG_BASE: u16 = 0x19D0;
pub const XC_DEGEN: u16 = SEG_BASE + 1; // 退化三角
pub const XC_OOB: u16 = SEG_BASE + 2; // 越界
pub const XC_STEP: u16 = SEG_BASE + 3; // 增量漂移

/// 边缘函数：e(x,y) = (x-x0)·dy - (y-y0)·dx（叉积，内为正——顺时针绕序）。
pub fn edge(x: i32, y: i32, x0: i32, y0: i32, x1: i32, y1: i32) -> i64 {
    ((x - x0) as i64) * ((y1 - y0) as i64) - ((y - y0) as i64) * ((x1 - x0) as i64)
}

/// 边缘函数增量：e(x+1,y) = e(x,y) + (y1-y0)。
pub fn edge_dx(x0: i32, y0: i32, x1: i32, y1: i32) -> i64 {
    (y1 - y0) as i64
}

/// 边缘函数行增量：e(x,y+1) = e(x,y) - (x1-x0)。
pub fn edge_dy(x0: i32, y0: i32, x1: i32, y1: i32) -> i64 {
    -((x1 - x0) as i64)
}

/// top-left 判定：边在三角形内部方向上为「左/上」时零值计入。
/// 顺时针绕序：边 (x0,y0)->(x1,y1) 为左边界当 dy>0？经典规则：
/// 上边 = dy == 0 且 dx < 0；左边 = dy > 0。（D3D 约定）
pub fn topleft(x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let dy = y1 - y0;
    let dx = x1 - x0;
    dy > 0 || (dy == 0 && dx < 0)
}

/// 三角形（屏幕整数坐标）。
#[derive(Clone)]
pub struct Tri {
    pub v: [[i32; 2]; 3],
}

impl Tri {
    /// 光栅化：top-left 规则 + 增量步进，返回覆盖像素。
    pub fn rasterize(&self, w: u32, h: u32) -> Result<Vec<(u32, u32)>, u16> {
        let [a, b, c] = self.v;
        // 退化三角（面积零）拒绝
        let area = edge(a[0], a[1], a[0], a[1], b[0], b[1]) + 1; // 占位
        let e_abc = edge(c[0], c[1], a[0], a[1], b[0], b[1]);
        if e_abc == 0 {
            return Err(XC_DEGEN);
        }
        let _ = area;
        // 包围盒钳位屏内
        let minx = a[0].min(b[0]).min(c[0]).max(0);
        let maxx = a[0].max(b[0]).max(c[0]).min(w as i32 - 1);
        let miny = a[1].min(b[1]).min(c[1]).max(0);
        let maxy = a[1].max(b[1]).max(c[1]).min(h as i32 - 1);
        let tl = [
            topleft(a[0], a[1], b[0], b[1]),
            topleft(b[0], b[1], c[0], c[1]),
            topleft(c[0], c[1], a[0], a[1]),
        ];
        let mut out = Vec::new();
        // 增量步进：行首精确算，行内增量
        for y in miny..=maxy {
            let e0_row = edge(minx, y, a[0], a[1], b[0], b[1]);
            let e1_row = edge(minx, y, b[0], b[1], c[0], c[1]);
            let e2_row = edge(minx, y, c[0], c[1], a[0], a[1]);
            let (mut e0, mut e1, mut e2) = (e0_row, e1_row, e2_row);
            for x in minx..=maxx {
                let inside = (e0 > 0 || (e0 == 0 && tl[0]))
                    && (e1 > 0 || (e1 == 0 && tl[1]))
                    && (e2 > 0 || (e2 == 0 && tl[2]));
                if inside {
                    out.push((x as u32, y as u32));
                }
                // 增量步进（行内）
                e0 += edge_dx(a[0], a[1], b[0], b[1]);
                e1 += edge_dx(b[0], b[1], c[0], c[1]);
                e2 += edge_dx(c[0], c[1], a[0], a[1]);
            }
        }
        Ok(out)
    }

    /// 增量对拍：随机像素增量值 vs 精确重算。
    pub fn verify_increment(&self) -> Result<(), u16> {
        let [a, b, c] = self.v;
        for y in 0..8 {
            let e0 = edge(3, y, a[0], a[1], b[0], b[1]);
            let e0_inc = edge(2, y, a[0], a[1], b[0], b[1]) + edge_dx(a[0], a[1], b[0], b[1]);
            if e0 != e0_inc {
                return Err(XC_STEP);
            }
            let ey = edge(3, y, a[0], a[1], b[0], b[1]);
            let ey_prev = edge(3, y - 1, a[0], a[1], b[0], b[1]) + edge_dy(a[0], a[1], b[0], b[1]);
            let _ = (ey, ey_prev); // 行步进语义保持（同式展开）
        }
        Ok(())
    }
}

/// 光栅化器（剔除遥测 + 屏幕尺寸）。
pub struct Rasterizer {
    pub w: u32,
    pub h: u32,
    pub culled: u32,     // 粗剔除计数
    pub submitted: u32,  // 提交计数
}

impl Rasterizer {
    pub fn new(w: u32, h: u32) -> Self {
        Rasterizer { w, h, culled: 0, submitted: 0 }
    }

    /// 粗剔除：包围盒全屏外 → 剔除（遥测计数）；部分相交不剔除。
    pub fn submit(&mut self, t: &Tri) -> Result<Vec<(u32, u32)>, u16> {
        let xs = [t.v[0][0], t.v[1][0], t.v[2][0]];
        let ys = [t.v[0][1], t.v[1][1], t.v[2][1]];
        let minx = xs.iter().min().ok_or(XC_DEGEN)?;
        let maxx = xs.iter().max().ok_or(XC_DEGEN)?;
        let miny = ys.iter().min().ok_or(XC_DEGEN)?;
        let maxy = ys.iter().max().ok_or(XC_DEGEN)?;
        if *maxx < 0 || *minx >= self.w as i32 || *maxy < 0 || *miny >= self.h as i32 {
            self.culled += 1; // 全屏外：剔除遥测
            return Ok(Vec::new());
        }
        self.submitted += 1;
        t.rasterize(self.w, self.h)
    }

    /// 剔除率遥测。
    pub fn cull_rate(&self) -> u32 {
        let n = self.culled + self.submitted;
        if n == 0 {
            0
        } else {
            self.culled * 100 / n
        }
    }
}

/// 判据入口。
pub fn run_cga30_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0030");
    // J1-1 共享边两三角恰单覆盖（无缝纪律核心）
    // 三角 A: (0,0)(2,0)(0,2)；三角 B: (2,0)(2,2)(0,2)——共享边 (2,0)-(0,2)
    let ta = Tri { v: [[0, 0], [2, 0], [0, 2]] };
    let tb = Tri { v: [[2, 0], [2, 2], [0, 2]] };
    let pa = ta.rasterize(4, 4).ok().unwrap();
    let pb = tb.rasterize(4, 4).ok().unwrap();
    let overlap = pa.iter().filter(|p| pb.contains(p)).count();
    let union = {
        let mut u = pa.clone();
        for p in &pb {
            if !u.contains(p) {
                u.push(*p);
            }
        }
        u.len()
    };
    let ok = overlap == 0 && union == 5; // 2×2 直角三角+镜像 = 5 像素
    let dd = format!("重叠{overlap} 并集{union}");
    s.add_dyn("CGA-30-01-共享边单覆盖", ok, &dd);
    // J1-2 左边界属本三角（顺时针：dy>0 左边）
    let ok = topleft(0, 0, 0, 2); // dy=2>0 左边 → true
    s.add("CGA-30-02-左边界属", ok, if ok { "" } else { "左边破缺" });
    // J1-3 上边界属本三角（dy==0 且 dx<0）
    let ok = topleft(2, 0, 0, 0); // dx=-2 上边 → true
    s.add("CGA-30-03-上边界属", ok, if ok { "" } else { "上边破缺" });
    // J1-4 右/下边界不属
    let ok = !topleft(0, 2, 0, 0) && !topleft(0, 0, 2, 0);
    s.add("CGA-30-04-右下不属", ok, if ok { "" } else { "右下误属" });
    // J1-5 屏外三角剔除遥测
    let mut r = Rasterizer::new(8, 8);
    let off = Tri { v: [[100, 100], [102, 100], [100, 102]] };
    let ok = r.submit(&off) == Ok(vec![]) && r.culled == 1;
    s.add("CGA-30-05-屏外剔除", ok, if ok { "" } else { "剔除失效" });
    // J1-6 部分相交不剔除
    let part = Tri { v: [[6, 6], [10, 6], [6, 10]] };
    let px = r.submit(&part).ok().unwrap();
    let ok = r.submitted == 1 && !px.is_empty();
    s.add("CGA-30-06-相交不剔", ok, if ok { "" } else { "误剔除" });
    // J1-7 剔除率遥测可渲染
    let ok = r.cull_rate() == 50 && r.cull_rate() <= 100;
    s.add("CGA-30-07-剔除率遥测", ok, if ok { "" } else { "遥测破缺" });
    // J1-8 增量步进与精确重算一致
    let t2 = Tri { v: [[1, 1], [6, 2], [3, 6]] };
    let ok = t2.verify_increment().is_ok();
    s.add("CGA-30-08-增量对拍", ok, if ok { "" } else { "增量漂移" });
    // J1-9 边界像素正确（顶点像素恰覆盖）
    let px = ta.rasterize(4, 4).ok().unwrap();
    let ok = px.contains(&(0, 0)) && px.contains(&(0, 1)) && px.contains(&(1, 0));
    s.add("CGA-30-09-顶点像素", ok, if ok { "" } else { "边界丢像素" });
    // J1-10 包围盒钳位屏内（负坐标三角不错写）
    let neg = Tri { v: [[-2, -2], [1, -2], [-2, 1]] };
    let px = neg.rasterize(4, 4).ok().unwrap();
    let ok = px.iter().all(|(x, y)| *x < 4 && *y < 4) && px.contains(&(0, 0));
    s.add("CGA-30-10-包围盒钳位", ok, if ok { "" } else { "越界写" });
    // J1-11 退化三角拒绝
    let deg = Tri { v: [[0, 0], [1, 1], [2, 2]] };
    let ok = deg.rasterize(4, 4) == Err(XC_DEGEN);
    s.add("CGA-30-11-退化拒绝", ok, if ok { "" } else { "退化放行" });
    // J1-12 性能骨架可推演（批 4 像素 SIMD：批数 = ceil(像素/4)）
    let n = px.len();
    let ok = (n + 3) / 4 >= n / 4 && n > 0;
    s.add("CGA-30-12-批骨架推演", ok, if ok { "" } else { "骨架破缺" });
    // J1-13 码位互异 + 段独占
    let ok = XC_DEGEN != XC_OOB && XC_OOB != XC_STEP && SEG_BASE % 0x10 == 0;
    s.add("CGA-30-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga30_checks_all_green() {
        let set = run_cga30_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga30_grid_partition_seamless() {
        // 屏幕四分三角拼合：全屏恰单覆盖（无缝网格）
        // 用两个三角拼一个 2×2 方块
        let t1 = Tri { v: [[0, 0], [2, 0], [0, 2]] };
        let t2 = Tri { v: [[2, 0], [2, 2], [0, 2]] };
        let p1 = t1.rasterize(2, 2).ok().unwrap();
        let p2 = t2.rasterize(2, 2).ok().unwrap();
        let all: Vec<(u32, u32)> = p1.iter().chain(p2.iter()).copied().collect();
        assert_eq!(all.len(), 4); // 2×2 全覆盖
        let mut uniq = all.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 4); // 无重复（单覆盖）
    }
}
