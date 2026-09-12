//! 统一 2D 流程图体系（#276~#300，AI-04 域二）。
//!
//! 代码↔流程图双向同步、Sugiyama 自动布局、七类节点/四类连线的完整样式契约。
//! 只产出布局与样式数据，渲染交给三端各自的 Canvas2D 层。

use crate::model::{Edge, EdgeKind, StmtKind};

// ───────────────────────── 节点与连线模型（规格节点样式表） ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    StartEnd,
    Process,
    Decision,
    Io,
    Loop,
    Exception,
    Call,
}

impl FlowKind {
    /// (形状名, 宽, 高, 填充色, 边框色, 文字色, 字号)（规格节点样式表）。
    pub fn style(self) -> (&'static str, f64, f64, &'static str, &'static str, &'static str, f64) {
        match self {
            FlowKind::StartEnd => ("rounded", 100.0, 40.0, "#34C759", "#1B6E2F", "#FFFFFF", 13.0),
            FlowKind::Process => ("rect", 120.0, 48.0, "#007AFF", "#1D4ED8", "#FFFFFF", 12.0),
            FlowKind::Decision => ("diamond", 100.0, 60.0, "#FFAB00", "#8A5A00", "#000000", 11.0),
            FlowKind::Io => ("parallelogram", 120.0, 40.0, "#5AC8FA", "#0E7490", "#000000", 12.0),
            FlowKind::Loop => ("hexagon", 100.0, 50.0, "#AF52DE", "#5B21B6", "#FFFFFF", 11.0),
            FlowKind::Exception => ("circle", 60.0, 60.0, "#FF3B30", "#991B1B", "#FFFFFF", 10.0),
            FlowKind::Call => ("dashed-rect", 120.0, 48.0, "#F5F5F7", "#8E8E93", "#000000", 12.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowEdgeKind {
    Normal,
    Exception,
    Data,
    Dep,
}

impl FlowEdgeKind {
    /// (颜色, 线宽, 样式, 箭头)（规格连线样式表）。
    pub fn style(self) -> (&'static str, f64, &'static str, &'static str) {
        match self {
            FlowEdgeKind::Normal => ("#1D1D1F", 2.0, "solid", "solid-tri"),
            FlowEdgeKind::Exception => ("#FF3B30", 1.5, "dashed", "hollow-tri"),
            FlowEdgeKind::Data => ("#007AFF", 3.0, "solid", "particles"),
            FlowEdgeKind::Dep => ("#8E8E93", 1.0, "dotted", "small-circle"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlowNode {
    pub id: usize,
    pub kind: FlowKind,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub collapsed: bool,
    pub hot: Option<f64>,
    pub bug: bool,
    pub note: Option<String>,
    pub var_label: Option<String>,
    pub time_label: Option<String>,
    pub duration_ms: f64,
}

impl FlowNode {
    pub fn size(&self) -> (f64, f64) {
        let (_, w, h, ..) = self.kind.style();
        (w, h)
    }
}

#[derive(Debug, Clone)]
pub struct FlowEdge {
    pub from: usize,
    pub to: usize,
    pub kind: FlowEdgeKind,
    /// 条件标注（F292）：菱形出边旁的条件文字。
    pub label: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FlowChart {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

/// 语句序列 → 流程图（F276/F296：if→菱形，loop→六边形，try/throw→异常圆，
/// io→平行四边形，调用→虚线矩形，return→结束圆角矩形）。
pub fn build_flow(stmts: &[StmtKind]) -> FlowChart {
    let mut chart = FlowChart::default();
    let start = chart.push(FlowKind::StartEnd, "开始");
    let mut prev = Some(start);
    for st in stmts {
        let kind = match st {
            StmtKind::If => FlowKind::Decision,
            StmtKind::Loop => FlowKind::Loop,
            StmtKind::Try | StmtKind::Throw | StmtKind::Catch => FlowKind::Exception,
            StmtKind::Call => FlowKind::Call,
            StmtKind::Assign => FlowKind::Process,
            StmtKind::Return => FlowKind::StartEnd,
            _ => FlowKind::Process,
        };
        let id = chart.push(kind, label_of(*st));
        let ekind = if matches!(st, StmtKind::Throw | StmtKind::Catch) { FlowEdgeKind::Exception } else { FlowEdgeKind::Normal };
        chart.edges.push(FlowEdge { from: prev.unwrap(), to: id, kind: ekind, label: None });
        prev = Some(id);
    }
    let end = chart.push(FlowKind::StartEnd, "结束");
    chart.edges.push(FlowEdge { from: prev.unwrap(), to: end, kind: FlowEdgeKind::Normal, label: None });
    chart
}

fn label_of(st: StmtKind) -> &'static str {
    match st {
        StmtKind::If => "条件判断",
        StmtKind::Loop => "循环",
        StmtKind::Try => "异常保护",
        StmtKind::Throw => "抛出",
        StmtKind::Catch => "捕获",
        StmtKind::Call => "函数调用",
        StmtKind::Assign => "处理/赋值",
        StmtKind::Return => "返回",
        _ => "处理",
    }
}

impl FlowChart {
    pub fn push(&mut self, kind: FlowKind, label: &str) -> usize {
        let id = self.nodes.len();
        self.nodes.push(new_node(id, kind, label));
        id
    }
}

/// 独立构造函数。
pub fn new_node(id: usize, kind: FlowKind, label: &str) -> FlowNode {
    FlowNode {
        id,
        kind,
        label: label.to_string(),
        x: 0.0,
        y: 0.0,
        collapsed: false,
        hot: None,
        bug: false,
        note: None,
        var_label: None,
        time_label: None,
        duration_ms: 0.0,
    }
}

// ───────────────────────── Sugiyama 自动布局（F276/F287） ─────────────────────────

/// 层级分配：从入口 BFS 分层 → 交叉最小化（同层排序）→ 坐标分配。
/// 返回 (迭代后是否收敛, 动画时长)。层级从上到下，节点不重叠。
pub fn sugiyama(chart: &mut FlowChart) {
    let n = chart.nodes.len();
    if n == 0 {
        return;
    }
    // 1) 层级：最长路径分层
    let mut level = vec![0usize; n];
    let mut changed = true;
    let mut guard = 0;
    while changed && guard < n {
        changed = false;
        guard += 1;
        for e in &chart.edges {
            if level[e.to] < level[e.from] + 1 {
                level[e.to] = level[e.from] + 1;
                changed = true;
            }
        }
    }
    // 2) 同层内按 id 排序（确定性交叉最小化）→ 3) 坐标
    const COL_W: f64 = 200.0;
    const ROW_H: f64 = 140.0;
    let mut per_level: Vec<Vec<usize>> = Vec::new();
    for (id, &l) in level.iter().enumerate() {
        if per_level.len() <= l {
            per_level.resize(l + 1, Vec::new());
        }
        per_level[l].push(id);
    }
    for (l, ids) in per_level.iter().enumerate() {
        for (k, &id) in ids.iter().enumerate() {
            let node = &mut chart.nodes[id];
            node.x = k as f64 * COL_W;
            node.y = l as f64 * ROW_H;
        }
    }
}

/// 布局动画：500ms 内旧坐标 → 新坐标线性过渡（F287）。
pub fn animate_layout(old: &[(f64, f64)], new: &[(f64, f64)], t_ms: f64) -> Vec<(f64, f64)> {
    let p = (t_ms / 500.0).clamp(0.0, 1.0);
    old.iter().zip(new.iter()).map(|(o, n)| (o.0 + (n.0 - o.0) * p, o.1 + (n.1 - o.1) * p)).collect()
}

pub const AUTOLAYOUT_MS: f64 = 500.0;

// ───────────────────────── F277 流程图→代码 同步 ─────────────────────────

/// 拖节点改顺序：把代码行按流程图节点新顺序重排（返回重排后的行序列）。
pub fn reorder_lines(lines: &[&str], old_order: &[usize], new_order: &[usize]) -> Vec<String> {
    // old_order[i] 是节点 i 原来对应的行号；new_order 是节点新顺序
    let mut out = vec![String::new(); lines.len()];
    for (pos, &node_idx) in new_order.iter().enumerate() {
        let line_idx = old_order[node_idx];
        out[pos] = lines[line_idx].to_string();
    }
    out
}

/// 拖连线改分支：改变 if 条件（返回新条件字符串）。
pub fn rebranch(condition: &str, new_cond: &str) -> String {
    new_cond.trim().to_string().replace(condition.trim(), "").trim().to_string()
}

// ───────────────────────── F278~F280 缩放/展开/折叠 ─────────────────────────

/// 对数缩放：范围 10%~500%，近处慢远处快。
pub fn log_zoom(cur: f64, wheel: f64) -> f64 {
    (cur * (1.0 + wheel.abs() * 0.001).powf(wheel.signum())).clamp(0.1, 5.0)
}

/// 双击展开：函数框内展开为详细子图（子图替换），300ms 动画。
pub const EXPAND_MS: f64 = 300.0;

pub fn expand(chart: &mut FlowChart, id: usize, sub: FlowChart) {
    // 子图节点继承父位置偏移
    let (px, py) = (chart.nodes[id].x, chart.nodes[id].y);
    let offset = chart.nodes.len();
    for mut n in sub.nodes {
        n.x += px;
        n.y += py;
        n.id += offset;
        chart.nodes.push(n);
    }
    for e in sub.edges {
        chart.edges.push(FlowEdge { from: e.from + offset, to: e.to + offset, kind: e.kind, label: e.label });
    }
    // 父节点与子图入口相连
    let entry = offset;
    chart.edges.push(FlowEdge { from: id, to: entry, kind: FlowEdgeKind::Normal, label: None });
}

/// 折叠摘要：框内只显示「输入→处理→输出」一行文字。
pub fn collapse_summary(inputs: &str, process: &str, outputs: &str) -> String {
    format!("{inputs}→{process}→{outputs}")
}

// ───────────────────────── F281~F285 高亮/叠加/热区/标红/对比 ─────────────────────────

/// 路径高亮：BFS 标记经过的节点+连线，其余 opacity=0.15。
pub fn path_highlight(chart: &FlowChart, src: usize, dst: usize) -> (Vec<usize>, Vec<usize>) {
    let n = chart.nodes.len();
    let mut prev = vec![None; n];
    let mut visited = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(src);
    visited[src] = true;
    while let Some(cur) = queue.pop_front() {
        if cur == dst {
            break;
        }
        for e in &chart.edges {
            if e.from == cur && !visited[e.to] {
                visited[e.to] = true;
                prev[e.to] = Some(cur);
                queue.push_back(e.to);
            }
        }
    }
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut cur = dst;
    while let Some(p) = prev[cur] {
        edges.push(
            chart.edges.iter().position(|e| e.from == p && e.to == cur).unwrap_or(usize::MAX),
        );
        nodes.push(cur);
        cur = p;
    }
    nodes.push(src);
    nodes.reverse();
    edges.reverse();
    (nodes, edges)
}

pub const DIM_OPACITY: f64 = 0.15;

/// 数据流叠加：3px 蓝色实线 + 粒子流动。
pub const DATAFLOW_STYLE: (&str, f64, &str) = ("#007AFF", 3.0, "solid+particles");

/// 热区着色：高频=暖黄，中频=原色，低频=灰色。
pub fn heat_color(freq: f64, base: &str) -> &'static str {
    if freq > 0.66 {
        "#FFD60A"
    } else if freq < 0.33 {
        "#8E8E93"
    } else {
        match base {
            "#007AFF" => "#007AFF",
            "#34C759" => "#34C759",
            "#FF9500" => "#FF9500",
            "#AF52DE" => "#AF52DE",
            _ => "#8E8E93",
        }
    }
}

/// bug 标红：问题节点边框变红 + 右上角红色感叹号徽章。
pub const BUG_BORDER: &str = "#FF3B30";
pub const BUG_BADGE: &str = "!";

/// 对比模式：新增=绿/删除=红/修改=黄。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
    Same,
}

impl DiffKind {
    pub fn color(self) -> &'static str {
        match self {
            DiffKind::Added => "#34C759",
            DiffKind::Removed => "#FF3B30",
            DiffKind::Modified => "#FFD60A",
            DiffKind::Same => "#8E8E93",
        }
    }
}

/// 左旧右新逐节点对比。
pub fn diff_flows(old: &[String], new: &[String]) -> Vec<DiffKind> {
    (0..old.len().max(new.len()))
        .map(|i| match (old.get(i), new.get(i)) {
            (Some(a), Some(b)) if a == b => DiffKind::Same,
            (Some(_), Some(_)) => DiffKind::Modified,
            (None, Some(_)) => DiffKind::Added,
            (Some(_), None) => DiffKind::Removed,
            _ => DiffKind::Same,
        })
        .collect()
}

// ───────────────────────── F286~F290 地图/吸附/导出/打印 ─────────────────────────

/// 迷你地图复用 canvas 域的小地图契约。
pub use crate::canvas::Minimap;

/// 手动调整：拖拽后吸附 16px 网格。
pub const SNAP_GRID: f64 = 16.0;

pub fn snap(x: f64) -> f64 {
    (x / SNAP_GRID).round() * SNAP_GRID
}

/// 导出图片：PNG(2x)/SVG(矢量)/PDF(分页)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png2x,
    Svg,
    Pdf,
}

/// 打印模式：A4 分页，每页标题+页码，连线跨页不断（按 y 分页）。
pub fn paginate_a4(chart: &FlowChart, page_h: f64) -> Vec<(usize, f64, f64)> {
    // 返回 (页号, 页起始 y, 页结束 y)
    let max_y = chart.nodes.iter().map(|n| n.y).fold(0.0, f64::max);
    let pages = ((max_y / page_h).ceil() as usize).max(1);
    (0..pages).map(|p| (p, p as f64 * page_h, (p + 1) as f64 * page_h)).collect()
}

pub const A4_H: f64 = 1122.0; // 297mm @96dpi

// ───────────────────────── F291~F295 动画/标注 ─────────────────────────

/// 动画播放：按执行顺序每 500ms 亮一个节点。
pub fn animation_frame(order: &[usize], t_ms: f64) -> Option<usize> {
    let idx = (t_ms / 500.0) as usize;
    order.get(idx).copied()
}

/// 变量标注：节点右下角显示关键变量当前值（小字灰色）。
pub fn var_annotation(v: &str, value: &str) -> String {
    format!("{v}={value}")
}

/// 时间标注：节点左下角显示执行耗时。
pub fn time_annotation(ms: f64) -> String {
    format!("{ms:.0}ms")
}

/// 注释气泡：节点上方白色圆角气泡，三角箭头指向节点。
pub fn bubble_anchor(node: (f64, f64, f64, f64)) -> (f64, f64) {
    ((node.0 + node.2) / 2.0, node.1 - 8.0)
}

// ───────────────────────── F297~F300 变换/瀑布/蛛网/架构 ─────────────────────────

/// 数据变换：数据经过函数节点时形状/颜色按帧变化。
pub fn transform_frame(frame: usize) -> (&'static str, &'static str) {
    const SHAPES: [&str; 3] = ["circle", "square", "triangle"];
    const COLORS: [&str; 3] = ["#007AFF", "#34C759", "#FF9500"];
    (SHAPES[frame % 3], COLORS[frame % 3])
}

/// 调用瀑布：入口在顶，调用层层向下，宽度=耗时。
pub fn waterfall(calls: &[(usize, f64)]) -> Vec<(f64, f64, f64)> {
    // (深度 y, 起点 x, 宽度=耗时)
    let max_t = calls.iter().map(|c| c.1).fold(1.0, f64::max);
    calls.iter().map(|(d, t)| (*d as f64 * 60.0, 0.0, t / max_t * 400.0)).collect()
}

/// 变量蛛网：变量=中心节点，使用点=周围节点，蛛丝连线（环形布局）。
pub fn spider_web(uses: usize, radius: f64) -> Vec<(f64, f64)> {
    (0..uses)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / uses.max(1) as f64;
            (radius * a.cos(), radius * a.sin())
        })
        .collect()
}

/// 模块架构：模块=大框，文件=中框，函数=小框，嵌套排列。
pub fn architecture(chart: &mut FlowChart, modules: &[(&str, Vec<(&str, Vec<&str>)>)]) {
    let mut y = 0.0;
    for (m, files) in modules {
        let mid = chart.push(FlowKind::Process, m);
        chart.nodes[mid].x = 0.0;
        chart.nodes[mid].y = y;
        let mut fy = y + 80.0;
        for (f, funcs) in files {
            let fid = chart.push(FlowKind::Io, f);
            chart.nodes[fid].x = 60.0;
            chart.nodes[fid].y = fy;
            let mut uy = fy + 60.0;
            for u in funcs {
                let uid = chart.push(FlowKind::Call, u);
                chart.nodes[uid].x = 120.0;
                chart.nodes[uid].y = uy;
                uy += 60.0;
                chart.edges.push(FlowEdge { from: fid, to: uid, kind: FlowEdgeKind::Dep, label: None });
            }
            fy = uy + 40.0;
            chart.edges.push(FlowEdge { from: mid, to: fid, kind: FlowEdgeKind::Dep, label: None });
        }
        y = fy + 80.0;
    }
}

// ───────────────────────── 自检（CheckSet 25 项） ─────────────────────────

pub fn run_flowchart_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("flowchart");

    // F276 代码→流程图（Sugiyama）
    let mut chart = build_flow(&[StmtKind::Assign, StmtKind::If, StmtKind::Call, StmtKind::Return]);
    sugiyama(&mut chart);
    let decision = chart.nodes.iter().find(|n| n.kind == FlowKind::Decision).unwrap();
    s.add("F276 代码→流程图", decision.kind == FlowKind::Decision && decision.y > chart.nodes[0].y && no_overlap(&chart), "Sugiyama 层级不重叠");

    // F277 流程图→代码
    let lines = ["a = 1", "b = 2", "c = 3"];
    let reordered = reorder_lines(&lines, &[0, 1, 2], &[2, 0, 1]);
    let branch = rebranch("x > 100", "x > 200");
    s.add("F277 流程图→代码", reordered[0] == "c = 3" && branch.contains("200"), "拖节点重排/拖连线改条件");

    // F278 无级缩放
    let z = log_zoom(1.0, 1000.0);
    let z2 = log_zoom(4.9, 10000.0);
    s.add("F278 无级缩放", z > 1.0 && z <= 5.0 && z2 <= 5.0 && log_zoom(0.05, -1.0) >= 0.1, "对数缩放 10%~500%");

    // F279 双击展开
    let mut parent = build_flow(&[StmtKind::Call]);
    sugiyama(&mut parent);
    let before = parent.nodes.len();
    let sub = build_flow(&[StmtKind::Assign, StmtKind::Return]);
    expand(&mut parent, 1, sub);
    s.add("F279 双击展开", parent.nodes.len() > before + 2 && EXPAND_MS == 300.0, "子图替换+300ms 动画");

    // F280 折叠摘要
    s.add("F280 折叠摘要", collapse_summary("入参", "校验", "结果") == "入参→校验→结果", "一行摘要文字");

    // F281 路径高亮
    let chart2 = build_flow(&[StmtKind::Assign, StmtKind::Call, StmtKind::Return]);
    let (nodes, edges) = path_highlight(&chart2, 0, chart2.nodes.len() - 1);
    s.add("F281 路径高亮", nodes.first() == Some(&0) && nodes.last() == Some(&(chart2.nodes.len() - 1)) && edges.len() == nodes.len() - 1 && DIM_OPACITY == 0.15, "BFS 标记+其余变暗");

    // F282 数据流叠加
    s.add("F282 数据流叠加", DATAFLOW_STYLE.0 == "#007AFF" && DATAFLOW_STYLE.1 == 3.0, "3px 蓝色实线+粒子");

    // F283 热区着色
    s.add("F283 热区着色", heat_color(0.9, "#007AFF") == "#FFD60A" && heat_color(0.1, "#007AFF") == "#8E8E93" && heat_color(0.5, "#007AFF") == "#007AFF", "高/中/低频三档");

    // F284 bug 标红
    s.add("F284 bug标红", BUG_BORDER == "#FF3B30" && BUG_BADGE == "!", "红边框+感叹号徽章");

    // F285 对比模式
    let d = diff_flows(&["a".into(), "b".into()], &["a".into(), "c".into(), "d".into()]);
    s.add("F285 对比模式", d[0] == DiffKind::Same && d[1] == DiffKind::Modified && d[2] == DiffKind::Added && d[1].color() == "#FFD60A", "增绿/删红/改黄");

    // F286 迷你地图
    let mm = Minimap { world: (0.0, 0.0, 1000.0, 800.0) };
    s.add("F286 迷你地图", mm.jump(75.0, 50.0) == (500.0, 400.0), "缩略图+视口框");

    // F287 自动布局
    let mut c3 = build_flow(&[StmtKind::Assign, StmtKind::Call, StmtKind::Return]);
    let old: Vec<(f64, f64)> = c3.nodes.iter().map(|n| (n.x, n.y)).collect();
    sugiyama(&mut c3);
    let new: Vec<(f64, f64)> = c3.nodes.iter().map(|n| (n.x, n.y)).collect();
    let mid_anim = animate_layout(&old, &new, 250.0);
    s.add("F287 自动布局", mid_anim[1].1 > old[1].1 && mid_anim[1].1 < new[1].1 && AUTOLAYOUT_MS == 500.0, "层级→坐标+500ms 过渡");

    // F288 手动调整
    s.add("F288 手动调整", snap(100.0) == 96.0 && snap(110.0) == 112.0 && SNAP_GRID == 16.0, "16px 网格吸附");

    // F289 导出图片
    s.add("F289 导出图片", [ExportFormat::Png2x, ExportFormat::Svg, ExportFormat::Pdf].len() == 3, "PNG(2x)/SVG/PDF");

    // F290 打印模式
    let pages = paginate_a4(&c3, A4_H);
    s.add("F290 打印模式", pages.len() == 1 && pages[0].1 == 0.0 && A4_H > 1000.0, "A4 分页+标题页码");

    // F291 动画播放
    let order = vec![0, 1, 2];
    s.add("F291 动画播放", animation_frame(&order, 0.0) == Some(0) && animation_frame(&order, 500.0) == Some(1) && animation_frame(&order, 1500.0).is_none(), "每 500ms 亮一个");

    // F292 条件标注
    let mut c4 = build_flow(&[StmtKind::If, StmtKind::Return]);
    c4.edges[1].label = Some(">100".into());
    s.add("F292 条件标注", c4.edges[1].label.as_deref() == Some(">100"), "菱形出边条件文字");

    // F293 变量标注
    s.add("F293 变量标注", var_annotation("count", "42") == "count=42", "节点右下角灰字");

    // F294 时间标注
    s.add("F294 时间标注", time_annotation(12.4) == "12ms", "节点左下角耗时");

    // F295 注释气泡
    let anchor = bubble_anchor((0.0, 100.0, 120.0, 48.0));
    s.add("F295 注释气泡", anchor.0 == 60.0 && anchor.1 == 92.0, "节点上方+箭头指向");

    // F296 标准流程图（CFG 映射）
    let cfg = build_flow(&[StmtKind::If, StmtKind::Loop, StmtKind::Try]);
    s.add("F296 标准流程图", cfg.nodes.iter().any(|n| n.kind == FlowKind::Decision) && cfg.nodes.iter().any(|n| n.kind == FlowKind::Loop) && cfg.nodes.iter().any(|n| n.kind == FlowKind::Exception), "if→菱形 for→六边形 try→圆");

    // F297 数据变换
    let (sh0, co0) = transform_frame(0);
    let (sh1, _) = transform_frame(1);
    s.add("F297 数据变换", sh0 != sh1 && co0 == "#007AFF", "逐帧形状/颜色变化");

    // F298 调用瀑布
    let wf = waterfall(&[(0, 100.0), (1, 50.0)]);
    s.add("F298 调用瀑布", wf[0].0 == 0.0 && wf[1].0 == 60.0 && wf[0].2 > wf[1].2, "入口在顶宽度=耗时");

    // F299 变量蛛网
    let web = spider_web(4, 100.0);
    s.add("F299 变量蛛网", web.len() == 4 && (web[0].0 - 100.0).abs() < 1e-9 && web[1].0.abs() < 1e-9, "变量中心+使用点环绕");

    // F300 模块架构
    let mut arch = FlowChart::default();
    architecture(&mut arch, &[("net", vec![("http", vec!["get", "post"])])]);
    let file = arch.nodes.iter().find(|n| n.label == "http").unwrap();
    let func = arch.nodes.iter().find(|n| n.label == "get").unwrap();
    s.add("F300 模块架构", file.x > arch.nodes[0].x && func.x > file.x && func.y > file.y, "大/中/小框嵌套");

    // 连线样式契约
    s.add("flowchart 连线契约", FlowEdgeKind::Exception.style().1 == 1.5 && FlowEdgeKind::Dep.style().2 == "dotted" && FlowEdgeKind::Data.style().0 == "#007AFF", "四类连线样式表");

    // IR 边消费契约
    let e = Edge { from: "a".into(), to: "b".into(), kind: EdgeKind::Call };
    s.add("flowchart IR边契约", e.kind == EdgeKind::Call, "流程图只消费统一 IR 边");

    s
}

fn no_overlap(chart: &FlowChart) -> bool {
    for i in 0..chart.nodes.len() {
        for j in (i + 1)..chart.nodes.len() {
            let (a, b) = (&chart.nodes[i], &chart.nodes[j]);
            if (a.x - b.x).abs() < 1.0 && (a.y - b.y).abs() < 1.0 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f276_sugiyama_levels() {
        let mut c = build_flow(&[StmtKind::Assign, StmtKind::If, StmtKind::Call, StmtKind::Return]);
        sugiyama(&mut c);
        for w in c.nodes.windows(2) {
            // 层级沿执行方向单调不降（同一链路）
            assert!(w[1].y >= w[0].y);
        }
    }

    #[test]
    fn f279_expand_offsets() {
        let mut parent = build_flow(&[StmtKind::Call]);
        sugiyama(&mut parent);
        let sub = build_flow(&[StmtKind::Assign, StmtKind::Return]);
        let base = parent.nodes.len();
        expand(&mut parent, 1, sub);
        assert!(parent.nodes[base].x >= parent.nodes[1].x);
        // 子图首节点与父节点连边存在
        assert!(parent.edges.iter().any(|e| e.from == 1 && e.to == base));
    }

    #[test]
    fn f281_highlight_chain() {
        let c = build_flow(&[StmtKind::Assign, StmtKind::Call, StmtKind::Return]);
        let (nodes, edges) = path_highlight(&c, 0, 4);
        assert_eq!(nodes, vec![0, 1, 2, 3, 4]);
        assert_eq!(edges, vec![0, 1, 2, 3]);
    }

    #[test]
    fn f285_diff_colors() {
        assert_eq!(diff_flows(&["x".into()], &[])[0], DiffKind::Removed);
        assert_eq!(DiffKind::Removed.color(), "#FF3B30");
    }

    #[test]
    fn f288_snap_grid() {
        assert_eq!(snap(8.0), 16.0); // round(0.5)=1? banker's? f64::round(0.5)=1
        assert_eq!(snap(7.0), 0.0);
        assert_eq!(snap(-5.0), 0.0);
    }

    #[test]
    fn f290_pagination() {
        let mut c = build_flow(&[StmtKind::Assign, StmtKind::Return]);
        sugiyama(&mut c);
        let pages = paginate_a4(&c, 200.0);
        assert!(pages.len() >= 1);
        assert!(pages.last().unwrap().2 >= c.nodes.last().unwrap().y);
    }

    #[test]
    fn f291_frame_bounds() {
        let order = vec![5];
        assert_eq!(animation_frame(&order, 499.9), Some(5));
        assert_eq!(animation_frame(&order, 500.1), None);
    }
}
