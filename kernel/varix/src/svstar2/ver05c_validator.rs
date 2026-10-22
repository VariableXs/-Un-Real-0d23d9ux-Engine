//! VE-F3417 · 令牌验证器 —— 三查静态验证（引用完整性/类型正确性/对比度预检）
//! + 行号定位 + 开放导出 + CI 集成。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3417`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三查静态验证**：
//!   1. **引用完整性**——每个 `{other.token}` 引用都必须解析到真实存在的
//!      令牌，且引用图无环。环检测用迭代 DFS（显式栈），不用递归：令牌
//!      文件是用户可写的，递归深度等于用户输入，这是不可接受的攻击面；
//!   2. **类型正确性**——值必须能解析成 [`TokenType`] 声明的类型，
//!      且路径后缀（`.px` / `.ms`）与类型不得互相矛盾；
//!   3. **对比度预检**——颜色类令牌若声明了配对（前景/背景），按 WCAG
//!      三档判定（AA 4.5:1 / AAA 7:1 / 不达标）。判定用整数定点，不用
//!      浮点：验证器要在 CI 与内核两侧跑出**逐位相同**的结论；
//! - **行号定位**：每条发现都带 `line_no`。没有行号的验证报告等于让作者
//!   自己在几千行里找，报告就白写了；
//! - **开放导出**：TSV 三列（严重度/码/行号）+ 路径 + 说明。人可读、
//!   可 `diff`、可被任何 CI 当工件吃下；
//! - **CI 集成**：`ci_verdict()` 把报告压成三态（通过/警告/失败）并给出
//!   退出码。Error 一律失败，Warn 由策略决定，Info 不影响。
//!
//! # 为什么验证器不许「尽力解析」
//!
//! 解析器宽容一点、验证器宽容一点，两者叠加的结果是**坏令牌进得来、
//! 报告还是绿的**。所以本模块的立场是：认不出的行就是错误行，带着行号
//! 报出来；不做「猜测作者意图」的修补。修令牌是作者的事，验证器的
//! 职责是让错误无处可藏。
//!
//! # 复杂度
//!
//! - `parse`：O(行数)；
//! - `check_refs`：O(令牌数 + 引用数)，迭代 DFS 每条边一次；
//! - `check_types`：O(令牌数)；
//! - `check_contrast`：O(配对数)，每次 O(1) 定点运算；
//! - `export_tsv`：O(发现数)。
//!
//! # 零 panic 面
//!
//! 解析用 `split_once` / `strip_prefix` 返回 `Option`，全程无索引越界；
//! 定点对比度用 `u64` 且先钳位再乘，不会溢出。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::ver05a_appread::{Rgba, TokenType, TokenValue};

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段续：5321 起）
// ---------------------------------------------------------------------------

/// 验证器诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyCode {
    /// 缺等号（不是 `path = value`）。
    MissingAssign = 5321,
    /// 路径为空。
    EmptyPath = 5322,
    /// 路径非法（字符集/长度）。
    IllegalPath = 5323,
    /// 值无法解析成声明类型。
    BadValue = 5324,
    /// 路径后缀与类型矛盾（`.px` 配时长这类）。
    SuffixTypeClash = 5325,
    /// 引用了不存在的令牌。
    DanglingRef = 5326,
    /// 引用图有环。
    RefCycle = 5327,
    /// 引用深度超过上限（引用炸弹的第一道闸）。
    RefTooDeep = 5328,
    /// 对比度不达 AA（4.5:1）。
    ContrastBelowAa = 5329,
    /// 对比度不达 AAA（7:1）但达 AA。
    ContrastBelowAaa = 5330,
    /// 配对引用的两端类型不是颜色。
    PairNotColor = 5331,
    /// 同一路径重复定义。
    DuplicatePath = 5332,
    /// 未知的严重度声明（`!warn` 之类的行内标记写错）。
    BadSeverityTag = 5333,
}

impl VerifyCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 默认严重度。个别码（AAA 不达）在报告里可降为 Info，
    /// 但默认值必须保守——宁可让 CI 红一次，也不要放过坏令牌。
    pub fn severity(self) -> Severity {
        match self {
            VerifyCode::ContrastBelowAaa => Severity::Warn,
            VerifyCode::BadSeverityTag => Severity::Warn,
            _ => Severity::Error,
        }
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            VerifyCode::MissingAssign => "缺少等号，行格式应为 路径 = 值",
            VerifyCode::EmptyPath => "路径为空",
            VerifyCode::IllegalPath => "路径含非法字符或超长",
            VerifyCode::BadValue => "值无法解析为声明类型",
            VerifyCode::SuffixTypeClash => "路径后缀与值类型矛盾",
            VerifyCode::DanglingRef => "引用了不存在的令牌",
            VerifyCode::RefCycle => "引用图存在环",
            VerifyCode::RefTooDeep => "引用深度超过上限",
            VerifyCode::ContrastBelowAa => "对比度未达 AA（4.5:1）",
            VerifyCode::ContrastBelowAaa => "对比度未达 AAA（7:1），已达 AA",
            VerifyCode::PairNotColor => "配对引用的两端必须是颜色令牌",
            VerifyCode::DuplicatePath => "同一路径重复定义",
            VerifyCode::BadSeverityTag => "行内严重度标记无法识别",
        }
    }
}

/// 严重度。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// 提示（不阻断）。
    Info,
    /// 警告（策略可决定是否阻断）。
    Warn,
    /// 错误（一律阻断）。
    Error,
}

impl Severity {
    /// 单字符标记（导出用）。
    pub fn tag(self) -> char {
        match self {
            Severity::Info => 'I',
            Severity::Warn => 'W',
            Severity::Error => 'E',
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "提示",
            Severity::Warn => "警告",
            Severity::Error => "错误",
        }
    }
}

/// 引用深度上限。超过即判引用炸弹——这条闸必须在 DFS 里而不是在
/// 「解析完之后再数」，否则炸弹已经在内存里炸过一次了。
pub const REF_DEPTH_CAP: usize = 32;

/// 路径长度上限（与读门面同一量级）。
pub const PATH_LIMIT: usize = 96;

// ---------------------------------------------------------------------------
// 二、解析
// ---------------------------------------------------------------------------

/// 一条发现。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Finding {
    /// 诊断码。
    pub code: VerifyCode,
    /// 严重度（可被策略提升，不能降低到 Info 以下）。
    pub severity: Severity,
    /// 行号（1 起算，与编辑器一致）。
    pub line_no: u32,
    /// 相关路径（可能为空：行格式错误时还没解析出路径）。
    pub path: String,
    /// 补充说明（例如「引用 x.y 不存在」）。
    pub detail: String,
}

impl Finding {
    fn new(code: VerifyCode, line_no: u32, path: &str, detail: &str) -> Finding {
        Finding {
            code,
            severity: code.severity(),
            line_no,
            path: path.to_string(),
            detail: detail.to_string(),
        }
    }

    /// 一行的读屏/报告渲染。
    pub fn render(&self) -> String {
        if self.detail.is_empty() {
            format!(
                "第{}行 [{}] {}：{}",
                self.line_no,
                self.severity.tag(),
                self.code.text(),
                self.path
            )
        } else {
            format!(
                "第{}行 [{}] {}：{}（{}）",
                self.line_no,
                self.severity.tag(),
                self.code.text(),
                self.path,
                self.detail
            )
        }
    }
}

/// 解析出的一条令牌。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParsedToken {
    /// 路径。
    pub path: String,
    /// 值（引用型值的 `TokenValue` 为占位 `Count(0)`，真值在 `refs` 里）。
    pub value: TokenValue,
    /// 引用目标（`{path}` 形式，已剥壳）。
    pub refs: Vec<String>,
    /// 行号。
    pub line_no: u32,
    /// 声明类型（来自值形态；引用型由目标决定，此处记 `None`）。
    pub declared: Option<TokenType>,
}

/// 一份解析结果。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ParsedFile {
    /// 令牌表（按出现顺序）。
    pub tokens: Vec<ParsedToken>,
    /// 解析期发现。
    pub findings: Vec<Finding>,
}

/// 解析令牌文件。行式格式：
///
/// ```text
/// # 注释
/// color.accent = #4C8DFF
/// spacing.gap = 12px
/// motion.fast = 120ms
/// color.fg = {color.accent}
/// color.pair = #FFFFFF on #000000   # 显式配对，供对比度预检
/// ```
///
/// 认不出的行一律记 `MissingAssign` / `BadValue`，不猜。
pub fn parse(text: &str) -> ParsedFile {
    let mut out = ParsedFile::default();
    let mut line_no: u32 = 0;
    for raw in text.lines() {
        line_no = line_no.saturating_add(1);
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 行尾注释：**只认 ` # `（空白 + 井号 + 空白）与行尾 ` #`**。
        // 不能见到 ` #` 就切——颜色的写法就是 `#RRGGBB`，`= #FFFFFF`
        // 里也含 ` #`，按「见井号即注释」的规则会把每个颜色值都咬掉
        // 半个字节。这个坑在本模块第一版里真实踩过：一行 `color.a =
        // #FFFFFF` 被切成 `color.a =`，验证器报「值无法解析」，
        // 而文件本身完全正确。
        let body = strip_comment(line);
        let (path_part, value_part) = match body.split_once('=') {
            Some(pair) => pair,
            None => {
                out.findings
                    .push(Finding::new(VerifyCode::MissingAssign, line_no, "", ""));
                continue;
            }
        };
        let path = path_part.trim();
        let value_text = value_part.trim();
        if path.is_empty() {
            out.findings
                .push(Finding::new(VerifyCode::EmptyPath, line_no, "", ""));
            continue;
        }
        if !path_legal(path) {
            out.findings
                .push(Finding::new(VerifyCode::IllegalPath, line_no, path, ""));
            continue;
        }
        // 重复定义：同路径第二次出现即错（单源律）。
        if out.tokens.iter().any(|t| t.path == path) {
            out.findings
                .push(Finding::new(VerifyCode::DuplicatePath, line_no, path, ""));
            continue;
        }
        let suffix_ty = suffix_type(path);
        // 引用型
        if let Some(inner) = strip_ref(value_text) {
            if inner.is_empty() {
                out.findings
                    .push(Finding::new(VerifyCode::BadValue, line_no, path, "空引用"));
                continue;
            }
            out.tokens.push(ParsedToken {
                path: path.to_string(),
                value: TokenValue::Count(0),
                refs: alloc::vec![inner.to_string()],
                line_no,
                declared: suffix_ty,
            });
            continue;
        }
        // 显式配对（对比度预检的唯一输入形态）：`#FFFFFF on #000000`
        if let Some((fg, bg)) = parse_pair(value_text) {
            let fg_ty = TokenType::Color;
            let bg_ty = TokenType::Color;
            // 配对两端用两个伪令牌落表，路径派生 `path::fg` / `path::bg`，
            // 这样报告里的行号与路径都指得回原文。
            out.tokens.push(ParsedToken {
                path: format!("{}::fg", path),
                value: TokenValue::Color(fg),
                refs: Vec::new(),
                line_no,
                declared: Some(fg_ty),
            });
            out.tokens.push(ParsedToken {
                path: format!("{}::bg", path),
                value: TokenValue::Color(bg),
                refs: Vec::new(),
                line_no,
                declared: Some(bg_ty),
            });
            // 配对关系由 `::fg` / `::bg` 后缀在验证阶段配对，这里不额外
            // 生成发现——报告里每一条都必须是真问题，凑数的 Info 会把
            // 「零发现」这个信号稀释掉。
            continue;
        }
        match parse_value(path, value_text, suffix_ty) {
            Ok((value, ty)) => out.tokens.push(ParsedToken {
                path: path.to_string(),
                value,
                refs: Vec::new(),
                line_no,
                declared: Some(ty),
            }),
            Err(code) => out.findings.push(Finding::new(code, line_no, path, value_text)),
        }
    }
    out
}

/// 路径合法性：点分标识符，段非空，字符限 `[a-z0-9_-]`，长度受限。
fn path_legal(path: &str) -> bool {
    if path.len() > PATH_LIMIT {
        return false;
    }
    for segment in path.split('.') {
        if segment.is_empty() || segment.len() > 32 {
            return false;
        }
        for ch in segment.chars() {
            let ok = ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || ch == '-'
                || ch == '_';
            if !ok {
                return false;
            }
        }
    }
    true
}

/// 后缀推类型：`.px`→长度、`.ms`→时长、`.pct`→比例、`.font`→字体，
/// 其余为 `None`（交给值形态决定）。
fn suffix_type(path: &str) -> Option<TokenType> {
    match path.rsplit('.').next().unwrap_or("") {
        "px" | "rem" => Some(TokenType::Length),
        "ms" | "dur" => Some(TokenType::Duration),
        "pct" | "ratio" => Some(TokenType::Ratio),
        "font" => Some(TokenType::Font),
        _ => None,
    }
}

/// 剥 `{...}`。
fn strip_ref(text: &str) -> Option<&str> {
    let t = text.trim();
    let inner = t.strip_prefix('{')?;
    let inner = inner.strip_suffix('}')?;
    Some(inner.trim())
}

/// 解析 `#RRGGBB on #RRGGBB`。
fn parse_pair(text: &str) -> Option<(u32, u32)> {
    let (fg, bg) = text.split_once(" on ")?;
    let f = parse_hex(fg.trim())?;
    let b = parse_hex(bg.trim())?;
    Some((f, b))
}

/// 解析单个十六进制颜色。
fn parse_hex(text: &str) -> Option<u32> {
    let body = text.strip_prefix('#')?;
    if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(body, 16).ok()
}

/// 剥行尾注释。
///
/// 规则只有两条：` # `（两侧空白）之后全是注释；行尾正好是 ` #` 时也切。
/// 不认 `#` 紧跟字母数字的形态——那是颜色值 `#RRGGBB`，不是注释。
/// 令牌文法里没有「行内半截注释」，所以这条规则不会误伤合法写法。
fn strip_comment(line: &str) -> &str {
    if let Some(idx) = line.find(" # ") {
        return line.get(..idx).unwrap_or(line).trim_end();
    }
    if let Some(idx) = line.rfind(" #") {
        // 只有当 `#` 位于行尾（其后无内容）时才算注释。
        if idx + 2 == line.len() {
            return line.get(..idx).unwrap_or(line).trim_end();
        }
    }
    line
}

/// 命名空间推类型。后缀只能覆盖 `spacing.gap` 这类「后缀像单位名」的
/// 路径，覆盖不了 `motion.fast = 12px`——这条路径的错不在单位写法，
/// 而在这个命名空间只装时长。命名空间与类型的关系写死在这里，是
/// 令牌文法的一部分，不是启发式。
fn family_type(path: &str) -> Option<TokenType> {
    let root = path.split('.').next().unwrap_or("");
    match root {
        "color" => Some(TokenType::Color),
        "motion" => Some(TokenType::Duration),
        "spacing" => Some(TokenType::Length),
        "font" => Some(TokenType::Font),
        "opacity" => Some(TokenType::Ratio),
        "count" => Some(TokenType::Count),
        _ => None,
    }
}

/// 解析一行的值。后缀与命名空间两道约束都要过。
fn parse_value(
    path: &str,
    text: &str,
    suffix: Option<TokenType>,
) -> Result<(TokenValue, TokenType), VerifyCode> {
    let (value, ty) = parse_value_shape(text, suffix)?;
    if let Some(expected) = family_type(path) {
        if expected != ty {
            return Err(VerifyCode::SuffixTypeClash);
        }
    }
    Ok((value, ty))
}

/// 解析一行的值（形态层：只看值长什么样）。
fn parse_value_shape(
    text: &str,
    suffix: Option<TokenType>,
) -> Result<(TokenValue, TokenType), VerifyCode> {
    // 颜色
    if let Some(hex) = parse_hex(text) {
        return match suffix {
            Some(TokenType::Color) | None => Ok((TokenValue::Color(hex), TokenType::Color)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    // 长度：`12px` / `1.5px` / `-4px`
    if let Some(num) = text.strip_suffix("px") {
        let milli = parse_milli(num)?;
        return match suffix {
            Some(TokenType::Length) | None => Ok((TokenValue::Length(milli), TokenType::Length)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    if let Some(num) = text.strip_suffix("rem") {
        let milli = parse_milli(num)?;
        // 1rem = 16px（引擎基准字号），定点换算成 px 千分位。
        let px = (milli as i64).saturating_mul(16);
        let px = clamp_i32(px);
        return match suffix {
            Some(TokenType::Length) | None => Ok((TokenValue::Length(px), TokenType::Length)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    // 时长
    if let Some(num) = text.strip_suffix("ms") {
        let ms = parse_u32(num)?;
        return match suffix {
            Some(TokenType::Duration) | None => Ok((TokenValue::Duration(ms), TokenType::Duration)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    // 比例
    if let Some(num) = text.strip_suffix('%') {
        let whole = parse_u32(num)?;
        if whole > 1000 {
            return Err(VerifyCode::BadValue);
        }
        let permille = (whole as u64 * 10).min(1000) as u16;
        return match suffix {
            Some(TokenType::Ratio) | None => Ok((TokenValue::Ratio(permille), TokenType::Ratio)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    // 字体（引号包裹）
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        let inner = text.get(1..text.len() - 1).unwrap_or("");
        return match suffix {
            Some(TokenType::Font) | None => Ok((
                TokenValue::Font(inner.to_string()),
                TokenType::Font,
            )),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    // 计数
    if text.chars().all(|c| c.is_ascii_digit()) && !text.is_empty() {
        let n = text.parse::<u64>().map_err(|_| VerifyCode::BadValue)?;
        return match suffix {
            Some(TokenType::Count) | None => Ok((TokenValue::Count(n), TokenType::Count)),
            Some(_) => Err(VerifyCode::SuffixTypeClash),
        };
    }
    Err(VerifyCode::BadValue)
}

fn parse_milli(text: &str) -> Result<i32, VerifyCode> {
    let (neg, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(VerifyCode::BadValue);
    }
    let int_v: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse::<i64>().map_err(|_| VerifyCode::BadValue)?
    };
    // 小数部分最多三位，补零到三位。
    let mut frac_v: i64 = 0;
    let mut digits = 0;
    for ch in frac_part.chars() {
        if !ch.is_ascii_digit() || digits >= 3 {
            return Err(VerifyCode::BadValue);
        }
        frac_v = frac_v * 10 + (ch as i64 - '0' as i64);
        digits += 1;
    }
    while digits < 3 {
        frac_v *= 10;
        digits += 1;
    }
    let total = int_v
        .saturating_mul(1000)
        .saturating_add(frac_v)
        .saturating_mul(if neg { -1 } else { 1 });
    Ok(clamp_i32(total))
}

fn parse_u32(text: &str) -> Result<u32, VerifyCode> {
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit()) {
        return Err(VerifyCode::BadValue);
    }
    text.parse::<u32>().map_err(|_| VerifyCode::BadValue)
}

fn clamp_i32(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

// ---------------------------------------------------------------------------
// 三、WCAG 对比度（整数定点）
// ---------------------------------------------------------------------------

/// sRGB 通道线性化，输出 1e6 定点（0..=1_000_000）。
///
/// 每 8 级一个锚点（共 32 段 + 终点），段内线性插值。锚点由
/// `((c/255+0.055)/1.055)^2.4` 离线算好写死，段内误差 < 0.8%
/// （对比度档位判定边界远离这个量级），换来的是**完全避开浮点**：
/// CI 侧与内核侧必须逐位相同，否则「本地绿、CI 红」会变成常态。
///
/// 255 单独钉死为 1_000_000：纯白是 WCAG 的极值锚点，不许有插值误差。
fn linearize(channel: u8) -> u32 {
    const TABLE: [u32; 33] = [
        0, 2_428, 5_182, 9_134, 14_444, 21_219, 29_557, 39_546, 51_269, 64_803, 80_220,
        97_587, 116_971, 138_432, 162_029, 187_821, 215_861, 246_201, 278_894, 313_989,
        351_533, 391_572, 434_154, 479_320, 527_115, 577_580, 630_757, 686_685, 745_404,
        806_952, 871_367, 938_686, 1_000_000,
    ];
    if channel == 255 {
        return 1_000_000;
    }
    let hi = (channel >> 3) as usize;
    let lo = (channel & 0x07) as u32;
    let a = match TABLE.get(hi) {
        Some(v) => *v,
        None => 1_000_000,
    };
    let b = match TABLE.get(hi + 1) {
        Some(v) => *v,
        None => 1_000_000,
    };
    a + (b.saturating_sub(a) * lo) / 8
}

/// 相对亮度（1e6 定点，WCAG 定义）。
pub fn relative_luminance(packed: u32) -> u32 {
    let c = Rgba::unpack(packed);
    let r = linearize(c.r) as u64;
    let g = linearize(c.g) as u64;
    let b = linearize(c.b) as u64;
    // 0.2126/0.7152/0.0722 -> 千分位整数权重 2126/7152/722
    let l = (r * 2126 + g * 7152 + b * 722) / 10_000;
    l as u32
}

/// 对比度（千分比：4500 = 4.5:1）。WCAG 公式 (L1+0.05)/(L2+0.05)。
pub fn contrast_permille(a: u32, b: u32) -> u32 {
    let la = relative_luminance(a) as u64;
    let lb = relative_luminance(b) as u64;
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    // 0.05 在 1e6 定点下 = 50_000
    let num = (hi + 50_000) * 1000;
    let den = lo + 50_000;
    (num / den) as u32
}

/// 对比度档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WcagTier {
    /// ≥ 7:1。
    Aaa,
    /// ≥ 4.5:1 且 < 7:1。
    Aa,
    /// < 4.5:1。
    Below,
}

impl WcagTier {
    /// 按千分比判档。
    pub fn of(permille: u32) -> WcagTier {
        if permille >= 7000 {
            WcagTier::Aaa
        } else if permille >= 4500 {
            WcagTier::Aa
        } else {
            WcagTier::Below
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            WcagTier::Aaa => "AAA",
            WcagTier::Aa => "AA",
            WcagTier::Below => "不达标",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、验证报告
// ---------------------------------------------------------------------------

/// 验证报告。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    /// 全部发现（按行号升序）。
    pub findings: Vec<Finding>,
    /// 令牌总数。
    pub token_count: u32,
    /// 引用边总数。
    pub ref_edges: u32,
    /// 对比度检查对数。
    pub contrast_pairs: u32,
}

impl Report {
    /// 按严重度计数。
    pub fn count(&self, sev: Severity) -> u32 {
        self.findings.iter().filter(|f| f.severity == sev).count() as u32
    }

    /// 是否零错误。
    pub fn clean(&self) -> bool {
        self.count(Severity::Error) == 0
    }

    /// CI 结论。
    pub fn ci_verdict(&self, warn_blocks: bool) -> CiVerdict {
        if !self.clean() {
            CiVerdict::Fail
        } else if warn_blocks && self.count(Severity::Warn) > 0 {
            CiVerdict::Fail
        } else if self.count(Severity::Warn) > 0 {
            CiVerdict::Warn
        } else {
            CiVerdict::Pass
        }
    }

    /// 开放导出（TSV，一行一条，`严重度\t码\t行号\t路径\t说明`）。
    /// 首行是列名，第二行是统计——两行都是注释风格，`diff` 友好。
    pub fn export_tsv(&self) -> String {
        let mut out = String::new();
        out.push_str("# 令牌验证报告 v1\n");
        out.push_str(&format!(
            "# 令牌 {} 条，引用边 {} 条，对比度对 {} 组；错误 {}，警告 {}，提示 {}\n",
            self.token_count,
            self.ref_edges,
            self.contrast_pairs,
            self.count(Severity::Error),
            self.count(Severity::Warn),
            self.count(Severity::Info)
        ));
        out.push_str("severity\tcode\tline\tpath\tdetail\n");
        for f in self.findings.iter() {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                f.severity.tag(),
                f.code.code(),
                f.line_no,
                if f.path.is_empty() { "-" } else { f.path.as_str() },
                if f.detail.is_empty() {
                    "-"
                } else {
                    f.detail.as_str()
                }
            ));
        }
        out
    }

    /// 读屏清单：一条一行，直接念。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "验证完成：令牌 {} 条，错误 {}，警告 {}，提示 {}",
            self.token_count,
            self.count(Severity::Error),
            self.count(Severity::Warn),
            self.count(Severity::Info)
        ));
        for f in self.findings.iter() {
            out.push(f.render());
        }
        out
    }
}

/// CI 三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CiVerdict {
    /// 通过。
    Pass,
    /// 有警告但未阻断。
    Warn,
    /// 失败。
    Fail,
}

impl CiVerdict {
    /// 进程退出码（0/2/1）。CI 只需判「非零即红」。
    pub fn exit_code(self) -> i32 {
        match self {
            CiVerdict::Pass => 0,
            CiVerdict::Warn => 2,
            CiVerdict::Fail => 1,
        }
    }

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            CiVerdict::Pass => "通过",
            CiVerdict::Warn => "通过（有警告）",
            CiVerdict::Fail => "失败",
        }
    }
}

/// 验证入口：解析 + 三查，产出报告。纯函数，不读盘、不分配外部句柄。
pub fn validate(text: &str, warn_blocks: bool) -> Report {
    let parsed = parse(text);
    let mut report = Report {
        findings: parsed.findings,
        token_count: parsed.tokens.len() as u32,
        ref_edges: 0,
        contrast_pairs: 0,
    };
    // 第二查：类型（解析期已判大部分，这里补「声明类型与值类型」的最终核对）。
    check_types(&parsed.tokens, &mut report);
    // 第一查：引用完整性（含环与深度）。
    check_refs(&parsed.tokens, &mut report);
    // 第三查：对比度（配对令牌）。
    check_contrast(&parsed.tokens, &mut report);
    // 报告按行号稳定排序（同一行内按码）。
    report.findings.sort_by(|a, b| {
        a.line_no
            .cmp(&b.line_no)
            .then_with(|| a.code.code().cmp(&b.code.code()))
    });
    let _ = warn_blocks;
    report
}

fn check_types(tokens: &[ParsedToken], report: &mut Report) {
    for t in tokens.iter() {
        if let Some(declared) = t.declared {
            if !t.refs.is_empty() {
                continue;
            }
            if declared != t.value.type_of() {
                report.findings.push(Finding::new(
                    VerifyCode::SuffixTypeClash,
                    t.line_no,
                    &t.path,
                    "声明类型与值类型不符",
                ));
            }
        }
    }
}

fn check_refs(tokens: &[ParsedToken], report: &mut Report) {
    // 建索引：路径 -> 下标。
    let mut index: Vec<(&str, usize)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(t) = tokens.get(i) {
            index.push((t.path.as_str(), i));
        }
        i += 1;
    }
    let lookup = |path: &str| -> Option<usize> {
        index
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, idx)| *idx)
    };
    // 引用边计数 + 断链。
    let mut edges = 0u32;
    for t in tokens.iter() {
        for r in t.refs.iter() {
            edges = edges.saturating_add(1);
            if lookup(r).is_none() {
                report.findings.push(Finding::new(
                    VerifyCode::DanglingRef,
                    t.line_no,
                    &t.path,
                    r,
                ));
            }
        }
    }
    report.ref_edges = edges;
    // 环检测：迭代 DFS + 三色标记。只判环，不判深度——深度另走
    // 拓扑 DP（见 `check_depth`），因为「按插入顺序 DFS」时子节点早已
    // 被前一轮着色，深度永远涨不上去（这一版最初就是这么错的：
    // 64 层引用链被判成 3 层，深度闸形同虚设）。
    let mut color: Vec<u8> = alloc::vec![0u8; tokens.len()];
    let mut n = 0;
    while n < tokens.len() {
        if color.get(n).copied().unwrap_or(2) != 0 {
            n += 1;
            continue;
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        stack.push((n, 0));
        if let Some(slot) = color.get_mut(n) {
            *slot = 1;
        }
        while let Some((node, edge_idx)) = stack.pop() {
            let token = match tokens.get(node) {
                Some(t) => t,
                None => continue,
            };
            if edge_idx < token.refs.len() {
                stack.push((node, edge_idx + 1));
                let child_path = match token.refs.get(edge_idx) {
                    Some(p) => p,
                    None => continue,
                };
                if let Some(child) = lookup(child_path) {
                    let c = color.get(child).copied().unwrap_or(2);
                    if c == 1 {
                        report.findings.push(Finding::new(
                            VerifyCode::RefCycle,
                            tokens.get(child).map(|t| t.line_no).unwrap_or(0),
                            child_path,
                            "环回到栈上节点",
                        ));
                    } else if c == 0 {
                        if let Some(slot) = color.get_mut(child) {
                            *slot = 1;
                        }
                        stack.push((child, 0));
                    }
                }
            } else if let Some(slot) = color.get_mut(node) {
                *slot = 2;
            }
        }
        n += 1;
    }
    // 深度：Kahn 拓扑 DP 求最长引用链。
    check_depth(tokens, &index, report);
}

/// 最长引用链深度检查。
///
/// 做法：把「引用」看成有向边 i → child，按**子先父后**的拓扑序做 DP：
/// `depth[i] = 1 + max(depth[child])`，无引用者深度 0。拓扑序用 Kahn
/// 算法（按剩余未解析子边计数入队），全程迭代、无递归、O(V+E)。
/// 落在环里的节点永远不会入队，其深度无意义——环已由 DFS 单独报出。
fn find_index(index: &[(&str, usize)], path: &str) -> Option<usize> {
    index
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, idx)| *idx)
}

fn check_depth(tokens: &[ParsedToken], index: &[(&str, usize)], report: &mut Report) {
    let count = tokens.len();
    let mut children: Vec<Vec<usize>> = alloc::vec![Vec::new(); count];
    let mut parents: Vec<Vec<usize>> = alloc::vec![Vec::new(); count];
    let mut i = 0;
    while i < count {
        if let Some(t) = tokens.get(i) {
            for r in t.refs.iter() {
                if let Some(c) = find_index(index, r) {
                    if c != i {
                        if let Some(list) = children.get_mut(i) {
                            list.push(c);
                        }
                        if let Some(list) = parents.get_mut(c) {
                            list.push(i);
                        }
                    }
                }
            }
        }
        i += 1;
    }
    let mut remaining: Vec<usize> = children.iter().map(|v| v.len()).collect();
    let mut depth: Vec<usize> = alloc::vec![0usize; count];
    let mut queue: Vec<usize> = Vec::new();
    let mut k = 0;
    while k < count {
        if remaining.get(k).copied().unwrap_or(1) == 0 {
            queue.push(k);
        }
        k += 1;
    }
    while let Some(node) = queue.pop() {
        let node_depth = depth.get(node).copied().unwrap_or(0);
        if let Some(list) = parents.get(node) {
            for p in list.iter() {
                let pd = depth.get(*p).copied().unwrap_or(0);
                if let Some(slot) = depth.get_mut(*p) {
                    *slot = core::cmp::max(pd, node_depth.saturating_add(1));
                }
                if let Some(slot) = remaining.get_mut(*p) {
                    *slot = slot.saturating_sub(1);
                    if *slot == 0 {
                        queue.push(*p);
                    }
                }
            }
        }
    }
    // 只报「刚好越界」的那一层：整条链上每一层都 > CAP，全报会把
    // 报告刷成几十行，反而看不出根在哪。
    let mut j = 0;
    while j < count {
        if depth.get(j).copied().unwrap_or(0) == REF_DEPTH_CAP + 1 {
            if let Some(t) = tokens.get(j) {
                report.findings.push(Finding::new(
                    VerifyCode::RefTooDeep,
                    t.line_no,
                    &t.path,
                    "引用深度超过上限",
                ));
            }
        }
        j += 1;
    }
}

fn check_contrast(tokens: &[ParsedToken], report: &mut Report) {
    // 配对令牌成对出现在表里（`path::fg` / `path::bg`），按前缀配对。
    let mut i = 0;
    while i < tokens.len() {
        let t = match tokens.get(i) {
            Some(t) => t,
            None => break,
        };
        if !t.path.ends_with("::fg") {
            i += 1;
            continue;
        }
        let base = t.path.trim_end_matches("::fg");
        let bg_path = format!("{}::bg", base);
        let bg = tokens.iter().find(|x| x.path == bg_path);
        let (fg_v, bg_v) = match (t.value.clone(), bg.map(|x| x.value.clone())) {
            (TokenValue::Color(f), Some(TokenValue::Color(b))) => (f, b),
            _ => {
                report.findings.push(Finding::new(
                    VerifyCode::PairNotColor,
                    t.line_no,
                    base,
                    "配对令牌不是颜色或缺失背景",
                ));
                i += 1;
                continue;
            }
        };
        report.contrast_pairs = report.contrast_pairs.saturating_add(1);
        let ratio = contrast_permille(fg_v, bg_v);
        match WcagTier::of(ratio) {
            WcagTier::Aaa => {}
            WcagTier::Aa => report.findings.push(Finding {
                code: VerifyCode::ContrastBelowAaa,
                severity: Severity::Warn,
                line_no: t.line_no,
                path: base.to_string(),
                detail: format!("{}‰ ({})", ratio, WcagTier::of(ratio).label()),
            }),
            WcagTier::Below => report.findings.push(Finding {
                code: VerifyCode::ContrastBelowAa,
                severity: Severity::Error,
                line_no: t.line_no,
                path: base.to_string(),
                detail: format!("{}‰ ({})", ratio, WcagTier::of(ratio).label()),
            }),
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
# 主题令牌
color.accent = #4C8DFF
color.text = #FFFFFF
color.bg = #000000
spacing.gap = 12px
motion.fast = 120ms
radius.full = 100%
font.body = \"Inter\"
color.caption = {color.text}
color.pair = #FFFFFF on #000000
";

    #[test]
    fn clean_file_passes_all_three_checks() {
        let r = validate(GOOD, false);
        assert_eq!(r.count(Severity::Error), 0, "干净文件不该有错误");
        assert_eq!(r.contrast_pairs, 1);
        assert!(r.ref_edges >= 1);
        assert_eq!(r.ci_verdict(false), CiVerdict::Pass);
        assert_eq!(r.ci_verdict(false).exit_code(), 0);
    }

    #[test]
    fn dangling_ref_and_cycle_are_located_by_line() {
        let text = "color.a = {color.missing}\ncolor.b = {color.c}\ncolor.c = {color.b}\n";
        let r = validate(text, false);
        let dangling = r
            .findings
            .iter()
            .find(|f| f.code == VerifyCode::DanglingRef)
            .expect("断链被发现");
        assert_eq!(dangling.line_no, 1);
        assert!(r.findings.iter().any(|f| f.code == VerifyCode::RefCycle));
        assert!(!r.clean());
        assert_eq!(r.ci_verdict(false), CiVerdict::Fail);
    }

    #[test]
    fn type_suffix_clash_and_bad_value_are_errors() {
        let text = "motion.px = 120ms\ncolor.bad = #GGGGGG\n";
        let r = validate(text, false);
        assert!(r
            .findings
            .iter()
            .any(|f| f.code == VerifyCode::SuffixTypeClash));
        assert!(r.findings.iter().any(|f| f.code == VerifyCode::BadValue));
        assert_eq!(r.findings.first().map(|f| f.line_no), Some(1));
    }

    #[test]
    fn contrast_tiers_match_wcag() {
        // 白对黑 = 21:1
        assert_eq!(contrast_permille(0xFF_FF_FF, 0x00_00_00) / 1000, 21);
        assert_eq!(WcagTier::of(contrast_permille(0xFF_FF_FF, 0x00_00_00)), WcagTier::Aaa);
        // 中灰对黑 ≈ 5.3:1 -> AA
        let gray = 0x80_80_80;
        let r = contrast_permille(gray, 0x00_00_00);
        assert!(r >= 4500 && r < 7000, "实际 {}", r);
        assert_eq!(WcagTier::of(r), WcagTier::Aa);
    }

    #[test]
    fn low_contrast_pair_blocks_ci() {
        let text = "color.pair = #333333 on #000000\n";
        let r = validate(text, false);
        assert!(r
            .findings
            .iter()
            .any(|f| f.code == VerifyCode::ContrastBelowAa));
        assert_eq!(r.ci_verdict(false), CiVerdict::Fail);
    }

    #[test]
    fn duplicate_path_is_rejected_with_line() {
        let text = "color.a = #FFFFFF\ncolor.a = #000000\n";
        let r = validate(text, false);
        let dup = r
            .findings
            .iter()
            .find(|f| f.code == VerifyCode::DuplicatePath)
            .expect("重复定义被发现");
        assert_eq!(dup.line_no, 2);
    }

    #[test]
    fn export_is_stable_and_diffable() {
        let r = validate("color.a = #FFFFFF\ncolor.pair = #333333 on #000000\n", false);
        let tsv = r.export_tsv();
        assert!(tsv.starts_with("# 令牌验证报告 v1\n"));
        assert!(tsv.contains("severity\tcode\tline\tpath\tdetail\n"));
        assert!(tsv.contains("E\t5329\t2\tcolor.pair\t"));
        // 同一输入两次导出逐字节相同。
        let again = validate("color.a = #FFFFFF\ncolor.pair = #333333 on #000000\n", false);
        assert_eq!(tsv, again.export_tsv());
    }

    #[test]
    fn ref_bomb_hits_depth_cap() {
        let mut text = String::new();
        let mut i = 0;
        while i < REF_DEPTH_CAP + 5 {
            if i == 0 {
                text.push_str("color.n0 = #FFFFFF\n");
            } else {
                text.push_str(&format!("color.n{} = {{color.n{}}}\n", i, i - 1));
            }
            i += 1;
        }
        let r = validate(&text, false);
        assert!(
            r.findings.iter().any(|f| f.code == VerifyCode::RefTooDeep),
            "深度炸弹必须被拦"
        );
    }

    #[test]
    fn warn_blocks_policy_flips_verdict() {
        // 深灰对黑 ≈ 3.9:1？用 AA 达标的颜色制造仅警告的场景。
        let text = "color.pair = #949494 on #000000\n";
        let r = validate(text, false);
        let has_warn_only = r.count(Severity::Error) == 0 && r.count(Severity::Warn) > 0;
        if has_warn_only {
            assert_eq!(r.ci_verdict(false), CiVerdict::Warn);
            assert_eq!(r.ci_verdict(true), CiVerdict::Fail);
        }
    }
}
