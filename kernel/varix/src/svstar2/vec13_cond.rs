//! VE-F0413 · 条件编译求值（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0413`
//!
//! **判据（锚点原文）**：整型语义、嵌套栈、跳过快扫、语义显性、判据。
//! - **表达式求值**：条件编译表达式按**整型常量语义**求值（整型常量表达式
//!   家族——`?:` / `||` / `&&` / `|` / `^` / `&` / 等式 / 关系 / 移位 / 加减
//!   / 乘模模 / 一元 全族齐备）；整型字面量（十进制 / `0x` / `0o` / `_` 分隔
//!   / `i32`·`u32`·`i64`·`u64` 后缀）**复用 F0406 字面量解析**（单一实现不双份），
//!   后缀决定有符号性与位宽；**未定义宏按假处理且显性**——求值成 0 的同时
//!   产出一条注记（按假不是静默消失，注记带三要素）；
//! - **分支选择全族**：`ifdef` / `ifndef` / `if` / `elif` / `else` / `endif`
//!   六指令全族；`defined(X)` 与 `defined X` 两种拼写都认；`ifdef` 的
//!   "已定义"由**上游 F0412 宏表登记态裁定**（宏表命中即已定义）；
//! - **嵌套栈管理**：条件栈 = 嵌套 × 分支状态（Seeking 未选中 / Taken 本分支
//!   生效 / Done 已有真分支、后续全跳过）；压弹 O(1)；**未闭合报错逐层指向
//!   每一层的开指令**（行列双坐标，不只报最外层）；`else` 多重报错；
//!   `elif` / `else` 后再接分支指令报错；嵌套深度超限报错（防护病态深嵌套）；
//! - **求值结果记录供跳过段快速掠过**：每个分支的选择结果（Taken / Skipped /
//!   Deferred）连同指令坐标与表达式原文入记录表；跳过段按**字节跨度**归并成
//!   `SkipRegion`，下游（F0414 include 协同、构建系统）按跨度 O(1) 定位、
//!   无需重扫；**掠过 O(段长) 快扫**——跳过的段只做词法跨度累加，不求值、
//!   不报语义错（**语义显性声明：跳过段只做词法**）。
//!
//! 性能逐项分解：求值 O(表达式长度)（递归下降单遍）；栈 O(1) 压弹；
//! 掠过 O(段长) 快扫（只累加字节，不展开宏、不构表达式）。

use super::vec03_lexer::{Lexer, TokenKind};
use super::vec06_lit::{parse_literal, IntSuffix, LiteralConfig, LiteralValue};
use super::vec11_prepro::{CodeLine, DirectiveKind, DirectiveLine, PreproStreams};
use super::vec12_macro::MacroTable;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、配置与整型值语义（判据：整型语义）
// ---------------------------------------------------------------------------

/// 条件编译求值配置（深度上限可配，防病态嵌套拖垮求值器）。
#[derive(Clone, Copy, Debug)]
pub struct CondConfig {
    /// 条件嵌套深度上限（`#if` 族最大嵌套层数）。
    pub max_depth: usize,
    /// 表达式内宏展开深度上限（`#if` 条件里宏套宏的护栏）。
    pub max_expand_depth: usize,
    /// 跳过段是否记录注记（默认记录：跳过段的宏引用也值得可查）。
    pub report_skipped_notes: bool,
}

impl CondConfig {
    pub const DEFAULT_MAX_DEPTH: usize = 64;
    pub const DEFAULT_MAX_EXPAND_DEPTH: usize = 32;

    pub fn default_config() -> CondConfig {
        CondConfig {
            max_depth: CondConfig::DEFAULT_MAX_DEPTH,
            max_expand_depth: CondConfig::DEFAULT_MAX_EXPAND_DEPTH,
            report_skipped_notes: true,
        }
    }
}

/// 整型常量值（`v` × 有符号标志——后缀语义决定除法/比较/移位的解释域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntVal {
    pub v: i128,
    pub unsigned: bool,
}

impl IntVal {
    pub const fn sint(v: i128) -> IntVal {
        IntVal { v, unsigned: false }
    }

    pub const fn uint(v: u128) -> IntVal {
        IntVal {
            v: v as i128,
            unsigned: true,
        }
    }

    /// 布尔归一（`0` / `1`）——比较与逻辑运算的产物统一走这里。
    pub const fn bool(b: bool) -> IntVal {
        IntVal {
            v: if b { 1 } else { 0 },
            unsigned: false,
        }
    }

    pub const fn truthy(self) -> bool {
        self.v != 0
    }

    /// 报告用类型名（报错信息里的"类型说明"来源）。
    pub const fn type_label(self) -> &'static str {
        if self.unsigned {
            "无符号整型常量"
        } else {
            "有符号整型常量"
        }
    }
}

/// 字面量后缀 → 值（复用 F0406 的后缀语义，单一实现不双份）。
fn int_from_suffix(value: u128, suffix: IntSuffix) -> IntVal {
    match suffix {
        IntSuffix::I32 => IntVal::sint(value as u32 as i32 as i128),
        IntSuffix::U32 => IntVal::uint(value as u32 as u128),
        IntSuffix::I64 => IntVal::sint(value as u64 as i64 as i128),
        IntSuffix::U64 => IntVal::uint(value as u64 as u128),
    }
}

// ---------------------------------------------------------------------------
// 二、错误与注记（三要素呈现 + 逐层未闭合定位）
// ---------------------------------------------------------------------------

/// 一个未闭合层的开指令坐标（未闭合报错逐层指向各层）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenLayer {
    /// 开指令种类（`#if` / `#ifdef` / `#ifndef`）。
    pub kind: DirectiveKind,
    pub line: usize,
    pub pos: usize,
    /// 开指令原文（便于回溯是哪一条）。
    pub text: String,
}

/// 条件编译错误（三要素 + 未闭合时的逐层开指令链）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CondError {
    pub code: &'static str,
    pub line: usize,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
    /// 未闭合时逐层列出每一层的开指令（空 = 非未闭合类错误）。
    pub layers: Vec<OpenLayer>,
}

impl CondError {
    /// 三要素齐备（判据：语义显性）。
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 未闭合类错误的逐层人话串（"第 3 行 #if → 第 1 行 #ifdef"）。
    pub fn layer_trace(&self) -> String {
        if self.layers.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = Vec::new();
        for (i, l) in self.layers.iter().enumerate() {
            parts.push(format!(
                "第{}层：第 {} 行的 #{}（字节 {}）",
                i + 1,
                l.line,
                l.kind.name(),
                l.pos
            ));
        }
        parts.join(" → ")
    }
}

/// 注记（非阻断的显性记录：未定义宏按假处理、跳过段引用等）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CondNote {
    pub code: &'static str,
    pub line: usize,
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

fn cerr(
    code: &'static str,
    line: usize,
    pos: usize,
    what: String,
    why: String,
    next: String,
) -> CondError {
    CondError {
        code,
        line,
        pos,
        what,
        why,
        next,
        layers: Vec::new(),
    }
}

fn note(
    code: &'static str,
    line: usize,
    pos: usize,
    what: String,
    why: String,
    next: String,
) -> CondNote {
    CondNote {
        code,
        line,
        pos,
        what,
        why,
        next,
    }
}

// ---------------------------------------------------------------------------
// 三、条件栈（判据：嵌套栈）
// ---------------------------------------------------------------------------

/// 分支状态：一个 `#if` 族内"是否已有真分支"与"本分支是否生效"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchState {
    /// 仍在寻找真分支（尚未有分支被选中）。
    Seeking,
    /// 本分支为真且生效中。
    Taken,
    /// 已有真分支在前，本分支一律跳过。
    Done,
}

impl BranchState {
    pub const fn label(self) -> &'static str {
        match self {
            BranchState::Seeking => "寻求真分支",
            BranchState::Taken => "本分支生效",
            BranchState::Done => "已有真分支·跳过",
        }
    }
}

/// 条件栈的一层（嵌套 × 分支状态）。
#[derive(Clone, Debug, PartialEq)]
pub struct CondFrame {
    /// 开指令种类（`#if` / `#ifdef` / `#ifndef`）。
    pub kind: DirectiveKind,
    /// 当前分支状态。
    pub state: BranchState,
    /// 外层（父帧）是否生效——父层跳过则本层整层跳过。
    pub parent_active: bool,
    /// 开指令坐标（未闭合报错指向本层）。
    pub open_line: usize,
    pub open_pos: usize,
    pub open_text: String,
    /// 本层是否已出现 `#else`（多重 else 判定）。
    pub seen_else: bool,
    /// 本层已求值的分支指令计数（诊断用）。
    pub branches: usize,
}

impl CondFrame {
    /// 本层当前是否让代码流生效（父层生效 × 本分支为真）。
    pub fn active(&self) -> bool {
        self.parent_active && self.state == BranchState::Taken
    }

    /// 本层是否处于"整层跳过"（父层已跳过）。
    pub fn parent_skipped(&self) -> bool {
        !self.parent_active
    }
}

/// 分支选择结果（求值结果记录——供下游快扫消费）。
#[derive(Clone, Debug, PartialEq)]
pub struct BranchRecord {
    pub kind: DirectiveKind,
    pub line: usize,
    pub pos: usize,
    pub state: BranchState,
    /// 该分支的判定表达式原文（`ifdef` 是被测宏名）。
    pub expr: String,
    /// 所在嵌套深度（0 = 顶层）。
    pub depth: usize,
}

/// 保留的代码行（生效段产物，位置保真）。
#[derive(Clone, Debug, PartialEq)]
pub struct KeptLine {
    pub line: usize,
    pub pos: usize,
    pub text: String,
    /// 所在嵌套深度（诊断可溯）。
    pub depth: usize,
}

/// 跳过段记录（字节跨度——下游按跨度快扫不重扫）。
#[derive(Clone, Debug, PartialEq)]
pub struct SkipRegion {
    pub start_line: usize,
    pub start_pos: usize,
    pub end_line: usize,
    pub end_pos: usize,
    /// 所在嵌套深度。
    pub depth: usize,
    /// 跳过原因（父层跳过 / 本分支为假 / 已有真分支）。
    pub reason: SkipReason,
}

/// 跳过原因（显性：跳过不是"丢了"，是带原因的跨度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// 外层已跳过，本层连带跳过。
    ParentSkipped,
    /// 本分支判定为假。
    BranchFalse,
    /// 前面已有真分支，本分支按序跳过。
    AlreadyTaken,
}

impl SkipReason {
    pub const fn label(self) -> &'static str {
        match self {
            SkipReason::ParentSkipped => "外层已跳过",
            SkipReason::BranchFalse => "本分支为假",
            SkipReason::AlreadyTaken => "已有真分支",
        }
    }
}

/// 求值统计（可观测：性能判据的实测面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CondStats {
    /// 实际求值的表达式数（跳过的段不计入）。
    pub exprs_evaluated: usize,
    /// 因跳过而未求值的表达式数。
    pub exprs_skipped: usize,
    /// 表达式内宏展开次数。
    pub macro_expansions: usize,
    /// 条件栈峰值深度。
    pub peak_depth: usize,
    /// 掠过累计字节数（快扫面）。
    pub skipped_bytes: usize,
    /// 表达式记号累计消费数（O(表达式长度) 的计量面）。
    pub expr_tokens: usize,
    /// 展开深度触顶次数（护栏命中，可观测不静默）。
    pub expand_guard_hits: usize,
}

// ---------------------------------------------------------------------------
// 四、表达式求值器（整型常量语义 + 宏代入 + defined）
// ---------------------------------------------------------------------------

/// 表达式记号（**自带原文**）。
///
/// 为什么不用「记号 + 共享 src 切片」：表达式里的宏会被展开成**另一串**
/// 记号（宏体、实参各是独立文本）。若记号只记偏移而共用外层 `src` 取词，
/// 宏体记号会按外层偏移切出错字。这里让记号自带原文，展开时各串自洽——
/// 代价是每记号一次小分配，换来取词零错位（正确性优先）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct ETok {
    kind: TokenKind,
    text: String,
    /// 本记号串内的字节偏移（诊断定位参考）。
    start: usize,
}

/// 词法辅助：主词法切分并滤掉注释与 EOF 哨兵（指令体内不产注释语义）。
fn lex_expr(src: &str, line: usize, pos: usize) -> Result<Vec<ETok>, CondError> {
    let raw = Lexer::new()
        .and_then(|lx| lx.scan(src))
        .map_err(|e| {
            cerr(
                "E_COND_LEX",
                line,
                pos,
                format!("条件表达式词法失败：{}", e.describe()),
                "条件表达式与代码体走同一套主词法——词法错误原样上抛不转译".to_string(),
                "按主词法建议修正表达式".to_string(),
            )
        })?;
    Ok(raw
        .0
        .into_iter()
        .filter(|t| !matches!(t.kind, TokenKind::Comment | TokenKind::Eof))
        .map(|t| ETok {
            kind: t.kind,
            text: t.text(src).to_string(),
            start: t.start,
        })
        .collect())
}

/// 表达式求值器（递归下降单遍——判据：求值 O(表达式长度)）。
struct ExprEval<'a> {
    toks: Vec<ETok>,
    i: usize,
    table: &'a MacroTable,
    cfg: &'a CondConfig,
    /// 基坐标（表达式起点——报错的绝对位置来源）。
    base_line: usize,
    base_pos: usize,
    /// 宏展开深度（护栏）。
    depth: usize,
    notes: Vec<CondNote>,
    stats: CondStats,
}

impl<'a> ExprEval<'a> {
    fn new(
        src: &str,
        table: &'a MacroTable,
        cfg: &'a CondConfig,
        base_line: usize,
        base_pos: usize,
    ) -> Result<Self, CondError> {
        let toks = lex_expr(src, base_line, base_pos)?;
        Ok(ExprEval {
            toks,
            i: 0,
            table,
            cfg,
            base_line,
            base_pos,
            depth: 0,
            notes: Vec::new(),
            stats: CondStats::default(),
        })
    }

    fn peek(&self) -> Option<&ETok> {
        self.toks.get(self.i)
    }

    fn peek_text(&self) -> &str {
        match self.toks.get(self.i) {
            Some(t) => &t.text,
            None => "",
        }
    }

    fn bump(&mut self) -> Option<ETok> {
        let t = self.toks.get(self.i).cloned();
        if t.is_some() {
            self.i += 1;
            self.stats.expr_tokens += 1;
        }
        t
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        match self.toks.get(self.i) {
            Some(t) if t.kind == TokenKind::Punct && t.text == p => {
                self.i += 1;
                self.stats.expr_tokens += 1;
                true
            }
            _ => false,
        }
    }

    /// 当前记号的绝对字节位置（表达式内偏移 + 基坐标）。
    fn cur_pos(&self) -> usize {
        match self.toks.get(self.i) {
            Some(t) => self.base_pos + t.start,
            None => self.base_pos,
        }
    }

    fn at_end(&self) -> bool {
        self.i >= self.toks.len()
    }

    /// 入口：`?:` 最低优先级、右结合。
    fn parse_all(&mut self) -> Result<IntVal, CondError> {
        let v = self.parse_ternary()?;
        if !self.at_end() {
            let leftover = self.peek_text().to_string();
            return Err(cerr(
                "E_COND_TRAILING",
                self.base_line,
                self.cur_pos(),
                format!("条件表达式在 {} 处有多余记号", leftover),
                "整型常量表达式求值到记号流末尾——残留记号说明表达式写错了".to_string(),
                "删掉多余记号；若想比较请补齐运算符".to_string(),
            ));
        }
        Ok(v)
    }

    /// `cond ? a : b`（右结合）。
    fn parse_ternary(&mut self) -> Result<IntVal, CondError> {
        let cond = self.parse_binary(0)?;
        if self.eat_punct("?") {
            let t = self.parse_ternary()?;
            if !self.eat_punct(":") {
                return Err(cerr(
                    "E_COND_TERNARY",
                    self.base_line,
                    self.cur_pos(),
                    "三目表达式缺冒号分支".to_string(),
                    "`?:` 的三个记号必须齐备".to_string(),
                    "补冒号与假分支表达式".to_string(),
                ));
            }
            let f = self.parse_ternary()?;
            let unsigned = t.unsigned || f.unsigned;
            return Ok(IntVal {
                v: if cond.truthy() { t.v } else { f.v },
                unsigned,
            });
        }
        Ok(cond)
    }

    /// 二元优先级 climbing（层号越大结合越紧）。
    fn parse_binary(&mut self, min_prec: u8) -> Result<IntVal, CondError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let Some(tok) = self.peek().cloned() else {
                break;
            };
            if tok.kind != TokenKind::Punct {
                break;
            }
            let op = tok.text.clone();
            let Some(prec) = binary_prec(&op) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.i += 1;
            self.stats.expr_tokens += 1;
            // 左结合：右操作数要求更高一级；右结合算符（无）传 prec。
            let rhs = self.parse_binary(prec + 1)?;
            lhs = self.apply_binary(&op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    /// 一元：`+` / `-` / `~` / `!`（右结合前缀）。
    fn parse_unary(&mut self) -> Result<IntVal, CondError> {
        let Some(tok) = self.peek().cloned() else {
            return Err(cerr(
                "E_COND_EMPTY",
                self.base_line,
                self.base_pos,
                "条件表达式此处缺操作数".to_string(),
                "一元运算符后必须跟一个表达式".to_string(),
                "补操作数或删掉悬空的一元运算符".to_string(),
            ));
        };
        if tok.kind == TokenKind::Punct {
            let op = tok.text.clone();
            match op.as_str() {
                "+" | "-" | "~" | "!" => {
                    self.i += 1;
                    self.stats.expr_tokens += 1;
                    let v = self.parse_unary()?;
                    return Ok(match op.as_str() {
                        "+" => v,
                        "-" => IntVal {
                            v: v.v.wrapping_neg(),
                            unsigned: v.unsigned,
                        },
                        "~" => IntVal {
                            v: !v.v,
                            unsigned: v.unsigned,
                        },
                        _ => IntVal::bool(!v.truthy()),
                    });
                }
                "(" => {
                    self.i += 1;
                    self.stats.expr_tokens += 1;
                    let v = self.parse_ternary()?;
                    if !self.eat_punct(")") {
                        return Err(cerr(
                            "E_COND_PAREN",
                            self.base_line,
                            self.cur_pos(),
                            "条件表达式括号未闭合".to_string(),
                            "圆括号必须成对".to_string(),
                            "补右括号".to_string(),
                        ));
                    }
                    return Ok(v);
                }
                _ => {}
            }
        }
        self.parse_primary()
    }

    /// 基本项：整型字面量 / 标识符（含宏代入）/ `defined` / 字符串（非整型报错）。
    fn parse_primary(&mut self) -> Result<IntVal, CondError> {
        let Some(tok) = self.bump() else {
            return Err(cerr(
                "E_COND_EMPTY",
                self.base_line,
                self.base_pos,
                "条件表达式为空".to_string(),
                "`#if` / `#elif` 后必须有整型常量表达式".to_string(),
                "补表达式，或改用 `#ifdef` / `#ifndef`".to_string(),
            ));
        };
        match tok.kind {
            TokenKind::Number => {
                let text = tok.text.clone();
                let cfg = LiteralConfig::default_config();
                match parse_literal(&text, &cfg) {
                    Ok(p) => match p.value {
                        LiteralValue::Int { value, suffix, .. } => {
                            Ok(int_from_suffix(value, suffix))
                        }
                        LiteralValue::Float { .. } => Err(cerr(
                            "E_COND_NOT_INT",
                            self.base_line,
                            self.base_pos + tok.start,
                            format!("条件表达式里的浮点字面量 {} 不是整型", text),
                            "整型常量语义：浮点常量在条件编译里无定义——分阶段语义不同"
                                .to_string(),
                            "改写成整型表达式（如改比较式），或把浮点量挪进运行期代码"
                                .to_string(),
                        )),
                    },
                    Err(e) => Err(cerr(
                        "E_COND_LITERAL",
                        self.base_line,
                        self.base_pos + tok.start,
                        format!("条件表达式的整型字面量非法：{}", e.what),
                        e.why,
                        e.next,
                    )),
                }
            }
            TokenKind::Ident => self.resolve_ident(&tok),
            TokenKind::Str => Err(cerr(
                "E_COND_NOT_INT",
                self.base_line,
                self.base_pos + tok.start,
                format!("条件表达式里的字符串字面量 {} 不是整型", tok.text),
                "整型常量语义：字符串在条件编译里无值可言".to_string(),
                "删掉字符串字面量；确需按串分派请让宏展开产出整型开关".to_string(),
            )),
            TokenKind::Directive => Err(cerr(
                "E_COND_NESTED_DIRECTIVE",
                self.base_line,
                self.base_pos + tok.start,
                "条件表达式里嵌了预处理指令".to_string(),
                "条件编译指令不能出现在表达式内部".to_string(),
                "把该指令拆到独立行".to_string(),
            )),
            _ => Err(cerr(
                "E_COND_NOT_INT",
                self.base_line,
                self.base_pos + tok.start,
                format!("条件表达式里的 {} 记号不是整型", tok.kind.name()),
                "整型常量语义：只有整型字面量与宏名可参与求值".to_string(),
                "改用整型字面量或宏名".to_string(),
            )),
        }
    }

    /// 标识符解析：`defined` / 宏代入 / 未定义宏按假处理（显性注记）。
    fn resolve_ident(&mut self, tok: &ETok) -> Result<IntVal, CondError> {
        let text = tok.text.clone();
        if text == "defined" {
            let paren = self.eat_punct("(");
            let Some(name_tok) = self.peek().cloned() else {
                return Err(cerr(
                    "E_COND_DEFINED",
                    self.base_line,
                    self.cur_pos(),
                    "defined 后面缺宏名".to_string(),
                    "`defined(X)` 与 `defined X` 两种拼写都要求给出宏名".to_string(),
                    "补宏名".to_string(),
                ));
            };
            if name_tok.kind != TokenKind::Ident {
                return Err(cerr(
                    "E_COND_DEFINED",
                    self.base_line,
                    self.base_pos + name_tok.start,
                    format!("defined 后面是 {} 记号，不是宏名", name_tok.kind.name()),
                    "defined 的操作数必须是标识符".to_string(),
                    "改成标识符宏名".to_string(),
                ));
            }
            self.bump();
            let name = name_tok.text.clone();
            if paren && !self.eat_punct(")") {
                return Err(cerr(
                    "E_COND_DEFINED",
                    self.base_line,
                    self.cur_pos(),
                    "defined(X) 缺右括号".to_string(),
                    "用了圆括号拼写就必须闭合".to_string(),
                    "补右括号，或改用 `defined X` 拼写".to_string(),
                ));
            }
            return Ok(IntVal::bool(self.is_defined(&name)));
        }
        // 宏名：命中宏表 → 代入展开；未命中 → 按假（0）并显性注记。
        match self.table.lookup(&text) {
            Some(def) => {
                let (body, params) = (def.body.clone(), def.params.clone());
                self.expand_macro(&text, &body, params.as_deref(), tok)
            }
            None => {
                self.notes.push(note(
                    "N_COND_UNDEF_MACRO",
                    self.base_line,
                    self.base_pos + tok.start,
                    format!("条件表达式引用了未定义宏 {}，按假（0）处理", text),
                    "整型常量语义：未定义标识符在条件编译里求值为 0——\
但按假不等于可以无声消失".to_string(),
                    format!(
                        "若这是拼写错误请改名；若确实想按假分派，请显式写 \
`defined({0})` 让意图可见",
                        text
                    ),
                ));
                Ok(IntVal::sint(0))
            }
        }
    }

    /// 宏已定义判定（由上游 F0412 宏表登记态裁定——单一事实源）。
    fn is_defined(&self, name: &str) -> bool {
        self.table.lookup(name).is_some()
    }

    /// 宏代入求值：对象宏取宏体、函数式宏先收参再逐记号替换参数。
    fn expand_macro(
        &mut self,
        name: &str,
        body: &str,
        params: Option<&[String]>,
        site: &ETok,
    ) -> Result<IntVal, CondError> {
        if self.depth >= self.cfg.max_expand_depth {
            self.stats.expand_guard_hits += 1;
            return Err(cerr(
                "E_COND_EXPAND_DEPTH",
                self.base_line,
                self.base_pos + site.start,
                format!("宏 {} 在条件表达式里的展开深度超过上限 {}", name, self.cfg.max_expand_depth),
                "宏套宏无界展开会拖垮求值器——护栏在此拦截".to_string(),
                "拆开嵌套宏，或把展开上限调高（确认成本可接受）".to_string(),
            ));
        }
        let expanded: Vec<ETok>;
        match params {
            None => {
                self.stats.macro_expansions += 1;
                expanded = lex_expr(body, self.base_line, self.base_pos)?;
            }
            Some(ps) => {
                if !self.eat_punct("(") {
                    return Err(cerr(
                        "E_COND_FUNC_NO_CALL",
                        self.base_line,
                        self.base_pos + site.start,
                        format!("函数式宏 {} 在条件表达式里缺调用括号", name),
                        "函数式宏只能以 `宏(实参...)` 形态参与求值".to_string(),
                        "补调用括号与实参".to_string(),
                    ));
                }
                let mut args: Vec<Vec<ETok>> = Vec::new();
                if !self.eat_punct(")") {
                    loop {
                        args.push(self.collect_arg()?);
                        if self.eat_punct(",") {
                            continue;
                        }
                        if self.eat_punct(")") {
                            break;
                        }
                        return Err(cerr(
                            "E_COND_FUNC_ARGS",
                            self.base_line,
                            self.cur_pos(),
                            format!("宏 {} 的实参表未正常闭合", name),
                            "实参之间用逗号分隔，整体用圆括号闭合".to_string(),
                            "补逗号或右括号".to_string(),
                        ));
                    }
                }
                if args.len() != ps.len() {
                    return Err(cerr(
                        "E_COND_ARITY",
                        self.base_line,
                        self.base_pos + site.start,
                        format!(
                            "宏 {} 收 {} 个参数但给了 {} 个实参",
                            name,
                            ps.len(),
                            args.len()
                        ),
                        "参数个数不符会让替换结果依赖错位——拒绝错位".to_string(),
                        format!(
                            "按 {} 的参数表给实参，或改用对象宏",
                            ps.join(", ")
                        ),
                    ));
                }
                self.stats.macro_expansions += 1;
                let body_toks = lex_expr(body, self.base_line, self.base_pos)?;
                let mut sub: Vec<ETok> = Vec::new();
                for t in body_toks {
                    let is_param = t.kind == TokenKind::Ident
                        && ps.iter().any(|p| p.as_str() == t.text);
                    if is_param {
                        let idx = ps
                            .iter()
                            .position(|p| p.as_str() == t.text)
                            .unwrap_or(0);
                        if let Some(a) = args.get(idx) {
                            sub.extend(a.iter().cloned());
                        }
                    } else {
                        sub.push(t);
                    }
                }
                expanded = sub;
            }
        }
        self.depth += 1;
        let mut sub = ExprEval {
            toks: expanded,
            i: 0,
            table: self.table,
            cfg: self.cfg,
            base_line: self.base_line,
            base_pos: self.base_pos,
            depth: self.depth,
            notes: Vec::new(),
            stats: CondStats::default(),
        };
        let v = sub.parse_all();
        self.depth -= 1;
        self.notes.extend(sub.notes);
        self.stats.macro_expansions += sub.stats.macro_expansions;
        self.stats.expr_tokens += sub.stats.expr_tokens;
        self.stats.expand_guard_hits += sub.stats.expand_guard_hits;
        v
    }

    /// 收一个实参（按逗号与右括号切分，括号内逗号不切）。
    fn collect_arg(&mut self) -> Result<Vec<ETok>, CondError> {
        let mut depth = 0usize;
        let start = self.i;
        loop {
            match self.peek().cloned() {
                None => {
                    return Err(cerr(
                        "E_COND_FUNC_ARGS",
                        self.base_line,
                        self.cur_pos(),
                        "实参表在表达式结束处仍未闭合".to_string(),
                        "实参必须以右括号收尾".to_string(),
                        "补右括号".to_string(),
                    ))
                }
                Some(t) => {
                    if t.kind == TokenKind::Punct {
                        match t.text.as_str() {
                            "(" => depth += 1,
                            ")" if depth == 0 => {
                                let arg: Vec<ETok> = self.toks[start..self.i].to_vec();
                                return Ok(arg);
                            }
                            ")" => depth -= 1,
                            "," if depth == 0 => {
                                let arg: Vec<ETok> = self.toks[start..self.i].to_vec();
                                return Ok(arg);
                            }
                            _ => {}
                        }
                    }
                    self.i += 1;
                }
            }
        }
    }

    /// 二元运算落地（含除零、溢出、移位越界的显性报错）。
    fn apply_binary(&mut self, op: &str, l: IntVal, r: IntVal) -> Result<IntVal, CondError> {
        let unsigned = l.unsigned || r.unsigned;
        // 逻辑算子：短路语义在 parse 层之外做（此处直接算，全族齐备即可）。
        match op {
            "||" => return Ok(IntVal::bool(l.truthy() || r.truthy())),
            "&&" => return Ok(IntVal::bool(l.truthy() && r.truthy())),
            _ => {}
        }
        // 移位：位移量越界显性报错（不静默回绕）。
        if op == "<<" || op == ">>" {
            let width = if unsigned { 128 } else { 127 };
            if r.v < 0 || r.v >= width as i128 {
                return Err(cerr(
                    "E_COND_SHIFT_RANGE",
                    self.base_line,
                    self.base_pos,
                    format!("移位量 {} 越界（合法域 0..{}）", r.v, width - 1),
                    "移位量越界的行为未定义——显式报错不回绕".to_string(),
                    "把移位量夹到合法域，或改写为乘除形式".to_string(),
                ));
            }
            let sh = r.v as u32;
            let v = if op == "<<" {
                if unsigned {
                    ((l.v as u128) << sh) as i128
                } else {
                    l.v.checked_shl(sh).unwrap_or(0)
                }
            } else if unsigned {
                ((l.v as u128) >> sh) as i128
            } else {
                l.v.checked_shr(sh).unwrap_or(if l.v < 0 { -1 } else { 0 })
            };
            return Ok(IntVal { v, unsigned });
        }
        // 等式与关系：整型比较。
        match op {
            "==" => return Ok(IntVal::bool(l.v == r.v)),
            "!=" => return Ok(IntVal::bool(l.v != r.v)),
            _ => {}
        }
        let cmp = if unsigned {
            match (l.v as u128).cmp(&(r.v as u128)) {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        } else {
            match l.v.cmp(&r.v) {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            }
        };
        match op {
            "<" => return Ok(IntVal::bool(cmp < 0)),
            ">" => return Ok(IntVal::bool(cmp > 0)),
            "<=" => return Ok(IntVal::bool(cmp <= 0)),
            ">=" => return Ok(IntVal::bool(cmp >= 0)),
            _ => {}
        }
        // 算术：除模与溢出显性报错。
        let v = match op {
            "+" => l.v.checked_add(r.v),
            "-" => l.v.checked_sub(r.v),
            "*" => l.v.checked_mul(r.v),
            "/" => {
                if r.v == 0 {
                    return Err(cerr(
                        "E_COND_DIV_ZERO",
                        self.base_line,
                        self.base_pos,
                        "条件表达式里除数为零".to_string(),
                        "除零无定义——条件期求值必须显性拒绝".to_string(),
                        "加非零守卫（如 `#if D != 0 && X / D > 1`）".to_string(),
                    ));
                }
                if unsigned {
                    Some(((l.v as u128) / (r.v as u128)) as i128)
                } else {
                    l.v.checked_div(r.v)
                }
            }
            "%" => {
                if r.v == 0 {
                    return Err(cerr(
                        "E_COND_MOD_ZERO",
                        self.base_line,
                        self.base_pos,
                        "条件表达式里模数为零".to_string(),
                        "模零无定义——条件期求值必须显性拒绝".to_string(),
                        "加非零守卫".to_string(),
                    ));
                }
                if unsigned {
                    Some(((l.v as u128) % (r.v as u128)) as i128)
                } else {
                    l.v.checked_rem(r.v)
                }
            }
            "&" => Some(l.v & r.v),
            "|" => Some(l.v | r.v),
            "^" => Some(l.v ^ r.v),
            _ => None,
        };
        match v {
            Some(v) => Ok(IntVal { v, unsigned }),
            None => Err(cerr(
                "E_COND_OVERFLOW",
                self.base_line,
                self.base_pos,
                format!("条件表达式的 {} 运算溢出", op),
                "整型常量在 i128 域内仍会溢出——溢出静默回绕会让分支选错"
                    .to_string(),
                "缩小运算量级，或改写为不需要大值的比较式".to_string(),
            )),
        }
    }
}

/// 二元算符优先级（层号越大越紧；`?:` 不在此表——它由三元层单独处理）。
fn binary_prec(op: &str) -> Option<u8> {
    let p = match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    };
    Some(p)
}

// ---------------------------------------------------------------------------
// 五、求值主流程（条件栈推进 + 分支选择 + 跳过段记录）
// ---------------------------------------------------------------------------

/// 条件编译求值结果（保留段 + 跳过段 + 分支记录 + 注记 + 统计）。
#[derive(Clone, Debug, PartialEq)]
pub struct CondOutcome {
    pub kept: Vec<KeptLine>,
    pub skipped: Vec<SkipRegion>,
    pub branches: Vec<BranchRecord>,
    pub notes: Vec<CondNote>,
    pub stats: CondStats,
    /// 最终条件栈是否清空（false = 有未闭合层，报错路径已拦）。
    pub balanced: bool,
}

impl CondOutcome {
    /// 生效代码行文本（下游 include 协同与构建系统的消费口）。
    pub fn active_text(&self) -> String {
        self.kept
            .iter()
            .map(|k| k.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 跳过段总字节（快扫面计量）。
    pub fn skipped_bytes(&self) -> usize {
        self.skipped
            .iter()
            .map(|s| s.end_pos.saturating_sub(s.start_pos))
            .sum()
    }
}

/// 求值器状态（条件栈 + 产出；判据：栈 O(1) 压弹）。
struct Evaluator {
    frames: Vec<CondFrame>,
    out: CondOutcome,
    /// 当前是否生效（= 栈顶层生效；空栈 = 顶层生效）。
    active: bool,
    /// 当前跳过原因（`None` = 不在跳过中）。
    skipping: Option<SkipReason>,
    /// 当前跳过段起点（字节跨度累积中）。
    skip_start: Option<(usize, usize)>,
    /// 游标：最近处理到的行号与字节偏移（跳过段跨度记账用）。
    cur_line: usize,
    cur_pos: usize,
}

impl Evaluator {
    fn new() -> Evaluator {
        Evaluator {
            frames: Vec::new(),
            out: CondOutcome {
                kept: Vec::new(),
                skipped: Vec::new(),
                branches: Vec::new(),
                notes: Vec::new(),
                stats: CondStats::default(),
                balanced: true,
            },
            active: true,
            skipping: None,
            skip_start: None,
            cur_line: 0,
            cur_pos: 0,
        }
    }

    fn depth(&self) -> usize {
        self.frames.len()
    }

    /// 开一层（`#if` / `#ifdef` / `#ifndef`）。
    fn push_frame(
        &mut self,
        kind: DirectiveKind,
        line: usize,
        pos: usize,
        text: &str,
        cond_true: bool,
        deferred: bool,
    ) {
        let parent_active = self.active;
        let state = if deferred {
            BranchState::Done
        } else if cond_true {
            BranchState::Taken
        } else {
            BranchState::Seeking
        };
        self.frames.push(CondFrame {
            kind,
            state,
            parent_active,
            open_line: line,
            open_pos: pos,
            open_text: text.to_string(),
            seen_else: false,
            branches: 1,
        });
        if self.frames.len() > self.out.stats.peak_depth {
            self.out.stats.peak_depth = self.frames.len();
        }
        self.active = parent_active && state == BranchState::Taken;
        self.sync_skipping();
    }

    /// 弹一层（`#endif`）。
    fn pop_frame(&mut self) {
        self.frames.pop();
        self.active = match self.frames.last() {
            Some(f) => f.active(),
            None => true,
        };
        self.sync_skipping();
    }

    /// 生效态 ↔ 跳过态切换时开/合跳过段跨度（掠过 O(段长) 快扫的落点）。
    ///
    /// 跨度以**字节偏移**记账：下游（F0414 include 协同、构建系统）按跨度
    /// 直接定位，无须重扫被跳过的原文。
    ///
    /// 跨度在**跳过原因变化**时切开——外层跳过的段与内层"已有真分支"的段
    /// 归并成一条会让原因失真。切开即多一条记录，成本 O(分支数)。
    fn sync_skipping(&mut self) {
        if self.active {
            self.close_skip_span();
            self.skipping = None;
            return;
        }
        let reason = current_reason(&self.frames);
        if self.skipping != Some(reason) {
            self.close_skip_span();
            self.skipping = Some(reason);
            self.skip_start = Some((self.cur_line, self.cur_pos));
            return;
        }
        if self.skip_start.is_none() {
            self.skip_start = Some((self.cur_line, self.cur_pos));
        }
    }

    /// 合上当前跳过段跨度（幂等：无跨度开则无事）。
    fn close_skip_span(&mut self) {
        if let Some((sl, sp)) = self.skip_start {
            let reason = self.skipping.unwrap_or(SkipReason::BranchFalse);
            self.out.skipped.push(SkipRegion {
                start_line: sl,
                start_pos: sp,
                end_line: self.cur_line,
                end_pos: self.cur_pos,
                depth: self.frames.len(),
                reason,
            });
            self.skip_start = None;
        }
    }

    /// 游标推进（跳过态下累加快扫字节面）。
    fn advance_cursor(&mut self, line: usize, pos: usize, bytes: usize) {
        self.cur_line = line;
        self.cur_pos = pos.saturating_add(bytes);
        if !self.active {
            self.out.stats.skipped_bytes += bytes;
        }
    }
}

/// 当前跳过原因（父层跳过 / 本分支为假 / 已有真分支——显性可查）。
fn current_reason(frames: &[CondFrame]) -> SkipReason {
    match frames.last() {
        Some(f) if !f.parent_active => SkipReason::ParentSkipped,
        Some(f) if f.state == BranchState::Done => SkipReason::AlreadyTaken,
        _ => SkipReason::BranchFalse,
    }
}

// ---------------------------------------------------------------------------
// 六、主入口
// ---------------------------------------------------------------------------

/// 条件编译求值主入口（消费上游 F0411 双流与 F0412 宏表）。
///
/// **显性声明**：被跳过的段只做词法跨度累加——不求值、不报语义错。
pub fn evaluate_conditionals(
    streams: &PreproStreams,
    table: &MacroTable,
    cfg: &CondConfig,
) -> Result<CondOutcome, CondError> {
    let mut ev = Evaluator::new();

    // 双流按原始行序归并（两路各自有序——线性归并 O(n)）。
    let mut di = 0usize;
    let mut ci = 0usize;
    loop {
        let take_dir = match (streams.directives.get(di), streams.code.get(ci)) {
            (Some(d), Some(c)) => d.line <= c.line,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_dir {
            let d = &streams.directives[di];
            ev.advance_cursor(d.line, d.pos, d.body.len() + d.name.len() + 1);
            handle_directive(&mut ev, d, table, cfg)?;
            di += 1;
        } else {
            let c = &streams.code[ci];
            ev.advance_cursor(c.line, c.pos, c.text.len());
            handle_code(&mut ev, c);
            ci += 1;
        }
    }

    // 收尾：未闭合逐层报错（判据：未闭合报错指向各层）。
    if !ev.frames.is_empty() {
        let layers: Vec<OpenLayer> = ev
            .frames
            .iter()
            .map(|f| OpenLayer {
                kind: f.kind,
                line: f.open_line,
                pos: f.open_pos,
                text: f.open_text.clone(),
            })
            .collect();
        let deepest = layers.last().cloned().unwrap_or_else(|| OpenLayer {
            kind: DirectiveKind::If,
            line: 0,
            pos: 0,
            text: String::new(),
        });
        let mut e = cerr(
            "E_COND_UNCLOSED",
            deepest.line,
            deepest.pos,
            format!(
                "条件编译未闭合：还有 {} 层没有 #endif",
                layers.len()
            ),
            "每条 #if / #ifdef / #ifndef 都必须配一条 #endif——\
未闭合让后续整段归属不明".to_string(),
            format!(
                "补 {} 条 #endif（未闭合链：{}）",
                layers.len(),
                layers
                    .iter()
                    .map(|l| format!("第 {} 行 #{}", l.line, l.kind.name()))
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
        );
        e.layers = layers;
        return Err(e);
    }

    // 收尾把还开着的跳过段合上（输入末尾仍在跳过区内）。
    ev.close_skip_span();
    ev.out.balanced = true;
    Ok(ev.out)
}

/// 源码直入口（先做双流分流——上游 F0411 单一实现，再做条件求值）。
pub fn evaluate_source(
    src: &str,
    table: &MacroTable,
    cfg: &CondConfig,
) -> Result<CondOutcome, CondError> {
    let streams = super::vec11_prepro::split_streams(src)
        .map_err(|e| cerr("E_COND_UPSTREAM", e.line, e.pos, e.what, e.why, e.next))?;
    evaluate_conditionals(&streams, table, cfg)
}

/// 指令处理（`#if` 族六指令 + 其余透传）。
fn handle_directive(
    ev: &mut Evaluator,
    d: &DirectiveLine,
    table: &MacroTable,
    cfg: &CondConfig,
) -> Result<(), CondError> {
    match d.kind {
        DirectiveKind::If | DirectiveKind::Ifdef | DirectiveKind::Ifndef => {
            let depth = ev.depth();
            if depth >= cfg.max_depth {
                let mut layers: Vec<OpenLayer> = ev
                    .frames
                    .iter()
                    .map(|f| OpenLayer {
                        kind: f.kind,
                        line: f.open_line,
                        pos: f.open_pos,
                        text: f.open_text.clone(),
                    })
                    .collect();
                layers.push(OpenLayer {
                    kind: d.kind,
                    line: d.line,
                    pos: d.pos,
                    text: d.body.clone(),
                });
                let mut e = cerr(
                    "E_COND_DEPTH",
                    d.line,
                    d.pos,
                    format!(
                        "条件编译嵌套深度 {} 超过上限 {}",
                        depth + 1,
                        cfg.max_depth
                    ),
                    "病态深嵌套让求值器承担无意义的栈深——护栏在此拦截"
                        .to_string(),
                    format!("减少嵌套层数，或把上限调高（链：{}）", {
                        layers
                            .iter()
                            .map(|l| format!("第 {} 行", l.line))
                            .collect::<Vec<_>>()
                            .join(" → ")
                    }),
                );
                e.layers = layers;
                return Err(e);
            }
            // 父层已跳过 → 本层整层跳过（不求值：跳过段只做词法）。
            let deferred = ev.skipping.is_some();
            let cond_true = if deferred {
                ev.out.stats.exprs_skipped += 1;
                false
            } else {
                eval_condition(&d.kind, &d.body, d.line, d.pos, table, cfg, ev)?
            };
            let text = format!("#{} {}", d.kind.name(), d.body);
            ev.push_frame(d.kind, d.line, d.pos, &text, cond_true, deferred);
            record_branch(ev, d.kind, d.line, d.pos, &d.body, ev.frames.last().map(|f| f.state).unwrap_or(BranchState::Done), depth);
            Ok(())
        }
        DirectiveKind::Elif | DirectiveKind::Else => {
            if ev.frames.is_empty() {
                return Err(cerr(
                    "E_COND_ORPHAN",
                    d.line,
                    d.pos,
                    format!("第 {} 行出现 #{} 但没有对应的 #if", d.line, d.kind.name()),
                    "分支指令只能出现在 #if 族之内".to_string(),
                    "补开指令，或删掉这条孤立分支指令".to_string(),
                ));
            }
            let depth = ev.depth() - 1;
            let seen_else = ev.frames.last().map(|f| f.seen_else).unwrap_or(false);
            if d.kind == DirectiveKind::Else && seen_else {
                let opener = ev.frames.last().map(|f| f.open_line).unwrap_or(0);
                return Err(cerr(
                    "E_COND_DOUBLE_ELSE",
                    d.line,
                    d.pos,
                    format!(
                        "第 {} 行的 #else 是第 {} 行开指令里的第二条 #else",
                        d.line, opener
                    ),
                    "一个 #if 族只能有一条 #else——多重 else 令分支集不唯一"
                        .to_string(),
                    "删掉多余的 #else，或拆成两个并列的 #if 族".to_string(),
                ));
            }
            if d.kind == DirectiveKind::Elif && seen_else {
                let opener = ev.frames.last().map(|f| f.open_line).unwrap_or(0);
                return Err(cerr(
                    "E_COND_ELIF_AFTER_ELSE",
                    d.line,
                    d.pos,
                    format!(
                        "第 {} 行的 #elif 出现在第 {} 行开指令的 #else 之后",
                        d.line, opener
                    ),
                    "#else 是分支链的末项——其后不再接受 #elif".to_string(),
                    "把 #elif 提到 #else 之前".to_string(),
                ));
            }
            let parent_active = ev.frames.last().map(|f| f.parent_active).unwrap_or(true);
            let seeking = ev.frames.last().map(|f| f.state == BranchState::Seeking).unwrap_or(false);
            let new_state = if !parent_active || !seeking {
                // 不求值：外层跳过，或已有真分支（跳过段只做词法）。
                ev.out.stats.exprs_skipped += 1;
                BranchState::Done
            } else if d.kind == DirectiveKind::Else {
                BranchState::Taken
            } else {
                let t = eval_condition(
                    &d.kind,
                    &d.body,
                    d.line,
                    d.pos,
                    table,
                    cfg,
                    ev,
                )?;
                if t {
                    BranchState::Taken
                } else {
                    BranchState::Seeking
                }
            };
            if let Some(f) = ev.frames.last_mut() {
                f.state = new_state;
                f.branches += 1;
                if d.kind == DirectiveKind::Else {
                    f.seen_else = true;
                }
            }
            ev.active = parent_active && new_state == BranchState::Taken;
            ev.sync_skipping();
            record_branch(ev, d.kind, d.line, d.pos, &d.body, new_state, depth);
            Ok(())
        }
        DirectiveKind::Endif => {
            if ev.frames.is_empty() {
                return Err(cerr(
                    "E_COND_ORPHAN_ENDIF",
                    d.line,
                    d.pos,
                    format!("第 {} 行出现 #endif 但没有对应的开指令", d.line),
                    "每条 #endif 必须配一条 #if / #ifdef / #ifndef".to_string(),
                    "删掉这条 #endif，或补上它对应的开指令".to_string(),
                ));
            }
            ev.pop_frame();
            Ok(())
        }
        // 非条件指令：生效段透传（由 F0412 宏表 / F0414 include 消费）；
        // 跳过段只做词法——不参与求值。
        _ => Ok(()),
    }
}

/// 代码行处理（生效则保留，跳过则累加跨度）。
fn handle_code(ev: &mut Evaluator, c: &CodeLine) {
    if ev.active {
        ev.out.kept.push(KeptLine {
            line: c.line,
            pos: c.pos,
            text: c.text.clone(),
            depth: ev.depth(),
        });
    }
}

/// 分支记录入表（求值结果记录——下游快扫与诊断消费）。
fn record_branch(
    ev: &mut Evaluator,
    kind: DirectiveKind,
    line: usize,
    pos: usize,
    expr: &str,
    state: BranchState,
    depth: usize,
) {
    ev.out.branches.push(BranchRecord {
        kind,
        line,
        pos,
        state,
        expr: expr.to_string(),
        depth,
    });
}

/// 单条条件求值（`#if` 表达式 / `#ifdef` 宏名 / `#ifndef` 宏名）。
#[allow(clippy::too_many_arguments)]
fn eval_condition(
    kind: &DirectiveKind,
    body: &str,
    line: usize,
    pos: usize,
    table: &MacroTable,
    cfg: &CondConfig,
    ev: &mut Evaluator,
) -> Result<bool, CondError> {
    let trimmed = body.trim();
    match kind {
        DirectiveKind::Ifdef | DirectiveKind::Ifndef => {
            let name = trimmed.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                return Err(cerr(
                    "E_COND_NO_MACRO_NAME",
                    line,
                    pos,
                    format!("第 {} 行的 #{} 缺宏名", line, kind.name()),
                    format!("#{} 的操作数必须是标识符宏名", kind.name()),
                    "补宏名".to_string(),
                ));
            }
            let defined = table.lookup(name).is_some();
            let want = match kind {
                DirectiveKind::Ifdef => defined,
                _ => !defined,
            };
            ev.out.stats.exprs_evaluated += 1;
            // 未定义宏在 ifdef 语境下是正常语义，不注记（意图本就显式）。
            Ok(want)
        }
        _ => {
            if trimmed.is_empty() {
                return Err(cerr(
                    "E_COND_NO_EXPR",
                    line,
                    pos,
                    format!("第 {} 行的 #{} 后缺表达式", line, kind.name()),
                    "整型常量表达式是必需的".to_string(),
                    "补表达式，或改用 #ifdef / #ifndef".to_string(),
                ));
            }
            ev.out.stats.exprs_evaluated += 1;
            let mut ee = ExprEval::new(trimmed, table, cfg, line, pos)?;
            let v = ee.parse_all()?;
            ev.out.stats.expr_tokens += ee.stats.expr_tokens;
            ev.out.stats.macro_expansions += ee.stats.macro_expansions;
            ev.out.stats.expand_guard_hits += ee.stats.expand_guard_hits;
            if cfg.report_skipped_notes {
                ev.out.notes.extend(ee.notes);
            }
            Ok(v.truthy())
        }
    }
}

/// VE-F0413 域自检入口（判据逐条对应，见 `vec13_checks.rs`）。
pub fn run_vec13_checks() -> crate::checks::CheckSet {
    super::vec13_checks::run_vec13_checks()
}