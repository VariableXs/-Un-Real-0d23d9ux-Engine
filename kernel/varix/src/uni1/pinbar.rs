//! F418 任务栏固定应用 · 完整设计（STAR I 主册 G-I-18）。
//!
//! **判据（主册）**：三固定两取消路径；运行中语义；拖拽排序与持久化；
//! 图标常驻渲染；固定上限（合理值 20 提示）。＋通12。
//!
//! 设计：任务栏钉选语义核——固定三路（磁贴右键/运行中右键/拖入）与
//! 取消两路（右键取消/拖出删除）同表；运行中取消固定不关窗（钉选态与
//! 进程态解耦的双标记模型）；拖拽排序持久化；图标常驻（没开也显示）；
//! 上限 20（满额提示记账）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 固定上限（满额提示）。
pub const PIN_CAP: usize = 20;

/// 一枚任务栏图标：钉选态与运行态解耦。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarIcon {
    pub app: &'static str,
    pub pinned: bool,
    pub running: bool,
}

/// 任务栏钉选核。
pub struct PinBar {
    icons: Vec<BarIcon>,
    /// 满额提示次数。
    pub cap_warnings: u64,
}

impl PinBar {
    pub fn new() -> PinBar {
        PinBar { icons: Vec::new(), cap_warnings: 0 }
    }

    fn find(&self, app: &str) -> Option<usize> {
        self.icons.iter().position(|i| i.app == app)
    }

    /// 固定（三路统一口；source 仅记账语义）。上限 20：满额拒绝 + 提示计数。
    pub fn pin(&mut self, app: &'static str, from_running: bool) -> bool {
        if let Some(p) = self.find(app) {
            self.icons[p].pinned = true;
            self.icons[p].running = self.icons[p].running || from_running;
            return true;
        }
        if self.icons.iter().filter(|i| i.pinned).count() >= PIN_CAP {
            self.cap_warnings += 1;
            return false;
        }
        self.icons.push(BarIcon { app, pinned: true, running: from_running });
        true
    }

    /// 运行中应用出现在任务栏（未钉选 → 临时图标）。
    pub fn set_running(&mut self, app: &'static str, running: bool) {
        match self.find(app) {
            Some(p) => {
                self.icons[p].running = running;
                // 运行结束且未钉选 → 临时图标退场（不常驻）。
                if !running && !self.icons[p].pinned {
                    self.icons.remove(p);
                }
            }
            None if running => {
                self.icons.push(BarIcon { app, pinned: false, running: true });
            }
            None => {}
        }
    }

    /// 取消固定（两路同口）：运行中取消固定不关窗（图标仍在，转临时）。
    pub fn unpin(&mut self, app: &str) -> bool {
        match self.find(app) {
            Some(p) if self.icons[p].pinned => {
                self.icons[p].pinned = false;
                if !self.icons[p].running {
                    self.icons.remove(p);
                }
                true
            }
            _ => false,
        }
    }

    /// 拖拽排序（钉选图标之间换位；持久化快照即 icons 序）。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.icons.len() || to >= self.icons.len() || from == to {
            return false;
        }
        let it = self.icons.remove(from);
        self.icons.insert(to, it);
        true
    }

    /// 图标常驻判定：钉选态图标在应用未运行时也必须渲染。
    pub fn resident_icons(&self) -> Vec<&'static str> {
        self.icons.iter().filter(|i| i.pinned).map(|i| i.app).collect()
    }

    /// 持久化快照（重启恢复序）。
    pub fn snapshot(&self) -> Vec<(&'static str, bool, bool)> {
        self.icons.iter().map(|i| (i.app, i.pinned, i.running)).collect()
    }

    pub fn icon_count(&self) -> usize {
        self.icons.len()
    }

    pub fn icon(&self, app: &str) -> Option<BarIcon> {
        self.icons.iter().find(|i| i.app == app).copied()
    }
}

pub fn run_pinbar_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F418");
    let mut b = PinBar::new();
    // 固定三路（磁贴右键/运行中右键/拖入——统一口，运行态分别注入）。
    set.add("f418-pin-from-tile", b.pin("终端", false), "");
    b.set_running("编辑器", true);
    set.add("f418-pin-from-running", b.pin("编辑器", true), "");
    set.add("f418-pin-from-drag", b.pin("画图", false), "");
    // 图标常驻：未运行也显示。
    set.add(
        "f418-resident",
        b.resident_icons() == alloc::vec!["终端", "编辑器", "画图"],
        "",
    );
    // 运行中语义：取消固定不关窗（图标转临时）。
    set.add("f418-unpin-running-keeps", b.unpin("编辑器") && b.icon("编辑器").map(|i| i.running).unwrap_or(false) && !b.icon("编辑器").map(|i| i.pinned).unwrap_or(true), "");
    // 未运行取消固定 → 退场。
    set.add("f418-unpin-idle-gone", b.unpin("画图") && b.icon("画图").is_none(), "");
    // 拖拽排序 + 持久化快照。
    b.pin("浏览器", false);
    b.pin("音乐", false);
    set.add(
        "f418-reorder",
        b.reorder(0, 2) && b.snapshot()[2].0 == "终端" && b.snapshot()[0].0 == "编辑器",
        "",
    );
    let snap = b.snapshot();
    set.add(
        "f418-snapshot-shape",
        snap.len() == b.icon_count()
            && snap.iter().any(|(_, p, _)| *p)
            && snap.iter().any(|(_, p, r)| !*p && *r),
        "",
    );
    // 越界拖拽拒绝。
    set.add("f418-reorder-bounds", !b.reorder(0, 99) && !b.reorder(0, 0), "");
    // 上限 20 + 提示记账：直接构造满额（20 枚钉选）。
    let mut full = PinBar::new();
    for i in 0..PIN_CAP {
        full.icons_push_for_test(i);
    }
    set.add("f418-cap-at-limit", !full.pin("溢出者", false) && full.cap_warnings == 1, "");
    set.add("f418-cap-count", full.icon_count() == PIN_CAP, "");
    set
}

/// 测试辅助（仅测试路径使用）。
impl PinBar {
    fn icons_push_for_test(&mut self, i: usize) {
        // 20 枚以内名字唯一：用 pinned-only 图标直接注入（绕过 pin 的查重）。
        let apps = ["a1","a2","a3","a4","a5","a6","a7","a8","a9","a10","a11","a12","a13","a14","a15","a16","a17","a18","a19","a20"];
        if i < apps.len() {
            self.icons.push(BarIcon { app: apps[i], pinned: true, running: false });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_icon_lifecycle() {
        let mut b = PinBar::new();
        b.set_running("临时应用", true);
        assert!(b.icon("临时应用").map(|i| !i.pinned && i.running).unwrap_or(false));
        b.set_running("临时应用", false);
        assert!(b.icon("临时应用").is_none(), "未钉选应用退出后图标退场");
    }

    #[test]
    fn pin_existing_idle_app() {
        let mut b = PinBar::new();
        assert!(b.pin("x", false));
        assert_eq!(b.icon_count(), 1);
        assert!(b.pin("x", false), "重复固定幂等");
        assert_eq!(b.icon_count(), 1);
    }
}
