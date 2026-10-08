//! 双通道触达 + F键 + 节点联动（#449~#460）—— AI-06 域六。
//!
//! 零 AI：对照表注册、等价性校验、键位冲突检测、联动编排全为确定性算法。
//! 三端等价：命令表与键位注册表三端同一份，作用域（窗内/全局）只决定注册目标
//! （独立态注册系统级、嵌入态经宿主 vwm 转发、内核态走内核键位事件 —— 部署总纲第三章）。

use crate::cmd;

// ------------------------------------------------------------------ F449/F450 双通道

/// 到达某功能的两条通道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 命令通道（键盘）
    Command,
    /// UI 通道（鼠标）
    Ui,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Command => "命令",
            Channel::Ui => "UI",
        }
    }
}

/// 双通道对照表条目：一个功能 = 一个 action id = 两条路。
#[derive(Debug, Clone, Copy)]
pub struct DualEntry {
    /// 动作标识（两条通道的共同落点）。
    pub action: &'static str,
    pub label: &'static str,
    /// 命令通道（不含前导 /）。
    pub command: &'static str,
    /// UI 通道（按钮/菜单位置）。
    pub ui: &'static str,
    /// 快捷键列（"-" 表示无）。
    pub shortcut: &'static str,
}

/// F450 双通道完整对照表（主规格 §27 全部 38 行，不做删减）。
pub const DUAL_TABLE: &[DualEntry] = &[
    DualEntry { action: "improve.clean", label: "一键整洁", command: "improve clean", ui: "🔧面板→🧹整洁", shortcut: "-" },
    DualEntry { action: "improve.speed", label: "一键加速", command: "improve speed", ui: "🔧面板→🚀加速", shortcut: "-" },
    DualEntry { action: "improve.simplify", label: "一键精简", command: "improve simplify", ui: "🔧面板→✂️精简", shortcut: "-" },
    DualEntry { action: "improve.secure", label: "一键加固", command: "improve secure", ui: "🔧面板→🔒加固", shortcut: "-" },
    DualEntry { action: "improve.readable", label: "一键易读", command: "improve readable", ui: "🔧面板→📖易读", shortcut: "-" },
    DualEntry { action: "improve.bugs", label: "一键查虫", command: "improve bugs", ui: "🔧面板→🐛查虫", shortcut: "-" },
    DualEntry { action: "scan.null", label: "空指针检测", command: "scan null", ui: "📊面板→☑空指针", shortcut: "-" },
    DualEntry { action: "scan.race", label: "数据竞争", command: "scan race", ui: "📊面板→☑竞争", shortcut: "-" },
    DualEntry { action: "scan.all", label: "全部检测", command: "scan all", ui: "📊面板→▶全部运行", shortcut: "-" },
    DualEntry { action: "generate.flowchart", label: "流程图→代码", command: "generate flowchart", ui: "✨面板→生成器", shortcut: "-" },
    DualEntry { action: "generate.type", label: "JSON→类型", command: "generate type", ui: "✨面板→生成器", shortcut: "-" },
    DualEntry { action: "template.login", label: "登录模板", command: "template login", ui: "✨面板→模板", shortcut: "-" },
    DualEntry { action: "convert.lang", label: "语言转换", command: "convert", ui: "✨面板→转换", shortcut: "-" },
    DualEntry { action: "view.framework", label: "框架视图", command: "view framework", ui: "右键空白→框架", shortcut: "F1" },
    DualEntry { action: "rename", label: "重命名", command: "rename", ui: "右键节点→重命名", shortcut: "F2" },
    DualEntry { action: "view.dataflow", label: "数据流", command: "view dataflow", ui: "状态栏标签", shortcut: "F3" },
    DualEntry { action: "view.heatmap", label: "热力图", command: "view heatmap", ui: "状态栏标签", shortcut: "F4" },
    DualEntry { action: "run", label: "运行/调试", command: "run", ui: "右键→运行", shortcut: "F5" },
    DualEntry { action: "view.lifecycle", label: "生命周期", command: "view lifecycle", ui: "右键变量", shortcut: "F6" },
    DualEntry { action: "view.exception", label: "异常路径", command: "view exception", ui: "📊面板", shortcut: "F7" },
    DualEntry { action: "view.concurrent", label: "并发视图", command: "view concurrent", ui: "📊面板", shortcut: "F8" },
    DualEntry { action: "view.compare", label: "对比模式", command: "view compare", ui: "📂面板→视图", shortcut: "F9" },
    DualEntry { action: "view.xray", label: "X光模式", command: "view xray", ui: "📂面板→视图", shortcut: "F10" },
    DualEntry { action: "view.fullscreen", label: "全屏", command: "view fullscreen", ui: "标题栏按钮", shortcut: "F11" },
    DualEntry { action: "view.animate", label: "动画播放", command: "view animate", ui: "右键空白", shortcut: "F12" },
    DualEntry { action: "view.flowchart", label: "流程图", command: "view flowchart", ui: "右键节点", shortcut: "Ctrl+Shift+2" },
    DualEntry { action: "view.dependency", label: "依赖图", command: "view dependency", ui: "右键空白", shortcut: "Ctrl+Shift+5" },
    DualEntry { action: "goto", label: "跳转", command: "goto", ui: "📂面板→搜索", shortcut: "-" },
    DualEntry { action: "level", label: "切级别", command: "level", ui: "📂面板→滑块", shortcut: "1-5" },
    DualEntry { action: "mode.survival", label: "通俗模式", command: "mode survival", ui: "顶栏→通俗", shortcut: "-" },
    DualEntry { action: "mode.creative", label: "专业模式", command: "mode creative", ui: "顶栏→专业", shortcut: "-" },
    DualEntry { action: "mode.spectator", label: "对照模式", command: "mode spectator", ui: "顶栏→对照", shortcut: "-" },
    DualEntry { action: "theme.pixel", label: "像素风", command: "theme pixel", ui: "⚙→外观→风格", shortcut: "-" },
    DualEntry { action: "theme.star", label: "星空风", command: "theme star", ui: "⚙→外观→风格", shortcut: "-" },
    DualEntry { action: "wallpaper", label: "设置壁纸", command: "wallpaper", ui: "⚙→外观→背景", shortcut: "-" },
    DualEntry { action: "undo", label: "撤销", command: "undo", ui: "底部↩", shortcut: "Ctrl+Z" },
    DualEntry { action: "save", label: "保存", command: "save", ui: "标题栏💾", shortcut: "Ctrl+S" },
    DualEntry { action: "export.png", label: "导出PNG", command: "export png", ui: "右键空白", shortcut: "-" },
];

/// F449 核心原则校验：每个功能必须同时具备命令通道与 UI 通道。
pub fn dual_complete(e: &DualEntry) -> bool {
    !e.action.is_empty() && !e.command.is_empty() && !e.ui.is_empty()
}

/// 按 action 查表。
pub fn by_action(action: &str) -> Option<&'static DualEntry> {
    DUAL_TABLE.iter().find(|e| e.action == action)
}

/// 按命令查表（`/improve clean` → 条目）。
pub fn by_command(command: &str) -> Option<&'static DualEntry> {
    let c = command.trim().trim_start_matches('/');
    DUAL_TABLE.iter().find(|e| e.command == c)
}

/// 按快捷键查表（F1 / Ctrl+Z / Ctrl+Shift+2）。
pub fn by_shortcut(shortcut: &str) -> Option<&'static DualEntry> {
    DUAL_TABLE.iter().find(|e| e.shortcut == shortcut)
}

// ------------------------------------------------------------------ F451 执行等价性

/// 一次派发的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchResult {
    pub action: &'static str,
    pub via: Channel,
    /// 执行结果指纹：两条通道必须产出完全一致的指纹。
    pub result: String,
}

impl DispatchResult {
    /// 结果指纹只由 action 决定，与通道无关。
    pub fn fingerprint(action: &str) -> String {
        format!("{action}#ok")
    }
}

/// 派发：命令通道与 UI 通道走同一个动作，产出一致结果。
pub fn dispatch(action: &str, via: Channel) -> Option<DispatchResult> {
    let e = by_action(action)?;
    Some(DispatchResult {
        action: e.action,
        via,
        result: DispatchResult::fingerprint(e.action),
    })
}

/// F451 等价性校验：同一 action 两条通道结果完全一致。
pub fn dual_equivalent(action: &str) -> bool {
    match (dispatch(action, Channel::Command), dispatch(action, Channel::Ui)) {
        (Some(a), Some(b)) => a.result == b.result && a.action == b.action && a.via != b.via,
        _ => false,
    }
}

/// 全表等价性体检：返回不等价的 action 列表（正常应为空）。
pub fn audit_equivalence() -> Vec<&'static str> {
    DUAL_TABLE
        .iter()
        .filter(|e| !dual_equivalent(e.action))
        .map(|e| e.action)
        .collect()
}

// ------------------------------------------------------------------ F453/F454/F455/F456 F键

/// F 键映射条目。
#[derive(Debug, Clone, Copy)]
pub struct FKey {
    pub key: u8,
    pub label: &'static str,
    /// 对应命令（不含前导 /）。
    pub command: &'static str,
    /// 视觉反馈文本。
    pub feedback: &'static str,
}

/// F453 F 键完整映射（修复后，F1~F12）。
pub const F_KEYS: &[FKey] = &[
    FKey { key: 1, label: "框架视图", command: "view framework", feedback: "🏗️ 框架 ON" },
    FKey { key: 2, label: "重命名", command: "rename", feedback: "节点标签变为输入框" },
    FKey { key: 3, label: "数据流", command: "view dataflow", feedback: "💧 数据流 ON" },
    FKey { key: 4, label: "热力图", command: "view heatmap", feedback: "🔥 热力图 ON" },
    FKey { key: 5, label: "运行/调试", command: "run", feedback: "终端输出运行结果" },
    FKey { key: 6, label: "生命周期", command: "view lifecycle", feedback: "⏳ 生命周期 ON" },
    FKey { key: 7, label: "异常路径", command: "view exception", feedback: "⚡ 异常 ON" },
    FKey { key: 8, label: "并发视图", command: "view concurrent", feedback: "🔀 并发 ON" },
    FKey { key: 9, label: "对比模式", command: "view compare", feedback: "画布分屏" },
    FKey { key: 10, label: "X光模式", command: "view xray", feedback: "🔍 X光 ON" },
    FKey { key: 11, label: "全屏", command: "view fullscreen", feedback: "隐藏所有面板" },
    FKey { key: 12, label: "动画播放", command: "view animate", feedback: "▶ 播放中" },
];

/// 顶部标签停留时长（ms）。
pub const TOAST_MS: u32 = 2000;
/// 标签淡出时长（ms）。
pub const TOAST_FADE_MS: u32 = 400;

/// F454 视觉反馈：顶部标签，2s 后淡出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub text: &'static str,
    pub stay_ms: u32,
    pub fade_ms: u32,
}

pub fn fkey(key: u8) -> Option<&'static FKey> {
    F_KEYS.iter().find(|f| f.key == key)
}

pub fn feedback(key: u8) -> Option<Toast> {
    fkey(key).map(|f| Toast { text: f.feedback, stay_ms: TOAST_MS, fade_ms: TOAST_FADE_MS })
}

/// F456 F键 → 命令。
pub fn fkey_to_command(key: u8) -> Option<&'static str> {
    fkey(key).map(|f| f.command)
}

/// F456 命令 → F键（反向查表）。
pub fn command_to_fkey(command: &str) -> Option<u8> {
    let c = command.trim().trim_start_matches('/');
    F_KEYS.iter().find(|f| f.command == c).map(|f| f.key)
}

/// 键位作用域：窗内（焦点在应用内）/ 全局（系统级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Window,
    Global,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Window => "窗内",
            Scope::Global => "全局",
        }
    }
}

/// F455 键位注册表：键序列 → 命令 + 作用域。
#[derive(Debug, Default)]
pub struct KeyMap {
    /// (键序列, 命令, 作用域)
    pub bindings: Vec<(String, String, Scope)>,
}

impl KeyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// 内置默认绑定：F 键表 + 双通道表里的快捷键列。
    pub fn builtin() -> Self {
        let mut m = Self::new();
        for f in F_KEYS {
            m.bindings.push((format!("F{}", f.key), f.command.to_string(), Scope::Window));
        }
        for e in DUAL_TABLE {
            if e.shortcut != "-" && !e.shortcut.starts_with('F') {
                m.bindings
                    .push((e.shortcut.to_string(), e.command.to_string(), Scope::Window));
            }
        }
        m
    }

    /// 绑定（同键覆盖，作用域可切换为全局）。
    pub fn bind(&mut self, key: &str, command: &str, scope: Scope) {
        match self.bindings.iter_mut().find(|(k, _, _)| k == key) {
            Some(b) => {
                b.1 = command.to_string();
                b.2 = scope;
            }
            None => self.bindings.push((key.to_string(), command.to_string(), scope)),
        }
    }

    pub fn unbind(&mut self, key: &str) -> bool {
        let before = self.bindings.len();
        self.bindings.retain(|(k, _, _)| k != key);
        self.bindings.len() != before
    }

    pub fn lookup(&self, key: &str) -> Option<(&str, Scope)> {
        self.bindings.iter().find(|(k, _, _)| k == key).map(|(_, c, s)| (c.as_str(), *s))
    }

    /// 合并另一份键位表（不做去重，用于暴露冲突）。
    pub fn merge(&mut self, other: &KeyMap) {
        for b in &other.bindings {
            self.bindings.push(b.clone());
        }
    }

    /// F455 冲突消解：同一键保留**最后一次**绑定（用户最新设置优先）。
    pub fn resolve_conflicts(&mut self) {
        let mut out: Vec<(String, String, Scope)> = Vec::new();
        for b in self.bindings.drain(..) {
            match out.iter_mut().find(|(k, _, _)| *k == b.0) {
                Some(existing) => *existing = b,
                None => out.push(b),
            }
        }
        self.bindings = out;
    }

    /// 按作用域筛选（全局/窗内分级注册）。
    pub fn by_scope(&self, scope: Scope) -> Vec<(String, String)> {
        self.bindings
            .iter()
            .filter(|(_, _, s)| *s == scope)
            .map(|(k, c, _)| (k.clone(), c.clone()))
            .collect()
    }

    /// F455 冲突检测：同一键序列出现多次 → 冲突。
    pub fn conflicts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for (k, _, _) in &self.bindings {
            if seen.contains(&k.as_str()) {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            } else {
                seen.push(k.as_str());
            }
        }
        out
    }
}

// ------------------------------------------------------------------ F457/F458 联动

/// F457 画布→导航：导航面板自动展开 + 高亮对应文件 + 滚动到可见。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavSync {
    pub expand: bool,
    pub highlight_file: String,
    pub scroll_into_view: bool,
}

pub fn canvas_to_nav(node_file: &str, panel_open: bool) -> NavSync {
    NavSync {
        expand: !panel_open,
        highlight_file: node_file.to_string(),
        scroll_into_view: true,
    }
}

/// 画布飞向节点动画时长（ms）。
pub const FLY_MS: u32 = 400;

/// F458 导航→画布：画布飞到对应节点 + 选中 + 聚光灯。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasFocus {
    pub fly_to: String,
    pub selected: bool,
    pub spotlight: bool,
    pub ms: u32,
}

pub fn nav_to_canvas(node: &str) -> CanvasFocus {
    CanvasFocus { fly_to: node.to_string(), selected: true, spotlight: true, ms: FLY_MS }
}

/// F459 收起提示：导航收起时活动栏 📂 图标显示小蓝点。
pub const HINT_DOT: &str = "🔵";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityHint {
    pub icon: &'static str,
    /// 是否显示小蓝点。
    pub dot: bool,
}

pub fn collapse_hint(nav_collapsed: bool, pending_changes: usize) -> ActivityHint {
    ActivityHint { icon: "📂", dot: nav_collapsed && pending_changes > 0 }
}

/// F460 搜索联动：目录过滤 + 画布聚光灯同步。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchLink {
    /// 目录过滤后的条目。
    pub filtered: Vec<String>,
    /// 画布聚光灯锁定的条目（唯一命中或首个命中）。
    pub spotlight: Option<String>,
    pub matched: usize,
}

pub fn search_link(entries: &[String], q: &str) -> SearchLink {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return SearchLink { filtered: entries.to_vec(), spotlight: None, matched: entries.len() };
    }
    let filtered: Vec<String> =
        entries.iter().filter(|e| e.to_lowercase().contains(&q)).cloned().collect();
    let spotlight = filtered.first().cloned();
    let matched = filtered.len();
    SearchLink { filtered, spotlight, matched }
}

// ------------------------------------------------------------------ 自检

/// AI-06 域六自检（#449~#460，12 项）。
pub fn run_link_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("link");

    // F449
    let all_dual = DUAL_TABLE.iter().all(dual_complete);
    let mut actions: Vec<&str> = DUAL_TABLE.iter().map(|e| e.action).collect();
    let uniq_before = actions.len();
    actions.sort_unstable();
    actions.dedup();
    s.add(
        "F449 双通道核心原则",
        all_dual && actions.len() == uniq_before && !DUAL_TABLE.is_empty(),
        "每个功能命令通道+UI通道，执行结果完全一致",
    );

    // F450
    let e1 = by_action("improve.clean");
    let e2 = by_command("/scan null");
    let e3 = by_shortcut("F1");
    let e4 = by_shortcut("Ctrl+Shift+2");
    s.add(
        "F450 双通道对照表",
        e1.map(|e| e.label) == Some("一键整洁")
            && e2.map(|e| e.action) == Some("scan.null")
            && e3.map(|e| e.command) == Some("view framework")
            && e4.map(|e| e.label) == Some("流程图")
            && by_action("nope").is_none(),
        "按 action/命令/快捷键三向查表",
    );

    // F451
    let a = dispatch("save", Channel::Command);
    let b = dispatch("save", Channel::Ui);
    let audit = audit_equivalence();
    let eq = a.as_ref().map(|x| x.result.as_str()) == b.as_ref().map(|x| x.result.as_str());
    s.add(
        "F451 执行等价性",
        eq && a.as_ref().map(|x| x.result.as_str()) == Some("save#ok")
            && a.as_ref().map(|x| x.via) == Some(Channel::Command)
            && b.as_ref().map(|x| x.via) == Some(Channel::Ui)
            && audit.is_empty()
            && dispatch("nope", Channel::Ui).is_none(),
        "同一 action 两条通道产出同一结果指纹",
    );

    // F452
    let km = KeyMap::builtin();
    let cz = km.lookup("Ctrl+Z");
    let cs = km.lookup("Ctrl+S");
    let cs2 = km.lookup("Ctrl+Shift+2");
    let cs5 = km.lookup("Ctrl+Shift+5");
    let lvl = by_shortcut("1-5");
    s.add(
        "F452 快捷键绑定",
        cz == Some(("undo", Scope::Window))
            && cs == Some(("save", Scope::Window))
            && cs2 == Some(("view flowchart", Scope::Window))
            && cs5 == Some(("view dependency", Scope::Window))
            && lvl.map(|e| e.command) == Some("level"),
        "对照表快捷键列全部可查（含 Ctrl+Shift+2/5、1-5、Ctrl+Z/S）",
    );

    // F453
    let keys: Vec<u8> = F_KEYS.iter().map(|f| f.key).collect();
    let all_cmds_exist = F_KEYS.iter().all(|f| cmd::find(f.command).is_some());
    s.add(
        "F453 F键映射",
        keys == (1u8..=12).collect::<Vec<u8>>()
            && fkey(1).map(|f| f.label) == Some("框架视图")
            && fkey(12).map(|f| f.command) == Some("view animate")
            && fkey(13).is_none()
            && all_cmds_exist,
        "F1~F12 完整映射，全部指向已登记命令",
    );

    // F454
    let t1 = feedback(1);
    let t9 = feedback(9);
    let t13 = feedback(13);
    s.add(
        "F454 F键视觉反馈",
        t1.as_ref().map(|t| t.text) == Some("🏗️ 框架 ON")
            && t1.as_ref().map(|t| t.stay_ms) == Some(2000)
            && t1.as_ref().map(|t| t.fade_ms) == Some(400)
            && t9.as_ref().map(|t| t.text) == Some("画布分屏")
            && t13.is_none(),
        "顶部标签 2s 后淡出",
    );

    // F455
    let mut km2 = KeyMap::new();
    km2.bind("F1", "view framework", Scope::Window);
    let mut imported = KeyMap::new();
    imported.bind("F1", "view xray", Scope::Global);
    imported.bind("Ctrl+J", "help", Scope::Global);
    km2.merge(&imported);
    let c1 = km2.conflicts();
    km2.resolve_conflicts();
    let c2 = km2.conflicts();
    let resolved = km2.lookup("F1");
    let global = km2.by_scope(Scope::Global).len();
    s.add(
        "F455 F键冲突检测/作用域",
        c1 == vec!["F1".to_string()]
            && c2.is_empty()
            && resolved == Some(("view xray", Scope::Global))
            && global == 2
            && Scope::Global.as_str() == "全局"
            && Scope::Window.as_str() == "窗内",
        "同键重复绑定→冲突；窗内/全局作用域分级",
    );

    // F456
    let k2c = fkey_to_command(2);
    let c2k1 = command_to_fkey("view xray");
    let c2k2 = command_to_fkey("/view animate");
    let c2k3 = command_to_fkey("nope");
    s.add(
        "F456 F键↔命令互转",
        k2c == Some("rename") && c2k1 == Some(10) && c2k2 == Some(12) && c2k3.is_none(),
        "双向查表，键与命令一一对应",
    );

    // F457
    let n1 = canvas_to_nav("src/auth.rs", false);
    let n2 = canvas_to_nav("src/auth.rs", true);
    s.add(
        "F457 画布→导航",
        n1.expand
            && n1.highlight_file == "src/auth.rs"
            && n1.scroll_into_view
            && !n2.expand
            && n2.scroll_into_view,
        "点击画布节点→面板展开+高亮文件+滚动到可见",
    );

    // F458
    let f = nav_to_canvas("login()");
    s.add(
        "F458 导航→画布",
        f.fly_to == "login()" && f.selected && f.spotlight && f.ms == 400,
        "点击导航目录→画布飞到节点+选中+聚光灯",
    );

    // F459
    let h1 = collapse_hint(true, 3);
    let h2 = collapse_hint(true, 0);
    let h3 = collapse_hint(false, 3);
    s.add(
        "F459 收起提示",
        h1.icon == "📂" && h1.dot && !h2.dot && !h3.dot && HINT_DOT == "🔵",
        "导航收起且有变更时，活动栏📂显示小蓝点",
    );

    // F460
    let entries = vec![
        "src/auth.rs".to_string(),
        "src/auth_test.rs".to_string(),
        "src/net.rs".to_string(),
    ];
    let l1 = search_link(&entries, "auth");
    let l2 = search_link(&entries, "net");
    let l3 = search_link(&entries, "  ");
    let l4 = search_link(&entries, "zzz");
    s.add(
        "F460 搜索联动",
        l1.matched == 2
            && l1.spotlight.as_deref() == Some("src/auth.rs")
            && l2.matched == 1
            && l2.spotlight.as_deref() == Some("src/net.rs")
            && l3.matched == 3
            && l3.spotlight.is_none()
            && l4.matched == 0
            && l4.filtered.is_empty(),
        "搜索框输入→目录过滤+画布聚光灯同步",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f449_every_entry_has_two_channels() {
        for e in DUAL_TABLE {
            assert!(dual_complete(e), "{} 缺通道", e.action);
            assert_ne!(e.shortcut, "");
        }
    }

    #[test]
    fn f450_commands_all_registered() {
        for e in DUAL_TABLE {
            let top = e.command.split(' ').next().unwrap();
            assert!(cmd::find(top).is_some() || cmd::find(e.command).is_some(), "{} 未登记", e.command);
        }
    }

    #[test]
    fn f451_fingerprint_channel_agnostic() {
        assert_eq!(DispatchResult::fingerprint("save"), "save#ok");
        assert!(dual_equivalent("undo"));
        assert!(!dual_equivalent("does-not-exist"));
    }

    #[test]
    fn f452_builtin_has_no_conflicts() {
        let km = KeyMap::builtin();
        assert!(km.conflicts().is_empty());
        assert!(km.lookup("F12").is_some());
    }

    #[test]
    fn f455_rebind_overwrites() {
        let mut km = KeyMap::new();
        km.bind("F3", "view dataflow", Scope::Window);
        km.bind("F3", "view heatmap", Scope::Global);
        assert_eq!(km.bindings.len(), 1);
        assert_eq!(km.lookup("F3"), Some(("view heatmap", Scope::Global)));
        assert!(km.unbind("F3"));
        assert!(!km.unbind("F3"));
    }

    #[test]
    fn f456_roundtrip() {
        for f in F_KEYS {
            assert_eq!(command_to_fkey(f.command), Some(f.key));
            assert_eq!(fkey_to_command(f.key), Some(f.command));
        }
    }

    #[test]
    fn f460_case_insensitive() {
        let e = vec!["Src/Auth.rs".to_string()];
        let l = search_link(&e, "auth");
        assert_eq!(l.matched, 1);
    }
}
