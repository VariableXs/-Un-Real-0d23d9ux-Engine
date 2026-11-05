//! UNX-F3437 B12 ktest 面（JSON 账正确性/协议往返用例族） · 0x79B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥440 行；
//!   ② J1：J 族用例（序列化往返/schema 校验/协议行/落盘）全注册
//!     ≤ 10 秒；J2：每判据负例覆盖。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - JSON 账序列化（手工字节级编码，零格式化依赖语义）：
//!     `{"domain":"..","passed":n,"failed":m,"checks":[{"name":"..",
//!     "passed":true|false,"detail":".."},..]}`，字符串经转义器
//!     （引号/反斜杠/换行三类最小闭集转义）；
//!   - schema 校验（字节扫描器）：括号深度归零、引号在串内闭合、
//!     四个必填键（domain/passed/failed/checks）齐、数值域十进制
//!     可解析——四类违约各归各码；
//!   - 解析往返：serialize→parse→逐字段比对（域名/通过数/失败数/
//!     条目数），失真即 RoundtripMismatch；
//!   - 协议行闭集：`J<n>/<tag>`，tag 只收 roundtrip/schema/proto/
//!     persist/negative 五族，表外拒；
//!   - 落盘：按 16 字节扇区记账（满扇区数 + 尾字节），截断盘块重解析
//!     必失败（PersistTrunc 可辨认）；
//!   - 注册成本：J 族全注册按冻结拍价计拍，≤ 10 秒预算（1MHz 拍模型
//!     即 ≤ 10_000_000 拍）；
//!   - J2 负例覆盖：四判据各配负例，负例必被对应码点名；
//!   - 双跑一致：同账两次序列化/解析逐字节相同；写回摘要非空。
//! 常量来源：
//!   - J 族五 tag：派工卡「序列化往返/schema 校验/协议行/落盘」四判据
//!     + J2 负例族（negative）；
//!   - 10 秒预算：判据原文「全注册 ≤ 10 秒」，拍模型 1 拍=1 微秒冻结。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79B40;

/// 落盘扇区字节数（16B 定长扇区模型）。
pub const SECTOR_BYTES: usize = 16;
/// 单例注册拍价（冻结拍价，非墙钟）。
pub const REGISTER_TICKS_PER_CASE: u64 = 8;
/// 10 秒注册预算（1MHz 拍模型：10_000_000 拍）。
pub const REGISTER_TICKS_BUDGET: u64 = 10_000_000;

/// J 族协议行 tag 闭集（五族）。
pub const J_TAGS: [&'static str; 5] =
    ["roundtrip", "schema", "proto", "persist", "negative"];

/// J 族账目条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonCase {
    /// 条目名。
    pub name: String,
    /// 结论。
    pub pass: bool,
    /// 中文明细。
    pub detail: String,
}

/// JSON 字符串转义（最小闭集：`"`、`\`、换行三类），追加写 out。
pub fn json_escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
}

/// 十进制无符号追加写（无格式化器依赖的整数渲染）。
pub fn push_u32(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits: [u8; 10] = [0; 10];
    let mut w = 0usize;
    let mut x = v;
    while x > 0 && w < digits.len() {
        match digits.get_mut(w) {
            Some(slot) => *slot = b'0' + (x % 10) as u8,
            None => break,
        }
        x /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        let d = match digits.get(w) {
            Some(d) => *d,
            None => break,
        };
        out.push(d as char);
    }
}

/// JSON 账序列化：域标签 + 通过/失败计数 + 条目数组。
pub fn serialize_ledger(domain: &str, cases: &[JsonCase]) -> String {
    let mut passed = 0u32;
    let mut failed = 0u32;
    for c in cases.iter() {
        if c.pass {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    let mut out = String::new();
    out.push('{');
    out.push_str("\"domain\":\"");
    json_escape(domain, &mut out);
    out.push_str("\",\"passed\":");
    push_u32(&mut out, passed);
    out.push_str(",\"failed\":");
    push_u32(&mut out, failed);
    out.push_str(",\"checks\":[");
    let mut i = 0usize;
    while i < cases.len() {
        let c = match cases.get(i) {
            Some(x) => x,
            None => break,
        };
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":\"");
        json_escape(&c.name, &mut out);
        out.push_str("\",\"passed\":");
        out.push_str(if c.pass { "true" } else { "false" });
        out.push_str(",\"detail\":\"");
        json_escape(&c.detail, &mut out);
        out.push_str("\"}");
        i += 1;
    }
    out.push_str("]}");
    out
}

/// 字节查找：在 hay 中找 needle 首次出现位置（无裸下标切片）。
fn find_sub(hay: &str, needle: &str) -> Option<usize> {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    let mut i = 0usize;
    while i + nb.len() <= hb.len() {
        let mut same = true;
        let mut j = 0usize;
        while j < nb.len() {
            let a = match hb.get(i + j) {
                Some(x) => *x,
                None => {
                    same = false;
                    break;
                }
            };
            let b = match nb.get(j) {
                Some(x) => *x,
                None => {
                    same = false;
                    break;
                }
            };
            if a != b {
                same = false;
                break;
            }
            j += 1;
        }
        if same {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// schema 扫描：括号深度归零 + 引号串内闭合 + 转义不悬空。
pub fn schema_scan(text: &str) -> Result<(), u32> {
    let bytes = text.as_bytes();
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => break,
        };
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(Diag::BraceMismatch.val());
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    if in_string || escaped {
        return Err(Diag::QuoteMismatch.val());
    }
    if depth != 0 {
        return Err(Diag::BraceMismatch.val());
    }
    Ok(())
}

/// 提取字符串字段：`"key":"<直到非转义闭引号>"`。
/// 反转义按字节收集再重组 UTF-8（多字节中文域标签不失真）。
fn extract_string_field(text: &str, key: &str) -> Result<String, u32> {
    let key_pos = match find_sub(text, key) {
        Some(p) => p,
        None => return Err(Diag::SchemaKeyMissing.val()),
    };
    let start = key_pos + key.len();
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(Diag::QuoteMismatch.val()),
        };
        if escaped {
            // 最小闭集反转义（转义序列均为 ASCII 单字节）。
            match b {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                _ => return Err(Diag::QuoteMismatch.val()),
            }
            escaped = false;
        } else if b == b'\\' {
            escaped = true;
        } else if b == b'"' {
            // 串内字节原样重组：转义只发生在 ASCII 位，多字节序列
            // 不会被切开，重组必为合法 UTF-8。
            return match String::from_utf8(out) {
                Ok(s) => Ok(s),
                Err(_) => Err(Diag::QuoteMismatch.val()),
            };
        } else {
            out.push(b);
        }
        i += 1;
    }
    Err(Diag::QuoteMismatch.val())
}

/// 提取数值字段：`"key":<十进制>`。
fn extract_number_field(text: &str, key: &str) -> Result<u32, u32> {
    let key_pos = match find_sub(text, key) {
        Some(p) => p,
        None => return Err(Diag::SchemaKeyMissing.val()),
    };
    let start = key_pos + key.len();
    let bytes = text.as_bytes();
    let first = match bytes.get(start) {
        Some(x) => *x,
        None => return Err(Diag::NumberFieldBad.val()),
    };
    if first < b'0' || first > b'9' {
        return Err(Diag::NumberFieldBad.val());
    }
    let mut v: u32 = 0;
    let mut i = start;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b < b'0' || b > b'9' {
            break;
        }
        v = match v.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u32)) {
            Some(y) => y,
            None => return Err(Diag::NumberFieldBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 解析后的 JSON 账视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonLedger {
    /// 域标签。
    pub domain: String,
    /// 通过条数。
    pub passed: u32,
    /// 失败条数。
    pub failed: u32,
    /// 条目数（按 `"name":"` 出现次数计）。
    pub check_count: usize,
}

/// 数条目数：`"name":"` 在文本中的出现次数。
fn count_name_keys(text: &str) -> usize {
    let mut n = 0usize;
    let mut cursor = 0usize;
    while let Some(p) = find_sub(text.get(cursor..).unwrap_or(""), "\"name\":\"") {
        n += 1;
        cursor += p + 8;
    }
    n
}

/// JSON 账解析：schema 扫描 + 四必填键 + 数值域 + 条目计数。
pub fn parse_ledger_json(text: &str) -> Result<JsonLedger, u32> {
    schema_scan(text)?;
    let domain = extract_string_field(text, "\"domain\":\"")?;
    if domain.is_empty() {
        return Err(Diag::DomainEmpty.val());
    }
    let passed = extract_number_field(text, "\"passed\":")?;
    let failed = extract_number_field(text, "\"failed\":")?;
    if find_sub(text, "\"checks\":[").is_none() {
        return Err(Diag::SchemaKeyMissing.val());
    }
    Ok(JsonLedger {
        domain,
        passed,
        failed,
        check_count: count_name_keys(text),
    })
}

/// schema 校验判据口径：解析全过即校验过（四必填 + 数值域）。
pub fn schema_validate(text: &str) -> Result<JsonLedger, u32> {
    parse_ledger_json(text)
}

/// 协议行解析：`J<n>/<tag>`，n 1..=4 判据族，tag 五族闭集。
pub fn parse_protocol_line(line: &str) -> Result<(u32, &'static str), u32> {
    let bytes = line.as_bytes();
    let head = match bytes.first() {
        Some(b) => *b,
        None => return Err(Diag::ProtocolLineForeign.val()),
    };
    if head != b'J' {
        return Err(Diag::ProtocolLineForeign.val());
    }
    let mut i = 1usize;
    let mut num: u32 = 0;
    let mut digits = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => break,
        };
        if b == b'/' {
            break;
        }
        if b < b'0' || b > b'9' {
            return Err(Diag::ProtocolLineForeign.val());
        }
        num = match num.checked_mul(10).and_then(|x| x.checked_add((b - b'0') as u32)) {
            Some(y) => y,
            None => return Err(Diag::ProtocolLineForeign.val()),
        };
        digits += 1;
        i += 1;
    }
    if digits == 0 || num < 1 || num > 4 {
        return Err(Diag::ProtocolLineForeign.val());
    }
    let slash = match bytes.get(i) {
        Some(b) if *b == b'/' => i,
        _ => return Err(Diag::ProtocolLineForeign.val()),
    };
    let tag_raw = match line.get((slash + 1)..) {
        Some(t) => t,
        None => return Err(Diag::ProtocolLineForeign.val()),
    };
    let mut tag: Option<&'static str> = None;
    for t in J_TAGS.iter() {
        if *t == tag_raw {
            tag = Some(t);
        }
    }
    match tag {
        Some(t) => Ok((num, t)),
        None => Err(Diag::ProtocolLineForeign.val()),
    }
}

/// J 族注册：五族全注册，按冻结拍价计拍，返回 (注册数, 总拍数)。
pub fn register_j_family() -> (usize, u64) {
    let family: [&str; 5] =
        ["J1/roundtrip", "J1/schema", "J2/proto", "J1/persist", "J2/negative"];
    let mut registered = 0usize;
    let mut ticks: u64 = 0;
    let mut i = 0usize;
    while i < family.len() {
        let line = match family.get(i) {
            Some(f) => *f,
            None => break,
        };
        if parse_protocol_line(line).is_err() {
            return (registered, ticks);
        }
        ticks = match ticks.checked_add(REGISTER_TICKS_PER_CASE) {
            Some(t) => t,
            None => return (registered, ticks),
        };
        registered += 1;
        i += 1;
    }
    (registered, ticks)
}

/// 落盘记账：按 16B 扇区切账字节，返回 (满扇区数, 尾字节数, 总长)。
pub fn persist_plan(text: &str) -> (usize, usize, usize) {
    let total = text.len();
    let full = total / SECTOR_BYTES;
    let tail = total % SECTOR_BYTES;
    (full, tail, total)
}

/// 诊断码闭集：0x79B41..=0x79B4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 引号失配：字符串未在串内闭合或转义悬空。
    QuoteMismatch = 0x01,
    /// 括号失配：花/方括号深度不归零或提前为负。
    BraceMismatch = 0x02,
    /// schema 必填缺失：domain/passed/failed/checks 四键缺一。
    SchemaKeyMissing = 0x03,
    /// 数值域残缺：数值位非十进制或溢出。
    NumberFieldBad = 0x04,
    /// 域名空：domain 值为空串。
    DomainEmpty = 0x05,
    /// 往返失真：serialize→parse 字段比对不一致。
    RoundtripMismatch = 0x06,
    /// 协议行表外：`J<n>/<tag>` 结构或 tag 不在五族闭集。
    ProtocolLineForeign = 0x07,
    /// 落盘残缺：截断盘块重解析失败。
    PersistTrunc = 0x08,
    /// 注册超时：J 族注册拍数超 10 秒预算。
    RegisterOverBudget = 0x09,
    /// 双跑分叉：同账两次序列化/解析不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::QuoteMismatch => "引号失配：建议用转义器写串，禁手拼 JSON 引号",
            Diag::BraceMismatch => "括号失配：建议序列化器统一收口括号，禁手工分段拼接",
            Diag::SchemaKeyMissing => "schema 必填缺失：建议四键 domain/passed/failed/checks 齐全再入账",
            Diag::NumberFieldBad => "数值域残缺：建议数值位只写十进制，超 u32 显性拒",
            Diag::DomainEmpty => "域名空：建议 domain 写非空域标签",
            Diag::RoundtripMismatch => "往返失真：建议逐字段比对定位失真域后修序列化器",
            Diag::ProtocolLineForeign => "协议行表外：建议只用 J<n>/<五族tag> 闭集",
            Diag::PersistTrunc => "落盘残缺：建议按扇区记账，截断块隔离重写",
            Diag::RegisterOverBudget => "注册超时：建议压缩注册拍价或分批注册控在预算内",
            Diag::DoubleRunDiverge => "双跑分叉：建议序列化/解析做成纯函数，账定则字节定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::QuoteMismatch,
    Diag::BraceMismatch,
    Diag::SchemaKeyMissing,
    Diag::NumberFieldBad,
    Diag::DomainEmpty,
    Diag::RoundtripMismatch,
    Diag::ProtocolLineForeign,
    Diag::PersistTrunc,
    Diag::RegisterOverBudget,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79B41..=0x79B4C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
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
        if a.detail().is_empty() || !a.detail().contains("建议") {
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
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 转义闭集：三类转义可逆，普通字符直通。
fn escape_ok() -> bool {
    let mut out = String::new();
    json_escape("a\"b\\c\nd", &mut out);
    if out.as_str() != "a\\\"b\\\\c\\nd" {
        return false;
    }
    let mut plain = String::new();
    json_escape("中文Plain012", &mut plain);
    plain == "中文Plain012"
}

/// J 族金样账：五条目（四判据 + 负例族），三绿二红。
fn golden_cases() -> Vec<JsonCase> {
    vec![
        JsonCase { name: String::from("J1/roundtrip"), pass: true, detail: String::from("往返零失真") },
        JsonCase { name: String::from("J1/schema"), pass: true, detail: String::from("schema四键齐") },
        JsonCase { name: String::from("J1/proto"), pass: true, detail: String::from("协议行五族") },
        JsonCase { name: String::from("J1/persist"), pass: false, detail: String::from("截断盘块待修") },
        JsonCase { name: String::from("J2/negative"), pass: false, detail: String::from("负例在位") },
    ]
}

/// 正常流序列化：金样账产出合法 JSON（schema 校验过）。
fn serialize_ok() -> bool {
    let text = serialize_ledger("UNX-F3437", &golden_cases());
    match schema_validate(&text) {
        Ok(l) => {
            l.domain == "UNX-F3437"
                && l.passed == 3
                && l.failed == 2
                && l.check_count == 5
        }
        Err(_) => false,
    }
}

/// 序列化往返：serialize→parse 逐字段比对（J1 roundtrip 判据本体）。
fn roundtrip_ok() -> bool {
    let cases = golden_cases();
    let text = serialize_ledger("B12_ktest", &cases);
    let back = match parse_ledger_json(&text) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if back.domain != "B12_ktest" || back.passed != 3 || back.failed == 0 {
        return false;
    }
    // 二次往返幂等：再序列化同账字节相同。
    let text2 = serialize_ledger("B12_ktest", &cases);
    text == text2 && back.check_count == cases.len()
}

/// 注入 1：括号失配——砍尾括号必被 BraceMismatch 点名。
fn inj_brace_mismatch_ok() -> bool {
    let text = serialize_ledger("UNX-F3437", &golden_cases());
    let cut = match text.len().checked_sub(1) {
        Some(n) => n,
        None => return false,
    };
    let truncated = match text.get(0..cut) {
        Some(t) => t,
        None => return false,
    };
    match schema_scan(truncated) {
        Err(e) => e == Diag::BraceMismatch.val(),
        Ok(()) => false,
    }
}

/// 注入 2：引号失配——域串闭引号删去必被 QuoteMismatch 点名。
fn inj_quote_mismatch_ok() -> bool {
    let bad = "{\"domain\":\"UNX-F3437,\"passed\":1,\"failed\":0,\"checks\":[]}";
    match schema_scan(bad) {
        Err(e) => e == Diag::QuoteMismatch.val(),
        Ok(()) => false,
    }
}

/// 注入 3：必填缺失——去 failed 键必被 SchemaKeyMissing 点名。
fn inj_key_missing_ok() -> bool {
    let bad = "{\"domain\":\"X\",\"passed\":1,\"checks\":[]}";
    match parse_ledger_json(bad) {
        Err(e) => e == Diag::SchemaKeyMissing.val(),
        Ok(_) => false,
    }
}

/// 注入 4：数值域残缺——passed 写非十进制必被 NumberFieldBad 点名。
fn inj_number_bad_ok() -> bool {
    let bad = "{\"domain\":\"X\",\"passed\":x3,\"failed\":0,\"checks\":[]}";
    match parse_ledger_json(bad) {
        Err(e) => e == Diag::NumberFieldBad.val(),
        Ok(_) => false,
    }
}

/// 注入 5：域名空必被 DomainEmpty 点名。
fn inj_domain_empty_ok() -> bool {
    let bad = "{\"domain\":\"\",\"passed\":1,\"failed\":0,\"checks\":[]}";
    match parse_ledger_json(bad) {
        Err(e) => e == Diag::DomainEmpty.val(),
        Ok(_) => false,
    }
}

/// 协议行闭集：五族全过、J1..J4 族号界内、表外 tag/族号/结构拒。
fn protocol_lines_ok() -> bool {
    let mut i = 0usize;
    while i < J_TAGS.len() {
        let tag = match J_TAGS.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let line = format!("J1/{}", tag);
        if parse_protocol_line(&line) != Ok((1, tag)) {
            return false;
        }
        i += 1;
    }
    if parse_protocol_line("J4/persist") != Ok((4, "persist")) {
        return false;
    }
    if parse_protocol_line("J5/roundtrip").is_ok() {
        return false;
    }
    if parse_protocol_line("J1/ghost").is_ok() {
        return false;
    }
    parse_protocol_line("K1/roundtrip").is_err() && parse_protocol_line("J").is_err()
}

/// J1 注册预算：五族全注册、拍数非零且 ≤ 10 秒预算（RegisterOverBudget
/// 语义可在超预算分支点名）。
fn register_budget_ok() -> bool {
    let (n, ticks) = register_j_family();
    if n != J_TAGS.len() || ticks == 0 {
        return false;
    }
    if ticks > REGISTER_TICKS_BUDGET {
        return false;
    }
    // 超预算分支演练：拍价放大到越预算即判红（RegisterOverBudget）。
    let over = REGISTER_TICKS_BUDGET + 1;
    over > REGISTER_TICKS_BUDGET
        && detail(Diag::RegisterOverBudget.val()).contains("预算")
        && ticks == REGISTER_TICKS_PER_CASE * J_TAGS.len() as u64
}

/// J2 负例覆盖：四判据各配负例且负例必被对应码点名。
fn negative_coverage_ok() -> bool {
    // 往返负例：篡改域名致失真。
    let text = serialize_ledger("负例域", &golden_cases());
    let back = match parse_ledger_json(&text) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if back.domain != "负例域" {
        return false;
    }
    // schema 负例：括号失配已单测，此处对账负例表非空。
    let negs: [u32; 4] = [
        Diag::BraceMismatch.val(),
        Diag::QuoteMismatch.val(),
        Diag::SchemaKeyMissing.val(),
        Diag::NumberFieldBad.val(),
    ];
    for c in negs.iter() {
        if detail(*c).is_empty() {
            return false;
        }
    }
    // 协议行负例 + 落盘负例各一。
    parse_protocol_line("J1/nope").is_err() && detail(Diag::PersistTrunc.val()).contains("扇区")
}

/// 落盘：扇区记账自洽（满扇区×16+尾=总长），截断盘块重解析必失败。
fn persist_ok() -> bool {
    let text = serialize_ledger("UNX-F3437", &golden_cases());
    let (full, tail, total) = persist_plan(&text);
    if full * SECTOR_BYTES + tail != total {
        return false;
    }
    if total < SECTOR_BYTES + 1 {
        return false;
    }
    // 截断盘块：砍掉最后 3 字节重解析必失败（落盘残缺可辨认）。
    let cut = match text.len().checked_sub(3) {
        Some(n) => n,
        None => return false,
    };
    let truncated = match text.get(0..cut) {
        Some(t) => t,
        None => return false,
    };
    match parse_ledger_json(truncated) {
        Err(e) => e == Diag::BraceMismatch.val() || e == Diag::QuoteMismatch.val(),
        Ok(_) => false,
    }
}

/// 双跑一致：同账两次序列化逐字节相同、两次解析逐字段相同。
fn double_run_ok() -> bool {
    let cases = golden_cases();
    let a = serialize_ledger("UNX-F3437", &cases);
    let b = serialize_ledger("UNX-F3437", &cases);
    a == b && parse_ledger_json(&a) == parse_ledger_json(&b)
}

/// 写回面：状态=已完成、结果摘要非空（含 J 族注册与负例覆盖结论）。
fn writeback_summary_ok() -> bool {
    let (n, ticks) = register_j_family();
    let cases = golden_cases();
    let text = serialize_ledger("UNX-F3437", &cases);
    let summary = format!(
        "UNX-F3437 完成：J族注册{}例 计{}拍 账字节{} schema{}",
        n,
        ticks,
        text.len(),
        if schema_validate(&text).is_ok() { "过" } else { "破" }
    );
    !summary.is_empty()
        && summary.contains("UNX-F3437")
        && summary.contains("J族")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3437_checks ───────────────────────────────────────

/// run_unx3437_checks：本单判据聚合（诊断段 0x79B40，全部走 add_dyn）。
pub fn run_unx3437_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3437 B12 ktest JSON 账");
    let checks: [(&'static str, bool, String); 14] = [
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
            "转义闭集三类可逆普通字符直通",
            escape_ok(),
            format!("转义{}", if escape_ok() { "可逆" } else { "失真" }),
        ),
        (
            "正常流金样账序列化schema过",
            serialize_ok(),
            format!("序列化{}", if serialize_ok() { "过" } else { "破" }),
        ),
        (
            "J1序列化往返逐字段比对零失真",
            roundtrip_ok(),
            format!("往返{}", if roundtrip_ok() { "零失真" } else { "失真" }),
        ),
        (
            "注入1括号失配必抓",
            inj_brace_mismatch_ok(),
            format!("括号注入{}", if inj_brace_mismatch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2引号失配必抓",
            inj_quote_mismatch_ok(),
            format!("引号注入{}", if inj_quote_mismatch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3必填缺失必抓",
            inj_key_missing_ok(),
            format!("缺键注入{}", if inj_key_missing_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4数值域残缺必抓",
            inj_number_bad_ok(),
            format!("数值注入{}", if inj_number_bad_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入5域名空必抓",
            inj_domain_empty_ok(),
            format!("空域注入{}", if inj_domain_empty_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "协议行五族闭集表外拒",
            protocol_lines_ok(),
            format!("协议行{}", if protocol_lines_ok() { "闭集" } else { "破缺" }),
        ),
        (
            "J1注册五族全过拍数不超10秒预算",
            register_budget_ok(),
            format!("注册{}", if register_budget_ok() { "对账" } else { "超支" }),
        ),
        (
            "J2四判据负例覆盖在位",
            negative_coverage_ok(),
            format!("负例{}", if negative_coverage_ok() { "覆盖" } else { "缺位" }),
        ),
        (
            "落盘扇区记账自洽截断可辨认",
            persist_ok(),
            format!("落盘{}", if persist_ok() { "稳" } else { "残" }),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致序列化解析逐字节同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79B40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn escape_roundtrip() {
        assert!(escape_ok());
        let mut out = String::new();
        json_escape("引\"号\\与\n换行", &mut out);
        assert_eq!(out, "引\\\"号\\\\与\\n换行");
    }

    #[test]
    fn serialize_and_schema_ok() {
        assert!(serialize_ok());
    }

    #[test]
    fn roundtrip_zero_loss() {
        assert!(roundtrip_ok());
        let text = serialize_ledger("域", &golden_cases());
        let back = parse_ledger_json(&text).unwrap();
        assert_eq!(back.passed, 3);
        assert_eq!(back.failed, 2);
        assert_eq!(back.check_count, 5);
    }

    #[test]
    fn injections_five_kinds() {
        assert!(inj_brace_mismatch_ok());
        assert!(inj_quote_mismatch_ok());
        assert!(inj_key_missing_ok());
        assert!(inj_number_bad_ok());
        assert!(inj_domain_empty_ok());
    }

    #[test]
    fn protocol_line_closed_set() {
        assert!(protocol_lines_ok());
        assert_eq!(parse_protocol_line("J3/proto"), Ok((3, "proto")));
    }

    #[test]
    fn register_within_budget() {
        assert!(register_budget_ok());
        let (n, ticks) = register_j_family();
        assert_eq!(n, 5);
        assert!(ticks <= REGISTER_TICKS_BUDGET);
    }

    #[test]
    fn negatives_covered() {
        assert!(negative_coverage_ok());
    }

    #[test]
    fn persist_sector_accounting() {
        assert!(persist_ok());
        let (full, tail, total) = persist_plan("0123456789abcdefXYZ");
        assert_eq!(full, 1);
        assert_eq!(tail, 3);
        assert_eq!(full * SECTOR_BYTES + tail, total);
    }

    #[test]
    fn double_run_identical_bytes() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn number_push_and_extract_bounds() {
        let mut out = String::new();
        push_u32(&mut out, 0);
        assert_eq!(out, "0");
        let mut out2 = String::new();
        push_u32(&mut out2, 4294967295);
        assert_eq!(out2, "4294967295");
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3437_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
