//! 核心数据模型：统一语义 IR 的基础结构（部署总纲 core 输出契约的 W1 落点）。
//!
//! 七级节点层级：项目→模块→子系统→文件→类/结构→函数→行。

/// 节点种类（Flyweight 内部以 u8 共享，见 infra F011）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Project,
    Module,
    Subsystem,
    File,
    Class,
    Func,
    Stmt,
    Line,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Project => "project",
            NodeKind::Module => "module",
            NodeKind::Subsystem => "subsystem",
            NodeKind::File => "file",
            NodeKind::Class => "class",
            NodeKind::Func => "func",
            NodeKind::Stmt => "stmt",
            NodeKind::Line => "line",
        }
    }

    /// 七级下钻层级（0=项目 … 6=行），C03 契约。
    pub fn level(self) -> u8 {
        match self {
            NodeKind::Project => 0,
            NodeKind::Module => 1,
            NodeKind::Subsystem => 2,
            NodeKind::File => 3,
            NodeKind::Class => 4,
            NodeKind::Func => 5,
            _ => 6,
        }
    }
}

/// 语句种类（逻辑链与 CFG 消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StmtKind {
    Call,
    Assign,
    If,
    Else,
    Loop,
    Return,
    Throw,
    Try,
    Catch,
    Spawn,
    Lock,
    Unlock,
    Emit,
    On,
    New,
    Free,
    Switch,
    Case,
    Other,
}

impl StmtKind {
    pub fn parse(line: &str) -> StmtKind {
        let t = line.trim_start();
        let first = t.split_whitespace().next().unwrap_or("");
        match first {
            "if" => StmtKind::If,
            "else" => StmtKind::Else,
            "for" | "while" => StmtKind::Loop,
            "return" => StmtKind::Return,
            "throw" | "raise" => StmtKind::Throw,
            "try" => StmtKind::Try,
            "catch" | "except" => StmtKind::Catch,
            "switch" | "match" => StmtKind::Switch,
            "case" => StmtKind::Case,
            "spawn" | "thread" | "go " => StmtKind::Spawn,
            "lock" => StmtKind::Lock,
            "unlock" => StmtKind::Unlock,
            "emit" | "publish" => StmtKind::Emit,
            "on" | "subscribe" => StmtKind::On,
            _ => {
                let rhs_call = t.split('=').last().map_or(false, |r| r.contains('(') && !r.contains("=="));
                if t.contains('=') && !t.contains("==") && rhs_call {
                    // 赋值右值为调用：本质是 Call（如 `cfg = load(url)`）
                    StmtKind::Call
                } else if t.contains('=') && !t.contains("==") {
                    StmtKind::Assign
                } else if t.contains('(') {
                    StmtKind::Call
                } else {
                    StmtKind::Other
                }
            }
        }
    }
}

/// AST/IR 节点。子节点用索引引用，树存储在 `Vec` 中（扁平化，便于虚拟化/快照）。
#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub kind: NodeKind,
    pub name: String,
    /// 源内行区间 [start, end)，半开区间，0 基。
    pub span: (usize, usize),
    pub children: Vec<usize>,
    /// 语句种类（仅 Stmt/Line 有效）。
    pub stmt: Option<StmtKind>,
    /// 语句原文（仅行级，供续写/翻译消费）。
    pub text: Option<String>,
    /// 调用的目标名（Call 语句）。
    pub callee: Option<String>,
    /// 赋值左侧变量名（Assign 语句）。
    pub target: Option<String>,
}

impl Node {
    pub fn new(id: usize, kind: NodeKind, name: impl Into<String>, span: (usize, usize)) -> Self {
        Node {
            id,
            kind,
            name: name.into(),
            span,
            children: Vec::new(),
            stmt: None,
            text: None,
            callee: None,
            target: None,
        }
    }
}

/// 一棵扁平化 AST/IR 树。
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
}

impl Tree {
    pub fn with_root(name: &str, kind: NodeKind) -> Self {
        let mut t = Tree::default();
        t.root = 0;
        t.nodes.push(Node::new(0, kind, name, (0, 0)));
        t
    }

    pub fn add(&mut self, parent: usize, node: Node) -> usize {
        let id = self.nodes.len();
        let mut n = node;
        n.id = id;
        self.nodes.push(n);
        self.nodes[parent].children.push(id);
        id
    }

    pub fn node(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    /// 先序遍历。
    pub fn walk<F: FnMut(&Node)>(&self, id: usize, f: &mut F) {
        f(&self.nodes[id]);
        for &c in &self.nodes[id].children {
            self.walk(c, f);
        }
    }
}

/// 跨文件有向边（调用边/数据流边/事件边等共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Call,
    DataFlow,
    Event,
    Contains,
}

/// 项目级统一语义 IR（部署总纲 core 输出契约）。
#[derive(Debug, Default)]
pub struct ProjectIR {
    pub name: String,
    pub loc: usize,
    pub file_count: usize,
    pub lang_stats: Vec<(String, usize)>,
    pub tree: Tree,
    pub calls: Vec<Edge>,
    pub dataflow: Vec<Edge>,
    pub events: Vec<Edge>,
}

impl ProjectIR {
    /// 七级下钻：取某节点的下一层子节点（C03 数据模型）。
    pub fn drill(&self, id: usize) -> Vec<usize> {
        self.tree.nodes[id].children.clone()
    }
}
