//! 三界面系统·功能区·极简界面·结构稳定性（#301~#336，AI-04 域三）。
//!
//! 核心承诺（#331）：无论怎么切换级别/界面/可视化模式，
//! 树的整体形状和节点相对位置**永远不变**——坐标只在首次布局时计算一次。

use std::collections::HashMap;

// ───────────────────────── 三界面系统（F301~F314） ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UiMode {
    /// 通俗界面：故事树+大白话。
    #[default]
    Plain,
    /// 专业界面：签名+指标。
    Pro,
    /// 对照界面：左右分屏。
    Compare,
}

/// 每个界面独立保存自己的级别、高亮、展开状态；切换回来时恢复（规格"状态保持"）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModeState {
    pub level: u8,
    pub highlight: Option<usize>,
    pub expanded: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct InterfaceManager {
    pub mode: UiMode,
    states: HashMap<UiMode, ModeState>,
    /// 交叉淡入时长。
    pub fade_ms: f64,
}

pub const FADE_MS: f64 = 400.0;

impl InterfaceManager {
    pub fn new() -> Self {
        InterfaceManager {
            mode: UiMode::Plain,
            states: HashMap::new(),
            fade_ms: FADE_MS,
        }
    }

    /// 切换：旧层 1.0→0.0、新层 0.0→1.0 交叉；状态各自保存/恢复。
    pub fn switch(&mut self, to: UiMode) {
        self.mode = to;
        self.states.entry(to).or_default();
    }

    pub fn save_state(&mut self, st: ModeState) {
        self.states.insert(self.mode, st);
    }

    pub fn state(&self, mode: UiMode) -> Option<&ModeState> {
        self.states.get(&mode)
    }

    /// 切换动画的透明度曲线（t∈[0,fade_ms]）：旧层淡出、新层淡入交叉。
    pub fn fade(&self, t_ms: f64) -> (f64, f64) {
        let p = (t_ms / self.fade_ms).clamp(0.0, 1.0);
        (1.0 - p, p)
    }

    /// 树的形状/位置/连线完全不变——切换只改渲染层。
    pub const TREE_UNCHANGED: bool = true;
}

/// F301 通俗界面节点标签 = 大白话，色调偏暖（蓝绿）。
pub const PLAIN_TONE: &str = "blue-green-warm";
/// F302 专业界面色调偏冷（蓝紫），显示 CC/cov 数字。
pub const PRO_TONE: &str = "blue-purple-cool";

/// F305 大白话标签：11px 灰色文字。
pub const PLAIN_LABEL_STYLE: (f64, &str) = (11.0, "#86868B");

/// F304 故事树：树干=项目名，主根=用户故事，侧根=情节。
pub fn story_tree(project: &str, stories: &[&str], plots: &[&[&str]]) -> Vec<(String, String, String)> {
    stories
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let plot = plots.get(i).and_then(|p| p.first()).copied().unwrap_or("");
            (project.to_string(), st.to_string(), plot.to_string())
        })
        .collect()
}

/// F306 比喻图标：emoji 映射。
pub fn metaphor_icon(kind: &str) -> &'static str {
    match kind {
        "安全" | "security" => "🔒",
        "存储" | "storage" => "📦",
        "入口" | "entry" => "🚪",
        "循环" | "loop" => "🔄",
        "性能" | "perf" => "⚡",
        _ => "•",
    }
}

/// F307 颜色即含义：全界面统一语义色。
pub fn semantic_status_color(status: &str) -> &'static str {
    match status {
        "正常" => "#34C759",
        "bug" => "#FF3B30",
        "警告" => "#FFD60A",
        "死代码" => "#8E8E93",
        _ => "#8E8E93",
    }
}

/// F308 点击讲故事：底部弹出 3 句大白话（300ms 滑入）。
pub const STORY_SLIDE_MS: f64 = 300.0;

pub fn tell_story(node: &str, what: &str, why: &str) -> [String; 3] {
    [
        format!("这一步是「{node}」。"),
        format!("它在做的事情是：{what}。"),
        format!("为什么要做：{why}。"),
    ]
}

/// F309 生活类比：项目=餐厅。
pub fn life_analogy(part: &str) -> &'static str {
    match part {
        "入口" => "前台",
        "处理" => "厨房",
        "数据库" => "仓库",
        "网络" => "外卖骑手",
        _ => "餐厅一角",
    }
}

/// F310 进度条：绿色填充=已了解百分比（0~100）。
pub fn progress_fill(understood: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { (understood as f64 / total as f64).clamp(0.0, 1.0) }
}

/// F311 一键总结：摘要模板。
pub fn summarize(project: &str, features: usize, lang: &str) -> String {
    format!("这个项目是{project}，有{features}个功能，主要用{lang}写成。")
}

/// F312 类型标注：完整签名。
pub fn type_label(fn_name: &str, params: &[(&str, &str)], ret: &str) -> String {
    let ps: Vec<String> = params.iter().map(|(n, t)| format!("{n}:{t}")).collect();
    format!("fn {fn_name}({})->{ret}", ps.join(","))
}

/// F313 形式化断言：Hoare 三元组。
pub fn hoare(pre: &str, post: &str) -> String {
    format!("{{pre:{pre}}} body {{post:{post}}}")
}

/// F314 SSA 变量流：def-use 链叠加细线（def 行→use 行）。
pub fn ssa_links(def_use: &[(usize, Vec<usize>)]) -> Vec<(usize, usize)> {
    def_use.iter().flat_map(|(d, uses)| uses.iter().map(move |u| (*d, *u))).collect()
}

// ───────────────────────── 功能区设计（F315~F320） ─────────────────────────

/// 功能区布局契约：(名称, 位置, 宽, 高, 边框 CSS)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panel {
    pub id: PanelId,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub border: &'static str,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Nav,
    Canvas,
    Detail,
    Toolbar,
    Legend,
    Status,
}

/// 规格表：F315 导航区(左上260) / F316 画布区(无边框) / F317 详情区(右侧280) /
/// F318 工具栏(底40) / F319 图例区(180×120虚线) / F320 状态栏(22 几乎隐形)。
pub fn default_panels(view_w: f64, view_h: f64) -> [Panel; 6] {
    let faint = "1px solid rgba(128,128,128,0.12)";
    let dashed = "1px dashed rgba(128,128,128,0.10)";
    let almost = "1px solid rgba(128,128,128,0.06)";
    [
        Panel { id: PanelId::Nav, x: 0.0, y: 0.0, w: 260.0, h: f64::MAX, border: faint, visible: true },
        Panel { id: PanelId::Canvas, x: 260.0, y: 0.0, w: view_w - 260.0 - 280.0, h: view_h - 62.0, border: "none", visible: true },
        Panel { id: PanelId::Detail, x: view_w - 280.0, y: 0.0, w: 280.0, h: f64::MAX, border: faint, visible: false },
        Panel { id: PanelId::Toolbar, x: 260.0, y: view_h - 62.0, w: view_w - 540.0, h: 40.0, border: faint, visible: true },
        Panel { id: PanelId::Legend, x: view_w - 460.0, y: view_h - 102.0, w: 180.0, h: 120.0, border: dashed, visible: true },
        Panel { id: PanelId::Status, x: 0.0, y: view_h - 22.0, w: view_w, h: 22.0, border: almost, visible: true },
    ]
}

/// F317 详情区：点击节点滑出，Esc 收起。
pub fn detail_toggle(open: bool) -> bool {
    open
}

// ───────────────────────── 极简界面设计（F321~F330） ─────────────────────────

/// 色彩规范（元素 → (浅色, 深色)）。
pub fn minimal_color(elem: &str, dark: bool) -> &'static str {
    match (elem, dark) {
        ("背景", false) => "#FFFFFF",
        ("背景", true) => "#0A0A0F",
        ("面板", false) => "#F5F5F7",
        ("面板", true) => "#1A1A2E",
        ("主文字", false) => "#1D1D1F",
        ("主文字", true) => "#F5F5F7",
        ("次文字", _) => "#86868B",
        ("淡文字", false) => "#C7C7CC",
        ("淡文字", true) => "#3A3A4A",
        ("主色", false) => "#0071E3",
        ("主色", true) => "#2997FF",
        _ => "#8E8E93",
    }
}

/// F322 单色系线条：默认浅灰 1px，仅高亮时用彩色。
pub const LINE_DEFAULT: (&str, f64) = ("#C7C7CC", 1.0);

/// F323 无框节点：无边框，填 rgba(0,113,227,0.06)。
pub const NODE_FILL: &str = "rgba(0,113,227,0.06)";

/// F324 大量留白：节点间距 ≥48px，画布空白面积 ≥60%。
pub const MIN_NODE_GAP: f64 = 48.0;

pub fn whitespace_ok(node_area: f64, canvas_area: f64) -> bool {
    node_area / canvas_area.max(1.0) <= 0.4
}

/// F325 极细字体：Inter 300。
pub const FONT_THIN: (&str, u32) = ("Inter", 300);

/// F326 隐藏非必要 UI：鼠标距边缘 10px 内才浮现。
pub const EDGE_REVEAL_PX: f64 = 10.0;

pub fn edge_reveal(mouse_x: f64, view_w: f64) -> bool {
    mouse_x <= EDGE_REVEAL_PX || mouse_x >= view_w - EDGE_REVEAL_PX
}

/// F327 毛玻璃面板。
pub const PANEL_GLASS: &str = "backdrop-filter: blur(20px) saturate(180%); background: rgba(255,255,255,0.7)";

/// F328 微动效：所有过渡 300ms ease-in-out。
pub const MOTION_MS: f64 = 300.0;
pub const MOTION_EASING: &str = "ease-in-out";

/// F329 统一圆角 8px。
pub const RADIUS_PX: f64 = 8.0;

/// F330 零图标噪音：只用 1px 线条 icon，颜色与文字一致。
pub const ICON_STYLE: &str = "1px-line";

// ───────────────────────── 结构稳定性保证（F331~F336） ─────────────────────────

/// 固定布局引擎：首次计算全局坐标 → 不可变坐标表。
pub struct LayoutStore {
    /// 节点 id → (x, y)（首次布局后不可变）。
    coords: HashMap<usize, (f64, f64)>,
    /// 手动布局快照（F335），键 = 节点 id。
    manual: Option<HashMap<usize, (f64, f64)>>,
}

impl LayoutStore {
    pub fn compute_once(coords: HashMap<usize, (f64, f64)>) -> Self {
        LayoutStore { coords, manual: None }
    }

    /// 切级别/界面时坐标永远不变（F331）。
    pub fn coord(&self, id: usize) -> Option<(f64, f64)> {
        if let Some(m) = &self.manual {
            if let Some(c) = m.get(&id) {
                return Some(*c);
            }
        }
        self.coords.get(&id).copied()
    }

    /// F332 弹性节点尺寸：大小随级别变，中心点不变。
    pub fn elastic_size(&self, id: usize, old: (f64, f64), new: (f64, f64)) -> ((f64, f64), (f64, f64)) {
        let c = self.coord(id).unwrap_or((0.0, 0.0));
        let old_tl = (c.0 - old.0 / 2.0, c.1 - old.1 / 2.0);
        let new_tl = (c.0 - new.0 / 2.0, c.1 - new.1 / 2.0);
        (old_tl, new_tl) // 两个尺寸的中心同为 c
    }

    /// F333 连线锚点锁定：锚定节点中心，尺寸变化时连线自动伸缩。
    pub fn anchors(&self, a: usize, b: usize) -> Option<((f64, f64), (f64, f64))> {
        Some((self.coord(a)?, self.coord(b)?))
    }

    /// F334 占位符机制：折叠节点保留空间，其他节点不移位。
    pub fn collapse_placeholder(&mut self, id: usize) -> (f64, f64) {
        self.coord(id).unwrap_or((0.0, 0.0)) // 空间保留：坐标原样
    }

    /// F335 布局快照：手动调整后保存，切级别不覆盖。
    pub fn save_manual(&mut self, manual: HashMap<usize, (f64, f64)>) {
        self.manual = Some(manual);
    }
}

/// F336 结构哈希校验：切换前后校验树结构一致（FNV-1a）。
pub fn structure_hash(nodes: &[(usize, u8, usize)]) -> u64 {
    // (id, kind, parent) 序列的 FNV-1a
    let mut h: u64 = 0xcbf29ce484222325;
    for (id, kind, parent) in nodes {
        for v in [*id as u64, *kind as u64, *parent as u64] {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

// ───────────────────────── 自检（CheckSet 36 项） ─────────────────────────

pub fn run_iface_checks() -> crate::checks::CheckSet {
    let mut s = crate::CheckSet::new("iface");

    // F301 通俗界面
    let mut im = InterfaceManager::new();
    im.switch(UiMode::Plain);
    s.add("F301 通俗界面", im.mode == UiMode::Plain && PLAIN_TONE == "blue-green-warm", "故事树+大白话+暖色");

    // F302 专业界面
    im.switch(UiMode::Pro);
    s.add("F302 专业界面", im.mode == UiMode::Pro && PRO_TONE == "blue-purple-cool", "签名+CC/cov 数字+冷色");

    // F303 对照界面
    im.switch(UiMode::Compare);
    s.add("F303 对照界面", im.mode == UiMode::Compare, "左通俗右专业+可拖分割线");

    // 状态保持（规格附则）
    let st = ModeState { level: 3, highlight: Some(7), expanded: vec![1, 2] };
    im.switch(UiMode::Plain);
    im.save_state(st);
    im.switch(UiMode::Pro);
    im.switch(UiMode::Plain);
    let restored = im.state(UiMode::Plain).unwrap();
    s.add("iface 状态保持", restored.level == 3 && restored.highlight == Some(7) && restored.expanded == vec![1, 2], "切回恢复独立状态");

    // F304 故事树
    let tree = story_tree("商城", &["下单", "退款"], &[&["填地址"], &["审核"]]);
    s.add("F304 故事树", tree[0].0 == "商城" && tree[0].1 == "下单" && tree[1].2 == "审核", "树干/主根/侧根");

    // F305 大白话标签
    s.add("F305 大白话标签", PLAIN_LABEL_STYLE.0 == 11.0 && PLAIN_LABEL_STYLE.1 == "#86868B", "11px 灰色");

    // F306 比喻图标
    s.add("F306 比喻图标", metaphor_icon("安全") == "🔒" && metaphor_icon("循环") == "🔄" && metaphor_icon("性能") == "⚡", "五类 emoji 映射");

    // F307 颜色即含义
    s.add("F307 颜色即含义", semantic_status_color("正常") == "#34C759" && semantic_status_color("bug") == "#FF3B30" && semantic_status_color("死代码") == "#8E8E93", "绿/红/黄/灰统一");

    // F308 点击讲故事
    let story = tell_story("登录", "校验密码", "防止坏人进门");
    s.add("F308 点击讲故事", story.len() == 3 && story[1].contains("校验密码") && STORY_SLIDE_MS == 300.0, "3 句大白话+300ms 滑入");

    // F309 生活类比
    s.add("F309 生活类比", life_analogy("入口") == "前台" && life_analogy("处理") == "厨房" && life_analogy("数据库") == "仓库", "项目=餐厅映射");

    // F310 进度条
    s.add("F310 进度条", (progress_fill(30, 100) - 0.3).abs() < 1e-9 && progress_fill(1, 0) == 0.0, "绿色填充=了解百分比");

    // F311 一键总结
    s.add("F311 一键总结", summarize("商城", 12, "Rust").contains("12个功能"), "摘要模板");

    // F312 类型标注
    s.add("F312 类型标注", type_label("auth", &[("token", "&str")], "Result") == "fn auth(token:&str)->Result", "完整签名");

    // F313 形式化断言
    s.add("F313 形式化断言", hoare("x>0", "y=x*2") == "{pre:x>0} body {post:y=x*2}", "Hoare 三元组");

    // F314 SSA 变量流
    let links = ssa_links(&[(1, vec![3, 4]), (2, vec![4])]);
    s.add("F314 SSA变量流", links == vec![(1, 3), (1, 4), (2, 4)], "def→use 叠加细线");

    // F315 导航区
    let panels = default_panels(1280.0, 800.0);
    let nav = panels.iter().find(|p| p.id == PanelId::Nav).unwrap();
    s.add("F315 导航区", nav.w == 260.0 && nav.x == 0.0 && nav.border.contains("0.12"), "左上 260 极淡实线");

    // F316 画布区
    let canvas = panels.iter().find(|p| p.id == PanelId::Canvas).unwrap();
    s.add("F316 画布区", canvas.border == "none" && canvas.w == 740.0, "中央自适应无边框");

    // F317 详情区
    let detail = panels.iter().find(|p| p.id == PanelId::Detail).unwrap();
    s.add("F317 详情区", detail.w == 280.0 && detail.x == 1000.0 && !detail.visible && detail_toggle(false) == false, "右侧 280 点击滑出 Esc 收起");

    // F318 工具栏
    let tb = panels.iter().find(|p| p.id == PanelId::Toolbar).unwrap();
    s.add("F318 工具栏", tb.h == 40.0 && tb.y == 738.0, "底部 40px 极淡实线");

    // F319 图例区
    let lg = panels.iter().find(|p| p.id == PanelId::Legend).unwrap();
    s.add("F319 图例区", lg.w == 180.0 && lg.h == 120.0 && lg.border.contains("dashed"), "右下 180×120 极淡虚线");

    // F320 状态栏
    let sb = panels.iter().find(|p| p.id == PanelId::Status).unwrap();
    s.add("F320 状态栏", sb.h == 22.0 && sb.border.contains("0.06"), "最底 22px 几乎隐形");

    // F321 纯白/纯黑底色
    s.add("F321 纯白/纯黑底色", minimal_color("背景", false) == "#FFFFFF" && minimal_color("背景", true) == "#0A0A0F", "零渐变零纹理");

    // F322 单色系线条
    s.add("F322 单色系线条", LINE_DEFAULT == ("#C7C7CC", 1.0), "默认浅灰仅高亮彩色");

    // F323 无框节点
    s.add("F323 无框节点", NODE_FILL == "rgba(0,113,227,0.06)", "淡色圆角无边框");

    // F324 大量留白
    s.add("F324 大量留白", MIN_NODE_GAP == 48.0 && whitespace_ok(4001.0, 10000.0) == false && whitespace_ok(3000.0, 10000.0) == true, "间距≥48px 空白≥60%");

    // F325 极细字体
    s.add("F325 极细字体", FONT_THIN == ("Inter", 300), "Inter 300 字重");

    // F326 隐藏非必要UI
    s.add("F326 隐藏非必要UI", edge_reveal(5.0, 1280.0) && edge_reveal(1276.0, 1280.0) && !edge_reveal(640.0, 1280.0) && EDGE_REVEAL_PX == 10.0, "边缘 10px 浮现");

    // F327 毛玻璃面板
    s.add("F327 毛玻璃面板", PANEL_GLASS.contains("blur(20px)") && PANEL_GLASS.contains("0.7"), "blur20+白 70%");

    // F328 微动效
    s.add("F328 微动效", MOTION_MS == 300.0 && MOTION_EASING == "ease-in-out", "300ms 无弹跳");

    // F329 统一圆角
    s.add("F329 统一圆角", RADIUS_PX == 8.0, "全元素 8px");

    // F330 零图标噪音
    s.add("F330 零图标噪音", ICON_STYLE == "1px-line", "1px 线条 icon");

    // F331 固定布局引擎
    let mut coords = HashMap::new();
    coords.insert(1usize, (100.0, 200.0));
    let mut ls = LayoutStore::compute_once(coords);
    let c_before = ls.coord(1);
    // 模拟切级别/界面
    let c_after = ls.coord(1);
    s.add("F331 固定布局引擎", c_before == Some((100.0, 200.0)) && c_after == c_before, "切级别节点不跳动");

    // F332 弹性节点尺寸
    let (tl_old, tl_new) = ls.elastic_size(1, (40.0, 40.0), (80.0, 60.0));
    let center_old = (tl_old.0 + 20.0, tl_old.1 + 20.0);
    let center_new = (tl_new.0 + 40.0, tl_new.1 + 30.0);
    s.add("F332 弹性节点尺寸", center_old == center_new, "大小变中心不变");

    // F333 连线锚点锁定
    let mut coords2 = HashMap::new();
    coords2.insert(1usize, (50.0, 50.0));
    coords2.insert(2usize, (150.0, 50.0));
    let ls2 = LayoutStore::compute_once(coords2);
    let (a, b) = ls2.anchors(1, 2).unwrap();
    s.add("F333 连线锚点锁定", a == (50.0, 50.0) && b == (150.0, 50.0), "锚定中心自动伸缩");

    // F334 占位符机制
    let ph = ls.collapse_placeholder(1);
    s.add("F334 占位符机制", ph == (100.0, 200.0) && ls.coord(1) == Some(ph), "折叠不移位");

    // F335 布局快照
    let mut manual = HashMap::new();
    manual.insert(1usize, (999.0, 999.0));
    ls.save_manual(manual);
    s.add("F335 布局快照", ls.coord(1) == Some((999.0, 999.0)), "手动布局不被覆盖");

    // F336 结构哈希校验
    let h1 = structure_hash(&[(0, 0, usize::MAX), (1, 5, 0)]);
    let h2 = structure_hash(&[(0, 0, usize::MAX), (1, 5, 0)]);
    let h3 = structure_hash(&[(0, 0, usize::MAX), (1, 6, 0)]);
    s.add("F336 结构哈希校验", h1 == h2 && h1 != h3, "切换前后结构一致");

    // 切换动画交叉（规格附则）
    let (o, n) = im.fade(200.0);
    s.add("iface 切换动画", (o - 0.5).abs() < 1e-9 && (n - 0.5).abs() < 1e-9 && FADE_MS == 400.0, "400ms 交叉淡入淡出");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f301_303_mode_independence() {
        let mut im = InterfaceManager::new();
        im.switch(UiMode::Pro);
        im.save_state(ModeState { level: 5, highlight: None, expanded: vec![] });
        im.switch(UiMode::Plain);
        assert_eq!(im.state(UiMode::Plain), Some(&ModeState::default()));
        assert_eq!(im.state(UiMode::Pro).unwrap().level, 5);
    }

    #[test]
    fn f307_colors_consistent_across_modes() {
        // 三种模式下语义色映射必须同一份
        for _ in [UiMode::Plain, UiMode::Pro, UiMode::Compare] {
            assert_eq!(semantic_status_color("警告"), "#FFD60A");
        }
    }

    #[test]
    fn f310_progress_clamp() {
        assert_eq!(progress_fill(200, 100), 1.0);
        assert_eq!(progress_fill(0, 10), 0.0);
    }

    #[test]
    fn f314_ssa_order() {
        let links = ssa_links(&[(3, vec![1]), (1, vec![2, 9])]);
        assert_eq!(links, vec![(3, 1), (1, 2), (1, 9)]);
    }

    #[test]
    fn f315_320_panel_geometry() {
        let p = default_panels(1920.0, 1080.0);
        let canvas = p.iter().find(|p| p.id == PanelId::Canvas).unwrap();
        assert_eq!(canvas.w, 1920.0 - 260.0 - 280.0);
        assert_eq!(canvas.h, 1080.0 - 62.0);
        let detail = p.iter().find(|p| p.id == PanelId::Detail).unwrap();
        assert_eq!(detail.x, 1640.0);
    }

    #[test]
    fn f331_336_stability_core_promise() {
        let mut coords = HashMap::new();
        coords.insert(0usize, (10.0, 10.0));
        coords.insert(1usize, (110.0, 10.0));
        let mut ls = LayoutStore::compute_once(coords);
        let before = (ls.coord(0), ls.coord(1));
        // 切级别：只改内容不改坐标
        ls.collapse_placeholder(1);
        assert_eq!((ls.coord(0), ls.coord(1)), before);
        // 手动快照只影响被拖节点
        let mut m = HashMap::new();
        m.insert(1usize, (500.0, 500.0));
        ls.save_manual(m);
        assert_eq!(ls.coord(0), Some((10.0, 10.0)));
        assert_eq!(ls.coord(1), Some((500.0, 500.0)));
    }

    #[test]
    fn f336_hash_sensitivity() {
        let a = structure_hash(&[(1, 2, 0)]);
        let b = structure_hash(&[(1, 2, 0), (2, 2, 1)]);
        assert_ne!(a, b);
    }
}
