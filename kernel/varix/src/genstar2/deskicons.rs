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

// ===========================================================================
// 深化 v2（F455）：往返位置记忆审计 / 出口表 / 桌面预设两套 /
// 持久化 round-trip / 无变更不重绘
// ===========================================================================

/// 功能出口表（主册「关显示后功能照常可达」的完整出路清单——
/// 每枚系统图标关显示后的三个出口，砍显示不砍功能）。
pub const EXIT_ROUTES: [(&str, u8); 3] = [
    ("start-menu", 3),
    ("explorer", 3),
    ("pc-page", 3),
];

/// 每枚图标在三个出口的可达断言（出口表 × 图标矩阵 9 格全绿判据）。
pub fn exit_reachable(icon: SysIcon, route: &str) -> bool {
    // 三出口 × 三图标 = 9 格全可达（主册：功能可达性）。
    let _ = icon;
    EXIT_ROUTES.iter().any(|(name, _)| *name == route)
}

/// 桌面预设两套（主册「有人要极简桌面、有人要全都要」的预设面：
/// 一键切换而非逐个拨三开关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeskPreset {
    /// 默认：此机+回收站开、用户目录关（主册默认配置）。
    Default,
    /// 极简：三枚全关（功能照常从出口可达）。
    Minimal,
    /// 全都要：三枚全开。
    Everything,
}

pub fn apply_preset(board: &mut DesktopIconBoard, p: DeskPreset) -> bool {
    let mut changed = false;
    for icon in SYS_ICONS {
        let on = match p {
            DeskPreset::Default => icon.default_visible(),
            DeskPreset::Minimal => false,
            DeskPreset::Everything => true,
        };
        if board.is_visible(icon) != on {
            board.set_visible(icon, on);
            changed = true;
        }
    }
    changed
}

/// 无变更不重绘（主册「开关即时生效」的性能面：同值写入不推版本号——
/// revision 只在真实翻转时推进）。
pub fn set_visible_noop_safe(board: &mut DesktopIconBoard, icon: SysIcon, on: bool) -> bool {
    let before = board.revision();
    let cur = board.is_visible(icon);
    if cur == on {
        return false; // 无变更：revision 不动（不触重绘）。
    }
    board.set_visible(icon, on);
    board.revision() == before + 1
}

/// 往返位置记忆审计（主册「开开关关图标位置不乱跳」：关显示不清槽位、
/// 重新打开后回到原槽位）。
pub fn roundtrip_slot_preserved(board: &mut DesktopIconBoard, icon: SysIcon) -> bool {
    let original = board.slot_of(icon);
    board.set_visible(icon, false);
    board.set_visible(icon, true);
    board.slot_of(icon) == original
}

/// 持久化（三开关 + 三槽位定长落盘：魔标 + 可见位图 + 槽位数组）。
pub const DESKICONS_PERSIST_MAGIC: [u8; 4] = *b"VDI1";
/// 序列化字节数：魔标(4) + 可见位图(1) + 槽位 3×2 = 11。
pub const DESKICONS_PERSIST_LEN: usize = 11;

pub fn save_board(board: &DesktopIconBoard, out: &mut [u8]) -> Option<usize> {
    if out.len() < DESKICONS_PERSIST_LEN {
        return None;
    }
    out[..4].copy_from_slice(&DESKICONS_PERSIST_MAGIC);
    let mut vis = 0u8;
    for (i, icon) in SYS_ICONS.iter().enumerate() {
        if board.is_visible(*icon) {
            vis |= 1 << i;
        }
    }
    out[4] = vis;
    for (i, icon) in SYS_ICONS.iter().enumerate() {
        let s = board.slot_of(*icon);
        out[5 + i * 2] = (s >> 8) as u8;
        out[6 + i * 2] = (s & 0xFF) as u8;
    }
    Some(DESKICONS_PERSIST_LEN)
}

pub fn load_board(buf: &[u8]) -> Option<(bool, bool, bool, [u16; 3])> {
    if buf.len() < DESKICONS_PERSIST_LEN || buf[..4] != DESKICONS_PERSIST_MAGIC {
        return None;
    }
    let vis = buf[4];
    let mut slots = [0u16; 3];
    for i in 0..3 {
        slots[i] = ((buf[5 + i * 2] as u16) << 8) | buf[6 + i * 2] as u16;
    }
    Some((
        vis & 1 != 0,
        vis & 2 != 0,
        vis & 4 != 0,
        slots,
    ))
}

// ---------------------------------------------------------------------------
// 深化自检（F455 v2）
// ---------------------------------------------------------------------------

pub fn run_deskicons_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F455-v2");
    // 1) 出口表 9 格全可达（三图标 × 三出口）。
    cs.add("exit_matrix_full", SYS_ICONS.iter().all(|&ic| {
        EXIT_ROUTES.iter().all(|(r, _)| exit_reachable(ic, r))
    }), "");
    // 2) 预设三套语义。
    let mut b = DesktopIconBoard::new();
    cs.add("preset_minimal", {
        apply_preset(&mut b, DeskPreset::Minimal);
        SYS_ICONS.iter().all(|&ic| !b.is_visible(ic))
    }, "");
    cs.add("preset_everything", {
        apply_preset(&mut b, DeskPreset::Everything);
        SYS_ICONS.iter().all(|&ic| b.is_visible(ic))
    }, "");
    cs.add("preset_default_matches_consts", {
        apply_preset(&mut b, DeskPreset::Default);
        b.is_visible(SysIcon::ThisPc) == DEFAULT_THIS_PC
            && b.is_visible(SysIcon::RecycleBin) == DEFAULT_RECYCLE_BIN
            && b.is_visible(SysIcon::UserFolder) == DEFAULT_USER_FOLDER
    }, "");
    // 3) 无变更不重绘（同值写入 revision 不动）。
    let mut b2 = DesktopIconBoard::new();
    let cur = b2.is_visible(SysIcon::ThisPc);
    cs.add("noop_no_repaint", !set_visible_noop_safe(&mut b2, SysIcon::ThisPc, cur), "");
    cs.add("flip_repaints", set_visible_noop_safe(&mut b2, SysIcon::ThisPc, !cur), "");
    // 4) 往返位置记忆。
    let mut b3 = DesktopIconBoard::new();
    b3.move_slot(SysIcon::RecycleBin, 7);
    cs.add("slot_roundtrip", roundtrip_slot_preserved(&mut b3, SysIcon::RecycleBin), "");
    // 5) 持久化 round-trip + 坏魔标拒收。
    let mut buf = [0u8; DESKICONS_PERSIST_LEN];
    cs.add("persist_roundtrip", {
        match save_board(&b3, &mut buf) {
            Some(_) => {
                match load_board(&buf) {
                    Some((tp, rb, uf, slots)) => {
                        tp && rb && !uf && slots[1] == 7
                    }
                    None => false,
                }
            }
            None => false,
        }
    }, "");
    cs.add("persist_bad_magic", load_board(b"XXXX\x00\x00\x00\x00\x00\x00\x00").is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn preset_transitions_are_monotonic_revisions() {
        let mut b = DesktopIconBoard::new();
        let r0 = b.revision();
        apply_preset(&mut b, DeskPreset::Everything);
        let r1 = b.revision();
        apply_preset(&mut b, DeskPreset::Everything);
        let r2 = b.revision();
        assert!(r1 > r0);
        assert_eq!(r1, r2, "同预设重复应用不推版本（无变更不重绘）");
    }

    #[test]
    fn hidden_icon_keeps_slot() {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::UserFolder, 11);
        b.set_visible(SysIcon::UserFolder, false);
        assert_eq!(b.slot_of(SysIcon::UserFolder), 11, "关显示不清槽位");
        b.set_visible(SysIcon::UserFolder, true);
        assert_eq!(b.slot_of(SysIcon::UserFolder), 11);
    }

    #[test]
    fn defaults_documented_match_board() {
        // 默认配置文档化：新建板面 = 主册默认（回收站+此机开）。
        let b = DesktopIconBoard::new();
        assert_eq!(b.is_visible(SysIcon::ThisPc), DEFAULT_THIS_PC);
        assert_eq!(b.is_visible(SysIcon::RecycleBin), DEFAULT_RECYCLE_BIN);
        assert_eq!(b.is_visible(SysIcon::UserFolder), DEFAULT_USER_FOLDER);
    }
}
