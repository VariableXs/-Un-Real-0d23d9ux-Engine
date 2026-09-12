//! 手册界面 + 自定义按钮（#421~#432）—— AI-06 域二。
//!
//! 零 AI：搜索/分类/收藏/按钮序列化全为确定性算法。
//! 手册内容单一数据源来自 `cmd::COMMANDS`（命令表三端同一份，部署总纲第三章）。

use crate::cmd::{self, CmdSpec};

// ------------------------------------------------------------------ F421 手册界面

/// 手册浮层尺寸（宽 800px 高 600px，半透明遮罩背景）。
pub const MANUAL_W: u32 = 800;
pub const MANUAL_H: u32 = 600;

/// 手册显示模式：通俗 / 专业 / 双屏 / 自动（跟随当前界面模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualMode {
    Plain,
    Pro,
    Dual,
    Auto,
}

impl ManualMode {
    pub fn label(self) -> &'static str {
        match self {
            ManualMode::Plain => "[通俗]",
            ManualMode::Pro => "[专业]",
            ManualMode::Dual => "[双屏]",
            ManualMode::Auto => "[自动]",
        }
    }

    /// Auto 跟随当前三界面：survival→通俗，creative→专业，spectator→双屏。
    pub fn resolve(self, iface_mode: &str) -> ManualMode {
        match self {
            ManualMode::Auto => match iface_mode {
                "creative" => ManualMode::Pro,
                "spectator" => ManualMode::Dual,
                _ => ManualMode::Plain,
            },
            other => other,
        }
    }
}

/// F421 手册独立界面：`/manual` 或 Ctrl+H 打开，浮层覆盖画布。
#[derive(Debug)]
pub struct Manual {
    pub open: bool,
    pub mode: ManualMode,
    pub w: u32,
    pub h: u32,
    /// 半透明遮罩背景。
    pub mask: bool,
}

impl Manual {
    pub fn new() -> Self {
        Manual { open: false, mode: ManualMode::Auto, w: MANUAL_W, h: MANUAL_H, mask: true }
    }

    /// 打开热键：Ctrl+H（H 大小写均可）。
    pub fn is_hotkey(key: &str, ctrl: bool) -> bool {
        ctrl && (key == "H" || key == "h")
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn handle_key(&mut self, key: &str, ctrl: bool) -> bool {
        if Self::is_hotkey(key, ctrl) {
            self.toggle();
            return true;
        }
        false
    }
}

impl Default for Manual {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ F422 分类浏览

/// 手册分类（按主规格手册布局图逐栏列出，不做删减）。
pub const CATEGORIES: &[&str] =
    &["改进", "分析", "创作", "视图", "导航", "界面", "系统", "节点", "翻译"];

/// 分类 → 命令列表。
pub fn browse(cat: &str) -> Vec<&'static CmdSpec> {
    cmd::command_index().into_iter().filter(|c| c.category == cat).collect()
}

/// 分类 → (分类名, 条数)，手册左侧括号内显示数量。
pub fn category_counts() -> Vec<(&'static str, usize)> {
    CATEGORIES.iter().map(|c| (*c, browse(c).len())).collect()
}

// ------------------------------------------------------------------ F423 手册搜索

/// 实时过滤：中英文均可，匹配命令名、摘要、分类。
pub fn search(q: &str) -> Vec<&'static CmdSpec> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return cmd::command_index();
    }
    cmd::command_index()
        .into_iter()
        .filter(|c| {
            c.path.to_lowercase().contains(&q)
                || c.summary.to_lowercase().contains(&q)
                || c.category.contains(&q)
        })
        .collect()
}

/// 命中片段高亮（大小写不敏感）。
pub fn highlight(text: &str, q: &str) -> String {
    if q.is_empty() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let needle = q.to_lowercase();
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(rel) = lower[i..].find(&needle) {
        let start = i + rel;
        out.push_str(&text[i..start]);
        out.push_str("<mark>");
        out.push_str(&text[start..start + needle.len()]);
        out.push_str("</mark>");
        i = start + needle.len();
    }
    out.push_str(&text[i..]);
    out
}

// ------------------------------------------------------------------ F424/F425 收藏与最近

/// 手册状态：收藏、最近命令、固定项。
#[derive(Debug, Default)]
pub struct ManualState {
    /// 收藏（⭐），保持插入顺序、去重。
    pub fav: Vec<String>,
    /// 最近命令，最近 10 条，时间倒序（下标 0 最新）。
    pub recent: Vec<String>,
}

impl ManualState {
    pub fn new() -> Self {
        Self::default()
    }

    /// F424 收藏/取消收藏。
    pub fn toggle_fav(&mut self, path: &str) -> bool {
        match self.fav.iter().position(|p| p == path) {
            Some(i) => {
                self.fav.remove(i);
                false
            }
            None => {
                self.fav.push(path.to_string());
                true
            }
        }
    }

    pub fn is_fav(&self, path: &str) -> bool {
        self.fav.iter().any(|p| p == path)
    }

    pub fn favorites(&self) -> Vec<&str> {
        self.fav.iter().map(|s| s.as_str()).collect()
    }

    /// F425 记录最近命令：去重 + 提到最前 + 截断 10 条。
    pub fn touch_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(10);
    }

    pub fn recents(&self) -> Vec<&str> {
        self.recent.iter().map(|s| s.as_str()).collect()
    }
}

// ------------------------------------------------------------------ F421/F426 详情渲染与执行

/// 按显示模式渲染命令详情。
pub fn render_detail(state: &Manual, iface_mode: &str, path: &str) -> Option<String> {
    let doc = cmd::help_detail(path)?;
    let mode = state.mode.resolve(iface_mode);
    let mut s = String::new();
    match mode {
        ManualMode::Plain => {
            s.push_str(&format!("/{}\n", doc.path));
            s.push_str(&format!("────────────────────\n大白话: {}\n", doc.summary));
            s.push_str(&format!("怎么点: {}\n", doc.ui));
        }
        ManualMode::Pro => {
            s.push_str(&format!("{}\n", doc.usage));
            s.push_str(&format!("────────────────────\n参数: {}\n", doc.args));
            s.push_str(&format!("权限: {} {}\n", doc.perm.badge(), doc.perm.as_str()));
            for e in &doc.examples {
                s.push_str(&format!("  {}\n", e));
            }
        }
        ManualMode::Dual => {
            s.push_str(&format!("/{}\n", doc.path));
            s.push_str(&format!("────────────────────\n大白话: {}\n", doc.summary));
            s.push_str(&format!("技术: {}\n", doc.usage));
            s.push_str(&format!("等效按钮: {}\n", doc.ui));
        }
        ManualMode::Auto => unreachable!("Auto 已被 resolve 消解"),
    }
    s.push_str(&format!(
        "关联命令: {}\n",
        doc.related.iter().map(|r| format!("/{r}")).collect::<Vec<_>>().join(" ")
    ));
    Some(s)
}

/// F426 一键执行：手册内点 ▶ 直接运行命令，结果输出到终端。
/// 返回是否执行成功（命令存在且非危险命令自动放行；危险命令需先确认）。
pub fn execute(
    state: &mut ManualState,
    term: &mut crate::cmd::Terminal,
    path: &str,
    confirmed: bool,
) -> bool {
    let Some(spec) = cmd::find(path) else {
        term.push(crate::cmd::LineKind::Error, cmd::unknown_command_line(path));
        return false;
    };
    term.push(crate::cmd::LineKind::Input, format!("> /{}", spec.path));
    if spec.perm.needs_confirm() && !confirmed {
        term.push(
            crate::cmd::LineKind::Error,
            format!("{} /{} 为危险操作，需确认 Y/N", spec.perm.badge(), spec.path),
        );
        return false;
    }
    state.touch_recent(spec.path);
    term.push(crate::cmd::LineKind::Output, format!("{} 已执行：{}", spec.perm.badge(), spec.summary));
    // F416：结果下方补 2-3 条关联命令（淡灰提示行）
    let rel = cmd::related_after(spec.path, 3);
    if !rel.is_empty() {
        term.push(
            crate::cmd::LineKind::Hint,
            format!("关联：{}", rel.iter().map(|r| format!("/{r}")).collect::<Vec<_>>().join("  ")),
        );
    }
    true
}

// ------------------------------------------------------------------ F427~F430 自定义按钮

/// F429 按钮放置位置（5 种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    BottomToolbar,
    ActivityBar,
    CanvasEdge,
    Sidebar,
    Floating,
}

impl Place {
    pub fn as_str(self) -> &'static str {
        match self {
            Place::BottomToolbar => "bottom-toolbar",
            Place::ActivityBar => "activity-bar",
            Place::CanvasEdge => "canvas-edge",
            Place::Sidebar => "sidebar",
            Place::Floating => "floating",
        }
    }

    pub fn parse(s: &str) -> Option<Place> {
        match s {
            "bottom-toolbar" | "bottom" => Some(Place::BottomToolbar),
            "activity-bar" | "activity" => Some(Place::ActivityBar),
            "canvas-edge" | "canvas" => Some(Place::CanvasEdge),
            "sidebar" => Some(Place::Sidebar),
            "floating" => Some(Place::Floating),
            _ => None,
        }
    }

    pub fn all() -> &'static [Place] {
        &[
            Place::BottomToolbar,
            Place::ActivityBar,
            Place::CanvasEdge,
            Place::Sidebar,
            Place::Floating,
        ]
    }
}

/// F430 图标来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Emoji,
    Pixel,
    Custom,
}

impl IconKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IconKind::Emoji => "emoji",
            IconKind::Pixel => "pixel",
            IconKind::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<IconKind> {
        match s {
            "emoji" => Some(IconKind::Emoji),
            "pixel" => Some(IconKind::Pixel),
            "custom" => Some(IconKind::Custom),
            _ => None,
        }
    }
}

/// F428~F430 自定义按钮。
#[derive(Debug, Clone, PartialEq)]
pub struct CustomButton {
    pub id: String,
    pub command: String,
    pub label: String,
    pub icon: IconKind,
    pub icon_src: String,
    pub color: String,
    /// 仅允许 24 / 32 / 48。
    pub size: u32,
    pub place: Place,
}

impl CustomButton {
    /// 尺寸归一到 24/32/48（非法值取最接近档）。
    pub fn normalize_size(size: u32) -> u32 {
        if size <= 28 {
            24
        } else if size <= 40 {
            32
        } else {
            48
        }
    }

    pub fn with_size(mut self, size: u32) -> Self {
        self.size = Self::normalize_size(size);
        self
    }

    /// 外观摘要（图标+颜色+大小+标签）。
    pub fn look(&self) -> String {
        format!("{}({}) {} {}px [{:?}]", self.label, self.icon_src, self.color, self.size, self.place)
    }
}

/// F428 按钮创建方式（四种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateWay {
    /// 手册内 📌 固定
    Manual,
    /// 命令行 `/button create`
    Command,
    /// 画布右键创建
    RightClick,
    /// 拖拽命令到工具栏
    Drag,
}

impl CreateWay {
    pub fn as_str(self) -> &'static str {
        match self {
            CreateWay::Manual => "manual",
            CreateWay::Command => "command",
            CreateWay::RightClick => "rightclick",
            CreateWay::Drag => "drag",
        }
    }
}

/// F431 按钮管理器（`/button list|create|remove|export|import`）。
#[derive(Debug, Default)]
pub struct ButtonManager {
    pub buttons: Vec<CustomButton>,
    seq: usize,
}

impl ButtonManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// F428 创建按钮，返回 id。
    pub fn create(
        &mut self,
        command: &str,
        label: &str,
        icon: IconKind,
        icon_src: &str,
        color: &str,
        size: u32,
        place: Place,
        _way: CreateWay,
    ) -> String {
        self.seq += 1;
        let id = format!("btn-{}", self.seq);
        self.buttons.push(CustomButton {
            id: id.clone(),
            command: command.to_string(),
            label: label.to_string(),
            icon,
            icon_src: icon_src.to_string(),
            color: color.to_string(),
            size: CustomButton::normalize_size(size),
            place,
        });
        id
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.buttons.len();
        self.buttons.retain(|b| b.id != id);
        self.buttons.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&CustomButton> {
        self.buttons.iter().find(|b| b.id == id)
    }

    pub fn list(&self) -> Vec<&CustomButton> {
        self.buttons.iter().collect()
    }

    pub fn by_place(&self, place: Place) -> Vec<&CustomButton> {
        self.buttons.iter().filter(|b| b.place == place).collect()
    }

    /// F431 `/button <子命令> ...` 的统一入口，返回终端输出文本。
    pub fn run_button_cmd(&mut self, args: &[&str]) -> String {
        match args.first().copied().unwrap_or("list") {
            "list" => {
                if self.buttons.is_empty() {
                    return "（无自定义按钮）".to_string();
                }
                self.buttons
                    .iter()
                    .map(|b| format!("{} /{} — {}", b.id, b.command, b.look()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "create" => {
                // /button create <命令> <标签> [emoji|pixel|custom] <图标> <颜色> <24|32|48> <位置>
                if args.len() < 3 {
                    return "用法: /button create <命令> <标签> [图标类型 图标 颜色 大小 位置]".to_string();
                }
                let command = args[1];
                let label = args[2];
                let icon = args.get(3).and_then(|s| IconKind::parse(s)).unwrap_or(IconKind::Emoji);
                let icon_src = args.get(4).copied().unwrap_or("▶");
                let color = args.get(5).copied().unwrap_or("#007AFF");
                let size = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(32);
                let place = args.get(7).and_then(|s| Place::parse(s)).unwrap_or(Place::BottomToolbar);
                let id = self.create(command, label, icon, icon_src, color, size, place, CreateWay::Command);
                format!("已创建按钮 {id} → /{command}")
            }
            "remove" => match args.get(1) {
                Some(id) => {
                    if self.remove(id) {
                        format!("已删除按钮 {id}")
                    } else {
                        format!("未找到按钮 {id}")
                    }
                }
                None => "用法: /button remove <id>".to_string(),
            },
            "export" => export_json(&self.buttons),
            "import" => match args.get(1) {
                Some(json) => {
                    let n = self.import_pack(json);
                    format!("已导入 {n} 个按钮")
                }
                None => "用法: /button import <json>".to_string(),
            },
            other => format!("未知子命令 {other}，可用: list|create|remove|export|import"),
        }
    }

    /// F432 导入按钮包（返回新增条数）。
    pub fn import_pack(&mut self, json: &str) -> usize {
        let incoming = import_json(json);
        let n = incoming.len();
        for b in incoming {
            self.buttons.push(b);
        }
        n
    }
}

// ------------------------------------------------------------------ F432 JSON 分享

fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// F432 导出为 JSON（按钮分享 / 社区按钮库）。
pub fn export_json(buttons: &[CustomButton]) -> String {
    let mut s = String::from("{\"kind\":\"ca-buttons\",\"v\":1,\"buttons\":[");
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":\"{}\",\"command\":\"{}\",\"label\":\"{}\",\"icon\":\"{}\",\"icon_src\":\"{}\",\"color\":\"{}\",\"size\":{},\"place\":\"{}\"}}",
            esc(&b.id),
            esc(&b.command),
            esc(&b.label),
            b.icon.as_str(),
            esc(&b.icon_src),
            esc(&b.color),
            b.size,
            b.place.as_str()
        ));
    }
    s.push_str("]}");
    s
}

/// 极简扁平 JSON 解析（无第三方依赖）：读取 `buttons` 数组内每项的字段。
pub fn import_json(json: &str) -> Vec<CustomButton> {
    let mut out = Vec::new();
    let bytes: Vec<char> = json.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != '{' {
            i += 1;
            continue;
        }
        let mut depth = 0usize;
        let start = i;
        while i < bytes.len() {
            match bytes[i] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let obj: String = bytes[start..i.min(bytes.len())].iter().collect();
        if let Some(b) = parse_button_obj(&obj) {
            out.push(b);
        }
    }
    out
}

fn field<'a>(obj: &'a str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":");
    let pos = obj.find(&pat)?;
    let rest = &obj[pos + pat.len()..];
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let mut s = String::new();
        let mut chars = stripped.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => {
                    if let Some(n) = chars.next() {
                        match n {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        }
                    }
                }
                _ => s.push(c),
            }
        }
        Some(s)
    } else {
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn parse_button_obj(obj: &str) -> Option<CustomButton> {
    let command = field(obj, "command")?;
    if command.is_empty() || command == "ca-buttons" {
        return None;
    }
    let label = field(obj, "label").unwrap_or_else(|| command.clone());
    let icon = field(obj, "icon").and_then(|s| IconKind::parse(&s)).unwrap_or(IconKind::Emoji);
    let icon_src = field(obj, "icon_src").unwrap_or_else(|| "▶".to_string());
    let color = field(obj, "color").unwrap_or_else(|| "#007AFF".to_string());
    let size = field(obj, "size").and_then(|s| s.parse().ok()).unwrap_or(32);
    let place = field(obj, "place").and_then(|s| Place::parse(&s)).unwrap_or(Place::BottomToolbar);
    let id = field(obj, "id").unwrap_or_else(|| format!("btn-{}", command));
    Some(CustomButton { id, command, label, icon, icon_src, color, size, place })
}

/// F432 社区按钮库：随产品自带的常用按钮包。
pub fn community_library() -> Vec<CustomButton> {
    let mk = |command: &str, label: &str, icon_src: &str, size: u32, place: Place| CustomButton {
        id: format!("lib-{command}"),
        command: command.to_string(),
        label: label.to_string(),
        icon: IconKind::Emoji,
        icon_src: icon_src.to_string(),
        color: "#007AFF".to_string(),
        size,
        place,
    };
    vec![
        mk("scan all", "全部检测", "▶", 32, Place::BottomToolbar),
        mk("improve clean", "一键整洁", "🧹", 32, Place::BottomToolbar),
        mk("improve speed", "一键加速", "🚀", 32, Place::BottomToolbar),
        mk("report health", "健康报告", "📊", 24, Place::Sidebar),
    ]
}

/// F427 固定到工具栏：把手册里的命令变成自定义按钮（默认底部工具栏）。
pub fn pin(state: &mut ButtonManager, path: &str, place: Place) -> Option<String> {
    let spec = cmd::find(path)?;
    Some(state.create(
        spec.path,
        spec.summary.split('：').next().unwrap_or(spec.path),
        IconKind::Emoji,
        "📌",
        "#FFAB00",
        32,
        place,
        CreateWay::Manual,
    ))
}

// ------------------------------------------------------------------ 自检

/// AI-06 域二自检（#421~#432，12 项）。
pub fn run_manual_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    use crate::cmd::{LineKind, Terminal};
    let mut s = CheckSet::new("manual");

    // F421
    let mut m = Manual::new();
    let h1 = m.handle_key("h", true);
    let opened = m.open;
    let h2 = m.handle_key("h", false);
    m.mode = ManualMode::Auto;
    let auto_plain = m.mode.resolve("survival");
    let auto_pro = m.mode.resolve("creative");
    let auto_dual = m.mode.resolve("spectator");
    s.add(
        "F421 手册独立界面",
        h1 && opened && !h2 && m.w == 800 && m.h == 600 && m.mask
            && auto_plain == ManualMode::Plain
            && auto_pro == ManualMode::Pro
            && auto_dual == ManualMode::Dual,
        "Ctrl+H 浮层 800x600 + 通俗/专业/双屏/自动四模式",
    );

    // F422
    let counts = category_counts();
    let improve = browse("改进");
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    s.add(
        "F422 分类浏览",
        counts.len() == CATEGORIES.len() && !improve.is_empty() && improve.iter().all(|c| c.category == "改进") && total == cmd::COMMANDS.len(),
        "8+ 分类，点击展开子命令，括号内显示数量",
    );

    // F423
    let r_cn = search("改进");
    let r_en = search("SCAN");
    let hl = highlight("improve speed", "speed");
    s.add(
        "F423 手册搜索",
        !r_cn.is_empty() && r_en.iter().any(|c| c.path.starts_with("scan")) && hl == "improve <mark>speed</mark>",
        "中英文实时过滤 + 命中高亮",
    );

    // F424
    let mut st = ManualState::new();
    let fav_on = st.toggle_fav("scan all");
    let fav_off = st.toggle_fav("scan all");
    let fav_empty = st.favorites().is_empty();
    let fav_zoom = st.toggle_fav("zoom");
    let fav_list = st.favorites();
    s.add(
        "F424 收藏命令",
        fav_on && !fav_off && !st.is_fav("scan all") && fav_empty && fav_zoom && fav_list == vec!["zoom"],
        "⭐ 收藏/取消，出现在左侧收藏区",
    );

    // F425
    let mut st2 = ManualState::new();
    for i in 0..12 {
        st2.touch_recent(&format!("c{i}"));
    }
    st2.touch_recent("c5");
    s.add(
        "F425 最近命令",
        st2.recent.len() == 10 && st2.recent[0] == "c5" && st2.recent[1] == "c11" && !st2.recent.contains(&"c0".to_string()),
        "最近 10 条，按时间倒序",
    );

    // F426
    let mut st3 = ManualState::new();
    let mut term = Terminal::new();
    let ok = execute(&mut st3, &mut term, "scan all", false);
    let last = term.lines.last().cloned();
    let blocked = execute(&mut st3, &mut term, "delete", false);
    let passed = execute(&mut st3, &mut term, "delete", true);
    s.add(
        "F426 一键执行",
        ok
            && term.lines[0].text == "> /scan all"
            && matches!(last.as_ref().map(|l| l.kind), Some(LineKind::Hint))
            && !blocked
            && passed
            && st3.recents()[0] == "delete",
        "手册 ▶ 直接运行，结果输出到终端；危险命令需确认",
    );

    // F427
    let mut bm = ButtonManager::new();
    let id = pin(&mut bm, "improve clean", Place::BottomToolbar);
    let pinned = bm.get(id.as_deref().unwrap_or("")).map(|b| (b.command.as_str(), b.place));
    s.add(
        "F427 固定到工具栏",
        id.is_some() && pinned == Some(("improve clean", Place::BottomToolbar)) && bm.by_place(Place::BottomToolbar).len() == 1,
        "📌 命令变自定义按钮，可选放置位置",
    );

    // F428
    let mut bm2 = ButtonManager::new();
    let a = bm2.create("save", "保存", IconKind::Emoji, "💾", "#34C759", 32, Place::BottomToolbar, CreateWay::Manual);
    let b = bm2.create("zoom", "缩放", IconKind::Pixel, "px", "#AF52DE", 32, Place::ActivityBar, CreateWay::Command);
    let c = bm2.create("help", "帮助", IconKind::Custom, "u", "#FFAB00", 32, Place::Sidebar, CreateWay::RightClick);
    let d = bm2.create("open", "打开", IconKind::Emoji, "📂", "#007AFF", 32, Place::CanvasEdge, CreateWay::Drag);
    s.add(
        "F428 自定义按钮创建",
        a == "btn-1" && b == "btn-2" && c == "btn-3" && d == "btn-4" && bm2.list().len() == 4,
        "手册固定/命令创建/右键创建/拖拽创建 四种方式",
    );

    // F429
    let places = Place::all();
    let mut bm3 = ButtonManager::new();
    for (i, p) in places.iter().enumerate() {
        bm3.create(&format!("c{i}"), "x", IconKind::Emoji, "▶", "#000000", 24, *p, CreateWay::Command);
    }
    s.add(
        "F429 按钮放置位置",
        places.len() == 5 && bm3.by_place(Place::Floating).len() == 1 && Place::parse("sidebar") == Some(Place::Sidebar) && Place::parse("nope").is_none(),
        "底部工具栏/活动栏/画布边缘/侧边栏/自由浮动 5 种",
    );

    // F430
    let mut bm4 = ButtonManager::new();
    let id4 = bm4.create("theme", "风格", IconKind::Pixel, "px", "#FF3B30", 40, Place::Floating, CreateWay::Drag);
    let b4 = bm4.get(&id4).unwrap();
    let s24 = CustomButton::normalize_size(24);
    let s32 = CustomButton::normalize_size(28);
    let s48 = CustomButton::normalize_size(99);
    s.add(
        "F430 按钮外观",
        b4.icon == IconKind::Pixel && b4.color == "#FF3B30" && b4.size == 32 && b4.label == "风格" && s24 == 24 && s32 == 32 && s48 == 48,
        "图标(emoji/像素/自定义)+颜色+大小(24/32/48)+标签",
    );

    // F431
    let mut bm5 = ButtonManager::new();
    let out_create = bm5.run_button_cmd(&["create", "save", "保存", "emoji", "💾", "#34C759", "48", "sidebar"]);
    let out_list = bm5.run_button_cmd(&["list"]);
    let out_bad = bm5.run_button_cmd(&["remove", "nope"]);
    let out_rm = bm5.run_button_cmd(&["remove", "btn-1"]);
    s.add(
        "F431 按钮管理",
        out_create.contains("btn-1")
            && out_list.contains("/save")
            && out_bad.contains("未找到")
            && out_rm.contains("已删除")
            && bm5.list().is_empty()
            && bm5.run_button_cmd(&["bogus"]).contains("未知子命令"),
        "/button list|create|remove|export|import",
    );

    // F432
    let lib = community_library();
    let json = export_json(&lib);
    let back = import_json(&json);
    let mut bm6 = ButtonManager::new();
    let n = bm6.import_pack(&json);
    let roundtrip = bm6.run_button_cmd(&["export"]);
    s.add(
        "F432 按钮分享",
        lib.len() == 4 && back.len() == 4 && back[0].command == "scan all" && n == 4 && roundtrip.contains("\"kind\":\"ca-buttons\"") && import_json(roundtrip.as_str()).len() == 4,
        "JSON 导出/导入 + 社区按钮库",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{LineKind, Terminal};

    #[test]
    fn f421_hotkey_only_with_ctrl() {
        let mut m = Manual::new();
        assert!(!m.handle_key("h", false));
        assert!(!m.open);
        assert!(m.handle_key("H", true) && m.open);
    }

    #[test]
    fn f422_every_command_categorized() {
        for c in cmd::COMMANDS {
            assert!(CATEGORIES.contains(&c.category), "{} 分类缺失", c.path);
        }
    }

    #[test]
    fn f423_search_empty_returns_all() {
        assert_eq!(search("").len(), cmd::COMMANDS.len());
        assert!(search("不存在的词xyz").is_empty());
    }

    #[test]
    fn f424_fav_dedup() {
        let mut st = ManualState::new();
        st.toggle_fav("zoom");
        st.toggle_fav("zoom"); // 再次点击 = 取消收藏
        assert_eq!(st.fav, Vec::<String>::new());
        st.toggle_fav("zoom");
        st.toggle_fav("help");
        assert_eq!(st.fav, vec!["zoom".to_string(), "help".to_string()]);
    }

    #[test]
    fn f425_recent_cap() {
        let mut st = ManualState::new();
        for i in 0..20 {
            st.touch_recent(&format!("c{i}"));
        }
        assert_eq!(st.recent.len(), 10);
        assert_eq!(st.recent[0], "c19");
    }

    #[test]
    fn f426_unknown_command_goes_to_terminal() {
        let mut st = ManualState::new();
        let mut t = Terminal::new();
        assert!(!execute(&mut st, &mut t, "nope", true));
        assert_eq!(t.lines[0].kind, LineKind::Error);
    }

    #[test]
    fn f430_size_clamped() {
        assert_eq!(CustomButton::normalize_size(0), 24);
        assert_eq!(CustomButton::normalize_size(48), 48);
    }

    #[test]
    fn f432_json_escapes_quotes() {
        let mut bm = ButtonManager::new();
        bm.create("a", "say \"hi\"", IconKind::Emoji, "▶", "#fff", 24, Place::Sidebar, CreateWay::Command);
        let json = export_json(&bm.buttons);
        let back = import_json(&json);
        assert_eq!(back[0].label, "say \"hi\"");
    }

    #[test]
    fn detail_render_modes_differ() {
        let mut m = Manual::new();
        m.mode = ManualMode::Plain;
        let plain = render_detail(&m, "survival", "scan all").unwrap();
        m.mode = ManualMode::Pro;
        let pro = render_detail(&m, "survival", "scan all").unwrap();
        m.mode = ManualMode::Dual;
        let dual = render_detail(&m, "survival", "scan all").unwrap();
        assert!(plain.contains("大白话"));
        assert!(pro.contains("权限"));
        assert!(dual.contains("大白话") && dual.contains("技术"));
        assert!(render_detail(&m, "x", "nope").is_none());
    }
}
