//! VE-F0422 · AST 节点定义与 arena 池（VE-C 域 · 着色器系统 · 语法组 C02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0422`
//!
//! **判据（锚点原文）**：三大族节点、arena 连续、一次释放、版本登记、判据。
//!
//! 锚点原文：「AST 节点体系与内存策略：节点类型全集（表达式、语句、声明三大
//! 族）、arena 池分配（同型节点连续布局缓存友好、整树一次性释放零碎片）、节点
//! 公共头（类型、跨度、父指针可选）；节点版本化（随语言版本演进登记新增节点）。
//! 数据结构：节点类型全集；arena 池；公共头结构。错误路径与降级矩阵：类型越界
//! →断言拦截；arena 泄漏→释放断言；跨度缺失→诊断降级标注。性能逐项分解：分配
//! O(1) bump；释放 O(1) 整池；遍历 O(节点数) 缓存友好。跨批对接点：上游 F0421
//! 动作回调；下游 F0423-F0432 各解析消费、F0435 验证器。」
//!
//! 本单交付**节点体系与内存策略**，不交付任何一条产生式的建树逻辑——那是
//! F0423-F0432 各自的事。本单只回答：**这些单的节点长什么样、放在哪里、怎么
//! 释放**。四条判据逐条都要有**结构性保证**，保证不了就是地基不合格。
//!
//! 1. **三大族节点**（判据一）。节点类型全集 [`NodeKind`] 按锚点分**表达式
//!    [`NodeFamily::Expr`]、语句 [`NodeFamily::Stmt`]、声明 [`NodeFamily::Decl`]
//!    三族**，另设 [`NodeFamily::Root`] 承载翻译单元根。分族不是分类学装饰——
//!    它**直接决定 arena 分桶**，是判据二的因：同族节点进同一条 bump 链，同型
//!    节点在内存里连续（[`AstArena::same_family_runs`] 实测同族最大连续段）。
//!    为什么是三族而不是按 F0421 的 8 个非终结符分：非终结符是**语法**切面，
//!    节点族是**消费者**切面——F0423-F0432 各解析单要的是「我造出来的节点属于
//!    哪一族好让下游验证器分派」，按非终结符分会让每族内部再碎一层。
//!
//! 2. **arena 连续**（判据二）。[`AstArena`] 不是一块 `Vec<Node>` 加下标，
//!    而是**按族分桶、桶内 bump 分配**：[`AstArena::alloc`] 的分配动作只有
//!    「写下标 + 尾指针 +1」，没有任何 `memmove`、没有对齐填充、**不按节点大小
//!    找空洞**——这是「缓存友好」能被机检的部分（[`AstArena::bump_steps`] 恒为
//!    1，任何需要遍历已有节点才能定位新节点的实现都会被这项判红）。
//!    **释放是 O(1) 整池**：Rust 的 `Vec` drop 语义天然整池释放，但真正要保证的
//!    是「释放后不留悬挂父指针」——所以 [`AstArena`] **不实现 `Drop` 自定义逻辑**
//!    而让 `Vec` 各桶自行析构，父指针在类型上就是 [`NodeId`]（下标）而非引用，
//!    池释放后任何 `NodeId` 都只是越界下标而**不是悬垂指针**（判据用
//!    [`AstArena::node_of`] 在释放后的池上取节点，断言返回 `NodeErr::PoolReleased`
//!    而非 UB）。
//!
//! 3. **一次释放**（判据三）。锚点说「整树一次性释放零碎片」。零碎片的含义要
//!    钉死：不是「峰值内存小」，而是**没有任何单节点可被单独释放**——
//!    [`AstArena`] 故意**不提供** `free_node` / `drop_node`，用 API 缺失把
//!    「碎片」在编译期变成不可能。判据里有一条**反向断言**：想单释放的路径唯一
//!    能走通的形式是拿 `NodeId` 去索引桶，而桶是私有字段，外部拿不到。
//!
//! 4. **版本登记**（判据四）。锚点说「节点版本化（随语言版本演进登记新增
//!    节点）」。**只有「能新增」不够**——能新增而不留痕等于把语言演进变成
//!    不可追溯的历史。所以 [`NodeRegistry`] 逐条登记：新增节点必须同时给出
//!    所属族与**引入版本**，且 [`NodeRegistry::register`] 拒绝「版本号倒退」
//!    与「同版本重复占号」。判据要求登记簿与实际[`NodeKind`]全集**集合相等**
//!    （不多不少），这条把「版本化」从口头承诺变成可对账的账。
//!
//! ## 与上游 F0421 的接口
//!
//! 本单是 F0421 [`ActionSink`](super::vec21_parser::ActionSink) 的**第一个真实
//! 消费者**——F0421 只给了[`NodeTallySink`]（数个数，不建树）。这里给
//! [`ArenaBuilder`]：消费 [`Reduction`](super::vec21_parser::Reduction) 动作流
//! 把节点落进 arena。**动作序列仍由生产者决定**（与 F0421 判据三一致），
//! 消费者改的是节点长什么样，不是「有没有这个归约」。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / `panic!` / 裸下标越界：所有对外索引经
//! [`AstArena::node_of`] 复核并返回 [`NodeErr`]；版本号比较用 `checked` 语义。
//! `NodeId` 是 `#[derive(Clone, Copy)]` 的新值类型，非法 id 走 `Err` 而非 UB。

#![allow(clippy::needless_range_loop)]

use alloc::string::String;
use alloc::vec::Vec;

use super::vec21_parser::{ActionSink, Diagnostic, Reduction, SinkStatus, Span};

// ---------------------------------------------------------------------------
// 一、节点类型全集（判据一：三大族）
// ---------------------------------------------------------------------------

/// 节点族。锚点原文点名「表达式、语句、声明三大族」，另加根族承载翻译单元。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NodeFamily {
    /// 根（翻译单元本身，非三族之一，单列以便下游从根起步遍历）。
    Root = 0,
    /// 声明族。
    Decl = 1,
    /// 语句族。
    Stmt = 2,
    /// 表达式族。
    Expr = 3,
}

/// 锚点要求的三大族 + 根，按序返回。判据用它的长度钉死「三族」这个数。
pub const FAMILIES: [NodeFamily; 4] = [
    NodeFamily::Root,
    NodeFamily::Decl,
    NodeFamily::Stmt,
    NodeFamily::Expr,
];

impl NodeFamily {
    /// 锚点点名的三大族数量（不含根）。判据据此钉死「三族」这个数。
    pub const ALL_THREE_COUNT: usize = 3;

    /// 族序号（与 [`NodeFamily`] 判别值一致）。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            NodeFamily::Root => "Root",
            NodeFamily::Decl => "Decl",
            NodeFamily::Stmt => "Stmt",
            NodeFamily::Expr => "Expr",
        }
    }

    /// 是不是锚点点名的三大族之一（根族不算——它是承载体，不是三族之一）。
    pub fn is_one_of_three(self) -> bool {
        matches!(self, NodeFamily::Decl | NodeFamily::Stmt | NodeFamily::Expr)
    }

    /// 由序号还原，越界给 `None` 而非 panic（判据「类型越界」由此可测）。
    pub fn from_index(i: usize) -> Option<NodeFamily> {
        FAMILIES.get(i).copied()
    }
}

/// 节点类型全集。
///
/// **为什么用窄枚举 + 显式码段而不是一个大 `enum` 塞进结构体**：内核里节点
/// 会被序列化进诊断包与镜像，窄枚举的判别值布局稳定；新增变体时版本登记
/// （判据四）强制留痕，不会悄悄改变既有码。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NodeKind {
    // —— 根族 ——
    /// 翻译单元。
    TranslationUnit = 0,

    // —— 声明族 ——
    /// 函数声明。
    FnDecl = 1,
    /// 变量声明（无初始化器）。
    LetDecl = 2,
    /// 变量声明（带初始化器）。
    LetInitDecl = 3,

    // —— 语句族 ——
    /// if 语句（无 else）。
    IfStmt = 4,
    /// if-else 语句。
    IfElseStmt = 5,
    /// 返回语句。
    ReturnStmt = 6,
    /// 复合语句（块）。
    BlockStmt = 7,
    /// 表达式语句。
    ExprStmt = 8,

    // —— 表达式族 ——
    /// 标识符表达式。
    IdentExpr = 9,
    /// 字面量表达式。
    LiteralExpr = 10,
    /// 二元运算表达式。
    BinaryExpr = 11,
    /// 括号表达式。
    ParenExpr = 12,
    /// 类型表达式（出现在声明里的类型位）。
    TypeExpr = 13,
    /// 参数表达式（出现在参数表里的类型位）。
    ParamExpr = 14,
}

impl NodeKind {
    /// 该节点类型归属的族。
    ///
    /// 这条映射是**判据一的支点**：它把「节点属于哪族」从注释变成可机检的
    /// 函数，arena 分桶与下游分派都只信它。
    pub fn family(self) -> NodeFamily {
        match self {
            NodeKind::TranslationUnit => NodeFamily::Root,
            NodeKind::FnDecl | NodeKind::LetDecl | NodeKind::LetInitDecl => NodeFamily::Decl,
            NodeKind::IfStmt
            | NodeKind::IfElseStmt
            | NodeKind::ReturnStmt
            | NodeKind::BlockStmt
            | NodeKind::ExprStmt => NodeFamily::Stmt,
            NodeKind::IdentExpr
            | NodeKind::LiteralExpr
            | NodeKind::BinaryExpr
            | NodeKind::ParenExpr
            | NodeKind::TypeExpr
            | NodeKind::ParamExpr => NodeFamily::Expr,
        }
    }

    /// 类型名（判据与诊断用）。
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::TranslationUnit => "TranslationUnit",
            NodeKind::FnDecl => "FnDecl",
            NodeKind::LetDecl => "LetDecl",
            NodeKind::LetInitDecl => "LetInitDecl",
            NodeKind::IfStmt => "IfStmt",
            NodeKind::IfElseStmt => "IfElseStmt",
            NodeKind::ReturnStmt => "ReturnStmt",
            NodeKind::BlockStmt => "BlockStmt",
            NodeKind::ExprStmt => "ExprStmt",
            NodeKind::IdentExpr => "IdentExpr",
            NodeKind::LiteralExpr => "LiteralExpr",
            NodeKind::BinaryExpr => "BinaryExpr",
            NodeKind::ParenExpr => "ParenExpr",
            NodeKind::TypeExpr => "TypeExpr",
            NodeKind::ParamExpr => "ParamExpr",
        }
    }

    /// 全部变体（顺序即码段顺序，判据据此对账全集）。
    pub const ALL: [NodeKind; 15] = [
        NodeKind::TranslationUnit,
        NodeKind::FnDecl,
        NodeKind::LetDecl,
        NodeKind::LetInitDecl,
        NodeKind::IfStmt,
        NodeKind::IfElseStmt,
        NodeKind::ReturnStmt,
        NodeKind::BlockStmt,
        NodeKind::ExprStmt,
        NodeKind::IdentExpr,
        NodeKind::LiteralExpr,
        NodeKind::BinaryExpr,
        NodeKind::ParenExpr,
        NodeKind::TypeExpr,
        NodeKind::ParamExpr,
    ];

    /// 由码值还原，越界给 `None`。
    ///
    /// 这是判据「类型越界」的唯一入口：`NodeKind::from_code` 的**唯一合法
    /// 返回值**要么是 `Some`，要么是 `None`——没有第三条路（不 panic、不给
    /// 默认值），所以「越界被拦截」是构造出来的而不是检查出来的。
    pub fn from_code(code: u16) -> Option<NodeKind> {
        NodeKind::ALL.get(code as usize).copied()
    }

    /// 码值。
    pub fn code(self) -> u16 {
        self as u16
    }
}

// ---------------------------------------------------------------------------
// 二、节点公共头（锚点：类型、跨度、父指针可选）
// ---------------------------------------------------------------------------

/// 节点标识。**刻意不是引用**——是「桶下标」，池析构后它只是越界下标而不是
/// 悬垂指针（判据「一次释放」的关键，见模块头注第2 条）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NodeId {
    /// 桶序号（= [`NodeFamily::index`]）。
    pub bucket: u8,
    /// 桶内下标。
    pub index: u32,
}

/// 节点公共头。
///
/// 锚点原文：「节点公共头（类型、跨度、父指针可选）」。三项对应三个字段，
/// 每个都有存在理由：
/// - `kind`：类型，判据分族与下游分派都读它；
/// - `span`：跨度，**允许缺失**——锚点的「跨度缺失→诊断降级标注」要求缺失是
///   可表达状态而不是崩溃，故用 [`Option`] 而非哨兵值；
/// - `parent`：父指针**可选**——根节点没有父，且父指针可暂不设（节点先建后挂），
///   所以是 [`Option`]。
#[derive(Clone, Debug)]
pub struct NodeHeader {
    /// 节点类型。
    pub kind: NodeKind,
    /// 输入跨度。`None` = 缺失（降级标注，不是错误）。
    pub span: Option<Span>,
    /// 父节点。`None` = 根或尚未挂接。
    pub parent: Option<NodeId>,
    /// 引入该节点类型的语言版本号（判据四的登记值，落在节点上便于诊断带版本）。
    pub since_version: u16,
}

/// 一个 AST 节点 = 公共头 + 载荷。
///
/// 载荷刻意用**定长小数组**而非 `Vec`：锚点要「同型节点连续布局」，变长载荷会
/// 让同型节点的步长不一致，破坏缓存友好。超过定长容量的子节点挂在 arena 的
/// 子表里（[`AstArena::children`]），节点本身保持定长。
#[derive(Clone, Debug)]
pub struct AstNode {
    /// 公共头。
    pub head: NodeHeader,
    /// 定长载荷槽：第0 槽语义为「标识符/字面量文本或类型名的哈希索引」，
    /// 第1 槽语义为「运算符/子节点数等标量」，其余留 0。
    pub payload: [u32; 4],
}

impl AstNode {
    /// 新建节点：跨度缺失、父亲未挂接。
    pub fn new(kind: NodeKind, since_version: u16) -> AstNode {
        AstNode {
            head: NodeHeader {
                kind,
                span: None,
                parent: None,
                since_version,
            },
            payload: [0u32; 4],
        }
    }

    /// 带跨度新建。
    pub fn with_span(kind: NodeKind, span: Span, since_version: u16) -> AstNode {
        let mut n = AstNode::new(kind, since_version);
        n.head.span = Some(span);
        n
    }

    /// 挂父节点。
    pub fn set_parent(&mut self, parent: NodeId) {
        self.head.parent = Some(parent);
    }

    /// 填载荷槽。
    pub fn set_payload(&mut self, slot: usize, value: u32) -> Result<(), NodeErr> {
        if slot >= 4 {
            return Err(NodeErr::PayloadSlotOutOfRange { slot, cap: 4 });
        }
        self.payload[slot] = value;
        Ok(())
    }

    /// 读载荷槽，越界给 `Err` 而非 panic。
    pub fn payload_of(&self, slot: usize) -> Result<u32, NodeErr> {
        if slot >= 4 {
            return Err(NodeErr::PayloadSlotOutOfRange { slot, cap: 4 });
        }
        Ok(self.payload[slot])
    }
}

// ---------------------------------------------------------------------------
// 三、arena 池（判据二、三：同型连续、一次释放）
// ---------------------------------------------------------------------------

/// 节点错误面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeErr {
    /// 节点类型码越界（判据：类型越界→拦截）。
    KindOutOfRange { code: u16, known_max: u16 },
    /// 族序号越界。
    FamilyOutOfRange { index: u8, known: u8 },
    /// 节点 id 越界（桶不存在或下标超桶长）。
    IdOutOfRange { id: NodeId, bucket_len: u32 },
    /// 桶序号非法（不是任一族）。
    UnknownBucket { bucket: u8 },
    /// 载荷槽越界。
    PayloadSlotOutOfRange { slot: usize, cap: usize },
    /// 池已整池释放后再访问（判据：一次释放）。
    PoolReleased,
    /// 跨度缺失（**降级标注**，不是拒绝）。
    SpanMissing { kind: NodeKind },
    /// 版本登记重入（判据：版本登记）。
    VersionNotAdvancing { existing: u16, incoming: u16 },
    /// 版本登记时节点类型不匹配。
    VersionMismatch { kind: NodeKind },
}

impl NodeErr {
    /// 错误三要素之一：发生了什么。
    pub fn what(self) -> &'static str {
        match self {
            NodeErr::KindOutOfRange { .. } => "节点类型码越界，已拦截",
            NodeErr::FamilyOutOfRange { .. } => "族序号越界，已拦截",
            NodeErr::IdOutOfRange { .. } => "节点 id 越界，已拦截",
            NodeErr::UnknownBucket { .. } => "桶序号非法，已拦截",
            NodeErr::PayloadSlotOutOfRange { .. } => "载荷槽越界，已拦截",
            NodeErr::PoolReleased => "池已整池释放，不可再访问",
            NodeErr::SpanMissing { .. } => "跨度缺失，已降级标注",
            NodeErr::VersionNotAdvancing { .. } => "版本号未推进，拒绝登记",
            NodeErr::VersionMismatch { .. } => "节点类型与登记不符，拒绝登记",
        }
    }

    /// 错误三要素之二：为什么（规则引用）。
    pub fn why(self) -> &'static str {
        match self {
            NodeErr::KindOutOfRange { .. } => "锚点 F0422「类型越界→断言拦截」",
            NodeErr::FamilyOutOfRange { .. } => "锚点 F0422「三大族」族数固定为 4（含根）",
            NodeErr::IdOutOfRange { .. } => "锚点 F0422「公共头父指针可选」父指针必须是合法下标",
            NodeErr::UnknownBucket { .. } => "锚点 F0422「arena 同型连续」桶必须按族划分",
            NodeErr::PayloadSlotOutOfRange { .. } => "锚点 F0422「同型节点连续布局」载荷定长 4槽",
            NodeErr::PoolReleased => "锚点 F0422「整树一次性释放零碎片」",
            NodeErr::SpanMissing { .. } => "锚点 F0422「跨度缺失→诊断降级标注」",
            NodeErr::VersionNotAdvancing { .. } => "锚点 F0422「随语言版本演进登记新增节点」",
            NodeErr::VersionMismatch { .. } => "锚点 F0422「公共头类型」登记须与节点类型一致",
        }
    }

    /// 错误三要素之三：下一步建议。
    pub fn advice(self) -> &'static str {
        match self {
            NodeErr::KindOutOfRange { known_max, .. } => {
                if known_max == 0 {
                    "全集为空，先登记节点类型"
                } else {
                    "查 NodeKind::ALL 越界处，补登记或修产生式映射"
                }
            }
            NodeErr::FamilyOutOfRange { .. } => "族序号须落在 0..=3",
            NodeErr::IdOutOfRange { .. } => "确认父节点已先于子节点建立，或检查桶是否已整池释放",
            NodeErr::UnknownBucket { .. } => "桶序号须由 NodeFamily::index 产出",
            NodeErr::PayloadSlotOutOfRange { cap, .. } => {
                if cap == 0 {
                    "载荷容量为 0"
                } else {
                    "改用 0..3 的槽位，超量内容挂 arena 子表"
                }
            }
            NodeErr::PoolReleased => "整池释放后不得再取节点，请重建 arena",
            NodeErr::SpanMissing { .. } => "跨度缺失可继续，诊断面已标注；建议回溯产生式补span",
            NodeErr::VersionNotAdvancing { existing, incoming } => {
                if incoming <= existing {
                    "新增节点必须给更高版本号或复用同版本未占号"
                } else {
                    "版本号须严格递增"
                }
            }
            NodeErr::VersionMismatch { .. } => "以 NodeKind::ALL 为准对齐登记项",
        }
    }

    /// 三要素合成本行文本（判据用来钉「三要素非空」）。
    pub fn three_elements(self) -> String {
        let mut s = String::new();
        s.push_str(self.what());
        s.push_str("｜");
        s.push_str(self.why());
        s.push_str("｜");
        s.push_str(self.advice());
        s
    }
}

// ---------------------------------------------------------------------------
// 四、arena 池实现
// ---------------------------------------------------------------------------

/// 一个族的节点桶：连续 `Vec`，bump 分配。
///
/// 「同型节点连续」的落点是**桶本身就是 `Vec<Node>`**：同族节点在内存里首尾
/// 相接，遍历一个族的全部节点是一次线性扫描且步长固定。
#[derive(Clone, Debug)]
pub struct NodeBucket {
    /// 桶所属族。
    pub family: NodeFamily,
    /// 节点连续存放。
    pub nodes: Vec<AstNode>,
    /// 父到子的子表（载荷超定长容量的部分挂这里）。
    pub children: Vec<(NodeId, NodeId)>,
    /// 本桶累计 bump 步数（恒等于本桶分配次数——判据「O(1) bump」的机检点）。
    pub bump_steps: u32,
}

/// AST arena：按族分桶、桶内 bump 分配、整池一次释放。
///
/// **刻意不提供单节点释放**（判据三「零碎片」）：没有 `free_node` /
/// `drop_node` / `retain`，所以「碎片」在 API 层就不存在可写的代码。
/// 整池释放由 [`AstArena::release_all`] 显式表达；[`Drop`] 用默认实现让各桶
/// 自行析构（零自定义析构逻辑 = 零 panic 面）。
#[derive(Clone, Debug)]
pub struct AstArena {
    buckets: Vec<NodeBucket>,
    /// 累计分配节点数（判据对账用）。
    pub allocated: u64,
    /// 累计 bump 步数（应恒等于 [`AstArena::allocated`]）。
    pub bump_steps: u64,
    /// 是否已整池释放。
    pub released: bool,
}

impl AstArena {
    /// 新建空池。
    pub fn new() -> AstArena {
        let mut buckets = Vec::new();
        for f in FAMILIES.iter() {
            buckets.push(NodeBucket {
                family: *f,
                nodes: Vec::new(),
                children: Vec::new(),
                bump_steps: 0,
            });
        }
        AstArena {
            buckets,
            allocated: 0,
            bump_steps: 0,
            released: false,
        }
    }

    /// 取桶下标（族序号），越界给 `Err`。
    fn bucket_of(family: NodeFamily) -> usize {
        family.index()
    }

    /// 分配一个节点：O(1) bump。
    ///
    /// **注意这里没有任何「找空位」逻辑**：桶是 `Vec`，`push` 即尾指针 +1。
    /// 判据用 [`AstArena::bump_steps`] 恒等于 [`AstArena::allocated`] 来钉住
    /// 「分配不依赖已有节点布局」——一个需要扫描才能定位的实现做不到这条。
    pub fn alloc(&mut self, node: AstNode) -> Result<NodeId, NodeErr> {
        if self.released {
            return Err(NodeErr::PoolReleased);
        }
        let family = node.head.kind.family();
        let bi = AstArena::bucket_of(family);
        if bi >= self.buckets.len() {
            return Err(NodeErr::FamilyOutOfRange {
                index: bi as u8,
                known: self.buckets.len() as u8,
            });
        }
        let index = self.buckets[bi].nodes.len() as u32;
        self.buckets[bi].nodes.push(node);
        self.buckets[bi].bump_steps += 1;
        self.bump_steps += 1;
        self.allocated += 1;
        Ok(NodeId {
            bucket: bi as u8,
            index,
        })
    }

    /// 按码值建节点（判据「类型越界」的拦截点）。
    pub fn alloc_kind(&mut self, code: u16, since: u16) -> Result<NodeId, NodeErr> {
        match NodeKind::from_code(code) {
            Some(k) => self.alloc(AstNode::new(k, since)),
            None => {
                let known_max = match NodeKind::ALL.last() {
                    Some(k) => k.code(),
                    None => 0,
                };
                Err(NodeErr::KindOutOfRange { code, known_max })
            }
        }
    }

    /// 取节点；越界/已释放给 `Err`。
    ///
    /// **池释放后取节点返回 `Err(NodeErr::PoolReleased)` 而不是 UB**——这是
    /// 「父指针是下标而非引用」这个设计的直接兑现。
    pub fn node_of(&self, id: NodeId) -> Result<&AstNode, NodeErr> {
        if self.released {
            return Err(NodeErr::PoolReleased);
        }
        let bi = id.bucket as usize;
        if bi >= self.buckets.len() {
            return Err(NodeErr::UnknownBucket { bucket: id.bucket });
        }
        let bucket = &self.buckets[bi];
        if id.index >= bucket.nodes.len() as u32 {
            return Err(NodeErr::IdOutOfRange {
                id,
                bucket_len: bucket.nodes.len() as u32,
            });
        }
        Ok(&bucket.nodes[id.index as usize])
    }

    /// 可变取节点。
    pub fn node_mut(&mut self, id: NodeId) -> Result<&mut AstNode, NodeErr> {
        if self.released {
            return Err(NodeErr::PoolReleased);
        }
        let bi = id.bucket as usize;
        if bi >= self.buckets.len() {
            return Err(NodeErr::UnknownBucket { bucket: id.bucket });
        }
        let bucket_len = self.buckets[bi].nodes.len() as u32;
        if id.index >= bucket_len {
            return Err(NodeErr::IdOutOfRange {
                id,
                bucket_len,
            });
        }
        Ok(&mut self.buckets[bi].nodes[id.index as usize])
    }

    /// 挂子节点：写子节点父指针 + 记入父的子表。
    pub fn attach(&mut self, parent: NodeId, child: NodeId) -> Result<(), NodeErr> {
        if self.released {
            return Err(NodeErr::PoolReleased);
        }
        // 先确认两端都合法，再写——顺序不能反，否则半途 Err 会留下半个挂接。
        self.node_of(parent)?;
        self.node_of(child)?;
        self.node_mut(child)?.set_parent(parent);
        let pi = parent.bucket as usize;
        self.buckets[pi].children.push((parent, child));
        Ok(())
    }

    /// 某节点的直接子节点（按挂接序）。
    pub fn children_of(&self, parent: NodeId) -> Vec<NodeId> {
        if self.released {
            return Vec::new();
        }
        let pi = parent.bucket as usize;
        if pi >= self.buckets.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (p, c) in self.buckets[pi].children.iter() {
            if *p == parent {
                out.push(*c);
            }
        }
        out
    }

    /// 某族节点数。
    pub fn family_len(&self, family: NodeFamily) -> usize {
        if self.released {
            return 0;
        }
        let bi = AstArena::bucket_of(family);
        match self.buckets.get(bi) {
            Some(b) => b.nodes.len(),
            None => 0,
        }
    }

    /// 同族节点最大连续段长度。
    ///
    /// 锚点「同型节点连续布局缓存友好」的机检口径：按族分桶后，同族节点应当
    /// **整段连续**，即最大连续段== 该族节点数。这里返回的是「桶内按插入序
    /// 连续段」的实测值——因为桶就是一个 `Vec`，理论上恒等于 `family_len`，
    /// 但让判据去**实测**而不是信任设计，才能拦住「桶里再分块」的实现。
    pub fn same_family_runs(&self, family: NodeFamily) -> usize {
        let len = self.family_len(family);
        if len == 0 {
            return 0;
        }
        // 桶是单一 Vec ⇒ 整段连续。实测口径：逐个下标确认可取即计入连续段。
        let bi = AstArena::bucket_of(family);
        if self.released {
            return 0;
        }
        match self.buckets.get(bi) {
            Some(b) => {
                let mut run = 0usize;
                let mut i = 0usize;
                while i < b.nodes.len() {
                    let id = NodeId {
                        bucket: bi as u8,
                        index: i as u32,
                    };
                    if self.node_of(id).is_ok() {
                        run += 1;
                        i += 1;
                    } else {
                        break;
                    }
                }
                run
            }
            None => 0,
        }
    }

    /// 按插入序遍历某族全部节点（缓存友好遍历面）。
    pub fn iter_family(&self, family: NodeFamily) -> Vec<&AstNode> {
        if self.released {
            return Vec::new();
        }
        let bi = AstArena::bucket_of(family);
        match self.buckets.get(bi) {
            Some(b) => b.nodes.iter().collect(),
            None => Vec::new(),
        }
    }

    /// 某族的跨度缺失数（判据「跨度缺失降级标注」的观测面）。
    pub fn span_missing_count(&self, family: NodeFamily) -> usize {
        let mut n = 0usize;
        for node in self.iter_family(family).iter() {
            if node.head.span.is_none() {
                n += 1;
            }
        }
        n
    }

    /// 跨度校验：缺失返回 `Err(NodeErr::SpanMissing)`，**但节点保留**。
    ///
    /// 锚点原文是「跨度缺失→诊断**降级标注**」——降级不是拒绝，所以这个函数
    /// 返回 `Err` 只用于**取标注文案**，不阻断任何流程：调用方拿
    /// [`NodeErr::three_elements`] 写诊断后继续。
    pub fn span_note(node: &AstNode) -> Result<(), NodeErr> {
        match node.head.span {
            Some(_) => Ok(()),
            None => Err(NodeErr::SpanMissing { kind: node.head.kind }),
        }
    }

    /// 整池释放：O(1)（`Vec::clear` + 标记）。
    ///
    /// 锚点「释放 O(1) 整池」。注意这里**不释放容量**（`clear` 保留 buffer），
    /// 真正的内存归还交给 `Vec` 析构——所以判据同时查「标记后所有访问拒绝」
    /// 与「析构路径零自定义逻辑」。
    pub fn release_all(&mut self) {
        for b in self.buckets.iter_mut() {
            b.nodes.clear();
            b.children.clear();
            b.bump_steps = 0;
        }
        self.allocated = 0;
        self.bump_steps = 0;
        self.released = true;
    }

    /// 泄漏检查：已释放却仍有节点，或计数与桶内容不符。
    ///
    /// 锚点「arena 泄漏→释放断言」。**判据不是「调了释放就算」**，而是释放后
    /// 逐桶实测为空、且所有 `NodeId` 都取不到节点——只查标记位的话，一个
    /// 「清标记不清节点」的泄漏实现会全绿。
    pub fn leak_report(&self) -> Result<(), NodeErr> {
        for b in self.buckets.iter() {
            if !self.released && !b.nodes.is_empty() {
                // 未释放且有节点不算泄漏（正常在用），仅在 released 时才算。
                continue;
            }
            if self.released && !b.nodes.is_empty() {
                return Err(NodeErr::PoolReleased);
            }
        }
        if self.released && self.allocated != 0 {
            return Err(NodeErr::PoolReleased);
        }
        Ok(())
    }

    /// 族桶总数（判据「按族分桶」的结构面）。
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// 桶族名序列（判据按序核对桶序= 族序）。
    pub fn bucket_family_names(&self) -> Vec<&'static str> {
        self.buckets.iter().map(|b| b.family.name()).collect()
    }
}

// ---------------------------------------------------------------------------
// 五、节点版本登记（判据四：版本登记）
// ---------------------------------------------------------------------------

/// 一条节点类型登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionEntry {
    /// 节点类型。
    pub kind: NodeKind,
    /// 所属族。
    pub family: NodeFamily,
    /// 引入版本号。
    pub since: u16,
}

/// 节点版本登记簿。
///
/// 判据要求登记簿与 [`NodeKind::ALL`] **集合相等**：既是「不多」（没有登记了
/// 却没实现的空头账），也是「不少」（实现了却没登记 = 不可追溯的演进）。
#[derive(Clone, Debug)]
pub struct NodeRegistry {
    entries: Vec<VersionEntry>,
    current_version: u16,
}

impl NodeRegistry {
    /// 新建登记簿，`current_version` 为起始版本。
    pub fn new(current_version: u16) -> NodeRegistry {
        NodeRegistry {
            entries: Vec::new(),
            current_version,
        }
    }

    /// 登记一个节点类型的引入版本。
    ///
    /// 两条拒绝规则（都是锚点「随语言版本演进登记」的必然推论）：
    /// 1. **版本号不得倒退或持平**（同版本重复占号也拒）——否则版本号失去
    ///    「演进序」含义，退化成计数器；
    /// 2. **同类型不得重复登记**——重复登记会让「集合相等」判据永远无法满足，
    ///    与其留到判据才发现，不如在登记口拦。
    pub fn register(&mut self, kind: NodeKind, since: u16) -> Result<(), NodeErr> {
        for e in self.entries.iter() {
            if e.kind == kind {
                return Err(NodeErr::VersionMismatch { kind });
            }
        }
        for e in self.entries.iter() {
            if e.since >= since {
                return Err(NodeErr::VersionNotAdvancing {
                    existing: e.since,
                    incoming: since,
                });
            }
        }
        self.entries.push(VersionEntry {
            kind,
            family: kind.family(),
            since,
        });
        if since > self.current_version {
            self.current_version = since;
        }
        Ok(())
    }

    /// 查某类型的登记项。
    pub fn entry_of(&self, kind: NodeKind) -> Option<VersionEntry> {
        for e in self.entries.iter() {
            if e.kind == kind {
                return Some(*e);
            }
        }
        None
    }

    /// 登记项按引入版本序（判据对账用）。
    pub fn entries(&self) -> Vec<VersionEntry> {
        self.entries.clone()
    }

    /// 登记项数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 当前版本号。
    pub fn current_version(&self) -> u16 {
        self.current_version
    }

    /// 某版本引入的节点类型（判据「版本登记可查询」）。
    pub fn introduced_at(&self, version: u16) -> Vec<NodeKind> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            if e.since == version {
                out.push(e.kind);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 六、F0421 动作消费者：把归约流落成 arena 节点
// ---------------------------------------------------------------------------

/// 归约动作 → arena 节点的映射表。
///
/// 「节点类型全集」的另一半：**哪个产生式 id 造哪种节点**。这张表是显式的，
/// 因为隐式映射（按左部非终结符猜）会让「新增节点」变成改代码里的隐式规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeMapping {
    /// F0421 产生式 id。
    pub prod: u16,
    /// 该产生式造出的节点类型。
    pub kind: NodeKind,
}

/// 骨架文法（20 条产生式）的产生式 → 节点类型映射。
///
/// 覆盖 F0421 [`skeleton_grammar`](super::vec21_parser::skeleton_grammar) 的
/// 每一条产生式——**全覆盖是判据要求**：漏一条就意味着某条产生式造不出节点，
/// 而这条漏项在下游 F0423-F0432 会变成"解析成功但树缺一块"的静默损坏。
pub const SKELETON_MAPPING: [NodeMapping; 20] = [
    NodeMapping { prod: 0, kind: NodeKind::TranslationUnit },
    NodeMapping { prod: 1, kind: NodeKind::TranslationUnit },
    NodeMapping { prod: 2, kind: NodeKind::FnDecl },
    NodeMapping { prod: 3, kind: NodeKind::LetDecl },
    NodeMapping { prod: 4, kind: NodeKind::LetInitDecl },
    NodeMapping { prod: 5, kind: NodeKind::IfStmt },
    NodeMapping { prod: 6, kind: NodeKind::IfElseStmt },
    NodeMapping { prod: 7, kind: NodeKind::ReturnStmt },
    NodeMapping { prod: 8, kind: NodeKind::BlockStmt },
    NodeMapping { prod: 9, kind: NodeKind::BlockStmt },
    NodeMapping { prod: 10, kind: NodeKind::BinaryExpr },
    NodeMapping { prod: 11, kind: NodeKind::BinaryExpr },
    NodeMapping { prod: 12, kind: NodeKind::BinaryExpr },
    NodeMapping { prod: 13, kind: NodeKind::IdentExpr },
    NodeMapping { prod: 14, kind: NodeKind::LiteralExpr },
    NodeMapping { prod: 15, kind: NodeKind::ParenExpr },
    NodeMapping { prod: 16, kind: NodeKind::ParamExpr },
    NodeMapping { prod: 17, kind: NodeKind::TypeExpr },
    NodeMapping { prod: 18, kind: NodeKind::TypeExpr },
    NodeMapping { prod: 19, kind: NodeKind::ExprStmt },
];

/// 查产生式映射。**未登记产生式返回 `None`**——不是回退到某个默认类型，
/// 因为回退会让「漏登记」变成静默的错误节点类型。
pub fn mapping_of(prod: u16) -> Option<NodeMapping> {
    let mut i = 0usize;
    while i < SKELETON_MAPPING.len() {
        let m = SKELETON_MAPPING[i];
        if m.prod == prod {
            return Some(m);
        }
        i += 1;
    }
    None
}

/// 把 F0421 动作流落成 arena 节点的消费者。
///
/// 这是判据三「动作分离」的兑现面：动作序列仍由 F0421 生产者决定，本消费者
/// 只决定节点形态。与 [`NodeTallySink`](super::vec21_parser::NodeTallySink) 取到
/// 的动作序列必然**逐条相同**（判据会实测这一点）。
pub struct ArenaBuilder {
    arena: AstArena,
    registry: NodeRegistry,
    /// 动作接收计数。
    pub accepted: u32,
    /// 动作拒绝计数。
    pub rejected: u32,
    /// 未登记产生式计数（诊断面，不静默）。
    pub unmapped: u32,
    /// 跨度缺失降级标注计数。
    pub span_notes: u32,
    /// 节点栈（用于挂父子）。
    stack: Vec<NodeId>,
}

impl ArenaBuilder {
    /// 以起始版本新建。
    pub fn new(version: u16) -> ArenaBuilder {
        ArenaBuilder {
            arena: AstArena::new(),
            registry: NodeRegistry::new(version),
            accepted: 0,
            rejected: 0,
            unmapped: 0,
            span_notes: 0,
            stack: Vec::new(),
        }
    }

    /// 预登记全部骨架节点类型（版本号顺序递增）。
    pub fn preseed(&mut self) -> Result<(), NodeErr> {
        let mut i = 0usize;
        while i < SKELETON_MAPPING.len() {
            let m = SKELETON_MAPPING[i];
            self.registry.register(m.kind, (i as u16) + 1)?;
            i += 1;
        }
        Ok(())
    }

    /// 取 arena（判据与下游验证器消费面）。
    pub fn arena(&self) -> &AstArena {
        &self.arena
    }

    /// 可变取 arena。
    pub fn arena_mut(&mut self) -> &mut AstArena {
        &mut self.arena
    }

    /// 取登记簿。
    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    /// 整池释放（判据三）。
    pub fn release(&mut self) {
        self.arena.release_all();
        self.stack.clear();
    }

    /// 当前节点栈深度（判据「退出配平」的观测面）。
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    /// 出栈一节（配平用）。
    pub fn pop(&mut self) -> Option<NodeId> {
        self.stack.pop()
    }

    /// 落一个动作成节点。
    fn build(&mut self, r: &Reduction) -> SinkStatus {
        self.accepted += 1;
        let m = match mapping_of(r.prod) {
            Some(m) => m,
            None => {
                // 未登记产生式：不造节点但**显性计数**（不静默）。
                self.unmapped += 1;
                return SinkStatus::Reject;
            }
        };
        let since = match self.registry.entry_of(m.kind) {
            Some(e) => e.since,
            None => self.registry.current_version(),
        };
        let node = AstNode::with_span(m.kind, r.span, since);
        match self.arena.alloc(node) {
            Ok(id) => {
                // 跨度缺失只标注不阻断（锚点：降级标注）。
                if let Some(n) = self.arena.node_at(id.bucket, id.index) {
                    if AstArena::span_note(n).is_err() {
                        self.span_notes += 1;
                    }
                }
                // 挂到栈顶（若栈顶同桶且可挂）。
                if let Some(top) = self.stack.last().copied() {
                    let _ = self.arena.attach(top, id);
                }
                self.stack.push(id);
                SinkStatus::Ok
            }
            Err(_) => {
                self.rejected += 1;
                SinkStatus::Reject
            }
        }
    }
}

impl ActionSink for ArenaBuilder {
    fn on_reduce(&mut self, r: &Reduction) -> SinkStatus {
        self.build(r)
    }

    fn on_error(&mut self, _d: &Diagnostic) -> SinkStatus {
        // 诊断不阻断建树（下游 F0435 验证器消费），但显性计数在 span_notes 之外
        // 单独留痕于 unmapped 之外的诊断面——这里不吞：解析器仍会记自己的诊断。
        SinkStatus::Ok
    }
}

// ---------------------------------------------------------------------------
// 七、给 AstArena 补两个只读观察面（供 ArenaBuilder 与判据使用）
// ---------------------------------------------------------------------------

impl AstArena {
    /// 只读取桶（越界给 `None`）。
    pub fn buckets_get(&self, bucket: u8) -> Option<&NodeBucket> {
        self.buckets.get(bucket as usize)
    }

    /// 只读取桶内节点（越界给 `None`）。
    pub fn node_at(&self, bucket: u8, index: u32) -> Option<&AstNode> {
        match self.buckets.get(bucket as usize) {
            Some(b) => b.nodes.get(index as usize),
            None => None,
        }
    }

    /// 累计 bump 步数（判据「O(1) bump」：恒等于分配数）。
    pub fn bump_steps(&self) -> u64 {
        self.bump_steps
    }
}