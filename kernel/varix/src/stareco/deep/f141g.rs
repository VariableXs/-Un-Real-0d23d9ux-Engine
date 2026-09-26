//! 深化层四 · F141 无障碍开放标准（2026-09-27 深化批次四 · g 层）。
//!
//! 键盘导航图生成（焦点序→步骤说明）、读屏标注完备性、动效替代
//! 通路表（减弱动效必有余路）、仅色传意检测、触达目标尺寸审计。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 键盘导航图：焦点序声明 → 步骤说明序列（字节预算护栏）
// ---------------------------------------------------------------------------

pub struct NavStep {
    pub widget: &'static str,
    /// 到达该控件的按键（如 "Tab" / "Tab×3"）。
    pub keys: &'static str,
}

/// 渲染步骤行（≤64 字节/步——超预算即导航图不可读）。
pub fn render_nav(steps: &[NavStep]) -> Result<alloc::vec::Vec<alloc::string::String>, &'static str> {
    let mut out = alloc::vec::Vec::new();
    for (i, s) in steps.iter().enumerate() {
        if s.widget.is_empty() || s.keys.is_empty() {
            return Err("导航步骤字段缺失");
        }
        let line = alloc::format!("{}. [{}] {}", i + 1, s.keys, s.widget);
        if line.len() > 64 {
            return Err("导航步骤超 64 字节预算");
        }
        out.push(line);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 读屏标注完备性：可交互控件三件（label/role/state）缺一即点名
// ---------------------------------------------------------------------------

pub struct AriaSpec {
    pub widget: &'static str,
    pub has_label: bool,
    pub has_role: bool,
    pub has_state: bool,
}

pub fn aria_incomplete(specs: &[AriaSpec]) -> alloc::vec::Vec<&'static str> {
    specs
        .iter()
        .filter(|s| !(s.has_label && s.has_role && s.has_state))
        .map(|s| s.widget)
        .collect()
}

// ---------------------------------------------------------------------------
// 动效替代通路：每个动效 ID 在减弱模式下必须有信息等价通路
// ---------------------------------------------------------------------------

pub struct MotionAlt {
    pub anim_id: &'static str,
    /// 替代通路：文字提示/状态常显/无（None = 缺失 = 违例）。
    pub alternative: Option<&'static str>,
}

pub fn motion_alt_violations(alts: &[MotionAlt]) -> alloc::vec::Vec<&'static str> {
    alts.iter()
        .filter(|a| a.alternative.is_none())
        .map(|a| a.anim_id)
        .collect()
}

// ---------------------------------------------------------------------------
// 仅色传意检测：语义状态必须有形状/文字冗余（色弱可用判据）
// ---------------------------------------------------------------------------

pub struct ColorUsage {
    pub element: &'static str,
    pub semantic: bool,
    pub has_redundant_shape: bool,
}

pub fn color_only_violations(usages: &[ColorUsage]) -> alloc::vec::Vec<&'static str> {
    usages
        .iter()
        .filter(|u| u.semantic && !u.has_redundant_shape)
        .map(|u| u.element)
        .collect()
}

// ---------------------------------------------------------------------------
// 触达目标尺寸：可交互元素 ≥44×44（逻辑像素）
// ---------------------------------------------------------------------------

pub struct TouchTarget {
    pub element: &'static str,
    pub w: u32,
    pub h: u32,
}

pub const TOUCH_MIN_PX: u32 = 44;

pub fn touch_violations(ts: &[TouchTarget]) -> alloc::vec::Vec<&'static str> {
    ts.iter().filter(|t| t.w < TOUCH_MIN_PX || t.h < TOUCH_MIN_PX).map(|t| t.element).collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141G_TAG: &str = "stareco-F141-deep4";

pub fn run_f141_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F141G_TAG);

    // 导航图
    let nav = [
        NavStep { widget: "工具栏-新建", keys: "Tab" },
        NavStep { widget: "工具栏-关闭标签", keys: "Tab" },
        NavStep { widget: "查找条", keys: "Ctrl+F" },
    ];
    let lines = render_nav(&nav).expect("ok");
    set.add(
        "f141g nav render",
        lines[0] == "1. [Tab] 工具栏-新建" && lines.len() == 3,
        "步骤行渲染",
    );
    let long = [NavStep { widget: "一个名字特别特别长的控件名称以至于超出预算限", keys: "Tab×12" }];
    set.add("f141g nav budget", render_nav(&long).is_err(), "步骤超 64 字节拒绝");
    set.add(
        "f141g nav empty field",
        render_nav(&[NavStep { widget: "", keys: "Tab" }]).is_err(),
        "空字段拒绝",
    );

    // 读屏标注
    let specs = [
        AriaSpec { widget: "保存", has_label: true, has_role: true, has_state: true },
        AriaSpec { widget: "静音钮", has_label: true, has_role: false, has_state: false },
    ];
    set.add(
        "f141g aria",
        aria_incomplete(&specs) == alloc::vec!["静音钮"],
        "不完备标注点名",
    );

    // 动效替代
    let alts = [
        MotionAlt { anim_id: "toast-slide", alternative: Some("状态常显") },
        MotionAlt { anim_id: "progress-shimmer", alternative: None },
    ];
    set.add(
        "f141g motion alt",
        motion_alt_violations(&alts) == alloc::vec!["progress-shimmer"],
        "无替代通路点名",
    );

    // 仅色传意
    let usages = [
        ColorUsage { element: "成功徽标", semantic: true, has_redundant_shape: true },
        ColorUsage { element: "错误描边", semantic: true, has_redundant_shape: false },
        ColorUsage { element: "装饰分隔", semantic: false, has_redundant_shape: false },
    ];
    set.add(
        "f141g color only",
        color_only_violations(&usages) == alloc::vec!["错误描边"],
        "仅色传意检出（装饰色不判）",
    );

    // 触达目标
    let ts = [
        TouchTarget { element: "主按钮", w: 120, h: 44 },
        TouchTarget { element: "迷你关闭钮", w: 20, h: 20 },
    ];
    set.add(
        "f141g touch",
        touch_violations(&ts) == alloc::vec!["迷你关闭钮"],
        "44px 线点名",
    );
    set.add(
        "f141g touch boundary",
        touch_violations(&[TouchTarget { element: "x", w: 44, h: 44 }]).is_empty(),
        "44 恰好放行",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn nav_render_full_chain() {
        let nav = [NavStep { widget: "a", keys: "Tab" }, NavStep { widget: "b", keys: "Tab" }];
        let lines = render_nav(&nav).unwrap();
        assert_eq!(lines[1], "2. [Tab] b");
    }

    #[test]
    fn aria_all_complete() {
        let specs = [AriaSpec { widget: "x", has_label: true, has_role: true, has_state: true }];
        assert!(aria_incomplete(&specs).is_empty());
    }
}
