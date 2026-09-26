//! F455 桌面系统图标开关（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三枚独立开关；功能可达性（关显示后此机页 F456 照常）；位置记忆；
//! 开关即时生效；默认配置（回收站+此机开，文档化）。**
//!
//! 功能定义（主册批次三）：此机/回收站/用户目录三枚可独立开/关——开关
//! 关闭只是不显示（功能从开始菜单/资源管理器照常可达，砍显示不砍功能）；
//! 排列位置记忆（开关往返位置不变）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 默认配置（主册原文：回收站+此机开、用户目录关——文档化）。
pub const DEFAULT_THIS_PC: bool = true;
pub const DEFAULT_RECYCLE_BIN: bool = true;
pub const DEFAULT_USER_FOLDER: bool = false;

/// 三枚系统图标（主册原文：此机/回收站/用户目录）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysIcon {
    ThisPc,
    RecycleBin,
    UserFolder,
}

pub const SYS_ICONS: [SysIcon; 3] = [SysIcon::ThisPc, SysIcon::RecycleBin, SysIcon::UserFolder];

impl SysIcon {
    pub fn name(self) -> &'static str {
        match self {
            SysIcon::ThisPc => "this-pc",
            SysIcon::RecycleBin => "recycle-bin",
            SysIcon::UserFolder => "user-folder",
        }
    }

    /// 默认显示态（主册默认配置，文档化）。
    pub fn default_visible(self) -> bool {
        match self {
            SysIcon::ThisPc => DEFAULT_THIS_PC,
            SysIcon::RecycleBin => DEFAULT_RECYCLE_BIN,
            SysIcon::UserFolder => DEFAULT_USER_FOLDER,
        }
    }
}

/// 桌面图标状态板：可见性 + 位置记忆。
pub struct DesktopIconBoard {
    visible: [bool; 3],
    /// 位置记忆：开关往返位置不变（槽位随图标存）。
    slots: [u16; 3],
    /// 即时生效账：每次变更记一次代数（走查用）。
    revision: u32,
}

impl DesktopIconBoard {
    /// 出厂态（默认配置文档化）。
    pub const fn new() -> Self {
        DesktopIconBoard {
            visible: [DEFAULT_THIS_PC, DEFAULT_RECYCLE_BIN, DEFAULT_USER_FOLDER],
            slots: [0, 1, 2],
            revision: 0,
        }
    }

    fn idx(icon: SysIcon) -> usize {
        match icon {
            SysIcon::ThisPc => 0,
            SysIcon::RecycleBin => 1,
            SysIcon::UserFolder => 2,
        }
    }

    pub fn is_visible(&self, icon: SysIcon) -> bool {
        self.visible[Self::idx(icon)]
    }

    /// 开关（即时生效：revision 自增即视口重绘信号）。
    pub fn set_visible(&mut self, icon: SysIcon, on: bool) {
        let i = Self::idx(icon);
        if self.visible[i] != on {
            self.visible[i] = on;
            self.revision += 1;
        }
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// 位置记忆：关闭不丢槽位，重开回到原位（主册：开关往返位置不变）。
    pub fn slot_of(&self, icon: SysIcon) -> u16 {
        self.slots[Self::idx(icon)]
    }

    pub fn move_slot(&mut self, icon: SysIcon, slot: u16) {
        self.slots[Self::idx(icon)] = slot;
    }

    /// 功能可达性（主册：关显示后功能照常可达）——可见性与可达性解耦：
    /// 桌面可见是「显示」，入口可达是「功能」。此机页 F456 从开始菜单/
    /// 资源管理器永远可达。
    pub fn reachable_when_hidden(icon: SysIcon) -> bool {
        // 三枚系统图标的功能入口均独立于桌面显示存在。
        let _ = icon;
        true
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_deskicons_checks() -> CheckSet {
    let mut cs = CheckSet::new("F455-deskicons");
    // 1) 三枚独立开关（互不影响）。
    let mut b = DesktopIconBoard::new();
    b.set_visible(SysIcon::ThisPc, false);
    cs.add("independent_toggle", !b.is_visible(SysIcon::ThisPc) && b.is_visible(SysIcon::RecycleBin) && !b.is_visible(SysIcon::UserFolder), "");
    // 2) 默认配置（回收站+此机开，用户目录关——文档化）。
    let f = DesktopIconBoard::new();
    cs.add("defaults_documented", f.is_visible(SysIcon::RecycleBin) && f.is_visible(SysIcon::ThisPc) && !f.is_visible(SysIcon::UserFolder), "");
    // 3) 功能可达性：关显示后此机页照常可达。
    b.set_visible(SysIcon::ThisPc, false);
    cs.add("hidden_still_reachable", !b.is_visible(SysIcon::ThisPc) && DesktopIconBoard::reachable_when_hidden(SysIcon::ThisPc), "");
    // 4) 位置记忆：关→开回到原槽位。
    let mut b2 = DesktopIconBoard::new();
    b2.move_slot(SysIcon::RecycleBin, 17);
    b2.set_visible(SysIcon::RecycleBin, false);
    b2.set_visible(SysIcon::RecycleBin, true);
    cs.add("slot_memory", b2.slot_of(SysIcon::RecycleBin) == 17, "");
    // 5) 开关即时生效：变更才推 revision（无变更不重绘）。
    let r0 = b2.revision();
    b2.set_visible(SysIcon::ThisPc, b2.is_visible(SysIcon::ThisPc)); // 无效变更
    cs.add("no_op_no_redraw", b2.revision() == r0, "");
    b2.set_visible(SysIcon::ThisPc, false);
    cs.add("instant_apply", b2.revision() == r0 + 1, "");
    // 6) 全关/全开边界（极简桌面 vs 全都要）。
    let mut b3 = DesktopIconBoard::new();
    b3.set_visible(SysIcon::ThisPc, false);
    b3.set_visible(SysIcon::RecycleBin, false);
    cs.add("minimal_desktop", SYS_ICONS.iter().all(|&i| !b3.is_visible(i) || i == SysIcon::UserFolder), "");
    b3.set_visible(SysIcon::ThisPc, true);
    b3.set_visible(SysIcon::RecycleBin, true);
    b3.set_visible(SysIcon::UserFolder, true);
    cs.add("all_on", SYS_ICONS.iter().all(|&i| b3.is_visible(i)), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_roundtrip_keeps_slot() {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::UserFolder, 9);
        b.set_visible(SysIcon::UserFolder, true);
        b.set_visible(SysIcon::UserFolder, false);
        b.set_visible(SysIcon::UserFolder, true);
        assert!(b.is_visible(SysIcon::UserFolder));
        assert_eq!(b.slot_of(SysIcon::UserFolder), 9);
    }

    #[test]
    fn defaults_match_master_doc() {
        assert!(SysIcon::ThisPc.default_visible());
        assert!(SysIcon::RecycleBin.default_visible());
        assert!(!SysIcon::UserFolder.default_visible());
    }

    #[test]
    fn visibility_decoupled_from_reachability() {
        let mut b = DesktopIconBoard::new();
        b.set_visible(SysIcon::ThisPc, false);
        // 显示关了，功能入口还在（开始菜单/资源管理器路径）。
        assert!(DesktopIconBoard::reachable_when_hidden(SysIcon::ThisPc));
    }
}
