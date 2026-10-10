//! VE-F0423 · 翻译单元与全局声明解析（VE-C 域 · 着色器系统 · 语法组 C02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0423`
//!
//! **判据（锚点原文）**：四类声明、职责边界、依赖登记、语义期不越权、判据。
//!
//! 锚点原文：「翻译单元顶层解析：全局声明序（变量、函数、类型定义、接口块四类）
//! 循环解析、声明间依赖只登记不解析（语义期职责边界）、文件级属性与布局指令挂接；
//! 空翻译单元与纯注释文件语义按规范显性。数据结构：顶层声明序；依赖登记表；
//! 文件级属性挂接。错误路径与降级矩阵：声明重复→登记交语义期裁定（词法语法期不抢
//! 职责）；顶层非法记号→恢复到下一声明；空单元→按规范裁定告知。性能逐项分解：
//! 解析 O(记号数)；登记 O(声明数)；挂接 O(1)。跨批对接点：上游 F0421/F0422；下游
//! F0424-F0426 各声明解析、F0441 语义入口。」
//!
//! 本单交付**顶层那一层循环**：声明怎么切、四类怎么分、声明之间怎么记一笔账、
//! 属性怎么挂、空文件怎么讲。**声明内部不建树**——那是 F0424（结构体与接口块）、
//! F0425（函数与参数）、F0426（变量与限定符）各自的事；本单只负责把每个声明的
//! **区间（extent）** 切干净并交给它们。五条判据逐条都要有**结构性保证**，而不
//! 是「我代码里没写」这种自觉性保证：
//!
//! 1. **四类声明**（判据一）。锚点点名「变量、函数、类型定义、接口块四类」。
//!    这四类在 [`DeclClass`] 里是**恰好四个**码段，判据用
//!    `DECL_CLASSES.len() == 4` 加 [`DeclClass::ALL_FOUR_COUNT`] 双向钉死；每个
//!    类必须能在引导词表 [`TOP_LEADS`] 里找到自己的引导词，且表里每个词都必须登
//!    记来源（F0404 已有关键字 / F0423 扩展）与**非空理由**——理由不可为空，否
//!    则「我把 `type` 认成类型定义引导词」就是一次没留痕的私自扩表。
//!
//! 2. **职责边界**（判据二）。锚点写「声明间依赖只登记不解析（语义期职责边界）」。
//!    本单把边界落成一张 [`DutyLedger`] 移交账：每一条**本单故意不做**的裁定都
//!    登记成 [`DeferredDuty`]，带**阶段归属**（语法期下游 / 语义期）、**责任单号**
//!    与**非空理由**。边界不是靠注释画线，而是靠一张可枚举、可对账、非空的账。
//!
//! 3. **依赖登记**（判据三）。[`DepLedger`] 只记**位置关系**——「用在前」「定义
//!    在前」「根本没有定义」「定义了两次」，四种全部由**记号下标的先后**决定，纯
//!    语法可判。它**不持有任何类型、值或符号对象**，也没有任何「解析成什么」的
//!    入口：[`DepEdge`] 的字段只有驻留号与两个跨度。这不是自律，是形状——想越权
//!    就得先把语义对象塞进账里，而字段里没有地方放。
//!
//! 4. **语义期不越权**（判据四）。判据二说「不抢职责」，判据四说「不越权」，两句
//!    不是一回事：「不抢」是不裁决，「越权」是**偷偷裁决了**。所以本单给两条反证：
//!    - **纯度**：[`TranslationUnit::syntax_digest`]（吃终结符序列）与
//!      [`struct_digest`]（吃类别/修饰/判决/移交项/诊断码）都**不吃标识符文本、
//!      不吃字面量取值**。任何只改名字或只改数字而语法不变的源码，两者必须**逐位
//!      相等**；而改语法的源码必须变（判据配反向对照，防恒常量的假摘要）。
//!    - **不拒**：重复声明**不报错**，[`TranslationUnit::accepted`] 仍可为 `true`，
//!      重复事实只进 [`DepLedger`] 的 `Duplicate` 边与移交账。一个「顺手把重复声明
//!      判红」的实现会在纯度项与不拒项上同时转红——两条都过才放行。
//!
//! 5. **判据**（判据五）。`vec23_checks.rs` 把上面四条拆成机检项，并给每条判据配
//!    **反向对照**（negative control）：纯度项必须同时证明「改名字摘要不变」与「改
//!    语法摘要必变」，否则一个恒返回常量的摘要实现会全绿。
//!
//! ## 与上下游的接口
//!
//! - **上游 F0403**（[`super::vec03_lexer`]）：本单一律消费 `Token`/`TokenKind`，
//!   **不做二次词法**。注释与指令记号在顶层属**背景**：注释计数走记号流（单一
//!   事实来源），指令记号是文件级属性的唯一载体。
//! - **上游 F0404**（[`super::vec04_keywords`]）：引导词表里凡是 F0404 已登记的
//!   关键字，来源字段如实标 `LexerKeyword`；F0404 没有的（`type` / `buffer` /
//!   `interface` / `layout`）标 `SyntaxExtension` 并附理由——**扩展显性化**。
//! - **上游 F0421**（[`super::vec21_parser`]）：借力两处。一是 `Span` 记法；二是
//!   [`term_digest`] 用 `classify` 把记号归到**终结符**，使「语法摘要」与 F0421
//!   的词素层同口径。本单**不改** F0421 的任何东西。
//! - **上游 F0422**（[`super::vec22_ast`]）：翻译单元根与「已有登记节点类型」的
//!   声明（变量 / 函数）落进 [`AstArena`]；**类型定义与接口块在 F0422 尚无登记
//!   节点类型**，本单不擅自扩表，而是登记 [`PendingNodeKind`] 提案移交 F0422 的
//!   版本登记簿——把「缺一张登记」变成**可见的欠账**而不是静默的降级映射。
//! - **下游 F0424/F0425/F0426**：按 [`TopDecl`] 的 `token_lo..token_hi` 与
//!   `extent` 各自回扫声明内部；本单保证这些区间**互不重叠且覆盖全部代码记号**
//!   （判据由 [`TopDeclSeq::coverage`] 实测）。
//! - **下游 F0441**（语义入口）：[`TranslationUnit::replay_into`] 把本单产生的
//!   声明级归约动作重放进任意 [`ActionSink`](super::vec21_parser::ActionSink)；
//!   动作由本单产出（只有顶层扫描知道声明边界），但**动作语义由文法决定**，序
//!   号取 F0421 骨架产生式号，故可被 F0422 的 `mapping_of` 直接消费。
//!
//! ## 零 panic 面 / 零静默
//!
//! 生产代码无 `unwrap` / `expect` / `panic!` / 裸下标越界。名字驻留表定容且**满即
//! 拒绝**（[`TopDiagCode::NameTableFull`]），arena 分配失败给
//! [`TopDiagCode::ArenaRejected`]，恢复无进展强制前进并记账
//! （[`TopDiagCode::RecoveryForcedAdvance`]）。恢复循环每轮都推进游标，故顶层
//! 解析必然终止（判据以「喂十万个垃圾记号仍返回」实测）。零 IO、零全局可变状态。

#![allow(clippy::needless_range_loop)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vec03_lexer::{Token, TokenKind};
use super::vec04_keywords;
use super::vec21_parser::{ActionSink, Reduction, Span, Term};
use super::vec22_ast::{AstArena, AstNode, NodeId, NodeKind};

// ---------------------------------------------------------------------------
// 零、锚点与诊断面
// ---------------------------------------------------------------------------

/// 本单的锚点串。诊断文本里的 `VE-F04xx` 引用一律经 [`TopDiagCode::anchor`] 派生，
/// 不允许调用方自己拼串——拼串一定会拼错，且拼错无法机检。
pub const ANCHOR: &str = "VE-F0423";

/// 诊断严重度。锚点要求「空单元→按规范裁定**告知**」——告知不是错误，所以本单
/// 的诊断面必须有第三档「注记」，否则空单元只能被当成错误报出来（假阳性）或被
/// 吞掉（静默），两条都不合规。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// 错误：语法层确实走不通。
    Error = 0,
    /// 警告：走通了但有降级 / 兜底处置。
    Warning = 1,
    /// 注记：按规范裁定告知，不影响通过。
    Note = 2,
}

impl Severity {
    /// 人话名（渲染与读屏用）。
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "错误",
            Severity::Warning => "警告",
            Severity::Note => "注记",
        }
    }

    /// 全部档位（判据核对三档齐备）。
    pub const ALL: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Note];
}

/// 本单诊断码。每码绑定**唯一**锚点片段；删码留空洞不复用（码位是诊断契约）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum TopDiagCode {
    /// 顶层出现不属于任何声明起始的记号（恢复前记一次）。
    IllegalTopToken = 0,
    /// 恢复无进展，强制前进一个记号（死循环兜底）。
    RecoveryForcedAdvance = 1,
    /// 声明区间未以顶层 `;` 收口。
    ExtentUnclosed = 2,
    /// 声明重复——**仅告知与登记**，不作为拒绝理由（判据四的「不拒」面）。
    DuplicateDeclared = 3,
    /// 空翻译单元（无声明、无代码、无注释、无指令）。
    EmptyUnit = 4,
    /// 纯注释文件（有注释、无代码、无声明）。
    CommentOnlyUnit = 5,
    /// 纯指令文件（有预处理指令、无代码、无声明）。
    DirectiveOnlyUnit = 6,
    /// 有代码记号但一条声明都没解析出来。
    RecoveredToEmpty = 7,
    /// 未知文件级属性键（登记，不拒绝）。
    UnknownAttrKey = 8,
    /// 属性键值括号不闭合 / 值为空（登记，不拒绝）。
    AttrValueMalformed = 9,
    /// 名字驻留表定容已满。
    NameTableFull = 10,
    /// 名字未能驻留，该声明的名字不可查。
    NameNotInterned = 11,
    /// arena 分配被拒（节点未落池，已记账）。
    ArenaRejected = 12,
    /// 声明缺可识别名字（匿名声明，合法性归 F0442）。
    MissingDeclName = 13,
}

impl TopDiagCode {
    /// 码值。
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            TopDiagCode::IllegalTopToken => "顶层非法记号",
            TopDiagCode::RecoveryForcedAdvance => "恢复无进展强制前进",
            TopDiagCode::ExtentUnclosed => "声明区间未收口",
            TopDiagCode::DuplicateDeclared => "声明重复（登记交语义期）",
            TopDiagCode::EmptyUnit => "空翻译单元（按规范告知）",
            TopDiagCode::CommentOnlyUnit => "纯注释文件（按规范告知）",
            TopDiagCode::DirectiveOnlyUnit => "纯指令文件（按规范告知）",
            TopDiagCode::RecoveredToEmpty => "恢复至空（无声明解析出）",
            TopDiagCode::UnknownAttrKey => "未知文件级属性键（登记）",
            TopDiagCode::AttrValueMalformed => "属性键值形制不合法（登记）",
            TopDiagCode::NameTableFull => "名字驻留表已满（登记）",
            TopDiagCode::NameNotInterned => "名字未能驻留（登记）",
            TopDiagCode::ArenaRejected => "节点分配被拒（已记账）",
            TopDiagCode::MissingDeclName => "声明无可识别名字（登记）",
        }
    }

    /// 严重度。**空单元一族一律是注记**——「合法但空」不是错误。
    pub fn severity(self) -> Severity {
        match self {
            TopDiagCode::IllegalTopToken
            | TopDiagCode::RecoveredToEmpty
            | TopDiagCode::ExtentUnclosed => Severity::Error,
            TopDiagCode::RecoveryForcedAdvance
            | TopDiagCode::UnknownAttrKey
            | TopDiagCode::AttrValueMalformed
            | TopDiagCode::NameTableFull
            | TopDiagCode::NameNotInterned
            | TopDiagCode::ArenaRejected => Severity::Warning,
            TopDiagCode::DuplicateDeclared
            | TopDiagCode::EmptyUnit
            | TopDiagCode::CommentOnlyUnit
            | TopDiagCode::DirectiveOnlyUnit
            | TopDiagCode::MissingDeclName => Severity::Note,
        }
    }

    /// 本码的锚点引用（`VE-F04xx#…` 形态）。**唯一的锚点来源**。
    pub fn anchor(self) -> &'static str {
        match self {
            TopDiagCode::IllegalTopToken => "VE-F0423#全局声明序",
            TopDiagCode::RecoveryForcedAdvance => "VE-F0423#恢复到下一声明",
            TopDiagCode::ExtentUnclosed => "VE-F0423#全局声明序",
            TopDiagCode::DuplicateDeclared => "VE-F0423#职责边界",
            TopDiagCode::EmptyUnit => "VE-F0423#空翻译单元",
            TopDiagCode::CommentOnlyUnit => "VE-F0423#纯注释文件",
            TopDiagCode::DirectiveOnlyUnit => "VE-F0423#文件级属性",
            TopDiagCode::RecoveredToEmpty => "VE-F0423#恢复到下一声明",
            TopDiagCode::UnknownAttrKey => "VE-F0423#文件级属性挂接",
            TopDiagCode::AttrValueMalformed => "VE-F0423#文件级属性挂接",
            TopDiagCode::NameTableFull => "VE-F0423#依赖登记表",
            TopDiagCode::NameNotInterned => "VE-F0423#顶层声明序",
            TopDiagCode::ArenaRejected => "VE-F0422#arena池",
            TopDiagCode::MissingDeclName => "VE-F0423#职责边界",
        }
    }

    /// 下一步建议。**不接受空理由**：没有建议的诊断等于把负担甩给用户。
    pub fn advice(self) -> &'static str {
        match self {
            TopDiagCode::IllegalTopToken => "顶层只接受四类声明引导词；删掉该记号或补上引导词",
            TopDiagCode::RecoveryForcedAdvance => {
                "恢复已强制前进以保终止；请检查该记号附近是否缺引导词"
            }
            TopDiagCode::ExtentUnclosed => "声明未以分号收口；补分号或补齐花括号",
            TopDiagCode::DuplicateDeclared => "语法期不裁决重复；请交语义期按规范裁定，或改名",
            TopDiagCode::EmptyUnit => "空翻译单元按规范合法；如非本意，请确认文件内容已保存",
            TopDiagCode::CommentOnlyUnit => "纯注释文件按规范合法；如需生成产物，请补声明",
            TopDiagCode::DirectiveOnlyUnit => "仅含预处理指令按规范合法；指令语义归条件编译词法",
            TopDiagCode::RecoveredToEmpty => "顶层全程恢复到流尾；检查首个非法记号处",
            TopDiagCode::UnknownAttrKey => {
                "未知属性键已登记；合法性交属性与布局限定符语法专项裁定"
            }
            TopDiagCode::AttrValueMalformed => "属性键值应形如 key(value)；括号须成对且值非空",
            TopDiagCode::NameTableFull => "名字表定容已满；缩减同单元顶层名字数量或分文件",
            TopDiagCode::NameNotInterned => "该声明名字不可查；依赖边将退化为按序号登记",
            TopDiagCode::ArenaRejected => "节点未落池；检查 arena 是否已整池释放",
            TopDiagCode::MissingDeclName => "匿名声明合法性交语义期；如需具名请补标识符",
        }
    }

    /// 全部诊断码（判据核对「每码都有锚点」覆盖全集）。
    pub const ALL: [TopDiagCode; 14] = [
        TopDiagCode::IllegalTopToken,
        TopDiagCode::RecoveryForcedAdvance,
        TopDiagCode::ExtentUnclosed,
        TopDiagCode::DuplicateDeclared,
        TopDiagCode::EmptyUnit,
        TopDiagCode::CommentOnlyUnit,
        TopDiagCode::DirectiveOnlyUnit,
        TopDiagCode::RecoveredToEmpty,
        TopDiagCode::UnknownAttrKey,
        TopDiagCode::AttrValueMalformed,
        TopDiagCode::NameTableFull,
        TopDiagCode::NameNotInterned,
        TopDiagCode::ArenaRejected,
        TopDiagCode::MissingDeclName,
    ];
}

/// 一条诊断。只能经 [`TopDiag::new`] 构造，保证锚点由码派生、不可缺省。
#[derive(Clone, Debug)]
pub struct TopDiag {
    /// 诊断码。
    pub code: TopDiagCode,
    /// 源跨度。
    pub span: Span,
    /// 细节文本。
    pub detail: String,
}

impl TopDiag {
    /// 构造诊断。
    pub fn new(code: TopDiagCode, span: Span, detail: &str) -> TopDiag {
        TopDiag {
            code,
            span,
            detail: detail.to_string(),
        }
    }

    /// 严重度（由码派生，**不可被调用方改写**——避免「把错误降级成注记」作弊）。
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// 本诊断的锚点引用。
    pub fn anchored(&self) -> &'static str {
        self.code.anchor()
    }

    /// 三要素合成本行文本（判据用来钉「三要素非空」）。
    pub fn three_elements(&self) -> String {
        let mut s = String::new();
        s.push_str(self.code.anchor());
        s.push(' ');
        s.push_str(self.code.name());
        s.push('：');
        s.push_str(&self.detail);
        s.push_str("｜为什么：");
        s.push_str(self.code.anchor());
        s.push_str("｜下一步：");
        s.push_str(self.code.advice());
        s
    }

    /// 人话一行式。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.code.anchor());
        s.push_str(": ");
        s.push_str(self.severity().name());
        s.push(' ');
        s.push_str(self.code.name());
        s.push_str(" @");
        s.push_str(&self.span.line.to_string());
        s.push(':');
        s.push_str(&self.span.col.to_string());
        s.push(' ');
        s.push_str(&self.detail);
        s
    }
}

// ---------------------------------------------------------------------------
// 一、四类全局声明（判据一）
// ---------------------------------------------------------------------------

/// 全局声明类别。锚点原文点名四类，此处是**恰好四个**码段。
///
/// 为什么窄枚举：类别码会进诊断包与移交清单，判别值布局必须稳定；新增第五类
/// 等于语言演进，须走 [`PendingNodeKind`] 那样的显式登记，而不是悄悄加变体。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DeclClass {
    /// 变量（`let` / `var` / `const`）。
    Variable = 0,
    /// 函数（`fn`）。
    Function = 1,
    /// 类型定义（`struct` / `alias` / `type`）。
    TypeDef = 2,
    /// 接口块（`uniform` / `buffer` / `interface`）。
    InterfaceBlock = 3,
}

/// 锚点要求的四类，按码序返回。判据用它的长度钉死「四类」这个数。
pub const DECL_CLASSES: [DeclClass; 4] = [
    DeclClass::Variable,
    DeclClass::Function,
    DeclClass::TypeDef,
    DeclClass::InterfaceBlock,
];

impl DeclClass {
    /// 锚点点名的四类数量（判据据此钉死这个数）。
    pub const ALL_FOUR_COUNT: usize = 4;

    /// 码值（与判别值一致）。
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DeclClass::Variable => "变量",
            DeclClass::Function => "函数",
            DeclClass::TypeDef => "类型定义",
            DeclClass::InterfaceBlock => "接口块",
        }
    }

    /// 由码值还原，越界给 `None`（判据「四类恰为四」的入口）。
    pub fn from_code(code: u8) -> Option<DeclClass> {
        DECL_CLASSES.get(code as usize).copied()
    }

    /// 本类在 F0422 登记簿里**已有**的节点类型。
    ///
    /// 返回 `None` 的两类（类型定义 / 接口块）意味着 F0422 的 [`NodeKind`] 全集
    /// 里还没有对应节点类型。本单**不擅自扩表**（那是 F0422 的版本登记职责），
    /// 而是生成 [`PendingNodeKind`] 提案，把欠账显性化。
    pub fn registered_node_kind(self, has_init: bool) -> Option<NodeKind> {
        match self {
            DeclClass::Variable => Some(if has_init {
                NodeKind::LetInitDecl
            } else {
                NodeKind::LetDecl
            }),
            DeclClass::Function => Some(NodeKind::FnDecl),
            DeclClass::TypeDef | DeclClass::InterfaceBlock => None,
        }
    }
}

/// 引导词来源。**扩展必须显性**：本单认的引导词里，F0404 已登记的与本单扩展的
/// 分开记账，判据逐条核对来源字段非空、扩展项理由非空。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeadSource {
    /// F0404 关键字表已登记（[`super::vec04_keywords::KEYWORDS`]）。
    LexerKeyword,
    /// F0423 扩展（F0404 未覆盖，接口块语法面需要）。
    SyntaxExtension,
}

/// 引导词在顶层的作用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeadRole {
    /// 引导一个声明（带所属类）。
    ClassLead,
    /// 前置修饰（可叠加，本身不开启声明），如布局限定前缀。
    Modifier,
}

/// 一条顶层引导词登记项。
#[derive(Clone, Copy, Debug)]
pub struct LeadWord {
    /// 词面。
    pub word: &'static str,
    /// 所属类（`Modifier` 角色时无意义，置 [`DeclClass::Variable`] 占位）。
    pub class: DeclClass,
    /// 角色。
    pub role: LeadRole,
    /// 来源。
    pub source: LeadSource,
    /// 登记理由（**不可为空**——空理由的引导词等于私自扩表）。
    pub reason: &'static str,
}

/// 顶层引导词表。
///
/// **为什么需要它而不直接复用 F0421 的 [`Term`]**：F0421 的 17 个终结符是**骨架
/// 文法**的词素集，只含 `fn` / `let` / `if` / `else` / `return`。类型定义与接口
/// 块在骨架里**根本不存在**，而锚点要求本单认四类。改 F0421 会踩已收口单的地
/// 盘，所以本单自带引导词表，并把「哪些是扩展」逐条写明——扩展显性化，比悄悄
/// 扩别人的表好。
pub const TOP_LEADS: [LeadWord; 11] = [
    LeadWord {
        word: "let",
        class: DeclClass::Variable,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "变量声明主引导词，F0404 既有",
    },
    LeadWord {
        word: "var",
        class: DeclClass::Variable,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "变量声明的存储类变体，F0404 既有",
    },
    LeadWord {
        word: "const",
        class: DeclClass::Variable,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "常量变量仍属变量类，F0404 既有",
    },
    LeadWord {
        word: "fn",
        class: DeclClass::Function,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "函数声明唯一引导词，F0404 既有",
    },
    LeadWord {
        word: "struct",
        class: DeclClass::TypeDef,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "结构体类型定义，F0404 既有",
    },
    LeadWord {
        word: "alias",
        class: DeclClass::TypeDef,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "类型别名机制按规范登记为类型定义，F0404 既有",
    },
    LeadWord {
        word: "type",
        class: DeclClass::TypeDef,
        role: LeadRole::ClassLead,
        source: LeadSource::SyntaxExtension,
        reason: "类型定义的显式写法；F0404 关键字表未收，本单扩展并登记理由",
    },
    LeadWord {
        word: "uniform",
        class: DeclClass::InterfaceBlock,
        role: LeadRole::ClassLead,
        source: LeadSource::LexerKeyword,
        reason: "uniform 接口块，F0404 既有",
    },
    LeadWord {
        word: "buffer",
        class: DeclClass::InterfaceBlock,
        role: LeadRole::ClassLead,
        source: LeadSource::SyntaxExtension,
        reason: "buffer 接口块；F0404 未收，本单扩展并登记理由",
    },
    LeadWord {
        word: "interface",
        class: DeclClass::InterfaceBlock,
        role: LeadRole::ClassLead,
        source: LeadSource::SyntaxExtension,
        reason: "通用接口块；F0404 未收，本单扩展并登记理由",
    },
    LeadWord {
        word: "layout",
        class: DeclClass::Variable,
        role: LeadRole::Modifier,
        source: LeadSource::SyntaxExtension,
        reason: "布局限定前缀，可叠加于任意声明前；本身不开启声明",
    },
];

/// 查引导词（定长表扫描 → 相对记号数 O(1)）。
pub fn lead_of(word: &str) -> Option<&'static LeadWord> {
    let mut i = 0usize;
    while i < TOP_LEADS.len() {
        if TOP_LEADS[i].word == word {
            return Some(&TOP_LEADS[i]);
        }
        i += 1;
    }
    None
}

/// 该词是否是顶层声明起始（引导词，含修饰词）。
pub fn is_decl_lead_word(word: &str) -> bool {
    lead_of(word).is_some()
}

/// 该词是否是前置修饰（可叠加）。
pub fn is_modifier_word(word: &str) -> bool {
    match lead_of(word) {
        Some(l) => l.role == LeadRole::Modifier,
        None => false,
    }
}

/// 按类取引导词（判据用来核对「四类各有引导词」）。
pub fn leads_of_class(class: DeclClass) -> Vec<&'static LeadWord> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < TOP_LEADS.len() {
        if TOP_LEADS[i].role == LeadRole::ClassLead && TOP_LEADS[i].class == class {
            out.push(&TOP_LEADS[i]);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 二、名字驻留（O(1) 驻留，定容满即拒）
// ---------------------------------------------------------------------------

/// 名字表容量（2 的幂，定容避免重哈希）。
pub const NAME_CAP: usize = 256;

/// 槽位空标记。
const NAME_SLOT_EMPTY: u32 = u32::MAX;

/// 名字驻留表：定容开放寻址，驻留与查回均 O(1)。
///
/// **为什么定容而不是可增长**：内核无全局分配预算，且「驻留失败」必须是一条
/// 可见的诊断而不是 panic 或悄悄降级。定容让「满」这件事在语法层就可判定。
#[derive(Clone, Debug)]
pub struct NameTable {
    names: Vec<String>,
    slots: Vec<u32>,
}

impl NameTable {
    /// 新建空表。
    pub fn new() -> NameTable {
        NameTable {
            names: Vec::new(),
            slots: vec![NAME_SLOT_EMPTY; NAME_CAP],
        }
    }

    /// FNV-1a 定容散列。
    fn hash(s: &str) -> usize {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in s.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        (h % (NAME_CAP as u64)) as usize
    }

    /// 驻留一个名字；空串或表满返回 `None`。
    pub fn intern(&mut self, s: &str) -> Option<u32> {
        if s.is_empty() || self.names.len() >= NAME_CAP {
            return None;
        }
        let mut idx = NameTable::hash(s);
        let mut probes = 0usize;
        while probes < NAME_CAP {
            let slot = self.slots[idx];
            if slot == NAME_SLOT_EMPTY {
                let id = self.names.len() as u32;
                self.names.push(String::from(s));
                self.slots[idx] = id;
                return Some(id);
            }
            if let Some(existing) = self.names.get(slot as usize) {
                if existing.as_str() == s {
                    return Some(slot);
                }
            }
            idx = (idx + 1) % NAME_CAP;
            probes += 1;
        }
        None
    }

    /// 由驻留号查回名字；越界给 `None`。
    pub fn resolve(&self, id: u32) -> Option<&str> {
        match self.names.get(id as usize) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 已驻留名字数。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 定容容量。
    pub fn capacity(&self) -> usize {
        NAME_CAP
    }
}

// ---------------------------------------------------------------------------
// 三、依赖登记（判据三：只登记不解析）
// ---------------------------------------------------------------------------

/// 依赖边的关系分类。**四种全部由记号下标先后决定**，纯语法可判。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepRelation {
    /// 定义在引用之前。
    DefinedEarlier = 0,
    /// 引用在定义之前（前向引用）——**登记，不报错**。
    Forward = 1,
    /// 本单元内找不到定义（可能外部注入或真错）——**登记，不报错**。
    Unresolved = 2,
    /// 同名定义了多次——**登记，交语义期裁定**。
    Duplicate = 3,
}

impl DepRelation {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DepRelation::DefinedEarlier => "定义在前",
            DepRelation::Forward => "前向引用",
            DepRelation::Unresolved => "本单元无定义",
            DepRelation::Duplicate => "重复定义",
        }
    }

    /// 全部关系（判据核对四类齐备——锚点列的三种情形加重复定义共四类）。
    pub const ALL: [DepRelation; 4] = [
        DepRelation::DefinedEarlier,
        DepRelation::Forward,
        DepRelation::Unresolved,
        DepRelation::Duplicate,
    ];
}

/// 一条依赖边。
///
/// **注意这个结构里没有类型、没有值、没有符号对象**——只有驻留号与两个跨度。
/// 「只登记不解析」之所以是结构性保证而不是自律，正是因为想解析也得先有地方
/// 放解析结果，而这里没有。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepEdge {
    /// 名字驻留号。
    pub name: u32,
    /// 引用点跨度（`Unresolved` / `Duplicate` 时为该点的跨度）。
    pub use_site: Span,
    /// 定义点跨度（`Unresolved` 时等于 `use_site`）。
    pub define_site: Span,
    /// 关系分类。
    pub relation: DepRelation,
    /// 定义所属类别（引用侧无定义时为 `Function`，仅占位不参与裁决）。
    pub class: DeclClass,
}

/// 一次定义记录（驻留号 + 跨度 + 类别 + 记号序）。
#[derive(Clone, Copy, Debug)]
struct DefineRec {
    name: u32,
    span: Span,
    class: DeclClass,
    seq: u32,
}

/// 一次引用记录。
#[derive(Clone, Copy, Debug)]
struct UseRec {
    name: u32,
    span: Span,
    seq: u32,
}

/// 依赖登记表。
///
/// 登记流程**两趟**：扫描期只压 [`UseRec`] / [`DefineRec`]（O(1)/条，对齐锚点
/// 「登记 O(声明数)」）；`finalize` 趟按 `seq` 先后配对出 [`DepEdge`]。两趟分离
/// 的理由：扫描期还不知道后面的定义，配对只能等扫完；而「等扫完再配对」比
/// 「边扫边猜」诚实——猜出来的前后关系会被一个后置定义推翻。
#[derive(Clone, Debug)]
pub struct DepLedger {
    defines: Vec<DefineRec>,
    uses: Vec<UseRec>,
    edges: Vec<DepEdge>,
    finalized: bool,
    counts: [u32; 4],
}

impl DepLedger {
    /// 新建空账。
    pub fn new() -> DepLedger {
        DepLedger {
            defines: Vec::new(),
            uses: Vec::new(),
            edges: Vec::new(),
            finalized: false,
            counts: [0u32; 4],
        }
    }

    /// 记一条定义。返回 `true` 表示首次定义，`false` 表示重复（**不拒绝**）。
    pub fn note_define(&mut self, name: u32, span: Span, class: DeclClass, seq: u32) -> bool {
        let mut first = true;
        let mut i = 0usize;
        while i < self.defines.len() {
            if self.defines[i].name == name {
                first = false;
                break;
            }
            i += 1;
        }
        self.defines.push(DefineRec {
            name,
            span,
            class,
            seq,
        });
        first
    }

    /// 记一条引用（O(1)）。
    pub fn note_use(&mut self, name: u32, span: Span, seq: u32) {
        self.uses.push(UseRec { name, span, seq });
    }

    /// 配对成依赖边。**幂等**：重复调用不重复计数。
    pub fn finalize(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        for u in self.uses.iter() {
            // 找该名字的**首个**定义（记号序最小者）。
            let mut best: Option<DefineRec> = None;
            let mut i = 0usize;
            while i < self.defines.len() {
                let d = self.defines[i];
                if d.name == u.name {
                    match best {
                        None => best = Some(d),
                        Some(b) => {
                            if d.seq < b.seq {
                                best = Some(d);
                            }
                        }
                    }
                }
                i += 1;
            }
            let rel = match best {
                None => DepRelation::Unresolved,
                Some(d) => {
                    if d.seq <= u.seq {
                        DepRelation::DefinedEarlier
                    } else {
                        DepRelation::Forward
                    }
                }
            };
            let (define_site, class) = match best {
                Some(d) => (d.span, d.class),
                None => (u.span, DeclClass::Function),
            };
            self.counts[rel as usize] += 1;
            self.edges.push(DepEdge {
                name: u.name,
                use_site: u.span,
                define_site,
                relation: rel,
                class,
            });
        }
        // 重复定义：每个名字的第 2..n 个定义各记一条 Duplicate 边。
        let mut i = 0usize;
        while i < self.defines.len() {
            let target = self.defines[i].name;
            let mut seen = 0u32;
            let mut j = 0usize;
            while j < self.defines.len() {
                if self.defines[j].name == target {
                    seen += 1;
                    if seen > 1 {
                        let d = self.defines[j];
                        self.counts[DepRelation::Duplicate as usize] += 1;
                        self.edges.push(DepEdge {
                            name: d.name,
                            use_site: d.span,
                            define_site: d.span,
                            relation: DepRelation::Duplicate,
                            class: d.class,
                        });
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }

    /// 依赖边（须先 [`DepLedger::finalize`]）。
    pub fn edges(&self) -> &[DepEdge] {
        &self.edges
    }

    /// 某关系的边数。
    pub fn count_of(&self, rel: DepRelation) -> u32 {
        self.counts[rel as usize]
    }

    /// 边总数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 定义记录数（判据对齐「登记 O(声明数)」）。
    pub fn define_count(&self) -> usize {
        self.defines.len()
    }

    /// 引用记录数。
    pub fn use_count(&self) -> usize {
        self.uses.len()
    }

    /// 是否已配对。
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }
}

// ---------------------------------------------------------------------------
// 四、职责边界：移交账（判据二）
// ---------------------------------------------------------------------------

    /// 移交阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DutyStage {
    /// 仍在语法组，但属于别的单（错误恢复）。
    Syntax = 0,
    /// 语义期（F0441 起）。
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

/// 一条**本单故意不做**的裁定。
///
/// 这张表是判据二的载体。「职责边界」写成注释，谁都能说自己没越界；写成枚举，
/// 越界就成了「表里没有这项却做了」或「表里有这项却悄悄做了」两种可查的错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeferredDuty {
    /// 同名重复定义是否合法（语法期只登记）。
    DuplicateDefinition = 0,
    /// 前向引用是否允许（语法期只登记）。
    ForwardReference = 1,
    /// 本单元无定义的名字能否成立（可能外部注入）。
    UnresolvedName = 2,
    /// 声明名命中保留字是否允许。
    ReservedName = 3,
    /// 未知文件级属性键是否合法。
    AttrKeyLegality = 4,
    /// 布局限定组合是否合法。
    LayoutCombination = 5,
    /// 匿名声明（无可识别名字）是否合法。
    MissingDeclName = 6,
    /// 类型定义 / 接口块尚无 F0422 登记节点类型，需补登记。
    NodeKindUnregistered = 7,
    /// 声明区间未收口时的错误恢复（归 F0434）。
    ExtentUnclosedRecovery = 8,
}

/// 锚点点名的四类，**逐条**登记：阶段 + 责任单号 + 理由。
///
/// 责任单号不是装饰——判据核对「语义期项的责任单号必须落在 F044x 段」，这样
/// 「我把某个语义裁定偷偷在语法期做了」会表现为「某个语义期项没有被任何责任单
/// 认领」或「被一个不属 F044x 的单号认领」。
pub const DEFERRED_DUTIES: [DeferredDuty; 9] = [
    DeferredDuty::DuplicateDefinition,
    DeferredDuty::ForwardReference,
    DeferredDuty::UnresolvedName,
    DeferredDuty::ReservedName,
    DeferredDuty::AttrKeyLegality,
    DeferredDuty::LayoutCombination,
    DeferredDuty::MissingDeclName,
    DeferredDuty::NodeKindUnregistered,
    DeferredDuty::ExtentUnclosedRecovery,
];

impl DeferredDuty {
    /// 移交阶段。
    ///
    /// **阶段是按责任单所在的批次划的，不是按「这事难不难」划的**：
    /// 属性键合法性与布局组合合法性虽然听起来像语义问题，但它们的责任单是
    /// F0433（属性与布局限定符语法，与本单同在语法组 C02），所以归
    /// `Syntax`；而「重名是否合法」的责任单是 F0442（语义组），才归 `Semantic`。
    /// 阶段与责任单必须一致——判据据此核对「`Semantic` 项的责任单一律在
    /// F044x 段」，两者对不上就说明这张表本身在撒谎。
    pub fn stage(self) -> DutyStage {
        match self {
            DeferredDuty::AttrKeyLegality
            | DeferredDuty::LayoutCombination
            | DeferredDuty::NodeKindUnregistered
            | DeferredDuty::ExtentUnclosedRecovery => DutyStage::Syntax,
            _ => DutyStage::Semantic,
        }
    }

    /// 责任单号。
    pub fn owner(self) -> &'static str {
        match self {
            DeferredDuty::DuplicateDefinition => "VE-F0442",
            DeferredDuty::ForwardReference => "VE-F0442",
            DeferredDuty::UnresolvedName => "VE-F0442",
            DeferredDuty::ReservedName => "VE-F0445",
            DeferredDuty::AttrKeyLegality => "VE-F0433",
            DeferredDuty::LayoutCombination => "VE-F0433",
            DeferredDuty::MissingDeclName => "VE-F0442",
            DeferredDuty::NodeKindUnregistered => "VE-F0422",
            DeferredDuty::ExtentUnclosedRecovery => "VE-F0434",
        }
    }

    /// 移交理由（**不可为空**）。
    pub fn reason(self) -> &'static str {
        match self {
            DeferredDuty::DuplicateDefinition => {
                "重名是否合法取决于作用域链与重载规则，属符号表裁决"
            }
            DeferredDuty::ForwardReference => "前向引用是否允许取决于语言版本的求值序规范条款",
            DeferredDuty::UnresolvedName => "名字可能由外部单元注入，语法期无全视图不能判未定义",
            DeferredDuty::ReservedName => {
                "保留字可用性随语言版本变化（F0404 版本差异表），语法期无版本输入"
            }
            DeferredDuty::AttrKeyLegality => "属性键的合法集随资源模型演进，值域校验归属性专项",
            DeferredDuty::LayoutCombination => "布局限定组合合法性依赖类型系统与目标平台，属专项裁定",
            DeferredDuty::MissingDeclName => "匿名结构体 / 匿名成员是否合法按类型系统规范裁决",
            DeferredDuty::NodeKindUnregistered => "节点类型新增须走 F0422 版本登记簿，语法期不得私扩表",
            DeferredDuty::ExtentUnclosedRecovery => "区间未收口的处置策略（三层同步集）属错误恢复单",
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            DeferredDuty::DuplicateDefinition => "重复定义裁定",
            DeferredDuty::ForwardReference => "前向引用裁定",
            DeferredDuty::UnresolvedName => "未定义名裁定",
            DeferredDuty::ReservedName => "保留字名裁定",
            DeferredDuty::AttrKeyLegality => "属性键合法性裁定",
            DeferredDuty::LayoutCombination => "布局组合合法性裁定",
            DeferredDuty::MissingDeclName => "匿名声明裁定",
            DeferredDuty::NodeKindUnregistered => "节点类型补登记",
            DeferredDuty::ExtentUnclosedRecovery => "区间未收口恢复",
        }
    }

    /// 责任单号是否落在语义段（F044x）。
    pub fn owner_is_semantic_phase(self) -> bool {
        match self.stage() {
            DutyStage::Semantic => self.owner().starts_with("VE-F044"),
            DutyStage::Syntax => true,
        }
    }
}

/// 移交账：逐项计数（不做去重——同一项触发多次就是要看次数）。
#[derive(Clone, Debug)]
pub struct DutyLedger {
    counts: [u32; 9],
    raised: u32,
}

impl DutyLedger {
    /// 新建空账。
    pub fn new() -> DutyLedger {
        DutyLedger {
            counts: [0u32; 9],
            raised: 0,
        }
    }

    /// 记一次移交（未登记项被拒并返回 `false`，不 panic）。
    pub fn raise(&mut self, duty: DeferredDuty) -> bool {
        let idx = duty as usize;
        if idx >= DEFERRED_DUTIES.len() || DEFERRED_DUTIES[idx] != duty {
            return false;
        }
        self.counts[idx] += 1;
        self.raised += 1;
        true
    }

    /// 某项计数。
    pub fn count_of(&self, duty: DeferredDuty) -> u32 {
        let idx = duty as usize;
        if idx >= DEFERRED_DUTIES.len() {
            return 0;
        }
        self.counts[idx]
    }

    /// 累计移交次数。
    pub fn total(&self) -> u32 {
        self.raised
    }

    /// 被触发过的移交项（去重列表，判据对账用）。
    pub fn raised_kinds(&self) -> Vec<DeferredDuty> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < DEFERRED_DUTIES.len() {
            if self.counts[i] > 0 {
                out.push(DEFERRED_DUTIES[i]);
            }
            i += 1;
        }
        out
    }

    /// 语义期移交项的累计次数（判据：语义期项必须有责任单）。
    pub fn semantic_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < DEFERRED_DUTIES.len() {
            if DEFERRED_DUTIES[i].stage() == DutyStage::Semantic {
                n += self.counts[i];
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 五、文件级属性挂接（锚点「挂接 O(1)」）
// ---------------------------------------------------------------------------

/// 文件级属性键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttrKey {
    /// `location(n)`。
    Location = 0,
    /// `binding(n)`。
    Binding = 1,
    /// `set(n)`。
    Set = 2,
    /// `layout(...)` 布局限定。
    Layout = 3,
    /// `precision(...)` 精度限定。
    Precision = 4,
    /// `interface(...)` 接口阶段。
    Interface = 5,
    /// `uniform(...)` 资源类别参数。
    Uniform = 6,
    /// 预处理控制指令（非属性；只计数并移交条件编译 / 包含两单）。
    PreproControl = 7,
    /// 未知键（**登记，不拒绝**）。
    Unknown = 8,
}

impl AttrKey {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            AttrKey::Location => "location",
            AttrKey::Binding => "binding",
            AttrKey::Set => "set",
            AttrKey::Layout => "layout",
            AttrKey::Precision => "precision",
            AttrKey::Interface => "interface",
            AttrKey::Uniform => "uniform",
            AttrKey::PreproControl => "预处理控制指令",
            AttrKey::Unknown => "未知属性键",
        }
    }

    /// 是不是**已登记**的属性（控制指令与未知键都不是）。
    pub fn is_registered_attribute(self) -> bool {
        matches!(
            self,
            AttrKey::Location
                | AttrKey::Binding
                | AttrKey::Set
                | AttrKey::Layout
                | AttrKey::Precision
                | AttrKey::Interface
                | AttrKey::Uniform
        )
    }

    /// 全部键（判据核对全集）。
    pub const ALL: [AttrKey; 9] = [
        AttrKey::Location,
        AttrKey::Binding,
        AttrKey::Set,
        AttrKey::Layout,
        AttrKey::Precision,
        AttrKey::Interface,
        AttrKey::Uniform,
        AttrKey::PreproControl,
        AttrKey::Unknown,
    ];
}

/// 已知属性键词表（词面 → 键）。**定长表**，查表 O(1)。
pub const ATTR_KEYS: [(&str, AttrKey); 7] = [
    ("location", AttrKey::Location),
    ("binding", AttrKey::Binding),
    ("set", AttrKey::Set),
    ("layout", AttrKey::Layout),
    ("precision", AttrKey::Precision),
    ("interface", AttrKey::Interface),
    ("uniform", AttrKey::Uniform),
];

/// 预处理控制指令词表（与 F0411 词法登记的指令名一致）。
///
/// **为什么要单列**：控制指令不是属性，把 `#ifdef` 当成未知属性键报警告是**假
/// 阳性**——它在 F0413/F0414 已被消费。本单对它们只计数并移交，不产生任何属
/// 性类诊断。词表与 `vec11_prepro::DIRECTIVES` 对齐，避免两处各写一份。
pub const CONTROL_WORDS: [&str; 9] = [
    "include", "define", "undef", "if", "ifdef", "ifndef", "elif", "else", "endif",
];

/// 指令记号的属性解析结果。
#[derive(Clone, Debug)]
pub struct AttrParse {
    /// 解析出的键。
    pub key: AttrKey,
    /// 键值文本（已剥一层括号；控制指令为空）。
    pub value: String,
    /// 形制是否不合法（登记，不拒绝）。
    pub malformed: bool,
}

/// 解析一条指令记号的文本为属性键值。
///
/// 指令记号文本形如 `# location(0)` / `#layout(std140)`。规则：
/// 1. 必须以 `#` 起，否则 `malformed`；
/// 2. 剥 `#` 与其后空白，取标识符词面；
/// 3. 词面命中 [`CONTROL_WORDS`] → [`AttrKey::PreproControl`]，不取键值；
/// 4. 否则查 [`ATTR_KEYS`]，未命中 → [`AttrKey::Unknown`]；
/// 5. 键值：紧随的 `(...)`，成对取内文；不成对或值为空 → `malformed`。
pub fn parse_directive_attr(text: &str) -> AttrParse {
    let mut malformed = false;
    let mut key = AttrKey::Unknown;
    let mut value = String::new();
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'#') {
        return AttrParse {
            key,
            value,
            malformed: true,
        };
    }
    let after = &text[1..];
    let lead = after.len() - after.trim_start().len();
    let body = after[lead..].trim_start();
    // 取标识符词面。
    let bb = body.as_bytes();
    let mut wend = 0usize;
    while wend < bb.len() && (bb[wend].is_ascii_alphanumeric() || bb[wend] == b'_') {
        wend += 1;
    }
    if wend == 0 {
        return AttrParse {
            key,
            value,
            malformed: true,
        };
    }
    let word = &body[..wend];
    let mut i = 0usize;
    while i < CONTROL_WORDS.len() {
        if CONTROL_WORDS[i] == word {
            return AttrParse {
                key: AttrKey::PreproControl,
                value: String::new(),
                malformed: false,
            };
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < ATTR_KEYS.len() {
        if ATTR_KEYS[k].0 == word {
            key = ATTR_KEYS[k].1;
            break;
        }
        k += 1;
    }
    let rest = body[wend..].trim_start();
    if rest.is_empty() {
        // 无参属性合法（裸键形如 `#pragma`），不算不合法。
        return AttrParse { key, value, malformed: false };
    }
    if !rest.starts_with('(') {
        return AttrParse {
            key,
            value,
            malformed: true,
        };
    }
    match rest.find(')') {
        None => AttrParse {
            key,
            value,
            malformed: true,
        },
        Some(close) => {
            if close == 1 {
                // 空括号：键存在但值缺失，登记为不合法而不是当作空值放行。
                malformed = true;
            }
            value = String::from(&rest[1..close]);
            AttrParse {
                key,
                value,
                malformed,
            }
        }
    }
}

/// 一条文件级属性。
#[derive(Clone, Debug)]
pub struct FileAttr {
    /// 属性键。
    pub key: AttrKey,
    /// 键值。
    pub value: String,
    /// 源跨度（指令记号跨度）。
    pub span: Span,
    /// 出现序（按源序，判据对账用）。
    pub ordinal: u32,
}

/// 属性归属的「未认领槽位」哨兵。
const SLOT_NONE: u32 = u32::MAX;

/// 文件级属性表。
///
/// **挂接 O(1) 的做法**：属性在普查期就按源序压进一条 `Vec`，声明在扫描期按序
/// 定界，于是「哪些属性属于第 k 个声明」可以用**单调游标**一次前推解决——总
/// 代价 O(属性数)，与声明数无关。判据用 [`FileAttrTable::scans`] 实测：任何线性
/// 搜索的实现都会让这个计数大于 0，而定址路径**恒为 0**。
#[derive(Clone, Debug)]
pub struct FileAttrTable {
    attrs: Vec<FileAttr>,
    decl_head: Vec<u32>,
    decl_count: Vec<u32>,
    unit_slots: Vec<u32>,
    /// 线性扫描计数（O(1) 判据的观测面；生产路径恒为 0）。
    pub scans: u32,
    /// 定址读取计数。
    pub lookups: u32,
    /// 首条代码记号的位置；它之前的属性一律是文件级。
    pub code_start_pos: usize,
}

impl FileAttrTable {
    /// 新建空表。
    pub fn new(code_start_pos: usize) -> FileAttrTable {
        FileAttrTable {
            attrs: Vec::new(),
            decl_head: Vec::new(),
            decl_count: Vec::new(),
            unit_slots: Vec::new(),
            scans: 0,
            lookups: 0,
            code_start_pos,
        }
    }

    /// 登记一条文件级属性（普查期调用，按源序）。
    pub fn push(&mut self, key: AttrKey, value: String, span: Span) -> u32 {
        let ordinal = self.attrs.len() as u32;
        let slot = self.push_raw(FileAttr {
            key,
            value,
            span,
            ordinal,
        });
        self.unit_slots.push(slot);
        slot
    }

    /// 只压属性不登记为单元级（诊断面用）。
    pub fn push_raw(&mut self, attr: FileAttr) -> u32 {
        let slot = self.attrs.len() as u32;
        self.attrs.push(attr);
        slot
    }

    /// 记录一条单元级槽位（普查期内部用）。
    pub fn mark_unit_slot(&mut self, slot: u32) {
        self.unit_slots.push(slot);
    }

    /// 更新首条代码记号位置。
    pub fn set_code_start(&mut self, pos: usize) {
        self.code_start_pos = pos;
    }

    /// 为下一个声明预留槽位（与 `decl_head`/`decl_count` 同序）。
    pub fn open_decl(&mut self) {
        self.decl_head.push(SLOT_NONE);
        self.decl_count.push(0);
    }

    /// 单调游标前推：把位置落在 `[cursor, end_pos)` 窗内的属性划给第
    /// `decl_index` 个声明。返回 `(头槽, 条数)`。
    ///
    /// `cursor` 由调用方持有且**单调不回退**，所以总推进次数等于属性总数——
    /// O(属性数)，与声明数无关（锚点「挂接 O(1)」的兑现）。
    pub fn claim_forward(
        &mut self,
        decl_index: usize,
        cursor: &mut u32,
        end_pos: usize,
    ) -> (u32, u32) {
        if self.decl_head.get(decl_index).copied() != Some(SLOT_NONE) {
            // 已认领过（防御：同一声明不应被划两次）。
            return (SLOT_NONE, 0);
        }
        let start = *cursor;
        let mut i: u32 = start;
        let mut taken = 0u32;
        while (i as usize) < self.attrs.len() {
            let inside = match self.attrs.get(i as usize) {
                Some(a) => a.span.start < end_pos,
                None => break,
            };
            if !inside {
                break;
            }
            i += 1;
            taken += 1;
        }
        if taken == 0 {
            return (SLOT_NONE, 0);
        }
        *cursor = i;
        if let Some(h) = self.decl_head.get_mut(decl_index) {
            *h = start;
        }
        if let Some(c) = self.decl_count.get_mut(decl_index) {
            *c = taken;
        }
        let lo = start;
        let hi = i;
        // 保留**不在** [lo, hi) 内的单元级槽位。注意判据是「槽位号落在区间外」
        // 而不是「位置在 end_pos 之前」——后者会把后续声明的属性误删，而那些
        // 属性此刻还没被认领。守恒判据（各声明挂接数 + 单元级 == 总数）会当场
        // 抓住这个错。
        self.unit_slots.retain(|s| *s < lo || *s >= hi);
        (start, taken)
    }

    /// 取某声明的属性（O(1) 定址切片，不扫描）。
    pub fn attrs_of_decl(&mut self, decl_index: usize) -> &[FileAttr] {
        self.lookups += 1;
        let head = match self.decl_head.get(decl_index) {
            Some(h) => *h,
            None => return &[],
        };
        if head == SLOT_NONE {
            return &[];
        }
        let count = self.decl_count.get(decl_index).copied().unwrap_or(0);
        if count == 0 {
            return &[];
        }
        let lo = head as usize;
        let hi = lo + count as usize;
        match self.attrs.get(lo..hi) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 某声明的挂接条数（O(1)，不切片不扫描）。
    pub fn attr_count_of_decl(&self, decl_index: usize) -> u32 {
        if self.decl_head.get(decl_index).copied() == Some(SLOT_NONE) {
            return 0;
        }
        self.decl_count.get(decl_index).copied().unwrap_or(0)
    }

    /// 仍属单元级的属性槽位（未被任何声明认领）。
    pub fn unit_slots(&self) -> &[u32] {
        &self.unit_slots
    }

    /// 按槽位取属性（越界给 `None`）。
    pub fn attr_at(&self, slot: u32) -> Option<&FileAttr> {
        self.attrs.get(slot as usize)
    }

    /// 属性总数。
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// 某键的属性数（**线性扫描**，只给诊断 / 移交用；会记账到 `scans`）。
    pub fn count_key(&mut self, key: AttrKey) -> u32 {
        self.scans += 1;
        let mut n = 0u32;
        for a in self.attrs.iter() {
            if a.key == key {
                n += 1;
            }
        }
        n
    }

    /// 已登记属性键的种类数（定容表，非扫描）。
    pub fn registered_kinds() -> usize {
        ATTR_KEYS.len()
    }
}

// ---------------------------------------------------------------------------
// 六、顶层声明序（判据一的落点）
// ---------------------------------------------------------------------------

/// 一条顶层声明。
///
/// **只有区间与身份，没有内部结构**——这是本单与 F0424/F0425/F0426 的分工线：
/// 它们从 `token_lo..token_hi` 回扫自己的内部，本单保证这段区间**互不重叠且
/// 覆盖全部代码记号**（判据由 [`TopDeclSeq::coverage`] 实测）。
#[derive(Clone, Debug)]
pub struct TopDecl {
    /// 声明类别。
    pub class: DeclClass,
    /// 声明序（从 0 起）。
    pub ordinal: u32,
    /// 名字驻留号；`name_present == false` 时为 `u32::MAX`。
    pub name: u32,
    /// 是否有可识别名字。
    pub name_present: bool,
    /// 名字跨度。
    pub name_span: Span,
    /// 引导词起始跨度（含前置修饰）。
    pub head_span: Span,
    /// 声明区间跨度（首记号起至区间末记号止）。
    pub extent: Span,
    /// 记号下标半开区间 `[token_lo, token_hi)`。
    pub token_lo: u32,
    pub token_hi: u32,
    /// 变量类：区间顶层深度上是否出现 `=`。
    pub has_init: bool,
    /// 前置修饰个数（布局限定等）。
    pub modifiers: u32,
    /// 区间是否以顶层 `;` 收口。
    pub extent_closed: bool,
    /// 对应 arena 节点。
    pub node: NodeId,
    /// 节点是否落池成功。
    pub node_valid: bool,
    /// 挂接属性的头槽（O(1) 取）。
    pub attr_head: u32,
    /// 挂接属性条数。
    pub attr_count: u32,
}

impl TopDecl {
    /// 本条声明占的记号数。
    pub fn token_span(&self) -> u32 {
        self.token_hi.saturating_sub(self.token_lo)
    }

    /// 是否与第 `other` 条声明的记号区间重叠。
    pub fn overlaps(&self, other: &TopDecl) -> bool {
        self.token_lo < other.token_hi && other.token_lo < self.token_hi
    }
}

/// 顶层声明序。
#[derive(Clone, Debug)]
pub struct TopDeclSeq {
    decls: Vec<TopDecl>,
    class_counts: [u32; 4],
}

impl TopDeclSeq {
    /// 新建空序。
    pub fn new() -> TopDeclSeq {
        TopDeclSeq {
            decls: Vec::new(),
            class_counts: [0u32; 4],
        }
    }

    /// 追加一条声明（类计数同步，O(1)）。
    pub fn push(&mut self, d: TopDecl) {
        let ci = d.class.code() as usize;
        if ci < self.class_counts.len() {
            self.class_counts[ci] += 1;
        }
        self.decls.push(d);
    }

    /// 声明数。
    pub fn len(&self) -> usize {
        self.decls.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// 取第 k 条声明（越界给 `None`）。
    pub fn get(&self, k: usize) -> Option<&TopDecl> {
        self.decls.get(k)
    }

    /// 全部声明（按序）。
    pub fn all(&self) -> &[TopDecl] {
        &self.decls
    }

    /// 某类的声明数。
    pub fn class_count(&self, class: DeclClass) -> u32 {
        self.class_counts[class.code() as usize]
    }

    /// 类别序列。
    pub fn class_sequence(&self) -> Vec<DeclClass> {
        self.decls.iter().map(|d| d.class).collect()
    }

    /// 实际出现过的类别（按码序）。
    pub fn present_classes(&self) -> Vec<DeclClass> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < DECL_CLASSES.len() {
            if self.class_counts[i] > 0 {
                out.push(DECL_CLASSES[i]);
            }
            i += 1;
        }
        out
    }

    /// 四类是否齐全。
    pub fn all_four_present(&self) -> bool {
        self.present_classes().len() == DeclClass::ALL_FOUR_COUNT
    }

    /// 四类计数之和（独立重算，判据防「计数与实表脱钩」）。
    pub fn class_counts_sum(&self) -> u32 {
        let mut n = 0u32;
        for c in self.class_counts.iter() {
            n += *c;
        }
        n
    }

    /// 有可识别名字的声明数。
    pub fn named_count(&self) -> usize {
        let mut n = 0usize;
        for d in self.decls.iter() {
            if d.name_present {
                n += 1;
            }
        }
        n
    }

    /// 区间重叠对数（判据要求恒为 0）。
    pub fn overlap_pairs(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.decls.len() {
            let mut j = i + 1;
            while j < self.decls.len() {
                if self.decls[i].overlaps(&self.decls[j]) {
                    n += 1;
                }
                j += 1;
            }
            i += 1;
        }
        n
    }

    /// 代码记号覆盖：各声明区间下标并集是否**恰好**等于代码记号下标集。
    ///
    /// 返回 `(覆盖数, 多重覆盖数)`。判据要求覆盖数 == 代码记号数且多重覆盖为
    /// 0——这条同时钉死「区间互不重叠」与「无代码记号被声明区间吞掉」。
    pub fn coverage(&self, code_token_indices: &[u32]) -> (u32, u32) {
        let mut covered = 0u32;
        let mut multi = 0u32;
        for i in code_token_indices.iter() {
            let mut hits = 0u32;
            for d in self.decls.iter() {
                if *i >= d.token_lo && *i < d.token_hi {
                    hits += 1;
                }
            }
            if hits >= 1 {
                covered += 1;
            }
            if hits >= 2 {
                multi += 1;
            }
        }
        (covered, multi)
    }
}

/// 空跨度（自检与缺省值用）。
pub const EMPTY_SPAN: Span = Span {
    start: 0,
    end: 0,
    line: 0,
    col: 0,
};

/// 一个 F0422 尚无登记节点类型的声明类别及其**提案版本**。
///
/// 版本提案规则：须**严格高于** F0422 已登记的全部节点类型的引入版本（否则
/// F0422 的登记簿会因「版本未推进」而拒绝），且多个提案之间严格递增。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingNodeKind {
    /// 待补登记的类别。
    pub class: DeclClass,
    /// 提案引入版本。
    pub proposed_since: u16,
}

/// F0422 已登记节点的引入版本上界（= 全集长度，作为基线）。
pub fn registered_version_ceiling() -> u16 {
    let mut n = 0u16;
    let mut i = 0usize;
    while i < NodeKind::ALL.len() {
        n += 1;
        i += 1;
    }
    n
}

/// 生成节点类型提案表：对**本单元实际出现**的未登记类别各给一条。
///
/// 类别清单是**推导**出来的（[`DeclClass::registered_node_kind`] 返回 `None` 的
/// 那些），不是手写常量——手写常量会在 F0422 补登记后悄悄过期。
pub fn pending_node_kinds(version: u16, present: &[DeclClass]) -> Vec<PendingNodeKind> {
    let ceil = registered_version_ceiling();
    // 基线必须**严格高于**既有引入版本上界：F0422 登记簿会拒绝「版本未推进」，
    // 而既有节点占用 1..=ceil，所以取 ceil+1（语言版本更高时直接用语言版本）。
    let base = if version > ceil { version } else { ceil + 1 };
    let mut out = Vec::new();
    let mut bump = 0u16;
    let mut i = 0usize;
    while i < DECL_CLASSES.len() {
        let c = DECL_CLASSES[i];
        let present_here = present.iter().any(|p| *p == c);
        if present_here && c.registered_node_kind(false).is_none() {
            out.push(PendingNodeKind {
                class: c,
                proposed_since: base.saturating_add(bump),
            });
            bump = bump.saturating_add(1);
        }
        i += 1;
    }
    out
}

/// F0421 骨架产生式号：声明级动作的序号取自 F0421 的产生式号，使动作可被
/// F0422 的 `mapping_of` 直接消费（「动作语义由文法决定」的兑现）。
pub fn prod_for_kind(k: NodeKind) -> u16 {
    match k {
        NodeKind::FnDecl => 2,
        NodeKind::LetDecl => 3,
        NodeKind::LetInitDecl => 4,
        _ => u16::MAX,
    }
}

// ---------------------------------------------------------------------------
// 七、同步集与恢复（锚点错误路径：恢复到下一声明）
// ---------------------------------------------------------------------------

/// 顶层同步集（F0421 [`Term`] 侧的**骨架子集**）。
///
/// **为什么词面侧才是全集**：F0421 的骨架 Term 只覆盖骨架文法的引导词，而顶层
/// 四类里有两类（类型定义 / 接口块）的引导词不在骨架里。判据要求「同步集能找回
/// **全部**声明起始」，所以词面侧（[`TOP_LEADS`]）是全集，本表只是它在骨架
/// Term 上的投影。两份都要被核对。
pub const TOP_SYNC_TERMS: [Term; 3] = [Term::KwFn, Term::KwLet, Term::Eof];

/// 记号是否是顶层声明起始。
pub fn is_decl_lead(t: &Token, src: &str) -> bool {
    if t.kind != TokenKind::Ident {
        return false;
    }
    is_decl_lead_word(t.text(src))
}

/// 恢复统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct RecoveryStats {
    /// 恢复次数（每段非法记号一次）。
    pub recoveries: u32,
    /// 恢复跳过的记号数。
    pub skipped: u32,
    /// 无进展强制前进次数（**恒应 > 0 才算真被触发过**，判据反查）。
    pub forced: u32,
    /// 恢复后抵达声明起始的次数。
    pub landed: u32,
}

/// 恢复推进决策：给定非法段起点 `from` 与候选同步点 `target`，返回
/// `(推进到的下标, 是否强制前进)`。
///
/// **为什么把它提成独立函数而不是埋在解析循环里**：判据要求「恢复死循环→强制
/// 同步点兜底」这条兜底**可被验证**。埋进循环里的话，这条分支从 `parse_unit`
/// 的入口不可达（同步点永远在 `from` 之后），判据就只能写成一句恒真的摆设。提
/// 成**全函数**（`target <= from` 是合法输入）之后，兜底逻辑本身能被直接喂进
/// 违规输入验证，而不是靠「反正跑不到」蒙混过去。
///
/// 契约：返回值**恒 > `from`**，故任何调用方按返回值推进游标都必然终止。
pub fn recovery_advance(from: usize, target: usize) -> (usize, bool) {
    if target > from {
        (target, false)
    } else {
        (from + 1, true)
    }
}

// ---------------------------------------------------------------------------
// 八、翻译单元与判决
// ---------------------------------------------------------------------------

/// 翻译单元语义裁定。锚点要求「空翻译单元与纯注释文件语义按规范**显性**」——
/// 显性的含义是这五种情形**各有各的名字**，而不是都塌成「空」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitVerdict {
    /// 有声明。
    Declared = 0,
    /// 空单元：无声明、无代码、无注释、无指令。
    Empty = 1,
    /// 纯注释文件。
    CommentOnly = 2,
    /// 纯指令文件（只有预处理指令）。
    DirectiveOnly = 3,
    /// 有代码但一条声明都没解析出（顶层全程恢复到流尾）。
    RecoveredToEmpty = 4,
}

impl UnitVerdict {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            UnitVerdict::Declared => "有声明",
            UnitVerdict::Empty => "空翻译单元",
            UnitVerdict::CommentOnly => "纯注释文件",
            UnitVerdict::DirectiveOnly => "纯指令文件",
            UnitVerdict::RecoveredToEmpty => "恢复至空",
        }
    }

    /// 是否属于「无声明」族（判据区分 `Declared` 与其余四种）。
    pub fn is_decl_free(self) -> bool {
        self != UnitVerdict::Declared
    }

    /// 对应的告知诊断码（**空单元一族必须是注记**——合法但空不是错误）。
    pub fn notice_diag(self) -> Option<TopDiagCode> {
        match self {
            UnitVerdict::Declared => None,
            UnitVerdict::Empty => Some(TopDiagCode::EmptyUnit),
            UnitVerdict::CommentOnly => Some(TopDiagCode::CommentOnlyUnit),
            UnitVerdict::DirectiveOnly => Some(TopDiagCode::DirectiveOnlyUnit),
            UnitVerdict::RecoveredToEmpty => Some(TopDiagCode::RecoveredToEmpty),
        }
    }

    /// 全部裁定（判据核对全集）。
    pub const ALL: [UnitVerdict; 5] = [
        UnitVerdict::Declared,
        UnitVerdict::Empty,
        UnitVerdict::CommentOnly,
        UnitVerdict::DirectiveOnly,
        UnitVerdict::RecoveredToEmpty,
    ];
}

/// 翻译单元。
#[derive(Clone, Debug)]
pub struct TranslationUnit {
    /// 根节点（`TranslationUnit` 族）。
    pub root: NodeId,
    /// 根节点是否落池成功。
    pub root_valid: bool,
    /// 根跨度。
    pub root_span: Span,
    /// 语言版本（提案版本基线）。
    pub version: u16,
    /// 顶层声明序。
    pub seq: TopDeclSeq,
    /// 依赖登记表。
    pub deps: DepLedger,
    /// 职责边界移交账。
    pub duties: DutyLedger,
    /// 文件级属性表。
    pub attrs: FileAttrTable,
    /// 节点类型提案（移交 F0422 补登记）。
    pub pending: Vec<PendingNodeKind>,
    /// 诊断。
    pub diags: Vec<TopDiag>,
    /// 顶层摘要（吃终结符序列，**不吃标识符**）。
    pub syntax_digest: u64,
    /// 结构摘要（吃类别 / 修饰 / 判决 / 移交 / 诊断码）。
    pub struct_digest: u64,
    /// 语义裁定。
    pub verdict: UnitVerdict,
    /// 语法期是否接受（**重复声明不影响本项**——判据四的「不拒」面）。
    pub accepted: bool,
    /// 代码记号数（不含注释 / 指令 / Eof）。
    pub code_tokens: u32,
    /// 注释记号数。
    pub comment_tokens: u32,
    /// 指令记号数。
    pub directive_tokens: u32,
    /// 恢复统计。
    pub recovery: RecoveryStats,
    /// 声明级归约动作（供重放进任意 [`ActionSink`]）。
    pub actions: Vec<Reduction>,
}

impl TranslationUnit {
    /// 错误级诊断数。
    pub fn error_count(&self) -> u32 {
        let mut n = 0u32;
        for d in self.diags.iter() {
            if d.severity() == Severity::Error {
                n += 1;
            }
        }
        n
    }

    /// 某级诊断数。
    pub fn count_of(&self, sev: Severity) -> u32 {
        let mut n = 0u32;
        for d in self.diags.iter() {
            if d.severity() == sev {
                n += 1;
            }
        }
        n
    }

    /// 某码的诊断数。
    pub fn count_diag(&self, code: TopDiagCode) -> u32 {
        let mut n = 0u32;
        for d in self.diags.iter() {
            if d.code == code {
                n += 1;
            }
        }
        n
    }

    /// 声明级动作序号序列（与 F0421/F0422 的动作序号同口径）。
    pub fn prod_sequence(&self) -> Vec<u16> {
        self.actions.iter().map(|r| r.prod).collect()
    }

    /// 把本单产生的声明级动作重放进任意消费者。
    ///
    /// 下游按需选用：重放进 F0421 的 `NodeTallySink` 核对动作序，重放进 F0422
    /// 的 `ArenaBuilder` 核对节点计数。
    pub fn replay_into<S>(&self, sink: &mut S)
    where
        S: ActionSink + ?Sized,
    {
        for r in self.actions.iter() {
            let _ = ActionSink::on_reduce(sink, r);
        }
    }

    /// 某声明的挂接属性（O(1) 定址）。
    pub fn attrs_of_decl(&mut self, k: usize) -> &[FileAttr] {
        self.attrs.attrs_of_decl(k)
    }

    /// 某声明的挂接属性条数（O(1)）。
    pub fn attr_count_of_decl(&self, k: usize) -> u32 {
        self.attrs.attr_count_of_decl(k)
    }
}

/// 终结符摘要（跳过注释与指令记号——它们不是语法）。
pub fn term_digest(src: &str, toks: &[Token]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for t in toks.iter() {
        if t.kind == TokenKind::Comment || t.kind == TokenKind::Directive {
            continue;
        }
        let term = super::vec21_parser::classify(t, src);
        h ^= term as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// FNV 混合步。
fn mix(acc: u64, v: u64) -> u64 {
    (acc ^ v).wrapping_mul(0x100_0000_01b3)
}

// ---------------------------------------------------------------------------
// 九、顶层扫描器（施工）
// ---------------------------------------------------------------------------

/// 零值节点 id（分配失败时的占位；`node_valid == false` 时不可用）。
const NO_NODE: NodeId = NodeId {
    bucket: 0,
    index: 0,
};

/// 顶层扫描器（施工态）。**私有**：对外只有 [`parse_unit`] 一个入口，这样
/// 「单元的判决只能由一次完整顶层扫描产生」是结构性的。
struct UnitBuilder<'a> {
    src: &'a str,
    toks: &'a [Token],
    version: u16,
    arena: AstArena,
    root_valid: bool,
    seq: TopDeclSeq,
    names: NameTable,
    deps: DepLedger,
    duties: DutyLedger,
    attrs: FileAttrTable,
    diags: Vec<TopDiag>,
    actions: Vec<Reduction>,
    code_tokens: u32,
    comment_tokens: u32,
    directive_tokens: u32,
    recovery: RecoveryStats,
    attr_cursor: u32,
    root_span: Span,
}

impl<'a> UnitBuilder<'a> {
    fn new(src: &'a str, toks: &'a [Token], version: u16) -> UnitBuilder<'a> {
        UnitBuilder {
            src,
            toks,
            version,
            arena: AstArena::new(),
            root_valid: false,
            seq: TopDeclSeq::new(),
            names: NameTable::new(),
            deps: DepLedger::new(),
            duties: DutyLedger::new(),
            attrs: FileAttrTable::new(usize::MAX),
            diags: Vec::new(),
            actions: Vec::new(),
            code_tokens: 0,
            comment_tokens: 0,
            directive_tokens: 0,
            recovery: RecoveryStats::default(),
            attr_cursor: 0,
            root_span: EMPTY_SPAN,
        }
    }

    fn span_of(&self, i: usize) -> Span {
        match self.toks.get(i) {
            Some(t) => Span {
                start: t.start,
                end: t.end,
                line: t.line as u16,
                col: t.col as u16,
            },
            None => Span {
                start: self.src.len(),
                end: self.src.len(),
                line: 0,
                col: 0,
            },
        }
    }

    fn span_join(&self, a: Span, b: Span) -> Span {
        Span {
            start: a.start,
            end: if b.end > a.end { b.end } else { a.end },
            line: a.line,
            col: a.col,
        }
    }

    fn at_eof(&self, i: usize) -> bool {
        i >= self.toks.len()
    }

    /// 普查：背景记号计数 + 指令属性入表 + 挂接游标起点。
    fn census(&mut self) {
        let mut code_start = usize::MAX;
        let mut i = 0usize;
        while i < self.toks.len() {
            let t = match self.toks.get(i) {
                Some(t) => t,
                None => break,
            };
            match t.kind {
                TokenKind::Comment => self.comment_tokens += 1,
                TokenKind::Directive => {
                    self.directive_tokens += 1;
                    let sp = Span {
                        start: t.start,
                        end: t.end,
                        line: t.line as u16,
                        col: t.col as u16,
                    };
                    let parsed = parse_directive_attr(t.text(self.src));
                    let key = parsed.key;
                    let malformed = parsed.malformed;
                    self.attrs.push(key, parsed.value, sp);
                    match key {
                        AttrKey::PreproControl => {}
                        AttrKey::Unknown => {
                            self.duties.raise(DeferredDuty::AttrKeyLegality);
                            push_diag(
                                &mut self.diags,
                                TopDiagCode::UnknownAttrKey,
                                sp,
                                "指令键不在已登记属性键表内；已登记，合法性交属性专项裁定",
                            );
                        }
                        _ => {
                            if malformed {
                                self.duties.raise(DeferredDuty::AttrKeyLegality);
                                push_diag(
                                    &mut self.diags,
                                    TopDiagCode::AttrValueMalformed,
                                    sp,
                                    "属性键值形制应形如 key(value)；括号未成对或值为空",
                                );
                            }
                        }
                    }
                }
                TokenKind::Eof => {}
                _ => {
                    self.code_tokens += 1;
                    if code_start == usize::MAX {
                        code_start = t.start;
                    }
                }
            }
            i += 1;
        }
        self.attrs.set_code_start(code_start);
        // 挂接游标起点 = 首条代码记号之后的首个属性；此前的属性是文件级。
        let mut c = 0u32;
        while c < self.attrs.len() as u32 {
            let inside = match self.attrs.attr_at(c) {
                Some(a) => a.span.start >= code_start,
                None => break,
            };
            if inside {
                break;
            }
            c += 1;
        }
        self.attr_cursor = c;
    }

    /// k=1 前瞻：j 处是否已是声明起始或流尾（与 F0421 窗口口径一致）。
    ///
    /// **Eof 记号算流尾**：F0403 的词法收口会补一枚 `Eof`，若不把它当边界，
    /// 最后一个声明的右花括号会因为「下一个记号不是引导词」而收不了口，被误
    /// 报成区间未收口。这条是「收口规则」与「词法收口」的对齐点。
    fn at_decl_boundary(&self, j: usize) -> bool {
        if self.at_eof(j) {
            return true;
        }
        match self.toks.get(j) {
            Some(t) => t.kind == TokenKind::Eof || is_decl_lead(t, self.src),
            None => true,
        }
    }

    /// 声明区间扫描：从 `i` 起找区间末记号的**后一位**下标。
    ///
    /// 返回 `(end_index, closed, saw_init_at_top)`。收口规则：
    /// 1. 顶层深度的 `;` 收口；
    /// 2. 深度由正转零的 `}` 之后紧跟声明起始或流尾，也收口；
    /// 3. 走到流尾仍未收口 → `closed == false`，登记区间未收口。
    ///
    /// **保证游标单调推进**（判据「不挂起」的结构依据）。
    fn scan_extent(&self, mut i: usize) -> (usize, bool, bool) {
        let mut depth: i32 = 0;
        let mut saw_init = false;
        while i < self.toks.len() {
            let t = match self.toks.get(i) {
                Some(t) => t,
                None => break,
            };
            if t.kind == TokenKind::Punct {
                let txt = t.text(self.src);
                match txt {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => {
                        if depth > 0 {
                            depth -= 1;
                            if depth == 0 && txt == "}" && self.at_decl_boundary(i + 1) {
                                return (i + 1, true, saw_init);
                            }
                        } else {
                            // 顶层孤立闭括号：区间在此截断并登记（不吞记号）。
                            return (i + 1, false, saw_init);
                        }
                    }
                    ";" => {
                        if depth == 0 {
                            return (i + 1, true, saw_init);
                        }
                    }
                    "=" => {
                        if depth == 0 {
                            saw_init = true;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        (self.toks.len(), false, saw_init)
    }

    /// 登记声明体顶层深度上的标识符引用（嵌套体内的名字归 F0424-F0426）。
    fn register_uses(&mut self, from: usize, to: usize, seq: u32) {
        let mut d: i32 = 0;
        let mut k = from;
        while k < to {
            let tk = match self.toks.get(k) {
                Some(x) => x,
                None => break,
            };
            if tk.kind == TokenKind::Punct {
                match tk.text(self.src) {
                    "{" | "(" | "[" => d += 1,
                    "}" | ")" | "]" => {
                        if d > 0 {
                            d -= 1;
                        }
                    }
                    _ => {}
                }
            } else if d == 0 && tk.kind == TokenKind::Ident {
                let text = tk.text(self.src);
                if !is_decl_lead_word(text) {
                    let sp = self.span_of(k);
                    if let Some(id) = self.names.intern(text) {
                        self.deps.note_use(id, sp, seq);
                    }
                }
            }
            k += 1;
        }
    }

    /// 解析一条顶层声明；返回区间末下标（后一位）。
    fn parse_one_decl(&mut self, start: usize) -> usize {
        let head_span = self.span_of(start);
        let mut i = start;
        let mut modifiers = 0u32;

        // —— 前置修饰（layout(...) 可叠加）——
        loop {
            if self.at_eof(i) {
                break;
            }
            let t = match self.toks.get(i) {
                Some(t) => t,
                None => break,
            };
            if t.kind != TokenKind::Ident || !is_modifier_word(t.text(self.src)) {
                break;
            }
            modifiers += 1;
            if modifiers > 1 {
                self.duties.raise(DeferredDuty::LayoutCombination);
            }
            // 吃掉紧随的括号组。
            let mut j = i + 1;
            while j < self.toks.len() {
                let tj = match self.toks.get(j) {
                    Some(x) => x,
                    None => break,
                };
                if tj.kind == TokenKind::Punct && tj.text(self.src) == "(" {
                    let mut d = 0i32;
                    while j < self.toks.len() {
                        let tk = match self.toks.get(j) {
                            Some(x) => x,
                            None => break,
                        };
                        if tk.kind == TokenKind::Punct {
                            match tk.text(self.src) {
                                "(" => d += 1,
                                ")" => {
                                    d -= 1;
                                    if d == 0 {
                                        j += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        j += 1;
                    }
                    break;
                }
                j += 1;
            }
            i = j;
        }

        // —— 类引导词 ——
        if self.at_eof(i) {
            return i;
        }
        let lead_word = match self.toks.get(i) {
            Some(t) => t.text(self.src),
            None => return i,
        };
        let class = match lead_of(lead_word) {
            Some(l) if l.role == LeadRole::ClassLead => l.class,
            _ => {
                // 只有修饰词而无类引导词：区间到流尾，登记未收口。
                let sp = self.span_of(i);
                let mut d = String::new();
                d.push_str("修饰词 ");
                d.push_str(lead_word);
                d.push_str(" 之后没有四类声明引导词");
                push_diag(&mut self.diags, TopDiagCode::ExtentUnclosed, sp, &d);
                self.duties.raise(DeferredDuty::ExtentUnclosedRecovery);
                return self.toks.len();
            }
        };
        i += 1;

        // —— 名字 ——
        let mut name_present = false;
        let mut name = u32::MAX;
        let mut name_span = EMPTY_SPAN;
        let seq = self.seq.len() as u32;
        if !self.at_eof(i) {
            let is_ident = match self.toks.get(i) {
                Some(t) => t.kind == TokenKind::Ident,
                None => false,
            };
            if is_ident {
                let text = match self.toks.get(i) {
                    Some(t) => t.text(self.src),
                    None => "",
                };
                name_span = self.span_of(i);
                name_present = true;
                match self.names.intern(text) {
                    Some(id) => {
                        name = id;
                        // 声明间依赖登记：只登记，不解析。
                        let first = self.deps.note_define(id, name_span, class, seq);
                        if !first {
                            self.duties.raise(DeferredDuty::DuplicateDefinition);
                            push_diag(
                                &mut self.diags,
                                TopDiagCode::DuplicateDeclared,
                                name_span,
                                "同名顶层声明；语法期不裁决，登记交语义期按规范裁定",
                            );
                        }
                        if vec04_keywords::lookup(text).is_some() {
                            self.duties.raise(DeferredDuty::ReservedName);
                        }
                    }
                    None => {
                        let code = if self.names.len() >= NAME_CAP {
                            TopDiagCode::NameTableFull
                        } else {
                            TopDiagCode::NameNotInterned
                        };
                        push_diag(
                            &mut self.diags,
                            code,
                            name_span,
                            "名字未能驻留；该声明依赖边将退化为按序号登记",
                        );
                    }
                }
                i += 1;
            }
        }
        if !name_present {
            self.duties.raise(DeferredDuty::MissingDeclName);
            push_diag(
                &mut self.diags,
                TopDiagCode::MissingDeclName,
                head_span,
                "该顶层声明无可识别名字（匿名）；合法性交语义期裁定",
            );
        }

        // —— 区间 ——
        let (end, closed, saw_init) = self.scan_extent(i);
        if !closed {
            let sp = self.span_of(if end > start { end - 1 } else { start });
            push_diag(
                &mut self.diags,
                TopDiagCode::ExtentUnclosed,
                sp,
                "声明区间走到记号流尾仍未以顶层分号收口",
            );
            self.duties.raise(DeferredDuty::ExtentUnclosedRecovery);
        }
        let last_span = self.span_of(if end > start { end - 1 } else { start });
        let extent = self.span_join(head_span, last_span);
        let has_init = match class {
            DeclClass::Variable => saw_init,
            _ => false,
        };

        // —— 依赖边：声明体顶层深度上的标识符按「引用」登记 ——
        self.register_uses(i, end, seq);

        // —— 节点与动作 ——
        let node_kind = class.registered_node_kind(has_init);
        let mut node = NO_NODE;
        let mut node_valid = false;
        let mut prod = u16::MAX;
        match node_kind {
            Some(k) => {
                let built = AstNode::with_span(k, extent, self.version);
                match self.arena.alloc(built) {
                    Ok(id) => {
                        node = id;
                        node_valid = true;
                        prod = prod_for_kind(k);
                    }
                    Err(_) => {
                        push_diag(
                            &mut self.diags,
                            TopDiagCode::ArenaRejected,
                            extent,
                            "arena 拒绝本次节点分配；声明已登记但未落池",
                        );
                    }
                }
            }
            None => {
                self.duties.raise(DeferredDuty::NodeKindUnregistered);
            }
        }
        if prod != u16::MAX {
            self.actions.push(Reduction {
                prod,
                nt: super::vec21_parser::nt::DECL,
                span: extent,
            });
        }

        // —— 属性挂接（O(1) 单调游标）——
        self.attrs.open_decl();
        let decl_index = self.seq.len();
        let ordinal = decl_index as u32;
        let mut cur = self.attr_cursor;
        let (head, taken) = self.attrs.claim_forward(decl_index, &mut cur, extent.end);
        self.attr_cursor = cur;

        self.seq.push(TopDecl {
            class,
            ordinal,
            name,
            name_present,
            name_span,
            head_span,
            extent,
            token_lo: start as u32,
            token_hi: end as u32,
            has_init,
            modifiers,
            extent_closed: closed,
            node,
            node_valid,
            attr_head: head,
            attr_count: taken,
        });
        end
    }

    /// 顶层循环 + 恢复。
    fn run(&mut self) {
        let n = self.toks.len();
        let mut i = 0usize;
        while i < n {
            let t = match self.toks.get(i) {
                Some(t) => t,
                None => break,
            };
            // 背景记号：注释与指令在普查期已处理，顶层循环只跳过。
            if t.kind == TokenKind::Comment || t.kind == TokenKind::Directive {
                i += 1;
                continue;
            }
            if t.kind == TokenKind::Eof {
                i += 1;
                continue;
            }
            if is_decl_lead(t, self.src) {
                let next = self.parse_one_decl(i);
                // 强制推进：解析必须永远返回。
                i = if next > i { next } else { i + 1 };
                continue;
            }
            // —— 非法顶层记号 → 恢复到下一声明 ——
            let sp = self.span_of(i);
            let mut d = String::new();
            d.push_str("顶层位置出现 ");
            d.push_str(t.text(self.src));
            d.push_str("，不属于四类声明起始");
            push_diag(&mut self.diags, TopDiagCode::IllegalTopToken, sp, &d);
            self.recovery.recoveries += 1;
            let mut j = i;
            let mut landed = false;
            while j < n {
                let tj = match self.toks.get(j) {
                    Some(x) => x,
                    None => break,
                };
                if tj.kind == TokenKind::Comment || tj.kind == TokenKind::Directive {
                    j += 1;
                    continue;
                }
                if is_decl_lead(tj, self.src) {
                    landed = true;
                    break;
                }
                j += 1;
            }
            if !landed {
                j = n;
            }
            let (target, forced) = recovery_advance(i, j);
            if forced {
                self.recovery.forced += 1;
                push_diag(
                    &mut self.diags,
                    TopDiagCode::RecoveryForcedAdvance,
                    sp,
                    "恢复未找到前进位置，已强制前进一个记号以保终止",
                );
            } else if landed {
                self.recovery.landed += 1;
            }
            self.recovery.skipped += (target - i) as u32;
            i = target;
        }
    }

    /// 收尾：依赖配对、节点类型提案、语义裁定、根节点、两路摘要。
    fn finish(mut self) -> TranslationUnit {
        self.deps.finalize();

        let present = self.seq.present_classes();
        let pending = pending_node_kinds(self.version, &present);
        if !pending.is_empty() {
            // 逐条声明已 raise 过；此处保证至少一次，使「有提案必移交」恒成立。
            self.duties.raise(DeferredDuty::NodeKindUnregistered);
        }

        // —— 语义裁定（显性五态）——
        let verdict = if !self.seq.is_empty() {
            UnitVerdict::Declared
        } else if self.code_tokens > 0 {
            UnitVerdict::RecoveredToEmpty
        } else if self.comment_tokens > 0 {
            UnitVerdict::CommentOnly
        } else if self.directive_tokens > 0 {
            UnitVerdict::DirectiveOnly
        } else {
            UnitVerdict::Empty
        };
        let notice_span = self.span_of(0);
        match verdict {
            UnitVerdict::Declared => {}
            UnitVerdict::Empty => push_diag(
                &mut self.diags,
                TopDiagCode::EmptyUnit,
                notice_span,
                "翻译单元无声明、无代码、无注释、无指令；按规范为空单元，语义显性告知",
            ),
            UnitVerdict::CommentOnly => push_diag(
                &mut self.diags,
                TopDiagCode::CommentOnlyUnit,
                notice_span,
                "翻译单元仅含注释；按规范为纯注释文件，语义显性告知",
            ),
            UnitVerdict::DirectiveOnly => push_diag(
                &mut self.diags,
                TopDiagCode::DirectiveOnlyUnit,
                notice_span,
                "翻译单元仅含预处理指令；指令语义归条件编译与包含两单",
            ),
            UnitVerdict::RecoveredToEmpty => push_diag(
                &mut self.diags,
                TopDiagCode::RecoveredToEmpty,
                notice_span,
                "有代码记号但顶层全程恢复到流尾，未解析出任何声明",
            ),
        }

        let accepted = self.error_count_of() == 0;

        // —— 结构摘要（吃类别 / 修饰 / 判决 / 移交 / 诊断码，不吃标识符）——
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = mix(h, verdict as u64);
        h = mix(h, self.seq.len() as u64);
        for d in self.seq.all().iter() {
            h = mix(h, d.class.code() as u64);
            h = mix(h, d.modifiers as u64);
            h = mix(h, d.has_init as u64);
            h = mix(h, d.name_present as u64);
            h = mix(h, d.extent_closed as u64);
        }
        h = mix(h, self.duties.total() as u64);
        let mut di = 0usize;
        while di < self.diags.len() {
            h = mix(h, self.diags[di].code as u64);
            di += 1;
        }
        let struct_digest = h;
        let syntax_digest = term_digest(self.src, self.toks);

        // —— 根节点（最后落池，使 arena 里声明在前、根在后，符合「先子后父」
        //    的 parent 挂接方向由下游按需补写）——
        let root_span = self.root_span;
        let root_node = match self
            .arena
            .alloc(AstNode::with_span(
                NodeKind::TranslationUnit,
                root_span,
                self.version,
            ))
        {
            Ok(id) => {
                self.root_valid = true;
                id
            }
            Err(_) => {
                push_diag(
                    &mut self.diags,
                    TopDiagCode::ArenaRejected,
                    root_span,
                    "arena 拒绝翻译单元根节点；本单元以无根态交付",
                );
                NO_NODE
            }
        };

        TranslationUnit {
            root: root_node,
            root_valid: self.root_valid,
            root_span,
            version: self.version,
            seq: self.seq,
            deps: self.deps,
            duties: self.duties,
            attrs: self.attrs,
            pending,
            diags: self.diags,
            syntax_digest,
            struct_digest,
            verdict,
            accepted,
            code_tokens: self.code_tokens,
            comment_tokens: self.comment_tokens,
            directive_tokens: self.directive_tokens,
            recovery: self.recovery,
            actions: self.actions,
        }
    }

    fn error_count_of(&self) -> u32 {
        let mut n = 0u32;
        for d in self.diags.iter() {
            if d.severity() == Severity::Error {
                n += 1;
            }
        }
        n
    }
}

/// 记一条诊断（施工期便捷函数，避免每个分支写三行）。
fn push_diag(v: &mut Vec<TopDiag>, code: TopDiagCode, span: Span, detail: &str) {
    v.push(TopDiag::new(code, span, detail));
}

/// 主入口：解析一个翻译单元的顶层。
///
/// 签名刻意只收 `(&str, &[Token], u16)`：本单**不做二次词法**（F0403 是唯一记
/// 号来源），也**不读文件**（IO 不进语法层）。
pub fn parse_unit(src: &str, toks: &[Token], version: u16) -> TranslationUnit {
    let mut b = UnitBuilder::new(src, toks, version);
    // 根跨度 = 首个记号起点 → 末记号终点；空流用 (0,0) 显式表达而不是猜。
    let root = match (toks.first(), toks.last()) {
        (Some(f), Some(l)) => Span {
            start: f.start,
            end: l.end,
            line: f.line as u16,
            col: f.col as u16,
        },
        _ => Span {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        },
    };
    b.root_span = root;
    b.census();
    b.run();
    b.finish()
}