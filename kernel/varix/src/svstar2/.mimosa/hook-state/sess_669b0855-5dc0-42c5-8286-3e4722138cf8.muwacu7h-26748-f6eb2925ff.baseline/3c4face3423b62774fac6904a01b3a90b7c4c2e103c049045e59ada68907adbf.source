//! VE-F0410 · 括号配对与作用域标记（VE-C 域 · 着色器系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0410`
//!
//! **判据（锚点原文）**：栈检查、双侧定位、作用域直供、深度上限、判据。
//! - 三类括号（圆 `()`、方 `[]`、花 `{}`）**配对栈检查**：单程 O(字符数)；
//! - 失配报错**双侧定位**：开括号位置与当前位置同时给出（不是只报扫挂处）；
//! - **交叉嵌套**（先花后圆错序，如 `{ ( }`）→ 报错带语境建议（与普通失
//!   配分流：栈内存在同类开符但栈顶不是它 = 交叉，栈顶即最近候选 = 失配）；
//! - **栈溢出** → 深度上限报错（嵌套深度上限可配，默认 128——防护病态
//!   深嵌套拖垮符号表期）；
//! - **作用域开合标记流**：花括号的开/合事件带深度记账生成标记流，供
//!   F0422 符号表期直接消费不重扫（判据：作用域直供）；
//! - 字符串与注释内的括号不参与配对（复用 F0408 的字符串保护语义，
//!   一份实现零冗余）。
//!
//! 性能逐项分解：配对 O(字符数) 栈；标记 O(作用域数)；定位 O(1)。

use super::vec08_comment::string_end;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、类型
// ---------------------------------------------------------------------------

/// 括号类别（三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BraceKind {
    Paren,
    Bracket,
    Brace,
}

impl BraceKind {
    pub fn open_char(self) -> u8 {
        match self {
            BraceKind::Paren => b'(',
            BraceKind::Bracket => b'[',
            BraceKind::Brace => b'{',
        }
    }

    pub fn close_char(self) -> u8 {
        match self {
            BraceKind::Paren => b')',
            BraceKind::Bracket => b']',
            BraceKind::Brace => b'}',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BraceKind::Paren => "圆括号",
            BraceKind::Bracket => "方括号",
            BraceKind::Brace => "花括号",
        }
    }

    /// 按开/闭字符 O(1) 派发。
    pub fn of_byte(b: u8) -> Option<BraceKind> {
        match b {
            b'(' | b')' => Some(BraceKind::Paren),
            b'[' | b']' => Some(BraceKind::Bracket),
            b'{' | b'}' => Some(BraceKind::Brace),
            _ => None,
        }
    }

    pub fn is_open(b: u8) -> bool {
        matches!(b, b'(' | b'[' | b'{')
    }

    pub fn is_close(b: u8) -> bool {
        matches!(b, b')' | b']' | b'}')
    }
}

/// 作用域事件（花括号专用——作用域语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeEvent {
    Open,
    Close,
}

/// 作用域标记（符号表期的直接消费单元——判据"作用域直供不重扫"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeMark {
    pub event: ScopeEvent,
    /// 事件位置（字节）
    pub pos: usize,
    /// 事件后的嵌套深度（Open 后含本层；Close 后为弹出后的深度）
    pub depth: usize,
    pub line: usize,
}

/// 一条配对记录（栈检查的产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRecord {
    pub kind: BraceKind,
    pub open_pos: usize,
    pub close_pos: usize,
    pub depth: usize,
}

/// 扫描产物：配对记录 + 作用域标记流（两流分离，符号表期只取 scopes）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PairResult {
    pub pairs: Vec<PairRecord>,
    pub scopes: Vec<ScopeMark>,
}

/// 配对错误（三要素 + 双侧定位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BraceError {
    pub code: &'static str,
    /// 当前位置（扫挂/违规处）
    pub pos: usize,
    /// 配对另一侧（开符位置；无开符 = None——判据"双侧定位"）
    pub open_pos: Option<usize>,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl BraceError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 行:列 换算（1 起计）。
    pub fn line_col(input: &str, pos: usize) -> (usize, usize) {
        let bytes = input.as_bytes();
        let pos = pos.min(bytes.len());
        let mut line = 1usize;
        let mut col = 1usize;
        for &b in &bytes[..pos] {
            if b == b'\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// 复用 F0408 字符串保护语义时的错误转换（上游注释词法错误原样上抛）。
impl From<super::vec08_comment::CommentError> for BraceError {
    fn from(e: super::vec08_comment::CommentError) -> BraceError {
        BraceError {
            code: "E_BRACE_INTERN",
            pos: e.pos,
            open_pos: None,
            what: e.what,
            why: e.why,
            next: e.next,
        }
    }
}

/// 扫描配置（深度上限可配——判据"深度上限"）。
#[derive(Clone, Copy, Debug)]
pub struct BraceConfig {
    pub max_depth: usize,
}

impl BraceConfig {
    pub const DEFAULT_MAX_DEPTH: usize = 128;

    pub fn default_config() -> BraceConfig {
        BraceConfig {
            max_depth: BraceConfig::DEFAULT_MAX_DEPTH,
        }
    }
}

fn err(code: &'static str, pos: usize, open: Option<usize>, what: String, why: String, next: String) -> BraceError {
    BraceError { code, pos, open_pos: open, what, why, next }
}

// ---------------------------------------------------------------------------
// 二、配对扫描器
// ---------------------------------------------------------------------------

/// 括号配对与作用域标记主入口（判据：栈检查 / 双侧定位 / 作用域直供 /
/// 深度上限）。字符串与注释内的括号不参与配对。
pub fn scan_braces(input: &str, config: &BraceConfig) -> Result<PairResult, BraceError> {
    let bytes = input.as_bytes();
    let mut result = PairResult::default();
    // 配对栈：(类别, 开符位置)。栈深度即嵌套深度。
    let mut stack: Vec<(BraceKind, usize)> = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'"' => {
                // 字符串保护（复用 F0408 语义）：串内括号不参与配对
                if let Some(end) = string_end(bytes, i)? {
                    let nl = bytes[i..=end.min(bytes.len() - 1)]
                        .iter()
                        .filter(|&&c| c == b'\n')
                        .count();
                    line += nl;
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b'r' if i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') => {
                if let Some(end) = string_end(bytes, i)? {
                    let nl = bytes[i..=end.min(bytes.len() - 1)]
                        .iter()
                        .filter(|&&c| c == b'\n')
                        .count();
                    line += nl;
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // 行注释跳过（注释内括号不参与配对）
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // 块注释跳过（含跨行记账；注释词法错误由 F0408 负责，这里只定界）
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    if bytes[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ if BraceKind::is_open(b) => {
                // 深度上限（判据：栈溢出→深度上限报错）
                if stack.len() >= config.max_depth {
                    let (l, c) = BraceError::line_col(input, i);
                    return Err(err(
                        "E_BRACE_DEPTH",
                        i,
                        None,
                        format!("括号嵌套深度超过上限 {}（第 {} 行 {} 列）", config.max_depth, l, c),
                        "病态深嵌套会拖垮符号表期的栈展开——上限是防御不是限制表达"
                            .to_string(),
                        "拆解嵌套层级或调整 BraceConfig::max_depth".to_string(),
                    ));
                }
                let kind = BraceKind::of_byte(b).unwrap_or(BraceKind::Paren);
                stack.push((kind, i));
                // 作用域开合标记（仅花括号——作用域语义；判据：标记 O(作用域数)）
                if kind == BraceKind::Brace {
                    result.scopes.push(ScopeMark {
                        event: ScopeEvent::Open,
                        pos: i,
                        depth: stack.len(),
                        line,
                    });
                }
                i += 1;
            }
            _ if BraceKind::is_close(b) => {
                let kind = BraceKind::of_byte(b).unwrap_or(BraceKind::Paren);
                let close_pos = i;
                match stack.last() {
                    Some(&(top_kind, open_pos)) if top_kind == kind => {
                        // 正常配对
                        stack.pop();
                        let depth = stack.len();
                        result.pairs.push(PairRecord {
                            kind,
                            open_pos,
                            close_pos,
                            depth,
                        });
                        if kind == BraceKind::Brace {
                            result.scopes.push(ScopeMark {
                                event: ScopeEvent::Close,
                                pos: close_pos,
                                depth,
                                line,
                            });
                        }
                    }
                    _ => {
                        let (l, c) = BraceError::line_col(input, close_pos);
                        if stack.is_empty() {
                            // 孤悬收符：连候选开符都没有——open_pos 置 None
                            return Err(err(
                                "E_BRACE_MISMATCH",
                                close_pos,
                                None,
                                format!(
                                    "第 {} 行 {} 列的 {} 收符没有对应开符",
                                    l, c, kind.label()
                                ),
                                "收符先于任何开符出现——配对从开符开始，孤悬收符无侧可配"
                                    .to_string(),
                                format!("删掉孤悬收符，或补 {} 开符", kind.open_char() as char),
                            ));
                        }
                        let (top_kind, top_open) = *stack.last().unwrap();
                        let deeper = stack.iter().any(|&(k, _)| k == kind);
                        if deeper {
                            // 交叉嵌套：同类开符在更深层，栈顶挡住了它
                            let (ol, oc) = BraceError::line_col(input, top_open);
                            return Err(err(
                                "E_BRACE_CROSSED",
                                close_pos,
                                Some(top_open),
                                format!(
                                    "第 {} 行 {} 列的 {} 收符与栈顶 {} 交叉嵌套",
                                    l, c, kind.label(), top_kind.label()
                                ),
                                format!(
                                    "栈顶开符在 {} 行 {} 列还没闭合——先花后圆错序是交叉嵌套，\
两类括号各自必须先开后合",
                                    ol, oc
                                ),
                                format!(
                                    "先闭合栈顶的{}，或把错序的收符移到正确层级",
                                    top_kind.label()
                                ),
                            ));
                        }
                        // 普通失配：收符找不到任何同类开符（双侧定位 = 栈顶开符）
                        let (ol, oc) = BraceError::line_col(input, top_open);
                        return Err(err(
                            "E_BRACE_MISMATCH",
                            close_pos,
                            Some(top_open),
                            format!(
                                "第 {} 行 {} 列的 {} 收符失配（栈顶开符在 {} 行 {} 列）",
                                l, c, kind.label(), ol, oc
                            ),
                            format!(
                                "{} 的收符与最近的 {} 开符不成对——双侧定位同时给出，\
从两侧对照着修",
                                kind.label(),
                                top_kind.label()
                            ),
                            format!(
                                "核对开收两侧是否写反/漏写（{} 应配 {}）",
                                top_kind.open_char() as char,
                                top_kind.close_char() as char
                            ),
                        ));
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    // 扫到 EOF 栈非空：未闭合（双侧定位：最深未闭合开符 + EOF 位置）
    if let Some(&(kind, open_pos)) = stack.last() {
        let (l, c) = BraceError::line_col(input, open_pos);
        return Err(err(
            "E_BRACE_UNCLOSED",
            bytes.len(),
            Some(open_pos),
            format!(
                "输入扫到末尾仍有 {} 处未闭合括号（最深一处开符在 {} 行 {} 列）",
                stack.len(),
                l,
                c
            ),
            "未闭合括号的双侧定位：开符位置与输入末尾同时给出".to_string(),
            format!("补 {} 个收符（嵌套层数即缺的收符数）", stack.len()),
        ));
    }
    Ok(result)
}

/// F0410 判据自检（域聚合入口）。
pub fn run_vec10_checks() -> crate::checks::CheckSet {
    super::vec10_checks::run_vec10_checks()
}
