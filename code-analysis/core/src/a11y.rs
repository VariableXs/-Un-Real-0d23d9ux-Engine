//! AI-07 · 无障碍与边界场景（#506~#515）。
//!
//! 空项目 / 只读 / 离线 / 内存降级 / 多显示器 / 高DPI / 色盲 /
//! 纯键盘 / 屏幕阅读器 / 键盘导航。零 AI：确定性状态分派。

/// #506 空项目欢迎态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    pub show: bool,
    pub guide: &'static str,
    pub examples: usize,
}

/// #506 打开空项目 → 显示引导 + 示例入口。
pub fn welcome(file_count: usize, loc: usize) -> Welcome {
    if file_count == 0 || loc == 0 {
        Welcome {
            show: true,
            guide: "拖入一个项目文件夹，或点下方示例开始",
            examples: 3,
        }
    } else {
        Welcome { show: false, guide: "", examples: 0 }
    }
}

/// #507 只读模式状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOnlyState {
    pub readonly: bool,
    pub badge: &'static str,
    pub hint: &'static str,
}

/// #507 无权限文件自动只读，节点标 🔒。
pub fn perm_state(writable: bool) -> ReadOnlyState {
    if writable {
        ReadOnlyState { readonly: false, badge: "", hint: "可编辑" }
    } else {
        ReadOnlyState { readonly: true, badge: "🔒", hint: "无写权限，已自动只读" }
    }
}

/// #508 功能可用性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Full,
    Grayed,
}

/// #508 离线模式：本地功能正常，远程功能灰显。
pub fn feature_state(network_up: bool, remote: bool) -> Availability {
    if remote && !network_up {
        Availability::Grayed
    } else {
        Availability::Full
    }
}

/// #509 内存降级阶梯（顺序固定：粒子→光晕→动态壁纸→降低LOD）。
pub const DEGRADE_LADDER: [&str; 4] = ["粒子", "光晕", "动态壁纸", "降低LOD"];

/// #509 按内存压力（0~4）返回被关闭的能力。
pub fn degrade_actions(pressure: u8) -> Vec<&'static str> {
    let n = pressure.min(DEGRADE_LADDER.len() as u8) as usize;
    DEGRADE_LADDER[..n].to_vec()
}

/// #510 显示器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Screen {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
}

/// #510 面板拖到副屏：返回下一个显示器 id（多屏时循环）。
pub fn neighbour_screen(current: u32, screens: &[Screen]) -> u32 {
    if screens.is_empty() {
        return current;
    }
    let idx = screens.iter().position(|s| s.id == current).unwrap_or(0);
    screens[(idx + 1) % screens.len()].id
}

/// #510 画布跨屏：画布矩形是否跨越两个及以上显示器。
pub fn spans_screens(canvas_x: i32, canvas_w: i32, screens: &[Screen]) -> bool {
    let hits = screens
        .iter()
        .filter(|s| canvas_x < s.right() && canvas_x + canvas_w > s.x)
        .count();
    hits >= 2
}

/// #511 缩放策略。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub factor: f64,
    /// 像素风必须整数缩放（1x/2x/4x）。
    pub integer: bool,
}

/// #511 高DPI：像素风整数缩放，其余矢量自适应。
pub fn scale_for(style: &str, dpi: f64) -> Scale {
    if style == "pixel" {
        let factor = if dpi < 1.5 {
            1.0
        } else if dpi < 3.0 {
            2.0
        } else {
            4.0
        };
        Scale { factor, integer: true }
    } else {
        Scale { factor: dpi, integer: false }
    }
}

/// #512 色盲模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlindMode {
    /// 红绿盲（deuteranopia/protanopia）。
    RedGreen,
    /// 蓝黄盲（tritanopia）。
    BlueYellow,
    /// 全色盲（achromatopsia）。
    Achromatopsia,
}

/// #512 用形状 + 纹理代替颜色区分；返回 (形状, 纹理)。
pub fn colorblind_encode(mode: BlindMode, slot: u8) -> (&'static str, &'static str) {
    const RG: [(&str, &str); 4] = [("圆", "实心"), ("方", "斜纹"), ("三角", "点阵"), ("菱", "网格")];
    const BY: [(&str, &str); 4] = [("三角", "实心"), ("菱", "斜纹"), ("圆", "点阵"), ("方", "网格")];
    const AC: [(&str, &str); 4] = [("实心", "粗边"), ("斜纹", "中边"), ("点阵", "细边"), ("网格", "虚线边")];
    let i = (slot as usize) % 4;
    match mode {
        BlindMode::RedGreen => RG[i],
        BlindMode::BlueYellow => BY[i],
        BlindMode::Achromatopsia => AC[i],
    }
}

/// #513 纯键盘操作结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStep {
    pub focus: usize,
    pub action: &'static str,
}

/// #513 Tab 遍历所有节点 / Enter 选中 / Space 展开 / Esc 关闭。
pub fn keyboard_step(focus: usize, key: &str, total: usize) -> KeyStep {
    if total == 0 {
        return KeyStep { focus: 0, action: "空" };
    }
    let f = focus.min(total - 1);
    match key {
        "Tab" => KeyStep { focus: (f + 1) % total, action: "遍历下一节点" },
        "Shift+Tab" => KeyStep { focus: (f + total - 1) % total, action: "遍历上一节点" },
        "Enter" => KeyStep { focus: f, action: "选中" },
        "Space" => KeyStep { focus: f, action: "展开" },
        "Esc" => KeyStep { focus: f, action: "关闭" },
        _ => KeyStep { focus: f, action: "无" },
    }
}

/// #514 屏幕阅读器 ARIA 标签：节点名称 / 类型 / 状态 / 级别。
pub fn aria_label(name: &str, kind: &str, state: &str, level: u8) -> String {
    format!("{kind}「{name}」，{state}，第 {level} 级")
}

/// #515 键盘导航：Home 左上，End 右下，PageUp/Down 翻页。
pub fn nav_page(key: &str, page: usize, pages: usize) -> usize {
    if pages == 0 {
        return 0;
    }
    let p = page.min(pages - 1);
    match key {
        "Home" => 0,
        "End" => pages - 1,
        "PageUp" => p.saturating_sub(1),
        "PageDown" => (p + 1).min(pages - 1),
        _ => p,
    }
}

/// #506~#515 域自检。
pub fn run_a11y_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("a11y");

    // #506 空项目欢迎
    let w0 = welcome(0, 0);
    let w1 = welcome(12, 3000);
    s.add(
        "#506 空项目欢迎",
        w0.show && w0.examples >= 1 && !w1.show && w1.examples == 0,
        "引导 + 示例入口",
    );

    // #507 只读模式
    let ro = perm_state(false);
    let rw = perm_state(true);
    s.add(
        "#507 只读模式",
        ro.readonly && ro.badge == "🔒" && !rw.readonly && rw.badge.is_empty(),
        "无权限文件自动只读+节点标🔒",
    );

    // #508 离线模式
    let off_local = feature_state(false, false);
    let off_remote = feature_state(false, true);
    let on_remote = feature_state(true, true);
    s.add(
        "#508 离线模式",
        off_local == Availability::Full && off_remote == Availability::Grayed && on_remote == Availability::Full,
        "本地正常，远程灰显",
    );

    // #509 内存降级
    let d0 = degrade_actions(0);
    let d2 = degrade_actions(2);
    let d9 = degrade_actions(9);
    s.add(
        "#509 内存降级",
        d0.is_empty()
            && d2 == vec!["粒子", "光晕"]
            && d9.len() == 4
            && d9[2] == "动态壁纸"
            && d9[3] == "降低LOD",
        "粒子→光晕→动态壁纸→降低LOD",
    );

    // #510 多显示器
    let screens = [
        Screen { id: 0, x: 0, y: 0, w: 1920, h: 1080 },
        Screen { id: 1, x: 1920, y: 0, w: 2560, h: 1440 },
    ];
    let next = neighbour_screen(0, &screens);
    let across = spans_screens(1800, 400, &screens);
    let single = spans_screens(100, 400, &screens);
    s.add(
        "#510 多显示器",
        next == 1 && across && !single && neighbour_screen(1, &screens) == 0,
        "面板可拖到副屏，画布跨屏",
    );

    // #511 高DPI
    let px1 = scale_for("pixel", 1.25);
    let px2 = scale_for("pixel", 2.0);
    let px4 = scale_for("pixel", 3.5);
    let v = scale_for("star", 1.75);
    s.add(
        "#511 高DPI",
        px1 == (Scale { factor: 1.0, integer: true })
            && px2 == (Scale { factor: 2.0, integer: true })
            && px4 == (Scale { factor: 4.0, integer: true })
            && v.integer == false
            && (v.factor - 1.75).abs() < 1e-9,
        "像素风 1x/2x/4x 整数缩放，其余矢量自适应",
    );

    // #512 色盲模式
    let rg: Vec<(&str, &str)> = (0..3).map(|i| colorblind_encode(BlindMode::RedGreen, i)).collect();
    let by: Vec<(&str, &str)> = (0..3).map(|i| colorblind_encode(BlindMode::BlueYellow, i)).collect();
    let ac: Vec<(&str, &str)> = (0..3).map(|i| colorblind_encode(BlindMode::Achromatopsia, i)).collect();
    let distinct = |v: &[(&str, &str)]| {
        v[0] != v[1] && v[1] != v[2] && v[0] != v[2]
    };
    s.add(
        "#512 色盲模式",
        distinct(&rg) && distinct(&by) && distinct(&ac) && rg[0] != by[0],
        "形状+纹理替代颜色：红绿盲/蓝黄盲/全色盲",
    );

    // #513 纯键盘
    let tab1 = keyboard_step(2, "Tab", 5);
    let tab2 = keyboard_step(4, "Tab", 5);
    let back = keyboard_step(0, "Shift+Tab", 5);
    let enter = keyboard_step(1, "Enter", 5);
    let space = keyboard_step(1, "Space", 5);
    let esc = keyboard_step(1, "Esc", 5);
    s.add(
        "#513 纯键盘",
        tab1 == (KeyStep { focus: 3, action: "遍历下一节点" })
            && tab2 == (KeyStep { focus: 0, action: "遍历下一节点" })
            && back == (KeyStep { focus: 4, action: "遍历上一节点" })
            && enter.action == "选中"
            && space.action == "展开"
            && esc.action == "关闭",
        "Tab 遍历 / Enter 选中 / Space 展开 / Esc 关闭",
    );

    // #514 屏幕阅读器
    let aria = aria_label("load_config", "函数", "存在泄漏风险", 6);
    s.add(
        "#514 屏幕阅读器",
        aria.contains("load_config") && aria.contains("函数") && aria.contains("泄漏") && aria.contains("6"),
        "ARIA：名称/类型/状态/级别",
    );

    // #515 键盘导航
    let home = nav_page("Home", 5, 10);
    let end = nav_page("End", 5, 10);
    let up = nav_page("PageUp", 5, 10);
    let down = nav_page("PageDown", 5, 10);
    let up0 = nav_page("PageUp", 0, 10);
    let down_end = nav_page("PageDown", 9, 10);
    s.add(
        "#515 键盘导航",
        home == 0 && end == 9 && up == 4 && down == 6 && up0 == 0 && down_end == 9,
        "Home=左上 / End=右下 / PageUp/Down=翻页",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f506_welcome_only_when_empty() {
        assert!(welcome(0, 0).show);
        assert!(!welcome(1, 10).show);
    }

    #[test]
    fn f509_ladder_order() {
        assert_eq!(degrade_actions(1), vec!["粒子"]);
        assert_eq!(degrade_actions(4).len(), 4);
        assert_eq!(degrade_actions(200).len(), 4);
    }

    #[test]
    fn f511_pixel_integer_scale() {
        assert_eq!(scale_for("pixel", 1.0).factor, 1.0);
        assert_eq!(scale_for("pixel", 2.5).factor, 2.0);
        assert_eq!(scale_for("pixel", 4.0).factor, 4.0);
        assert!(!scale_for("modern", 2.5).integer);
    }

    #[test]
    fn f515_nav_bounds() {
        assert_eq!(nav_page("Home", 3, 8), 0);
        assert_eq!(nav_page("End", 3, 8), 7);
        assert_eq!(nav_page("PageDown", 0, 1), 0);
    }
}
