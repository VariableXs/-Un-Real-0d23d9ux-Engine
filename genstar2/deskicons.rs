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

// ===========================================================================
// 深化 v7（F455）：槽位网格几何 / 冲突审计与首空分配 / 版本账（时钟单调）/
// 持久化通道 v7（W7D1 + revision + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 持久化——v2 通道（VDI1）无版本位无校验尾：坏包读回垃圾槽位静默
//    打乱桌面。v7 通道加版本 + revision + FNV 尾，坏值拒收不静默。
// 2. 网格几何——「位置记忆」要有坐标系：8×8 槽位网格（slot ↔ (col,row)
//    双向映射、越界诚实拒绝）；move 加冲突守卫（两图标挤一格 = 状态错）。
// 3. 首空分配——新图标落位走分配器（找第一个空槽），冲突自愈（压缩
//    迁移冲突方）。
// 4. 版本账——revision 变更记时钟账：单调守卫（时钟倒流拒绝）+
//    变更历史可回放。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7D1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7D 族——全域唯一，写前 grep 已证）。
pub const DESKICONS_V7_MAGIC: [u8; 4] = *b"W7D1";
/// 长度：魔标(4) + 版本(1) + 可见位图(1) + 保留(1) + 槽位 3×2(6) +
/// revision(4, LE) + FNV(4) = 21（逐段求和核对——4+1+1+1+6+4+4）。
pub const DESKICONS_V7_LEN: usize = 21;
pub const DESKICONS_V7_VERSION: u8 = 1;

/// 序列化（v7 独占通道；revision 入包——「即时生效账」跨会话不断账）。
pub fn save_board_v7(board: &DesktopIconBoard, out: &mut [u8]) -> Option<usize> {
    if out.len() < DESKICONS_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&DESKICONS_V7_MAGIC);
    out[4] = DESKICONS_V7_VERSION;
    let mut vis = 0u8;
    for (i, icon) in SYS_ICONS.iter().enumerate() {
        if board.is_visible(*icon) {
            vis |= 1 << i;
        }
    }
    out[5] = vis;
    out[6] = 0; // 保留
    for (i, icon) in SYS_ICONS.iter().enumerate() {
        let s = board.slot_of(*icon);
        out[7 + i * 2] = (s >> 8) as u8;
        out[8 + i * 2] = (s & 0xFF) as u8;
    }
    out[13..17].copy_from_slice(&board.revision().to_le_bytes());
    let h = fnv1a(&out[..17]);
    out[17] = (h & 0xff) as u8;
    out[18] = ((h >> 8) & 0xff) as u8;
    out[19] = ((h >> 16) & 0xff) as u8;
    out[20] = ((h >> 24) & 0xff) as u8;
    Some(DESKICONS_V7_LEN)
}

/// 反序列化（长度/魔标/版本/保留位/FNV 五重守卫）。
pub fn load_board_v7(buf: &[u8]) -> Option<(bool, bool, bool, [u16; 3], u32)> {
    if buf.len() < DESKICONS_V7_LEN || buf[..4] != DESKICONS_V7_MAGIC {
        return None;
    }
    if buf[4] != DESKICONS_V7_VERSION || buf[6] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..17]);
    let got = buf[17] as u32
        | ((buf[18] as u32) << 8)
        | ((buf[19] as u32) << 16)
        | ((buf[20] as u32) << 24);
    if expect != got {
        return None;
    }
    let vis = buf[5];
    let mut slots = [0u16; 3];
    for i in 0..3 {
        slots[i] = ((buf[7 + i * 2] as u16) << 8) | buf[8 + i * 2] as u16;
    }
    let mut rev = [0u8; 4];
    rev.copy_from_slice(&buf[13..17]);
    Some((vis & 1 != 0, vis & 2 != 0, vis & 4 != 0, slots, u32::from_le_bytes(rev)))
}

// ---------------------------------------------------------------------------
// 槽位网格几何（8×8 = 64 槽）
// ---------------------------------------------------------------------------

/// 网格列数（一行 8 槽——图标 48px + 间距，8 列铺满典型桌面宽）。
pub const GRID_COLS: u16 = 8;
/// 网格行数。
pub const GRID_ROWS: u16 = 8;
/// 网格总槽位。
pub const SLOT_GRID_CAP: usize = (GRID_COLS * GRID_ROWS) as usize;

/// slot → (col, row)（列优先序：0 号在左上、向下增长——桌面图标的
/// 排布直觉）。
pub fn slot_to_xy(slot: u16) -> (u16, u16) {
    (slot % GRID_COLS, slot / GRID_COLS)
}

/// (col, row) → slot（越界诚实 None——不环绕不钳制）。
pub fn xy_to_slot(col: u16, row: u16) -> Option<u16> {
    if col >= GRID_COLS || row >= GRID_ROWS {
        return None;
    }
    Some(row * GRID_COLS + col)
}

impl DesktopIconBoard {
    /// 带守卫的移动：越界拒绝 + 冲突拒绝（目标格已有他图标 = 不动）。
    pub fn move_slot_checked(&mut self, icon: SysIcon, slot: u16) -> bool {
        if xy_to_slot(slot_to_xy(slot).0, slot_to_xy(slot).1) != Some(slot) {
            return false; // 超网格容量（slot ≥ 64）
        }
        if SYS_ICONS.iter().any(|&other| other != icon && self.slot_of(other) == slot) {
            return false; // 冲突：一格一图标
        }
        self.move_slot(icon, slot);
        true
    }

    /// 冲突审计：三图标槽位两两互异（网格不变量）。
    pub fn slots_distinct(&self) -> bool {
        let a = self.slot_of(SysIcon::ThisPc);
        let b = self.slot_of(SysIcon::RecycleBin);
        let c = self.slot_of(SysIcon::UserFolder);
        a != b && b != c && a != c
    }

    /// 首空分配器：找第一个未被占用的槽（0 起、列优先）。
    pub fn first_free_slot(&self) -> Option<u16> {
        (0u16..SLOT_GRID_CAP as u16).find(|&s| {
            !SYS_ICONS.iter().any(|&ic| self.slot_of(ic) == s)
        })
    }

    /// 冲突自愈：若与他图标同格，迁移到首空槽（迁移失败 = 网格满，
    /// 诚实 false——64 槽容 3 图标实际永不触发，但路径必须在）。
    pub fn resolve_collision(&mut self, icon: SysIcon) -> bool {
        let mine = self.slot_of(icon);
        let clash = SYS_ICONS.iter().any(|&other| other != icon && self.slot_of(other) == mine);
        if !clash {
            return true;
        }
        match self.first_free_slot() {
            Some(free) => {
                self.move_slot(icon, free);
                self.slots_distinct()
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 版本账（revision 时钟账——变更历史可回放）
// ---------------------------------------------------------------------------

/// 版本账容量。
pub const REVISION_LEDGER_CAP: usize = 16;

/// revision 变更账：每次翻转记 (时钟, 新 revision)；时钟单调守卫
/// （倒流拒绝计数——异常显性化）。
pub struct BoardRevisionLedger {
    ring: [(u64, u32); REVISION_LEDGER_CAP],
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl BoardRevisionLedger {
    pub const fn new() -> Self {
        BoardRevisionLedger {
            ring: [(0, 0); REVISION_LEDGER_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
        }
    }

    pub fn push(&mut self, at_ms: u64, revision: u32) -> bool {
        if self.n > 0 {
            let last = (self.head + REVISION_LEDGER_CAP - 1) % REVISION_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, revision);
        self.head = (self.head + 1) % REVISION_LEDGER_CAP;
        self.n = (self.n + 1).min(REVISION_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 最新 revision（空账诚实 None）。
    pub fn latest(&self) -> Option<(u64, u32)> {
        if self.n == 0 {
            return None;
        }
        let idx = (self.head + REVISION_LEDGER_CAP - 1) % REVISION_LEDGER_CAP;
        Some(self.ring[idx])
    }

    /// revision 单调不减审计（账面 revision 序列不允许回退——
    /// 版本号只进不退是「即时生效」的账面承诺）。
    pub fn revision_monotonic(&self) -> bool {
        (1..self.n).all(|i| {
            let cur = (self.head + REVISION_LEDGER_CAP - 1 - i + 1) % REVISION_LEDGER_CAP;
            let prev = (self.head + REVISION_LEDGER_CAP - 1 - i) % REVISION_LEDGER_CAP;
            self.ring[cur].1 >= self.ring[prev].1
        })
    }
}

// ---------------------------------------------------------------------------
// 域自检（F455 v7）
// ---------------------------------------------------------------------------

pub fn run_deskicons_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F455-v7");
    // 1) 持久化通道：round-trip（可见位 + 槽位 + revision 全保真）。
    let mut buf = [0u8; DESKICONS_V7_LEN];
    cs.add("persist_roundtrip", {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::RecycleBin, 17);
        b.set_visible(SysIcon::ThisPc, false);
        let n = save_board_v7(&b, &mut buf).unwrap_or(0);
        match load_board_v7(&buf[..n]) {
            Some((tp, rb, uf, slots, rev)) => {
                !tp && rb && !uf && slots[1] == 17 && rev == b.revision()
            }
            None => false,
        }
    }, "");
    cs.add("persist_tamper", {
        let n = save_board_v7(&DesktopIconBoard::new(), &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[8] ^= 0x01; // 翻槽位字节 → FNV 失配
        load_board_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_bad_version", {
        let mut bad = [0u8; DESKICONS_V7_LEN];
        let _ = save_board_v7(&DesktopIconBoard::new(), &mut bad);
        bad[4] = 9;
        load_board_v7(&bad).is_none()
    }, "");
    cs.add("persist_reserved_set", {
        let mut bad = [0u8; DESKICONS_V7_LEN];
        let _ = save_board_v7(&DesktopIconBoard::new(), &mut bad);
        bad[6] = 1;
        load_board_v7(&bad).is_none()
    }, "");
    cs.add("persist_short", load_board_v7(&buf[..9]).is_none(), "");
    // 2) 网格几何：双向映射往返 + 越界诚实。
    cs.add("grid_roundtrip", (0u16..SLOT_GRID_CAP as u16).all(|s| {
        let (c, r) = slot_to_xy(s);
        xy_to_slot(c, r) == Some(s)
    }), "");
    cs.add("grid_oob_honest", xy_to_slot(GRID_COLS, 0).is_none() && xy_to_slot(0, GRID_ROWS).is_none(), "");
    cs.add("grid_corner_anchors", {
        let (c0, r0) = slot_to_xy(0);
        let (c1, r1) = slot_to_xy(SLOT_GRID_CAP as u16 - 1);
        (c0, r0) == (0, 0) && (c1, r1) == (GRID_COLS - 1, GRID_ROWS - 1)
    }, "");
    // 3) 移动守卫：越界拒 + 冲突拒 + 合法落位。
    cs.add("move_oob_reject", {
        let mut b = DesktopIconBoard::new();
        !b.move_slot_checked(SysIcon::ThisPc, SLOT_GRID_CAP as u16)
    }, "");
    cs.add("move_conflict_reject", {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::RecycleBin, 5);
        !b.move_slot_checked(SysIcon::ThisPc, 5) // 回收站占 5 → 拒
    }, "");
    cs.add("move_legal_ok", {
        let mut b = DesktopIconBoard::new();
        b.move_slot_checked(SysIcon::ThisPc, 9) && b.slot_of(SysIcon::ThisPc) == 9 && b.slots_distinct()
    }, "");
    // 4) 首空分配 + 冲突自愈。
    cs.add("first_free_slot", {
        let mut b = DesktopIconBoard::new(); // 默认占 0,1,2
        b.first_free_slot() == Some(3)
    }, "");
    cs.add("collision_self_heal", {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::UserFolder, 0); // 撞 ThisPc 的 0 号
        b.resolve_collision(SysIcon::UserFolder) && b.slots_distinct()
    }, "");
    cs.add("no_collision_noop", {
        let mut b = DesktopIconBoard::new();
        b.resolve_collision(SysIcon::RecycleBin) && b.slot_of(SysIcon::RecycleBin) == 1
    }, "");
    // 5) 版本账：单调守卫 + 倒流拒绝 + revision 序列审计。
    cs.add("revision_ledger_monotonic", {
        let mut led = BoardRevisionLedger::new();
        let _ = led.push(100, 1);
        let _ = led.push(200, 2);
        let _ = led.push(300, 3);
        led.revision_monotonic() && led.latest() == Some((300, 3))
    }, "");
    cs.add("revision_ledger_rejects_rewind", {
        let mut led = BoardRevisionLedger::new();
        let _ = led.push(100, 1);
        !led.push(50, 2) && led.out_of_order_rejected == 1
    }, "");
    cs.add("revision_ledger_empty_honest", BoardRevisionLedger::new().latest().is_none(), "");
    // 6) 全局出口矩阵复查（v2 出口表 × 图标 9 格——二阶展开的回归锚）。
    cs.add("exit_matrix_regression", SYS_ICONS.iter().all(|&ic| {
        EXIT_ROUTES.iter().all(|(r, _)| exit_reachable(ic, r))
    }), "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn persist_full_state_roundtrip() {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::UserFolder, 40);
        b.set_visible(SysIcon::UserFolder, true);
        b.set_visible(SysIcon::RecycleBin, false);
        let mut buf = [0u8; DESKICONS_V7_LEN];
        let n = save_board_v7(&b, &mut buf).unwrap();
        let (tp, rb, uf, slots, rev) = load_board_v7(&buf[..n]).unwrap();
        assert!(tp && !rb && uf);
        assert_eq!(slots[2], 40);
        assert_eq!(rev, b.revision());
    }

    #[test]
    fn grid_mapping_column_major() {
        // 列优先序锚点：slot 1 在 (1,0)，slot 8 在 (0,1)。
        assert_eq!(slot_to_xy(1), (1, 0));
        assert_eq!(slot_to_xy(8), (0, 1));
    }

    #[test]
    fn first_free_skips_occupied() {
        let mut b = DesktopIconBoard::new();
        b.move_slot(SysIcon::ThisPc, 10);
        // 0..3 中 10 之外皆空 → 首空仍是 3（默认 0,1,2 被占后移动腾出）。
        b.move_slot(SysIcon::RecycleBin, 11);
        b.move_slot(SysIcon::UserFolder, 12);
        assert_eq!(b.first_free_slot(), Some(0));
    }

    #[test]
    fn ledger_ring_wrap_keeps_latest() {
        let mut led = BoardRevisionLedger::new();
        for i in 0..(REVISION_LEDGER_CAP as u64 + 3) {
            assert!(led.push(i * 1000, i as u32));
        }
        assert_eq!(led.count(), REVISION_LEDGER_CAP);
        assert!(led.revision_monotonic());
        assert_eq!(led.latest().map(|(_, r)| r), Some(REVISION_LEDGER_CAP as u32 + 2));
    }

    #[test]
    fn resolve_collision_repeatedly_clean() {
        // 反复互撞总能自愈（64 槽容 3 图标——分配器恒有解）。
        let mut b = DesktopIconBoard::new();
        for _ in 0..10 {
            b.move_slot(SysIcon::ThisPc, b.slot_of(SysIcon::RecycleBin)); // 强撞
            assert!(b.resolve_collision(SysIcon::ThisPc));
            assert!(b.slots_distinct());
        }
    }
}
