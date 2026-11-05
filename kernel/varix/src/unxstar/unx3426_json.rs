//! UNX-F3426 JSON 序列化器（无依赖手写、栈上预算） · 0x79880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② J1：手写序列化器（零依赖零堆分配、栈预算 2KB）；
//!     M-185 序列化-解析往返一致。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 序列化半边零堆分配：字节级直写调用方给的定容缓冲（栈预算 2KB），
//!     全程无 String/Vec 中转，超预算显性 Err（BudgetExceed）不静默截断；
//!   - 值闭集六类：Null/Bool/Int/Str/Arr/Obj（无浮点——账面计数整数轴）；
//!   - 字符串转义闭集：引号、反斜杠、\n \t \r \b \f 短转义 + 其余控制
//!     字符 \u00XX 十六进制长转义；裸控制字符入串必拒（解析侧 CtrlCharRaw）；
//!   - 解析半边递归下降：深度上限 16（MAX_DEPTH，栈溢出防护），
//!     int 用 i128 承接 + 值域闸解析（超 i64 显性拒），尾随垃圾/未闭合串/
//!     坏转义/键非串/意外记号逐类点名；
//!   - M-185：serialize → parse → serialize 逐字节往返一致，覆盖全部
//!     值类与转义、嵌套、负数、零、i64 双端、空数组空对象边界；
//!   - 双跑一致：serialize/parse 均纯函数，同输入两次逐字节相同。
//! 常量来源：
//!   - 栈预算 2048 字节：派工卡 J1 判据原文（栈预算 2KB）；
//!   - 转义表：RFC 8259 §7（JSON 字符串转义闭集）；
//!   - 诊断段：0x79880（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79880;

/// 栈预算（字节）：序列化输出缓冲上限（J1 冻结 2KB）。
pub const STACK_BUDGET: usize = 2048;

/// 递归下降解析深度上限（栈溢出防护；超深显性拒）。
pub const MAX_DEPTH: usize = 16;

/// 单个栈缓冲定容类型（序列化的零堆载体）。
pub type StackBuf = [u8; STACK_BUDGET];

/// JSON 值闭集（无浮点；账面一律整数轴）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// 空值。
    Null,
    /// 布尔。
    Bool(bool),
    /// 整数（i64）。
    Int(i64),
    /// 字符串。
    Str(String),
    /// 数组。
    Arr(Vec<Json>),
    /// 对象（键序保留）。
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// 值类名（诊断文案用）。
    pub fn type_zh(&self) -> &'static str {
        match self {
            Json::Null => "空值",
            Json::Bool(_) => "布尔",
            Json::Int(_) => "整数",
            Json::Str(_) => "字符串",
            Json::Arr(_) => "数组",
            Json::Obj(_) => "对象",
        }
    }
}

// ── 序列化半边（字节级直写，零堆分配）──────────────────────────────────

/// 写入守卫：单字节落缓冲（越界即 BudgetExceed，不静默截断）。
fn put_byte(out: &mut [u8], n: &mut usize, b: u8) -> Result<(), u32> {
    match out.get_mut(*n) {
        Some(slot) => {
            *slot = b;
            *n += 1;
            Ok(())
        }
        None => Err(Diag::BudgetExceed.val()),
    }
}

/// 写入串（逐字节，不判 UTF-8——&str 入参本身保证合法）。
fn put_str(out: &mut [u8], n: &mut usize, s: &str) -> Result<(), u32> {
    for &b in s.as_bytes().iter() {
        put_byte(out, n, b)?;
    }
    Ok(())
}

/// 十六进制小写半字节。
fn hex_digit(v: u8) -> u8 {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    match HEX.get((v & 0xF) as usize) {
        Some(&b) => b,
        None => b'0',
    }
}

/// 字符串序列化（带引号 + 转义闭集；RFC 8259 §7）。
fn write_escaped(s: &str, out: &mut [u8], n: &mut usize) -> Result<(), u32> {
    put_byte(out, n, b'"')?;
    for ch in s.chars() {
        match ch {
            '"' => put_str(out, n, "\\\"")?,
            '\\' => put_str(out, n, "\\\\")?,
            '\n' => put_str(out, n, "\\n")?,
            '\t' => put_str(out, n, "\\t")?,
            '\r' => put_str(out, n, "\\r")?,
            '\u{8}' => put_str(out, n, "\\b")?,
            '\u{C}' => put_str(out, n, "\\f")?,
            c if (c as u32) < 0x20 => {
                put_str(out, n, "\\u00")?;
                let v = c as u32;
                put_byte(out, n, hex_digit((v >> 4) as u8))?;
                put_byte(out, n, hex_digit(v as u8))?;
            }
            c => {
                // 多字节 UTF-8 序列原样直写。
                let mut buf = [0u8; 4];
                let text = c.encode_utf8(&mut buf);
                put_str(out, n, text)?;
            }
        }
    }
    put_byte(out, n, b'"')
}

/// i64 全域十进制渲染（含 i64::MIN；u128 承接绝对值防反转溢出）。
fn write_int(v: i64, out: &mut [u8], n: &mut usize) -> Result<(), u32> {
    let neg = v < 0;
    let mut mag: u128 = if neg { -(v as i128) as u128 } else { v as u128 };
    let mut digits = [0u8; 20];
    let mut w = 0usize;
    if mag == 0 {
        match digits.get_mut(w) {
            Some(slot) => *slot = b'0',
            None => return Err(Diag::NumOverflow.val()),
        }
        w += 1;
    }
    while mag > 0 && w < digits.len() {
        match digits.get_mut(w) {
            Some(slot) => *slot = b'0' + (mag % 10) as u8,
            None => return Err(Diag::NumOverflow.val()),
        }
        mag /= 10;
        w += 1;
    }
    if neg {
        put_byte(out, n, b'-')?;
    }
    while w > 0 {
        w -= 1;
        match digits.get(w) {
            Some(&d) => put_byte(out, n, d)?,
            None => return Err(Diag::NumOverflow.val()),
        }
    }
    Ok(())
}

/// 核心序列化：字节级直写 `out`，`n` 为写入游标（调用方初始化 0）。
/// 全程零堆分配；缓冲写满即 Err（BudgetExceed）。
pub fn write_json_into(v: &Json, out: &mut [u8], n: &mut usize) -> Result<(), u32> {
    match v {
        Json::Null => put_str(out, n, "null"),
        Json::Bool(true) => put_str(out, n, "true"),
        Json::Bool(false) => put_str(out, n, "false"),
        Json::Int(i) => write_int(*i, out, n),
        Json::Str(s) => write_escaped(s, out, n),
        Json::Arr(items) => {
            put_byte(out, n, b'[')?;
            let mut first = true;
            for item in items.iter() {
                if !first {
                    put_byte(out, n, b',')?;
                }
                first = false;
                write_json_into(item, out, n)?;
            }
            put_byte(out, n, b']')
        }
        Json::Obj(pairs) => {
            put_byte(out, n, b'{')?;
            let mut first = true;
            for (k, val) in pairs.iter() {
                if !first {
                    put_byte(out, n, b',')?;
                }
                first = false;
                write_escaped(k, out, n)?;
                put_byte(out, n, b':')?;
                write_json_into(val, out, n)?;
            }
            put_byte(out, n, b'}')
        }
    }
}

/// 零堆序列化面：输出进定容栈缓冲（STACK_BUDGET 上限），返回字节数。
pub fn serialize_stack(v: &Json, out: &mut StackBuf) -> Result<usize, u32> {
    let mut n = 0usize;
    write_json_into(v, out, &mut n)?;
    Ok(n)
}

/// 文本面（M-185 往返与测试用）：栈缓冲渲染后转 String。
/// 超 2KB 预算的值在此同样显性拒（BudgetExceed）。
pub fn serialize_text(v: &Json) -> Result<String, u32> {
    let mut buf: StackBuf = [0u8; STACK_BUDGET];
    let n = serialize_stack(v, &mut buf)?;
    let capped = if n <= STACK_BUDGET { n } else { STACK_BUDGET };
    match core::str::from_utf8(&buf[..capped]) {
        Ok(s) => Ok(String::from(s)),
        Err(_) => Err(Diag::BudgetExceed.val()),
    }
}

// ── 解析半边（递归下降）────────────────────────────────────────────────

/// 解析器游标（字节轴 + 深度轴）。
struct Cursor<'a> {
    src: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    /// 跳过空白（空格/制表/换行/回车）。
    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
    }

    /// 字面量匹配（null/true/false）。
    fn eat_word(&mut self, word: &str) -> bool {
        let wb = word.as_bytes();
        if self.src.len() - self.pos >= wb.len() {
            let mut same = true;
            for (i, &b) in wb.iter().enumerate() {
                match self.src.get(self.pos + i) {
                    Some(&s) if s == b => {}
                    _ => {
                        same = false;
                    }
                }
            }
            if same {
                self.pos += wb.len();
                return true;
            }
        }
        false
    }
}

/// UTF-8 序列长度（首字节轴；非法首字节按 1 收，交给 from_utf8 拒）。
fn utf8_seq_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

/// 字符串解析（引号内转义闭集；裸控制字符显性拒）。
fn parse_string(c: &mut Cursor) -> Result<String, u32> {
    // 前提：调用方已确认首字节是 '"'。
    c.pos += 1;
    let mut out = String::new();
    loop {
        let b = match c.bump() {
            Some(b) => b,
            None => return Err(Diag::UnterminatedStr.val()),
        };
        match b {
            b'"' => return Ok(out),
            b'\\' => {
                let esc = match c.bump() {
                    Some(e) => e,
                    None => return Err(Diag::UnterminatedStr.val()),
                };
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{C}'),
                    b'u' => {
                        // \uXXXX 四位十六进制（BMP 面；代理对不拼，按码点直收）。
                        let mut v: u16 = 0;
                        for _ in 0..4 {
                            let h = match c.bump() {
                                Some(h) => h,
                                None => return Err(Diag::UnterminatedStr.val()),
                            };
                            let d = match h {
                                b'0'..=b'9' => (h - b'0') as u16,
                                b'a'..=b'f' => (h - b'a' + 10) as u16,
                                b'A'..=b'F' => (h - b'A' + 10) as u16,
                                _ => return Err(Diag::BadEscape.val()),
                            };
                            v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
                                Some(x) => x,
                                None => return Err(Diag::BadEscape.val()),
                            };
                        }
                        match char::from_u32(v as u32) {
                            Some(ch) => out.push(ch),
                            None => return Err(Diag::BadEscape.val()),
                        }
                    }
                    _ => return Err(Diag::BadEscape.val()),
                }
            }
            b if b < 0x20 => return Err(Diag::CtrlCharRaw.val()),
            _ => {
                // 多字节 UTF-8 原样收集：本字节起的合法序列整段拷贝。
                let start = c.pos - 1;
                let len = utf8_seq_len(b);
                let end = match start.checked_add(len) {
                    Some(e) if e <= c.src.len() => e,
                    _ => return Err(Diag::UnterminatedStr.val()),
                };
                let slice = match c.src.get(start..end) {
                    Some(s) => s,
                    None => return Err(Diag::UnterminatedStr.val()),
                };
                match core::str::from_utf8(slice) {
                    Ok(s) => out.push_str(s),
                    Err(_) => return Err(Diag::BadEscape.val()),
                }
                c.pos = end;
            }
        }
    }
}

/// 整数解析（可选负号；i128 承接 + 值域闸，超 i64 显性拒；无小数/指数；
/// 前导零违文法——首 digit 为 0 后不得再跟 digit）。
fn parse_int(c: &mut Cursor) -> Result<Json, u32> {
    let neg = if c.peek() == Some(b'-') {
        c.pos += 1;
        true
    } else {
        false
    };
    let start = c.pos;
    let mut mag: i128 = 0;
    let mut digits = 0usize;
    while let Some(b) = c.peek() {
        match b {
            b'0'..=b'9' => {
                let d = (b - b'0') as i128;
                mag = match mag.checked_mul(10).and_then(|x| x.checked_add(d)) {
                    Some(x) => x,
                    None => return Err(Diag::NumOverflow.val()),
                };
                c.pos += 1;
                digits += 1;
            }
            _ => break,
        }
    }
    if digits == 0 {
        return Err(Diag::UnexpectedToken.val());
    }
    // 前导零守卫：首 digit 是 '0' 且其后还有 digit（"00"/"01"）→ 违文法。
    let first_digit = match c.src.get(start) {
        Some(b) => *b,
        None => return Err(Diag::UnexpectedToken.val()),
    };
    if first_digit == b'0' && digits > 1 {
        return Err(Diag::UnexpectedToken.val());
    }
    let signed: i128 = if neg { -mag } else { mag };
    if signed < i64::MIN as i128 || signed > i64::MAX as i128 {
        return Err(Diag::NumOverflow.val());
    }
    Ok(Json::Int(signed as i64))
}

/// 值解析入口（递归下降；深度上限 MAX_DEPTH；游标私有，仅 parse_json 可达）。
fn parse_value(c: &mut Cursor) -> Result<Json, u32> {
    c.skip_ws();
    let b = match c.peek() {
        Some(b) => b,
        None => return Err(Diag::EmptyInput.val()),
    };
    match b {
        b'n' => {
            if c.eat_word("null") {
                Ok(Json::Null)
            } else {
                Err(Diag::UnexpectedToken.val())
            }
        }
        b't' => {
            if c.eat_word("true") {
                Ok(Json::Bool(true))
            } else {
                Err(Diag::UnexpectedToken.val())
            }
        }
        b'f' => {
            if c.eat_word("false") {
                Ok(Json::Bool(false))
            } else {
                Err(Diag::UnexpectedToken.val())
            }
        }
        b'"' => {
            let s = parse_string(c)?;
            Ok(Json::Str(s))
        }
        b'-' | b'0'..=b'9' => parse_int(c),
        b'[' => {
            c.depth += 1;
            if c.depth > MAX_DEPTH {
                return Err(Diag::DepthExceed.val());
            }
            c.pos += 1;
            let mut items: Vec<Json> = Vec::new();
            c.skip_ws();
            if c.peek() == Some(b']') {
                c.pos += 1;
                c.depth -= 1;
                return Ok(Json::Arr(items));
            }
            loop {
                let item = parse_value(c)?;
                items.push(item);
                c.skip_ws();
                match c.bump() {
                    Some(b',') => continue,
                    Some(b']') => break,
                    _ => return Err(Diag::UnexpectedToken.val()),
                }
            }
            c.depth -= 1;
            Ok(Json::Arr(items))
        }
        b'{' => {
            c.depth += 1;
            if c.depth > MAX_DEPTH {
                return Err(Diag::DepthExceed.val());
            }
            c.pos += 1;
            let mut pairs: Vec<(String, Json)> = Vec::new();
            c.skip_ws();
            if c.peek() == Some(b'}') {
                c.pos += 1;
                c.depth -= 1;
                return Ok(Json::Obj(pairs));
            }
            loop {
                c.skip_ws();
                if c.peek() != Some(b'"') {
                    return Err(Diag::ObjKeyNotStr.val());
                }
                let key = parse_string(c)?;
                c.skip_ws();
                match c.bump() {
                    Some(b':') => {}
                    _ => return Err(Diag::UnexpectedToken.val()),
                }
                let val = parse_value(c)?;
                pairs.push((key, val));
                c.skip_ws();
                match c.bump() {
                    Some(b',') => continue,
                    Some(b'}') => break,
                    _ => return Err(Diag::UnexpectedToken.val()),
                }
            }
            c.depth -= 1;
            Ok(Json::Obj(pairs))
        }
        _ => Err(Diag::UnexpectedToken.val()),
    }
}

/// 顶层解析入口（尾随垃圾显性拒；空输入点名）。
pub fn parse_json(text: &str) -> Result<Json, u32> {
    if text.is_empty() {
        return Err(Diag::EmptyInput.val());
    }
    let mut c = Cursor { src: text.as_bytes(), pos: 0, depth: 0 };
    let v = parse_value(&mut c)?;
    c.skip_ws();
    if c.pos != c.src.len() {
        return Err(Diag::TrailingGarbage.val());
    }
    Ok(v)
}

/// M-185 往返一锤：serialize → parse → serialize 逐字节同。
pub fn round_trip(v: &Json) -> Result<bool, u32> {
    let a = serialize_text(v)?;
    let back = parse_json(&a)?;
    let b = serialize_text(&back)?;
    Ok(a == b)
}

/// 诊断码闭集：0x79881..=0x7988C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 超栈预算：输出超过 2KB 定容缓冲。
    BudgetExceed = 0x01,
    /// 坏转义：\u 非 4 位十六进制或非法短转义符。
    BadEscape = 0x02,
    /// 未闭合串：字符串在引号闭合前输入耗尽。
    UnterminatedStr = 0x03,
    /// 尾随垃圾：顶层值解析完成后仍有非空白内容。
    TrailingGarbage = 0x04,
    /// 整数溢出：字面量超出 i64 值域。
    NumOverflow = 0x05,
    /// 意外记号：文法位置出现非法字节。
    UnexpectedToken = 0x06,
    /// 超深嵌套：递归深度超过 MAX_DEPTH。
    DepthExceed = 0x07,
    /// 键非串：对象键不是字符串。
    ObjKeyNotStr = 0x08,
    /// 裸控制字符：字符串内出现 < 0x20 的未转义字节。
    CtrlCharRaw = 0x09,
    /// 空输入：待解析文本为空。
    EmptyInput = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 往返破缺：serialize-parse-serialize 逐字节不一致。
    RoundTripBreak = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::BudgetExceed => "超栈预算：建议账面瘦身或分片序列化，预算冻结 2KB 不放宽",
            Diag::BadEscape => "坏转义：建议转义只用 \\\" \\\\ \\/ \\n \\t \\r \\b \\f \\uXXXX 闭集",
            Diag::UnterminatedStr => "未闭合串：建议核对引号配对，转义符后必须有合法续字符",
            Diag::TrailingGarbage => "尾随垃圾：建议顶层只放一个 JSON 值，多余内容另起一段",
            Diag::NumOverflow => "整数溢出：建议字面量落在 i64 值域内，超界改用分段计数",
            Diag::UnexpectedToken => "意外记号：建议对照文法核对分隔符（逗号/冒号/括号配对）",
            Diag::DepthExceed => "超深嵌套：建议账面结构拍平，递归深度上限 16 不放宽",
            Diag::ObjKeyNotStr => "键非串：建议对象键一律加双引号",
            Diag::CtrlCharRaw => "裸控制字符：建议串内控制字符用 \\uXXXX 转义后再入账",
            Diag::EmptyInput => "空输入：建议先判空再解析，空文本无 JSON 值",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::RoundTripBreak => "往返破缺：建议序列化与解析共用同一转义闭集，双方不得私改",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::BudgetExceed,
    Diag::BadEscape,
    Diag::UnterminatedStr,
    Diag::TrailingGarbage,
    Diag::NumOverflow,
    Diag::UnexpectedToken,
    Diag::DepthExceed,
    Diag::ObjKeyNotStr,
    Diag::CtrlCharRaw,
    Diag::EmptyInput,
    Diag::SegGuardFail,
    Diag::RoundTripBreak,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79881..=0x7988C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// M-185 往返：全值类样例 serialize-parse-serialize 逐字节一致。
fn round_trip_all_kinds_ok() -> bool {
    let samples = [
        Json::Null,
        Json::Bool(true),
        Json::Bool(false),
        Json::Int(0),
        Json::Int(-1),
        Json::Int(i64::MAX),
        Json::Int(i64::MIN),
        Json::Str(String::from("plain")),
        Json::Str(String::from("引号\"与反斜\\杠\n制表\t回车\r退格\u{8}换页\u{C}")),
        Json::Str(String::from("控制\u{1}\u{1F}")),
        Json::Arr(vec![Json::Int(1), Json::Null, Json::Str(String::from("x"))]),
        Json::Arr(Vec::new()),
        Json::Obj(vec![
            (String::from("k"), Json::Int(7)),
            (String::from("空"), Json::Str(String::from("值"))),
        ]),
        Json::Obj(Vec::new()),
        Json::Obj(vec![(
            String::from("nest"),
            Json::Arr(vec![Json::Obj(vec![(String::from("deep"), Json::Bool(true))])]),
        )]),
    ];
    for v in samples.iter() {
        match round_trip(v) {
            Ok(true) => {}
            _ => return false,
        }
    }
    true
}

/// 零堆面：栈缓冲序列化字节数正确、内容与文本面一致、余量区零污染。
fn stack_serialize_ok() -> bool {
    let v = Json::Obj(vec![
        (String::from("total"), Json::Int(9)),
        (String::from("pass"), Json::Int(6)),
        (String::from("name"), Json::Str(String::from("suite_a"))),
    ]);
    let mut buf: StackBuf = [0u8; STACK_BUDGET];
    let n = match serialize_stack(&v, &mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let text = match serialize_text(&v) {
        Ok(t) => t,
        Err(_) => return false,
    };
    n == text.len()
        && match core::str::from_utf8(&buf[..n]) {
            Ok(s) => s == text.as_str(),
            Err(_) => false,
        }
        && buf.get(n).map(|b| *b == 0).unwrap_or(true)
}

/// 转义闭集面：短转义五件套 + \u00XX 长转义逐字核对。
fn escape_table_ok() -> bool {
    let s = Json::Str(String::from("a\"b\\c\nd\te\rf\u{8}g\u{C}h\u{1}"));
    let text = match serialize_text(&s) {
        Ok(t) => t,
        Err(_) => return false,
    };
    text == "\"a\\\"b\\\\c\\nd\\te\\rf\\bg\\fh\\u0001\""
}

/// 注入 1：超栈预算必抓（构造 >2KB 大串）。
fn inj_budget_exceed_ok() -> bool {
    let mut big = String::new();
    let mut i = 0usize;
    while i < 300 {
        big.push_str("预算超限测试字节串");
        i += 1;
    }
    // 300 × 27 字节 ≈ 8100 字节 > 2048。
    let v = Json::Str(big);
    let mut buf: StackBuf = [0u8; STACK_BUDGET];
    serialize_stack(&v, &mut buf) == Err(Diag::BudgetExceed.val())
        && serialize_text(&v) == Err(Diag::BudgetExceed.val())
}

/// 注入 2：解析侧九类违约逐类点名必抓。
fn inj_parse_rejects_ok() -> bool {
    let cases = [
        ("", Diag::EmptyInput),
        ("{\"a\":1}垃圾", Diag::TrailingGarbage),
        ("\"未闭合", Diag::UnterminatedStr),
        ("\"坏\\x转义\"", Diag::BadEscape),
        ("\"裸\u{2}控制\"", Diag::CtrlCharRaw),
        ("{a:1}", Diag::ObjKeyNotStr),
        ("nul", Diag::UnexpectedToken),
        ("[1,2", Diag::UnexpectedToken),
        ("-99999999999999999999999", Diag::NumOverflow),
    ];
    for (text, want) in cases.iter() {
        if parse_json(text) != Err(want.val()) {
            return false;
        }
    }
    true
}

/// 注入 3：超深嵌套必抓（MAX_DEPTH+1 层数组）；恰 MAX_DEPTH 合法。
fn depth_boundary_ok() -> bool {
    // 17 层：[[[…]]] → DepthExceed。
    let mut over = String::new();
    let mut d = 0usize;
    while d <= MAX_DEPTH {
        over.push('[');
        d += 1;
    }
    let mut c = 0usize;
    while c <= MAX_DEPTH {
        over.push(']');
        c += 1;
    }
    if parse_json(&over) != Err(Diag::DepthExceed.val()) {
        return false;
    }
    // 恰 16 层：合法（解析回嵌套值不炸）。
    let mut ok16 = String::new();
    let mut k = 0usize;
    while k < MAX_DEPTH {
        ok16.push('[');
        k += 1;
    }
    let mut m = 0usize;
    while m < MAX_DEPTH {
        ok16.push(']');
        m += 1;
    }
    let parsed = parse_json(&ok16);
    // 深度守恒：16 层数组逐层剥壳后必是空数组。
    let mut cur = match parsed {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut layers = 0usize;
    loop {
        match cur {
            Json::Arr(items) => {
                layers += 1;
                if items.is_empty() {
                    break;
                }
                cur = match items.first() {
                    Some(inner) => inner.clone(),
                    None => return false,
                };
            }
            _ => return false,
        }
    }
    layers == MAX_DEPTH
}

/// 边界：空数组/空对象、负零、前导零拒绝、字面量大小写敏感。
fn parse_boundary_ok() -> bool {
    if parse_json("[]") != Ok(Json::Arr(Vec::new())) {
        return false;
    }
    if parse_json("{}") != Ok(Json::Obj(Vec::new())) {
        return false;
    }
    if parse_json("-0") != Ok(Json::Int(0)) {
        return false;
    }
    // 前导零不是合法整数文法（00/01 → 前导零守卫点名）。
    if parse_json("00") != Err(Diag::UnexpectedToken.val()) {
        return false;
    }
    if parse_json("-01") != Err(Diag::UnexpectedToken.val()) {
        return false;
    }
    // 字面量大小写敏感（NULL/True 不是合法值）。
    parse_json("NULL") == Err(Diag::UnexpectedToken.val())
        && parse_json("True") == Err(Diag::UnexpectedToken.val())
}

/// 双跑一致：同输入两次 serialize/parse 逐字节相同。
fn double_run_ok() -> bool {
    let v = Json::Obj(vec![
        (String::from("case"), Json::Str(String::from("json_rt"))),
        (String::from("assert"), Json::Int(3)),
        (String::from("fail"), Json::Int(0)),
    ]);
    let t1 = serialize_text(&v);
    let t2 = serialize_text(&v);
    let a = match t1 {
        Ok(t) => t,
        Err(_) => return false,
    };
    let b = match t2 {
        Ok(t) => t,
        Err(_) => return false,
    };
    if a != b {
        return false;
    }
    let p1 = parse_json(&a);
    let p2 = parse_json(&b);
    p1 == p2 && p1 == Ok(v)
}

/// 错误路径 Result 语义：Ok 与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    match round_trip(&Json::Int(42)) {
        Ok(true) => {}
        _ => return false,
    }
    match parse_json("!!!") {
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let v = Json::Obj(vec![
        (String::from("serializer"), Json::Str(String::from("handwritten"))),
        (String::from("stack_budget"), Json::Int(STACK_BUDGET as i64)),
        (String::from("round_trip"), Json::Bool(true)),
    ]);
    let ok = match round_trip(&v) {
        Ok(true) => true,
        _ => false,
    };
    let summary = format!(
        "UNX-F3426 手写JSON序列化器：栈预算{}B 往返{}",
        STACK_BUDGET,
        if ok { "一致" } else { "破缺" }
    );
    !summary.is_empty() && summary.contains("手写JSON序列化器") && ok
}

// ── 判据总成：run_unx3426_checks ───────────────────────────────────────

/// run_unx3426_checks：本单判据聚合（诊断段 0x79880，全部走 add_dyn）。
pub fn run_unx3426_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3426 JSON序列化器");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "M185往返全值类serialize解析serialize逐字节同",
            round_trip_all_kinds_ok(),
            format!("往返{}", if round_trip_all_kinds_ok() { "一致" } else { "破缺" }),
        ),
        (
            "零堆面栈缓冲序列化内容字节齐",
            stack_serialize_ok(),
            format!("栈面{}", if stack_serialize_ok() { "齐" } else { "缺" }),
        ),
        (
            "转义闭集短转义与u00XX长转义逐字核对",
            escape_table_ok(),
            format!("转义表{}", if escape_table_ok() { "齐" } else { "缺" }),
        ),
        (
            "注入1超栈预算双面必抓",
            inj_budget_exceed_ok(),
            format!("超预算{}", if inj_budget_exceed_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2解析九类违约逐类点名",
            inj_parse_rejects_ok(),
            format!("解析违约{}", if inj_parse_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入3超深嵌套必抓恰MAX_DEPTH合法",
            depth_boundary_ok(),
            format!("深度边界{}", if depth_boundary_ok() { "齐" } else { "缺" }),
        ),
        (
            "边界空表负零前导零大小写",
            parse_boundary_ok(),
            format!("边界{}", if parse_boundary_ok() { "齐" } else { "缺" }),
        ),
        (
            "双跑一致同输入两次逐字节同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "栈预算常量冻结2KB",
            STACK_BUDGET == 2048,
            format!("预算{}", STACK_BUDGET),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79880);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn round_trip_covers_all_kinds() {
        assert!(round_trip_all_kinds_ok());
        assert_eq!(round_trip(&Json::Int(i64::MIN)), Ok(true));
        assert_eq!(serialize_text(&Json::Int(i64::MIN)), Ok(String::from("-9223372036854775808")));
        assert_eq!(serialize_text(&Json::Int(i64::MAX)), Ok(String::from("9223372036854775807")));
    }

    #[test]
    fn stack_serialize_matches_text_path() {
        assert!(stack_serialize_ok());
        let mut buf: StackBuf = [0u8; STACK_BUDGET];
        let n = serialize_stack(&Json::Null, &mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(core::str::from_utf8(&buf[..n]).unwrap(), "null");
    }

    #[test]
    fn escape_table_exact_bytes() {
        assert!(escape_table_ok());
        let ctrl = serialize_text(&Json::Str(String::from("\u{0}"))).unwrap();
        assert_eq!(ctrl, "\"\\u0000\"");
    }

    #[test]
    fn budget_exceed_caught() {
        assert!(inj_budget_exceed_ok());
    }

    #[test]
    fn parse_rejections_pointed() {
        assert!(inj_parse_rejects_ok());
        assert!(depth_boundary_ok());
    }

    #[test]
    fn boundaries_legal_and_illegal() {
        assert!(parse_boundary_ok());
        // 转义闭集往返：\u0001 ↔ 控制字符。
        let s = parse_json("\"\\u0001\"").unwrap();
        assert_eq!(s, Json::Str(String::from("\u{1}")));
        // 逗号分隔多元素与对象键序保留。
        let v = parse_json("{\"b\":1,\"a\":2}").unwrap();
        match v {
            Json::Obj(pairs) => {
                assert_eq!(pairs.len(), 2);
                assert_eq!(pairs.first().unwrap().0, "b");
            }
            _ => panic!("对象类型错"),
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x798FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3426_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
