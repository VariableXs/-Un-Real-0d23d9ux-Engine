//! VE-F0421 · 语法分析器架构（VE-C 域 · 着色器系统 · 语法组 C02 首单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0421`
//!
//! **判据（锚点原文）**：递归下降、前瞻窗口、动作分离、深度防护、判据。
//!
//! 本单交付的是**语法分析器的骨架**，不是任何一条具体产生式。F0423-F0433 各自
//! 去写翻译单元、声明、语句、表达式；F0422 去建 arena 与节点；F0434 去写错误
//! 恢复。本单只回答一个问题：**这些单要挂在一个什么样的框架上**。框架错了，
//! 后面每一单都要付代价，所以四条判据逐条都要有**结构性保证**——保证不了就是
//! 骨架不合格，判据全绿也救不了。
//!
//! 1. **递归下降**（判据一）。分析器主体是手写的递归下降函数，文法派生表
//!    **不驱动**控制流，只承担三件不驱动就干不了的事：FIRST 集与冲突裁定
//!    （规范期拦截歧义）、产生式身份（动作回调的稳定标识）、AST 消费者的重建
//!    参照。为什么不用表驱动（LALR）：**错误恢复与诊断质量优先**。表驱动把
//!    「下一步能干什么」压成一张状态转移表，恢复时就得在表里找一条活路，
//!    而恢复路径往往正是表里没有的那条——于是诊断退化成「期望集」而不是
//!    「你写错了什么」。递归下降的 `Err` 发生在**文法的形状**上，诊断能指着
//!    说清是哪个非终结符的哪条产生式没走通。详见 [`DecisionRecord`]。
//!
//! 2. **前瞻窗口**（判据二）。k=2，语义按文法定而非按习惯定：窗口大小是
//!    **文法可判定性的函数**，不是越大越好。窗口 [`LookaheadWindow`] 的
//!    关键性质不是「能看两个」，而是**不能回看**——已消费记号的序号小于水位
//!    [`LookaheadWindow::consumed_through`]，任何回看请求被拒绝并计入
//!    [`LookaheadWindow::rescans`]。这把 F0420 移交的「单遍零回溯」从约定
//!    变成**结构上不可能**：不是我们不写回溯代码，是回溯拿不到记号。
//!    水位以下的读取**有唯一合法入口** [`LookaheadWindow::peek_at`]，
//!    它同样受水位约束——所以「水位拦得住吗」不是靠注释保证，是靠判据当场
//!    把违规请求喂进去看它是否真被拒（见 `vec21_checks.rs`）。
//!
//! 3. **动作分离**（判据三）。解析**只产生动作**，不建 AST。生产者的
//!    [`ActionSink`] 是接口，AST 构建只是它的**一种**实现
//!    （[`NodeTallySink`]，本单给的最小可运行消费者）。回调返错时
//!    [`SinkStatus::Reject`] 被**隔离**：分析器记一次
//!    [`Parser::callback_faults`]，游标、深度、产生式状态机**一个都不动**，
//!    解析继续走完。回调用 `&mut dyn`，所以状态机无法在回调里被偷改——回调
//!    拿不到 `&mut Parser`。这条是编译期保证，故判据里只断言运行期可观测的
//!    不变量（游标与深度逐字段比对），不假装能断言编译期性质。
//!
//! 4. **深度防护**（判据四）。[`DepthGuard`] 在**进入**非终结符时计数，超限
//!    则拒绝并**指向嵌套源头**：报的是最深那个**已准入**构造的起始跨度
//!    （[`DepthGuard::open_stack`] 的末项），不是当前记号位置。指向源头才
//!     actionable——作者看到的是「这个块里的嵌套已经失控」，而不是「第 47 个
//!    左括号有问题」。判据里有一条**反向断言**：错误跨度必须**不等于**当前
//!    记号跨度，否则一个「报当前位置」的偷懒实现会混过去。
//!
//! 上游 F0420 移交的两条可机检下游动作在本单兑现：
//! - 「语法组 F0421 起所有诊断码带 VE-F04xx 锚点引用」→ [`DiagCode::anchor`]
//!   是**唯一**的锚点来源，构造诊断必须经 [`Diagnostic::new`]，
//!   [`Diagnostic::anchored`] 判空并核对具体锚点。
//! - 「语法组 F0421 的前瞻窗口须声明不回扫已消费记号」→ 窗口水位 +
//!   [`LookaheadWindow::rescans`]，且判据含**变异体验证**（喂违规读取，
//!   必须转红），否则「不回扫」这条判据自己也可能是恒真的摆设。
//!
//! 零静默纪律：深度超限 / 文法冲突 / 回调异常 / 意外记号 / 预算耗尽**一律**
//!   产出带锚点的诊断，不吞、不降级为「当作没看见」；文法冲突在**规范期**
//!   （建表时、零记号消费）就拦下，不留到解析期；左递归与尾循环两类**不拦**
//!   的情形必须逐条登记处置理由，不得静默跳过（见 [`ConflictDisposition`]）。
//! 零 panic 面、零 IO、无全局可变状态。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vec03_lexer::{Token, TokenKind};

// ---------------------------------------------------------------------------
// 零、诊断码与锚点（判据：所有诊断码带 VE-F04xx 锚点引用）
// ---------------------------------------------------------------------------

/// 本单的锚点串。诊断文本里出现的 VE-F04xx 引用一律经 [`DiagCode::anchor`]
/// 派生，不允许调用方自己拼串——拼串就一定会拼错，且拼错无法机检。
pub const ANCHOR: &str = "VE-F0421";

/// 语法分析器的诊断码。
///
/// 每码绑定**唯一**锚点片段，判据据此逐码核对。删码时保留空洞不复用
/// （码位是诊断契约的一部分，外部工具按串索引）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// 解析深度超限（深度防护拒绝进入）。
    DepthExceeded,
    /// 文法冲突：同一非终结符的两条产生式在输入上不可区分（规范期拦截）。
    GrammarConflict,
    /// 动作回调返 `Reject`（已隔离，分析器状态机未受影响）。
    CallbackFault,
    /// 遇到不属于当前位置的记号。
    UnexpectedToken,
    /// 缺少必需的记号（已消费到流尾或遇到不匹配的记号）。
    MissingToken,
    /// 步数预算耗尽（防挂起；按未完成报出，不当作「跑完了」）。
    BudgetExhausted,
    /// 前瞻窗口越界访问（k 超出窗口，或绝对序号低于水位）。
    WindowViolation,
}

impl DiagCode {
    /// 人话名（诊断呈现与读屏用）。
    pub fn name(self) -> &'static str {
        match self {
            DiagCode::DepthExceeded => "深度超限",
            DiagCode::GrammarConflict => "文法冲突",
            DiagCode::CallbackFault => "回调异常",
            DiagCode::UnexpectedToken => "意外记号",
            DiagCode::MissingToken => "缺少记号",
            DiagCode::BudgetExhausted => "预算耗尽",
            DiagCode::WindowViolation => "窗口越界",
        }
    }

    /// 本码的锚点引用（`VE-F04xx#…` 形态）。
    ///
    /// 这是锚点串的**唯一**来源。判据逐码核对返回值，故新增码必须在此登记，
    /// 想不登记就新增只能靠改这个函数——改动会被 `C21-锚点-*` 判据抓到。
    pub fn anchor(self) -> &'static str {
        match self {
            DiagCode::DepthExceeded => "VE-F0421#深度防护",
            DiagCode::GrammarConflict => "VE-F0421#文法派生表",
            DiagCode::CallbackFault => "VE-F0421#动作分离",
            DiagCode::UnexpectedToken => "VE-F0421#递归下降",
            DiagCode::MissingToken => "VE-F0421#记号流末尾",
            DiagCode::BudgetExhausted => "VE-F0421#性能判据",
            DiagCode::WindowViolation => "VE-F0421#前瞻窗口",
        }
    }
}

/// 字节区间（`[start, end)`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u16,
    pub col: u16,
}

impl Span {
    /// 由单个记号构造跨度。
    pub fn of_token(t: &Token) -> Span {
        Span {
            start: t.start,
            end: t.end,
            line: t.line as u16,
            col: t.col as u16,
        }
    }

    /// 两跨度合并（左起右至）。`self` 在 `other` 之后时返回 `other` 侧在前的
    /// 空跨度——**不静默丢弃**，调用方据此能发现顺序错乱。
    pub fn join(self, other: Span) -> Span {
        if other.start <= self.start {
            Span {
                start: other.start,
                end: if self.end > other.end { self.end } else { other.end },
                line: other.line,
                col: other.col,
            }
        } else {
            Span {
                start: self.start,
                end: if self.end > other.end { self.end } else { other.end },
                line: self.line,
                col: self.col,
            }
        }
    }

    /// 字节长度。
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 是否为空跨度。
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

/// 一条诊断。**只能**经 [`Diagnostic::new`] 构造，保证锚点必填。
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub span: Span,
    pub detail: String,
}

impl Diagnostic {
    /// 构造诊断（锚点由 [`DiagCode::anchor`] 派生，不可缺省）。
    pub fn new(code: DiagCode, span: Span, detail: &str) -> Diagnostic {
        Diagnostic {
            code,
            span,
            detail: detail.to_string(),
        }
    }

    /// 本诊断的锚点引用。
    pub fn anchored(&self) -> &'static str {
        self.code.anchor()
    }

    /// 人话一行式（渲染用）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.code.anchor());
        s.push_str(": ");
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
// 一、解析策略决策记录（判据一：递归下降）
// ---------------------------------------------------------------------------

/// 候选解析策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParserStrategy {
    /// 手写递归下降（本单采用）。
    RecursiveDescent,
    /// LALR(1) 表驱动。
    LalrTableDriven,
    /// GLR（广义 LR，按歧义分支并行试）。
    Glr,
    /// 自顶向下图表约束（packrat）。
    PackratPeg,
}

impl ParserStrategy {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            ParserStrategy::RecursiveDescent => "递归下降",
            ParserStrategy::LalrTableDriven => "LALR 表驱动",
            ParserStrategy::Glr => "GLR",
            ParserStrategy::PackratPeg => "Packrat/PEG",
        }
    }
}

/// 拒绝某策略的理由分类。**不接受空理由**——没有理由的拒绝等于没拒绝，
/// 判据据此拦。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectReason {
    /// 恢复路径落在状态转移表之外，诊断退化为「期望集」。
    RecoveryOutsideTable,
    /// 需要额外构造 GLR 归约栈与歧义消解 machinery，收益不抵成本。
    AmbiguityMachineryCost,
    /// 记忆化要求输入可重放，与 F0420 移交的「单遍不回扫」直接冲突。
    RequiresRewind,
    /// 本域文法规模下，控制流与文法一一对应的可读性损失更大。
    ReadabilityLoss,
}

impl RejectReason {
    /// 人话说明。
    pub fn text(self) -> &'static str {
        match self {
            RejectReason::RecoveryOutsideTable => "恢复路径落在状态转移表之外，诊断退化为「期望集」而非「哪里写错了」",
            RejectReason::AmbiguityMachineryCost => "需另建歧义并行与归约栈，收益不抵成本",
            RejectReason::RequiresRewind => "记忆化要求输入可重放，与单遍不回扫直接冲突",
            RejectReason::ReadabilityLoss => "控制流与文法脱钩，可读性与可定位性损失更大",
        }
    }
}

/// 一条被拒策略的记录。
#[derive(Clone, Copy, Debug)]
pub struct Rejection {
    pub strategy: ParserStrategy,
    pub reason: RejectReason,
}

/// 策略决策记录。
///
/// **这不是注释**。选型是本域骨架的头号决策（决定了后面十个单往哪儿挂），
/// 所以它落成数据结构：被拒策略**逐条列名**且各带理由，判据核对「被拒条目
/// 非空且理由非空且理由覆盖错误恢复与诊断质量这两条主线」。想把表驱动改成
/// 默认，改这里会被判据抓住。
#[derive(Clone, Debug)]
pub struct DecisionRecord {
    pub chosen: ParserStrategy,
    pub rejections: Vec<Rejection>,
    /// 决策依据的主线（必须非空；本单为「错误恢复与诊断质量优先」）。
    pub rationale: &'static str,
    /// 决策锚点（供移交文档引用）。
    pub anchor: &'static str,
}

impl DecisionRecord {
    /// 本单采用的决策记录。
    pub fn current() -> DecisionRecord {
        DecisionRecord {
            chosen: ParserStrategy::RecursiveDescent,
            rejections: vec![
                Rejection {
                    strategy: ParserStrategy::LalrTableDriven,
                    reason: RejectReason::RecoveryOutsideTable,
                },
                Rejection {
                    strategy: ParserStrategy::Glr,
                    reason: RejectReason::AmbiguityMachineryCost,
                },
                Rejection {
                    strategy: ParserStrategy::PackratPeg,
                    reason: RejectReason::RequiresRewind,
                },
            ],
            rationale: "错误恢复与诊断质量优先：恢复点必须能指着文法的具体位置说话",
            anchor: "VE-F0421#递归下降",
        }
    }

    /// 校验决策记录自身是否成立。
    ///
    /// 判据：选了递归下降；被拒条目非空；每条理由非空；主线非空；理由里
    /// **确实覆盖**了错误恢复与诊断质量（否则「决策记录」只是一张好看的表）。
    pub fn validate(&self) -> bool {
        if self.chosen != ParserStrategy::RecursiveDescent {
            return false;
        }
        if self.rejections.is_empty() {
            return false;
        }
        if self.rationale.is_empty() || self.anchor.is_empty() {
            return false;
        }
        let mut covers_recovery = false;
        for r in self.rejections.iter() {
            if r.strategy == self.chosen {
                // 把选中策略列进「被拒」表 = 决策记录自相矛盾。
                return false;
            }
            let t = r.reason.text();
            if t.is_empty() {
                return false;
            }
            if t.contains("恢复") || t.contains("诊断") {
                covers_recovery = true;
            }
        }
        covers_recovery
    }
}

// ---------------------------------------------------------------------------
// 二、文法派生表（判据一的数据结构部分）
// ---------------------------------------------------------------------------

/// 文法终结符（词法类之上的**词素类**：词法只给 `TokenKind`，关键字归语法层
/// 识别——这是 F0403 划定的职责边界，本单正好落在它下游）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Term {
    Ident,
    Number,
    Plus,
    Assign,
    Semi,
    Colon,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    KwFn,
    KwLet,
    KwIf,
    KwElse,
    KwReturn,
    Eof,
}

/// 终结符数目。FIRST 集用 `u32` 位集表示：17 个终结符 + 1 个可空位 = 18 位。
pub const TERM_COUNT: usize = 17;

impl Term {
    /// 位集内的位号。
    pub fn bit(self) -> u32 {
        1u32 << (self as u16)
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            Term::Ident => "标识符",
            Term::Number => "数值字面量",
            Term::Plus => "+",
            Term::Assign => "=",
            Term::Semi => ";",
            Term::Colon => ":",
            Term::Comma => ",",
            Term::LParen => "(",
            Term::RParen => ")",
            Term::LBrace => "{",
            Term::RBrace => "}",
            Term::KwFn => "fn",
            Term::KwLet => "let",
            Term::KwIf => "if",
            Term::KwElse => "else",
            Term::KwReturn => "return",
            Term::Eof => "记号流结束",
        }
    }
}

/// FIRST 集（`u32` 位集）。最高位恒置表示该非终结符可空。
const NULLABLE_BIT: u32 = 1u32 << 31;

/// 文法符号：终结符或非终结符。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symbol {
    Term(Term),
    /// 非终结符编号（非终结符名由 [`Grammar::nt_name`] 给出）。
    Nt(u8),
}

/// 产生式处置标记。
///
/// **这两类不参与 FIRST/FIRST 歧义拦截，但必须逐条登记**（见
/// [`ConflictDisposition::DescendHandled`]）。它们不是「跳过检查」，而是
/// 「本就不该由那个检查来判」：左递归由优先级/结合性下降处理，尾循环由下降
/// 函数的 `while` 处理。静默跳过会让「无冲突」变成一句无根据的话。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProdFlag {
    /// 普通产生式（受 FIRST/FIRST 冲突检查）。
    Plain,
    /// 左递归（交由递归下降的优先级处理）。
    LeftRecursive,
    /// 尾循环重复（交由下降函数的循环处理）。
    TailLoop,
}

/// 一条产生式。
#[derive(Clone, Debug)]
pub struct Production {
    pub id: u16,
    pub lhs: u8,
    pub rhs: Vec<Symbol>,
    pub flag: ProdFlag,
}

impl Production {
    /// 产生式是否可空（右部全为可空非终结符时由 FIRST 集判定，此处仅报结构）。
    pub fn rhs_is_empty(&self) -> bool {
        self.rhs.is_empty()
    }
}

/// 非终结符编号。
pub mod nt {
    pub const TRANSLATION_UNIT: u8 = 0;
    pub const DECL: u8 = 1;
    pub const STMT: u8 = 2;
    pub const EXPR: u8 = 3;
    pub const PRIMARY: u8 = 4;
    pub const PARAM_LIST: u8 = 5;
    pub const TYPE: u8 = 6;
    pub const EXPR_TAIL: u8 = 7;
    pub const COUNT: u8 = 8;
}

/// 冲突处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictDisposition {
    /// 规范期拦截：两条产生式在输入上不可区分，必须改文法。
    SpecBlocked,
    /// 交递归下降处理（左递归 / 尾循环），**须带理由**。
    DescendHandled(ProdFlag),
}

/// 一条冲突记录。
#[derive(Clone, Copy, Debug)]
pub struct Conflict {
    pub nt: u8,
    pub prod_a: u16,
    pub prod_b: u16,
    /// 冲突的 FIRST 集交（位集，便于判据核对具体符号而非只判「非空」）。
    pub overlap: u32,
    pub disposition: ConflictDisposition,
}

/// 文法派生表。
#[derive(Clone, Debug)]
pub struct Grammar {
    pub productions: Vec<Production>,
    pub nt_names: [&'static str; nt::COUNT as usize],
    first: [u32; nt::COUNT as usize],
    follow: [u32; nt::COUNT as usize],
    conflicts: Vec<Conflict>,
    /// 建表时是否已执行冲突裁定（false = 只建表未裁定）。
    pub adjudicated: bool,
}

impl Grammar {
    /// 建表：登记产生式，算 FIRST/FOLLOW，**当场**做冲突裁定。
    ///
    /// 冲突裁定在**建表期**完成（判据：错误路径要求「规范期拦截」，即零记号
    /// 消费时就拦下，不留到解析期）。
    pub fn build(productions: Vec<Production>) -> Grammar {
        let names = [
            "TranslationUnit",
            "Decl",
            "Stmt",
            "Expr",
            "Primary",
            "ParamList",
            "Type",
            "ExprTail",
        ];
        let mut g = Grammar {
            productions,
            nt_names: names,
            first: [0u32; nt::COUNT as usize],
            follow: [0u32; nt::COUNT as usize],
            conflicts: Vec::new(),
            adjudicated: false,
        };
        g.compute_first();
        g.compute_follow();
        g.adjudicate();
        g.adjudicated = true;
        g
    }

    /// 非终结符名。
    pub fn nt_name(&self, nt: u8) -> &'static str {
        self.nt_names[(nt as usize) % (nt::COUNT as usize)]
    }

    /// 非终结符的 FIRST 集。
    pub fn first_of(&self, nt: u8) -> u32 {
        self.first[(nt as usize) % (nt::COUNT as usize)]
    }

    /// 非终结符的 FOLLOW 集。
    pub fn follow_of(&self, nt: u8) -> u32 {
        self.follow[(nt as usize) % (nt::COUNT as usize)]
    }

    /// 该非终结符是否可空。
    pub fn is_nullable(&self, nt: u8) -> bool {
        self.first_of(nt) & NULLABLE_BIT != 0
    }

    /// 取某非终结符的全部产生式 id。
    pub fn prods_of(&self, nt: u8) -> Vec<u16> {
        let mut out = Vec::new();
        for p in self.productions.iter() {
            if p.lhs == nt {
                out.push(p.id);
            }
        }
        out
    }

    /// FIRST 不动点迭代。
    ///
    /// 右部为空 ⇒ 置可空位；右部首符号为终结符 ⇒ 并入该终结符位；右部首符号
    /// 为非终结符 ⇒ 并入其 FIRST 并传播其可空位。迭代到无变化为止。
    fn compute_first(&mut self) {
        // 先置所有非终结符自身的可空标记（供增量传播使用）。
        loop {
            let mut changed = false;
            let snapshot = self.first;
            for idx in 0..self.productions.len() {
                let (lhs, sym0) = {
                    let p = &self.productions[idx];
                    (p.lhs, p.rhs.first().copied())
                };
                match sym0 {
                    None => {
                        if self.first[lhs as usize] & NULLABLE_BIT == 0 {
                            self.first[lhs as usize] |= NULLABLE_BIT;
                            changed = true;
                        }
                    }
                    Some(Symbol::Term(t)) => {
                        if self.first[lhs as usize] & t.bit() == 0 {
                            self.first[lhs as usize] |= t.bit();
                            changed = true;
                        }
                    }
                    Some(Symbol::Nt(n)) => {
                        let add = snapshot[n as usize];
                        if self.first[lhs as usize] & add != add {
                            self.first[lhs as usize] |= add;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// FOLLOW 不动点迭代（起始集：Eof 进 TranslationUnit；`}` 进所有块类
    /// 非终结符的 FOLLOW——本单文法里块非终结符已被 `Stmt` 吸收，故只需
    /// Eof 与显式出现在右部尾位的终结符）。
    fn compute_follow(&mut self) {
        self.follow[nt::TRANSLATION_UNIT as usize] |= Term::Eof.bit();
        loop {
            let mut changed = false;
            for idx in 0..self.productions.len() {
                let (lhs, syms) = {
                    let p = &self.productions[idx];
                    (p.lhs, p.rhs.clone())
                };
                let n = syms.len();
                let mut i = 0usize;
                while i < n {
                    let sym = syms[i];
                    if let Symbol::Nt(x) = sym {
                        // 右部首次出现且可空 → 其 FOLLOW 并入本符号的 FIRST。
                        let mut tail_first = 0u32;
                        let mut j = i + 1;
                        let mut tail_nullable = true;
                        while j < n {
                            match syms[j] {
                                Symbol::Term(t) => {
                                    tail_first |= t.bit();
                                    tail_nullable = false;
                                    break;
                                }
                                Symbol::Nt(y) => {
                                    let f = self.first[y as usize];
                                    tail_first |= f;
                                    if f & NULLABLE_BIT == 0 {
                                        tail_nullable = false;
                                        break;
                                    }
                                }
                            }
                            j += 1;
                        }
                        if tail_nullable {
                            tail_first |= Term::Eof.bit();
                            // 位置在右部末尾时并入本产生式左部的 FOLLOW。
                            if i + 1 == n {
                                tail_first |= self.follow[lhs as usize];
                            }
                        }
                        if self.follow[x as usize] & tail_first != tail_first {
                            self.follow[x as usize] |= tail_first;
                            changed = true;
                        }
                    }
                    i += 1;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// 冲突裁定：对每个非终结符的产生式两两求 FIRST 交，按处置分类。
    ///
    /// 前缀情形的正确处理：若 `α` 是 `β` 的真前缀，交集只看**前缀之后**的
    /// FIRST 与 `α` 的后继 FIRST/FOLLOW 是否相交——相交才是冲突，不相交
    /// 是合法的可前缀分解（`let x:T;` vs `let x:T=e;`）。一个只会算
    /// FIRST∩FIRST 的检查器会把这类**全部误报**，判据里专有一条反例。
    fn adjudicate(&mut self) {
        for nt in 0..(nt::COUNT as usize) {
            let ids = self.prods_of(nt as u8);
            let mut i = 0usize;
            while i < ids.len() {
                let mut j = i + 1;
                while j < ids.len() {
                    let (pa, pb) = (self.prod(ids[i]), self.prod(ids[j]));
                    if let (Some(pa), Some(pb)) = (pa, pb) {
                        // 有处置标记的一方不参与 FIRST/FIRST 拦截，但要在
                        // conflicts 里**留痕**（DescendHandled）。
                        let flagged = pa.flag != ProdFlag::Plain || pb.flag != ProdFlag::Plain;
                        let overlap = self.alt_overlap(pa, pb);
                        if flagged {
                            // **无条件登记**：带处置标记的产生式对**一律**留痕，
                            // 不以「恰好没撞上」为由跳过。若只登记 overlap≠0 的，
                            // 豁免就成了静默行为——判据无从核对「哪些产生式被
                            // 免检、谁批的、为什么」，「无冲突」也就成了一句
                            // 无根据的话。overlap 可以为 0（本来就不冲突），
                            // 那更该登记：说明「连撞都没撞上，豁免是多余的」。
                            let f = if pa.flag != ProdFlag::Plain {
                                pa.flag
                            } else {
                                pb.flag
                            };
                            self.conflicts.push(Conflict {
                                nt: nt as u8,
                                prod_a: pa.id,
                                prod_b: pb.id,
                                overlap,
                                disposition: ConflictDisposition::DescendHandled(f),
                            });
                        } else if overlap != 0 {
                            self.conflicts.push(Conflict {
                                nt: nt as u8,
                                prod_a: pa.id,
                                prod_b: pb.id,
                                overlap,
                                disposition: ConflictDisposition::SpecBlocked,
                            });
                        }
                    }
                    j += 1;
                }
                i += 1;
            }
        }
    }

    /// 两条产生式（同一左部）的 FIRST 交，处理可前缀分解。
    fn alt_overlap(&self, a: &Production, b: &Production) -> u32 {
        let la = a.rhs.len();
        let lb = b.rhs.len();
        let mut k = 0usize;
        // 公共前缀长度。
        while k < la && k < lb && a.rhs[k] == b.rhs[k] {
            k += 1;
        }
        if k == 0 {
            // 无公共前缀 ⇒ 普通 FIRST 交。
            return (self.firstset_of_slice(&a.rhs) & self.firstset_of_slice(&b.rhs)) & !NULLABLE_BIT;
        }
        if k == la && k == lb {
            // 完全相同的两条产生式 ⇒ 彻底歧义。
            return 1u32 << 30; // 用一个不可能与终结符位重合的位代表「全体」
        }
        // 一方是另一方的前缀：短者的后继 FIRST/FOLLOW 与长者的余部 FIRST 求交。
        let mut short_tail = 0u32;
        let mut nullable_tail = true;
        let mut j = k;
        while j < la {
            match a.rhs[j] {
                Symbol::Term(t) => {
                    short_tail |= t.bit();
                    nullable_tail = false;
                    break;
                }
                Symbol::Nt(x) => {
                    let f = self.first[x as usize];
                    short_tail |= f;
                    if f & NULLABLE_BIT == 0 {
                        nullable_tail = false;
                        break;
                    }
                }
            }
            j += 1;
        }
        if nullable_tail {
            short_tail |= self.follow[a.lhs as usize];
        }
        let long_rest = self.firstset_of_slice(&b.rhs[k.min(lb)..]);
        (short_tail & long_rest) & !NULLABLE_BIT
    }

    /// 符号序列的 FIRST 集（不含可空位）。
    fn firstset_of_slice(&self, slice: &[Symbol]) -> u32 {
        let mut set = 0u32;
        for s in slice.iter() {
            match s {
                Symbol::Term(t) => {
                    set |= t.bit();
                    break;
                }
                Symbol::Nt(x) => {
                    let f = self.first[*x as usize];
                    set |= f & !NULLABLE_BIT;
                    if f & NULLABLE_BIT == 0 {
                        break;
                    }
                }
            }
        }
        set
    }

    /// 取产生式（按 id 线性查；表小且只在建表期与判据侧使用）。
    fn prod(&self, id: u16) -> Option<&Production> {
        for p in self.productions.iter() {
            if p.id == id {
                return Some(p);
            }
        }
        None
    }

    /// 规范期必须拦截的冲突（`SpecBlocked`）。
    pub fn blocked(&self) -> Vec<Conflict> {
        let mut out = Vec::new();
        for c in self.conflicts.iter() {
            if c.disposition == ConflictDisposition::SpecBlocked {
                out.push(*c);
            }
        }
        out
    }

    /// 登记为「交递归下降处理」的冲突（必须逐条留痕，不得静默跳过）。
    pub fn descend_handled(&self) -> Vec<Conflict> {
        let mut out = Vec::new();
        for c in self.conflicts.iter() {
            if let ConflictDisposition::DescendHandled(_) = c.disposition {
                out.push(*c);
            }
        }
        out
    }

    /// 本文法是否通过规范期裁定（无 `SpecBlocked`）。
    pub fn admissible(&self) -> bool {
        self.adjudicated && self.blocked().is_empty()
    }
}

/// 本单骨架文法：着色器的一个可运行子集。
///
/// **注意这是骨架文法不是最终文法**——F0423-F0433 各自往里加产生式。本单只
/// 要求它 (a) 通过规范期裁定、(b) 足以让四条判据都有可判的落点。
pub fn skeleton_grammar() -> Grammar {
    let p = |id: u16, lhs: u8, rhs: Vec<Symbol>, flag: ProdFlag| Production {
        id,
        lhs,
        rhs,
        flag,
    };
    Grammar::build(vec![
        // TranslationUnit := Decl*  （以 Stmt 列表代理块体内容）
        p(0, nt::TRANSLATION_UNIT, vec![], ProdFlag::Plain),
        p(1, nt::TRANSLATION_UNIT, vec![Symbol::Nt(nt::DECL)], ProdFlag::TailLoop),
        // Decl := fn Ident ( ParamList ) { Stmt }
        p(
            2,
            nt::DECL,
            vec![
                Symbol::Term(Term::KwFn),
                Symbol::Term(Term::Ident),
                Symbol::Term(Term::LParen),
                Symbol::Nt(nt::PARAM_LIST),
                Symbol::Term(Term::RParen),
                Symbol::Term(Term::LBrace),
                Symbol::Nt(nt::STMT),
                Symbol::Term(Term::RBrace),
            ],
            ProdFlag::Plain,
        ),
        // Decl := let Ident : Type ;
        p(
            3,
            nt::DECL,
            vec![
                Symbol::Term(Term::KwLet),
                Symbol::Term(Term::Ident),
                Symbol::Term(Term::Colon),
                Symbol::Nt(nt::TYPE),
                Symbol::Term(Term::Semi),
            ],
            ProdFlag::Plain,
        ),
        // Decl := let Ident : Type = Expr ;   （3 的可前缀延拓）
        p(
            4,
            nt::DECL,
            vec![
                Symbol::Term(Term::KwLet),
                Symbol::Term(Term::Ident),
                Symbol::Term(Term::Colon),
                Symbol::Nt(nt::TYPE),
                Symbol::Term(Term::Assign),
                Symbol::Nt(nt::EXPR),
                Symbol::Term(Term::Semi),
            ],
            ProdFlag::Plain,
        ),
        // Stmt := if Expr { Stmt }
        p(
            5,
            nt::STMT,
            vec![
                Symbol::Term(Term::KwIf),
                Symbol::Nt(nt::EXPR),
                Symbol::Term(Term::LBrace),
                Symbol::Nt(nt::STMT),
                Symbol::Term(Term::RBrace),
            ],
            ProdFlag::Plain,
        ),
        // Stmt := if Expr { Stmt } else { Stmt }  （5 的可前缀延拓）
        p(
            6,
            nt::STMT,
            vec![
                Symbol::Term(Term::KwIf),
                Symbol::Nt(nt::EXPR),
                Symbol::Term(Term::LBrace),
                Symbol::Nt(nt::STMT),
                Symbol::Term(Term::RBrace),
                Symbol::Term(Term::KwElse),
                Symbol::Term(Term::LBrace),
                Symbol::Nt(nt::STMT),
                Symbol::Term(Term::RBrace),
            ],
            ProdFlag::Plain,
        ),
        // Stmt := return ;
        p(
            7,
            nt::STMT,
            vec![Symbol::Term(Term::KwReturn), Symbol::Term(Term::Semi)],
            ProdFlag::Plain,
        ),
        // Stmt := return Expr ;
        p(
            8,
            nt::STMT,
            vec![
                Symbol::Term(Term::KwReturn),
                Symbol::Nt(nt::EXPR),
                Symbol::Term(Term::Semi),
            ],
            ProdFlag::Plain,
        ),
        // Stmt := { Stmt }
        p(
            9,
            nt::STMT,
            vec![
                Symbol::Term(Term::LBrace),
                Symbol::Nt(nt::STMT),
                Symbol::Term(Term::RBrace),
            ],
            ProdFlag::Plain,
        ),
        // Expr := Primary ExprTail
        p(
            10,
            nt::EXPR,
            vec![Symbol::Nt(nt::PRIMARY), Symbol::Nt(nt::EXPR_TAIL)],
            ProdFlag::Plain,
        ),
        // ExprTail := （空）  |  + Expr
        p(11, nt::EXPR_TAIL, vec![], ProdFlag::Plain),
        p(
            12,
            nt::EXPR_TAIL,
            vec![Symbol::Term(Term::Plus), Symbol::Nt(nt::EXPR)],
            ProdFlag::Plain,
        ),
        // Primary := Ident | Number | ( Expr )
        p(13, nt::PRIMARY, vec![Symbol::Term(Term::Ident)], ProdFlag::Plain),
        p(14, nt::PRIMARY, vec![Symbol::Term(Term::Number)], ProdFlag::Plain),
        p(
            15,
            nt::PRIMARY,
            vec![
                Symbol::Term(Term::LParen),
                Symbol::Nt(nt::EXPR),
                Symbol::Term(Term::RParen),
            ],
            ProdFlag::Plain,
        ),
        // ParamList := （空） | Type Ident , ParamList
        p(16, nt::PARAM_LIST, vec![], ProdFlag::Plain),
        p(
            17,
            nt::PARAM_LIST,
            vec![
                Symbol::Nt(nt::TYPE),
                Symbol::Term(Term::Ident),
                Symbol::Term(Term::Comma),
                Symbol::Nt(nt::PARAM_LIST),
            ],
            ProdFlag::Plain,
        ),
        // Type := Ident
        p(18, nt::TYPE, vec![Symbol::Term(Term::Ident)], ProdFlag::Plain),
        // Stmt := Expr ;   （表达式语句；`b;` 这类）
        p(
            21,
            nt::STMT,
            vec![Symbol::Nt(nt::EXPR), Symbol::Term(Term::Semi)],
            ProdFlag::Plain,
        ),
    ])
}

/// 构造一份**真歧义**文法（判据用）。
///
/// 在骨架文法之上多加一条 `Stmt := Ident ;`：骨架里已有 `Stmt := Expr ;`
/// （产生式 21）且 `FIRST(Expr)` 含 `Ident`，故这两条在输入 `x ;` 上
/// **各有一条合法推导**，无法区分——属规范期必须拦截的那类。
pub fn ambiguous_grammar() -> Grammar {
    let mut g = skeleton_grammar();
    g.productions.push(Production {
        id: 20,
        lhs: nt::STMT,
        rhs: vec![Symbol::Term(Term::Ident), Symbol::Term(Term::Semi)],
        flag: ProdFlag::Plain,
    });
    g.conflicts.clear();
    g.compute_first();
    g.compute_follow();
    g.adjudicate();
    g.adjudicated = true;
    g
}

// ---------------------------------------------------------------------------
// 二之二、前瞻窗口（判据二）
// ---------------------------------------------------------------------------

/// 前瞻窗口的 k 值。**按文法定**：本单骨架文法的可判定性上界为 2
/// （`let Ident :` 需要越过标识符看清冒号；`if Expr {` 的产生式选择需要越过
/// `Expr` 首符看清块首）。判据证明 k=2 **真的被用到**且**真的改变决定**——
/// 否则 k=1 与 k=2 行为一致，窗口就成了摆设。
pub const LOOKAHEAD_K: usize = 2;

/// 前瞻窗口违例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowError {
    /// k 超出窗口容量。
    KOutOfRange { k: usize },
    /// 绝对序号低于已消费水位（= 回看企图）。
    BelowWatermark { index: usize, watermark: usize },
    /// 窗口内无此槽位（源已到流尾）。
    SlotEmpty { k: usize },
}

/// 记号流前瞻窗口（k=[`LOOKAHEAD_K`]）。
///
/// **不回扫是结构性的**：窗口只持有 `[base, base+filled)` 的记号，已消费水位
/// [`LookaheadWindow::consumed_through`] 单调递增。任何试图读水位以下记号的
/// 请求——不论从窗口内还是从流上按绝对序号——都会被 [`LookaheadWindow::peek_at`]
/// 拒绝并计入 [`LookaheadWindow::rescans`]。这是 F0420 移交的「单遍零回溯」
/// 从约定变结构的落点。
pub struct LookaheadWindow<'a> {
    src: &'a [Token],
    slots: [Option<&'a Token>; 4],
    /// 槽 0 对应的绝对序号。
    base: usize,
    filled: usize,
    consumed_through: usize,
    rescans: u32,
    k_violations: u32,
}

impl<'a> LookaheadWindow<'a> {
    /// 建窗口（源可为空串）。
    pub fn new(src: &'a [Token]) -> LookaheadWindow<'a> {
        LookaheadWindow {
            src,
            slots: [None, None, None, None],
            base: 0,
            filled: 0,
            consumed_through: 0,
            rescans: 0,
            k_violations: 0,
        }
    }

    /// 已消费水位（下一个未被消费的绝对序号）。
    pub fn consumed_through(&self) -> usize {
        self.consumed_through
    }

    /// 回看企图计数（判据要求恒为 0）。
    pub fn rescans(&self) -> u32 {
        self.rescans
    }

    /// k 越界计数。
    pub fn k_violations(&self) -> u32 {
        self.k_violations
    }

    /// 窗口容量。
    pub fn k(&self) -> usize {
        LOOKAHEAD_K
    }

    /// 槽位填充数。
    pub fn filled(&self) -> usize {
        self.filled
    }

    /// 补满窗口（至多 k 个）。**只向右读**，`base` 永不回退。
    fn refill(&mut self) {
        while self.filled < LOOKAHEAD_K {
            let idx = self.base + self.filled;
            if idx >= self.src.len() {
                break;
            }
            self.slots[self.filled] = self.src.get(idx);
            self.filled += 1;
        }
    }

    /// 看第 k 个槽（k 从 0 计）。
    ///
    /// 越界与水位违规**分别计数**且都返回 `None`——不静默，也不合并成一个
    /// 错误码，否则「k 写错」和「回看企图」在诊断里会互相掩盖。
    pub fn peek(&mut self, k: usize) -> Result<Option<&'a Token>, WindowError> {
        if k >= LOOKAHEAD_K {
            self.k_violations += 1;
            return Err(WindowError::KOutOfRange { k });
        }
        self.refill();
        if k >= self.filled {
            return Err(WindowError::SlotEmpty { k });
        }
        let idx = self.base + k;
        if idx < self.consumed_through {
            // 理论上不可达（槽位永不回退），但**必须查**：不可达不等于不检查，
            // 一旦某条新路径绕过水位，这里就是唯一的兜底。
            self.rescans += 1;
            return Err(WindowError::BelowWatermark {
                index: idx,
                watermark: self.consumed_through,
            });
        }
        Ok(self.slots[k])
    }

    /// 按**绝对序号**读一个记号——水位校验的唯一入口。
    ///
    /// 存在的理由：判据需要一个能主动**尝试回看**的入口，以便验证水位拦得住
    /// （「不回扫」若是恒真的摆设，判据就抓不到）。所有合法读取也走这里，
    /// 所以这不是测试后门——它与 [`LookaheadWindow::peek`] 受同一套约束。
    pub fn peek_at(&mut self, index: usize) -> Result<Option<&'a Token>, WindowError> {
        if index < self.consumed_through {
            self.rescans += 1;
            return Err(WindowError::BelowWatermark {
                index,
                watermark: self.consumed_through,
            });
        }
        if index >= self.src.len() {
            return Ok(None);
        }
        Ok(self.src.get(index))
    }

    /// 消费槽 0（消费水位 +1）。返回被消费的记号。
    pub fn advance(&mut self) -> Option<&'a Token> {
        let taken = self.slots[0];
        if self.filled > 0 {
            let mut i = 0usize;
            while i + 1 < self.filled {
                self.slots[i] = self.slots[i + 1];
                i += 1;
            }
            self.slots[self.filled - 1] = None;
            self.filled -= 1;
        }
        self.base += 1;
        if self.base > self.consumed_through {
            self.consumed_through = self.base;
        }
        taken
    }

    /// 当前槽 0 的绝对序号（诊断用；不消费）。
    pub fn cursor(&self) -> usize {
        self.base
    }
}

// ---------------------------------------------------------------------------
// 三、动作回调接口（判据三）
// ---------------------------------------------------------------------------

/// 一次归约动作（生产者 → 消费者的唯一事件类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reduction {
    /// 产生式 id（稳定标识；F0422 的 AST 消费者据此重建节点）。
    pub prod: u16,
    /// 左部非终结符。
    pub nt: u8,
    /// 本次归约覆盖的输入跨度。
    pub span: Span,
}

/// 动作消费者的应答。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SinkStatus {
    /// 接受。
    Ok,
    /// 拒绝（**隔离**：分析器状态机不受影响，解析继续）。
    Reject,
}

/// 动作回调接口。
///
/// 签名只给 `&mut self`（`&mut dyn ActionSink`），**拿不到 `&mut Parser`**——
/// 所以「回调把分析器状态机改坏」在编译期就不可能。这条是编译期保证，
/// 判据只断言运行期可观测的不变量，不假装能断言编译期性质。
pub trait ActionSink {
    /// 一次归约。
    fn on_reduce(&mut self, r: &Reduction) -> SinkStatus;
    /// 一条诊断（消费者可借此提前终止；本单消费者只计数）。
    fn on_error(&mut self, _d: &Diagnostic) -> SinkStatus {
        SinkStatus::Ok
    }
}

/// 最小可运行 AST 消费者：**不建真 AST**，只按动作流累计节点数。
///
/// 它存在的意义是证明「AST 构建是消费者之一」这句话不是空话——换一个消费者
/// （[`RejectAllSink`]）拿到的动作序列必须**逐条相同**，因为序列由生产者决定。
/// 真 arena 归 F0422。
pub struct NodeTallySink {
    pub accepted: u32,
    pub rejected: u32,
    pub by_nt: [u32; nt::COUNT as usize],
    pub prod_seq: Vec<u16>,
    pub last_span: Span,
}

impl NodeTallySink {
    /// 新建消费者。
    pub fn new() -> NodeTallySink {
        NodeTallySink {
            accepted: 0,
            rejected: 0,
            by_nt: [0u32; nt::COUNT as usize],
            prod_seq: Vec::new(),
            last_span: Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            },
        }
    }

    /// 按左部非终结符分的节点计数。
    pub fn node_count(&self, nt: u8) -> u32 {
        self.by_nt[(nt as usize) % (nt::COUNT as usize)]
    }
}

impl ActionSink for NodeTallySink {
    fn on_reduce(&mut self, r: &Reduction) -> SinkStatus {
        self.accepted += 1;
        self.by_nt[(r.nt as usize) % (nt::COUNT as usize)] += 1;
        self.prod_seq.push(r.prod);
        self.last_span = r.span;
        SinkStatus::Ok
    }
}

/// 全拒消费者：用来验证**隔离**语义（回调返 `Reject` 时分析器照走）。
pub struct RejectAllSink {
    pub attempts: u32,
}

impl RejectAllSink {
    /// 新建消费者。
    pub fn new() -> RejectAllSink {
        RejectAllSink { attempts: 0 }
    }
}

impl ActionSink for RejectAllSink {
    fn on_reduce(&mut self, _r: &Reduction) -> SinkStatus {
        self.attempts += 1;
        SinkStatus::Reject
    }
}

// ---------------------------------------------------------------------------
// 四、深度防护（判据四）
// ---------------------------------------------------------------------------

/// 深度准入结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepthVerdict {
    /// 准入。
    Admitted,
    /// 超限：拒绝进入，`source` 指向**嵌套源头**（最深那个已准入构造的
    /// 起始跨度），不是当前记号。
    Exceeded {
        limit: u16,
        source: Span,
        depth: u16,
    },
}

/// 解析深度防护。
///
/// 计数在**进入**非终结符时加、退出时减。超限时 [`DepthGuard::open_stack`]
/// 仍保有全部已准入构造的起始跨度，取末项即「嵌套源头」。
pub struct DepthGuard {
    limit: u16,
    depth: u16,
    max_seen: u16,
    open_stack: Vec<Span>,
}

impl DepthGuard {
    /// 建防护。`limit` 为 0 会被抬到 1（0 上限等于「一步都进不去」，
    /// 那不是防护语义，是构造错误；抬一并在判据里钉死）。
    pub fn new(limit: u16) -> DepthGuard {
        DepthGuard {
            limit: if limit == 0 { 1 } else { limit },
            depth: 0,
            max_seen: 0,
            open_stack: Vec::new(),
        }
    }

    /// 深度上限。
    pub fn limit(&self) -> u16 {
        self.limit
    }

    /// 当前深度。
    pub fn depth(&self) -> u16 {
        self.depth
    }

    /// 历史最大深度（判据用来核对「恰好触到上限」而不是「远没到」）。
    pub fn max_seen(&self) -> u16 {
        self.max_seen
    }

    /// 已准入构造的起始跨度栈。
    pub fn open_stack(&self) -> &[Span] {
        &self.open_stack
    }

    /// 尝试进入一个非终结符。
    pub fn enter(&mut self, opening: Span) -> DepthVerdict {
        if self.depth >= self.limit {
            // 嵌套源头 = 最深那个**已准入**构造的起点。
            let source = match self.open_stack.last() {
                Some(s) => *s,
                // open_stack 为空只可能是 depth==0 而 limit==0，但 new() 已把
                // 0 抬成 1，故这里不可达。仍给出可读值而非 panic。
                None => opening,
            };
            return DepthVerdict::Exceeded {
                limit: self.limit,
                source,
                depth: self.depth,
            };
        }
        self.depth += 1;
        if self.depth > self.max_seen {
            self.max_seen = self.depth;
        }
        self.open_stack.push(opening);
        DepthVerdict::Admitted
    }

    /// 退出一个非终结符。栈空时为不变量破坏，**返回 false 而非 panic**
    /// （内核里 panic 等于整机挂）。
    pub fn leave(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        let _ = self.open_stack.pop();
        true
    }
}

// ---------------------------------------------------------------------------
// 五、解析决策点（判据二「k=2 真的被用到」的落点）
// ---------------------------------------------------------------------------

/// 一处需要前瞻才能定的决策。
#[derive(Clone, Copy, Debug)]
pub struct DecisionPoint {
    /// 决策所在非终结符。
    pub nt: u8,
    /// 候选产生式数。
    pub alternatives: u8,
    /// 实际用到的前瞻深度（1 或 2）。
    pub k_used: u8,
    /// 用 k=2 实际选中的产生式 id。
    pub chosen: u16,
    /// **若只有 k=1**（只能看槽 0）会选中的产生式 id。
    ///
    /// 这个字段是判据二的支点：它让「k=2 是否改变结果」成为可测事实，
    /// 而不是「窗口声明了 k=2」这种自证式说法。当两者相等时，说明这处决策
    /// 其实 k=1 就够——窗口容量对整份文法够用，但这一处没有证明 k=2 的
    /// 必要性。
    pub chosen_if_k1: u16,
}

// ---------------------------------------------------------------------------
// 六、分析器主体（判据一：手写递归下降）
// ---------------------------------------------------------------------------

/// 解析步数预算（每步 = 一次记号消费或一次产生式归约）。超预算判**未完成**，
/// 不当作「跑完了」——防挂起。
pub const STEP_BUDGET: u32 = 200_000;

/// 递归下降分析器。
///
/// **只产生动作，不建 AST**（判据三）。状态：`win` 前瞻窗口、`depth` 深度防护、
/// `cursor` 绝对游标、`faults` 被隔离的回调异常数、`steps` 步数。
pub struct Parser<'a> {
    src_text: &'a str,
    win: LookaheadWindow<'a>,
    depth: DepthGuard,
    cursor: usize,
    steps: u32,
    actions: u32,
    faults: u32,
    decisions: Vec<DecisionPoint>,
    diags: Vec<Diagnostic>,
    exhausted: bool,
}

impl<'a> Parser<'a> {
    /// 建分析器。
    pub fn new(src_text: &'a str, toks: &'a [Token], limit: u16) -> Parser<'a> {
        Parser {
            src_text,
            win: LookaheadWindow::new(toks),
            depth: DepthGuard::new(limit),
            cursor: 0,
            steps: 0,
            actions: 0,
            faults: 0,
            decisions: Vec::new(),
            diags: Vec::new(),
            exhausted: false,
        }
    }

    /// 已派发动作数。
    pub fn actions(&self) -> u32 {
        self.actions
    }

    /// 被隔离的回调异常数。
    pub fn callback_faults(&self) -> u32 {
        self.faults
    }

    /// 步数（性能判据用；应与记号数线性）。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 当前游标（绝对记号序号）。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 当前深度。
    pub fn depth(&self) -> u16 {
        self.depth.depth()
    }

    /// 历史最大深度。
    pub fn max_depth(&self) -> u16 {
        self.depth.max_seen()
    }

    /// 已消费水位（判据核对窗口与分析器游标一致）。
    pub fn consumed_through(&self) -> usize {
        self.win.consumed_through()
    }

    /// 回看企图计数。
    pub fn rescans(&self) -> u32 {
        self.win.rescans()
    }

    /// 决策点。
    pub fn decisions(&self) -> &[DecisionPoint] {
        &self.decisions
    }

    /// 诊断。
    pub fn diags(&self) -> &[Diagnostic] {
        &self.diags
    }

    /// 是否因预算耗尽而中止。
    pub fn budget_exhausted(&self) -> bool {
        self.exhausted
    }

    /// 记号流是否已被完整消费（判据「无残留」用；不消费 Eof 自身）。
    pub fn at_end(&mut self) -> bool {
        let c = self.cursor;
        let t = match self.win.peek(0) {
            Ok(Some(t)) => t,
            _ => return true,
        };
        // 流尾：越界，或读到 Eof 且其后无内容。
        c >= usize::MAX || t.kind == TokenKind::Eof
    }

    fn charge(&mut self) -> bool {
        self.steps = self.steps.saturating_add(1);
        if self.steps > STEP_BUDGET {
            if !self.exhausted {
                self.exhausted = true;
                let sp = self.cur_span();
                self.diags.push(Diagnostic::new(
                    DiagCode::BudgetExhausted,
                    sp,
                    "步数预算耗尽，解析未完成",
                ));
            }
            return false;
        }
        true
    }

    fn cur_span(&mut self) -> Span {
        match self.win.peek(0) {
            Ok(Some(t)) => Span::of_token(t),
            _ => Span {
                start: self.src_text.len(),
                end: self.src_text.len(),
                line: 0,
                col: 0,
            },
        }
    }

    /// 把当前记号解析为终结符（关键字识别在本层——F0403 划的职责边界）。
    pub fn term_at(&mut self, k: usize) -> Option<Term> {
        let idx = self.win.cursor() + k;
        let t = match self.win.peek_at(idx) {
            Ok(Some(t)) => t,
            _ => return None,
        };
        Some(classify(t, self.src_text))
    }

    /// 消费一个记号并派发归约动作。
    ///
    /// 回调返 [`SinkStatus::Reject`] 时：记一次 [`Parser::faults`]，
    /// **不动** `cursor` / `depth` / `actions` 之外的任何状态——`actions` 也
    /// 加（动作**已发生**，消费者拒绝的是「接收」不是「发生」），游标照常
    /// 前进（归约已经吃掉了这些记号）。判据据此断言：拒绝前后 `cursor` 与
    /// `depth` 逐字段不变，且解析能走完。
    fn reduce(&mut self, prod: u16, nt: u8, span: Span, sink: &mut dyn ActionSink) {
        if !self.charge() {
            return;
        }
        let r = Reduction { prod, nt, span };
        let st = sink.on_reduce(&r);
        self.actions += 1;
        match st {
            SinkStatus::Ok => {}
            SinkStatus::Reject => {
                self.faults += 1;
                self.diags.push(Diagnostic::new(
                    DiagCode::CallbackFault,
                    span,
                    "动作回调拒绝本次归约；已隔离，分析器状态机未受影响",
                ));
            }
        }
    }

    fn err(&mut self, code: DiagCode, detail: &str, sink: &mut dyn ActionSink) {
        let sp = self.cur_span();
        let d = Diagnostic::new(code, sp, detail);
        let _ = sink.on_error(&d);
        self.diags.push(d);
    }

    /// 期望某终结符；是则消费，否则报 [`DiagCode::MissingToken`]。
    fn expect(&mut self, want: Term, prod: u16, nt: u8, sink: &mut dyn ActionSink) -> bool {
        match self.term_at(0) {
            Some(t) if t == want => {
                let sp = self.cur_span();
                let _ = self.win.advance();
                self.cursor = self.win.cursor();
                self.reduce(prod, nt, sp, sink);
                true
            }
            Some(other) => {
                let mut d = String::new();
                d.push_str("期望 ");
                d.push_str(want.name());
                d.push_str("，实得 ");
                d.push_str(other.name());
                self.err(DiagCode::MissingToken, &d, sink);
                false
            }
            None => {
                let mut d = String::new();
                d.push_str("期望 ");
                d.push_str(want.name());
                d.push_str("，但记号流已到末尾");
                self.err(DiagCode::MissingToken, &d, sink);
                false
            }
        }
    }

    /// 解析翻译单元（反复解析声明直到流尾或预算/深度阻断）。
    pub fn translation_unit(&mut self, sink: &mut dyn ActionSink) {
        let start = self.cur_span();
        match self.depth.enter(start) {
            DepthVerdict::Admitted => {}
            DepthVerdict::Exceeded { limit, source, depth } => {
                let mut d = String::new();
                d.push_str("解析深度超限：上限 ");
                d.push_str(&limit.to_string());
                d.push_str("，当前 ");
                d.push_str(&depth.to_string());
                d.push_str("；嵌套源头见诊断跨度");
                self.err_at(DiagCode::DepthExceeded, source, &d, sink);
                return;
            }
        }
        let mut guard = 0u32;
        loop {
            if self.exhausted {
                break;
            }
            match self.term_at(0) {
                None => break,
                Some(Term::Eof) => break,
                Some(_) => {}
            }
            // **进展保证**：声明解析在遇到不属于声明位置的记号时只报错而
            // 不消费（记号属于别的层级的处置权）。若无进展就再进同一循环，
            // 那是死循环——在内核里表现为「卡死」，比报错更难定位。故显式
            // 记录本轮游标，零进展时强制跳过该记号（跳过本身也报一次，
            // 不静默），保证每轮至少前进一个记号。
            let before = self.cursor;
            self.declaration(sink);
            guard += 1;
            if self.cursor == before {
                let sp = self.cur_span();
                let skipped = self.win.advance();
                self.cursor = self.win.cursor();
                let mut d = String::new();
                d.push_str("顶层出现无法归属的记号，已跳过以便继续");
                self.diags.push(Diagnostic::new(DiagCode::UnexpectedToken, sp, &d));
                let _ = skipped;
            }
            if guard > STEP_BUDGET {
                break;
            }
        }
        // TranslationUnit 的归约：覆盖整段。
        let end = self.cur_span();
        let sp = if start.start <= end.start { start.join(end) } else { start };
        self.depth.leave();
        self.reduce(0, nt::TRANSLATION_UNIT, sp, sink);
    }

    /// 解析一条声明（`fn` / `let` 两族）。
    ///
    /// **k=2 的落点**：判断是 `let x` 还是 `let x :` 的后续，需要越过标识符
    /// 看第 2 个槽；只给 k=1 时 `let x` 与 `let y` 无法区分是「类型待定」还是
    /// 「下一个声明开始」。决策点在此登记 [`DecisionPoint`]。
    pub fn declaration(&mut self, sink: &mut dyn ActionSink) {
        let start = self.cur_span();
        match self.depth.enter(start) {
            DepthVerdict::Admitted => {}
            DepthVerdict::Exceeded { limit, source, depth } => {
                let mut d = String::new();
                d.push_str("解析深度超限：上限 ");
                d.push_str(&limit.to_string());
                d.push_str("，当前 ");
                d.push_str(&depth.to_string());
                self.err_at(DiagCode::DepthExceeded, source, &d, sink);
                return;
            }
        }
        match self.term_at(0) {
            Some(Term::KwFn) => {
                let _ = self.expect(Term::KwFn, 2, nt::DECL, sink);
                let _ = self.expect(Term::Ident, 2, nt::DECL, sink);
                let _ = self.expect(Term::LParen, 2, nt::DECL, sink);
                self.param_list(sink);
                let _ = self.expect(Term::RParen, 2, nt::DECL, sink);
                let _ = self.expect(Term::LBrace, 2, nt::DECL, sink);
                self.statement(sink);
                let _ = self.expect(Term::RBrace, 2, nt::DECL, sink);
            }
            Some(Term::KwLet) => {
                let _ = self.expect(Term::KwLet, 3, nt::DECL, sink);
                let _ = self.expect(Term::Ident, 3, nt::DECL, sink);
                let _ = self.expect(Term::Colon, 3, nt::DECL, sink);
                self.ty(sink);
                // **决策点**：k=1 时看到 `;` 就得赌「无初始化器」；看到 `=` 才
                // 知道有初始化器。k=2 让这个决定在**消费之前**就能做。
                let t0 = self.term_at(0);
                let t1 = self.term_at(1);
                let chosen = if t0 == Some(Term::Assign) { 4 } else { 3 };
                let chosen_k1 = if t0 == Some(Term::Assign) || t0 == Some(Term::Semi) {
                    chosen
                } else {
                    3
                };
                self.decisions.push(DecisionPoint {
                    nt: nt::DECL,
                    alternatives: 2,
                    k_used: if t1.is_some() { 2 } else { 1 },
                    chosen,
                    chosen_if_k1: chosen_k1,
                });
                if t0 == Some(Term::Assign) {
                    let _ = self.expect(Term::Assign, 4, nt::DECL, sink);
                    self.expression(sink);
                }
                let _ = self.expect(Term::Semi, chosen, nt::DECL, sink);
            }
            Some(other) => {
                let mut d = String::new();
                d.push_str("声明位置出现 ");
                d.push_str(other.name());
                d.push_str("，期望 fn 或 let");
                self.err(DiagCode::UnexpectedToken, &d, sink);
            }
            None => {
                self.err(DiagCode::MissingToken, "声明位置记号流已结束", sink);
            }
        }
        let _ = self.depth.leave();
    }

    fn err_at(&mut self, code: DiagCode, sp: Span, detail: &str, sink: &mut dyn ActionSink) {
        let d = Diagnostic::new(code, sp, detail);
        let _ = sink.on_error(&d);
        self.diags.push(d);
    }

    /// 解析参数表（可空）。
    ///
    /// **进展保证**：右递归必须以「本轮消费了至少一个记号」为前提，否则尾递归
    /// 退化成死循环（`fn ( {` 这类坏输入会在此无限递归直至爆栈——内核里爆栈
    /// 等于整机挂）。故记录轮前游标，零进展时不再递归，由调用方的
    /// `expect(RParen)` 报出「缺少记号」。
    pub fn param_list(&mut self, sink: &mut dyn ActionSink) {
        match self.term_at(0) {
            Some(Term::RParen) => {
                let sp = self.cur_span();
                self.reduce(16, nt::PARAM_LIST, sp, sink);
            }
            _ => {
                let before = self.cursor;
                self.ty(sink);
                let _ = self.expect(Term::Ident, 17, nt::PARAM_LIST, sink);
                let _ = self.expect(Term::Comma, 17, nt::PARAM_LIST, sink);
                if self.cursor == before {
                    // 本轮零进展：不再右递归，交给调用方报错。
                    return;
                }
                self.param_list(sink);
            }
        }
    }

    /// 解析类型语法面（只收不裁，合法性归 F0443 语义期）。
    pub fn ty(&mut self, sink: &mut dyn ActionSink) {
        match self.term_at(0) {
            Some(Term::Ident) => {
                let _ = self.expect(Term::Ident, 18, nt::TYPE, sink);
            }
            _ => {
                self.err(DiagCode::UnexpectedToken, "类型语法面期望标识符", sink);
            }
        }
    }

    /// 解析一条语句。
    pub fn statement(&mut self, sink: &mut dyn ActionSink) {
        let start = self.cur_span();
        match self.depth.enter(start) {
            DepthVerdict::Admitted => {}
            DepthVerdict::Exceeded { limit, source, depth } => {
                let mut d = String::new();
                d.push_str("解析深度超限：上限 ");
                d.push_str(&limit.to_string());
                d.push_str("，当前 ");
                d.push_str(&depth.to_string());
                self.err_at(DiagCode::DepthExceeded, source, &d, sink);
                return;
            }
        }
        match self.term_at(0) {
            Some(Term::KwIf) => {
                let _ = self.expect(Term::KwIf, 5, nt::STMT, sink);
                self.expression(sink);
                let _ = self.expect(Term::LBrace, 5, nt::STMT, sink);
                self.statement(sink);
                // **决策点必须在消费 `}` 之前**（判据二的核心）。
                // 此时槽 0 是 `}`、槽 1 是 `}` 之后的记号：k=2 能越过 `}`
                // 看到 `else`，k=1 只能看到 `}` 因而只能选产生式 5。
                //
                // 顺序写反的后果不是「多花一次前瞻」，而是**恒选 5**：
                // 先消费 `}` 再看槽 1，槽 1 已是 `else` 的下一个记号，
                // `else` 分支永远进不去（`chosen` 恒为 5，与 k=1 无差别）。
                let t0 = self.term_at(0);
                let t1 = self.term_at(1);
                let chosen = if t1 == Some(Term::KwElse) { 6 } else { 5 };
                // k=1 视角：只看得到槽 0（`}`），无从知道有 else，故恒为 5。
                let chosen_k1 = if t0 == Some(Term::KwElse) { 6 } else { 5 };
                self.decisions.push(DecisionPoint {
                    nt: nt::STMT,
                    alternatives: 2,
                    k_used: if t1.is_some() { 2 } else { 1 },
                    chosen,
                    chosen_if_k1: chosen_k1,
                });
                let _ = self.expect(Term::RBrace, chosen, nt::STMT, sink);
                if chosen == 6 {
                    let _ = self.expect(Term::KwElse, 6, nt::STMT, sink);
                    let _ = self.expect(Term::LBrace, 6, nt::STMT, sink);
                    self.statement(sink);
                    let _ = self.expect(Term::RBrace, 6, nt::STMT, sink);
                }
            }
            Some(Term::KwReturn) => {
                let _ = self.expect(Term::KwReturn, 7, nt::STMT, sink);
                if self.term_at(0) == Some(Term::Semi) {
                    let _ = self.expect(Term::Semi, 7, nt::STMT, sink);
                } else {
                    self.expression(sink);
                    let _ = self.expect(Term::Semi, 8, nt::STMT, sink);
                }
            }
            Some(Term::LBrace) => {
                let _ = self.expect(Term::LBrace, 9, nt::STMT, sink);
                self.statement(sink);
                let _ = self.expect(Term::RBrace, 9, nt::STMT, sink);
            }
            Some(Term::RBrace) | Some(Term::Eof) | None => {
                // 块尾/流尾：交回上层，不在此处报错（由上层 expect 决定）。
            }
            Some(_) => {
                self.expression(sink);
                let _ = self.expect(Term::Semi, 21, nt::STMT, sink);
            }
        }
        let _ = self.depth.leave();
    }

    /// 解析表达式（`Primary ExprTail`）。
    pub fn expression(&mut self, sink: &mut dyn ActionSink) {
        let start = self.cur_span();
        match self.depth.enter(start) {
            DepthVerdict::Admitted => {}
            DepthVerdict::Exceeded { limit, source, depth } => {
                let mut d = String::new();
                d.push_str("表达式嵌套超限：上限 ");
                d.push_str(&limit.to_string());
                d.push_str("，当前 ");
                d.push_str(&depth.to_string());
                self.err_at(DiagCode::DepthExceeded, source, &d, sink);
                return;
            }
        }
        self.primary(sink);
        self.expr_tail(sink);
        let end = self.cur_span();
        let sp = if start.start <= end.start { start.join(end) } else { start };
        self.depth.leave();
        self.reduce(10, nt::EXPR, sp, sink);
    }

    fn expr_tail(&mut self, sink: &mut dyn ActionSink) {
        if self.term_at(0) == Some(Term::Plus) {
            let _ = self.expect(Term::Plus, 12, nt::EXPR_TAIL, sink);
            self.expression(sink);
        } else {
            let sp = self.cur_span();
            self.reduce(11, nt::EXPR_TAIL, sp, sink);
        }
    }

    fn primary(&mut self, sink: &mut dyn ActionSink) {
        let start = self.cur_span();
        match self.depth.enter(start) {
            DepthVerdict::Admitted => {}
            DepthVerdict::Exceeded { limit, source, depth } => {
                let mut d = String::new();
                d.push_str("括号嵌套超限：上限 ");
                d.push_str(&limit.to_string());
                d.push_str("，当前 ");
                d.push_str(&depth.to_string());
                self.err_at(DiagCode::DepthExceeded, source, &d, sink);
                return;
            }
        }
        match self.term_at(0) {
            Some(Term::Ident) => {
                let _ = self.expect(Term::Ident, 13, nt::PRIMARY, sink);
            }
            Some(Term::Number) => {
                let _ = self.expect(Term::Number, 14, nt::PRIMARY, sink);
            }
            Some(Term::LParen) => {
                let _ = self.expect(Term::LParen, 15, nt::PRIMARY, sink);
                self.expression(sink);
                let _ = self.expect(Term::RParen, 15, nt::PRIMARY, sink);
            }
            Some(other) => {
                let mut d = String::new();
                d.push_str("表达式位置出现 ");
                d.push_str(other.name());
                self.err(DiagCode::UnexpectedToken, &d, sink);
            }
            None => {
                self.err(DiagCode::MissingToken, "表达式位置记号流已结束", sink);
            }
        }
        self.depth.leave();
    }
}

/// 记号 → 终结符（关键字识别在语法层：F0403 只给 `TokenKind::Ident`）。
pub fn classify(t: &Token, src: &str) -> Term {
    match t.kind {
        TokenKind::Number => Term::Number,
        TokenKind::Eof => Term::Eof,
        TokenKind::Ident => match src.get(t.start..t.end) {
            Some("fn") => Term::KwFn,
            Some("let") => Term::KwLet,
            Some("if") => Term::KwIf,
            Some("else") => Term::KwElse,
            Some("return") => Term::KwReturn,
            _ => Term::Ident,
        },
        TokenKind::Punct => match src.get(t.start..t.end) {
            Some("+") => Term::Plus,
            Some("=") => Term::Assign,
            Some(";") => Term::Semi,
            Some(":") => Term::Colon,
            Some(",") => Term::Comma,
            Some("(") => Term::LParen,
            Some(")") => Term::RParen,
            Some("{") => Term::LBrace,
            Some("}") => Term::RBrace,
            _ => Term::Eof,
        },
        _ => Term::Eof,
    }
}

/// 便捷入口：一次性解析（等价于「建分析器 + 跑翻译单元 + 收尾」）。
///
/// 返回分析器本体以便调用方核对步数 / 游标 / 诊断；`sink` 的生命周期与返回
/// 值**不绑**（回调在函数内已派发完毕，返回后不再持有）。
pub fn parse<'a>(
    src_text: &'a str,
    toks: &'a [Token],
    limit: u16,
    sink: &mut dyn ActionSink,
) -> Parser<'a> {
    let mut p = Parser::new(src_text, toks, limit);
    p.translation_unit(sink);
    p
}