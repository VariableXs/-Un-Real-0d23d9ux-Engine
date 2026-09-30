//! 框选 + 框选转树状图 + 动态流向（#446~#448）—— AI-06 域五。
//!
//! 零 AI：矩形命中测试、AST 分类、粒子编排全为确定性算法。
//! 三端等价：框选只是选中语义，真正改码才走写回通道（C08），内核态同样可用。

use crate::model::StmtKind;

// ------------------------------------------------------------------ F446 框选

/// 框选描边色（蓝色虚线矩形）。
pub const MARQUEE_COLOR: &str = "#007AFF";
/// 框选填充（半透明蓝）。
pub const MARQUEE_FILL: &str = "rgba(0,122,255,0.15)";
/// 框选→树状图变形动画时长（ms）。
pub const MORPH_MS: u32 = 500;

/// 屏幕/画布矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    /// 由拖拽两点构造（任意方向拖拽都规范化为正尺寸）。
    pub fn from_drag(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Rect { x: x0.min(x1), y: y0.min(y1), w: (x1 - x0).abs(), h: (y1 - y0).abs() }
    }

    pub fn area(&self) -> f64 {
        self.w * self.h
    }

    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    /// 与另一矩形是否相交（含包含）。
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x <= o.x + o.w && o.x <= self.x + self.w && self.y <= o.y + o.h && o.y <= self.y + self.h
    }

    /// 是否退化（拖拽未产生面积，视为点击而非框选）。
    pub fn degenerate(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// F446 框选：Shift+拖拽 → 蓝色虚线矩形 + 半透明填充。
#[derive(Debug, Clone)]
pub struct Marquee {
    pub active: bool,
    /// 是否按住 Shift（否则为普通拖拽/平移）。
    pub shift: bool,
    pub rect: Rect,
    origin: (f64, f64),
}

impl Marquee {
    pub fn new() -> Self {
        Marquee {
            active: false,
            shift: false,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            origin: (0.0, 0.0),
        }
    }

    /// 按下：只有 Shift+拖拽才进入框选。
    pub fn begin(&mut self, shift: bool, x: f64, y: f64) -> bool {
        self.shift = shift;
        self.active = shift;
        self.origin = (x, y);
        self.rect = Rect::new(x, y, 0.0, 0.0);
        self.active
    }

    pub fn update(&mut self, x: f64, y: f64) {
        if !self.active {
            return;
        }
        self.rect = Rect::from_drag(self.origin.0, self.origin.1, x, y);
    }

    /// 松开：返回最终矩形（退化的拖拽不算框选）。
    pub fn end(&mut self) -> Option<Rect> {
        if !self.active {
            return None;
        }
        self.active = false;
        if self.rect.degenerate() {
            return None;
        }
        Some(self.rect)
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.rect = Rect::new(0.0, 0.0, 0.0, 0.0);
    }

    /// 命中测试：与矩形相交的节点即被选中（按 y 排序，保证与视觉顺序一致）。
    pub fn hit_test(&self, nodes: &[(String, Rect)]) -> Vec<String> {
        if self.rect.degenerate() {
            return Vec::new();
        }
        let mut hits: Vec<(f64, String)> = nodes
            .iter()
            .filter(|(_, r)| self.rect.intersects(r))
            .map(|(n, r)| (r.y, n.clone()))
            .collect();
        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        hits.into_iter().map(|(_, n)| n).collect()
    }
}

impl Default for Marquee {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------ F447 框选→树状图

/// 树状图节点类型（主规格节点样式表七类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeNodeKind {
    /// 函数：圆角矩形 #007AFF 微光
    Func,
    /// 判断：菱形 #FFAB00
    If,
    /// 循环：六边形 #AF52DE 旋转光环
    Loop,
    /// 赋值：平行四边形 #34C759
    Assign,
    /// 返回：圆角矩形 #8E8E93
    Return,
    /// 异常：圆形 #FF3B30 脉冲
    Throw,
    /// 输入：平行四边形 #5AC8FA
    Input,
}

impl TreeNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TreeNodeKind::Func => "函数",
            TreeNodeKind::If => "判断",
            TreeNodeKind::Loop => "循环",
            TreeNodeKind::Assign => "赋值",
            TreeNodeKind::Return => "返回",
            TreeNodeKind::Throw => "异常",
            TreeNodeKind::Input => "输入",
        }
    }
}

/// 节点样式（形状 / 颜色 / 动态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeStyle {
    pub shape: &'static str,
    pub color: &'static str,
    pub motion: &'static str,
}

/// F447 节点样式表：类型 → 形状/颜色/动态。
pub fn node_style(kind: TreeNodeKind) -> NodeStyle {
    match kind {
        TreeNodeKind::Func => NodeStyle { shape: "圆角矩形", color: "#007AFF", motion: "微光" },
        TreeNodeKind::If => NodeStyle { shape: "菱形", color: "#FFAB00", motion: "无" },
        TreeNodeKind::Loop => NodeStyle { shape: "六边形", color: "#AF52DE", motion: "旋转光环" },
        TreeNodeKind::Assign => NodeStyle { shape: "平行四边形", color: "#34C759", motion: "无" },
        TreeNodeKind::Return => NodeStyle { shape: "圆角矩形", color: "#8E8E93", motion: "无" },
        TreeNodeKind::Throw => NodeStyle { shape: "圆形", color: "#FF3B30", motion: "脉冲" },
        TreeNodeKind::Input => NodeStyle { shape: "平行四边形", color: "#5AC8FA", motion: "无" },
    }
}

/// 由一行代码判定树节点类型：先按语法（StmtKind），再按输入类关键字细化。
pub fn kind_from_line(line: &str) -> TreeNodeKind {
    let lower = line.to_lowercase();
    let is_input = ["read(", "readline", "input(", "scanf", "recv", "fetch", "getchar"]
        .iter()
        .any(|k| lower.contains(k));
    match StmtKind::parse(line) {
        StmtKind::If | StmtKind::Else => TreeNodeKind::If,
        StmtKind::Loop | StmtKind::Switch | StmtKind::Case => TreeNodeKind::Loop,
        StmtKind::Return => TreeNodeKind::Return,
        StmtKind::Throw | StmtKind::Try | StmtKind::Catch => TreeNodeKind::Throw,
        other => {
            if is_input {
                TreeNodeKind::Input
            } else if other == StmtKind::Call || line.trim().contains('(') {
                TreeNodeKind::Func
            } else {
                TreeNodeKind::Assign
            }
        }
    }
}

/// 框选内容的 AST 解析结果（按行序）。
pub fn parse_selection(lines: &[&str]) -> Vec<(TreeNodeKind, String)> {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .map(|l| (kind_from_line(l), l.to_string()))
        .collect()
}

/// 变形动画关键帧：矩形收缩为中心点 → AST 解析 → 从中心点生长出树状图。
#[derive(Debug, Clone, PartialEq)]
pub struct MorphPlan {
    pub stages: Vec<(u32, &'static str)>,
    pub total_ms: u32,
    /// 收缩/生长的中心点（框选矩形中心）。
    pub cx: f64,
    pub cy: f64,
}

pub fn morph_plan(sel: &Rect) -> MorphPlan {
    MorphPlan {
        stages: vec![
            (0u32, "矩形收缩为中心点"),
            (200u32, "AST解析框选内容"),
            (MORPH_MS, "从中心点生长树状图"),
        ],
        total_ms: MORPH_MS,
        cx: sel.x + sel.w / 2.0,
        cy: sel.y + sel.h / 2.0,
    }
}

// ------------------------------------------------------------------ F448 动态流向

/// 流向种类：绿=控制流 蓝=数据流 品红=异常流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    Control,
    Data,
    Exception,
}

impl FlowKind {
    pub fn color(self) -> &'static str {
        match self {
            FlowKind::Control => "#34C759",
            FlowKind::Data => "#007AFF",
            FlowKind::Exception => "#FF2D55",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FlowKind::Control => "控制流",
            FlowKind::Data => "数据流",
            FlowKind::Exception => "异常流",
        }
    }
}

/// 流向粒子：从 `from` 到 `to`，`delay_ms` 后出发。
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    pub kind: FlowKind,
    pub from: String,
    pub to: String,
    pub delay_ms: u32,
    /// 每秒行进的比例（粒子速度）。
    pub speed: f64,
}

/// F448 动态流向：按节点类型编排三种粒子。
/// - 控制流：相邻节点依次连线；
/// - 数据流：输入/赋值节点 → 其后的函数节点；
/// - 异常流：判断节点 → 其后的异常节点。
pub fn flow_particles(nodes: &[(TreeNodeKind, String)], step_ms: u32) -> Vec<Particle> {
    let mut out = Vec::new();
    for i in 0..nodes.len().saturating_sub(1) {
        out.push(Particle {
            kind: FlowKind::Control,
            from: nodes[i].1.clone(),
            to: nodes[i + 1].1.clone(),
            delay_ms: i as u32 * step_ms,
            speed: 1.0,
        });
    }
    for i in 0..nodes.len() {
        if !matches!(nodes[i].0, TreeNodeKind::Input | TreeNodeKind::Assign) {
            continue;
        }
        if let Some((_, target)) = nodes[i + 1..].iter().find(|(k, _)| *k == TreeNodeKind::Func) {
            out.push(Particle {
                kind: FlowKind::Data,
                from: nodes[i].1.clone(),
                to: target.clone(),
                delay_ms: i as u32 * step_ms,
                speed: 0.8,
            });
        }
    }
    for i in 0..nodes.len() {
        if nodes[i].0 != TreeNodeKind::If {
            continue;
        }
        if let Some((_, target)) = nodes[i + 1..].iter().find(|(k, _)| *k == TreeNodeKind::Throw) {
            out.push(Particle {
                kind: FlowKind::Exception,
                from: nodes[i].1.clone(),
                to: target.clone(),
                delay_ms: i as u32 * step_ms,
                speed: 1.4,
            });
        }
    }
    out
}

/// 某一时刻处于飞行中的粒子（供渲染消费）。
pub fn active_particles(particles: &[Particle], t_ms: u32, travel_ms: u32) -> Vec<&Particle> {
    particles
        .iter()
        .filter(|p| t_ms >= p.delay_ms && t_ms <= p.delay_ms + travel_ms)
        .collect()
}

// ------------------------------------------------------------------ 自检

/// AI-06 域五自检（#446~#448，3 项）。
pub fn run_marquee_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("marquee");

    // F446
    let mut m = Marquee::new();
    let started = m.begin(true, 100.0, 100.0);
    m.update(300.0, 220.0);
    let rect = m.end().expect("框选矩形");
    let nodes = vec![
        ("a".to_string(), Rect::new(110.0, 110.0, 50.0, 20.0)),
        ("b".to_string(), Rect::new(260.0, 200.0, 50.0, 20.0)),
        ("c".to_string(), Rect::new(500.0, 500.0, 50.0, 20.0)),
    ];
    let hits = m.hit_test(&nodes);
    let no_shift = Marquee::new().begin(false, 0.0, 0.0);
    let degen = Rect::from_drag(10.0, 10.0, 10.0, 10.0).degenerate();
    s.add(
        "F446 框选代码",
        started
            && rect == Rect::new(100.0, 100.0, 200.0, 120.0)
            && rect.area() == 24000.0
            && hits == vec!["a".to_string(), "b".to_string()]
            && rect.contains_point(150.0, 150.0)
            && !rect.contains_point(400.0, 400.0)
            && !no_shift
            && degen
            && MARQUEE_COLOR == "#007AFF",
        "Shift+拖拽，蓝色虚线矩形+半透明填充",
    );

    // F447
    let plan = morph_plan(&rect);
    let sel = parse_selection(&[
        "function login() {",
        "// 注释",
        "if (check(token)) {",
        "user = read(db)",
        "redirect()",
        "throw error",
        "return ok",
    ]);
    let kinds: Vec<TreeNodeKind> = sel.iter().map(|(k, _)| *k).collect();
    let st_func = node_style(TreeNodeKind::Func);
    let st_loop = node_style(TreeNodeKind::Loop);
    s.add(
        "F447 框选→树状图",
        plan.total_ms == 500
            && plan.stages.len() == 3
            && plan.cx == 200.0
            && plan.cy == 160.0
            && sel.len() == 6
            && kinds[0] == TreeNodeKind::Func
            && kinds[1] == TreeNodeKind::If
            && kinds[2] == TreeNodeKind::Input
            && kinds[3] == TreeNodeKind::Func
            && kinds[4] == TreeNodeKind::Throw
            && kinds[5] == TreeNodeKind::Return
            && st_func.shape == "圆角矩形"
            && st_func.color == "#007AFF"
            && st_func.motion == "微光"
            && st_loop.shape == "六边形"
            && st_loop.motion == "旋转光环",
        "AST解析 + 500ms 变形动画 + 七类节点样式表",
    );

    // F448
    let ps = flow_particles(&sel, 100);
    let ctrl: Vec<&Particle> = ps.iter().filter(|p| p.kind == FlowKind::Control).collect();
    let data: Vec<&Particle> = ps.iter().filter(|p| p.kind == FlowKind::Data).collect();
    let exc: Vec<&Particle> = ps.iter().filter(|p| p.kind == FlowKind::Exception).collect();
    let at0 = active_particles(&ps, 0, 500).len();
    let at1200 = active_particles(&ps, 1200, 500).len();
    s.add(
        "F448 动态流向",
        ctrl.len() == 5
            && data.len() == 1
            && exc.len() == 1
            && ctrl[0].from.starts_with("function login")
            && data[0].from == "user = read(db)"
            && data[0].to == "redirect()"
            && exc[0].to == "throw error"
            && FlowKind::Control.color() == "#34C759"
            && FlowKind::Data.color() == "#007AFF"
            && FlowKind::Exception.color() == "#FF2D55"
            && at0 >= 1
            && at1200 == 0,
        "绿=控制流 蓝=数据流 品红=异常流",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f446_reverse_drag_normalized() {
        let r = Rect::from_drag(300.0, 300.0, 100.0, 100.0);
        assert_eq!(r, Rect::new(100.0, 100.0, 200.0, 200.0));
    }

    #[test]
    fn f446_cancel_clears() {
        let mut m = Marquee::new();
        m.begin(true, 0.0, 0.0);
        m.update(50.0, 50.0);
        m.cancel();
        assert!(!m.active);
        assert!(m.hit_test(&[("a".to_string(), Rect::new(0.0, 0.0, 10.0, 10.0))]).is_empty());
    }

    #[test]
    fn f447_comments_and_blanks_skipped() {
        let sel = parse_selection(&["  ", "# hash", "// x", "return 1"]);
        assert_eq!(sel.len(), 1);
        assert_eq!(sel[0].0, TreeNodeKind::Return);
    }

    #[test]
    fn f447_loop_and_assign() {
        assert_eq!(kind_from_line("for i in 0..10 {"), TreeNodeKind::Loop);
        assert_eq!(kind_from_line("while (ok) {"), TreeNodeKind::Loop);
        assert_eq!(kind_from_line("x = 1"), TreeNodeKind::Assign);
        assert_eq!(kind_from_line("do_it()"), TreeNodeKind::Func);
    }

    #[test]
    fn f448_no_nodes_no_particles() {
        assert!(flow_particles(&[], 100).is_empty());
    }

    #[test]
    fn f448_exception_needs_throw() {
        let nodes = vec![
            (TreeNodeKind::Func, "a()".to_string()),
            (TreeNodeKind::If, "if(x)".to_string()),
            (TreeNodeKind::Return, "return 1".to_string()),
        ];
        let ps = flow_particles(&nodes, 10);
        assert!(!ps.iter().any(|p| p.kind == FlowKind::Exception));
    }
}
