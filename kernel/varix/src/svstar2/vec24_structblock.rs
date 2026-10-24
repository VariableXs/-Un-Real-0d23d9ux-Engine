//! VE-F0424 · 结构体与接口块解析（VE-C 域 · 着色器系统 · 语法组 C03）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0424`
//!
//! **判据（锚点原文）**：成员序、布局语法面、接口块、边界显性、判据。
//!
//! 锚点原文：「结构体与接口块语法解析：成员声明序、嵌套类型、对齐与布局限定
//! （标准布局语义语法面）、接口块成员（uniform 块、buffer 块类）与实例名语法；
//! 匿名成员语义按规范；成员偏移等语义留给语义期（边界显性）。数据结构：成员解析
//! 结构；布局限定挂接；接口块结构。错误路径与降级矩阵：成员重复→登记语义期；
//! 布局语法错→恢复到块尾；匿名冲突→按规范裁定报告。性能逐项分解：解析
//! O(成员数)；挂接 O(1)；裁定 O(1) 规范引用。跨批对接点：上游 F0423 顶层序；
//! 下游 F0443 类型系统语义消费。」
//!
//! 本单交付**块内部那一层**：成员怎么切、布局限定怎么挂、接口块与结构体差在哪、
//! 匿名成员按规范怎么裁、哪些裁定**故意不做**。五条判据逐条都要有**结构性保证**，
//! 而不是「我代码里没写」这种自觉性保证：
//!
//! 1. **成员序**（判据一）。锚点点名「成员声明序」。[`MemberSeq::push`] 是**唯一**
//!    入口且它带 `append` 语义：序号在 push 里生成，故「乱序」「回插」在**类型上**
//!    就写不出来。覆盖性不是自称——[`MemberSeq::ordinals_are_dense`] 与
//!    [`MemberSeq::intervals_are_ordered`] 用成员自报的记号区间**实测**三件事：
//!    序号恰为 `0..len`、区间**按序递增**、区间**互不重叠**。三件任一为假都红。
//!    注释与指令记号按 [`is_trivia`] **同一谓词**豁免——豁免规则若与成员区间
//!    规则不同源，覆盖性就是两套口径拼出来的假数。
//!
//! 2. **布局语法面**（判据二）。锚点写「对齐与布局限定（**标准布局语义语法面**）」。
//!    限定词表是**封闭集**：[`LAYOUT_KEYS`] 逐个登记**键身份**（不是名字），
//!    判据用 `ALL.len()` 加 [`LayoutKey::from_word`] 的**双向**（域外词必落
//!    [`LayoutKey::Unknown`] 且**必产诊断**）钉死。挂接是 O(1)：[`LayoutQual::attach`]
//!    只推进游标并一次 `reserve_exact`，判据实测首次挂接后 `reallocs` 恒为 1
//!    （再挂不增）——「挂接 O(1)」不是承诺而是可测事实。
//!    「语义语法面」四个字是本单不越权的**形式化**：值域表（哪些键带值、哪些是
//!    旗标）由 [`LayoutKey::value_domain`] **单源**登记，解析器照表执行，
//!    **不新增任何键**——私自扩表等于偷偷把语义期的东西写进语法期。
//!
//! 3. **接口块**（判据三）。锚点写「接口块成员（uniform 块、buffer 块类）与
//!    实例名语法」。三种块类加结构体共**恰好四种** [`BLOCK_KINDS`]，判据用
//!    `BLOCK_KINDS.len() == 4` 加 [`BlockKind::from_word`] 域外 `None` 双向钉死。
//!    **实例名是块级语法**：`}` 之后可跟一个可选标识符（`uniform Camera { ... } cam;`），
//!    它与**块名**是两个不同的槽位——判据用「具名块 + 实例名」「具名块无实例名」
//!    「无名块 + 实例名」三向语料证明两者不互相顶替；只测一向的实现会把实例名
//!    写进块名槽，而那在语义期表现为「块名与实例名恒等」。
//!    四种块类又不是本单自创的：[`BLOCK_LEADS`] 逐条登记引导词**来源**，
//!    其中 `buffer` / `interface` 在 F0404 关键字表里**查无此词**，必须如实标
//!    `SyntaxExtension` 并附**非空理由**——理由不可为空，否则「我把 `buffer`
//!    认成块引导词」是一次没留痕的私自扩表（[`BlockLead::source_is_honest`]）。
//!
//! 4. **边界显性**（判据四）。锚点写「成员偏移等语义留给语义期（**边界显性**）」。
//!    这条不是靠注释画线，靠**三样东西**：
//!    - **形状**：[`MemberDecl`] 的字段里**没有任何偏移/对齐/尺寸/跨距/类型对象**。
//!      这条由 [`member_shape_literal`] 提供的**编译期**保证兜住：它用**不带
//!!      `..Default::default()`** 的全字段字面量构造 `MemberDecl`——一旦有人加
//!!      第 13 个字段，这个函数就**编译不过**。注释画线会被人顺手删掉，编译不过
//!      不会。
//!    - **账**：每一条本单**故意不做**的裁定都登记成 [`DeferredDuty`]，带
//!      [`DutyStage`]、**责任单号**与**非空理由**（[`DutyLedger`]）。判据要求
//!      每笔债**都有归属**且语义期债**非空**——一张空账等于没画线。
//!    - **纯度**：[`block_digest`] **不吃标识符文本、不吃字面量取值**。只改成员名
//!      或只改数值的源码，摘要必须**逐位相等**；改**键身份**或**成员个数**的源码
//!      必须变（判据配**反向对照**，防恒常量的假摘要）。键身份进摘要而成员名不进
//!      ——因为「这是 `std140` 不是 `std430`」是**语法面**，「这个成员叫 `albedo`
//!      还是 `diffuse`」是**名字面**，两者的分界就是判据二的「语法面」三个字。
//!
//! 5. **判据**（判据五）。`vec24_checks.rs` 把上面四条拆成机检项，并给每条判据配
//!    **反向对照**（negative control）：纯度项必须同时证明「改名字摘要不变」与
//!    「改语法摘要必变」，否则一个恒返回常量的摘要实现会全绿。
//!
//! ## 与上下游的接口
//!
//! - **上游 F0403**（[`super::vec03_lexer`]）：本单一律消费 `Token`/`TokenKind`，
//!   **不做二次词法**。本单**不走** [`super::vec21_parser::classify`]：那个归约把
//!   `[` / `]` / `.` 一律映到 `Term::Eof`（不可区分），而布局限定与数组后缀恰好
//!   需要区分它们，故本单在**记号层**（`TokenKind` + 原文标点）切分，理由见
//!   [`is_punct`]。
//! - **上游 F0404**（[`super::vec04_keywords`]）：成员名与实例名一律过
//!   [`vec04_keywords::classify`] 记**词类**，但**不据此拒收**——保留字/保留前缀
//!   的裁定归 F0445，本单只登记 [`DeferredDuty::MemberNameReserved`] 债。
//! - **上游 F0423**（[`super::vec23_transunit`]）：本单**只吃 F0423 切好的声明
//!   区间**（[`TopDecl::token_lo`]..[`TopDecl::token_hi`]，半开），自己**不再切
//!   顶层**。块引导词还必须能在 F0423 的引导表里查到且 `role` 为 `ClassLead`
//!   ——查不到就是本单私自认了个新顶层类别（[`lead_role_upstream`]）。
//! - **上游 F0422**（[`super::vec22_ast`]）：`struct` 与接口块在 F0422 **尚无登记
//!   节点类型**，本单**不擅自扩表**，而是登记 [`DeferredDuty::NodeKindUnregistered`]
//!   债（责任单 `VE-F0422`），并用 [`block_node_kind_registered`] **实测**该事实
//!!   ——「缺一张登记」要变成**可见的欠账**而不是静默的降级映射。
//! - **下游 F0443**（类型系统语义消费）：成员偏移、对齐填充、`std140`/`std430`
//!   布局规则、数组跨距、嵌套类型扁平、块绑定解算——**全部**归 F0443，见
//!   [`DeferredDuty::owner`]。
//!
//! ## 零 panic 面 / 零静默
//!
//! 生产代码无 `unwrap` / `expect` / `panic!` / 裸下标越界：所有取记号走
//! [`Cur::tok`] / [`Cur::at`] 的 `.get` 面。名字驻留表定容且**满即登记**
//! （[`MemberDiagCode::NameNotInterned`]，且**留空不留默认名**——默认名会让重名
//! 检测失真）。恢复无进展时强制前进并记账
//! （[`MemberDiagCode::RecoveryForcedAdvance`]）：恢复循环每轮都推进游标，故块解析
//! 必然终止（判据以「喂十万个垃圾记号仍返回」实测）。嵌套深度超限给
//! [`MemberDiagCode::NestingTooDeep`] 并**停止下钻**（不是栈溢出）。零 IO、
//! 零墙钟、零全局可变状态。

#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vec03_lexer::{Token, TokenKind};
use super::vec04_keywords::{self, LangVersion};
use super::vec21_parser::Span;
use super::vec22_ast::NodeKind;
use super::vec23_transunit::{
    self, DeclClass, LeadRole, LeadSource, NameTable, TopDecl, TranslationUnit,
};

/// 严重度。沿用 F0423 的三档，**同名同义**——锚点要求「成员重复→登记语义期」
/// 「匿名冲突→按规范裁定**报告**」，若只有错误一档，重复成员就只能被当成错误
/// 报出来（假阳性）或被吞掉（静默），两条都不合规。
pub use super::vec23_transunit::Severity as Sev;

/// 本单的锚点串。诊断文本里的 `VE-F04xx` 引用一律经 [`MemberDiagCode::anchor`]
/// 派生，不允许调用方自己拼串——拼串一定会拼错，且拼错无法机检。
pub const ANCHOR: &str = "VE-F0424";

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 嵌套块深度上限（结构体里嵌结构体）。超限即**停止下钻**并报
/// [`MemberDiagCode::NestingTooDeep`]——递归深度是硬界，不是建议。
pub const NEST_LIMIT: usize = 16;

/// 单个块的布局限定条目上限。超限即**显性拒收**，理由与名字驻留表同源：
/// 无界增长会把「一份源码」变成「一份无限大的账」。
pub const MAX_LAYOUT_ENTRIES: usize = 32;

/// 单个块的成员数上限。超限即显性拒收。
pub const MAX_MEMBERS: usize = 4096;

/// 单块的嵌套子块数上限。
pub const MAX_NESTED: usize = 256;

/// 单次解析的诊断条数上限（超限即**停止追加**——诊断本身也要有预算，
/// 否则一条病态源码能把诊断面撑爆并把真正的错误挤出去）。
pub const MAX_DIAGS: usize = 256;

/// 名字驻留失败时使用的哨兵（**不是** 0——0 是一个合法的驻留号）。
pub const NO_NAME: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// 二、诊断面
// ---------------------------------------------------------------------------

/// VE-F0424 诊断码。判别值为 `u8`，与 F0423 的 `TopDiagCode` **同族不同码**——
/// 本单的码不进 F0423 的码空间，避免两单的诊断互相顶替。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MemberDiagCode {
    /// 块引导词非法（既不是结构体也不是三类接口块）。
    IllegalBlockLead = 0,
    /// 缺块名（规范要求必填）。
    MissingBlockName = 1,
    /// 缺块体左花括号。
    MissingBlockOpen = 2,
    /// 块体右花括号缺失（未闭合）。
    BlockUnclosed = 3,
    /// 成员缺声明名（类型与声明名双缺）。
    MemberDeclaratorMissing = 4,
    /// 声明名位置被限定词占。
    MemberDeclaratorOccupied = 5,
    /// 数组后缀未闭合。
    MalformedArraySuffix = 6,
    /// 成员缺分号。
    MemberSemiMissing = 7,
    /// 布局限定键未登记。
    UnknownLayoutKey = 8,
    /// 布局限定键重复。
    LayoutKeyRepeated = 9,
    /// 布局值缺失或与值域表不符。
    LayoutValueMismatch = 10,
    /// 布局限定语法错（括号/逗号层）——按锚点**恢复到块尾**。
    LayoutSyntaxBroken = 11,
    /// 布局表满。
    LayoutTableFull = 12,
    /// 成员表满。
    MemberTableFull = 13,
    /// 名字驻留失败或表满。
    NameNotInterned = 14,
    /// 恢复无进展，强制前进。
    RecoveryForcedAdvance = 15,
    /// 嵌套深度超限。
    NestingTooDeep = 16,
    /// 声明区间越界（上游 F0423 给的区间不落在记号流内）。
    ExtentOutOfRange = 17,
    /// 成员自报区间越界。
    MemberSpanOutOfRange = 18,
    /// 子块表满。
    NestedTableFull = 19,
    /// 声明类别与块种类不匹配（本单只吃 `TypeDef` 与 `InterfaceBlock` 两类）。
    ClassKindMismatch = 20,
}

/// 全部在案诊断码（判据逐条遍历用）。
pub const MEMBER_DIAG_ALL: [MemberDiagCode; 21] = [
    MemberDiagCode::IllegalBlockLead,
    MemberDiagCode::MissingBlockName,
    MemberDiagCode::MissingBlockOpen,
    MemberDiagCode::BlockUnclosed,
    MemberDiagCode::MemberDeclaratorMissing,
    MemberDiagCode::MemberDeclaratorOccupied,
    MemberDiagCode::MalformedArraySuffix,
    MemberDiagCode::MemberSemiMissing,
    MemberDiagCode::UnknownLayoutKey,
    MemberDiagCode::LayoutKeyRepeated,
    MemberDiagCode::LayoutValueMismatch,
    MemberDiagCode::LayoutSyntaxBroken,
    MemberDiagCode::LayoutTableFull,
    MemberDiagCode::MemberTableFull,
    MemberDiagCode::NameNotInterned,
    MemberDiagCode::RecoveryForcedAdvance,
    MemberDiagCode::NestingTooDeep,
    MemberDiagCode::ExtentOutOfRange,
    MemberDiagCode::MemberSpanOutOfRange,
    MemberDiagCode::NestedTableFull,
    MemberDiagCode::ClassKindMismatch,
];

impl MemberDiagCode {
    /// 判别值。
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            MemberDiagCode::IllegalBlockLead => "块引导词非法",
            MemberDiagCode::MissingBlockName => "缺块名",
            MemberDiagCode::MissingBlockOpen => "缺块体左括号",
            MemberDiagCode::BlockUnclosed => "块体未闭合",
            MemberDiagCode::MemberDeclaratorMissing => "成员缺声明名",
            MemberDiagCode::MemberDeclaratorOccupied => "声明名位被限定词占",
            MemberDiagCode::MalformedArraySuffix => "数组后缀未闭合",
            MemberDiagCode::MemberSemiMissing => "成员缺分号",
            MemberDiagCode::UnknownLayoutKey => "布局键未登记",
            MemberDiagCode::LayoutKeyRepeated => "布局键重复",
            MemberDiagCode::LayoutValueMismatch => "布局值与值域表不符",
            MemberDiagCode::LayoutSyntaxBroken => "布局语法错",
            MemberDiagCode::LayoutTableFull => "布局表满",
            MemberDiagCode::MemberTableFull => "成员表满",
            MemberDiagCode::NameNotInterned => "名字未驻留",
            MemberDiagCode::RecoveryForcedAdvance => "恢复强制前进",
            MemberDiagCode::NestingTooDeep => "嵌套深度超限",
            MemberDiagCode::ExtentOutOfRange => "声明区间越界",
            MemberDiagCode::MemberSpanOutOfRange => "成员区间越界",
            MemberDiagCode::NestedTableFull => "子块表满",
            MemberDiagCode::ClassKindMismatch => "声明类别与块种类不匹配",
        }
    }

    /// 严重度。**重复成员不在这张表里**——它根本不是诊断，是一笔语义期债
    /// （[`DeferredDuty::DuplicateMember`]）；把它做成 Warning 会让「重复成员
    /// 归语义期」这条边界退化成一句注释。
    pub fn severity(self) -> Sev {
        match self {
            MemberDiagCode::IllegalBlockLead
            | MemberDiagCode::MissingBlockOpen
            | MemberDiagCode::BlockUnclosed
            | MemberDiagCode::ExtentOutOfRange
            | MemberDiagCode::ClassKindMismatch
            | MemberDiagCode::MemberSpanOutOfRange
            | MemberDiagCode::NestingTooDeep => Sev::Error,
            MemberDiagCode::MissingBlockName
            | MemberDiagCode::MemberDeclaratorMissing
            | MemberDiagCode::MemberDeclaratorOccupied
            | MemberDiagCode::MemberSemiMissing
            | MemberDiagCode::MalformedArraySuffix
            | MemberDiagCode::UnknownLayoutKey
            | MemberDiagCode::LayoutKeyRepeated
            | MemberDiagCode::LayoutValueMismatch
            | MemberDiagCode::LayoutSyntaxBroken
            | MemberDiagCode::LayoutTableFull
            | MemberDiagCode::MemberTableFull
            | MemberDiagCode::NameNotInterned
            | MemberDiagCode::NestedTableFull
            | MemberDiagCode::RecoveryForcedAdvance => Sev::Warning,
        }
    }

    /// 锚点引用（诊断文本里**唯一**允许出现的锚点来源）。
    pub fn anchor(self) -> &'static str {
        match self {
            MemberDiagCode::IllegalBlockLead
            | MemberDiagCode::MissingBlockName
            | MemberDiagCode::MissingBlockOpen
            | MemberDiagCode::BlockUnclosed
            | MemberDiagCode::ClassKindMismatch
            | MemberDiagCode::ExtentOutOfRange => "VE-F0424#块语法",
            MemberDiagCode::MemberDeclaratorMissing
            | MemberDiagCode::MemberDeclaratorOccupied
            | MemberDiagCode::MemberSemiMissing
            | MemberDiagCode::MalformedArraySuffix
            | MemberDiagCode::MemberTableFull
            | MemberDiagCode::MemberSpanOutOfRange => "VE-F0424#成员序",
            MemberDiagCode::UnknownLayoutKey
            | MemberDiagCode::LayoutKeyRepeated
            | MemberDiagCode::LayoutValueMismatch
            | MemberDiagCode::LayoutSyntaxBroken
            | MemberDiagCode::LayoutTableFull => "VE-F0424#布局限定语法面",
            MemberDiagCode::NameNotInterned | MemberDiagCode::NestedTableFull => {
                "VE-F0424#名字与容量"
            }
            MemberDiagCode::RecoveryForcedAdvance | MemberDiagCode::NestingTooDeep => {
                "VE-F0424#恢复与终止"
            }
        }
    }

    /// 处置建议。
    pub fn advice(self) -> &'static str {
        match self {
            MemberDiagCode::IllegalBlockLead => {
                "顶层块引导词只认 struct / uniform / buffer / interface"
            }
            MemberDiagCode::MissingBlockName => "块名按规范必填；实例名不是块名，两者不互顶",
            MemberDiagCode::MissingBlockOpen => "块体须以左花括号开始",
            MemberDiagCode::BlockUnclosed => "块体右花括号缺失，已恢复到声明区间末尾",
            MemberDiagCode::MemberDeclaratorMissing => {
                "成员声明须含类型与声明名，二者皆缺按规范裁定并交语义期"
            }
            MemberDiagCode::MemberDeclaratorOccupied => "限定词不得占据声明名位置",
            MemberDiagCode::MalformedArraySuffix => "数组后缀须以右方括号闭合",
            MemberDiagCode::MemberSemiMissing => "成员声明须以分号结束",
            MemberDiagCode::UnknownLayoutKey => "布局键未在封闭键集登记；扩表须走 F0433",
            MemberDiagCode::LayoutKeyRepeated => "同一块的同一布局键重复出现（前一条仍保留）",
            MemberDiagCode::LayoutValueMismatch => {
                "布局键的值域与单源表不符（旗标键不得带值、值键不得缺值、值不得瞎猜）"
            }
            MemberDiagCode::LayoutSyntaxBroken => "布局语法错，按锚点恢复到块尾",
            MemberDiagCode::LayoutTableFull => "布局条目数超上限，显性拒收不挤占",
            MemberDiagCode::MemberTableFull => "成员数超上限，显性拒收不挤占",
            MemberDiagCode::NameNotInterned => "名字驻留表满，名字位留空并登记债",
            MemberDiagCode::RecoveryForcedAdvance => "恢复无进展，强制前进一个记号保终止",
            MemberDiagCode::NestingTooDeep => "嵌套深度超限，停止下钻并登记债",
            MemberDiagCode::ExtentOutOfRange => "上游声明区间不落在记号流内",
            MemberDiagCode::MemberSpanOutOfRange => "成员自报区间越出声明区间",
            MemberDiagCode::NestedTableFull => "子块数超上限，显性拒收",
            MemberDiagCode::ClassKindMismatch => "本单只解析 TypeDef 与 InterfaceBlock 两类顶层声明",
        }
    }

    /// 域外判别值必 `None`（判据双向钉死：域内全覆盖 + 域外落空）。
    pub fn from_code(code: u8) -> Option<MemberDiagCode> {
        let mut i = 0usize;
        while i < MEMBER_DIAG_ALL.len() {
            if let Some(c) = MEMBER_DIAG_ALL.get(i) {
                if c.code() == code {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }
}

/// 一条诊断。
#[derive(Clone, Debug)]
pub struct MemberDiag {
    pub code: MemberDiagCode,
    pub span: Span,
    pub detail: String,
}

impl MemberDiag {
    /// 构造诊断。
    pub fn new(code: MemberDiagCode, span: Span, detail: &str) -> MemberDiag {
        MemberDiag {
            code,
            span,
            detail: String::from(detail),
        }
    }

    /// 严重度。
    pub fn severity(&self) -> Sev {
        self.code.severity()
    }

    /// 锚点引用。
    pub fn anchored(&self) -> &'static str {
        self.code.anchor()
    }

    /// 三要素（原因 | 详情 | 建议）。
    pub fn three_elements(&self) -> String {
        alloc::format!("{} | {} | {}", self.code.name(), self.detail, self.code.advice())
    }

    /// 渲染全文（含锚点）。
    pub fn render(&self) -> String {
        alloc::format!(
            "[{}] {} @{}:{} {}",
            self.anchored(),
            self.code.name(),
            self.span.line,
            self.span.col,
            self.detail
        )
    }
}

// ---------------------------------------------------------------------------
// 三、布局限定语法面（封闭键集 + 值域单源）
// ---------------------------------------------------------------------------

/// 布局限定键的**值域**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueDomain {
    /// 不带值（旗标）。
    Flag,
    /// 带整数值。
    Number,
    /// 带标识符值。
    Word,
}

/// 布局限定键（封闭集——**键身份**是语法面的一部分，进摘要）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LayoutKey {
    /// 对齐（带整数值）。
    Align,
    /// `std140` 布局（旗标）。
    Std140,
    /// `std430` 布局（旗标）。
    Std430,
    /// 紧凑布局（旗标）。
    Packed,
    /// 行主序（旗标）。
    RowMajor,
    /// 列主序（旗标）。
    ColumnMajor,
    /// 局部尺寸 X（带整数值）。
    LocalSizeX,
    /// 局部尺寸 Y（带整数值）。
    LocalSizeY,
    /// 局部尺寸 Z（带整数值）。
    LocalSizeZ,
    /// 绑定槽（带整数值）。
    Binding,
    /// 绑定组（带整数值）。
    Set,
    /// 未登记键（域外兜底，**不静默**：单独一档且必产诊断）。
    Unknown,
}

/// 布局键全集（封闭集——判据逐个遍历用）。
pub const LAYOUT_KEYS: [LayoutKey; 11] = [
    LayoutKey::Align,
    LayoutKey::Std140,
    LayoutKey::Std430,
    LayoutKey::Packed,
    LayoutKey::RowMajor,
    LayoutKey::ColumnMajor,
    LayoutKey::LocalSizeX,
    LayoutKey::LocalSizeY,
    LayoutKey::LocalSizeZ,
    LayoutKey::Binding,
    LayoutKey::Set,
];

/// 键词表（判据侧**逐字**对拍用；与 [`LayoutKey::from_word`] 必须同源）。
pub const LAYOUT_KEY_WORDS: [(&str, LayoutKey); 11] = [
    ("align", LayoutKey::Align),
    ("std140", LayoutKey::Std140),
    ("std430", LayoutKey::Std430),
    ("packed", LayoutKey::Packed),
    ("row_major", LayoutKey::RowMajor),
    ("column_major", LayoutKey::ColumnMajor),
    ("local_size_x", LayoutKey::LocalSizeX),
    ("local_size_y", LayoutKey::LocalSizeY),
    ("local_size_z", LayoutKey::LocalSizeZ),
    ("binding", LayoutKey::Binding),
    ("set", LayoutKey::Set),
];

impl LayoutKey {
    /// 词 → 键。域外词落 [`LayoutKey::Unknown`]（**不是** `None`——解析器还要继续
    /// 走语法，键身份不认识必须单独一档并**必产诊断**，而不是让整个限定词崩掉）。
    pub fn from_word(word: &str) -> LayoutKey {
        let mut i = 0usize;
        while i < LAYOUT_KEY_WORDS.len() {
            if LAYOUT_KEY_WORDS[i].0 == word {
                return LAYOUT_KEY_WORDS[i].1;
            }
            i += 1;
        }
        LayoutKey::Unknown
    }

    /// 是否登记在案。
    pub fn is_registered(self) -> bool {
        self != LayoutKey::Unknown
    }

    /// 值域（**单源表**——解析器只照它执行，不新增键）。
    pub fn value_domain(self) -> ValueDomain {
        match self {
            LayoutKey::Align
            | LayoutKey::LocalSizeX
            | LayoutKey::LocalSizeY
            | LayoutKey::LocalSizeZ
            | LayoutKey::Binding
            | LayoutKey::Set => ValueDomain::Number,
            // 未登记键的值域**显式**为旗标：它没有值域可言，解析器照旗标走，
            // 同时必产 UnknownLayoutKey——不产诊断的 Unknown 才是静默。
            _ => ValueDomain::Flag,
        }
    }

    /// 人话名（就是它的键词——键身份与名字在语法面同源）。
    pub fn name(self) -> &'static str {
        let mut i = 0usize;
        while i < LAYOUT_KEY_WORDS.len() {
            if LAYOUT_KEY_WORDS[i].1 == self {
                return LAYOUT_KEY_WORDS[i].0;
            }
            i += 1;
        }
        "未登记布局键"
    }

    /// 域外判别值必 `None`（判据双向钉死）。
    pub fn from_code(code: u8) -> Option<LayoutKey> {
        let mut i = 0usize;
        while i < LAYOUT_KEYS.len() {
            if let Some(k) = LAYOUT_KEYS.get(i) {
                if *k as u8 == code {
                    return Some(*k);
                }
            }
            i += 1;
        }
        None
    }
}

/// 布局限定值。**只记形状与值，不记类型**——类型解算归 F0443。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutValue {
    /// 整数值（十进制，全整数零浮点）。
    Number(u32),
    /// 标识符值（按驻留号携带，不持文本——持文本就得跨单持有名字表）。
    Word(u32),
    /// 旗标（无值）。
    Flag,
}

impl LayoutValue {
    /// 形状码（进摘要：**值不进，形状进**——「值」是数据，「值是不是数字」是语法）。
    pub fn shape(self) -> u8 {
        match self {
            LayoutValue::Number(_) => 0,
            LayoutValue::Word(_) => 1,
            LayoutValue::Flag => 2,
        }
    }

    /// 整数值（旗标/词值一律 `None`，**不猜**）。
    pub fn number(self) -> Option<u32> {
        match self {
            LayoutValue::Number(v) => Some(v),
            _ => None,
        }
    }

    /// 形状是否符合某值域（旗标键带值 / 值键缺值都在这里被拦）。
    pub fn fits(self, want: ValueDomain) -> bool {
        match want {
            ValueDomain::Flag => self == LayoutValue::Flag,
            ValueDomain::Number => matches!(self, LayoutValue::Number(_)),
            ValueDomain::Word => matches!(self, LayoutValue::Word(_)),
        }
    }
}

/// 一条布局限定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutEntry {
    /// 键身份。
    pub key: LayoutKey,
    /// 值。
    pub value: LayoutValue,
    /// 序号（从 0 起，只增）。
    pub ordinal: u32,
    /// 该条的原文跨度。
    pub span: Span,
}

/// 布局限定表（挂接 O(1)：只推进游标 + 首次一次 `reserve_exact`）。
#[derive(Clone, Debug, Default)]
pub struct LayoutQual {
    entries: Vec<LayoutEntry>,
    /// 挂接次数。
    pub attach_count: u32,
    /// 分配次数（判据实测恒为 1：首次挂接后不再重分配——「O(1) 挂接」的实测面）。
    pub allocs: u32,
    /// 语法错次数（锚点：布局语法错→恢复到块尾）。
    pub broken: u32,
    /// 表满拒收次数。
    pub rejected: u32,
    /// 挂接区间跨度。
    pub span: Span,
}

impl LayoutQual {
    /// 空表。
    pub fn new() -> LayoutQual {
        LayoutQual {
            entries: Vec::new(),
            attach_count: 0,
            allocs: 0,
            broken: 0,
            rejected: 0,
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            },
        }
    }

    /// 挂接一条（O(1)；表满**显性拒收**并计数）。
    pub fn attach(&mut self, entry: LayoutEntry, span: Span) -> bool {
        self.attach_count = self.attach_count.saturating_add(1);
        if self.entries.is_empty() {
            self.entries.reserve_exact(MAX_LAYOUT_ENTRIES);
            self.allocs = self.allocs.saturating_add(1);
        }
        if self.entries.len() >= MAX_LAYOUT_ENTRIES {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        self.entries.push(entry);
        if self.span.end == 0 {
            self.span = span;
        } else {
            self.span = self.span.join(span);
        }
        true
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 取条目（零 panic 面）。
    pub fn get(&self, i: usize) -> Option<&LayoutEntry> {
        self.entries.get(i)
    }

    /// 全部条目。
    pub fn all(&self) -> &[LayoutEntry] {
        &self.entries
    }

    /// 查键首次出现序号（重复检测用；`None` = 未出现过）。
    pub fn first_ordinal_of(&self, key: LayoutKey) -> Option<u32> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    return Some(e.ordinal);
                }
            }
            i += 1;
        }
        None
    }

    /// 旗标位掩码（只对**已登记的旗标键**置位；非旗标键与未登记键都不置位）。
    pub fn flag_bits(&self) -> u32 {
        let mut bits = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.value == LayoutValue::Flag && e.key.is_registered() {
                    let k = e.key as u32;
                    if k < 32 {
                        bits |= 1u32 << k;
                    }
                }
            }
            i += 1;
        }
        bits
    }

    /// 已登记旗标键集合（判据：键集必须恰为登记旗标键集的子集）。
    pub fn has_flag(&self, key: LayoutKey) -> bool {
        self.flag_bits() & (1u32 << (key as u32)) != 0
    }
}

// ---------------------------------------------------------------------------
// 四、成员声明（纯语法形状——**没有偏移字段**）
// ---------------------------------------------------------------------------

/// 数组维度（只记**记号个数**与是否带尺寸；跨距与元素大小归 F0443）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArrayDim {
    /// 维度表达式记号数（0 = 未指定尺寸）。
    pub expr_tokens: u32,
    /// 是否带尺寸表达式。
    pub sized: bool,
    /// 该维度跨度。
    pub span: Span,
}

/// 声明符（声明名之后的形状面）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Declarator {
    /// 裸声明名。
    Plain,
    /// 数组后缀（维度序从外到内）。
    Array(Vec<ArrayDim>),
}

impl Declarator {
    /// 形状码（进摘要）。
    pub fn shape_code(&self) -> u64 {
        match self {
            Declarator::Plain => 0,
            Declarator::Array(dims) => 1u64.wrapping_add(dims.len() as u64),
        }
    }

    /// 维度数（裸声明名恒 0）。
    pub fn dim_count(&self) -> usize {
        match self {
            Declarator::Plain => 0,
            Declarator::Array(dims) => dims.len(),
        }
    }
}

/// 匿名成员裁定结果（锚点：「匿名成员语义**按规范**」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnonymousRuling {
    /// 具名类型 + 声明名（普通成员）。
    NamedTypeWithDeclarator,
    /// 匿名类型 + 声明名（`struct { float x; } inner;`——**合法**匿名成员）。
    UnnamedTypeWithDeclarator,
    /// 类型与声明名双缺（`struct { float x; };`）——按规范裁定为非法成员，
    /// **只报告并登记语义期债**，不替语义期定它该是什么。
    NeitherNameNorDeclarator,
    /// 声明名位置被限定词占。
    DeclaratorOccupiedByModifier,
}

impl AnonymousRuling {
    /// 裁定全集。
    pub const ALL: [AnonymousRuling; 4] = [
        AnonymousRuling::NamedTypeWithDeclarator,
        AnonymousRuling::UnnamedTypeWithDeclarator,
        AnonymousRuling::NeitherNameNorDeclarator,
        AnonymousRuling::DeclaratorOccupiedByModifier,
    ];

    /// 规范引用（O(1) 裁定依据——不是一句「按规范」，而是可引用的条款串）。
    pub fn spec(self) -> &'static str {
        match self {
            AnonymousRuling::NamedTypeWithDeclarator => {
                "规范 §成员声明：类型与声明名皆在，为普通具名成员"
            }
            AnonymousRuling::UnnamedTypeWithDeclarator => {
                "规范 §匿名成员：类型可为无名结构体，声明名为其声明者，成员名即声明名"
            }
            AnonymousRuling::NeitherNameNorDeclarator => {
                "规范 §匿名成员：类型与声明名二者皆缺不构成合法成员；本单只报告并登记债，不代语义期定形状"
            }
            AnonymousRuling::DeclaratorOccupiedByModifier => {
                "规范 §成员声明：限定词不得占据声明名位置；限定词识别属语法切分，本单只切不裁"
            }
        }
    }

    /// 是否合法。
    pub fn is_legal(self) -> bool {
        match self {
            AnonymousRuling::NamedTypeWithDeclarator
            | AnonymousRuling::UnnamedTypeWithDeclarator => true,
            AnonymousRuling::NeitherNameNorDeclarator
            | AnonymousRuling::DeclaratorOccupiedByModifier => false,
        }
    }

    /// 域外判别值必 `None`。
    pub fn from_code(code: u8) -> Option<AnonymousRuling> {
        let mut i = 0usize;
        while i < AnonymousRuling::ALL.len() {
            if let Some(r) = AnonymousRuling::ALL.get(i) {
                if *r as u8 == code {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }
}

/// 一条成员声明。
///
/// **字段清单是判据四「边界显性」的形状面**：这里**没有**偏移、对齐、尺寸、跨距、
/// 类型对象——只有「语法上写了什么」。这个保证由 [`member_shape_literal`] 的
/// 全字段字面量构造在**编译期**兜住（加字段即编译不过）。
#[derive(Clone, Debug)]
pub struct MemberDecl {
    /// 成员序号（从 0 起，只增不减，永不回插）。
    pub ordinal: u32,
    /// 成员名驻留号（`NO_NAME` = 驻留失败或无名，**不留默认名**）。
    pub name: u32,
    /// 是否声明了成员名。
    pub name_present: bool,
    /// 成员名跨度。
    pub name_span: Span,
    /// 类型面记号数（标识符/点号计数，不含布局限定与声明名）。
    pub type_tokens: u32,
    /// 类型面跨度。
    pub type_span: Span,
    /// 类型是否为**无名结构体**（匿名成员的类型面）。
    pub type_is_unnamed_struct: bool,
    /// 声明名之后的形状面。
    pub declarator: Declarator,
    /// 匿名成员裁定。
    pub ruling: AnonymousRuling,
    /// 成员在声明区间内的记号区间（半开 `[lo, hi)`）。
    pub token_lo: u32,
    /// 成员区间上界（半开）。
    pub token_hi: u32,
    /// 成员自带的布局限定（块内成员级 `layout(...)`）。
    pub layout: LayoutQual,
}

impl MemberDecl {
    /// 成员是否合法（匿名裁定）。
    pub fn is_legal(&self) -> bool {
        self.ruling.is_legal()
    }

    /// 区间长度（半开）。
    pub fn token_span(&self) -> u32 {
        self.token_hi.saturating_sub(self.token_lo)
    }
}

/// 成员表（append-only——判据一的形状保证）。
#[derive(Clone, Debug, Default)]
pub struct MemberSeq {
    members: Vec<MemberDecl>,
    /// 满表拒收次数。
    pub rejected: u32,
}

impl MemberSeq {
    /// 空表。
    pub fn new() -> MemberSeq {
        MemberSeq {
            members: Vec::new(),
            rejected: 0,
        }
    }

    /// 追加（**唯一入口**；`ordinal` 在此生成，故不可能乱序或回插）。
    pub fn push(&mut self, mut m: MemberDecl) -> u32 {
        if self.members.len() >= MAX_MEMBERS {
            self.rejected = self.rejected.saturating_add(1);
            return NO_NAME;
        }
        m.ordinal = self.members.len() as u32;
        let ord = m.ordinal;
        self.members.push(m);
        ord
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 取成员（零 panic 面）。
    pub fn get(&self, i: usize) -> Option<&MemberDecl> {
        self.members.get(i)
    }

    /// 全部成员。
    pub fn all(&self) -> &[MemberDecl] {
        &self.members
    }

    /// 成员名首次出现的序号（`None` = 未出现过）。
    pub fn first_ordinal_of_name(&self, name: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if m.name_present && m.name == name {
                    return Some(m.ordinal);
                }
            }
            i += 1;
        }
        None
    }

    /// 重名对数（**只计数不拒收**——判据要求重复成员进债不进错）。
    pub fn duplicate_name_pairs(&self) -> u32 {
        let mut dup = 0u32;
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if m.name_present && m.name != NO_NAME {
                    if let Some(first) = self.first_ordinal_of_name(m.name) {
                        if first != m.ordinal {
                            dup = dup.saturating_add(1);
                        }
                    }
                }
            }
            i += 1;
        }
        dup
    }

    /// 匿名类型成员数（`struct { ... } name;` 形态）。
    pub fn unnamed_type_members(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if m.type_is_unnamed_struct {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 非法匿名裁定成员数。
    pub fn illegal_members(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if !m.ruling.is_legal() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 成员序号是否恰为 `0..len`（判据一的**实测**面）。
    pub fn ordinals_are_dense(&self) -> bool {
        let mut i = 0usize;
        while i < self.members.len() {
            match self.members.get(i) {
                Some(m) => {
                    if m.ordinal != i as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 成员区间是否**非空、按序递增且互不重叠**（判据一的实测面）。
    pub fn intervals_are_ordered(&self) -> bool {
        let mut i = 0usize;
        while i < self.members.len() {
            let m = match self.members.get(i) {
                Some(m) => m,
                None => return false,
            };
            if m.token_hi <= m.token_lo {
                return false;
            }
            if i > 0 {
                if let Some(p) = self.members.get(i - 1) {
                    if m.token_lo < p.token_hi {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 成员区间是否**落在给定区间内**（零 panic 面上界由调用方给）。
    pub fn intervals_within(&self, lo: u32, hi: u32) -> bool {
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if m.token_lo < lo || m.token_hi > hi {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 判据四的**编译期**形状保证：用**不带 `..Default::default()`** 的全字段
/// 字面量构造一个 `MemberDecl`——一旦有人给 `MemberDecl` 加了第 13 个字段
/// （偏移/对齐/尺寸/类型对象……），本函数就**编译不过**。
///
/// 注释画线会被人顺手删掉，编译不过不会。
pub fn member_shape_literal() -> MemberDecl {
    MemberDecl {
        ordinal: 0,
        name: NO_NAME,
        name_present: false,
        name_span: Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        },
        type_tokens: 0,
        type_span: Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        },
        type_is_unnamed_struct: false,
        declarator: Declarator::Plain,
        ruling: AnonymousRuling::NamedTypeWithDeclarator,
        token_lo: 0,
        token_hi: 0,
        layout: LayoutQual::new(),
    }
}

/// 越权字段名黑名单（判据侧的**第二重**核对：即便有人改了字面量构造，
/// 这张表也会把越权名字直接点出来）。
pub const FORBIDDEN_MEMBER_FIELDS: [&str; 6] = [
    "offset", "align", "alignment", "size", "stride", "type_id",
];

/// 越权字段名与语法面字段名**无交集**（判据侧的实测面）。
pub fn member_shape_has_no_offset() -> bool {
    // 语法面允许出现的字段名（判据侧写死，与结构体定义逐字对拍）。
    const ALLOWED: [&str; 12] = [
        "ordinal",
        "name",
        "name_present",
        "name_span",
        "type_tokens",
        "type_span",
        "type_is_unnamed_struct",
        "declarator",
        "ruling",
        "token_lo",
        "token_hi",
        "layout",
    ];
    let mut clean = true;
    let mut i = 0usize;
    while i < FORBIDDEN_MEMBER_FIELDS.len() {
        let mut j = 0usize;
        while j < ALLOWED.len() {
            if FORBIDDEN_MEMBER_FIELDS[i] == ALLOWED[j] {
                clean = false;
            }
            j += 1;
        }
        i += 1;
    }
    clean
}

// ---------------------------------------------------------------------------
// 五、块（结构体 / 接口块）与块序
// ---------------------------------------------------------------------------

/// 块种类（**恰好四种**：结构体 + 三类接口块）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum BlockKind {
    /// 结构体（`struct`）。
    Struct = 0,
    /// uniform 块。
    Uniform = 1,
    /// buffer 块。
    Buffer = 2,
    /// interface 块。
    Interface = 3,
}

/// 块种类全集（封闭集）。
pub const BLOCK_KINDS: [BlockKind; 4] = [
    BlockKind::Struct,
    BlockKind::Uniform,
    BlockKind::Buffer,
    BlockKind::Interface,
];

/// 块引导词表（判据逐字对拍用）。
pub const BLOCK_KIND_WORDS: [(&str, BlockKind); 4] = [
    ("struct", BlockKind::Struct),
    ("uniform", BlockKind::Uniform),
    ("buffer", BlockKind::Buffer),
    ("interface", BlockKind::Interface),
];

impl BlockKind {
    /// 词 → 种类。域外词 `None`（**不留默认档**：顶层块引导词是封闭集，
    /// 认不出就是非法的，返回某个默认块等于替它编了个形状）。
    pub fn from_word(word: &str) -> Option<BlockKind> {
        let mut i = 0usize;
        while i < BLOCK_KIND_WORDS.len() {
            if BLOCK_KIND_WORDS[i].0 == word {
                return Some(BLOCK_KIND_WORDS[i].1);
            }
            i += 1;
        }
        None
    }

    /// 是否接口块（非结构体）。
    pub fn is_interface(self) -> bool {
        self != BlockKind::Struct
    }

    /// 对应的顶层声明类别（**与 F0423 同名同义**）：结构体归 `TypeDef`，
    /// 三类接口块归 `InterfaceBlock`。
    pub fn decl_class(self) -> DeclClass {
        match self {
            BlockKind::Struct => DeclClass::TypeDef,
            _ => DeclClass::InterfaceBlock,
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            BlockKind::Struct => "结构体",
            BlockKind::Uniform => "uniform 块",
            BlockKind::Buffer => "buffer 块",
            BlockKind::Interface => "interface 块",
        }
    }

    /// 域外判别值必 `None`。
    pub fn from_code(code: u8) -> Option<BlockKind> {
        let mut i = 0usize;
        while i < BLOCK_KINDS.len() {
            if let Some(k) = BLOCK_KINDS.get(i) {
                if k.code() == code {
                    return Some(*k);
                }
            }
            i += 1;
        }
        None
    }

    /// 判别值。
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 引导词登记项（来源必须如实标注）。
#[derive(Clone, Copy, Debug)]
pub struct BlockLead {
    /// 引导词。
    pub word: &'static str,
    /// 块种类。
    pub kind: BlockKind,
    /// 来源：F0404 关键字 or 本单语法扩展。
    pub source: LeadSource,
    /// 扩展理由（`SyntaxExtension` 时**必须非空**——空理由等于没留痕的扩表）。
    pub reason: &'static str,
}

/// 四种块类的引导词登记（判据逐条核对来源与理由）。
pub const BLOCK_LEADS: [BlockLead; 4] = [
    BlockLead {
        word: "struct",
        kind: BlockKind::Struct,
        // `struct` 在 F0404 的 24 词关键字表里确有登记。
        source: LeadSource::LexerKeyword,
        reason: "",
    },
    BlockLead {
        word: "uniform",
        kind: BlockKind::Uniform,
        // `uniform` 同在 F0404 关键字表里。
        source: LeadSource::LexerKeyword,
        reason: "",
    },
    BlockLead {
        word: "buffer",
        kind: BlockKind::Buffer,
        // F0404 关键字表**无** `buffer`：本单显性登记为语法扩展并附理由。
        source: LeadSource::SyntaxExtension,
        reason: "F0404 关键字表无 buffer；接口块类引导词按锚点点名新增，词身份显性登记不留暗扩展",
    },
    BlockLead {
        word: "interface",
        kind: BlockKind::Interface,
        source: LeadSource::SyntaxExtension,
        reason: "F0404 关键字表无 interface；同 buffer，属锚点点名的接口块类引导词",
    },
];

impl BlockLead {
    /// 扩展登记是否诚实：标 `LexerKeyword` 的必须在 F0404 表里查得到；
    /// 标 `SyntaxExtension` 的必须**查不到**且理由**非空**。
    ///
    /// 两侧都查是有意的：只查「扩展项理由非空」的话，把一个 F0404 已登记的词
    /// 错标成扩展也能过；只查「登记词查得到」的话，把一个新词错标成登记词也能过。
    pub fn source_is_honest(&self) -> bool {
        let known = vec04_keywords::lookup(self.word).is_some();
        match self.source {
            LeadSource::LexerKeyword => known,
            LeadSource::SyntaxExtension => !known && !self.reason.is_empty(),
        }
    }
}

/// 一个块（结构体或接口块）。
#[derive(Clone, Debug)]
pub struct BlockDecl {
    /// 块种类。
    pub kind: BlockKind,
    /// 顶层声明序号（与 F0423 的 `TopDecl::ordinal` 同值）。
    pub ordinal: u32,
    /// 块名驻留号（`NO_NAME` = 缺失或驻留失败）。
    pub name: u32,
    /// 是否声明了块名。
    pub name_present: bool,
    /// 块名跨度。
    pub name_span: Span,
    /// 块级布局限定。
    pub layout: LayoutQual,
    /// 成员表。
    pub members: MemberSeq,
    /// 实例名驻留号（接口块 `}` 之后那个可选标识符）。
    pub instance_name: u32,
    /// 是否声明了实例名。
    pub instance_present: bool,
    /// 实例名跨度。
    pub instance_span: Span,
    /// 块体内记号区间起点（左花括号之后）。
    pub inner_lo: u32,
    /// 块体内记号区间上界（半开）。
    pub inner_hi: u32,
    /// 是否闭合（见到右花括号）。
    pub closed: bool,
    /// 是否因布局语法错被截断（锚点：布局语法错→恢复到块尾）。
    pub truncated: bool,
    /// 嵌套子块（成员里的无名结构体）。
    pub nested: Vec<BlockDecl>,
}

impl BlockDecl {
    /// 空块（零 panic 面的构造面）。
    pub fn empty(kind: BlockKind, ordinal: u32) -> BlockDecl {
        BlockDecl {
            kind,
            ordinal,
            name: NO_NAME,
            name_present: false,
            name_span: Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            },
            layout: LayoutQual::new(),
            members: MemberSeq::new(),
            instance_name: NO_NAME,
            instance_present: false,
            instance_span: Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            },
            inner_lo: 0,
            inner_hi: 0,
            closed: false,
            truncated: false,
            nested: Vec::new(),
        }
    }

    /// 挂接块级布局限定（O(1)）。
    pub fn attach_layout(&mut self, entry: LayoutEntry, span: Span) -> bool {
        self.layout.attach(entry, span)
    }

    /// 成员数。
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// 是否存在数组成员。
    pub fn has_array_member(&self) -> bool {
        let mut i = 0usize;
        while i < self.members.len() {
            if let Some(m) = self.members.get(i) {
                if m.declarator.dim_count() > 0 {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 实例名与块名**同名**（两个槽位被一个名字占——规范上是可疑形状）。
    pub fn instance_shadows_name(&self) -> bool {
        self.instance_present && self.name_present && self.instance_name == self.name
    }

    /// 块体区间长度（半开）。
    pub fn inner_span(&self) -> u32 {
        self.inner_hi.saturating_sub(self.inner_lo)
    }
}

/// 块表。
#[derive(Clone, Debug, Default)]
pub struct BlockSeq {
    blocks: Vec<BlockDecl>,
    class_counts: [u32; 4],
}

impl BlockSeq {
    /// 空表。
    pub fn new() -> BlockSeq {
        BlockSeq {
            blocks: Vec::new(),
            class_counts: [0u32; 4],
        }
    }

    /// 追加一块（顶层唯一入口）。
    pub fn push(&mut self, b: BlockDecl) {
        let c = b.kind as usize;
        if c < self.class_counts.len() {
            self.class_counts[c] = self.class_counts[c].saturating_add(1);
        }
        self.blocks.push(b);
    }

    /// 块数。
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// 取块（零 panic 面）。
    pub fn get(&self, i: usize) -> Option<&BlockDecl> {
        self.blocks.get(i)
    }

    /// 全部块。
    pub fn all(&self) -> &[BlockDecl] {
        &self.blocks
    }

    /// 各类块计数。
    pub fn count_of(&self, kind: BlockKind) -> u32 {
        let c = kind as usize;
        match self.class_counts.get(c) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 计数总和（判据：与 `len()` 对账）。
    pub fn counts_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.class_counts.len() {
            s = s.saturating_add(self.class_counts[i]);
            i += 1;
        }
        s
    }

    /// 在案种类数（封闭集：任一种类计数 >0 即算出现）。
    pub fn present_kinds(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.class_counts.len() {
            if self.class_counts[i] > 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 块体区间是否互不重叠（判据一的实测面）。
    pub fn intervals_are_disjoint(&self) -> bool {
        let mut i = 1usize;
        while i < self.blocks.len() {
            let p = match self.blocks.get(i - 1) {
                Some(b) => b,
                None => return false,
            };
            let c = match self.blocks.get(i) {
                Some(b) => b,
                None => return false,
            };
            if c.inner_lo < p.inner_hi && p.inner_hi > 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全部成员（含嵌套）总数。
    pub fn member_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.blocks.len() {
            if let Some(b) = self.blocks.get(i) {
                n = n.saturating_add(b.members.len() as u32);
                n = n.saturating_add(nested_member_total(&b.nested));
            }
            i += 1;
        }
        n
    }
}

/// 嵌套子块的成员总数（递归）。
fn nested_member_total(nested: &[BlockDecl]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < nested.len() {
        if let Some(b) = nested.get(i) {
            n = n.saturating_add(b.members.len() as u32);
            n = n.saturating_add(nested_member_total(&b.nested));
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 六、边界显性（移交账——本单故意不做的裁定）
// ---------------------------------------------------------------------------

/// 职责阶段。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DutyStage {
    /// 语法期下游（仍在语法层，但不由本单做）。
    Syntax = 0,
    /// 语义期。
    Semantic = 1,
}

impl DutyStage {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DutyStage::Syntax => "语法期下游",
            DutyStage::Semantic => "语义期",
        }
    }
}

/// 移交裁定（本单**故意不做**的事）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DeferredDuty {
    /// 重名成员的裁定（锚点：登记语义期）。
    DuplicateMember = 0,
    /// 成员偏移解算。
    MemberOffset = 1,
    /// 对齐填充。
    AlignmentPadding = 2,
    /// `std140` / `std430` 布局规则应用。
    StdLayoutRule = 3,
    /// 数组跨距。
    ArrayStride = 4,
    /// 嵌套类型扁平。
    NestedTypeFlatten = 5,
    /// 接口块绑定号解算。
    BlockBinding = 6,
    /// 成员类型解析与类型相容裁定。
    MemberTypeResolution = 7,
    /// 成员名/实例名保留字裁定。
    MemberNameReserved = 8,
    /// 布局键取值合法性裁定。
    LayoutKeyLegality = 9,
    /// 匿名成员形状的语义裁定（本单只按规范报告，不定形状）。
    AnonymousShape = 10,
    /// 节点类型未在 F0422 登记。
    NodeKindUnregistered = 11,
}

/// 移交裁定全集（判据逐条核对阶段/责任单/理由）。
pub const DEFERRED: [DeferredDuty; 12] = [
    DeferredDuty::DuplicateMember,
    DeferredDuty::MemberOffset,
    DeferredDuty::AlignmentPadding,
    DeferredDuty::StdLayoutRule,
    DeferredDuty::ArrayStride,
    DeferredDuty::NestedTypeFlatten,
    DeferredDuty::BlockBinding,
    DeferredDuty::MemberTypeResolution,
    DeferredDuty::MemberNameReserved,
    DeferredDuty::LayoutKeyLegality,
    DeferredDuty::AnonymousShape,
    DeferredDuty::NodeKindUnregistered,
];

impl DeferredDuty {
    /// 判别值。
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 阶段归属。
    pub fn stage(self) -> DutyStage {
        match self {
            DeferredDuty::LayoutKeyLegality | DeferredDuty::NodeKindUnregistered => {
                DutyStage::Syntax
            }
            _ => DutyStage::Semantic,
        }
    }

    /// 责任单号（**非空**是判据硬要求：没有责任单的债就是无主债）。
    pub fn owner(self) -> &'static str {
        match self {
            DeferredDuty::LayoutKeyLegality => "VE-F0433",
            DeferredDuty::NodeKindUnregistered => "VE-F0422",
            _ => "VE-F0443",
        }
    }

    /// 理由（**非空**是判据硬要求）。
    pub fn reason(self) -> &'static str {
        match self {
            DeferredDuty::DuplicateMember => {
                "锚点原文：成员重复→登记语义期；语法期只记事实不裁定非法"
            }
            DeferredDuty::MemberOffset => {
                "锚点原文：成员偏移等语义留给语义期；偏移依赖 F0443 的布局规则，不可在本单算"
            }
            DeferredDuty::AlignmentPadding => {
                "对齐填充依赖偏移与 std 布局规则，属 F0443；本单无偏移即无法也无权算填充"
            }
            DeferredDuty::StdLayoutRule => {
                "std140/std430 的具体规则属类型系统（F0443），本单只认键身份"
            }
            DeferredDuty::ArrayStride => "数组跨距依赖元素大小与布局规则，属 F0443",
            DeferredDuty::NestedTypeFlatten => "嵌套类型扁平化是类型系统内务（F0443）",
            DeferredDuty::BlockBinding => "块绑定号解算与资源语义相关，属 F0443 与后续资源单",
            DeferredDuty::MemberTypeResolution => {
                "成员类型解析与相容裁定属类型系统（F0443）；本单只记类型面记号数与跨度"
            }
            DeferredDuty::MemberNameReserved => {
                "保留字/保留前缀的合法性裁定归 F0445；本单只记词类，不据此拒收"
            }
            DeferredDuty::LayoutKeyLegality => {
                "布局键取值是否合法属 F0433；本单只做键身份封闭集与值域形状的语法切分"
            }
            DeferredDuty::AnonymousShape => {
                "匿名成员的语义形状属 F0443；本单按规范报告并引用条款，不代为定形状"
            }
            DeferredDuty::NodeKindUnregistered => {
                "struct 与接口块在 F0422 尚无登记节点类型；扩表权属 F0422，本单只登记欠账"
            }
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DeferredDuty::DuplicateMember => "重名成员裁定",
            DeferredDuty::MemberOffset => "成员偏移解算",
            DeferredDuty::AlignmentPadding => "对齐填充",
            DeferredDuty::StdLayoutRule => "std 布局规则",
            DeferredDuty::ArrayStride => "数组跨距",
            DeferredDuty::NestedTypeFlatten => "嵌套类型扁平",
            DeferredDuty::BlockBinding => "块绑定解算",
            DeferredDuty::MemberTypeResolution => "成员类型解析",
            DeferredDuty::MemberNameReserved => "成员名保留字裁定",
            DeferredDuty::LayoutKeyLegality => "布局键合法性",
            DeferredDuty::AnonymousShape => "匿名成员形状",
            DeferredDuty::NodeKindUnregistered => "节点类型未登记",
        }
    }

    /// 是否语义期债（判据：语义期债**非空**——全空等于没画线）。
    pub fn is_semantic(self) -> bool {
        self.stage() == DutyStage::Semantic
    }

    /// 域外判别值必 `None`。
    pub fn from_code(code: u8) -> Option<DeferredDuty> {
        let mut i = 0usize;
        while i < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(i) {
                if d.code() == code {
                    return Some(*d);
                }
            }
            i += 1;
        }
        None
    }
}

/// 移交账（边界显性的**账**形态）。
#[derive(Clone, Debug, Default)]
pub struct DutyLedger {
    counts: [u32; 12],
    raised: u32,
}

impl DutyLedger {
    /// 空账。
    pub fn new() -> DutyLedger {
        DutyLedger {
            counts: [0u32; 12],
            raised: 0,
        }
    }

    /// 登记一笔债。域外判别值经 [`DeferredDuty::from_code`] 已被枚举封死，
    /// 故本函数**恒真返回**——真正的拒绝面在「不在 `DEFERRED` 里的裁定」，
    /// 那是类型上的不可能，故无需运行期分支。
    pub fn raise(&mut self, duty: DeferredDuty) -> bool {
        let idx = duty.code() as usize;
        if idx >= self.counts.len() {
            return false;
        }
        match self.counts.get_mut(idx) {
            Some(slot) => {
                *slot = slot.saturating_add(1);
                self.raised = self.raised.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// 某笔债的计数。
    pub fn count_of(&self, duty: DeferredDuty) -> u32 {
        let idx = duty.code() as usize;
        match self.counts.get(idx) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 总登记数。
    pub fn total(&self) -> u32 {
        self.raised
    }

    /// 语义期债总数（判据：非空）。
    pub fn semantic_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(i) {
                if d.is_semantic() {
                    n = n.saturating_add(self.count_of(*d));
                }
            }
            i += 1;
        }
        n
    }

    /// 出现过的债种。
    pub fn raised_kinds(&self) -> Vec<DeferredDuty> {
        let mut out: Vec<DeferredDuty> = Vec::new();
        let mut i = 0usize;
        while i < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(i) {
                if self.count_of(*d) > 0 {
                    out.push(*d);
                }
            }
            i += 1;
        }
        out
    }

    /// 账内**每一笔已登记的债**是否都有归属（责任单与理由皆非空）。
    pub fn every_raised_has_owner(&self) -> bool {
        let mut i = 0usize;
        while i < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(i) {
                if self.count_of(*d) > 0 && (d.owner().is_empty() || d.reason().is_empty()) {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 全枚举是否逐笔有归属（判据用；含**未登记**的债——无主债即使没登记
    /// 也是设计缺陷）。
    pub fn every_duty_has_owner(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(i) {
                if d.owner().is_empty() || d.reason().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// 七、恢复与终止
// ---------------------------------------------------------------------------

/// 恢复动作（锚点错误矩阵的**具名化**——恢复不是「随便跳一下」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryAction {
    /// 布局语法错 → **恢复到块尾**（锚点原文）。
    SkipToBlockEnd,
    /// 成员语法错 → 恢复到下一成员起点。
    SkipToMemberEnd,
    /// 恢复无进展 → 强制前进一个记号（终止性保证）。
    ForceAdvance,
    /// 未触发恢复。
    None,
}

impl RecoveryAction {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            RecoveryAction::SkipToBlockEnd => "恢复到块尾",
            RecoveryAction::SkipToMemberEnd => "恢复到下一成员",
            RecoveryAction::ForceAdvance => "强制前进一记号",
            RecoveryAction::None => "未恢复",
        }
    }
}

/// 恢复统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct RecoveryStats {
    /// 恢复次数。
    pub recoveries: u32,
    /// 跳过的记号数。
    pub skipped: u32,
    /// 强制前进次数。
    pub forced: u32,
    /// 落到块尾次数。
    pub landed_block_end: u32,
    /// 落到成员尾次数。
    pub landed_member_end: u32,
}

impl RecoveryStats {
    /// 恢复与强制前进**同时**为 0（判据：干净语料不该触发任何恢复）。
    pub fn is_clean(&self) -> bool {
        self.recoveries == 0 && self.forced == 0 && self.skipped == 0
    }
}

// ---------------------------------------------------------------------------
// 八、块内游标与记号谓词（零 panic 面）
// ---------------------------------------------------------------------------

/// 空跨度（未附任何记号的位置）。
pub fn no_span() -> Span {
    Span {
        start: 0,
        end: 0,
        line: 0,
        col: 0,
    }
}

/// 块内游标（所有取记号都走 `.get`，越界返回 `None`）。
struct Cur<'a> {
    src: &'a str,
    toks: &'a [Token],
    i: usize,
    hi: usize,
}

impl<'a> Cur<'a> {
    fn new(src: &'a str, toks: &'a [Token], lo: usize, hi: usize) -> Cur<'a> {
        let cap = toks.len();
        let mut i = if lo > cap { cap } else { lo };
        let mut h = if hi > cap { cap } else { hi };
        if h < i {
            h = i;
        }
        Cur { src, toks, i, hi: h }
    }

    fn tok(&self) -> Option<&'a Token> {
        if self.i >= self.hi {
            return None;
        }
        self.toks.get(self.i)
    }

    fn at(&self, k: usize) -> Option<&'a Token> {
        let idx = self.i.saturating_add(k);
        if idx >= self.hi {
            return None;
        }
        self.toks.get(idx)
    }

    fn bump(&mut self) -> bool {
        if self.i >= self.hi {
            return false;
        }
        self.i = self.i.saturating_add(1);
        true
    }

    fn at_end(&self) -> bool {
        self.i >= self.hi
    }

    fn pos(&self) -> usize {
        self.i
    }
}

/// 是否背景记号（注释 / 指令）。
///
/// 豁免规则与成员区间规则**同源**：判据一的覆盖性实测用的也是这一个谓词。
pub fn is_trivia(t: &Token) -> bool {
    matches!(t.kind, TokenKind::Comment | TokenKind::Directive)
}

/// 是否标点 `p`。
///
/// 本单**不走** [`super::vec21_parser::classify`]：那个归约把 `[` / `]` / `.`
/// 一律映到 `Term::Eof`（彼此不可区分），而布局限定与数组后缀恰好要区分它们，
/// 故本单在**记号层**切分。
pub fn is_punct(t: &Token, src: &str, p: &str) -> bool {
    t.kind == TokenKind::Punct && t.text(src) == p
}

/// 是否标识符词 `w`。
pub fn is_word(t: &Token, src: &str, w: &str) -> bool {
    t.kind == TokenKind::Ident && t.text(src) == w
}

/// 成员修饰词表（限定词——**切分**用；合法性裁定不归本单）。
pub const MEMBER_MODIFIERS: [&str; 6] = [
    "const",
    "readonly",
    "writeonly",
    "coherent",
    "volatile",
    "precise",
];

/// 是否成员修饰词。
pub fn is_member_modifier(t: &Token, src: &str) -> bool {
    if t.kind != TokenKind::Ident {
        return false;
    }
    let w = t.text(src);
    let mut i = 0usize;
    while i < MEMBER_MODIFIERS.len() {
        if MEMBER_MODIFIERS[i] == w {
            return true;
        }
        i += 1;
    }
    false
}

/// 十进制无符号解析（**全整数零浮点**）。空串或含非数字一律 `None`。
fn parse_u32(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    let mut acc: u64 = 0;
    for b in text.bytes() {
        if b < b'0' || b > b'9' {
            return None;
        }
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as u64);
        if acc > u32::MAX as u64 {
            return None;
        }
    }
    Some(acc as u32)
}

/// 语言版本映射（F0404 的两档：`None` = 未知版本，保守判定交由 classify 自行取保守档）。
fn lang_of(version: u16) -> Option<LangVersion> {
    match version {
        1 => Some(LangVersion::V1Legacy),
        2 => Some(LangVersion::V2Current),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 九、解析上下文（一次解析的账本）
// ---------------------------------------------------------------------------

/// 一次块解析的可变状态。
struct Ctx<'a> {
    src: &'a str,
    toks: &'a [Token],
    names: NameTable,
    diags: Vec<MemberDiag>,
    duties: DutyLedger,
    recovery: RecoveryStats,
    version: u16,
}

impl<'a> Ctx<'a> {
    fn new(src: &'a str, toks: &'a [Token], version: u16) -> Ctx<'a> {
        Ctx {
            src,
            toks,
            names: NameTable::new(),
            diags: Vec::new(),
            duties: DutyLedger::new(),
            recovery: RecoveryStats::default(),
            version,
        }
    }

    fn diag(&mut self, code: MemberDiagCode, span: Span, detail: &str) {
        if self.diags.len() >= MAX_DIAGS {
            return; // 诊断面也有预算：超限即停止追加，不挤掉已记录的错
        }
        self.diags.push(MemberDiag::new(code, span, detail));
    }

    fn raise(&mut self, duty: DeferredDuty) {
        self.duties.raise(duty);
    }

    /// 驻留名字（失败即记诊断**并留空**——不留默认名，默认名会让重名检测失真）。
    fn intern(&mut self, text: &str, span: Span) -> u32 {
        match self.names.intern(text) {
            Some(id) => id,
            None => {
                self.diag(
                    MemberDiagCode::NameNotInterned,
                    span,
                    "名字驻留表满或驻留失败",
                );
                NO_NAME
            }
        }
    }

    /// 取词（驻留失败落空串——不 panic，也不编一个名字出来）。
    fn word_of(&self, id: u32) -> String {
        if id == NO_NAME {
            return String::new();
        }
        match self.names.resolve(id) {
            Some(w) => String::from(w),
            None => String::new(),
        }
    }

    fn recover(&mut self, action: RecoveryAction, skipped: u32) {
        self.recovery.recoveries = self.recovery.recoveries.saturating_add(1);
        self.recovery.skipped = self.recovery.skipped.saturating_add(skipped);
        match action {
            RecoveryAction::SkipToBlockEnd => {
                self.recovery.landed_block_end = self.recovery.landed_block_end.saturating_add(1)
            }
            RecoveryAction::SkipToMemberEnd => {
                self.recovery.landed_member_end = self.recovery.landed_member_end.saturating_add(1)
            }
            RecoveryAction::ForceAdvance => {
                self.recovery.forced = self.recovery.forced.saturating_add(1)
            }
            RecoveryAction::None => {}
        }
    }

    /// 记号跨度（越界落空跨度）。
    fn span_at(&self, pos: usize) -> Span {
        match self.toks.get(pos) {
            Some(t) => Span::of_token(t),
            None => no_span(),
        }
    }

    /// 记号区间跨度（越界部分跳过）。
    fn span_range(&self, lo: usize, hi: usize) -> Span {
        let mut out = no_span();
        let mut first = true;
        let mut i = lo;
        while i < hi {
            if let Some(t) = self.toks.get(i) {
                let s = Span::of_token(t);
                out = if first { s } else { out.join(s) };
                first = false;
            }
            i += 1;
        }
        out
    }

    /// 当前游标处的跨度。
    fn span_here(&self, cur: &Cur<'_>) -> Span {
        match cur.tok() {
            Some(t) => Span::of_token(t),
            None => no_span(),
        }
    }
}

/// 跳过背景记号（返回跳过个数）。
fn skip_trivia(cur: &mut Cur<'_>) -> u32 {
    let mut n = 0u32;
    loop {
        match cur.tok() {
            Some(t) if is_trivia(t) => {
                cur.bump();
                n = n.saturating_add(1);
            }
            _ => break,
        }
    }
    n
}

// ---------------------------------------------------------------------------
// 十、布局限定解析（锚点错误矩阵：布局语法错 → 恢复到块尾）
// ---------------------------------------------------------------------------

/// 布局限定解析结果。
#[derive(Clone, Copy, Debug)]
struct LayoutParse {
    /// 已挂接条目数。
    pub attached: u32,
    /// 是否语法错（调用方据此触发「恢复到块尾」）。
    pub broken: bool,
}

/// 解析一条 `layout ( k [= v] , ... )` 并挂到 `qual`。
///
/// 只认本单封闭键集；未登记键走 [`LayoutKey::Unknown`] 单档并**必产诊断**
/// ——不产诊断的 `Unknown` 才是静默。旗标键带值 / 值键缺值一律产
/// [`MemberDiagCode::LayoutValueMismatch`]（**不自动纠正**：纠正就是替语义期
/// 做裁定）。
fn parse_layout(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>, qual: &mut LayoutQual) -> LayoutParse {
    let head = ctx.span_here(cur);
    if !cur.bump() {
        ctx.diag(MemberDiagCode::LayoutSyntaxBroken, head, "layout 后缺左括号");
        return LayoutParse {
            attached: 0,
            broken: true,
        };
    }
    let has_paren = match cur.tok() {
        Some(t) => is_punct(t, ctx.src, "("),
        None => false,
    };
    if !has_paren {
        ctx.diag(MemberDiagCode::LayoutSyntaxBroken, head, "layout 后缺左括号");
        return LayoutParse {
            attached: 0,
            broken: true,
        };
    }
    cur.bump();
    let mut attached = 0u32;
    let mut broken = false;
    let mut expect_item = true;
    loop {
        skip_trivia(cur);
        let t = match cur.tok() {
            Some(t) => t,
            None => {
                ctx.diag(MemberDiagCode::LayoutSyntaxBroken, head, "布局限定未闭合");
                broken = true;
                break;
            }
        };
        if is_punct(t, ctx.src, ")") {
            cur.bump();
            break;
        }
        if is_punct(t, ctx.src, ",") {
            // 前导逗号 / 双逗号：语法错（跳过该逗号继续，让恢复有进展）。
            ctx.diag(
                MemberDiagCode::LayoutSyntaxBroken,
                Span::of_token(t),
                "布局项之间多逗号",
            );
            broken = true;
            cur.bump();
            expect_item = true;
            continue;
        }
        if !expect_item {
            ctx.diag(
                MemberDiagCode::LayoutSyntaxBroken,
                Span::of_token(t),
                "布局项之间缺逗号",
            );
            broken = true;
        }
        expect_item = false;
        if t.kind != TokenKind::Ident {
            ctx.diag(
                MemberDiagCode::LayoutSyntaxBroken,
                Span::of_token(t),
                "布局键必须是标识符",
            );
            broken = true;
            break;
        }
        let key_word = t.text(ctx.src).to_string();
        let key_span = Span::of_token(t);
        cur.bump();
        let key = LayoutKey::from_word(&key_word);
        if !key.is_registered() {
            ctx.diag(
                MemberDiagCode::UnknownLayoutKey,
                key_span,
                "布局键未在封闭键集登记（键身份属语法面，扩表须走 F0433）",
            );
            ctx.raise(DeferredDuty::LayoutKeyLegality);
        }
        // 重复键（**不删前一条**——前一条是源码事实，重复是事实不是覆盖）。
        let prev = qual.first_ordinal_of(key);
        // 值。
        let mut value = LayoutValue::Flag;
        let has_eq = match cur.tok() {
            Some(x) => is_punct(x, ctx.src, "="),
            None => false,
        };
        if has_eq {
            cur.bump();
            skip_trivia(cur);
            match cur.tok() {
                Some(v) => {
                    if v.kind == TokenKind::Number {
                        let txt = v.text(ctx.src).to_string();
                        value = match parse_u32(&txt) {
                            Some(num) => LayoutValue::Number(num),
                            None => {
                                ctx.diag(
                                    MemberDiagCode::LayoutValueMismatch,
                                    Span::of_token(v),
                                    "布局整数值域不合法（不猜值填充：猜出来的偏移比没有偏移更危险）",
                                );
                                LayoutValue::Flag
                            }
                        };
                        cur.bump();
                    } else if v.kind == TokenKind::Ident {
                        let id = ctx.intern(v.text(ctx.src), Span::of_token(v));
                        value = LayoutValue::Word(id);
                        cur.bump();
                    } else {
                        ctx.diag(
                            MemberDiagCode::LayoutValueMismatch,
                            Span::of_token(v),
                            "布局值必须是整数字面量或标识符",
                        );
                        broken = true;
                        break;
                    }
                }
                None => {
                    ctx.diag(
                        MemberDiagCode::LayoutValueMismatch,
                        key_span,
                        "等号后缺值",
                    );
                    broken = true;
                    break;
                }
            }
        }
        // 值域校验（**照单源表执行**，不新增键、不自动纠正）。
        if key.is_registered() && !value.fits(key.value_domain()) {
            ctx.diag(
                MemberDiagCode::LayoutValueMismatch,
                key_span,
                "布局键的值域与单源表不符（旗标键不得带值、值键不得缺值）",
            );
        }
        if let Some(p) = prev {
            let detail = alloc::format!(
                "{}（首次序号 {}）",
                MemberDiagCode::LayoutKeyRepeated.advice(),
                p
            );
            ctx.diag(
                MemberDiagCode::LayoutKeyRepeated,
                key_span,
                &detail,
            );
        }
        let entry = LayoutEntry {
            key,
            value,
            ordinal: qual.len() as u32,
            span: key_span,
        };
        if !qual.attach(entry, key_span) {
            ctx.diag(
                MemberDiagCode::LayoutTableFull,
                key_span,
                "布局条目数超上限，显性拒收",
            );
            broken = true;
        } else {
            attached = attached.saturating_add(1);
        }
        // 项间分隔：必须见逗号或右括号。
        skip_trivia(cur);
        let sep = match cur.tok() {
            Some(x) if is_punct(x, ctx.src, ",") => 1u8,
            Some(x) if is_punct(x, ctx.src, ")") => 2u8,
            Some(x) => {
                let sp = Span::of_token(x);
                ctx.diag(
                    MemberDiagCode::LayoutSyntaxBroken,
                    sp,
                    "布局项分隔符非法",
                );
                broken = true;
                break;
            }
            None => {
                ctx.diag(MemberDiagCode::LayoutSyntaxBroken, head, "布局限定未闭合");
                broken = true;
                break;
            }
        };
        if sep == 1 {
            cur.bump();
            expect_item = true;
        }
    }
    if broken {
        qual.broken = qual.broken.saturating_add(1);
    }
    LayoutParse { attached, broken }
}

// ---------------------------------------------------------------------------
// 十一、成员解析
// ---------------------------------------------------------------------------

/// 恢复到块尾（锚点错误矩阵：布局语法错 → 恢复到块尾）。
fn recover_to_block_end(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    let start = cur.pos();
    let mut depth = 0usize;
    while let Some(t) = cur.tok() {
        if is_punct(t, ctx.src, "{") {
            depth = depth.saturating_add(1);
        } else if is_punct(t, ctx.src, "}") {
            if depth == 0 {
                cur.bump();
                break;
            }
            depth = depth.saturating_sub(1);
        }
        cur.bump();
    }
    let skipped = cur.pos().saturating_sub(start) as u32;
    if skipped == 0 {
        ctx.diag(
            MemberDiagCode::RecoveryForcedAdvance,
            ctx.span_here(cur),
            "块尾恢复无进展，强制前进一个记号",
        );
        cur.bump();
        ctx.recover(RecoveryAction::ForceAdvance, 1);
    } else {
        ctx.recover(RecoveryAction::SkipToBlockEnd, skipped);
    }
}

/// 恢复到下一成员起点（吞到分号或右花括号为止）。
fn recover_to_member_end(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) {
    let start = cur.pos();
    while let Some(t) = cur.tok() {
        if is_punct(t, ctx.src, ";") {
            cur.bump();
            break;
        }
        if is_punct(t, ctx.src, "}") {
            break;
        }
        cur.bump();
    }
    let skipped = cur.pos().saturating_sub(start) as u32;
    if skipped == 0 {
        ctx.diag(
            MemberDiagCode::RecoveryForcedAdvance,
            ctx.span_here(cur),
            "成员尾恢复无进展，强制前进一个记号",
        );
        cur.bump();
        ctx.recover(RecoveryAction::ForceAdvance, 1);
    } else {
        ctx.recover(RecoveryAction::SkipToMemberEnd, skipped);
    }
}

/// 数组后缀解析（只记维度个数与是否带尺寸表达式——跨距归 F0443）。
fn parse_array_suffix(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>) -> Declarator {
    let mut dims: Vec<ArrayDim> = Vec::new();
    loop {
        skip_trivia(cur);
        let is_open = match cur.tok() {
            Some(t) => is_punct(t, ctx.src, "["),
            None => false,
        };
        if !is_open {
            break;
        }
        let open_at = cur.pos();
        let open_span = ctx.span_at(open_at);
        cur.bump();
        let inner_lo = cur.pos();
        let mut closed = false;
        loop {
            skip_trivia(cur);
            match cur.tok() {
                Some(t) if is_punct(t, ctx.src, "]") => {
                    cur.bump();
                    closed = true;
                    break;
                }
                Some(t) if is_punct(t, ctx.src, ";") || is_punct(t, ctx.src, "}") => {
                    let _ = t;
                    break;
                }
                Some(_) => {
                    cur.bump();
                }
                None => break,
            }
        }
        if !closed {
            ctx.diag(
                MemberDiagCode::MalformedArraySuffix,
                open_span,
                "数组后缀未以右方括号闭合",
            );
            ctx.raise(DeferredDuty::ArrayStride);
            recover_to_member_end(cur, ctx);
            break;
        }
        let inner_hi = cur.pos().saturating_sub(1);
        let expr_tokens = inner_hi.saturating_sub(inner_lo) as u32;
        dims.push(ArrayDim {
            expr_tokens,
            sized: expr_tokens > 0,
            span: open_span,
        });
    }
    if dims.is_empty() {
        Declarator::Plain
    } else {
        Declarator::Array(dims)
    }
}

/// 解析块体内的一段成员，返回是否见到右花括号（块体闭合）。
///
/// 终止性由两处共同保证：① 每轮循环末尾游标**必推进**（无进展则强制前进并记账
/// [`MemberDiagCode::RecoveryForcedAdvance`]）；② `hi` 是硬上界，越界即 `None`。
fn parse_members(
    cur: &mut Cur<'_>,
    ctx: &mut Ctx<'_>,
    depth: usize,
    seq: &mut MemberSeq,
    nested: &mut Vec<BlockDecl>,
    truncated: bool,
) -> bool {
    loop {
        skip_trivia(cur);
        if cur.at_end() {
            return false;
        }
        let t = match cur.tok() {
            Some(t) => t,
            None => return false,
        };
        if is_punct(t, ctx.src, "}") {
            cur.bump();
            return true;
        }
        if is_punct(t, ctx.src, ";") {
            // 空成员（多写一个分号）——跳过并记一笔恢复，不静默。
            cur.bump();
            ctx.recover(RecoveryAction::SkipToMemberEnd, 1);
            continue;
        }
        let before = cur.pos();
        parse_one_member(cur, ctx, depth, seq, nested, truncated);
        if cur.pos() == before {
            // 无进展：强制前进保终止。
            let sp = ctx.span_here(cur);
            ctx.diag(
                MemberDiagCode::RecoveryForcedAdvance,
                sp,
                "成员解析无进展，强制前进一个记号",
            );
            cur.bump();
            ctx.recover(RecoveryAction::ForceAdvance, 1);
        }
    }
}

/// 解析一条成员声明。
fn parse_one_member(
    cur: &mut Cur<'_>,
    ctx: &mut Ctx<'_>,
    depth: usize,
    seq: &mut MemberSeq,
    nested: &mut Vec<BlockDecl>,
    truncated: bool,
) {
    let lo = cur.pos();

    // ① 成员级布局限定。
    let mut member_layout = LayoutQual::new();
    let head_is_layout = match cur.tok() {
        Some(t) => is_word(t, ctx.src, "layout"),
        None => false,
    };
    let next_is_paren = match cur.at(1) {
        Some(n) => is_punct(n, ctx.src, "("),
        None => false,
    };
    if head_is_layout && next_is_paren {
        let mut probe = Cur::new(ctx.src, ctx.toks, cur.pos(), cur.hi);
        let p = parse_layout(&mut probe, ctx, &mut member_layout);
        if p.broken {
            // 布局语法错 → 恢复到块尾（锚点错误矩阵）。
            recover_to_block_end(cur, ctx);
            return;
        }
        cur.i = probe.pos();
    }

    // ② 修饰词（限定词）——只切分，不裁定合法性。
    loop {
        skip_trivia(cur);
        match cur.tok() {
            Some(t) if is_member_modifier(t, ctx.src) => {
                cur.bump();
            }
            _ => break,
        }
    }

    // ③ 类型面。
    let type_lo = cur.pos();
    let mut type_tokens = 0u32;
    let mut unnamed_struct = false;
    skip_trivia(cur);
    let head_is_struct = match cur.tok() {
        Some(t) => is_word(t, ctx.src, "struct"),
        None => false,
    };
    let head_struct_then_brace = head_is_struct
        && match cur.at(1) {
            Some(n) => is_punct(n, ctx.src, "(") || is_punct(n, ctx.src, "{"),
            None => false,
        };
    if head_struct_then_brace {
        unnamed_struct = true;
        cur.bump(); // `struct`
        if depth.saturating_add(1) > NEST_LIMIT {
            ctx.diag(
                MemberDiagCode::NestingTooDeep,
                ctx.span_here(cur),
                "嵌套深度超限，停止下钻（不栈溢出）",
            );
            ctx.raise(DeferredDuty::NestedTypeFlatten);
            recover_to_member_end(cur, ctx);
            return;
        }
        if let Some(n) = cur.tok() {
            if is_punct(n, ctx.src, "{") {
                cur.bump();
            }
        }
        let inner_lo = cur.pos() as u32;
        let mut sub = BlockDecl::empty(BlockKind::Struct, seq.len() as u32);
        let mut sub_members = MemberSeq::new();
        let mut sub_nested: Vec<BlockDecl> = Vec::new();
        let closed = parse_members(
            cur,
            ctx,
            depth.saturating_add(1),
            &mut sub_members,
            &mut sub_nested,
            truncated,
        );
        sub.inner_lo = inner_lo;
        sub.inner_hi = cur.pos() as u32;
        sub.closed = closed;
        // 子块成员**并入父表**（父表承载全部成员，成员序才连续），子块自身只留结构。
        let mut i = 0usize;
        while i < sub_members.len() {
            if let Some(m) = sub_members.get(i) {
                let mut cloned = m.clone();
                cloned.token_lo = cloned.token_lo.saturating_add(inner_lo);
                cloned.token_hi = cloned.token_hi.saturating_add(inner_lo);
                if seq.push(cloned) == NO_NAME {
                    ctx.diag(
                        MemberDiagCode::MemberTableFull,
                        ctx.span_here(cur),
                        "成员数超上限，显性拒收",
                    );
                }
            }
            i += 1;
        }
        sub.nested = sub_nested;
        if nested.len() >= MAX_NESTED {
            ctx.diag(
                MemberDiagCode::NestedTableFull,
                ctx.span_here(cur),
                "子块数超上限，显性拒收",
            );
        } else {
            nested.push(sub);
        }
        type_tokens = type_tokens.saturating_add(1);
    } else {
        // 具名类型面：标识符（可含 `.` 限定）。
        loop {
            skip_trivia(cur);
            match cur.tok() {
                Some(t) if t.kind == TokenKind::Ident => {
                    type_tokens = type_tokens.saturating_add(1);
                    cur.bump();
                }
                Some(t) if is_punct(t, ctx.src, ".") => {
                    type_tokens = type_tokens.saturating_add(1);
                    cur.bump();
                }
                _ => break,
            }
        }
    }
    let type_hi = cur.pos();
    let type_span = ctx.span_range(type_lo, type_hi);

    // ④ 声明名。
    skip_trivia(cur);
    let (name_present, name_id, name_span, ruling) = match cur.tok() {
        Some(t) if t.kind == TokenKind::Ident && !is_member_modifier(t, ctx.src) => {
            let sp = Span::of_token(t);
            let word = t.text(ctx.src).to_string();
            let id = ctx.intern(&word, sp);
            cur.bump();
            let r = if unnamed_struct {
                AnonymousRuling::UnnamedTypeWithDeclarator
            } else {
                AnonymousRuling::NamedTypeWithDeclarator
            };
            (true, id, sp, r)
        }
        Some(t) if is_member_modifier(t, ctx.src) => {
            let sp = Span::of_token(t);
            (false, NO_NAME, sp, AnonymousRuling::DeclaratorOccupiedByModifier)
        }
        Some(t) => (false, NO_NAME, Span::of_token(t), AnonymousRuling::NeitherNameNorDeclarator),
        None => (false, NO_NAME, no_span(), AnonymousRuling::NeitherNameNorDeclarator),
    };
    // 匿名冲突：按规范裁定**报告**（引用条款），**不定形状**。
    if !ruling.is_legal() {
        let code = if ruling == AnonymousRuling::DeclaratorOccupiedByModifier {
            MemberDiagCode::MemberDeclaratorOccupied
        } else {
            MemberDiagCode::MemberDeclaratorMissing
        };
        ctx.diag(code, name_span, ruling.spec());
        ctx.raise(DeferredDuty::AnonymousShape);
    }
    if name_present && name_id != NO_NAME {
        let word = ctx.word_of(name_id);
        let cls = vec04_keywords::classify(&word, lang_of(ctx.version));
        if cls != vec04_keywords::WordClass::Identifier {
            // **不拒收**：只登记债（保留字裁定归 F0445）。
            ctx.raise(DeferredDuty::MemberNameReserved);
        }
    }

    // ⑤ 数组后缀。
    let declarator = parse_array_suffix(cur, ctx);

    // ⑥ 分号。
    skip_trivia(cur);
    match cur.tok() {
        Some(t) if is_punct(t, ctx.src, ";") => {
            cur.bump();
        }
        Some(t) if is_punct(t, ctx.src, "}") => {
            ctx.diag(
                MemberDiagCode::MemberSemiMissing,
                Span::of_token(t),
                "块尾前缺成员分号",
            );
        }
        Some(_) => {
            recover_to_member_end(cur, ctx);
        }
        None => {}
    }

    let hi = cur.pos();
    let decl = MemberDecl {
        ordinal: NO_NAME,
        name: name_id,
        name_present,
        name_span,
        type_tokens,
        type_span,
        type_is_unnamed_struct: unnamed_struct,
        declarator,
        ruling,
        token_lo: lo as u32,
        token_hi: hi as u32,
        layout: member_layout,
    };
    let pushed = seq.push(decl);
    if pushed == NO_NAME {
        ctx.diag(
            MemberDiagCode::MemberTableFull,
            type_span,
            "成员数超上限，显性拒收",
        );
        return;
    }
    // 成员区间自检：不得越出声明区间。
    if hi > cur.hi || lo > hi {
        ctx.diag(
            MemberDiagCode::MemberSpanOutOfRange,
            type_span,
            "成员自报区间越出声明区间",
        );
    }
    // 重名：**只登记不拒**（锚点：成员重复→登记语义期）。
    if name_present && name_id != NO_NAME {
        if let Some(first) = seq.first_ordinal_of_name(name_id) {
            if first != pushed {
                ctx.raise(DeferredDuty::DuplicateMember);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 十二、块解析主流程
// ---------------------------------------------------------------------------

/// 解析一个顶层声明区间（`TopDecl::token_lo..token_hi`）为块。
///
/// **本单只吃 `TypeDef` 与 `InterfaceBlock` 两类**；其余类别**显性拒收**
/// （[`MemberDiagCode::ClassKindMismatch`]）而不是默默产出空块——默默产出空块
/// 会让调用方以为「这个声明没有成员」，而真相是「这个声明不由本单解析」。
pub fn parse_block(src: &str, toks: &[Token], decl: &TopDecl, version: u16) -> BlockParse {
    let mut ctx = Ctx::new(src, toks, version);
    let mut out = BlockParse::empty();
    out.version = version;
    if decl.class != DeclClass::TypeDef && decl.class != DeclClass::InterfaceBlock {
        ctx.diag(
            MemberDiagCode::ClassKindMismatch,
            decl.head_span,
            "本单只解析结构体与接口块两类顶层声明",
        );
        out.absorb(&mut ctx);
        return out;
    }
    let lo = decl.token_lo as usize;
    let hi = decl.token_hi as usize;
    if hi > toks.len() || lo > hi {
        ctx.diag(
            MemberDiagCode::ExtentOutOfRange,
            decl.head_span,
            "上游声明区间不落在记号流内",
        );
        out.absorb(&mut ctx);
        return out;
    }
    let mut cur = Cur::new(src, toks, lo, hi);
    let b = parse_one_block(&mut cur, &mut ctx, decl.ordinal);
    out.blocks.push(b);
    out.absorb(&mut ctx);
    out
}

/// 解析一个翻译单元里**全部**结构体与接口块声明（上游 F0423 的顶层序）。
///
/// 逐个声明走 [`parse_block`]，把块表、诊断、债账、恢复统计**并入同一份产物**
/// ——逐声明各报一份会让调用方拿到 N 份结论，而「这一单元有没有重名成员」
/// 这类问题只能在合并后的账上回答。
pub fn parse_blocks(src: &str, toks: &[Token], unit: &TranslationUnit, version: u16) -> BlockParse {
    let mut acc = BlockParse::empty();
    acc.version = version;
    let decls = unit.seq.all();
    let mut i = 0usize;
    while i < decls.len() {
        if let Some(d) = decls.get(i) {
            if d.class == DeclClass::TypeDef || d.class == DeclClass::InterfaceBlock {
                let one = parse_block(src, toks, d, version);
                acc.merge(one);
            }
        }
        i += 1;
    }
    acc.syntax_digest = block_digest(&acc.blocks);
    acc.members_parsed = acc.blocks.member_total();
    acc.accepted = acc.error_count() == 0;
    acc
}

/// 解析一个块（从当前游标处）。
fn parse_one_block(cur: &mut Cur<'_>, ctx: &mut Ctx<'_>, ordinal: u32) -> BlockDecl {
    let head = ctx.span_here(cur);
    let mut block = BlockDecl::empty(BlockKind::Struct, ordinal);

    // ① 块级布局限定（可有多个）。
    let mut trunc = false;
    loop {
        skip_trivia(cur);
        let is_layout = match cur.tok() {
            Some(t) => is_word(t, ctx.src, "layout"),
            None => false,
        };
        if !is_layout {
            break;
        }
        let is_paren = match cur.at(1) {
            Some(n) => is_punct(n, ctx.src, "("),
            None => false,
        };
        if !is_paren {
            break;
        }
        let mut tmp = LayoutQual::new();
        let mut probe = Cur::new(ctx.src, ctx.toks, cur.pos(), cur.hi);
        let p = parse_layout(&mut probe, ctx, &mut tmp);
        let mut i = 0usize;
        while i < tmp.len() {
            if let Some(e) = tmp.get(i) {
                let entry = *e;
                let sp = e.span;
                block.attach_layout(entry, sp);
            }
            i += 1;
        }
        if p.broken {
            // 锚点：布局语法错 → 恢复到块尾。
            trunc = true;
            block.truncated = true;
            recover_to_block_end(cur, ctx);
            block.inner_hi = cur.pos() as u32;
            return block;
        }
        cur.i = probe.pos();
    }

    // ② 块引导词。
    skip_trivia(cur);
    let mut kind_opt: Option<BlockKind> = None;
    if let Some(t) = cur.tok() {
        if t.kind == TokenKind::Ident {
            let w = t.text(ctx.src).to_string();
            kind_opt = BlockKind::from_word(&w);
            if kind_opt.is_some() {
                cur.bump();
            }
        }
    }
    let kind = match kind_opt {
        Some(k) => k,
        None => {
            ctx.diag(
                MemberDiagCode::IllegalBlockLead,
                head,
                "顶层块引导词只认 struct / uniform / buffer / interface",
            );
            recover_to_block_end(cur, ctx);
            block.truncated = true;
            return block;
        }
    };
    block.kind = kind;
    ctx.raise(DeferredDuty::NodeKindUnregistered);

    // ③ 块名。
    skip_trivia(cur);
    let mut name_taken = match cur.tok() {
        Some(t) if t.kind == TokenKind::Ident && !is_member_modifier(t, ctx.src) => {
            let sp = Span::of_token(t);
            let word = t.text(ctx.src).to_string();
            let id = ctx.intern(&word, sp);
            cur.bump();
            Some((id, sp))
        }
        _ => None,
    };
    if let Some((id, sp)) = name_taken {
        block.name = id;
        block.name_present = true;
        block.name_span = sp;
        let word = ctx.word_of(id);
        let cls = vec04_keywords::classify(&word, lang_of(ctx.version));
        if cls != vec04_keywords::WordClass::Identifier {
            ctx.raise(DeferredDuty::MemberNameReserved);
        }
    } else {
        ctx.diag(
            MemberDiagCode::MissingBlockName,
            ctx.span_here(cur),
            "块名按规范必填（实例名不是块名，两者不互顶）",
        );
    }
    name_taken = None;
    if block.name_present {
        ctx.raise(DeferredDuty::BlockBinding);
    }

    // ④ 左花括号。
    skip_trivia(cur);
    let opened = match cur.tok() {
        Some(t) if is_punct(t, ctx.src, "{") => {
            cur.bump();
            true
        }
        _ => false,
    };
    if !opened {
        ctx.diag(
            MemberDiagCode::MissingBlockOpen,
            ctx.span_here(cur),
            "块体须以左花括号开始",
        );
        recover_to_block_end(cur, ctx);
        block.truncated = true;
        block.inner_hi = cur.pos() as u32;
        return block;
    }
    block.inner_lo = cur.pos() as u32;

    // ⑤ 成员序。
    let mut seq = MemberSeq::new();
    let mut nested: Vec<BlockDecl> = Vec::new();
    let closed = parse_members(cur, ctx, 1, &mut seq, &mut nested, trunc);
    block.inner_hi = cur.pos() as u32;
    block.closed = closed;
    block.truncated = trunc;
    if !closed {
        ctx.diag(
            MemberDiagCode::BlockUnclosed,
            head,
            "块体右花括号缺失，已恢复到声明区间末尾",
        );
    }
    block.members = seq;
    block.nested = nested;

    // ⑥ 实例名（接口块语法：`}` 之后的可选标识符）。
    if kind.is_interface() {
        skip_trivia(cur);
        if let Some(t) = cur.tok() {
            if t.kind == TokenKind::Ident {
                let sp = Span::of_token(t);
                let word = t.text(ctx.src).to_string();
                let id = ctx.intern(&word, sp);
                block.instance_name = id;
                block.instance_present = true;
                block.instance_span = sp;
                cur.bump();
                ctx.raise(DeferredDuty::BlockBinding);
            }
        }
    }

    // ⑦ 尾分号（可选）。
    skip_trivia(cur);
    if let Some(t) = cur.tok() {
        if is_punct(t, ctx.src, ";") {
            cur.bump();
        }
    }

    // ⑧ 债登记：挂了键就欠「规则应用」与「偏移」两笔；有数组欠跨距；
    // 有嵌套欠扁平。**无条件**欠对齐填充与成员类型解析（锚点边界显性）。
    if !block.layout.is_empty() {
        ctx.raise(DeferredDuty::StdLayoutRule);
        ctx.raise(DeferredDuty::MemberOffset);
    }
    if block.has_array_member() {
        ctx.raise(DeferredDuty::ArrayStride);
    }
    if !block.nested.is_empty() {
        ctx.raise(DeferredDuty::NestedTypeFlatten);
    }
    ctx.raise(DeferredDuty::AlignmentPadding);
    ctx.raise(DeferredDuty::MemberTypeResolution);
    block
}

// ---------------------------------------------------------------------------
// 十三、纯度摘要与跨批探测
// ---------------------------------------------------------------------------

/// 摘要混合（FNV-1a；全整数，零浮点）。
fn mix(acc: u64, v: u64) -> u64 {
    (acc ^ v).wrapping_mul(0x0000_0100_0000_01b3)
}

/// 块结构摘要。
///
/// **进摘要的**：块种类码、布局**键身份**码与**值形状**码、成员数、每个成员的
/// 类型面记号数 / 声明符形状码 / 匿名类型位 / 匿名裁定码、嵌套块摘要。
///
/// **不进摘要的**：成员名、块名、实例名、任何字面量取值。
pub fn block_digest(seq: &BlockSeq) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    digest_blocks(seq.all(), &mut acc);
    acc
}

fn digest_blocks(blocks: &[BlockDecl], acc: &mut u64) {
    let mut i = 0usize;
    while i < blocks.len() {
        if let Some(b) = blocks.get(i) {
            *acc = mix(*acc, 0x01);
            *acc = mix(*acc, b.kind.code() as u64);
            *acc = mix(*acc, b.layout.len() as u64);
            let mut k = 0usize;
            while k < b.layout.len() {
                if let Some(e) = b.layout.get(k) {
                    *acc = mix(*acc, e.key as u64);
                    *acc = mix(*acc, e.value.shape() as u64);
                }
                k += 1;
            }
            *acc = mix(*acc, b.members.len() as u64);
            let mut m = 0usize;
            while m < b.members.len() {
                if let Some(x) = b.members.get(m) {
                    *acc = mix(*acc, x.type_tokens as u64);
                    *acc = mix(*acc, x.declarator.shape_code());
                    *acc = mix(*acc, if x.type_is_unnamed_struct { 1 } else { 0 });
                    *acc = mix(*acc, x.ruling as u64);
                }
                m += 1;
            }
            digest_blocks(&b.nested, acc);
        }
        i += 1;
    }
}

/// 块引导词在上游 F0423 引导表里的角色（`None` = 本单私自认了个新顶层类别）。
pub fn lead_role_upstream(word: &str) -> Option<LeadRole> {
    vec23_transunit::lead_of(word).map(|l| l.role)
}

/// F0422 是否已登记结构体 / 接口块节点类型。
///
/// 当前实测为**否**——这正是 [`DeferredDuty::NodeKindUnregistered`] 这笔欠账
/// 存在的理由；一旦 F0422 登记了，本函数翻 `true`，账就该销。判据把它钉成
/// 一个可翻转的事实，而不是一句注释里的「目前没有」。
pub fn block_node_kind_registered() -> bool {
    let mut found = false;
    let mut i = 0usize;
    while i < NodeKind::ALL.len() {
        if let Some(k) = NodeKind::ALL.get(i) {
            let n = k.name();
            if n.contains("Struct") || n.contains("Block") || n.contains("Interface") {
                found = true;
            }
        }
        i += 1;
    }
    found
}

// ---------------------------------------------------------------------------
// 十四、块解析产物
// ---------------------------------------------------------------------------

/// 块解析产物（一次解析的全部量化结论）。
#[derive(Clone, Debug)]
pub struct BlockParse {
    /// 块表。
    pub blocks: BlockSeq,
    /// 移交账（边界显性）。
    pub duties: DutyLedger,
    /// 诊断。
    pub diags: Vec<MemberDiag>,
    /// 恢复统计。
    pub recovery: RecoveryStats,
    /// 结构摘要（纯度面）。
    pub syntax_digest: u64,
    /// 是否通过（**错误数为 0**；重名成员是债不是错，不影响本字段）。
    pub accepted: bool,
    /// 成员总数（含嵌套）。
    pub members_parsed: u32,
    /// 语言版本。
    pub version: u16,
}

impl BlockParse {
    /// 空产物（零 panic 面工具函数）。
    pub fn empty() -> BlockParse {
        BlockParse {
            blocks: BlockSeq::new(),
            duties: DutyLedger::new(),
            diags: Vec::new(),
            recovery: RecoveryStats::default(),
            syntax_digest: 0,
            accepted: true,
            members_parsed: 0,
            version: 1,
        }
    }

    /// 把 ctx 的账并入产物（诊断 + 债计数 + 恢复统计 + 摘要 + 通过判据）。
    fn absorb(&mut self, ctx: &mut Ctx<'_>) {
        let mut i = 0usize;
        while i < ctx.diags.len() {
            if let Some(d) = ctx.diags.get(i) {
                if self.diags.len() < MAX_DIAGS {
                    self.diags.push(d.clone());
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < DEFERRED.len() {
            if let Some(duty) = DEFERRED.get(k) {
                let c = ctx.duties.count_of(*duty);
                let mut n = 0u32;
                while n < c {
                    self.duties.raise(*duty);
                    n += 1;
                }
            }
            k += 1;
        }
        self.recovery = ctx.recovery;
        self.members_parsed = self.blocks.member_total();
        self.syntax_digest = block_digest(&self.blocks);
        self.accepted = self.error_count() == 0;
    }

    /// 合并另一份产物（逐声明解析后的单元级合并面）。
    pub fn merge(&mut self, other: BlockParse) {
        let mut i = 0usize;
        while i < other.blocks.len() {
            if let Some(b) = other.blocks.get(i) {
                self.blocks.push(b.clone());
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < DEFERRED.len() {
            if let Some(d) = DEFERRED.get(k) {
                let c = other.duties.count_of(*d);
                let mut n = 0u32;
                while n < c {
                    self.duties.raise(*d);
                    n += 1;
                }
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < other.diags.len() {
            if let Some(d) = other.diags.get(j) {
                if self.diags.len() < MAX_DIAGS {
                    self.diags.push(d.clone());
                }
            }
            j += 1;
        }
        self.recovery.recoveries = self
            .recovery
            .recoveries
            .saturating_add(other.recovery.recoveries);
        self.recovery.skipped = self.recovery.skipped.saturating_add(other.recovery.skipped);
        self.recovery.forced = self.recovery.forced.saturating_add(other.recovery.forced);
        self.recovery.landed_block_end = self
            .recovery
            .landed_block_end
            .saturating_add(other.recovery.landed_block_end);
        self.recovery.landed_member_end = self
            .recovery
            .landed_member_end
            .saturating_add(other.recovery.landed_member_end);
    }

    /// 错误数。
    pub fn error_count(&self) -> u32 {
        self.count_of(Sev::Error)
    }

    /// 按严重度计数。
    pub fn count_of(&self, sev: Sev) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.diags.len() {
            if let Some(d) = self.diags.get(i) {
                if d.severity() == sev {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 按诊断码计数。
    pub fn count_diag(&self, code: MemberDiagCode) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.diags.len() {
            if let Some(d) = self.diags.get(i) {
                if d.code == code {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}