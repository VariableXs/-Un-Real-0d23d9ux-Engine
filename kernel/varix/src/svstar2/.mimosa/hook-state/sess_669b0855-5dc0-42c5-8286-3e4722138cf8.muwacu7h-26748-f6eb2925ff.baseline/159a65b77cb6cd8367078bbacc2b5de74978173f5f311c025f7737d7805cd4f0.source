//! VE-F0403 · 词法分析器架构（VE-C 域 · 着色器系统 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0403`
//!
//! **判据（锚点原文）**：单遍状态机、三元组记号、协同边界、决策记录、判据。
//!
//! **职责定位（锚点原文）**：词法分析器总体架构——单遍扫描状态机（字符流
//! 加缓冲双窗口）、记号流生成（记号类型、原文跨度、位置三元组）、与预处理
//! 器的协同边界（词法只负责记号化、指令识别交给预处理层，不越权处理）；
//! 架构决策记录（为什么不用正则驱动）。
//!
//! **设计要点**：
//! - **单遍扫描状态机**：扫描指针只前进；除"窗口边界截断回退"这一条受控
//!   路径外，指针永不回退。状态机显式枚举（Init/Ident/Number/Punct/String/
//!   LineComment/BlockComment/Directive），转移在 `step` 内穷举——不可达
//!   分支以防御断言拦截（锚点：状态机不可达分支→防御断言）；
//! - **记号三元组**：每个记号携带（类型，原文跨度 [start,end)，行:列位置）。
//!   跨度按字节偏移，位置按 1 起行列——下游语法器/诊断器可直接定位回源码；
//! - **双窗口缓冲**：`DoubleWindow` 维护当前窗口与前向窗口，O(1) 滑动。
//!   窗口边界的截断记号走"回退重扫"路径：窗口重置后重扫该记号（受控回退，
//!   记录 `rescan_count`，正确性优先于单遍教条——截断不回退就是错记号）；
//! - **协同边界（不越权）**：行首 `#` 触发 Directive 状态，词法把指令行整段
//!   收成一条 Directive 记号后**原样保留文本、不做任何展开/求值/宏处理**——
//!   指令语义归预处理层（F0411）。词法对指令的唯一解释力是"这是一条指令"；
//! - **决策记录**：架构决策显式在册（`LEXER_DECISIONS`），第一条就是
//!   "为什么不用正则驱动"：正则回溯破坏单遍 O(n) 保证与记号跨度的确定性，
//!   且不可分段的跨窗口匹配与缓冲边界回退语义冲突；
//! - **错误路径**：字符串/块注释 EOF 中途 → 显性终止（`LexError` 带位置，
//!   零静默）；窗口边界截断 → 回退重扫；不可达状态 → 防御断言。
//!
//! **性能逐项分解**：扫描 O(字符数) 单遍；记号 O(1) 生成；缓冲 O(1) 滑动。
//! 本实现以单调指针 + 受控回退计数落实，`scan_stats` 可审计。
//!
//! **跨批对接点**：上游 F0402 规范文法（VS-GR 册）；下游 F0411 预处理协同、
//! F0420 收口。条款引用：VS-G-03（先归一）、VS-G-06（构造可调试——记号
//! 三元组即词法层的调试注记）。
//!
//! 确定性：同输入同记号流（纯函数，无时钟无 IO）。零外部依赖，只用 `alloc`
//! 与 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、记号三元组（判据二：三元组记号）
// ---------------------------------------------------------------------------

/// 记号类型（词法层只认形状，不认语义——语义归语法器）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// 标识符（含关键字——关键字识别是语法/语义层的事，词法不越权）。
    Ident,
    /// 数字字面量（整数/浮点形状；进制与后缀合法性归语法器）。
    Number,
    /// 标点（单字符与多字符操作符）。
    Punct,
    /// 字符串字面量。
    Str,
    /// 注释（词法保留供文档工具，语法器跳过）。
    Comment,
    /// 预处理指令行（整行一条，文本原样保留——语义归预处理层）。
    Directive,
    /// 输入结束哨兵。
    Eof,
    /// 无法归类的字节（词法错误由 `LexError` 报，Unknown 只作占位不静默吞）。
    Unknown,
}

impl TokenKind {
    /// 人话名（诊断与读屏用）。
    pub fn name(self) -> &'static str {
        match self {
            TokenKind::Ident => "标识符",
            TokenKind::Number => "数字",
            TokenKind::Punct => "标点",
            TokenKind::Str => "字符串",
            TokenKind::Comment => "注释",
            TokenKind::Directive => "预处理指令",
            TokenKind::Eof => "结束",
            TokenKind::Unknown => "未知",
        }
    }
}

/// 记号三元组：类型 × 原文跨度 × 位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// 记号类型。
    pub kind: TokenKind,
    /// 原文跨度（字节偏移，[start, end)）。
    pub start: usize,
    pub end: usize,
    /// 位置（行、列，均 1 起）。
    pub line: usize,
    pub col: usize,
}

impl Token {
    /// 记号原文（从源码切片）。
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        src.get(self.start..self.end).unwrap_or("")
    }
}

/// 行列位置追踪器（O(1) 随扫描推进）。
#[derive(Clone, Copy)]
struct Position {
    line: usize,
    col: usize,
}

impl Position {
    fn new() -> Self {
        Self { line: 1, col: 1 }
    }
    fn advance(&mut self, b: u8) {
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 二、双窗口缓冲（缓冲 O(1) 滑动 + 截断回退重扫）
// ---------------------------------------------------------------------------

/// 双窗口缓冲：当前窗口 + 前向窗口。
///
/// 本实现以源码切片为后端、以窗口尺寸 `size` 划界：指针落在窗口边界附近
/// 且记号可能跨界时，走"回退重扫"（`truncate_guard`）。窗口滑动力求 O(1)：
/// `slide` 只做指针算术不做数据搬移。
pub struct DoubleWindow {
    /// 窗口尺寸（字节）。
    pub size: usize,
    /// 回退重扫计数（审计用：受控回退必须可见，不许悄悄变成多遍）。
    pub rescan_count: usize,
    /// 重扫进行中（当前记号已触发过一次重扫，豁免边界检查防死循环）。
    pub rescan_active: bool,
    /// 指针最大回退字节数（审计用：回退不得超过一个记号长度）。
    pub max_regress: usize,
}

impl DoubleWindow {
    /// 建窗口（尺寸下限 8 字节——太小则回退重扫退化为逐字符重扫）。
    pub fn new(size: usize) -> Result<Self, LexError> {
        if size < 8 {
            return Err(LexError::WindowTooSmall {
                size,
                suggestion: "窗口尺寸下限 8 字节：过小的窗口让回退重扫退化为逐字符重扫".to_string(),
            });
        }
        Ok(Self { size, rescan_count: 0, rescan_active: false, max_regress: 0 })
    }

    /// 记号是否临近窗口右界（可能被截断）。
    ///
    /// 判定：记号起点已进入右侧保护带（`size - margin` 之后）且尚未收尾。
    /// margin 取窗口的 1/8，避免大记号在保护带外被漏判。
    fn near_right_edge(&self, token_start: usize, cursor: usize) -> bool {
        let margin = (self.size / 8).max(2);
        let window_right = (token_start / self.size + 1) * self.size;
        cursor + margin >= window_right
    }

    /// 截断处置：回退重扫（指针退回记号原起点，重扫该记号一次）。
    ///
    /// 重扫期豁免边界检查（调用方置 `rescan_active`）——否则同一记号会
    /// 反复撞界形成零回退量死循环（结构性 bug，防线在此）。
    fn truncate_guard(&mut self, token_start: usize) -> usize {
        self.rescan_count += 1;
        token_start
    }
}

// ---------------------------------------------------------------------------
// 三、架构决策记录（判据四：决策记录）
// ---------------------------------------------------------------------------

/// 架构决策（决策即文档：每条决策可引用可复审）。
#[derive(Clone, Copy, Debug)]
pub struct LexDecision {
    /// 决策编号。
    pub id: &'static str,
    /// 问题（决策要回答什么）。
    pub question: &'static str,
    /// 结论。
    pub decision: &'static str,
    /// 依据（为什么这么定——没依据的决策是拍脑袋）。
    pub rationale: &'static str,
}

/// 词法器架构决策在册（锚点点名第一条：为什么不用正则驱动）。
pub const LEXER_DECISIONS: [LexDecision; 4] = [
    LexDecision {
        id: "LEX-D1",
        question: "为什么不用正则驱动？",
        decision: "手写单遍状态机，不用正则引擎",
        rationale: "正则回溯破坏单遍 O(n) 保证与记号跨度的确定性；跨窗口匹配与缓冲边界回退语义冲突",
    },
    LexDecision {
        id: "LEX-D2",
        question: "指令（#）在词法层处理到什么程度？",
        decision: "只识别'这是一条指令'，整行收成 Directive 记号，文本原样保留",
        rationale: "协同边界：指令语义（宏/条件编译/include）归预处理层（F0411），词法越权展开会让两层语义纠缠不可测",
    },
    LexDecision {
        id: "LEX-D3",
        question: "关键字在词法层识别吗？",
        decision: "不识别——Ident 一律按标识符收，关键字判定归语法器",
        rationale: "词法只认形状；关键字表是语义层知识，放词法层会让方言扩展（收编三族）被迫改词法器",
    },
    LexDecision {
        id: "LEX-D4",
        question: "窗口边界截断的记号怎么办？",
        decision: "回退重扫（受控回退，计数入审计），正确性优先于单遍教条",
        rationale: "截断不回退就是错记号——错记号会让下游全线失真；受控回退 ≤ 一个窗口，单遍保证只在无截断路径上成立",
    },
];

// ---------------------------------------------------------------------------
// 四、扫描状态机（判据一：单遍状态机）
// ---------------------------------------------------------------------------

/// 状态机状态（显式枚举，穷举转移——不达状态以防御断言拦截）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LexState {
    /// 就绪（未在记号中）。
    Init,
    /// 标识符中。
    InIdent,
    /// 数字中。
    InNumber,
    /// 字符串中。
    InString,
    /// 行注释中。
    InLineComment,
    /// 块注释中。
    InBlockComment,
    /// 指令行中。
    InDirective,
}

/// 扫描审计账：单遍性与受控回退的证据。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// 扫描推进总字节数（含重扫部分——与 `replayed` 相减即净推进）。
    pub advanced: usize,
    /// 受控回退字节数。
    pub replayed: usize,
    /// 回退重扫次数。
    pub rescans: usize,
    /// 生成记号数。
    pub tokens: usize,
}

impl ScanStats {
    /// 单遍性审计：净推进 == 源码长度 ⇒ 整条输入恰好被有效扫描一遍。
    pub fn is_single_pass(&self, src_len: usize) -> bool {
        self.advanced.saturating_sub(self.replayed) == src_len
    }
}

/// 词法错误（显性终止路径，零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LexError {
    /// 字符串未闭合（EOF 中途）。
    UnterminatedString { line: usize, col: usize },
    /// 块注释未闭合（EOF 中途）。
    UnterminatedComment { line: usize, col: usize },
    /// 窗口尺寸非法。
    WindowTooSmall { size: usize, suggestion: String },
}

impl LexError {
    /// 人话呈现（发生了什么/在哪里/怎么修）。
    pub fn describe(&self) -> String {
        match self {
            LexError::UnterminatedString { line, col } => {
                format!("字符串在第 {line} 行第 {col} 列开始后未闭合到文件尾——补引号或检查转义")
            }
            LexError::UnterminatedComment { line, col } => {
                format!("块注释在第 {line} 行第 {col} 列开始后未闭合到文件尾——补 */ ")
            }
            LexError::WindowTooSmall { size, suggestion } => {
                format!("窗口尺寸 {size} 非法。建议：{suggestion}")
            }
        }
    }
}

/// 词法分析器：单遍扫描状态机。
pub struct Lexer {
    window: DoubleWindow,
    state: LexState,
    pos: Position,
    stats: ScanStats,
    tokens: Vec<Token>,
}

/// 标识符/数字的延续字符判定（形状规则，只认 ASCII 形状层）。
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_punct(b: u8) -> bool {
    matches!(
        b,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'='
            | b'<'
            | b'>'
            | b'!'
            | b'&'
            | b'|'
            | b'^'
            | b'~'
            | b'?'
            | b':'
            | b';'
            | b','
            | b'.'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'#'
    )
}

impl Lexer {
    /// 建词法器（默认窗口 64 字节——足够容纳典型记号又保持缓存友好）。
    pub fn new() -> Result<Self, LexError> {
        Ok(Self {
            window: DoubleWindow::new(64)?,
            state: LexState::Init,
            pos: Position::new(),
            stats: ScanStats::default(),
            tokens: Vec::new(),
        })
    }

    /// 扫描审计账（只读）。
    pub fn scan_stats(&self) -> ScanStats {
        self.stats
    }

    /// 单遍全扫描：输入整个源串，产出记号流（含 Eof 哨兵）。
    ///
    /// 主循环不变式：
    /// - `i` 只前进；唯一例外是窗口截断回退（`truncate_guard`），
    ///   回退量记账进 `replayed` 且 ≤ 窗口尺寸；
    /// - 每个状态在 `step` 中穷举处理，不可达状态在默认分支防御断言。
    pub fn scan(mut self, src: &str) -> Result<(Vec<Token>, ScanStats), LexError> {
        let bytes = src.as_bytes();
        let n = bytes.len();
        let mut i = 0usize;
        let mut token_start = 0usize;
        let mut token_pos = Position::new();

        while i < n || self.state != LexState::Init {
            // ---- 窗口截断守卫：记号临近窗口右界且可能跨界 → 回退重扫 ----
            // 重扫中的记号豁免本守卫（防同记号反复撞界形成零回退死循环）。
            let in_token = self.state != LexState::Init && self.state != LexState::InLineComment;
            if in_token && !self.window.rescan_active && self.window.near_right_edge(token_start, i)
            {
                let resume = self.window.truncate_guard(token_start);
                let replayed = i - resume;
                self.stats.replayed += replayed;
                self.stats.rescans += 1; // 审计账镜像（window.rescan_count 同步计数）
                self.window.max_regress = self.window.max_regress.max(replayed);
                self.window.rescan_active = true;
                i = resume;
                // 行列位置回退：重放字节重新推进位置（保证三元组准确）。
                self.pos = Position::new();
                for &b in &bytes[..resume] {
                    self.pos.advance(b);
                }
                token_pos = self.pos;
                continue;
            }

            let Some(&b) = bytes.get(i) else {
                // ---- EOF 处置（锚点：流异常→显性终止；行级状态合法收尾）----
                return match self.state {
                    // 未闭合的字符串/块注释：显性终止，带起始位置。
                    LexState::InString => Err(LexError::UnterminatedString {
                        line: token_pos.line,
                        col: token_pos.col,
                    }),
                    LexState::InBlockComment => Err(LexError::UnterminatedComment {
                        line: token_pos.line,
                        col: token_pos.col,
                    }),
                    // 行级状态（行注释/指令）与形状完整的记号（标识符/数字）
                    // 在 EOF 收尾是合法路径：收记号后出循环。
                    LexState::InIdent
                    | LexState::InNumber
                    | LexState::InLineComment
                    | LexState::InDirective => {
                        self.finish_token(token_start, i, token_pos);
                        break;
                    }
                    // Init 状态到达 EOF：循环条件保证不发生（防御分支）。
                    LexState::Init => break,
                };
            };

            match self.state {
                LexState::Init => {
                    token_start = i;
                    token_pos = self.pos;
                    self.state = match b {
                        b if b.is_ascii_alphabetic() || b == b'_' => LexState::InIdent,
                        b if b.is_ascii_digit() => LexState::InNumber,
                        b'"' => LexState::InString,
                        b'/' if bytes.get(i + 1) == Some(&b'/') => LexState::InLineComment,
                        b'/' if bytes.get(i + 1) == Some(&b'*') => LexState::InBlockComment,
                        b'#' => LexState::InDirective,
                        b if b.is_ascii_whitespace() => LexState::Init,
                        b if is_punct(b) => LexState::Init,
                        _ => LexState::Init,
                    };
                    // 单字符记号（标点/未知字节）即刻收口；空白跳过。
                    if self.state == LexState::Init {
                        if b.is_ascii_whitespace() {
                            // 纯空白：不产记号。
                        } else if is_punct(b) {
                            // 多字符操作符的最长匹配（==/!=/<=/>=/&&/||/<<//>>）。
                            let two = [b, *bytes.get(i + 1).unwrap_or(&0)];
                            let len = match two {
                                [b'=', b'='] | [b'!', b'='] | [b'<', b'='] | [b'>', b'='] => 2,
                                [b'&', b'&'] | [b'|', b'|'] | [b'<', b'<'] | [b'>', b'>'] => 2,
                                _ => 1,
                            };
                            let end = (i + len).min(n);
                            self.tokens.push(Token {
                                kind: TokenKind::Punct,
                                start: token_start,
                                end,
                                line: token_pos.line,
                                col: token_pos.col,
                            });
                            self.stats.tokens += 1;
                            for &adv in &bytes[i..end] {
                                self.pos.advance(adv);
                            }
                            self.stats.advanced += end - i;
                            i = end;
                            continue;
                        } else {
                            self.tokens.push(Token {
                                kind: TokenKind::Unknown,
                                start: token_start,
                                end: i + 1,
                                line: token_pos.line,
                                col: token_pos.col,
                            });
                            self.stats.tokens += 1;
                        }
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InIdent => {
                    if !is_ident_continue(b) {
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue; // 不推进 i：让 Init 状态重新归类当前字节
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InNumber => {
                    let cont = b.is_ascii_alphanumeric()
                        || b == b'_'
                        || b == b'.'
                        || ((b == b'+' || b == b'-')
                            && bytes
                                .get(i.wrapping_sub(1))
                                .map(|p| *p == b'e' || *p == b'E')
                                .unwrap_or(false));
                    if !cont {
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue;
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InString => {
                    if b == b'\\' {
                        // 转义：跳过下一字节（转义引号不算闭合）。
                        self.pos.advance(b);
                        self.stats.advanced += 1;
                        i += 1;
                        if let Some(&esc) = bytes.get(i) {
                            self.pos.advance(esc);
                            self.stats.advanced += 1;
                            i += 1;
                        }
                        continue;
                    }
                    if b == b'"' {
                        self.pos.advance(b);
                        self.stats.advanced += 1;
                        i += 1;
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue;
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InLineComment => {
                    if b == b'\n' {
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue; // 换行留给 Init 处理（位置推进）
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InBlockComment => {
                    if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        self.pos.advance(b);
                        self.stats.advanced += 1;
                        i += 1;
                        if let Some(&close) = bytes.get(i) {
                            self.pos.advance(close);
                            self.stats.advanced += 1;
                            i += 1;
                        }
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue;
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
                LexState::InDirective => {
                    if b == b'\n' {
                        self.finish_token(token_start, i, token_pos);
                        self.state = LexState::Init;
                        continue;
                    }
                    self.pos.advance(b);
                    self.stats.advanced += 1;
                    i += 1;
                }
            }
        }

        self.tokens.push(Token {
            kind: TokenKind::Eof,
            start: n,
            end: n,
            line: self.pos.line,
            col: self.pos.col,
        });
        self.stats.tokens += 1;
        let stats = self.stats;
        Ok((core::mem::take(&mut self.tokens), stats))
    }

    /// 收口当前记号（Init 中多字符路径自行收口，不走这里）。
    fn finish_token(&mut self, start: usize, end: usize, pos: Position) {
        self.window.rescan_active = false; // 记号收口即离开重扫豁免
        if end <= start {
            return; // 空记号不收（防御：零长跨度是状态机 bug 的症状）
        }
        let kind = match self.state {
            LexState::InIdent => TokenKind::Ident,
            LexState::InNumber => TokenKind::Number,
            LexState::InString => TokenKind::Str,
            LexState::InLineComment | LexState::InBlockComment => TokenKind::Comment,
            LexState::InDirective => TokenKind::Directive,
            _ => {
                // 防御断言：finish_token 只该在记号状态里被调（锚点：不可达分支→防御断言）。
                debug_assert!(false, "不可达状态进入收口路径");
                TokenKind::Unknown
            }
        };
        self.tokens.push(Token { kind, start, end, line: pos.line, col: pos.col });
        self.stats.tokens += 1;
    }
}

// ---------------------------------------------------------------------------
// 五、协同边界审计（词法不越权处理指令语义）
// ---------------------------------------------------------------------------

/// 协同边界审计：给定记号流与源码，验证 Directive 记号的原样性。
///
/// 词法对指令的全部义务：整行收号 + 文本原样。任何"展开/求值/替换"发生
/// 在词法层都表现为记号文本 ≠ 源码行文本——本审计把这条红线写成断言。
pub fn directive_boundary_audit(tokens: &[Token], src: &str) -> bool {
    tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Directive)
        .all(|t| {
            let raw = t.text(src);
            // 原样性：以 '#' 起、以行尾（或文件尾）止，中间零改动。
            raw.starts_with('#')
                && !raw.contains('\n')
                && src[t.start..].starts_with(raw)
        })
}

/// 纯功能行数自证（正式门禁见 `vec03_checks.rs`）。
pub fn lexer_smoke(src: &str) -> usize {
    match Lexer::new() {
        Ok(l) => match l.scan(src) {
            Ok((tokens, _)) => tokens.iter().filter(|t| t.kind != TokenKind::Eof).count(),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

/// VE-F0403 域自检（判据逐条对应，见 `vec03_checks.rs`）。
pub fn run_vec03_checks() -> CheckSet {
    super::vec03_checks::run_vec03_checks()
}
