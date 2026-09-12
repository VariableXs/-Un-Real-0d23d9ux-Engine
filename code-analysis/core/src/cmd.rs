//! 终端命令系统（#411~#420）—— AI-06 域一：命令表 / 终端 UI / 推荐 / 权限 / 管道。
//!
//! 零 AI：全部为确定性算法（枚举、编辑距离、前缀匹配、顺序求值），不调用任何网络模型。
//! 三端等价：命令表是 core 的数据，Windows 独立 / Variable 嵌入 / VARIX 内核共用同一张表，
//! 端差异只体现在个别命令的执行后端（部署总纲第三章）。`/help` 不过滤、不隐藏任何命令。

// ------------------------------------------------------------------ F419 权限分级

/// 三级权限：🟢安全（只读） / 🟡修改（可撤销） / 🔴危险（需确认 Y/N）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perm {
    /// 只读，不改动任何工程状态。
    Safe,
    /// 会改动状态，但可 `/undo` 撤销。
    Modify,
    /// 破坏性操作，执行前必须 Y/N 确认。
    Danger,
}

impl Perm {
    pub fn badge(self) -> &'static str {
        match self {
            Perm::Safe => "🟢",
            Perm::Modify => "🟡",
            Perm::Danger => "🔴",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Perm::Safe => "safe",
            Perm::Modify => "modify",
            Perm::Danger => "danger",
        }
    }

    /// 危险命令执行前必须确认。
    pub fn needs_confirm(self) -> bool {
        self == Perm::Danger
    }

    /// 确认应答解析：只接受 Y / y / yes；N / n / no 与其他输入一律视为取消。
    pub fn confirm(self, answer: &str) -> bool {
        if !self.needs_confirm() {
            return true;
        }
        matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
    }
}

// ------------------------------------------------------------------ 终端 UI（F411/F412）

/// 终端背景（半透明黑，不遮挡画布）。
pub const TERM_BG: &str = "rgba(0,0,0,0.75)";
/// 历史上限：超过丢弃最旧一行。
pub const MAX_HISTORY: usize = 500;
/// `/help` 每页条数（F413）。
pub const HELP_PAGE_SIZE: usize = 20;

/// 终端字体跟随当前风格：像素风=像素字体，其余=等宽字体（F411）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Pixel,
    Modern,
    Fresh,
    Star,
    Cyber,
    Glass,
    Neu,
    Sketch,
}

impl Theme {
    pub fn font_family(self) -> &'static str {
        match self {
            Theme::Pixel => "'Press Start 2P', monospace",
            _ => "'JetBrains Mono', Consolas, monospace",
        }
    }
}

/// 行类型 → § 颜色代码（§a绿 §c红 §e黄 §b蓝 §7灰 §f白）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    System,
    Input,
    Output,
    Error,
    Hint,
}

impl LineKind {
    pub fn color_code(self) -> &'static str {
        match self {
            LineKind::System => "§7",
            LineKind::Input => "§f",
            LineKind::Output => "§a",
            LineKind::Error => "§c",
            LineKind::Hint => "§b",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TermLine {
    pub kind: LineKind,
    pub text: String,
}

impl TermLine {
    /// 渲染为带 § 颜色前缀的一行。
    pub fn render(&self) -> String {
        format!("{}{}", self.kind.color_code(), self.text)
    }
}

/// F411 终端 UI 框架：底部聊天框，自动滚到底部，最多 500 条历史。
#[derive(Debug, Default)]
pub struct Terminal {
    pub lines: Vec<TermLine>,
    /// 自动滚到底部（用户手动上滚时暂停，回到底部后恢复）。
    pub autoscroll: bool,
}

impl Terminal {
    pub fn new() -> Self {
        Terminal { lines: Vec::new(), autoscroll: true }
    }

    /// 追加一行；超过 500 条丢弃最旧。
    pub fn push(&mut self, kind: LineKind, text: impl Into<String>) {
        self.lines.push(TermLine { kind, text: text.into() });
        if self.lines.len() > MAX_HISTORY {
            let drop = self.lines.len() - MAX_HISTORY;
            self.lines.drain(0..drop);
        }
    }

    /// 可见行：自动滚到底部 ⇒ 取尾部 `rows` 行。
    pub fn visible(&self, rows: usize) -> Vec<&TermLine> {
        let rows = rows.max(1);
        let start = self.lines.len().saturating_sub(rows);
        self.lines[start..].iter().collect()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// F412 终端尺寸档位：折叠 36px/1 行、展开 120px/3-4 行、大展开 300px/10 行、全屏全部。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMode {
    Collapsed,
    Expanded,
    Large,
    Fullscreen,
}

impl SizeMode {
    /// 高度（px）；全屏为 0 表示占满 100%。
    pub fn height_px(self) -> u32 {
        match self {
            SizeMode::Collapsed => 36,
            SizeMode::Expanded => 120,
            SizeMode::Large => 300,
            SizeMode::Fullscreen => 0,
        }
    }

    pub fn rows(self) -> usize {
        match self {
            SizeMode::Collapsed => 1,
            SizeMode::Expanded => 4,
            SizeMode::Large => 10,
            SizeMode::Fullscreen => usize::MAX,
        }
    }

    /// Ctrl+J 循环：折叠 → 展开 → 大展开 → 全屏 → 折叠。
    pub fn cycle(self) -> SizeMode {
        match self {
            SizeMode::Collapsed => SizeMode::Expanded,
            SizeMode::Expanded => SizeMode::Large,
            SizeMode::Large => SizeMode::Fullscreen,
            SizeMode::Fullscreen => SizeMode::Collapsed,
        }
    }
}

/// 按键语义（F412 的判定结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Open,
    Close,
    Execute,
    CycleSize,
    None,
}

/// F412 终端开关：T（像素风）/ 反引号（其他风格）/ Ctrl+J（通用）打开，Esc 关闭，Enter 执行。
#[derive(Debug)]
pub struct TerminalView {
    pub open: bool,
    pub mode: SizeMode,
    pub theme: Theme,
}

impl TerminalView {
    pub fn new(theme: Theme) -> Self {
        TerminalView { open: false, mode: SizeMode::Collapsed, theme }
    }

    /// 该键在当前风格下是否为终端打开键。
    pub fn is_open_hotkey(&self, key: &str) -> bool {
        match self.theme {
            Theme::Pixel => key == "T" || key == "t",
            _ => key == "`",
        }
    }

    /// 统一入口：`ctrl` 表示 Ctrl 组合键。
    pub fn handle_key(&mut self, key: &str, ctrl: bool) -> KeyAction {
        if ctrl && (key == "J" || key == "j") {
            if !self.open {
                self.open = true;
                self.mode = SizeMode::Expanded;
                return KeyAction::Open;
            }
            self.mode = self.mode.cycle();
            return KeyAction::CycleSize;
        }
        if key == "Escape" {
            if self.open {
                self.open = false;
                self.mode = SizeMode::Collapsed;
                return KeyAction::Close;
            }
            return KeyAction::None;
        }
        if key == "Enter" {
            return if self.open { KeyAction::Execute } else { KeyAction::None };
        }
        if !self.open && self.is_open_hotkey(key) {
            self.open = true;
            self.mode = SizeMode::Expanded;
            return KeyAction::Open;
        }
        KeyAction::None
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.mode = SizeMode::Collapsed;
        } else if self.mode == SizeMode::Collapsed {
            self.mode = SizeMode::Expanded;
        }
    }
}

// ------------------------------------------------------------------ 命令表（F413）

/// 一条命令的完整规格（手册 F414 详情的单一数据源）。
#[derive(Debug)]
pub struct CmdSpec {
    /// 完整路径，如 `improve clean`。
    pub path: &'static str,
    /// 参数说明，如 `<节点>` `[目标]`。
    pub args: &'static str,
    pub summary: &'static str,
    pub perm: Perm,
    /// 手册八大分类之一。
    pub category: &'static str,
    /// 等效 UI 按钮（双通道的 UI 通道，F449）。
    pub ui: &'static str,
    /// 关联命令（F416 执行后推荐）。
    pub related: &'static [&'static str],
}

impl CmdSpec {
    /// 顶层命令名（`improve clean` → `improve`）。
    pub fn top(&self) -> &'static str {
        self.path.split(' ').next().unwrap_or(self.path)
    }

    pub fn sub(&self) -> Option<&'static str> {
        let mut it = self.path.split(' ');
        let _ = it.next();
        it.next()
    }

    pub fn usage(&self) -> String {
        if self.args.is_empty() {
            format!("/{}", self.path)
        } else {
            format!("/{} {}", self.path, self.args)
        }
    }

    /// 示例代码：无参一条，有参补一条带占位参数。
    pub fn examples(&self) -> Vec<String> {
        if self.args.is_empty() {
            return vec![format!("/{}", self.path)];
        }
        let placeholder: String =
            self.args.chars().filter(|c| !matches!(c, '<' | '>' | '[' | ']' | '|')).collect();
        let first = placeholder.split(' ').next().unwrap_or("");
        vec![format!("/{}", self.path), format!("/{} {}", self.path, first)]
    }
}

const fn cmd(
    path: &'static str,
    args: &'static str,
    summary: &'static str,
    perm: Perm,
    category: &'static str,
    ui: &'static str,
    related: &'static [&'static str],
) -> CmdSpec {
    CmdSpec { path, args, summary, perm, category, ui, related }
}

/// 主规格 §24「78 条命令完整列表（A-Z）」的落点：
/// 51 条顶层命令 + 27 条常用子命令 = **78 条主表**；
/// 另按 §27 双通道对照表补录 `/rename`、`/run`（主表 A-Z 段漏列但双通道表在用），
/// 且依总纲第六章第 3 条「`/help` 不过滤、不隐藏任何命令」全部登记，共 80 条。
pub const COMMANDS: &[CmdSpec] = &[
    // —— A ——
    cmd("add", "<for|if|log|nullcheck|try> <节点>", "在指定节点上插入结构（循环/判断/日志/空值检查/异常捕获）", Perm::Modify, "创作", "✨面板→生成器", &["delete", "inline", "template"]),
    // —— B ——
    cmd("bookmark", "add|list|remove <名称>", "书签：给节点打可跳转的标记", Perm::Modify, "导航", "📂面板→书签", &["mark", "goto", "locate"]),
    // —— C ——
    cmd("callers", "<函数>", "列出所有调用该函数的上游", Perm::Safe, "分析", "右键节点→调用者", &["callees", "track"]),
    cmd("callees", "<函数>", "列出该函数调用的所有下游", Perm::Safe, "分析", "右键节点→被调用", &["callers", "view dependency"]),
    cmd("collapse", "<节点>", "折叠节点子树", Perm::Safe, "视图", "节点左侧三角", &["expand", "level"]),
    cmd("continue", "", "继续上一次未完成的续写/改进", Perm::Modify, "创作", "✨面板→继续", &["add", "generate"]),
    cmd("convert", "<源语言> <目标语言>", "整块代码跨语言转换", Perm::Danger, "创作", "✨面板→转换", &["translate", "export"]),
    // —— D ——
    cmd("debug", "", "进入调试模式：断点/单步/变量监视", Perm::Safe, "分析", "🐛面板", &["run", "track"]),
    cmd("delete", "<节点>", "删除节点及其子树", Perm::Danger, "节点", "右键节点→删除", &["undo", "add"]),
    cmd("dict", "<概念>", "查概念词典：通俗解释 + 比喻", Perm::Safe, "翻译", "💬面板→词典", &["translate", "explain", "semantic"]),
    // —— E ——
    cmd("expand", "<节点>", "展开节点子树", Perm::Safe, "视图", "节点左侧三角", &["collapse", "level"]),
    cmd("explain", "error", "解释当前错误并给出修复建议", Perm::Safe, "翻译", "💬面板→解释", &["dict", "improve bugs"]),
    cmd("export", "png|svg|pdf|wallpaper", "导出当前画面/报告", Perm::Safe, "界面", "右键空白→导出", &["report", "save"]),
    cmd("export png", "", "导出当前画布为 PNG", Perm::Safe, "界面", "右键空白→导出PNG", &["export", "save"]),
    // —— F ——
    cmd("fps", "", "显示渲染帧率与性能预算", Perm::Safe, "系统", "状态栏 FPS", &["settings", "view heatmap"]),
    // —— G ——
    cmd("generate", "client|flowchart|orm|statemachine|switch|type", "按规格生成代码骨架", Perm::Modify, "创作", "✨面板→生成器", &["template", "add", "continue"]),
    cmd("generate flowchart", "", "由流程描述生成代码", Perm::Modify, "创作", "✨面板→生成器", &["generate", "view flowchart"]),
    cmd("generate type", "", "由 JSON 生成类型定义", Perm::Modify, "创作", "✨面板→生成器", &["generate", "convert"]),
    cmd("goto", "<名称>", "跳转到指定符号/文件", Perm::Safe, "导航", "📂面板→搜索", &["search", "locate", "bookmark"]),
    // —— H ——
    cmd("help", "[命令]", "分页列出全部命令；带参数时显示单命令文档", Perm::Safe, "系统", "📖手册 / Ctrl+H", &["manual", "dict"]),
    cmd("highlight", "<节点>", "高亮节点及其关联链路", Perm::Safe, "节点", "右键节点→高亮", &["select", "track"]),
    // —— I ——
    cmd("improve", "bugs|clean|comment|docs|readable|secure|simplify|speed|split|style|test|translate", "一键改进：12 种预设目标", Perm::Modify, "改进", "🔧改进面板", &["scan", "refactor", "undo"]),
    cmd("improve bugs", "[目标]", "一键查虫：定位并修复潜在缺陷", Perm::Modify, "改进", "🔧面板→🐛查虫", &["scan all", "improve secure"]),
    cmd("improve clean", "[目标]", "一键整洁：统一格式、去除冗余", Perm::Modify, "改进", "🔧面板→🧹整洁", &["improve style", "refactor"]),
    cmd("improve readable", "[目标]", "一键易读：命名与结构向人看齐", Perm::Modify, "改进", "🔧面板→📖易读", &["improve comment", "improve docs"]),
    cmd("improve secure", "[目标]", "一键加固：修补安全风险", Perm::Modify, "改进", "🔧面板→🔒加固", &["scan security", "improve bugs"]),
    cmd("improve simplify", "[目标]", "一键精简：压缩嵌套与重复", Perm::Modify, "改进", "🔧面板→✂️精简", &["refactor flatten", "improve split"]),
    cmd("improve speed", "[目标]", "一键加速：热点路径性能优化", Perm::Modify, "改进", "🔧面板→🚀加速", &["scan hot", "view heatmap"]),
    cmd("inline", "<函数>", "内联展开函数调用", Perm::Modify, "改进", "右键函数→内联", &["refactor extract", "improve simplify"]),
    // —— K ——
    cmd("keys", "list|search|set|clear|reset|preset|export|import|conflicts", "键位管理：查看/设置/导入导出/冲突检测", Perm::Modify, "系统", "⚙→键位", &["settings", "help"]),
    // —— L ——
    cmd("layout", "bottom|center|free|fullscreen|three|vscode", "切换整体布局", Perm::Modify, "界面", "⚙→布局", &["view fullscreen", "mode"]),
    cmd("level", "<1-7>", "切换七级下钻层级", Perm::Safe, "视图", "📂面板→滑块", &["expand", "collapse", "zoom"]),
    cmd("locate", "changed|issues|main", "定位到变更/问题/入口", Perm::Safe, "导航", "📂面板→定位", &["goto", "search"]),
    cmd("lock", "<节点>", "锁定节点结构，禁止改动", Perm::Modify, "节点", "右键节点→锁定", &["unlock", "save"]),
    // —— M ——
    cmd("manual", "", "打开完整操作手册", Perm::Safe, "系统", "📖手册 / Ctrl+H", &["help", "button"]),
    cmd("mark", "<名称>|list|clear|show", "标记与标记管理", Perm::Modify, "导航", "📂面板→标记", &["bookmark", "highlight"]),
    cmd("mode", "creative|spectator|survival", "切换三界面：专业/对照/通俗", Perm::Modify, "界面", "顶栏→三界面", &["translate", "theme"]),
    // —— O ——
    cmd("open", "<路径>", "打开本地项目", Perm::Safe, "系统", "📂面板→打开", &["project", "scan"]),
    // —— P ——
    cmd("project", "list|switch <名称>", "多项目管理与切换", Perm::Modify, "系统", "📂面板→项目", &["open", "search"]),
    // —— R ——
    cmd("redo", "", "重做上一步撤回", Perm::Modify, "系统", "底部↪", &["undo", "undo history"]),
    cmd("refactor", "async|dead|encapsulate|extract|flatten|split", "结构性重构", Perm::Modify, "改进", "🔧面板→重构", &["improve", "undo"]),
    cmd("rename", "<节点> <新名>", "重命名并同步全部引用", Perm::Modify, "节点", "右键节点→重命名 / F2", &["refactor", "search"]),
    cmd("report", "boss|complexity|health|security", "生成报告", Perm::Safe, "分析", "📊面板→报告", &["scan", "export"]),
    cmd("run", "", "运行/调试当前项目", Perm::Safe, "分析", "右键→运行 / F5", &["debug", "scan"]),
    // —— S ——
    cmd("save", "", "保存当前工程状态", Perm::Modify, "系统", "标题栏💾 / Ctrl+S", &["undo snapshot", "export"]),
    cmd("scan", "all|dead|hot|leak|null|race|security", "六类检测开关", Perm::Safe, "分析", "📊面板→检测", &["report", "improve"]),
    cmd("scan all", "", "运行全部检测", Perm::Safe, "分析", "📊面板→▶全部运行", &["scan", "report health"]),
    cmd("scan null", "", "空指针检测", Perm::Safe, "分析", "📊面板→☑空指针", &["scan all", "improve bugs"]),
    cmd("scan race", "", "数据竞争检测", Perm::Safe, "分析", "📊面板→☑竞争", &["scan all", "view concurrent"]),
    cmd("scope", "all|custom|feature|file|fn|framework|module", "设定分析作用域", Perm::Modify, "分析", "📊面板→作用域", &["scan", "level"]),
    cmd("search", "<关键词>", "全项目搜索", Perm::Safe, "导航", "📂面板→搜索", &["goto", "locate"]),
    cmd("select", "<节点>", "选中节点", Perm::Safe, "节点", "画布点击", &["highlight", "view"]),
    cmd("semantic", "<词>", "语义检索：按含义而非字面匹配", Perm::Safe, "分析", "💬面板→语义", &["search", "dict"]),
    cmd("settings", "", "打开设置", Perm::Modify, "系统", "⚙", &["keys", "theme"]),
    cmd("story", "", "按时间线回放项目演进", Perm::Safe, "系统", "状态栏→故事", &["undo time", "report"]),
    // —— T ——
    cmd("template", "crud|login|test", "套用工程模板", Perm::Modify, "创作", "✨面板→模板", &["generate", "add"]),
    cmd("template login", "", "生成登录流程模板", Perm::Modify, "创作", "✨面板→模板", &["template", "generate client"]),
    cmd("theme", "cyber|fresh|glass|modern|neu|pixel|sketch|star", "切换 8 种 UI 风格", Perm::Modify, "界面", "⚙→外观→风格", &["wallpaper", "mode"]),
    cmd("theme pixel", "", "切换像素风", Perm::Modify, "界面", "⚙→外观→风格", &["theme", "wallpaper"]),
    cmd("theme star", "", "切换星空风", Perm::Modify, "界面", "⚙→外观→风格", &["theme", "wallpaper"]),
    cmd("track", "<变量>", "追踪变量的完整生命周期", Perm::Safe, "分析", "右键变量 / F6", &["view dataflow", "view lifecycle"]),
    cmd("translate", "block|check|dual|file|fix|line|pro|simple", "通俗化翻译", Perm::Modify, "翻译", "💬面板→翻译", &["dict", "mode"]),
    cmd("tree", "animate|export|from|layout", "树根视图操作", Perm::Safe, "视图", "画布右键→树", &["view", "export"]),
    // —— U ——
    cmd("undo", "[N]|branch|history|snapshot|time", "撤回：单步/多步/快照/时间旅行", Perm::Modify, "系统", "底部↩ / Ctrl+Z", &["redo", "save"]),
    cmd("unlock", "<节点>", "解除节点锁定", Perm::Modify, "节点", "右键节点→解锁", &["lock", "improve"]),
    // —— V ——
    cmd("view", "animate|compare|concurrent|dataflow|dependency|exception|flowchart|framework|fullscreen|heatmap|lifecycle|mindmap|neural|xray", "切换可视化视图", Perm::Safe, "视图", "📂面板→视图", &["level", "zoom"]),
    cmd("view animate", "", "播放结构演化动画", Perm::Safe, "视图", "右键空白 / F12", &["view", "story"]),
    cmd("view compare", "", "对比模式：画布分屏", Perm::Safe, "视图", "📂面板→视图 / F9", &["view", "export"]),
    cmd("view concurrent", "", "并发视图", Perm::Safe, "视图", "📊面板 / F8", &["scan race", "view"]),
    cmd("view dataflow", "", "数据流视图", Perm::Safe, "视图", "状态栏标签 / F3", &["track", "view"]),
    cmd("view dependency", "", "依赖图", Perm::Safe, "视图", "右键空白 / Ctrl+Shift+5", &["callees", "view"]),
    cmd("view exception", "", "异常路径视图", Perm::Safe, "视图", "📊面板 / F7", &["improve bugs", "view"]),
    cmd("view flowchart", "", "2D 流程图", Perm::Safe, "视图", "右键节点 / Ctrl+Shift+2", &["generate flowchart", "view"]),
    cmd("view framework", "", "框架视图", Perm::Safe, "视图", "右键空白→框架 / F1", &["scope framework", "view"]),
    cmd("view fullscreen", "", "全屏", Perm::Safe, "视图", "标题栏按钮 / F11", &["layout fullscreen", "view"]),
    cmd("view heatmap", "", "热力图", Perm::Safe, "视图", "状态栏标签 / F4", &["scan hot", "view"]),
    cmd("view lifecycle", "", "生命周期视图", Perm::Safe, "视图", "右键变量 / F6", &["track", "view"]),
    cmd("view xray", "", "X 光模式：只看逻辑骨架", Perm::Safe, "视图", "📂面板→视图 / F10", &["view", "improve simplify"]),
    // —— W ——
    cmd("wallpaper", "<路径>|blur|clear|overlay", "设置应用内动态壁纸", Perm::Modify, "界面", "⚙→外观→背景", &["theme", "export wallpaper"]),
    // —— Z ——
    cmd("zoom", "<1-100>", "画布缩放", Perm::Safe, "视图", "滚轮 / 状态栏", &["level", "view fullscreen"]),
];

/// 手册八大分类（F422）。
pub const CATEGORIES: &[&str] = &["改进", "分析", "创作", "视图", "导航", "界面", "系统", "节点", "翻译"];

/// A-Z 排序后的命令索引（F413）。
pub fn command_index() -> Vec<&'static CmdSpec> {
    let mut v: Vec<&'static CmdSpec> = COMMANDS.iter().collect();
    v.sort_by(|a, b| a.path.cmp(b.path));
    v
}

/// 顶层命令数（去重）：主表 51 + 双通道补录 2 = 53。
pub fn top_level_count() -> usize {
    let mut tops: Vec<&str> = COMMANDS.iter().map(|c| c.top()).collect();
    tops.sort_unstable();
    tops.dedup();
    tops.len()
}

pub fn find(path: &str) -> Option<&'static CmdSpec> {
    let path = path.trim().trim_start_matches('/');
    COMMANDS.iter().find(|c| c.path == path)
}

/// F413 `/help` 分页：每页 20 条，按任意键翻页。
pub fn help_page(page: usize) -> Vec<&'static CmdSpec> {
    let idx = command_index();
    let start = page.saturating_mul(HELP_PAGE_SIZE);
    if start >= idx.len() {
        return Vec::new();
    }
    let end = (start + HELP_PAGE_SIZE).min(idx.len());
    idx[start..end].to_vec()
}

pub fn help_page_count() -> usize {
    (COMMANDS.len() + HELP_PAGE_SIZE - 1) / HELP_PAGE_SIZE
}

/// F414 详细帮助：用法 / 参数 / 示例 / 等效按钮 / 关联命令。
#[derive(Debug)]
pub struct HelpDoc {
    pub path: &'static str,
    pub summary: &'static str,
    pub usage: String,
    pub args: &'static str,
    pub examples: Vec<String>,
    pub ui: &'static str,
    pub related: Vec<&'static str>,
    pub perm: Perm,
}

impl HelpDoc {
    /// 渲染为手册详情面板文本。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("{} {}\n", self.perm.badge(), self.usage));
        s.push_str(&format!("─────────────────────\n用途: {}\n", self.summary));
        if !self.args.is_empty() {
            s.push_str(&format!("参数: {}\n", self.args));
        }
        s.push_str("\n示例:\n");
        for e in &self.examples {
            s.push_str(&format!("  {}\n", e));
        }
        s.push_str(&format!("\n等效按钮: {}\n", self.ui));
        s.push_str("关联命令: ");
        s.push_str(&self.related.iter().map(|r| format!("/{r}")).collect::<Vec<_>>().join(" "));
        s.push('\n');
        s
    }
}

pub fn help_detail(path: &str) -> Option<HelpDoc> {
    find(path).map(|c| HelpDoc {
        path: c.path,
        summary: c.summary,
        usage: c.usage(),
        args: c.args,
        examples: c.examples(),
        ui: c.ui,
        related: c.related.to_vec(),
        perm: c.perm,
    })
}

// ------------------------------------------------------------------ F415 实时推荐

/// 输入 `/` 后浮出 3-5 条推荐，半透明不挡视线，Tab 补全。
pub fn suggest(prefix: &str, limit: usize) -> Vec<&'static str> {
    let limit = limit.clamp(3, 5);
    let p = prefix.trim().trim_start_matches('/').to_lowercase();
    let idx = command_index();
    if p.is_empty() {
        return idx.iter().take(limit).map(|c| c.path).collect();
    }
    let mut out: Vec<&'static str> =
        idx.iter().filter(|c| c.path.starts_with(&p)).map(|c| c.path).collect();
    if out.is_empty() {
        // 退化为「最后一个词」匹配，支持 `/improve sp` 这类子命令补全
        let last = p.rsplit(' ').next().unwrap_or("");
        let head = p.strip_suffix(last).unwrap_or("").trim_end();
        out = idx
            .iter()
            .filter(|c| c.path.starts_with(head) && c.path[head.len()..].starts_with(last))
            .map(|c| c.path)
            .collect();
    }
    if out.is_empty() {
        // 再退化为包含匹配
        out = idx.iter().filter(|c| c.path.contains(&p)).map(|c| c.path).collect();
    }
    out.truncate(limit);
    out
}

/// Tab 补全：唯一命中补全称，多命中补最长公共前缀。
pub fn tab_complete(input: &str) -> String {
    let trimmed = input.trim_start_matches('/');
    let has_slash = input.starts_with('/');
    let cands = suggest(trimmed, 5);
    if cands.is_empty() {
        return input.to_string();
    }
    if cands.len() == 1 {
        let full = cands[0];
        return if full.len() > trimmed.len() {
            format!("{}{} ", if has_slash { "/" } else { "" }, full)
        } else {
            input.to_string()
        };
    }
    let lcp = longest_common_prefix(&cands);
    if lcp.len() > trimmed.len() {
        format!("{}{}", if has_slash { "/" } else { "" }, lcp)
    } else {
        input.to_string()
    }
}

fn longest_common_prefix(items: &[&str]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut prefix = items[0].to_string();
    for it in &items[1..] {
        while !it.starts_with(&prefix) {
            prefix.pop();
            if prefix.is_empty() {
                return String::new();
            }
        }
    }
    prefix
}

// ------------------------------------------------------------------ F416 关联推荐

/// 执行后结果下方淡灰显示 2-3 条关联命令。
pub fn related_after(path: &str, limit: usize) -> Vec<&'static str> {
    let limit = limit.clamp(2, 3);
    let known = match find(path) {
        Some(c) => c.related.to_vec(),
        None => Vec::new(),
    };
    let mut out: Vec<&'static str> = known
        .into_iter()
        .filter(|r| find(r).is_some() && *r != path.trim_start_matches('/'))
        .collect();
    if out.len() < limit {
        // 同顶层命令的兄弟子命令补齐
        let top = path.trim_start_matches('/').split(' ').next().unwrap_or("");
        for c in COMMANDS {
            if out.len() >= limit {
                break;
            }
            if c.top() == top && c.path != path.trim_start_matches('/') && !out.contains(&c.path) {
                out.push(c.path);
            }
        }
    }
    out.truncate(limit);
    out
}

// ------------------------------------------------------------------ F417 纠正推荐

/// Levenshtein 编辑距离（确定性 DP）。
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

/// 未知命令 → 推荐最接近的正确命令（距离阈值：len<6 允许 2，否则 len/3）。
pub fn correct(input: &str) -> Option<&'static str> {
    let q = input.trim().trim_start_matches('/').to_lowercase();
    if q.is_empty() {
        return None;
    }
    let len = q.chars().count();
    let budget = if len < 6 { 2 } else { len / 3 };
    let mut best: Option<(&'static str, usize)> = None;
    for c in command_index() {
        let d = levenshtein(&q, c.path);
        if d <= budget {
            match best {
                Some((_, bd)) if bd <= d => {}
                _ => best = Some((c.path, d)),
            }
        }
    }
    best.map(|(p, _)| p)
}

/// 未知命令的错误提示行（输出给终端）。
pub fn unknown_command_line(input: &str) -> String {
    match correct(input) {
        Some(c) => format!("未知命令 /{}，你是否想输入 /{}？", input.trim_start_matches('/'), c),
        None => format!("未知命令 /{}，输入 /help 查看全部命令", input.trim_start_matches('/')),
    }
}

// ------------------------------------------------------------------ F418 首次引导

/// 一次性快速开始：新用户首次打开显示，之后不再出现。
pub fn first_run_guide(seen: bool, theme: Theme) -> Vec<String> {
    if seen {
        return Vec::new();
    }
    let term_key = match theme {
        Theme::Pixel => "T",
        _ => "反引号 `",
    };
    vec![
        "§a欢迎进入代码世界 v1.0".to_string(),
        format!("§7按 §f{} §7或 §fCtrl+J §7打开命令终端", term_key),
        "§7输入 §f/help §7查看所有命令".to_string(),
        "§7输入 §f/help <命令> §7查看用法".to_string(),
        "§7拖入项目文件夹，或输入 §f/open <路径> §7开始".to_string(),
    ]
}

// ------------------------------------------------------------------ F420 命令管道

/// 按 `&&` 切分管道（不解析引号内的 &&，保持命令原文）。
pub fn split_pipeline(line: &str) -> Vec<String> {
    line.split("&&").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

#[derive(Debug)]
pub struct PipelineResult {
    /// 已成功执行的命令。
    pub executed: Vec<String>,
    /// 中断的命令（None 表示全部成功）。
    pub failed_at: Option<String>,
    pub ok: bool,
}

/// 顺序执行，前一步失败即中断（`/scan all && /improve clean && /report health`）。
pub fn run_pipeline<F: FnMut(&str) -> bool>(line: &str, f: &mut F) -> PipelineResult {
    let steps = split_pipeline(line);
    let mut executed = Vec::new();
    for step in steps {
        if f(&step) {
            executed.push(step);
        } else {
            return PipelineResult { executed, failed_at: Some(step), ok: false };
        }
    }
    PipelineResult { executed, failed_at: None, ok: true }
}

/// 解析一行输入：命中命令 → Ok(规格, 实参)；未命中 → Err(纠正建议)。
pub fn parse_input(line: &str) -> Result<(&'static CmdSpec, Vec<String>), Option<&'static str>> {
    let trimmed = line.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(None);
    }
    let mut parts: Vec<String> = trimmed.split_whitespace().map(|s| s.to_string()).collect();
    // 先尝试「顶层 + 子命令」双词命中（improve clean / scan null / theme pixel）
    if parts.len() >= 2 {
        let two = format!("{} {}", parts[0], parts[1]);
        if let Some(c) = find(&two) {
            parts.drain(0..2);
            return Ok((c, parts));
        }
    }
    if let Some(c) = find(&parts[0]) {
        parts.remove(0);
        return Ok((c, parts));
    }
    Err(correct(trimmed))
}

// ------------------------------------------------------------------ 自检

/// AI-06 域一自检（#411~#420，10 项）。
pub fn run_cmd_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("cmd");

    // F411
    let mut t = Terminal::new();
    for i in 0..520 {
        t.push(LineKind::Output, format!("line{i}"));
    }
    let vis = t.visible(10);
    s.add(
        "F411 终端UI框架",
        t.len() == MAX_HISTORY && vis.len() == 10 && vis[9].text == "line519" && t.lines[0].text == "line20",
        "500 条上限 + 自动滚到底部 + 半透明底",
    );

    // F412
    let mut v = TerminalView::new(Theme::Pixel);
    let k1 = v.handle_key("T", false);
    let mut v2 = TerminalView::new(Theme::Modern);
    let k2 = v2.handle_key("`", false);
    let k3 = v2.handle_key("j", true);
    let m1 = v2.mode;
    let k4 = v2.handle_key("Escape", false);
    let k5 = v2.handle_key("Enter", false);
    s.add(
        "F412 终端开关",
        k1 == KeyAction::Open
            && v.open
            && k2 == KeyAction::Open
            && k3 == KeyAction::CycleSize
            && m1 == SizeMode::Large
            && k4 == KeyAction::Close
            && !v2.open
            && k5 == KeyAction::None
            && SizeMode::Collapsed.height_px() == 36
            && SizeMode::Expanded.rows() == 4
            && SizeMode::Large.height_px() == 300,
        "T/反引号/Ctrl+J 打开，Esc 关闭，Enter 执行",
    );

    // F413
    let idx = command_index();
    let p0 = help_page(0);
    let p_last = help_page(help_page_count() - 1);
    let sorted_ok = idx.windows(2).all(|w| w[0].path <= w[1].path);
    s.add(
        "F413 78条完整命令",
        COMMANDS.len() >= 78 && top_level_count() == 53 && p0.len() == HELP_PAGE_SIZE && !p_last.is_empty() && sorted_ok,
        "A-Z 排列 + /help 每页 20 条",
    );

    // F414
    let d = help_detail("improve speed").expect("improve speed 必须存在");
    let doc = d.render();
    s.add(
        "F414 详细帮助",
        d.usage == "/improve speed [目标]"
            && d.examples.len() == 2
            && doc.contains("用途:")
            && doc.contains("等效按钮:")
            && doc.contains("关联命令:")
            && help_detail("nope").is_none(),
        "用法/参数/示例/等效按钮/关联命令",
    );

    // F415
    let sug = suggest("imp", 5);
    let comp = tab_complete("/improve sp");
    let comp1 = tab_complete("/zoom");
    s.add(
        "F415 实时推荐",
        sug.len() >= 3 && sug.len() <= 5 && sug.iter().all(|x| x.starts_with("imp")) && comp == "/improve speed " && comp1 == "/zoom",
        "3-5 条推荐 + Tab 补全",
    );

    // F416
    let rel = related_after("scan null", 3);
    s.add(
        "F416 关联推荐",
        rel.len() >= 2 && rel.len() <= 3 && rel.iter().all(|r| find(r).is_some()) && !rel.contains(&"scan null"),
        "执行后 2-3 条关联命令",
    );

    // F417
    let c1 = correct("/improv");
    let c2 = correct("/scna");
    let c3 = correct("zzzzzzzzzzzz");
    s.add(
        "F417 纠正推荐",
        c1 == Some("improve") && c2 == Some("scan") && c3.is_none() && levenshtein("kitten", "sitting") == 3,
        "编辑距离推荐最接近命令",
    );

    // F418
    let g1 = first_run_guide(false, Theme::Pixel);
    let g2 = first_run_guide(true, Theme::Pixel);
    s.add(
        "F418 首次引导",
        g1.len() == 5 && g1[1].contains("T") && g2.is_empty(),
        "一次性快速开始",
    );

    // F419
    let safe = find("help").unwrap().perm;
    let modify = find("improve clean").unwrap().perm;
    let danger = find("delete").unwrap().perm;
    s.add(
        "F419 权限分级",
        safe == Perm::Safe
            && !safe.needs_confirm()
            && modify == Perm::Modify
            && danger == Perm::Danger
            && danger.needs_confirm()
            && danger.confirm("Y")
            && !danger.confirm("n")
            && Perm::Safe.confirm(""),
        "🟢只读 / 🟡可撤销 / 🔴需 Y/N 确认",
    );

    // F420
    let steps = split_pipeline("/scan all && /improve clean && /report health");
    let mut seen: Vec<String> = Vec::new();
    let r1 = run_pipeline("/scan all && /bogus && /report health", &mut |c: &str| {
        seen.push(c.to_string());
        find(c.trim_start_matches('/')).is_some()
    });
    let mut n = 0;
    let r2 = run_pipeline("/scan all && /improve clean", &mut |_| {
        n += 1;
        true
    });
    s.add(
        "F420 命令管道",
        steps.len() == 3
            && r1.executed == vec!["/scan all".to_string()]
            && r1.failed_at.as_deref() == Some("/bogus")
            && !r1.ok
            && r2.ok
            && r2.executed.len() == 2
            && n == 2,
        "&& 连接，顺序执行，失败即中断",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f411_history_cap() {
        let mut t = Terminal::new();
        for i in 0..MAX_HISTORY + 10 {
            t.push(LineKind::System, i.to_string());
        }
        assert_eq!(t.len(), MAX_HISTORY);
        assert_eq!(t.lines[0].text, "10");
    }

    #[test]
    fn f412_cycle_wraps() {
        let mut m = SizeMode::Collapsed;
        for _ in 0..4 {
            m = m.cycle();
        }
        assert_eq!(m, SizeMode::Collapsed);
    }

    #[test]
    fn f413_pagination_covers_all() {
        let mut n = 0;
        for p in 0..help_page_count() {
            n += help_page(p).len();
        }
        assert_eq!(n, COMMANDS.len());
    }

    #[test]
    fn f414_unknown_returns_none() {
        assert!(help_detail("not-a-command").is_none());
        assert!(help_detail("/help").is_some());
    }

    #[test]
    fn f415_empty_prefix_lists_head() {
        let s = suggest("", 3);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn f417_distance_symmetric() {
        assert_eq!(levenshtein("abc", "abcd"), levenshtein("abcd", "abc"));
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn f418_only_once() {
        assert!(!first_run_guide(false, Theme::Star).is_empty());
        assert!(first_run_guide(true, Theme::Star).is_empty());
    }

    #[test]
    fn f419_danger_requires_confirm() {
        assert!(Perm::Danger.needs_confirm());
        assert!(Perm::Danger.confirm("yes"));
        assert!(!Perm::Danger.confirm(""));
        assert!(Perm::Modify.confirm(""));
    }

    #[test]
    fn f420_pipeline_empty() {
        let mut n = 0;
        let r = run_pipeline("   ", &mut |_| {
            n += 1;
            true
        });
        assert!(r.ok && r.executed.is_empty() && n == 0);
    }

    #[test]
    fn parse_input_resolves_subcommand() {
        let (c, args) = parse_input("/improve speed auth()").unwrap();
        assert_eq!(c.path, "improve speed");
        assert_eq!(args, vec!["auth()".to_string()]);
        assert!(parse_input("/nope").is_err());
    }
}
