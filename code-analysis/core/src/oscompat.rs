//! AI-07 · 操作系统兼容性修复（#491~#505）。
//!
//! 修复原则：对照用户最熟悉的软件（VS Code / Windows / Mac / Chrome / Figma），
//! 确保肌肉记忆不用改。零 AI：静态对照表 + 确定性分派。

/// 一条兼容性修复记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fix {
    pub id: u16,
    pub item: &'static str,
    pub before: &'static str,
    pub after: &'static str,
    /// 对照的熟悉软件。
    pub habit: &'static str,
}

/// 修复对照表（#491~#505，15 项）。
pub const FIXES: &[Fix] = &[
    Fix { id: 491, item: "F2", before: "流程图", after: "重命名", habit: "Windows" },
    Fix { id: 492, item: "F5", before: "依赖图", after: "运行/调试", habit: "VS Code" },
    Fix { id: 493, item: "流程图键位", before: "F2", after: "Ctrl+Shift+2", habit: "不冲突" },
    Fix { id: 494, item: "依赖图键位", before: "F5", after: "Ctrl+Shift+5", habit: "不冲突" },
    Fix { id: 495, item: "命令面板", before: "仅Ctrl+K", after: "Ctrl+K + Ctrl+Shift+P", habit: "VS Code" },
    Fix { id: 496, item: "终端键位", before: "仅T", after: "T(像素) + `(其他) + Ctrl+J(通用)", habit: "VS Code" },
    Fix { id: 497, item: "拖拽文件", before: "不支持", after: "拖文件到画布自动导入", habit: "通用" },
    Fix { id: 498, item: "面板调大小", before: "固定", after: "边缘拖拽调整", habit: "VS Code" },
    Fix { id: 499, item: "全屏", before: "仅F11", after: "F11 + 双击标题栏", habit: "浏览器+Windows" },
    Fix { id: 500, item: "多选", before: "仅框选", after: "Shift+点击多选", habit: "通用" },
    Fix { id: 501, item: "全选", before: "无", after: "Ctrl+A全选", habit: "通用" },
    Fix { id: 502, item: "复制节点", before: "无", after: "Ctrl+D复制", habit: "Figma" },
    Fix { id: 503, item: "拖拽复制", before: "无", after: "Alt+拖拽=复制", habit: "Figma" },
    Fix { id: 504, item: "侧键导航", before: "无", after: "鼠标侧键=前进/后退", habit: "浏览器" },
    Fix { id: 505, item: "全局搜索", before: "仅Ctrl+F", after: "Ctrl+Shift+F全局", habit: "VS Code" },
];

pub fn fix(id: u16) -> Option<&'static Fix> {
    FIXES.iter().find(|f| f.id == id)
}

/// 修复后的功能→键位映射（供键位表与命令面板消费）。
pub fn binding_of(action: &str) -> &'static str {
    match action {
        "重命名" => "F2",
        "运行/调试" => "F5",
        "流程图" => "Ctrl+Shift+2",
        "依赖图" => "Ctrl+Shift+5",
        "命令面板" => "Ctrl+K",
        "命令面板(备用)" => "Ctrl+Shift+P",
        "终端(像素风)" => "T",
        "终端(非像素风)" => "`",
        "切换终端" => "Ctrl+J",
        "全屏" => "F11",
        "全选" => "Ctrl+A",
        "复制节点" => "Ctrl+D",
        "全局搜索" => "Ctrl+Shift+F",
        _ => "",
    }
}

/// #493/#494：F1~F12 生命周期键位不再被流程图/依赖图占用。
pub fn function_keys_free() -> bool {
    binding_of("流程图") != "F2"
        && binding_of("依赖图") != "F5"
        && binding_of("重命名") == "F2"
        && binding_of("运行/调试") == "F5"
}

/// #495：命令面板双入口。
pub fn command_palette_triggers() -> [&'static str; 2] {
    ["Ctrl+K", "Ctrl+Shift+P"]
}

/// #496：终端三入口（按风格取主键，Ctrl+J 为通用）。
pub fn terminal_triggers(style: &str) -> Vec<&'static str> {
    let primary = if style == "pixel" { "T" } else { "`" };
    vec![primary, "Ctrl+J"]
}

/// #497：拖文件到画布 → 自动导入。返回导入目标描述；不支持的类型返回 None。
pub fn drop_target(name: &str) -> Option<&'static str> {
    let Some((_, ext)) = name.rsplit_once('.') else { return None };
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "" => None,
        "png" | "jpg" | "jpeg" | "svg" => Some("图片资产"),
        "json" | "toml" | "yaml" | "yml" | "xml" => Some("配置数据"),
        "sql" | "graphql" | "proto" => Some("接口定义"),
        _ => Some("源码项目"),
    }
}

/// #498：面板边缘拖拽调整大小。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEdge {
    Left,
    Right,
    Top,
    Bottom,
    Corner,
}

/// #498：面板边缘拖拽调整大小。左/上边缘向外拖为正增长，右/下边缘相反。
pub fn resize_panel(edge: PanelEdge, delta: i32, size: u32, min: u32, max: u32) -> u32 {
    let sign = match edge {
        PanelEdge::Left | PanelEdge::Top => 1,
        PanelEdge::Right | PanelEdge::Bottom | PanelEdge::Corner => -1,
    };
    let target = size as i32 + sign * delta;
    target.clamp(min as i32, max as i32) as u32
}

/// #499：全屏触发（F11 键 或 双击标题栏）。
pub fn fullscreen_trigger(key: &str, title_bar_double_click: bool) -> bool {
    key == "F11" || title_bar_double_click
}

/// #500~#504：鼠标动作 → 语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Select,
    MultiSelect,
    Marquee,
    DragMove,
    DragCopy,
    Forward,
    Backward,
    ContextMenu,
    None,
}

pub fn mouse_action(button: &str, shift: bool, alt: bool, double: bool) -> MouseAction {
    match button {
        "侧键前进" => MouseAction::Forward,
        "侧键后退" => MouseAction::Backward,
        "右键" => MouseAction::ContextMenu,
        "左键" if double => MouseAction::Select,
        "左键" if alt => MouseAction::DragCopy,
        "左键" if shift => MouseAction::MultiSelect,
        "左键" => MouseAction::Select,
        "拖拽" if alt => MouseAction::DragCopy,
        "拖拽" if shift => MouseAction::Marquee,
        "拖拽" => MouseAction::DragMove,
        _ => MouseAction::None,
    }
}

/// #505：全局搜索键位判定。
pub fn is_global_search(key: &str) -> bool {
    key == "Ctrl+Shift+F"
}

/// #491~#505 域自检。
pub fn run_oscompat_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("oscompat");

    // #491 F2=重命名
    let f491 = fix(491).unwrap();
    s.add(
        "#491 F2=重命名",
        f491.after == "重命名" && f491.habit == "Windows" && binding_of("重命名") == "F2",
        "符合 Windows 习惯",
    );

    // #492 F5=运行/调试
    let f492 = fix(492).unwrap();
    s.add(
        "#492 F5=运行/调试",
        f492.after == "运行/调试" && f492.habit == "VS Code" && binding_of("运行/调试") == "F5",
        "符合 VS Code 习惯",
    );

    // #493 流程图键位 → Ctrl+Shift+2
    s.add(
        "#493 流程图键位",
        binding_of("流程图") == "Ctrl+Shift+2" && binding_of("流程图") != "F2",
        "避开 F2 重命名",
    );

    // #494 依赖图键位 → Ctrl+Shift+5
    s.add(
        "#494 依赖图键位",
        binding_of("依赖图") == "Ctrl+Shift+5" && binding_of("依赖图") != "F5",
        "避开 F5 运行",
    );

    // #495 命令面板双入口
    let pal = command_palette_triggers();
    s.add(
        "#495 命令面板",
        pal == ["Ctrl+K", "Ctrl+Shift+P"] && fix(495).unwrap().habit == "VS Code",
        "Ctrl+K + Ctrl+Shift+P",
    );

    // #496 终端三入口
    let t_pixel = terminal_triggers("pixel");
    let t_modern = terminal_triggers("modern");
    s.add(
        "#496 终端键位",
        t_pixel == vec!["T", "Ctrl+J"] && t_modern == vec!["`", "Ctrl+J"] && binding_of("切换终端") == "Ctrl+J",
        "T(像素) + `(其他) + Ctrl+J(通用)",
    );

    // #497 拖拽文件自动导入
    let d_img = drop_target("logo.png");
    let d_code = drop_target("main.rs");
    let d_bad = drop_target("noext");
    s.add(
        "#497 拖拽文件",
        d_img == Some("图片资产") && d_code == Some("源码项目") && d_bad.is_none(),
        "拖文件到画布自动导入",
    );

    // #498 面板边缘拖拽调整
    let grow = resize_panel(PanelEdge::Left, 60, 300, 180, 800);
    let shrink = resize_panel(PanelEdge::Right, 60, 300, 180, 800);
    let clamp_hi = resize_panel(PanelEdge::Left, 9999, 300, 180, 800);
    let clamp_lo = resize_panel(PanelEdge::Left, -9999, 300, 180, 800);
    s.add(
        "#498 面板调大小",
        grow == 360 && shrink == 240 && clamp_hi == 800 && clamp_lo == 180,
        "边缘拖拽 + 最小/最大钳制",
    );

    // #499 全屏
    let fs_key = fullscreen_trigger("F11", false);
    let fs_dbl = fullscreen_trigger("X", true);
    let fs_no = fullscreen_trigger("X", false);
    s.add(
        "#499 全屏",
        fs_key && fs_dbl && !fs_no && fix(499).unwrap().habit == "浏览器+Windows",
        "F11 + 双击标题栏",
    );

    // #500 多选 Shift+点击
    s.add(
        "#500 多选",
        mouse_action("左键", true, false, false) == MouseAction::MultiSelect
            && mouse_action("左键", false, false, false) == MouseAction::Select,
        "Shift+点击多选",
    );

    // #501 全选 Ctrl+A
    s.add(
        "#501 全选",
        binding_of("全选") == "Ctrl+A" && fix(501).unwrap().before == "无",
        "Ctrl+A 全选",
    );

    // #502 复制节点 Ctrl+D
    s.add(
        "#502 复制节点",
        binding_of("复制节点") == "Ctrl+D" && fix(502).unwrap().habit == "Figma",
        "Ctrl+D 复制",
    );

    // #503 拖拽复制 Alt+拖拽
    s.add(
        "#503 拖拽复制",
        mouse_action("拖拽", false, true, false) == MouseAction::DragCopy
            && mouse_action("拖拽", false, false, false) == MouseAction::DragMove,
        "Alt+拖拽=复制",
    );

    // #504 侧键导航
    s.add(
        "#504 侧键导航",
        mouse_action("侧键后退", false, false, false) == MouseAction::Backward
            && mouse_action("侧键前进", false, false, false) == MouseAction::Forward,
        "鼠标侧键=前进/后退",
    );

    // #505 全局搜索 Ctrl+Shift+F
    let ctrl_f = is_global_search("Ctrl+F");
    let ctrl_shift_f = is_global_search("Ctrl+Shift+F");
    s.add(
        "#505 全局搜索",
        !ctrl_f && ctrl_shift_f && binding_of("全局搜索") == "Ctrl+Shift+F",
        "Ctrl+Shift+F 全局",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f491_f495_function_keys_restored() {
        assert!(function_keys_free());
        assert_eq!(binding_of("流程图"), "Ctrl+Shift+2");
        assert_eq!(binding_of("依赖图"), "Ctrl+Shift+5");
    }

    #[test]
    fn f497_drop_kinds() {
        assert_eq!(drop_target("a.sql"), Some("接口定义"));
        assert_eq!(drop_target("a.json"), Some("配置数据"));
        assert!(drop_target("README").is_none());
    }

    #[test]
    fn f498_resize_clamps() {
        assert_eq!(resize_panel(PanelEdge::Right, 0, 300, 180, 800), 300);
        assert_eq!(resize_panel(PanelEdge::Left, 500, 300, 180, 800), 800);
        assert_eq!(resize_panel(PanelEdge::Right, -500, 300, 180, 800), 800);
    }

    #[test]
    fn f503_alt_drag_copies() {
        assert_eq!(mouse_action("拖拽", false, true, false), MouseAction::DragCopy);
        assert_eq!(mouse_action("拖拽", true, false, false), MouseAction::Marquee);
    }
}
