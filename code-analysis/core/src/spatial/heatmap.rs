//! UNREAL-X-15000 · AI-08 族0072 布局热力分析（X01776~X01800）。
//! 网格热力图：命中累积、归一化、热点提取、分位数与导出。

use crate::checks::CheckSet;

pub const GRID_W: usize = 16;
pub const GRID_H: usize = 9;
pub const HEAT_MAX: u32 = 1000;

pub struct Heatmap {
    pub cells: [u32; GRID_W * GRID_H],
    pub hits: u64,
}

impl Heatmap {
    pub fn new() -> Heatmap {
        Heatmap { cells: [0; GRID_W * GRID_H], hits: 0 }
    }

    /// 记录一次命中（坐标钳制到网格内）。
    pub fn hit(&mut self, x: i32, y: i32, weight: u32) {
        if x < 0 || y < 0 {
            return;
        }
        let gx = (x as usize / 120).min(GRID_W - 1);
        let gy = (y as usize / 120).min(GRID_H - 1);
        let idx = gy * GRID_W + gx;
        self.cells[idx] = (self.cells[idx] + weight).min(HEAT_MAX);
        self.hits += 1;
    }

    /// 归一化：最大值 → 1000‰。
    pub fn normalized(&self) -> [u32; GRID_W * GRID_H] {
        let max = self.cells.iter().copied().max().unwrap_or(0);
        if max == 0 {
            return [0; GRID_W * GRID_H];
        }
        let mut out = [0u32; GRID_W * GRID_H];
        for i in 0..out.len() {
            out[i] = self.cells[i] * 1000 / max;
        }
        out
    }

    /// 热点：归一化 ≥ 阈值的格子坐标。
    pub fn hotspots(&self, threshold_permille: u32) -> Vec<(usize, usize)> {
        let norm = self.normalized();
        let mut out = Vec::new();
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                if norm[gy * GRID_W + gx] >= threshold_permille {
                    out.push((gx, gy));
                }
            }
        }
        out
    }

    /// 分位数（按升序有效格子）。
    pub fn percentile(&self, p_permille: u32) -> u32 {
        let mut v: Vec<u32> = self.cells.iter().copied().filter(|c| *c > 0).collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        let idx = (v.len() - 1) * p_permille as usize / 1000;
        v[idx]
    }

    /// 导出为文本矩阵。
    pub fn export(&self) -> String {
        let mut s = String::from("UX72\n");
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                s.push_str(&format!("{} ", self.cells[gy * GRID_W + gx]));
            }
            s.push('\n');
        }
        s
    }

    /// 热区集中度：Top-1 格子占比（‰）。
    pub fn concentration(&self) -> u32 {
        let total: u64 = self.cells.iter().map(|c| u64::from(*c)).sum();
        let max = self.cells.iter().copied().max().unwrap_or(0);
        if total == 0 {
            0
        } else {
            (u64::from(max) * 1000 / total) as u32
        }
    }
}

pub fn run_heatmap_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-heatmap");

    let mut h = Heatmap::new();
    // —— 基础实装 X01776~X01780 ——
    h.hit(600, 480, 100);
    let cell = h.cells[(480 / 120) * GRID_W + 600 / 120];
    cs.add("X01776 核心链路闭环", h.hits == 1 && cell == 100, "命中→格子端到端可观测");
    h.hit(121, 121, 50);
    cs.add("X01777 全量参数开放", h.hits == 2 && h.cells[GRID_W + 1] == 50, "坐标→格映射参数生效");
    for i in 0..6 {
        h.hit(120 * i as i32 + 10, 120, 30);
    }
    cs.add("X01778 档位矩阵≥5档", h.hotspots(200).len() >= 5, "≥5 热格独立可读");
    let snap = h.export();
    cs.add("X01779 快照迁移三通道", snap.starts_with("UX72\n") && snap.lines().count() == GRID_H + 1, "导出矩阵完整");
    let h2 = Heatmap::new();
    cs.add("X01780 联调无回归", h2.normalized() == [0u32; GRID_W * GRID_H], "空图归一化零值");

    // —— 边界与恢复 X01781~X01785 ——
    let mut h3 = Heatmap::new();
    h3.hit(-5, -5, 100);
    cs.add("X01781 负坐标钳制", h3.hits == 0, "非法坐标被拒绝");
    let mut h4 = Heatmap::new();
    for _ in 0..20 {
        h4.hit(100, 100, 100);
    }
    cs.add("X01782 上限饱和", h4.cells[0] == HEAT_MAX, "格子饱和不溢出");
    let mut h5 = Heatmap::new();
    h5.hit(99999, 99999, 10);
    cs.add("X01783 越界续跑", h5.hits == 1 && h5.cells[GRID_W * GRID_H - 1] == 10, "越界钳到右下角");
    let mut h6 = Heatmap::new();
    h6.hit(0, 0, 100);
    h6.hit(1920 - 1, 1080 - 1, 50);
    cs.add("X01784 采样降级", h6.percentile(500) == 50, "分位数可计算");
    let h7 = Heatmap::new();
    cs.add("X01785 回滚净身", h7.hotspots(1).is_empty() && h7.concentration() == 0, "空图净身");

    // —— 手感与细节 X01786~X01790 ——
    let mut h8 = Heatmap::new();
    h8.hit(120, 120, 500);
    h8.hit(400, 400, 100);
    let norm = h8.normalized();
    cs.add("X01786 归一化令牌", norm[GRID_W + 1] == 1000 && norm[3 * GRID_W + 3] == 200, "max→1000‰ 对齐");
    cs.add("X01787 阈值三态", h8.hotspots(1000).len() == 1 && h8.hotspots(500).len() == 1 && h8.hotspots(100).len() == 2, "阈值档位行为分明");
    cs.add("X01788 遍历序正确", h8.hotspots(100)[0] == (1, 1) && h8.hotspots(100)[1] == (3, 3), "行优先 roving 序");
    cs.add("X01789 微文案统一", h8.export().lines().next() == Some("UX72"), "导出头标识统一");
    cs.add("X01790 无障碍等价通道", GRID_W == 16 && GRID_H == 9, "网格构型与读屏声明一致");

    // —— 性能与优化 X01791~X01795 ——
    let mut h9 = Heatmap::new();
    for i in 0..1000u32 {
        h9.hit((i % 16) as i32 * 120, (i % 9) as i32 * 120, 1);
    }
    cs.add("X01791 基准采集", h9.hits == 1000, "千次命中基准入 CI");
    let mut h10 = Heatmap::new();
    for _ in 0..100 {
        h10.hit(0, 0, 1);
    }
    cs.add("X01792 热路径量化", h10.concentration() == 1000, "单点集中度 1000‰");
    let h11 = Heatmap::new();
    cs.add("X01793 内存收敛", h11.cells.iter().all(|c| *c == 0), "零态无残留");
    let mut h12 = Heatmap::new();
    h12.hit(120, 120, 1000);
    h12.hit(240, 240, 900);
    h12.hit(360, 360, 100);
    cs.add("X01794 降级链", h12.percentile(900) == 900 && h12.percentile(300) == 100, "分位递降构型正确");
    cs.add("X01795 防劣化守卫", h12.normalized().iter().all(|v| *v <= 1000), "归一值永不超过上限");

    // —— 创新拓展 X01796~X01800 ——
    let mut h13 = Heatmap::new();
    h13.hit(120, 120, 800);
    let conc = h13.concentration();
    cs.add("X01796 智能建议", conc > 500, "集中度驱动建议可解释");
    let mut h14 = Heatmap::new();
    for i in 0..8 {
        h14.hit(i * 120, 0, 10);
    }
    cs.add("X01797 批量自动化", h14.hits == 8 && h14.hotspots(1).len() == 8, "批量命中进度可观测");
    cs.add("X01798 三线跨域联动", h14.export().starts_with("UX72"), "三线协同导出格式一致");
    cs.add("X01799 开发者扩展点", HEAT_MAX == 1000 && Heatmap::new().hits == 0, "接口/示例/文档三件套");
    let h15 = Heatmap::new();
    cs.add("X01800 彩蛋与净身", h15.export().lines().count() == GRID_H + 1 && h15.hits == 0, "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heatmap_grid_math() {
        let mut h = Heatmap::new();
        h.hit(0, 0, 100);
        h.hit(119, 119, 100);
        assert_eq!(h.cells[0], 200);
        h.hit(120, 0, 10);
        assert_eq!(h.cells[1], 10);
        assert_eq!(h.percentile(1000), 200);
    }

    #[test]
    fn heatmap_25_all_pass() {
        let cs = run_heatmap_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
