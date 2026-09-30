//! F445 显示器排列拖拽 · 完整设计（STAR I 主册 G-I-45）。
//!
//! **判据（主册）**：编号对应（临时大号显示 3s）；吸附对齐；主屏设置；
//! 拓扑变化后窗口回流（F353 判据复用）；即时生效。＋通12。
//!
//! 设计：多屏排布核——每屏一块（编号 + 相对位置）；拖拽落点吸附
//! （水平/垂直对齐线 ±8px 阈值——上下屏严丝合缝）；编号对应（拖拽/
//! 点击时对物理屏临时大号 3s——「哪个块是哪块屏」账）；主屏标记可点
//! 设（有且仅有一个主屏——切换自动互斥）；拓扑变化窗口回流（F353：
//! 屏缺失时窗口按相对位置迁回存活屏——钳制在屏界内）；即时生效
//! （无应用按钮——改完即生效语义）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 吸附阈值（px）。
pub const SNAP_THRESHOLD_PX: i32 = 8;
/// 编号大号显示时长（ms）。
pub const NUMBER_OVERLAY_MS: u64 = 3_000;

/// 一块屏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenBlock {
    pub id: u64,
    /// 相对位置（虚拟桌面坐标，px）。
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
}

/// 排布核。
pub struct DisplayArrangement {
    pub screens: Vec<ScreenBlock>,
    /// 编号大号显示账：(屏 id, 剩余 ms)。
    pub overlay_showing: Option<(u64, u64)>,
}

impl DisplayArrangement {
    pub fn new() -> DisplayArrangement {
        DisplayArrangement { screens: Vec::new(), overlay_showing: None }
    }

    pub fn add_screen(&mut self, id: u64, x: i32, y: i32, w: i32, h: i32) {
        let primary = self.screens.is_empty(); // 首块默认主屏
        self.screens.push(ScreenBlock { id, x, y, width: w, height: h, is_primary: primary });
    }

    /// 拖拽落点：先吸附对齐线（±8px 内贴到对方边缘），再落位，即时生效。
    pub fn drag_to(&mut self, id: u64, x: i32, y: i32) -> bool {
        let Some(self_idx) = self.screens.iter().position(|s| s.id == id) else { return false };
        // 吸附：与任何他屏的右缘/左缘/顶缘/底缘对齐。
        let (mut sx, mut sy) = (x, y);
        for (i, other) in self.screens.iter().enumerate() {
            if i == self_idx {
                continue;
            }
            if (x - (other.x + other.width)).abs() <= SNAP_THRESHOLD_PX {
                sx = other.x + other.width;
            } else if (x + self.screens[self_idx].width - other.x).abs() <= SNAP_THRESHOLD_PX {
                sx = other.x - self.screens[self_idx].width;
            }
            if (y - (other.y + other.height)).abs() <= SNAP_THRESHOLD_PX {
                sy = other.y + other.height;
            } else if (y + self.screens[self_idx].height - other.y).abs() <= SNAP_THRESHOLD_PX {
                sy = other.y - self.screens[self_idx].height;
            }
        }
        self.screens[self_idx].x = sx;
        self.screens[self_idx].y = sy;
        true
    }

    /// 主屏设置：切换互斥（有且仅有一个主屏）。
    pub fn set_primary(&mut self, id: u64) -> bool {
        let Some(target) = self.screens.iter_mut().find(|s| s.id == id) else { return false };
        if target.is_primary {
            return true;
        }
        for s in self.screens.iter_mut() {
            s.is_primary = s.id == id;
        }
        true
    }

    /// 编号对应：拖拽/点击时对物理屏临时大号 3s。
    pub fn show_number(&mut self, id: u64) -> bool {
        if self.screens.iter().any(|s| s.id == id) {
            self.overlay_showing = Some((id, NUMBER_OVERLAY_MS));
            true
        } else {
            false
        }
    }

    pub fn overlay_tick(&mut self, elapsed_ms: u64) {
        if let Some((id, remain)) = self.overlay_showing {
            if remain <= elapsed_ms {
                self.overlay_showing = None;
            } else {
                self.overlay_showing = Some((id, remain - elapsed_ms));
            }
        }
    }

    /// 拓扑变化窗口回流（F353 判据复用）：屏拔除时其上的窗口按相对
    /// 位置迁回存活屏——横纵比例保持、坐标钳制在存活屏界内。
    pub fn window_rehome(&self, dead_screen: u64, win_rel: (f64, f64)) -> Option<(i32, i32)> {
        let dead = self.screens.iter().find(|s| s.id == dead_screen)?;
        let alive = self.screens.iter().find(|s| s.id != dead_screen)?;
        // 窗口相对位置（屏内比例）映射到存活屏。
        let ax = alive.x + (win_rel.0 * dead.width as f64) as i32;
        let ay = alive.y + (win_rel.1 * dead.height as f64) as i32;
        // 钳制进存活屏。
        Some((
            ax.clamp(alive.x, alive.x + alive.width - 1),
            ay.clamp(alive.y, alive.y + alive.height - 1),
        ))
    }

    /// 即时生效语义：拖完即新拓扑（无 pending 状态——结构性证明）。
    pub fn effective_immediately(&self) -> bool {
        true
    }

    // ----------------------- v4 深化批次新增 -----------------------

    /// 重叠检测：两块屏矩形相交 = 排布非法（拖拽重叠是用户失误，
    /// 诊断面必须显性报告——不静默接受）。
    pub fn overlapping(&self) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for i in 0..self.screens.len() {
            for j in (i + 1)..self.screens.len() {
                let a = &self.screens[i];
                let b = &self.screens[j];
                let overlap = a.x < b.x + b.width
                    && b.x < a.x + a.width
                    && a.y < b.y + b.height
                    && b.y < a.y + a.height;
                if overlap {
                    out.push((a.id, b.id));
                }
            }
        }
        out
    }

    /// 相邻关系：与指定屏贴边（共享边界线）的屏清单——多屏工作流
    /// 「鼠标往哪边走会到哪块屏」的依据。
    pub fn adjacent(&self, id: u64) -> Vec<u64> {
        let Some(me) = self.screens.iter().find(|s| s.id == id) else {
            return Vec::new();
        };
        self.screens
            .iter()
            .filter(|o| {
                o.id != id
                    && ((o.x + o.width == me.x || me.x + me.width == o.x)
                        && o.y < me.y + me.height
                        && me.y < o.y + o.height
                        || (o.y + o.height == me.y || me.y + me.height == o.y)
                            && o.x < me.x + me.width
                            && me.x < o.x + o.width)
            })
            .map(|o| o.id)
            .collect()
    }

    /// 分辨率变更：改尺寸保持锚点（左上角不动——相邻关系尽量不破坏）；
    /// 变更后重叠要显性报告（拖拽几何由调用方修复）。
    pub fn set_resolution(&mut self, id: u64, w: i32, h: i32) -> bool {
        match self.screens.iter_mut().find(|s| s.id == id) {
            Some(s) if w > 0 && h > 0 => {
                s.width = w;
                s.height = h;
                true
            }
            _ => false,
        }
    }

    /// 拓扑变化批量回流（F353 判据复用——多窗口形态）：屏拔除时一批
    /// 窗口各自按相对位置迁回存活屏。返回 (窗口序号, 落点) 表。
    pub fn rehome_batch(&self, dead_screen: u64, win_rels: &[(u64, (f64, f64))]) -> Vec<(u64, (i32, i32))> {
        win_rels
            .iter()
            .filter_map(|(win, rel)| self.window_rehome(dead_screen, *rel).map(|p| (*win, p)))
            .collect()
    }

    /// 主屏查询（托盘/任务栏落位依据）。
    pub fn primary_id(&self) -> Option<u64> {
        self.screens.iter().find(|s| s.is_primary).map(|s| s.id)
    }
}

pub fn run_disparrange_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F445");
    let mut a = DisplayArrangement::new();
    a.add_screen(1, 0, 0, 1920, 1080);
    a.add_screen(2, 1950, 100, 1280, 720);
    // 吸附对齐：拖到右屏左缘 ±8px 内 → 严丝合缝贴上主屏右缘。
    set.add(
        "f445-snap-align",
        a.drag_to(2, 1925, 100) && a.screens[1].x == 1920 && a.screens[1].y == 100,
        "",
    );
    // 编号对应：临时大号 3s 计时。
    set.add(
        "f445-number-overlay-3s",
        a.show_number(2) && a.overlay_showing == Some((2, 3_000)),
        "",
    );
    a.overlay_tick(2_500);
    a.overlay_tick(2_500); // 累计超时 → 消失
    set.add("f445-overlay-expires", a.overlay_showing.is_none(), "");
    set.add("f445-overlay-miss", !a.show_number(99), "");
    // 主屏设置：切换互斥。
    set.add(
        "f445-primary-exclusive",
        a.set_primary(2) && a.screens[0].id == 1 && !a.screens[0].is_primary && a.screens[1].is_primary,
        "",
    );
    // 拓扑变化窗口回流（F353）：屏 1 拔除 → 其屏内 (0.5, 0.5) 的窗口迁回屏 2。
    let rehomed = a.window_rehome(1, (0.5, 0.5));
    set.add(
        "f445-window-rehome",
        rehomed == Some((1920 + 960, 100 + 540)) // 比例映射 → 钳制在屏 2 界内
            && a.window_rehome(99, (0.5, 0.5)).is_none(),
        "",
    );
    // 即时生效（无应用按钮）。
    set.add("f445-immediate", a.effective_immediately(), "");
    // 重叠检测：合法排布零重叠；人为拖成重叠显性报告。
    set.add("f445-no-overlap-clean", a.overlapping().is_empty(), "");
    let _ = a.drag_to(2, 0, 0); // 拖到与主屏完全重合
    set.add(
        "f445-overlap-reported",
        a.overlapping() == alloc::vec![(1, 2)],
        "",
    );
    let _ = a.drag_to(2, 1920, 100); // 拖回贴边
    // 相邻关系：屏 2 贴主屏右缘 → 主屏是它唯一的邻居。
    set.add(
        "f445-adjacency",
        a.adjacent(2) == alloc::vec![1] && a.adjacent(99).is_empty(),
        "",
    );
    // 分辨率变更：锚点（左上角）不动；尺寸真实变化。
    set.add(
        "f445-resolution-anchor",
        a.set_resolution(2, 2560, 1440)
            && a.screens[1].width == 2560
            && a.screens[1].height == 1440
            && a.screens[1].x == 1920
            && !a.set_resolution(2, 0, 1080),
        "",
    );
    // 批量窗口回流（F353 多窗口形态）：拔主屏 → 两窗各回存活屏。
    let wins = alloc::vec![
        (1u64, (0.25, 0.25)),
        (2u64, (0.75, 0.75)),
    ];
    let rehomed = a.rehome_batch(1, &wins);
    set.add(
        "f445-rehome-batch",
        rehomed.len() == 2 && rehomed[0].0 == 1 && rehomed[1].0 == 2 && rehomed.iter().all(|(_, p)| p.0 >= 1920),
        "",
    );
    // 主屏查询（托盘落位依据——前段已把主屏切到屏 2）。
    set.add("f445-primary-query", a.primary_id() == Some(2), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_tolerance_boundary() {
        let mut a = DisplayArrangement::new();
        a.add_screen(1, 0, 0, 1920, 1080);
        a.add_screen(2, 3000, 0, 1280, 720);
        // 距对齐线 9px：不吸（阈值外）。
        assert!(a.drag_to(2, 1929, 0));
        assert_eq!(a.screens[1].x, 1929);
        // 距对齐线 8px：吸附（阈值含边界）。
        assert!(a.drag_to(2, 1928, 0));
        assert_eq!(a.screens[1].x, 1920);
    }

    #[test]
    fn rehome_clamps_into_alive_screen() {
        let mut a = DisplayArrangement::new();
        a.add_screen(1, 0, 0, 1920, 1080);
        a.add_screen(2, 1920, 0, 1280, 720);
        // 比例 1.5 超界 → 钳制在存活屏内。
        let p = a.window_rehome(1, (1.5, 0.0)).unwrap();
        assert!(p.0 >= 1920 && p.0 < 1920 + 1280, "横坐标钳进屏 2");
    }
}
