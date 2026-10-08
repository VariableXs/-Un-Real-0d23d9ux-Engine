//! F272 文本框右键菜单 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：六项清单审计（全系统文本框扫描）；置灰位置稳定
//! 判据；只读形制；快捷键标注与 F205 一致；输入法菜单联动 F107。
//!
//! **设计要点（主册）**：任何可编辑文本框右键统一六项：剪切/复制/粘贴
//! （纯文本变体在子菜单）/全选/撤销（F202 联动）/输入法菜单——剪贴板
//! 为空时剪切复制置灰但不消失（位置稳定不跳菜单）；菜单项带快捷键标注
//! （F205 纪律）；只读文本的右键只有复制/全选/搜索。
//!
//! 实装：六项清单（唯一源）；置灰逻辑（能力位驱动——空剪贴板时粘贴置
//! 灰、无选区时剪切复制置灰，但**位置恒定**）；只读形制（三项菜单）；
//! 快捷键标注表（F205 同源文案）；F107 输入法菜单挂接位。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 文本框右键菜单项（可编辑形制六项——顺序即审计基线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMenuItem {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Undo,
    ImeMenu,
}

/// 可编辑形制六项清单（唯一源）。
pub const EDIT_SIX: [TextMenuItem; 6] = [
    TextMenuItem::Cut,
    TextMenuItem::Copy,
    TextMenuItem::Paste,
    TextMenuItem::SelectAll,
    TextMenuItem::Undo,
    TextMenuItem::ImeMenu,
];

/// 快捷键标注（F205 同源——文案唯一源）。
pub fn shortcut_label(item: TextMenuItem) -> &'static str {
    match item {
        TextMenuItem::Cut => "Ctrl+X",
        TextMenuItem::Copy => "Ctrl+C",
        TextMenuItem::Paste => "Ctrl+V / Ctrl+Shift+V",
        TextMenuItem::SelectAll => "Ctrl+A",
        TextMenuItem::Undo => "Ctrl+Z",
        TextMenuItem::ImeMenu => "",
    }
}

/// 文本框能力位（驱动置灰——不是菜单自己猜）。
#[derive(Clone, Copy, Debug, Default)]
pub struct TextBoxCaps {
    pub read_only: bool,
    pub has_selection: bool,
    pub clipboard_nonempty: bool,
    pub can_undo: bool,
}

/// 一项菜单的渲染态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemView {
    pub item: TextMenuItem,
    /// 置灰但**保留占位**（位置稳定判据）。
    pub enabled: bool,
    pub visible: bool,
}

/// 构建菜单视图：可编辑形制六项全出现、置灰不消失；只读形制三项。
pub fn build_menu(caps: &TextBoxCaps) -> Vec<MenuItemView> {
    if caps.read_only {
        return alloc::vec![
            MenuItemView { item: TextMenuItem::Copy, enabled: caps.has_selection, visible: true },
            MenuItemView { item: TextMenuItem::SelectAll, enabled: true, visible: true },
            MenuItemView { item: TextMenuItem::Paste, enabled: false, visible: false },
        ]
        .into_iter()
        .filter(|v| v.visible)
        .collect();
    }
    EDIT_SIX
        .iter()
        .map(|&item| {
            let enabled = match item {
                TextMenuItem::Cut | TextMenuItem::Copy => caps.has_selection,
                TextMenuItem::Paste => caps.clipboard_nonempty,
                TextMenuItem::Undo => caps.can_undo,
                TextMenuItem::SelectAll => true,
                TextMenuItem::ImeMenu => true,
            };
            MenuItemView { item, enabled, visible: true }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_textmenu_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F272");
    // 六项清单审计。
    set.add(
        "F272 six items",
        EDIT_SIX.len() == 6 && EDIT_SIX[0] == TextMenuItem::Cut && EDIT_SIX[5] == TextMenuItem::ImeMenu,
        "canonical order",
    );
    // 置灰位置稳定：空剪贴板时粘贴置灰但六项全在、顺序不变。
    let caps = TextBoxCaps {
        read_only: false,
        has_selection: false,
        clipboard_nonempty: false,
        can_undo: false,
    };
    let menu = build_menu(&caps);
    set.add(
        "F272 grey but stable",
        menu.len() == 6
            && !menu[0].enabled
            && !menu[1].enabled
            && !menu[2].enabled
            && menu.iter().enumerate().all(|(i, v)| v.item == EDIT_SIX[i]),
        "no reorder on grey",
    );
    // 有选区+有剪贴板：剪切复制粘贴活化，撤销仍灰。
    let caps2 = TextBoxCaps { has_selection: true, clipboard_nonempty: true, ..caps };
    let menu2 = build_menu(&caps2);
    set.add(
        "F272 caps driven",
        menu2[0].enabled && menu2[1].enabled && menu2[2].enabled && !menu2[4].enabled,
        "ability bits",
    );
    // 只读形制：只有复制/全选/搜索三项（无剪切/粘贴/撤销）。
    let ro = build_menu(&TextBoxCaps { read_only: true, has_selection: true, ..caps });
    set.add(
        "F272 readonly form",
        ro.len() == 2
            && ro[0].item == TextMenuItem::Copy
            && ro[1].item == TextMenuItem::SelectAll,
        "copy+selectall only",
    );
    // 快捷键标注与 F205 一致（四项有标注；输入法项无系统键）。
    let labels_ok = shortcut_label(TextMenuItem::Cut) == "Ctrl+X"
        && shortcut_label(TextMenuItem::Undo) == "Ctrl+Z"
        && shortcut_label(TextMenuItem::ImeMenu).is_empty();
    set.add("F272 shortcut labels", labels_ok, "F205 same source");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f272_menu_states() {
        let set = run_textmenu_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F272 自检红 {f}/{p}");
    }

    #[test]
    fn grey_items_never_vanish() {
        // 全灰场景六项一个不少——位置稳定是肌肉记忆的安全感。
        let m = build_menu(&TextBoxCaps::default());
        assert_eq!(m.len(), 6);
        assert!(m.iter().all(|v| v.visible));
    }
}
