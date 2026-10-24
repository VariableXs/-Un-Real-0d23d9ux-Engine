//! UNX-F0216 · 报告导出格式（机器可读 JSON）
//!
//! 锚点判据（施工书原文）：UNX-F0216-J1 JSON 导出可被外部工具
//! 解析，schema 校验器通过率 100%。
//!
//! 设计：报告 → JSON 文本发射器（手工序列化：字符串转义 + 数值
//! 直书 + 布尔/空值分型）→ schema 校验器（必填键 / 类型分型 /
//! 未知键三查）→ 红线：①发射产物必须可被仓内解析器无损还原
//! （往返一致）；②缺必填键 / 类型错 / 未知键必拒；③转义正确
//! （引号/反斜杠/控制字符）。
//!
//! 错误码段：0x2C80（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0216_VERSION: &str = "F0216-jsonexp-v1";

/// 诊断码段基址（UNX-F0216 独占）。
pub const SEG_BASE: u16 = 0x2C80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 缺必填键。
    MissingKey = SEG_BASE + 1,
    /// 类型不符。
    TypeMismatch = SEG_BASE + 2,
    /// 未知键。
    UnknownKey = SEG_BASE + 3,
    /// 转义残缺（解析失败）。
    BadEscape = SEG_BASE + 4,
    /// 往返不一致。
    RoundTrip = SEG_BASE + 5,
    /// JSON 结构残缺（无闭合）。
    Unbalanced = SEG_BASE + 6,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// JSON 值分型（schema 口径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonVal {
    /// 字符串。
    Str(String),
    /// 无符号整数。
    Num(u64),
    /// 布尔。
    Bool(bool),
    /// 空值。
    Null,
}

impl JsonVal {
    /// 类型名（schema 用）。
    pub const fn type_name(&self) -> &'static str {
        match self {
            JsonVal::Str(_) => "string",
            JsonVal::Num(_) => "number",
            JsonVal::Bool(_) => "boolean",
            JsonVal::Null => "null",
        }
    }

    /// 发射（含字符串转义）。
    pub fn emit(&self) -> String {
        match self {
            JsonVal::Str(s) => quote_escape(s),
            JsonVal::Num(n) => {
                // 无 format 依赖的十进制直书。
                let mut buf = [0u8; 20];
                let mut i = buf.len();
                let mut v = *n;
                if v == 0 {
                    return String::from("0");
                }
                while v > 0 {
                    i -= 1;
                    buf[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                }
                String::from_utf8_lossy(&buf[i..]).into_owned()
            }
            JsonVal::Bool(b) => String::from(if *b { "true" } else { "false" }),
            JsonVal::Null => String::from("null"),
        }
    }
}

/// 字符串发射（引号包裹 + \" \\ \n \t \r 转义）。
fn quote_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 对象（键序参与发射）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JsonObj {
    /// 键值对。
    pub pairs: Vec<(String, JsonVal)>,
}

impl JsonObj {
    /// 发射为紧凑 JSON 文本。
    pub fn emit(&self) -> String {
        let mut out = String::from("{");
        for (i, (k, v)) in self.pairs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&quote_escape(k));
            out.push(':');
            out.push_str(&v.emit());
        }
        out.push('}');
        out
    }
}

/// schema 校验器：必填键 + 类型 + 未知键三查。
pub fn validate(obj: &JsonObj, schema: &[(&str, &str)], strict: bool) -> Result<(), u16> {
    for (k, ty) in schema.iter() {
        let v = obj
            .pairs
            .iter()
            .find(|(pk, _)| pk == k)
            .map(|(_, v)| v)
            .ok_or(E::MissingKey.code())?;
        if v.type_name() != *ty {
            return Err(E::TypeMismatch.code());
        }
    }
    if strict {
        for (k, _) in obj.pairs.iter() {
            if !schema.iter().any(|(sk, _)| sk == k) {
                return Err(E::UnknownKey.code());
            }
        }
    }
    Ok(())
}

/// 仓内解析器（往返验证用）：只解顶层对象、值取四型。
pub fn parse(text: &str) -> Result<JsonObj, u16> {
    let b = text.as_bytes();
    if b.first() != Some(&b'{') || b.last() != Some(&b'}') {
        return Err(E::Unbalanced.code());
    }
    // 简化往返解析：逐对扫描（键必须带引号，冒号分隔，逗号推进）。
    let inner = &text[1..text.len() - 1];
    let mut obj = JsonObj::default();
    let mut i = 0usize;
    let bytes = inner.as_bytes();
    while i < bytes.len() {
        // 键：带引号字符串。
        if bytes.get(i) != Some(&b'"') {
            return Err(E::Unbalanced.code());
        }
        let (key, ni) = scan_string(inner, i)?;
        i = ni;
        if bytes.get(i) != Some(&b':') {
            return Err(E::Unbalanced.code());
        }
        i += 1;
        // 值：按首字符分型。
        let v = match bytes.get(i) {
            Some(b'"') => {
                let (s, ni) = scan_string(inner, i)?;
                i = ni;
                JsonVal::Str(unescape(&s)?)
            }
            Some(b't') if inner[i..].starts_with("true") => {
                i += 4;
                JsonVal::Bool(true)
            }
            Some(b'f') if inner[i..].starts_with("false") => {
                i += 5;
                JsonVal::Bool(false)
            }
            Some(b'n') if inner[i..].starts_with("null") => {
                i += 4;
                JsonVal::Null
            }
            Some(c) if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let n = inner[start..i]
                    .parse::<u64>()
                    .map_err(|_| E::TypeMismatch.code())?;
                JsonVal::Num(n)
            }
            _ => return Err(E::Unbalanced.code()),
        };
        obj.pairs.push((key, v));
        if bytes.get(i) == Some(&b',') {
            i += 1;
        } else if i < bytes.len() {
            return Err(E::Unbalanced.code());
        }
    }
    Ok(obj)
}

/// 从 off 处扫带引号字符串（不处理嵌套引号外内容），返回 (内容, 下一偏移)。
fn scan_string(s: &str, off: usize) -> Result<(String, usize), u16> {
    let b = s.as_bytes();
    let mut i = off + 1;
    let mut out = String::new();
    while i < b.len() {
        match b[i] {
            b'"' => return Ok((out, i + 1)),
            b'\\' => {
                out.push('\\');
                i += 1;
                if i >= b.len() {
                    return Err(E::BadEscape.code());
                }
                out.push(b[i] as char);
                i += 1;
            }
            _ => {
                // 逐字符推进（按 UTF-8 长度，往返验证够用）。
                let ch = s[i..].chars().next().ok_or(E::BadEscape.code())?;
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Err(E::Unbalanced.code())
}

/// 反转义（与 quote_escape 对偶）。
fn unescape(s: &str) -> Result<String, u16> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            _ => return Err(E::BadEscape.code()),
        }
    }
    Ok(out)
}

/// J1 自证（判据原文：JSON 导出可被解析，schema 校验通过率 100%）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：五键报告（四型齐）。
    let obj = JsonObj {
        pairs: [
            (String::from("version"), JsonVal::Str(String::from("v1.0"))),
            (String::from("boot-ok"), JsonVal::Bool(true)),
            (String::from("cost-ms"), JsonVal::Num(412)),
            (String::from("panic-note"), JsonVal::Null),
            (String::from("detail"), JsonVal::Str(String::from("a\"b\\c\nd"))),
        ]
        .to_vec(),
    };
    let schema = [
        ("version", "string"),
        ("boot-ok", "boolean"),
        ("cost-ms", "number"),
        ("panic-note", "null"),
        ("detail", "string"),
    ];

    // 二、J1 主体：发射 → 解析 → schema 全绿 → 往返一致。
    let text = obj.emit();
    let back = parse(&text)?;
    validate(&back, &schema, true)?;
    if back != obj {
        return Err(E::RoundTrip.code());
    }
    if back.emit() != text {
        return Err(E::RoundTrip.code());
    }

    // 三、转义红线：内嵌引号/反斜杠/换行全数还原。
    let detail = back.pairs.iter().find(|(k, _)| k == "detail").map(|(_, v)| v);
    if detail != Some(&JsonVal::Str(String::from("a\"b\\c\nd"))) {
        return Err(E::BadEscape.code());
    }

    // 四、schema 三查红线：缺键 / 类型错 / 未知键。
    let mut nover = back.clone();
    nover.pairs.retain(|(k, _)| k != "version");
    if validate(&nover, &schema, false).is_ok() {
        return Err(E::MissingKey.code());
    }
    let mut wrongty = back.clone();
    if let Some(p) = wrongty.pairs.iter_mut().find(|(k, _)| k == "cost-ms") {
        p.1 = JsonVal::Str(String::from("412"));
    }
    if validate(&wrongty, &schema, false).is_ok() {
        return Err(E::TypeMismatch.code());
    }
    let mut extra = back.clone();
    extra.pairs.push((String::from("ghost"), JsonVal::Null));
    if validate(&extra, &schema, true).is_ok() {
        return Err(E::UnknownKey.code());
    }
    if validate(&extra, &schema, false).is_err() {
        // 宽松模式不查未知键：必须绿。
        return Err(E::UnknownKey.code());
    }

    // 五、结构红线：无闭合拒。
    if parse("{\"a\":1").is_ok() || parse("\"a\":1}").is_ok() {
        return Err(E::Unbalanced.code());
    }

    // 六、判别式段位对拍。
    if E::MissingKey.code() != SEG_BASE + 1 || E::Unbalanced.code() != SEG_BASE + 6 {
        return Err(E::MissingKey.code());
    }
    Ok(())
}
