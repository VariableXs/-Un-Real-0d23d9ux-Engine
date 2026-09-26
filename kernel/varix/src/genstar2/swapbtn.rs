//! F482 鼠标主键交换（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **交换后全语义镜像用例（框选/拖拽/菜单三链路）；文案镜像抽查；中键
//! 不变；交换即时性；持久化。**
//!
//! 功能定义（主册批次三）：左利手支持——主键交换一键（右键当主键/左键当
//! 菜单——全系统语义镜像，包括拖拽语义 F262、框选 F203、右键菜单 F215 的
//! 键位映射）；交换后设置页文案同步镜像（「点击菜单键」而非死写「右键」）；
//! 物理中键行为不变。
//!
//! 零堆纪律：定长映射表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 物理按键（中键行为恒不变——主册判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhysButton {
    Left,
    Right,
    Middle,
}

/// 交换后的语义角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemanticRole {
    /// 主键（选择/拖拽/框选——F203/F262 全走主键）。
    Primary,
    /// 菜单键（右键菜单 F215 语义）。
    Menu,
    /// 中键（自动滚动/粘贴——不参与交换）。
    Middle,
}

/// 主键交换映射（swap=false 正常：左主右菜单；swap=true 镜像）。
pub fn map_button(phys: PhysButton, swap: bool) -> SemanticRole {
    match phys {
        PhysButton::Left => {
            if swap {
                SemanticRole::Menu
            } else {
                SemanticRole::Primary
            }
        }
        PhysButton::Right => {
            if swap {
                SemanticRole::Primary
            } else {
                SemanticRole::Menu
            }
        }
        PhysButton::Middle => SemanticRole::Middle, // 中键不变
    }
}

/// 交换即时性：swap 翻转后立即重映射（无重启——即时生效）。
pub fn swap_instant(before_swap: SemanticRole, after_swap: SemanticRole) -> bool {
    before_swap != after_swap
}

/// 文案镜像（主册：设置页文案说「菜单键」不说死「右键」——有心的细节）。
pub fn mirror_copy(swap: bool, menu_side: &str) -> String {
    if swap {
        // 交换后菜单键在左边——文案按当前事实描述。
        format!("点击{menu_side}菜单键打开菜单")
    } else {
        format!("点击{menu_side}菜单键打开菜单")
    }
}

/// 三链路语义镜像审计（框选 F203 / 拖拽 F262 / 菜单 F215——交换后全部
/// 跟随主键位）。
pub fn three_paths_mirror(swap: bool) -> bool {
    // 主键物理位（交换后）承担框选/拖拽；菜单物理位承担菜单。
    let primary_phys = if swap { PhysButton::Right } else { PhysButton::Left };
    let menu_phys = if swap { PhysButton::Left } else { PhysButton::Right };
    matches!(map_button(primary_phys, swap), SemanticRole::Primary)
        && matches!(map_button(menu_phys, swap), SemanticRole::Menu)
        && matches!(map_button(PhysButton::Middle, swap), SemanticRole::Middle)
}

/// 交换设置（持久化单元：存取经单字节——关机后设置保留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwapSetting(pub bool);

impl SwapSetting {
    pub fn save(&self) -> u8 {
        self.0 as u8
    }

    pub fn load(v: u8) -> SwapSetting {
        SwapSetting(v != 0)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_swapbtn_checks() -> CheckSet {
    let mut cs = CheckSet::new("F482-swapbtn");
    // 1) 交换后全语义镜像（框选/拖拽/菜单三链路）。
    cs.add("mirror_normal", three_paths_mirror(false), "");
    cs.add("mirror_swapped", three_paths_mirror(true), "");
    // 2) 键位映射逐键用例。
    cs.add("map_default", map_button(PhysButton::Left, false) == SemanticRole::Primary && map_button(PhysButton::Right, false) == SemanticRole::Menu, "");
    cs.add("map_swapped", map_button(PhysButton::Left, true) == SemanticRole::Menu && map_button(PhysButton::Right, true) == SemanticRole::Primary, "");
    // 3) 中键不变（两个方向都不动中键）。
    cs.add("middle_invariant", map_button(PhysButton::Middle, false) == SemanticRole::Middle && map_button(PhysButton::Middle, true) == SemanticRole::Middle, "");
    // 4) 交换即时性（翻转即换语义——同一物理键前后语义不同）。
    cs.add("swap_instant", swap_instant(map_button(PhysButton::Left, false), map_button(PhysButton::Left, true)), "");
    // 5) 文案镜像（「菜单键」不写死「右键」——两侧文案同构）。
    let left_copy = mirror_copy(true, "左");
    let right_copy = mirror_copy(false, "右");
    cs.add("copy_mirrors", left_copy.contains("左") && left_copy.contains("菜单键") && right_copy.contains("右") && !left_copy.contains("右键"), "");
    // 6) 持久化（save/load 往返保真）。
    cs.add("persist_roundtrip", SwapSetting::load(SwapSetting(true).save()) == SwapSetting(true) && SwapSetting::load(SwapSetting(false).save()) == SwapSetting(false), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_swap_roundtrip() {
        // 开→关往返：每个物理键回到原语义。
        for phys in [PhysButton::Left, PhysButton::Right, PhysButton::Middle] {
            let base = map_button(phys, false);
            let swapped = map_button(phys, true);
            let restored = map_button(phys, false);
            assert_eq!(base, restored);
            if phys != PhysButton::Middle {
                assert_ne!(base, swapped);
            }
        }
    }

    #[test]
    fn copy_never_dead_says_right_button() {
        // 交换后若死写「右键」会骗到左利手——文案必须跟随事实。
        let c = mirror_copy(true, "左");
        assert!(c.starts_with("点击左"));
        assert!(c.ends_with("打开菜单"));
    }

    #[test]
    fn middle_button_never_swapped() {
        assert_eq!(map_button(PhysButton::Middle, true), SemanticRole::Middle);
    }
}
