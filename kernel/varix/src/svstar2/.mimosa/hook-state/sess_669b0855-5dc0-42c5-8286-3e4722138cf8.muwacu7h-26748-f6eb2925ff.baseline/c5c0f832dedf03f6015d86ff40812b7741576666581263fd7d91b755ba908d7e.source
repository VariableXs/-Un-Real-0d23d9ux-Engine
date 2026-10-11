//! VE-F0407 · 字符串字面量与转义（VE-C 域 · 着色器系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0407`
//!
//! **判据（锚点原文）**：转义全集、原始串、编码显性、未闭合指向、判据。
//! - 基本双引号串；转义序列**全集**：换行 `\n`、制表 `\t`、回车 `\r`、
//!   空字节 `\0`、反斜杠 `\\`、双引号 `\"`、单引号 `\'`、十六进制字节
//!   `\xNN`、Unicode 转义 `\uXXXX`（恰好四位，BMP）与 `\u{…}`（变长 1-6
//!   位十六进制，全 Unicode 域——没有花括号形式增补平面在 UTF-8/UTF-16
//!   串里就不可表达，"全集"缺一角）——全部走 O(1) 转义表，非法转义报错
//!   **带位置与合法集**（不猜意图）；
//! - 原始串语义：`r"…"` 与 `r#"…"#`（井号定界），内部零转义、跨行合法——
//!   原始串里写 `\n` 就是反斜杠加 n 两个字节，词法器不做任何解释；
//! - 编码显性：串记号携带「字节 × 源编码标记」（上游 F0415 源编码），
//!   `\xNN` 与 `\uXXXX` 的产出必须**在源编码内可编码**，不可编码即报错
//!   （编码歧义按源编码裁定并告知，不静默替换）；
//! - 跨行串规则显性：普通串内出现裸换行/扫到 EOF 未闭合 = 未闭合串，
//!   报错**指向开引号**位置（行:列），不是指向扫挂的地方；
//! - 性能逐项分解：扫描 O(字符数)、转义 O(1) 查表、编码裁定 O(1)。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、编码标记（判据：编码显性）
// ---------------------------------------------------------------------------

/// 源编码（与上游 VE-F0415 源编码对接；本项只消费标记，不负责探测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceEncoding {
    /// UTF-8（默认）
    Utf8,
    /// Latin-1 / ISO-8859-1（单字节，全 0-255 可编码）
    Latin1,
    /// UTF-16（16 位码元，代理对）
    Utf16,
}

impl SourceEncoding {
    pub fn label(self) -> &'static str {
        match self {
            SourceEncoding::Utf8 => "UTF-8",
            SourceEncoding::Latin1 => "Latin-1",
            SourceEncoding::Utf16 => "UTF-16",
        }
    }

    /// 单个原始字节在该编码内是否可直接存在（`\xNN` 的裁定）。
    pub fn byte_encodable(self, b: u8) -> bool {
        match self {
            // UTF-8 串内裸字节 ≥0x80 会破坏 UTF-8 良构性——显性拒绝
            SourceEncoding::Utf8 => b < 0x80,
            SourceEncoding::Latin1 => true,
            SourceEncoding::Utf16 => true,
        }
    }

    /// 码点在该编码内是否可编码（`\uXXXX` 的裁定；O(1) 区间判定）。
    pub fn codepoint_encodable(self, cp: u32) -> bool {
        match self {
            SourceEncoding::Utf8 => (0x00..=0x10FFFF).contains(&cp) && !(0xD800..=0xDFFF).contains(&cp),
            SourceEncoding::Latin1 => cp <= 0xFF,
            SourceEncoding::Utf16 => (0x00..=0x10FFFF).contains(&cp) && !(0xD800..=0xDFFF).contains(&cp),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、转义表（判据：转义全集；O(1) 查表）
// ---------------------------------------------------------------------------

/// 转义条目：查表命中的三类语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeKind {
    /// 直接映射到一个字符（\n \r \t \0 \\ \" \'）
    Char(char),
    /// 十六进制字节（\xNN：恰好两位十六进制）
    HexByte,
    /// Unicode 码点（\uXXXX：恰好四位十六进制）
    Unicode,
}

/// 转义表：合法转义引导符全集（判据"合法集"的单一事实源）。
pub struct EscapeTable;

impl EscapeTable {
    /// O(1) 查表：引导符 → 语义；查不到 = 非法转义。
    pub fn lookup(lead: u8) -> Option<EscapeKind> {
        match lead {
            b'n' => Some(EscapeKind::Char('\n')),
            b'r' => Some(EscapeKind::Char('\r')),
            b't' => Some(EscapeKind::Char('\t')),
            b'0' => Some(EscapeKind::Char('\0')),
            b'\\' => Some(EscapeKind::Char('\\')),
            b'"' => Some(EscapeKind::Char('"')),
            b'\'' => Some(EscapeKind::Char('\'')),
            b'x' => Some(EscapeKind::HexByte),
            b'u' => Some(EscapeKind::Unicode),
            _ => None,
        }
    }

    /// 合法集的人话清单（报错三要素的 next 用）。
    pub fn legal_set() -> &'static str {
        "\\n \\r \\t \\0 \\\\ \\\" \\' \\xNN \\uXXXX \\u{…}"
    }
}

// ---------------------------------------------------------------------------
// 三、串记号与错误
// ---------------------------------------------------------------------------

/// 串字面量类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringKind {
    /// 普通双引号串（转义生效）
    Quoted,
    /// 原始串（零转义、跨行合法）
    Raw { hashes: usize },
}

/// 串记号（判据：编码显性——字节 × 编码标记成对入记号）。
#[derive(Clone, Debug, PartialEq)]
pub struct StringToken {
    /// 原文逐字节保真（含引号与定界井号——诊断引用的原材料）
    pub original: String,
    /// 解码后的字节（按 encoding 编码；原始串 = 引号内原文逐字节）
    pub decoded: Vec<u8>,
    /// 源编码标记（与上游 F0415 对齐）
    pub encoding: SourceEncoding,
    pub kind: StringKind,
    /// 开引号字节位置（未闭合报错指向处）
    pub quote_pos: usize,
    /// 跨行信息（原始串可能跨行：占用行数）
    pub lines_spanned: usize,
}

/// 串字面量错误（三要素 + 位置 + 开引号位置）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrError {
    pub code: &'static str,
    /// 报错指向的位置（未闭合串 = 开引号处——判据"未闭合指向"）
    pub pos: usize,
    /// 开引号位置（未闭合类错误与 pos 相同；非法转义时供上下文）
    pub open_quote: usize,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl StrError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }

    /// 行:列 换算（O(pos) 一次扫描；1 起计，人话报错用）。
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

fn err(code: &'static str, pos: usize, open: usize, what: String, why: String, next: String) -> StrError {
    StrError {
        code,
        pos,
        open_quote: open,
        what,
        why,
        next,
    }
}

// ---------------------------------------------------------------------------
// 四、解析器
// ---------------------------------------------------------------------------

/// 解析配置（编码与裁定来源显性注入，不探测不猜）。
#[derive(Clone, Copy, Debug)]
pub struct StringConfig {
    pub encoding: SourceEncoding,
}

impl StringConfig {
    pub fn default_config() -> StringConfig {
        StringConfig {
            encoding: SourceEncoding::Utf8,
        }
    }
}

fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

/// 判定原始串的井号定界长度：输入以 `r` 开头且其后是 `"` 或 `#`。
/// 返回 Ok(Some(hashes)) = 原始串（hashes 个井号定界）；Ok(None) = 非原始串
/// （`r` 后跟其他字符——由调用方按标识符规则处理，本项不越权）。
fn raw_hashes(bytes: &[u8]) -> Result<Option<usize>, ()> {
    if bytes.first() != Some(&b'r') {
        return Ok(None);
    }
    let mut i = 1usize;
    let mut hashes = 0usize;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'"' {
        Ok(Some(hashes))
    } else {
        if hashes > 0 {
            // r### 后不是引号：形似原始串但定界残缺——显性报错而非当标识符
            return Err(());
        }
        Ok(None)
    }
}

/// 串字面量解析（判据：转义全集 / 原始串 / 编码显性 / 未闭合指向）。
pub fn parse_string(input: &str, config: &StringConfig) -> Result<StringToken, StrError> {
    let bytes = input.as_bytes();
    let original = input.to_string();
    if bytes.is_empty() {
        return Err(err(
            "E_STR_EMPTY",
            0,
            0,
            "串字面量为空输入".to_string(),
            "空输入连开引号都没有，不是串字面量".to_string(),
            "补全双引号与串内容".to_string(),
        ));
    }
    // 原始串分支（判据：原始串）
    match raw_hashes(bytes) {
        Err(()) => {
            return Err(err(
                "E_STR_RAW_DELIM",
                0,
                0,
                "原始串定界残缺：r 后跟井号但井号后不是引号".to_string(),
                "原始串定界是 r + N 个井号 + 引号（如 r#\"…\"#）——残缺定界不许猜成标识符"
                    .to_string(),
                "补全井号后的开引号，或去掉井号（r\"…\"）".to_string(),
            ));
        }
        Ok(Some(hashes)) => return parse_raw_string(input, bytes, hashes, config, original),
        Ok(None) => {}
    }
    if bytes[0] != b'"' {
        return Err(err(
            "E_STR_NO_QUOTE",
            0,
            0,
            format!("串字面量以 {:?} 开头——必须以双引号开头", bytes[0] as char),
            "本解析器只收双引号串与原始串（r…），单引号是字符字面量族（F0408）"
                .to_string(),
            "改用双引号；若确是字符字面量请走字符字面量解析器".to_string(),
        ));
    }
    let open = 0usize;
    let mut decoded: Vec<u8> = Vec::new();
    let mut i = 1usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                return Ok(StringToken {
                    original,
                    decoded,
                    encoding: config.encoding,
                    kind: StringKind::Quoted,
                    quote_pos: open,
                    // 普通串裸换行已在循环内拦截——能活着闭合的串恒单行
                    lines_spanned: 1,
                });
            }
            b'\n' => {
                // 判据：跨行串规则显性——普通串裸换行 = 未闭合，指向开引号
                let (line, col) = StrError::line_col(input, open);
                return Err(err(
                    "E_STR_UNCLOSED",
                    open,
                    open,
                    format!("普通串出现裸换行——未闭合（开引号在 {} 行 {} 列）", line, col),
                    "普通串不允许裸换行：换行必须写成 \\n，跨行内容用原始串 r\"…\""
                        .to_string(),
                    format!("补上收尾引号；跨行内容改用原始串或转义换行（合法转义集：{}）", EscapeTable::legal_set()),
                ));
            }
            b'\\' => {
                // 判据：转义全集 + 非法转义报错带位置与合法集
                i += 1;
                if i >= bytes.len() {
                    let (line, col) = StrError::line_col(input, open);
                    return Err(err(
                        "E_STR_UNCLOSED",
                        open,
                        open,
                        format!("反斜杠后串即结束——未闭合（开引号在 {} 行 {} 列）", line, col),
                        "转义序列被串结尾截断：反斜杠后面必须跟合法转义引导符".to_string(),
                        format!("补全转义或删掉孤悬反斜杠（合法转义集：{}）", EscapeTable::legal_set()),
                    ));
                }
                let lead = bytes[i];
                let kind = EscapeTable::lookup(lead).ok_or_else(|| {
                    let (line, col) = StrError::line_col(input, i);
                    err(
                        "E_STR_BAD_ESCAPE",
                        i,
                        open,
                        format!("第 {} 行 {} 列的转义 \\{} 非法", line, col, lead as char),
                        "非法转义在源码里是错义灾难——词法器不猜写的人想要什么".to_string(),
                        format!("改用合法转义集之一：{}", EscapeTable::legal_set()),
                    )
                })?;
                match kind {
                    EscapeKind::Char(c) => {
                        push_char(&mut decoded, c, config.encoding, i, open)?;
                        i += 1;
                    }
                    EscapeKind::HexByte => {
                        // \xNN：恰好两位十六进制，值须在源编码内可编码
                        if i + 2 >= bytes.len() {
                            return Err(err(
                                "E_STR_ESCAPE_TRUNC",
                                i,
                                open,
                                "\\x 转义缺两位十六进制数字".to_string(),
                                "\\xNN 的 NN 是硬性两位——缺位不许按 0 补齐静默通过".to_string(),
                                "补全两位十六进制数字（如 \\x41）".to_string(),
                            ));
                        }
                        let hi = hex_val(bytes[i + 1]).ok_or_else(|| {
                            err(
                                "E_STR_ESCAPE_HEX",
                                i + 1,
                                open,
                                format!("\\x 后第 1 位 {:?} 不是十六进制数字", bytes[i + 1] as char),
                                "\\xNN 需要两位十六进制数字".to_string(),
                                "改用 0-9 a-f A-F".to_string(),
                            )
                        })?;
                        let lo = hex_val(bytes[i + 2]).ok_or_else(|| {
                            err(
                                "E_STR_ESCAPE_HEX",
                                i + 2,
                                open,
                                format!("\\x 后第 2 位 {:?} 不是十六进制数字", bytes[i + 2] as char),
                                "\\xNN 需要两位十六进制数字".to_string(),
                                "改用 0-9 a-f A-F".to_string(),
                            )
                        })?;
                        let byte = ((hi << 4) | lo) as u8;
                        if !config.encoding.byte_encodable(byte) {
                            return Err(err(
                                "E_STR_ENCODING",
                                i,
                                open,
                                format!(
                                    "字节 0x{:02X} 在源编码 {} 内不可直接存在",
                                    byte,
                                    config.encoding.label()
                                ),
                                "\\xNN 产出的是源编码内的裸字节——UTF-8 串内裸高位字节会破坏良构性"
                                    .to_string(),
                                "改用 \\uXXXX 写码点（按源编码自动编码），或确认源编码标记"
                                    .to_string(),
                            ));
                        }
                        decoded.push(byte);
                        i += 3;
                    }
                    EscapeKind::Unicode => {
                        // 两种形：\uXXXX（恰好四位，BMP）与 \u{…}（变长 1-6 位，
                        // 全 Unicode——转义全集不允许增补平面在 UTF-8/UTF-16
                        // 串里不可表达）。
                        if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                            i += 2; // 越过 u{
                            let mut cp: u32 = 0;
                            let mut digits = 0usize;
                            loop {
                                if i >= bytes.len() {
                                    return Err(err(
                                        "E_STR_ESCAPE_TRUNC",
                                        i,
                                        open,
                                        "\\u{ 花括号转义未闭合（缺 }）".to_string(),
                                        "\\u{…} 的收尾 } 是硬性定界——未闭合不许静默截断".to_string(),
                                        "补全收尾 }（如 \\u{1F600}）".to_string(),
                                    ));
                                }
                                let b = bytes[i];
                                if b == b'}' {
                                    break;
                                }
                                if b == b'"' {
                                    // 收尾引号顶到数字位 = 花括号没闭合
                                    return Err(err(
                                        "E_STR_ESCAPE_TRUNC",
                                        i,
                                        open,
                                        "\\u{{ 花括号转义未闭合（缺 }}）——收尾引号顶到了数字位"
                                            .to_string(),
                                        "\\u{{…}} 的收尾 }} 是硬性定界——未闭合不许静默截断"
                                            .to_string(),
                                        "补全收尾 }}（如 \\u{{1F600}}）".to_string(),
                                    ));
                                }
                                let d = hex_val(b).ok_or_else(|| {
                                    err(
                                        "E_STR_ESCAPE_HEX",
                                        i,
                                        open,
                                        format!("\\u{{ 内 {:?} 不是十六进制数字", b as char),
                                        "\\u{…} 内只允许十六进制数字".to_string(),
                                        "改用 0-9 a-f A-F".to_string(),
                                    )
                                })?;
                                cp = (cp << 4) | d;
                                if cp > 0x10FFFF {
                                    return Err(err(
                                        "E_STR_ENCODING",
                                        i,
                                        open,
                                        format!("码点 U+{:X} 超出 Unicode 上界 U+10FFFF", cp),
                                        "超出 Unicode 表示域的码点不存在".to_string(),
                                        "核对码点数值".to_string(),
                                    ));
                                }
                                digits += 1;
                                i += 1;
                            }
                            if digits == 0 {
                                return Err(err(
                                    "E_STR_ESCAPE_TRUNC",
                                    i,
                                    open,
                                    "\\u{} 空花括号——缺十六进制数字".to_string(),
                                    "\\u{…} 至少一位十六进制数字".to_string(),
                                    "补全码点数字（如 \\u{4E2D}）".to_string(),
                                ));
                            }
                            i += 1; // 越过 }
                            encode_codepoint(&mut decoded, cp, config.encoding, i, open)?;
                        } else {
                            // \uXXXX：恰好四位十六进制，代理区拒绝，按源编码编码
                            if i + 4 >= bytes.len() {
                                return Err(err(
                                    "E_STR_ESCAPE_TRUNC",
                                    i,
                                    open,
                                    "\\u 转义缺四位十六进制数字".to_string(),
                                    "\\uXXXX 的 XXXX 是硬性四位——缺位不许静默通过；\
增补平面请用 \\u{…} 花括号形式"
                                        .to_string(),
                                    "补全四位十六进制数字（如 \\u4E2D）".to_string(),
                                ));
                            }
                            let mut cp: u32 = 0;
                            for k in 1..=4 {
                                let d = hex_val(bytes[i + k]).ok_or_else(|| {
                                    err(
                                        "E_STR_ESCAPE_HEX",
                                        i + k,
                                        open,
                                        format!(
                                            "\\u 后第 {} 位 {:?} 不是十六进制数字",
                                            k, bytes[i + k] as char
                                        ),
                                        "\\uXXXX 需要四位十六进制数字".to_string(),
                                        "改用 0-9 a-f A-F".to_string(),
                                    )
                                })?;
                                cp = (cp << 4) | d;
                            }
                            encode_codepoint(&mut decoded, cp, config.encoding, i, open)?;
                            i += 5;
                        }
                    }
                }
            }
            _ => {
                // 普通串内不可能有裸换行（上面已拦截）——其余字节原样入解码缓冲
                decoded.push(b);
                i += 1;
            }
        }
    }
    // 扫到 EOF 未闭合（判据：未闭合指向开引号）
    let (line, col) = StrError::line_col(input, open);
    Err(err(
        "E_STR_UNCLOSED",
        open,
        open,
        format!("串扫到输入末尾仍未闭合（开引号在 {} 行 {} 列）", line, col),
        "未闭合串的报错指向开引号——从那里补起，而不是从扫挂的地方瞎补".to_string(),
        "在内容末尾补收尾双引号".to_string(),
    ))
}

/// 原始串解析（零转义；跨行合法；收尾 = 引号 + 同数井号）。
fn parse_raw_string(
    input: &str,
    bytes: &[u8],
    hashes: usize,
    config: &StringConfig,
    original: String,
) -> Result<StringToken, StrError> {
    let open = hashes + 1; // r + hashes 之后即开引号
    let content_start = open + 1;
    let mut i = content_start;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            // 尝试匹配引号 + hashes 个井号
            let mut j = i + 1;
            let mut seen = 0usize;
            while j < bytes.len() && bytes[j] == b'#' && seen < hashes {
                j += 1;
                seen += 1;
            }
            if seen == hashes && j == bytes.len() {
                let decoded = bytes[content_start..i].to_vec();
                let lines = bytes[content_start..i]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count()
                    + 1;
                return Ok(StringToken {
                    original,
                    decoded,
                    encoding: config.encoding,
                    kind: StringKind::Raw { hashes },
                    quote_pos: open,
                    lines_spanned: lines,
                });
            }
            // 井号数不足：这是内容里的引号，继续扫
        }
        i += 1;
    }
    let (line, col) = StrError::line_col(input, open);
    Err(err(
        "E_STR_UNCLOSED",
        open,
        open,
        format!(
            "原始串扫到输入末尾仍未闭合（开引号在 {} 行 {} 列，需 {} 个井号收尾）",
            line, col, hashes
        ),
        "原始串的收尾是引号 + 与开头相同数量的井号".to_string(),
        format!("补收尾定界 \"{}（井号数与开头一致）", "#".repeat(hashes)),
    ))
}

/// 按源编码把字符压入解码缓冲（\n 等直映射字符共用）。
fn push_char(
    out: &mut Vec<u8>,
    c: char,
    enc: SourceEncoding,
    pos: usize,
    open: usize,
) -> Result<(), StrError> {
    encode_codepoint(out, c as u32, enc, pos, open)
}

/// 码点 → 源编码字节序列（O(1) 裁定 + 定长编码；代理区在可编码判定已拒绝）。
fn encode_codepoint(
    out: &mut Vec<u8>,
    cp: u32,
    enc: SourceEncoding,
    pos: usize,
    open: usize,
) -> Result<(), StrError> {
    if !enc.codepoint_encodable(cp) {
        return Err(err(
            "E_STR_ENCODING",
            pos,
            open,
            format!("码点 U+{:04X} 在源编码 {} 内不可编码", cp, enc.label()),
            "编码歧义按源编码裁定：超出编码表示域的码点不静默替换".to_string(),
            match enc {
                SourceEncoding::Latin1 => "改用 \\xNN 写 0x00-0xFF 内的字节，或换 UTF-8 源编码"
                    .to_string(),
                _ => "该码点超出 Unicode 表示域或落在代理区——代理区码点不合法".to_string(),
            },
        ));
    }
    match enc {
        SourceEncoding::Latin1 => out.push(cp as u8),
        SourceEncoding::Utf8 => {
            // UTF-8 定长分支编码（1-4 字节；无外部依赖）
            if cp < 0x80 {
                out.push(cp as u8);
            } else if cp < 0x800 {
                out.push(0xC0 | (cp >> 6) as u8);
                out.push(0x80 | (cp & 0x3F) as u8);
            } else if cp < 0x10000 {
                out.push(0xE0 | (cp >> 12) as u8);
                out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
                out.push(0x80 | (cp & 0x3F) as u8);
            } else {
                out.push(0xF0 | (cp >> 18) as u8);
                out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
                out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
                out.push(0x80 | (cp & 0x3F) as u8);
            }
        }
        SourceEncoding::Utf16 => {
            if cp < 0x10000 {
                out.extend_from_slice(&(cp as u16).to_le_bytes());
            } else {
                let v = cp - 0x10000;
                let hi = 0xD800 | (v >> 10);
                let lo = 0xDC00 | (v & 0x3FF);
                out.extend_from_slice(&(hi as u16).to_le_bytes());
                out.extend_from_slice(&(lo as u16).to_le_bytes());
            }
        }
    }
    Ok(())
}

/// F0407 判据自检（域聚合入口）。
pub fn run_vec07_checks() -> crate::checks::CheckSet {
    super::vec07_checks::run_vec07_checks()
}
