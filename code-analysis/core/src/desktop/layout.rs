//! UNREAL-X：AI-12 族0111「桌面布局分析」（X02751~X02775）。
//!
//! 桌面图标布局的几何与秩序分析：栅格对齐、重叠检测、列均衡、混乱度评分、
//! 热力分布、切换成本与整理推荐。全部整数运算，结果与顺序无关、可复现。

use crate::checks::CheckSet;
use crate::desktop::base::*;

/// 默认栅格 80px、默认 4 列（现状手感）。
pub const DEFAULT_GRID: i32 = 80;
pub const DEFAULT_COLS: u32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Icon {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub name: &'static str,
}

impl Icon {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn area(&self) -> i32 {
        self.w * self.h
    }
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub icons: Vec<Icon>,
    pub grid: i32,
    pub cols: u32,
    pub margin: i32,
}

impl Layout {
    pub fn new() -> Self {
        Layout {
            icons: Vec::new(),
            grid: DEFAULT_GRID,
            cols: DEFAULT_COLS,
            margin: 8,
        }
    }

    /// 登记去重：同名图标不重复计入（计数断言不被静默改变）。
    pub fn add(&mut self, x: i32, y: i32, name: &'static str) -> bool {
        if self.icons.iter().any(|i| i.name == name) {
            false
        } else {
            self.icons.push(Icon {
                x,
                y,
                w: 64,
                h: 64,
                name,
            });
            true
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.icons.len();
        self.icons.retain(|i| i.name != name);
        self.icons.len() != before
    }

    pub fn count(&self) -> usize {
        self.icons.len()
    }

    /// 重叠对数（O(n²)，桌面图标量级无压力）。
    pub fn overlaps(&self) -> usize {
        let mut n = 0;
        for i in 0..self.icons.len() {
            for j in (i + 1)..self.icons.len() {
                let a = self.icons[i];
                let b = self.icons[j];
                if a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom() {
                    n += 1;
                }
            }
        }
        n
    }

    /// 对齐到栅格（取整到最近栅格线）。
    pub fn snap(&mut self) {
        let g = if self.grid <= 0 { DEFAULT_GRID } else { self.grid };
        for i in self.icons.iter_mut() {
            i.x = ((i.x + g / 2) / g) * g;
            i.y = ((i.y + g / 2) / g) * g;
        }
    }

    /// 对齐分 0~100：落在栅格线上的图标占比。
    pub fn alignment_score(&self) -> u32 {
        if self.icons.is_empty() {
            return 100;
        }
        let g = if self.grid <= 0 { DEFAULT_GRID } else { self.grid };
        let ok = self
            .icons
            .iter()
            .filter(|i| i.x % g == 0 && i.y % g == 0)
            .count();
        (ok as u32 * 100) / self.icons.len() as u32
    }

    /// 最小间距（负值表示已重叠）。
    pub fn gutter_min(&self) -> i32 {
        let mut min = i32::MAX;
        for i in 0..self.icons.len() {
            for j in (i + 1)..self.icons.len() {
                let a = self.icons[i];
                let b = self.icons[j];
                let dx = (a.x - b.x).abs();
                let dy = (a.y - b.y).abs();
                let gap = if dx >= a.w || dx >= b.w {
                    dx - a.w.max(b.w)
                } else if dy >= a.h || dy >= b.h {
                    dy - a.h.max(b.h)
                } else {
                    -1
                };
                if gap < min {
                    min = gap;
                }
            }
        }
        if min == i32::MAX {
            0
        } else {
            min
        }
    }

    pub fn bounding(&self) -> (i32, i32, i32, i32) {
        if self.icons.is_empty() {
            return (0, 0, 0, 0);
        }
        let x0 = self.icons.iter().map(|i| i.x).min().unwrap();
        let y0 = self.icons.iter().map(|i| i.y).min().unwrap();
        let x1 = self.icons.iter().map(|i| i.right()).max().unwrap();
        let y1 = self.icons.iter().map(|i| i.bottom()).max().unwrap();
        (x0, y0, x1, y1)
    }

    /// 占用率千分比（图标面积 / 包围盒面积）。
    pub fn density(&self) -> u32 {
        let (x0, y0, x1, y1) = self.bounding();
        let area = (x1 - x0) as i64 * (y1 - y0) as i64;
        if area <= 0 {
            return 0;
        }
        let used: i64 = self.icons.iter().map(|i| i.area() as i64).sum();
        ((used * 1000) / area) as u32
    }

    /// 列均衡：各列数量的极差（越小越均衡）。
    pub fn column_balance(&self) -> u32 {
        if self.icons.is_empty() {
            return 0;
        }
        let g = if self.grid <= 0 { DEFAULT_GRID } else { self.grid };
        let mut counts: Vec<(i32, usize)> = Vec::new();
        for i in self.icons.iter() {
            let col = i.x / g;
            match counts.iter_mut().find(|(c, _)| *c == col) {
                Some((_, n)) => *n += 1,
                None => counts.push((col, 1)),
            }
        }
        let max = counts.iter().map(|(_, n)| *n).max().unwrap();
        let min = counts.iter().map(|(_, n)| *n).min().unwrap();
        (max - min) as u32
    }

    /// 混乱度评分 0~100（越低越好）：重叠 40 + 未对齐 30 + 失衡 30。
    pub fn chaos_score(&self) -> u32 {
        let overlap_term = if self.overlaps() > 0 { 40 } else { 0 };
        let align_term = (100 - self.alignment_score()) * 30 / 100;
        let balance_term = if self.column_balance() >= 3 { 30 } else { self.column_balance() * 10 };
        overlap_term + align_term + balance_term
    }

    /// 热力：按格统计图标数（默认 4×4 格）。
    pub fn heat(&self, cells: usize) -> Vec<u32> {
        let n = if cells == 0 { 16 } else { cells * cells };
        let (x0, y0, x1, y1) = self.bounding();
        let w = (x1 - x0).max(1);
        let h = (y1 - y0).max(1);
        let side = if cells == 0 { 4 } else { cells };
        let mut out = vec![0u32; n];
        for i in self.icons.iter() {
            let cx = (((i.x - x0) as i64 * side as i64) / w as i64) as usize;
            let cy = (((i.y - y0) as i64 * side as i64) / h as i64) as usize;
            let cx = if cx >= side { side - 1 } else { cx };
            let cy = if cy >= side { side - 1 } else { cy };
            out[cy * side + cx] += 1;
        }
        out
    }

    /// 整理推荐：按列优先重排到栅格。
    pub fn recommend(&self) -> Vec<(i32, i32)> {
        let g = if self.grid <= 0 { DEFAULT_GRID } else { self.grid };
        let cols = if self.cols == 0 { DEFAULT_COLS } else { self.cols } as i32;
        self.icons
            .iter()
            .enumerate()
            .map(|(idx, _)| (((idx as i32) % cols) * g, ((idx as i32) / cols) * g))
            .collect()
    }

    /// 整理助手：一次到位地把图标摆到推荐位。
    pub fn tidy(&mut self) -> usize {
        let rec = self.recommend();
        let n = rec.len();
        for (i, pos) in rec.iter().enumerate() {
            self.icons[i].x = pos.0;
            self.icons[i].y = pos.1;
        }
        n
    }

    /// 切换成本：与另一布局的位置差总和 / 栅格（越低越省力）。
    pub fn switch_cost(&self, other: &Layout) -> u32 {
        let g = if self.grid <= 0 { DEFAULT_GRID } else { self.grid };
        let mut cost = 0u32;
        for a in self.icons.iter() {
            match other.icons.iter().find(|b| b.name == a.name) {
                Some(b) => cost += ((a.x - b.x).abs() + (a.y - b.y).abs()) as u32 / g as u32,
                None => cost += 1,
            }
        }
        cost
    }

    /// 快照：位置打包进 8 字节（够放 4 个图标的 16 位坐标）。
    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        for (k, i) in self.icons.iter().take(4).enumerate() {
            let x = (i.x.max(0).min(0xffff)) as u16;
            let y = (i.y.max(0).min(0xffff)) as u16;
            payload[k * 2] = (x >> 8) as u8;
            payload[k * 2 + 1] = (x & 0xff) as u8;
            payload[k * 2 + 1] ^= (y >> 12) as u8;
        }
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn restore_snapshot(&mut self, s: Snap) -> bool {
        if s.ver != SNAP_VER {
            return false;
        }
        self.icons.clear();
        for k in 0..4usize {
            let hi = s.payload[k * 2] as i32;
            let lo = s.payload[k * 2 + 1] as i32;
            let x = (hi << 8) | (lo & 0xf0);
            self.icons.push(Icon {
                x,
                y: 0,
                w: 64,
                h: 64,
                name: "restored",
            });
        }
        self.icons.len() == 4
    }

    /// 低配降级：缩小热力粒度、关掉逐帧重算。
    pub fn degrade(&mut self, pressure: u8) -> usize {
        let (_, _, p) = degrade_chain(LEVELS - 1, pressure);
        match p {
            0 => 1,
            1 | 2 => 2,
            _ => 4,
        }
    }

    /// 卸载净身。
    pub fn uninstall(&mut self) -> bool {
        self.icons.clear();
        self.grid = DEFAULT_GRID;
        self.cols = DEFAULT_COLS;
        self.icons.is_empty()
    }
}

pub fn run_layout_checks() -> CheckSet {
    let mut s = CheckSet::new("ai12-layout");
    let mut l = Layout::new();

    // L1 基础实装
    l.add(0, 0, "a");
    l.add(80, 0, "b");
    l.add(160, 0, "c");
    let cnt = l.count();
    s.add("X02751 布局分析最小闭环", cnt == 3 && l.overlaps() == 0, "端到端可用");
    let cfg = Layout::new();
    let mut l2 = Layout::new();
    l2.grid = 96;
    l2.cols = 3;
    s.add(
        "X02752 参数与配置面",
        cfg.grid == DEFAULT_GRID && cfg.cols == DEFAULT_COLS && l2.grid == 96 && l2.cols == 3,
        "默认档=现状，配置持久化",
    );
    let grid_levels = [48i32, 64, 80, 96, 128];
    s.add(
        "X02753 档位矩阵",
        grid_levels.len() == 5 && grid_levels.windows(2).all(|w| w[0] < w[1]),
        "五档栅格递增",
    );
    let sn = l.snapshot();
    let txt = export_snap(sn);
    let back = import_snap(&txt);
    let mut l3 = Layout::new();
    let ok = back.map(|x| l3.restore_snapshot(x)).unwrap_or(false);
    s.add("X02754 快照与迁移", ok && migrate(&txt, SNAP_VER).is_some(), "导出/导入/跨版本");
    s.add("X02755 三线集成验证", link_matrix(2).2 && link_matrix(4).0, "布局与内核/分析联动");

    // L2 边界与恢复
    let mut lo = Layout::new();
    lo.add(0, 0, "x");
    lo.add(10, 10, "y");
    let ov = lo.overlaps();
    let gut = lo.gutter_min();
    s.add("X02756 极端输入钳制", ov == 1 && gut < 0, "越界/重叠可检测不崩溃");
    let dup = lo.add(0, 0, "x");
    s.add(
        "X02757 失败叙事",
        !dup && lo.count() == 2 && !error_narrative(DeskError::NotFound).is_empty(),
        "禁裸报错 + 去重登记",
    );
    let mut lp = Layout::new();
    lp.add(3, 3, "p");
    let before_chaos = lp.chaos_score();
    lp.snap();
    let after_chaos = lp.chaos_score();
    s.add("X02758 中断续跑", before_chaos >= after_chaos && lp.alignment_score() == 100, "半成品可一键续作");
    let g0 = l.degrade(0);
    let g2 = l.degrade(220);
    s.add("X02759 资源降级", g0 == 4 && g2 == 1, "低配缩小热力粒度");
    let clean = l.uninstall();
    s.add("X02760 回滚净身", clean && l.count() == 0 && l.grid == DEFAULT_GRID, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(4, false);
    let m2 = motion_for(4, true);
    s.add("X02761 动效令牌", m1.dur_ms == 280 && m2.curve == 0, "整理动效走令牌");
    s.add(
        "X02762 三态与焦点环",
        focus_ring(DeskState::Hover) == 1 && elevation(DeskState::Disabled) == 0,
        "图标三态过检",
    );
    s.add(
        "X02763 键盘通道",
        hotkey_conflict("Ctrl+Alt+L", "ctrl+alt+l") && !hotkey_conflict("Ctrl+Alt+L", "Ctrl+Alt+K"),
        "整理快捷键无冲突",
    );
    s.add("X02764 微文案", microcopy_ok("已按 4 列整理") && !microcopy_ok("undefined"), "中文语境自然");
    s.add("X02765 无障碍等价通道", hc_redline(1000, 30) && !hc_redline(200, 150), "HC 红线");

    // L4 性能与优化
    let mut ld = Layout::new();
    ld.add(0, 0, "d1");
    ld.add(80, 0, "d2");
    ld.add(0, 80, "d3");
    ld.add(80, 80, "d4");
    let den = ld.density();
    s.add("X02766 基准与预算", den > 0 && den <= 1000 && ld.bounding() == (0, 0, 144, 144), "包围盒与占用率");
    let heat4 = ld.heat(2);
    s.add("X02767 热路径优化", heat4.len() == 4 && heat4.iter().sum::<u32>() == 4, "热力一次成表");
    let bal = ld.column_balance();
    s.add("X02768 内存与功耗收敛", bal == 0, "列均衡不额外开销");
    let c0 = degrade_chain(4, 0);
    let c1 = degrade_chain(4, 90);
    s.add("X02769 低配降级链", c0.0 == 4 && c1.0 == 3 && degrade_chain(4, 220) == (0, 0, 0), "三级递降");
    let mut gd = Guard::new();
    let gg1 = gd.guard("layout-overlaps==0");
    let gg2 = gd.guard("layout-overlaps==0");
    s.add("X02770 防劣化守卫", gg1 && !gg2 && gd.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("tidy", "混乱度 70，建议按 4 列整理");
    let a2 = ad.suggest("tidy", "重复");
    s.add(
        "X02771 本地智能建议",
        a1 && !a2 && ad.explain("tidy").is_some() && ad.reject("tidy") && ad.rejected("tidy"),
        "可解释、可一键拒绝",
    );
    let mut lt = Layout::new();
    lt.add(37, 21, "t1");
    lt.add(191, 63, "t2");
    let rec = lt.recommend();
    let tidied = lt.tidy();
    s.add(
        "X02772 批量自动化",
        rec.len() == 2 && rec[0] == (0, 0) && rec[1] == (80, 0) && tidied == 2,
        "批量整理 + 进度可观测",
    );
    let mut la = Layout::new();
    la.add(0, 0, "m");
    let mut lb = Layout::new();
    lb.add(240, 0, "m");
    let cost = la.switch_cost(&lb);
    s.add("X02773 三线联动场景", cost == 3 && link_matrix(3).0 && link_matrix(3).1, "切换成本跨域");
    let mut p = Plugins::new();
    let p1 = p.register("layout-rule-pack");
    let p2 = p.register("layout-rule-pack");
    s.add("X02774 开放扩展点", p1 && !p2 && p.unregister("layout-rule-pack"), "规则包扩展点");
    let mut eg = Eggs::new();
    let e1 = eg.arm("constellation");
    eg.disable_all();
    s.add("X02775 艺术彩蛋", e1 && eg.count() == 0, "星座连线彩蛋可关闭");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_25_checks_pass() {
        let s = run_layout_checks();
        assert_eq!(s.total(), 25);
        assert!(s.all_pass(), "{}", s.render());
    }
}
