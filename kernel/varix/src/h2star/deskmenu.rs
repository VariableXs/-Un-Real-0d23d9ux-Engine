//! F258 桌面右键菜单全集 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：六项顺序对照表；扩展注入审计（拦截=有效）；三档
//! 图标大小联动 F084；各子菜单键盘可达。
//!
//! **设计要点（主册）**：桌面空白右键固定六项+扩展项：查看（图标大小
//! 三档/对齐网格 F084）、排序方式（名称/日期/类型）、刷新（F083）、
//! 新建（F259 入口）、显示设置、个性化（跳 E 域主题页）——顺序与
//! Windows 桌面右键同构；第三方 shell 扩展不开放（防右键菜单膨胀，
//! vxapp 不许往里塞项——与 Windows 的关键差异，文档化）。
//!
//! 实装：六项菜单表（顺序即定义——对照表唯一源）；扩展注入审计器
//! （vxapp 申请一律拒绝并留审计账——「拦截=有效」的机制保证）；查看
//! 子菜单三档图标大小联动口；每项带键盘路径标注（可达性判据）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 图标大小三档（F084 联动口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

/// 排序方式三选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Date,
    Type,
}

/// 桌面右键固定六项（顺序即主册对照表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopMenuItem {
    /// 查看（子菜单：三档图标大小 + 对齐网格）。
    View,
    /// 排序方式（名称/日期/类型）。
    SortBy,
    /// 刷新（F083 语义）。
    Refresh,
    /// 新建（F259 入口）。
    New,
    /// 显示设置。
    DisplaySettings,
    /// 个性化（跳 E 域主题页）。
    Personalize,
}

/// 六项顺序对照表（唯一源——渲染层与走查脚本都从这里取）。
pub const DESKTOP_MENU_ORDER: [DesktopMenuItem; 6] = [
    DesktopMenuItem::View,
    DesktopMenuItem::SortBy,
    DesktopMenuItem::Refresh,
    DesktopMenuItem::New,
    DesktopMenuItem::DisplaySettings,
    DesktopMenuItem::Personalize,
];

/// 每项的键盘路径（可达性判据：子菜单方向键进入、Esc 逐级退出）。
pub fn keyboard_path(item: DesktopMenuItem) -> &'static str {
    match item {
        DesktopMenuItem::View => "↓定位→进子菜单↑↓选择 Enter",
        DesktopMenuItem::SortBy => "↓定位→进子菜单↑↓选择 Enter",
        DesktopMenuItem::Refresh => "↓定位 Enter",
        DesktopMenuItem::New => "↓定位→进子菜单↑↓选择 Enter",
        DesktopMenuItem::DisplaySettings => "↓定位 Enter",
        DesktopMenuItem::Personalize => "↓定位 Enter",
    }
}

/// 一条被拦截的扩展注入申请（审计账条目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectAudit {
    pub app: String,
    pub item: String,
    pub at_min: u64,
}

/// 桌面右键服务：菜单状态 + 注入审计。
pub struct DesktopMenu {
    pub icon_size: IconSize,
    pub align_grid: bool,
    pub sort_by: SortBy,
    /// 被拦截的扩展注入申请（审计不可绕过——没有放行路径）。
    pub audits: Vec<InjectAudit>,
}

impl DesktopMenu {
    pub fn new() -> DesktopMenu {
        DesktopMenu {
            icon_size: IconSize::Medium,
            align_grid: true,
            sort_by: SortBy::Name,
            audits: Vec::new(),
        }
    }

    /// 渲染菜单项序列（恒等六项对照表——顺序不因状态改变）。
    pub fn items(&self) -> Vec<DesktopMenuItem> {
        DESKTOP_MENU_ORDER.to_vec()
    }

    /// vxapp 扩展注入申请：一律拒绝并留审计账（「扩展注入审计拦截=有效」）。
    pub fn inject_request(&mut self, app: &str, item: &str, at_min: u64) -> bool {
        self.audits.push(InjectAudit {
            app: String::from(app),
            item: String::from(item),
            at_min,
        });
        false
    }

    /// 查看→图标大小（三档联动 F084：改档即刻反映在桌面网格）。
    pub fn set_icon_size(&mut self, s: IconSize) {
        self.icon_size = s;
    }

    /// 查看→对齐网格开关。
    pub fn toggle_align_grid(&mut self) -> bool {
        self.align_grid = !self.align_grid;
        self.align_grid
    }

    /// 排序方式切换。
    pub fn set_sort(&mut self, s: SortBy) {
        self.sort_by = s;
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_deskmenu_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F258");
    let mut m = DesktopMenu::new();
    // 六项顺序对照表。
    let items = m.items();
    let expect = [
        DesktopMenuItem::View,
        DesktopMenuItem::SortBy,
        DesktopMenuItem::Refresh,
        DesktopMenuItem::New,
        DesktopMenuItem::DisplaySettings,
        DesktopMenuItem::Personalize,
    ];
    set.add(
        "F258 six order",
        items.len() == 6 && items.iter().zip(expect.iter()).all(|(a, b)| a == b),
        "order table",
    );
    // 扩展注入审计：拦截有效、账目留痕。
    let r1 = m.inject_request("某 shady 应用", "推广菜单项", 10);
    let r2 = m.inject_request("另一应用", "右键广告", 11);
    set.add(
        "F258 inject blocked",
        !r1 && !r2 && m.audits.len() == 2 && m.items().len() == 6,
        "block+audit",
    );
    // 三档图标大小联动。
    m.set_icon_size(IconSize::Large);
    m.set_icon_size(IconSize::Small);
    set.add(
        "F258 icon sizes",
        m.icon_size == IconSize::Small,
        "F084 link",
    );
    let toggled = m.toggle_align_grid();
    set.add("F258 grid toggle", !toggled, "align grid");
    m.set_sort(SortBy::Type);
    set.add("F258 sort", m.sort_by == SortBy::Type, "sort options");
    // 键盘可达：六项每项有非空键盘路径。
    let all_kbd = DESKTOP_MENU_ORDER.iter().all(|&i| !keyboard_path(i).is_empty());
    set.add("F258 keyboard reachable", all_kbd, "submenu arrows");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f258_menu_invariants() {
        let set = run_deskmenu_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F258 自检红 {f}/{p}");
    }

    #[test]
    fn audits_cannot_be_cleared_by_injection() {
        // 注入永远返回 false——不存在「注入成功」的输入。
        let mut m = DesktopMenu::new();
        for i in 0..20 {
            assert!(!m.inject_request("app", "item", i));
        }
        assert_eq!(m.items().len(), 6);
    }
}
