//! F262 拖拽复制/移动语义 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：六种组合语义用例全测；光标图标与实际操作一致性
//! （20 例）；右键拖菜单四项；跨盘判定的盘符边界（S: 共享卷与 VARIX 域）。
//!
//! **设计要点（主册）**：拖文件的潜规则全盘照抄 Windows：同盘拖=移动、
//! 跨盘拖=复制、Ctrl+拖=强制复制、Shift+拖=强制移动、Alt+拖=创建快捷
//! 方式（.lnk F013）、右键拖=松手弹菜单四选一（复制/移动/快捷方式/
//! 取消）——光标图标实时反映当前语义，用户松手前就知道会发生什么。
//!
//! 实装：语义解析器（同/跨盘 + 三修饰键 → 六组合动作）；光标图标映射
//! （动作 → 图标语义码——一致性判据的对照表）；右键拖四选一菜单；盘符
//! 边界规则（`S:` 共享卷与 VARIX 域视作跨盘）。

use crate::checks::CheckSet;

/// 拖拽动作语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragAction {
    Move,
    Copy,
    MakeShortcut,
    /// 右键拖松手弹菜单（四选一）。
    AskMenu,
}

/// 光标图标语义码（与实际操作一致性判据的对照表——图标绘制层按此码
/// 渲染：箭头+加号=复制、箭头=移动、弯箭头=快捷方式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorGlyph {
    Arrow,
    ArrowPlus,
    ArrowCurved,
}

/// 动作 → 光标图标（对照表唯一源——20 例一致性用例的对拍函数）。
pub fn glyph_of(a: DragAction) -> CursorGlyph {
    match a {
        DragAction::Move => CursorGlyph::Arrow,
        DragAction::Copy => CursorGlyph::ArrowPlus,
        DragAction::MakeShortcut => CursorGlyph::ArrowCurved,
        DragAction::AskMenu => CursorGlyph::Arrow,
    }
}

/// 修饰键。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DragMods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// 盘符判定：`C:`、`S:`（共享卷）、VARIX 域（`vx:`）各为不同盘。
/// 提取盘符前缀；无前缀视作当前盘 `vx:`。
pub fn volume_of(path: &str) -> &str {
    let bytes = path.as_bytes();
    // "X:\..." 形（X 为字母）
    if bytes.len() >= 2 && bytes[1] == b':' {
        return &path[..2];
    }
    "vx:"
}

/// 语义解析（主册规则写死，无配置旋钮——潜规则不许翻供）：
/// 1. Alt+拖 → 快捷方式；
/// 2. Ctrl+拖 → 强制复制；Shift+拖 → 强制移动；
/// 3. 右键拖 → AskMenu（修饰键不覆盖右键语义）；
/// 4. 默认：同盘移动、跨盘复制（S: 共享卷与 VARIX 域互为跨盘）。
pub fn resolve_drag(src: &str, dst: &str, mods: DragMods, right_button: bool) -> DragAction {
    if right_button {
        return DragAction::AskMenu;
    }
    if mods.alt {
        return DragAction::MakeShortcut;
    }
    if mods.ctrl {
        return DragAction::Copy;
    }
    if mods.shift {
        return DragAction::Move;
    }
    if volume_of(src) == volume_of(dst) {
        DragAction::Move
    } else {
        DragAction::Copy
    }
}

/// 右键拖松手菜单四选一（判据原文顺序）。
pub const ASK_MENU_ITEMS: [&str; 4] = ["复制到此处", "移动到此处", "创建快捷方式", "取消"];

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_dragsense_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F262");
    let none = DragMods::default();
    // 六组合语义全测。
    let c1 = resolve_drag("vx:/docs/a.txt", "vx:/docs/b.txt", none, false);
    let c2 = resolve_drag("vx:/docs/a.txt", "S:/backup/a.txt", none, false);
    let c3 = resolve_drag("vx:/a", "S:/a", DragMods { ctrl: true, ..Default::default() }, false);
    let c4 = resolve_drag("vx:/a", "vx:/b", DragMods { shift: true, ..Default::default() }, false);
    let c5 = resolve_drag("vx:/a", "vx:/b", DragMods { alt: true, ..Default::default() }, false);
    let c6 = resolve_drag("vx:/a", "vx:/b", none, true);
    set.add(
        "F262 six combos",
        c1 == DragAction::Move
            && c2 == DragAction::Copy
            && c3 == DragAction::Copy
            && c4 == DragAction::Move
            && c5 == DragAction::MakeShortcut
            && c6 == DragAction::AskMenu,
        "same/cross/ctrl/shift/alt/right",
    );
    // 光标图标与实际操作一致性（对照表逐动作对拍）。
    set.add(
        "F262 glyph table",
        glyph_of(DragAction::Move) == CursorGlyph::Arrow
            && glyph_of(DragAction::Copy) == CursorGlyph::ArrowPlus
            && glyph_of(DragAction::MakeShortcut) == CursorGlyph::ArrowCurved,
        "cursor=action",
    );
    // 右键拖菜单四项。
    set.add(
        "F262 ask menu 4",
        ASK_MENU_ITEMS.len() == 4 && ASK_MENU_ITEMS[3] == "取消",
        "copy/move/lnk/cancel",
    );
    // 盘符边界：S: 共享卷与 VARIX 域互为跨盘；S: 内部同盘移动。
    let s_internal = resolve_drag("S:/a", "S:/b", none, false);
    let s_to_vx = resolve_drag("S:/a", "vx:/a", none, false);
    let vx_to_s = resolve_drag("vx:/a", "S:/a", none, false);
    set.add(
        "F262 volume boundary",
        s_internal == DragAction::Move && s_to_vx == DragAction::Copy && vx_to_s == DragAction::Copy,
        "S: vs vx:",
    );
    // 无前缀路径归 VARIX 域。
    set.add(
        "F262 bare path",
        volume_of("docs/a.txt") == "vx:" && volume_of("C:/x") == "C:",
        "default volume",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f262_semantics_pinned() {
        let set = run_dragsense_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F262 自检红 {f}/{p}");
    }

    #[test]
    fn modifiers_priority_order() {
        // Alt > Ctrl > Shift（主册顺序推导：快捷方式语义最特化）。
        let a = resolve_drag("vx:/a", "vx:/b", DragMods { alt: true, ctrl: true, shift: true }, false);
        assert_eq!(a, DragAction::MakeShortcut);
        let c = resolve_drag("vx:/a", "vx:/b", DragMods { ctrl: true, shift: true, alt: false }, false);
        assert_eq!(c, DragAction::Copy);
    }
}
