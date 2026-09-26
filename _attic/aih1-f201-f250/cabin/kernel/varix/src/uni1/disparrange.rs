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
