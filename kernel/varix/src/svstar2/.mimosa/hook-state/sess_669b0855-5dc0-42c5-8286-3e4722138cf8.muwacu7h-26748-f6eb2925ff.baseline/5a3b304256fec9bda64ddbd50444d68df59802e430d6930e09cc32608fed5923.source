//! VE-F0408 · 注释与文档注释提取（VE-C 域 · 着色器系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0408`
//!
//! **判据（锚点原文）**：不嵌套语义、文档提取、元数据分离、零语义、判据。
//! - 行注释 `//…` 与块注释 `/*…*/`；块注释**不嵌套语义显性**——注释体内
//!   出现内层 `/*` 按不嵌套语义报错提示（不猜"用户想要嵌套"）；
//! - 未闭合块注释报错**指向开符**位置（行:列）；
//! - 文档注释 `///…`（行文档）与 `/**…*/`（块文档）的**结构化标注语法**
//!   （标注语法表：@brief/@param/@return/@see/@deprecated/@since）提取为
//!   **独立元数据流**供文档管线消费——不混入普通注释流，更不混入记号流；
//!   标注语法错（未知标注/缺参数）→ 提取期**警告**，不阻断编译；
//! - 注释内容**不参与编译语义**（零编译语义成本）：注释里写什么都扫得动，
//!   字符串字面量内的注释定界符不触发注释词法（字符串保护）。
//!
//! 性能逐项分解：扫描 O(字符数)；提取 O(标注数)；零编译语义成本。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、标注语法表（判据：文档提取——单一事实源）
// ---------------------------------------------------------------------------

/// 标注条目形态：标注名 → 参数要求数（0 = 纯文本，1 = 首词为参数名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationSpec {
    pub tag: &'static str,
    /// 参数个数要求（@param 必须带参数名，@brief 不带）
    pub args: usize,
}

/// 标注语法表：文档标注的合法集（未知标注 → 警告不阻断）。
pub struct AnnotationTable;

impl AnnotationTable {
    /// O(1) 查表（判据：提取 O(标注数)、单标注 O(1)）。
    pub fn lookup(tag: &str) -> Option<AnnotationSpec> {
        match tag {
            "brief" => Some(AnnotationSpec { tag: "brief", args: 0 }),
            "return" => Some(AnnotationSpec { tag: "return", args: 0 }),
            "see" => Some(AnnotationSpec { tag: "see", args: 0 }),
            "deprecated" => Some(AnnotationSpec { tag: "deprecated", args: 0 }),
            "since" => Some(AnnotationSpec { tag: "since", args: 0 }),
            "param" => Some(AnnotationSpec { tag: "param", args: 1 }),
            _ => None,
        }
    }

    pub fn legal_set() -> &'static str {
        "@brief @param @return @see @deprecated @since"
    }
}

/// 一条结构化标注（元数据流的原子）。
#[derive(Clone, Debug, PartialEq)]
pub struct DocAnnotation {
    pub tag: String,
    /// args=1 的标注：第一个词是参数名（如 @param 的形参名）
    pub arg: Option<String>,
    /// 标注后的自由文本（去除首词参数后的剩余）
    pub text: String,
    /// 所在行（1 起计）
    pub line: usize,
}

/// 文档元数据流（判据：元数据分离——独立于注释流与记号流）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocMeta {
    pub annotations: Vec<DocAnnotation>,
    /// 提取期警告（标注语法错不阻断编译——判据）
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// 二、注释记号与错误
// ---------------------------------------------------------------------------

/// 注释类别（文档类不进本流——进 DocMeta，判据"元数据分离"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
    /// 行注释 //
    Line,
    /// 块注释 /* */
    Block,
}

/// 注释记号（内容不含定界符；原文保真另存）。
#[derive(Clone, Debug, PartialEq)]
pub struct CommentToken {
    pub kind: CommentKind,
    /// 注释内容（不含 // 与 /* */）
    pub text: String,
    /// 开符位置（字节）
    pub pos: usize,
    /// 所在行（1 起计）
    pub line: usize,
    /// 原文保真（含定界符——诊断引用的原材料）
    pub original: String,
}

/// 注释扫描结果（普通注释流 + 文档元数据流——两流分离）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanResult {
    pub comments: Vec<CommentToken>,
    pub doc_meta: DocMeta,
    /// 文档注释原文（含定界符，供文档管线引用；不在 comments 流里）
    pub doc_originals: Vec<String>,
}

/// 注释词法错误（三要素 + 指向位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentError {
    pub code: &'static str,
    /// 报错指向位置（未闭合 = 开符处——判据"未闭合指向开符"）
    pub pos: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl CommentError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 行:列 换算（1 起计，人话报错用）。
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

fn err(code: &'static str, pos: usize, what: String, why: String, next: String) -> CommentError {
    CommentError { code, pos, what, why, next }
}

// ---------------------------------------------------------------------------
// 三、扫描器
// ---------------------------------------------------------------------------

/// 注释扫描配置（预留扩展点：标注表版本等）。
#[derive(Clone, Copy, Debug)]
pub struct CommentConfig;

impl CommentConfig {
    pub fn default_config() -> CommentConfig {
        CommentConfig
    }
}

/// 估算字符串字面量的结束位置（字符串保护：串内注释定界符不触发注释词法）。
/// 支持普通双引号串（反斜杠转义跳两字节）与原始串 r#"…"#。
/// 返回 Ok(Some(end))：end = 收尾引号位置；Ok(None)：非字符串起点。
/// 输入残缺（扫到 EOF 没收尾）按"剩余全部是字符串"处理——字符串语法错
/// 由 F0407 的解析器负责，本项只做定界保护不越权报错。
/// 跨批复用：F0410 括号配对扫描消费同一保护语义（一份实现零冗余）。
pub(crate) fn string_end(bytes: &[u8], start: usize) -> Result<Option<usize>, CommentError> {
    // 原始串：r + N# + " … " + N#
    if bytes[start] == b'r' {
        let mut i = start + 1;
        let mut hashes = 0usize;
        while i < bytes.len() && bytes[i] == b'#' {
            hashes += 1;
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            return Ok(None); // r 后不是引号 = 标识符，不是字符串
        }
        i += 1;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                let mut j = i + 1;
                let mut seen = 0usize;
                while j < bytes.len() && bytes[j] == b'#' && seen < hashes {
                    j += 1;
                    seen += 1;
                }
                if seen == hashes {
                    return Ok(Some(i));
                }
            }
            i += 1;
        }
        return Ok(Some(bytes.len().saturating_sub(1)));
    }
    if bytes[start] != b'"' {
        return Ok(None);
    }
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok(Some(i)),
            b'\\' => i += 2, // 转义跳两字节（\xNN 之类由串解析器管，这里只定界）
            b'\n' => return Ok(Some(i)), // 普通串不容裸换行——保护到此为止
            _ => i += 1,
        }
    }
    Ok(Some(bytes.len().saturating_sub(1)))
}

/// 注释扫描主入口（判据：不嵌套语义 / 文档提取 / 元数据分离 / 零语义）。
pub fn scan_comments(input: &str, _config: &CommentConfig) -> Result<ScanResult, CommentError> {
    let bytes = input.as_bytes();
    let mut result = ScanResult::default();
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
                // 字符串保护：串内一切照单全收，不触发注释词法
                if let Some(end) = string_end(bytes, i)? {
                    let nl = bytes[i..=end.min(bytes.len() - 1)].iter().filter(|&&c| c == b'\n').count();
                    line += nl;
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b'r' => {
                // r"…" / r#"…"# 原始串保护；r 后非引号 = 普通字节
                if i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') {
                    if let Some(end) = string_end(bytes, i)? {
                        let nl = bytes[i..=end.min(bytes.len() - 1)].iter().filter(|&&c| c == b'\n').count();
                        line += nl;
                        i = end + 1;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // 行注释或行文档注释
                let is_doc = i + 2 < bytes.len() && bytes[i + 2] == b'/';
                let open = i;
                let start_content = if is_doc { i + 3 } else { i + 2 };
                let mut j = start_content;
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                let content = &input[start_content..j];
                if is_doc {
                    // 文档注释：提取标注进元数据流（不进 comments 流）
                    result.doc_originals.push(input[open..j].to_string());
                    extract_line_doc(content, line, &mut result.doc_meta);
                } else {
                    result.comments.push(CommentToken {
                        kind: CommentKind::Line,
                        text: content.to_string(),
                        pos: open,
                        line,
                        original: input[open..j].to_string(),
                    });
                }
                i = j;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // 块注释或块文档注释（判据：不嵌套 / 未闭合指向开符）
                let is_doc = i + 2 < bytes.len() && bytes[i + 2] == b'*';
                let open = i;
                let open_line = line;
                let content_start = if is_doc { i + 3 } else { i + 2 };
                let mut j = content_start;
                let mut closed = false;
                while j + 1 < bytes.len() {
                    if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                        closed = true;
                        break;
                    }
                    if bytes[j] == b'/' && bytes[j + 1] == b'*' {
                        // 不嵌套语义显性：内层 /* 即报错，指向内层开符
                        let (l, c) = CommentError::line_col(input, j);
                        let (ol, oc) = CommentError::line_col(input, open);
                        return Err(err(
                            "E_CMT_NESTED",
                            j,
                            format!("第 {} 行 {} 列的块注释内出现内层 /*——块注释不嵌套", l, c),
                            format!(
                                "外层块注释开符在 {} 行 {} 列：本语言块注释不嵌套，\
内层 /* 不会被等待配对，写的人多半想要嵌套语义——本语言不支持",
                                ol, oc
                            ),
                            "拆成两个独立块注释，或把内层 /* 改成文字描述".to_string(),
                        ));
                    }
                    if bytes[j] == b'\n' {
                        line += 1;
                    }
                    j += 1;
                }
                if !closed {
                    let (l, c) = CommentError::line_col(input, open);
                    return Err(err(
                        "E_CMT_UNCLOSED",
                        open,
                        format!("块注释扫到输入末尾仍未闭合（开符在 {} 行 {} 列）", l, c),
                        "未闭合块注释的报错指向开符——从那里补起".to_string(),
                        "在内容末尾补收尾 */".to_string(),
                    ));
                }
                let content = &input[content_start..j];
                if is_doc {
                    result.doc_originals.push(input[open..j + 2].to_string());
                    extract_block_doc(content, open_line, &mut result.doc_meta);
                } else {
                    result.comments.push(CommentToken {
                        kind: CommentKind::Block,
                        text: content.to_string(),
                        pos: open,
                        line: open_line,
                        original: input[open..j + 2].to_string(),
                    });
                }
                i = j + 2;
            }
            _ => i += 1,
        }
    }
    Ok(result)
}

/// 行文档标注提取（判据：提取 O(标注数)；语法错 → 警告不阻断）。
fn extract_line_doc(content: &str, line: usize, meta: &mut DocMeta) {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix('@') {
        extract_annotation(rest, line, meta);
    }
}

/// 块文档标注提取：逐行找 @tag（块文档可多行多标注）。
fn extract_block_doc(content: &str, start_line: usize, meta: &mut DocMeta) {
    for (k, raw) in content.split('\n').enumerate() {
        let trimmed = raw.trim();
        // 块文档常见的行首装饰星号剥掉
        let deco = trimmed.strip_prefix('*').map(|s| s.trim_start()).unwrap_or(trimmed);
        if let Some(rest) = deco.strip_prefix('@') {
            extract_annotation(rest, start_line + k, meta);
        }
    }
}

/// 单条标注解析：tag + 可选参数名 + 自由文本；语法错全走警告。
fn extract_annotation(rest: &str, line: usize, meta: &mut DocMeta) {
    let tag_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let tag = &rest[..tag_end];
    let after = rest[tag_end..].trim_start();
    match AnnotationTable::lookup(tag) {
        None => {
            meta.warnings.push(format!(
                "第 {} 行的文档标注 @{} 不在标注语法表（{}）——按自由文本处理不阻断编译",
                line, tag, AnnotationTable::legal_set()
            ));
        }
        Some(spec) => {
            let (arg, text) = if spec.args >= 1 {
                let arg_end = after
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(after.len());
                let a = &after[..arg_end];
                if a.is_empty() {
                    meta.warnings.push(format!(
                        "第 {} 行的 @{} 缺参数名（标注语法要求一个参数词）",
                        line, tag
                    ));
                    (None, after.to_string())
                } else {
                    (Some(a.to_string()), after[arg_end..].trim().to_string())
                }
            } else {
                (None, after.to_string())
            };
            meta.annotations.push(DocAnnotation {
                tag: tag.to_string(),
                arg,
                text,
                line,
            });
        }
    }
}

/// F0408 判据自检（域聚合入口）。
pub fn run_vec08_checks() -> crate::checks::CheckSet {
    super::vec08_checks::run_vec08_checks()
}
