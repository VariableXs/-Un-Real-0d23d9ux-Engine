//! VE-F0411 · 预处理指令词法（VE-C 域 · 着色器系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0411`
//!
//! **判据（锚点原文）**：指令转预处理、续行归并、单一实现、双流分离、判据。
//! - **指令识别表**：井号开头行的指令集（include / define / undef /
//!   条件编译族 if·ifdef·ifndef·elif·else·endif）识别后转预处理器——
//!   未知指令报错**带已注册指令集建议**；
//! - **行延续符**（反斜杠续行）在词法期归并成单逻辑行（起始行号记账，
//!   预处理器看到的是一整行）；续行后 EOF 显性报错；
//! - **指令内记号化复用主词法**（F0403 [`super::vec03_lexer`] 的
//!   `Lexer::scan`，单一实现不双份——指令体记号流与代码体同一套词法）；
//! - **双流分离**：指令流 × 代码流显性分流（位置保真，互不混装）；
//!   行中井号（指令位置非法）按语境裁定显性——`#define` 体内井号是
//!   字符串化语义合法保留，代码流内的行中井号报错。
//!
//! 性能逐项分解：识别 O(1) 查表；归并 O(1)（每续行一次拼接）；双流 O(1) 分流。

use super::vec03_lexer::{Lexer, Token};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、指令识别表（判据：识别 O(1) 查表）
// ---------------------------------------------------------------------------

/// 指令类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectiveKind {
    Include,
    Define,
    Undef,
    If,
    Ifdef,
    Ifndef,
    Elif,
    Else,
    Endif,
}

impl DirectiveKind {
    pub fn name(self) -> &'static str {
        match self {
            DirectiveKind::Include => "include",
            DirectiveKind::Define => "define",
            DirectiveKind::Undef => "undef",
            DirectiveKind::If => "if",
            DirectiveKind::Ifdef => "ifdef",
            DirectiveKind::Ifndef => "ifndef",
            DirectiveKind::Elif => "elif",
            DirectiveKind::Else => "else",
            DirectiveKind::Endif => "endif",
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            DirectiveKind::Include => "包含",
            DirectiveKind::Define | DirectiveKind::Undef => "宏",
            _ => "条件编译",
        }
    }
}

/// 指令识别表（判据"已注册指令集"的单一事实源——9 指令）。
pub const DIRECTIVES: &[DirectiveKind] = &[
    DirectiveKind::Include,
    DirectiveKind::Define,
    DirectiveKind::Undef,
    DirectiveKind::If,
    DirectiveKind::Ifdef,
    DirectiveKind::Ifndef,
    DirectiveKind::Elif,
    DirectiveKind::Else,
    DirectiveKind::Endif,
];

/// O(1) 查表：指令名 → 类别。
pub fn lookup_directive(name: &str) -> Option<DirectiveKind> {
    match name {
        "include" => Some(DirectiveKind::Include),
        "define" => Some(DirectiveKind::Define),
        "undef" => Some(DirectiveKind::Undef),
        "if" => Some(DirectiveKind::If),
        "ifdef" => Some(DirectiveKind::Ifdef),
        "ifndef" => Some(DirectiveKind::Ifndef),
        "elif" => Some(DirectiveKind::Elif),
        "else" => Some(DirectiveKind::Else),
        "endif" => Some(DirectiveKind::Endif),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 二、逻辑行与续行归并
// ---------------------------------------------------------------------------

/// 一条逻辑行（续行已归并——预处理器与配对器的消费单元）。
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalLine {
    /// 归并后的文本（不含换行与续行反斜杠）
    pub text: String,
    /// 起始物理行号（1 起计）
    pub start_line: usize,
    /// 被归并的续行次数（0 = 无续行）
    pub continuations: usize,
    /// 起始字节位置
    pub start_pos: usize,
}

/// 预处理词法错误（三要素 + 位置 + 行号）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreproError {
    pub code: &'static str,
    pub pos: usize,
    pub line: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl PreproError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

fn err(code: &'static str, pos: usize, line: usize, what: String, why: String, next: String) -> PreproError {
    PreproError { code, pos, line, what, why, next }
}

/// 续行归并：物理行 → 逻辑行（判据：续行归并；每行一次拼接 O(1)）。
/// 归并规则：行尾反斜杠 = 续行（反斜杠被吃掉，两段拼接成一段）。
pub fn merge_continuations(src: &str) -> Result<Vec<LogicalLine>, PreproError> {
    let bytes = src.as_bytes();
    let mut out: Vec<LogicalLine> = Vec::new();
    let mut pending: Option<LogicalLine> = None;
    let mut i = 0usize;
    let mut line = 1usize;
    while i < bytes.len() {
        let start_pos = i;
        let start_line = line;
        // 找当前物理行终点
        let mut end = i;
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }
        // 行尾反斜杠 = 续行（EOF 处的反斜杠同样算续行——下一行不存在即报错）
        let trailing_backslash = end > start_pos && bytes[end - 1] == b'\\';
        let text_end = if trailing_backslash { end - 1 } else { end };
        let seg = src[start_pos..text_end].to_string();
        // 归并：有待续行 → 拼接；否则新逻辑行
        let mut logical = match pending.take() {
            Some(mut p) => {
                p.text = format!("{} {}", p.text.trim_end(), seg.trim_start());
                p.continuations += 1;
                p
            }
            None => LogicalLine {
                text: seg.clone(),
                start_line,
                continuations: 0,
                start_pos,
            },
        };
        if trailing_backslash {
            // 续行：本段挂起等下一段；下一段不存在（EOF）→ 显性报错
            if end >= bytes.len() {
                return Err(err(
                    "E_PP_CONT_EOF",
                    text_end,
                    line,
                    "续行反斜杠后没有下一行——输入在续行处结束".to_string(),
                    "续行符把两行并成一行，缺了下一段的逻辑行不完整".to_string(),
                    "补全续行内容，或删掉行尾反斜杠".to_string(),
                ));
            }
            pending = Some(logical);
        } else {
            out.push(logical);
        }
        // 越过换行
        i = end + 1;
        line += 1;
    }
    if let Some(p) = pending.take() {
        // 循环退出仍有挂起 = 末行续行后没有下一段（续行后 EOF）——显性报错
        let _ = p;
        return Err(err(
            "E_PP_CONT_EOF",
            bytes.len().saturating_sub(1),
            line.saturating_sub(1),
            "续行反斜杠后没有下一行——输入在续行处结束".to_string(),
            "续行符把两行并成一行，缺了下一段的逻辑行不完整".to_string(),
            "补全续行内容，或删掉行尾反斜杠".to_string(),
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 三、双流分流与指令解析
// ---------------------------------------------------------------------------

/// 指令流的一行（识别 + 记号化后）。
#[derive(Clone, Debug, PartialEq)]
pub struct DirectiveLine {
    pub kind: DirectiveKind,
    /// 指令名（与 kind.name() 一致）
    pub name: String,
    /// 指令名后的参数体（原样文本；include 的 <x>/\"x\" 保留）
    pub body: String,
    /// 指令体记号流（复用主词法 vec03——判据"单一实现不双份"）
    pub tokens: Vec<Token>,
    pub line: usize,
    pub pos: usize,
    pub continuations: usize,
}

/// 代码流的一行（非指令逻辑行，位置保真）。
#[derive(Clone, Debug, PartialEq)]
pub struct CodeLine {
    pub text: String,
    pub line: usize,
    pub pos: usize,
}

/// 双流结构（判据：指令流 × 代码流分离）。
#[derive(Clone, Debug, PartialEq)]
pub struct PreproStreams {
    pub directives: Vec<DirectiveLine>,
    pub code: Vec<CodeLine>,
}

/// 双流分流主入口：续行归并 → 井号行识别 → 指令体记号化（主词法复用）。
pub fn split_streams(src: &str) -> Result<PreproStreams, PreproError> {
    let logical = merge_continuations(src)?;
    let mut streams = PreproStreams {
        directives: Vec::new(),
        code: Vec::new(),
    };
    for ll in logical {
        let trimmed = ll.text.trim_start();
        if trimmed.starts_with('#') {
            // 指令行：井号后取指令名（标识符字节域，复用 vec05 的判定）
            let after_hash = &trimmed[1..];
            let lead_ws = after_hash.len() - after_hash.trim_start().len();
            let name_start = 1 + lead_ws;
            let rest = &after_hash[lead_ws..];
            let name_end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let name = &rest[..name_end];
            let kind = lookup_directive(name).ok_or_else(|| {
                let names: Vec<String> =
                    DIRECTIVES.iter().map(|d| format!("#{}", d.name())).collect();
                err(
                    "E_PP_UNKNOWN",
                    ll.start_pos,
                    ll.start_line,
                    format!("第 {} 行的指令 #{} 未注册", ll.start_line, name),
                    "未知指令在词法期拒绝——预处理器的行为表只认注册过的指令".to_string(),
                    format!("已注册指令集：{}", names.join(" ")),
                )
            })?;
            // 行中井号语境裁定：define 体内的 # 是字符串化语义，合法保留；
            // 其余指令体内再次出现井号 = 位置非法（指令不支持嵌套井号语义）
            let body_start = name_start + name_end;
            let body = trimmed[body_start..].trim_start().to_string();
            if kind != DirectiveKind::Define && body.contains('#') {
                return Err(err(
                    "E_PP_POSITION",
                    ll.start_pos,
                    ll.start_line,
                    format!("第 {} 行的 #{} 体内出现行中井号", ll.start_line, name),
                    "井号是指令起始符——只有 define 体（字符串化）语境的井号合法".to_string(),
                    "把体内井号去掉，或确认该行确实是指令行".to_string(),
                ));
            }
            // 指令体记号化：复用主词法（单一实现不双份）
            let (tokens, _) = Lexer::new()
                .and_then(|lx| lx.scan(&body))
                .map_err(|e| {
                    err(
                        "E_PP_TOKENIZE",
                        ll.start_pos,
                        ll.start_line,
                        format!("#{} 指令体记号化失败：{}", name, e.describe()),
                        "指令体与代码体走同一套词法——主词法报的错原样上抛不转译"
                            .to_string(),
                        "按主词法建议修正指令体".to_string(),
                    )
                })?;
            streams.directives.push(DirectiveLine {
                kind,
                name: name.to_string(),
                body,
                tokens,
                line: ll.start_line,
                pos: ll.start_pos,
                continuations: ll.continuations,
            });
        } else {
            // 代码流：非指令行。行中井号 = 指令位置非法（显性报错）
            if let Some(h) = trimmed.find('#') {
                if h > 0 {
                    return Err(err(
                        "E_PP_POSITION",
                        ll.start_pos + h,
                        ll.start_line,
                        format!("第 {} 行代码中出现行中井号", ll.start_line),
                        "井号是指令起始符——指令必须独占逻辑行的行首".to_string(),
                        "把 # 移到行首成指令行，或删除该井号".to_string(),
                    ));
                }
            }
            streams.code.push(CodeLine {
                text: ll.text.clone(),
                line: ll.start_line,
                pos: ll.start_pos,
            });
        }
    }
    Ok(streams)
}

/// F0411 判据自检（域聚合入口）。
pub fn run_vec11_checks() -> crate::checks::CheckSet {
    super::vec11_checks::run_vec11_checks()
}
