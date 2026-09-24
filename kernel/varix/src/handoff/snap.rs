//! handoff.json 快照 schema（篇 2.2）——字段冻结 = 全链第一闸。
//!
//! # 字段定义（与 MD2 篇 2.2 逐字对表，只增不改不删）
//!
//! - `schema_version`：整数，当前 1，跨域协商锚点——读到不认识的更高
//!   版本按"能读多少读多少"降级处理并在恢复提示里注明。
//! - `created_at`：UTC 时间戳带时区标记（冻结格式 RFC3339 `…Z`）。
//! - `source_domain`：`varix` | `windows`，快照作者。
//! - `windows`：窗口清单数组，每项 `app_id` / `title`（采集关闭时空串）/
//!   `geometry`（x、y、宽、高、显示输出标识的整数组合）/ `workspace` /
//!   `focused`。
//! - `drafts`：草稿数组，每项 `path`（共享可读路径）/ `sha256` / `app_id`。
//! - `clipboard`：`kind`（`text`|`files`）+ `content`（文本 ≤256KB 超出
//!   截断并置 `truncated`；files 时为路径数组）+ 可选
//!   `clipboard_skipped_reason`（密码管理器跳过时如实记录，Q13）。
//! - `restore_hint`：给对方域恢复提示框的文案。
//! - `integrity`：整个 JSON 文本（不含本字段）的 SHA-256——读方**先验
//!   哈希再解析**，分区损坏时宁可放弃恢复也不解析半截数据（Q6）。
//!
//! # 目录常量（篇 2.2：约定目录名在 schema 常量表里）
//!
//! 交接分区 `/vx-snap/`（VARIX 写）、`/var-snap/`（Windows 助手写）、
//! `/diag/`（双方只读对方）；草稿共享目录 `/vx-drafts/`、`/var-drafts/`
//! ——与快照目录同构的命名对称。
//!
//! # integrity 的正则化口径（两端必须一致，B-206 互通的地基）
//!
//! 写方：先序列化出**不含 integrity 成员**的完整文本 `body`（以 `}` 收
//! 尾），`final = body 去掉末尾 } + ","integrity":"<64位小写hex>"}`。
//! 读方：要求全文以 `,"integrity":"<64hex>"}` 收尾，剥掉该成员补回 `}`
//! 得到 body，`sha256(body)` 与内嵌 hex 逐一比对，全等才解析。
//!
//! # 内核纪律
//!
//! 零依赖（无 serde）：手写紧凑 JSON 写面与解析面；解析器有深度/长度/
//! 节点数上限（内核不做无界分配）；重复键一律拒绝（写方永不产生，读方
//! 见到即视为脏数据——integrity 的剥除规则依赖确定性序列化）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;

// ---------------------------------------------------------------------------
// schema 常量表（字段冻结的另一半：目录、上限、版本）
// ---------------------------------------------------------------------------

/// schema 版本（跨域协商锚点）。
pub const SCHEMA_VERSION: i64 = 1;

/// VARIX 写的快照目录（Windows 助手只读）。
pub const SNAP_DIR_VARIX: &str = "/vx-snap";
/// Windows 助手写的快照目录（VARIX 只读）。
pub const SNAP_DIR_WINDOWS: &str = "/var-snap";
/// 双方可写的诊断包暂存目录。
pub const SNAP_DIR_DIAG: &str = "/diag";
/// VARIX 侧草稿共享目录（DATA 分区，对方域只读）。
pub const DRAFT_DIR_VARIX: &str = "/vx-drafts";
/// Windows 侧草稿共享目录（DATA 分区，对方域只读）。
pub const DRAFT_DIR_WINDOWS: &str = "/var-drafts";

/// 剪贴板文本上限（256KB，超出截断并置 truncated——剪贴板接力不做大
/// 文件搬运，那是文件接力的活）。
pub const CLIP_TEXT_MAX: usize = 256 * 1024;
/// 已知清单字段（windows/drafts/files 路径）的条目上限——分区文件不该
/// 无界，内核更不做无界分配。
pub const SNAP_LIST_MAX: usize = 256;
/// 快照全文上限（解析面的内存护栏）。
pub const SNAP_TEXT_MAX: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// 数据类型（篇 2.2 字段表的 Rust 形态）
// ---------------------------------------------------------------------------

/// 快照作者域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceDomain {
    Varix,
    Windows,
}

impl SourceDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceDomain::Varix => "varix",
            SourceDomain::Windows => "windows",
        }
    }
    pub fn parse(s: &str) -> Option<SourceDomain> {
        match s {
            "varix" => Some(SourceDomain::Varix),
            "windows" => Some(SourceDomain::Windows),
            _ => None,
        }
    }
}

/// 窗口几何（x、y、宽、高、所在显示输出标识的整数组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geometry {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub output: i64,
}

/// 窗口清单条目。`title` 在用户关闭标题采集（或敏感应用声明
/// title_private，篇 2.3）时为空串。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WinEntry {
    /// 应用唯一标识（原生取 vxapp id；Wine 取"Wine/〈前缀名〉/〈程序名〉"）。
    pub app_id: String,
    pub title: String,
    pub geometry: Geometry,
    pub workspace: i64,
    pub focused: bool,
}

/// 草稿条目。`path` 必须落在对方域读得到的共享可读路径（常量表目录）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DraftEntry {
    pub path: String,
    /// 内容哈希（恢复时校验文件没换）。
    pub sha256: String,
    pub app_id: String,
}

/// 剪贴板种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipKind {
    Text,
    Files,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Files => "files",
        }
    }
}

/// 剪贴板内容。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ClipContent {
    Text(String),
    Files(Vec<String>),
}

/// 剪贴板对象。`skipped_reason` 只在采集被跳过时出现（Q13：密码管理器
/// 类默认跳过——跳过也要让用户知道不是系统吞了内容）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Clipboard {
    pub kind: ClipKind,
    pub content: ClipContent,
    pub truncated: bool,
    pub skipped_reason: Option<String>,
}

/// 快照本体（integrity 不在结构体里——它是封皮上的封条，不是内容）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub schema_version: i64,
    /// UTC 时间戳带时区标记（冻结格式 `YYYY-MM-DDTHH:MM:SSZ`）。
    pub created_at: String,
    pub source_domain: SourceDomain,
    pub windows: Vec<WinEntry>,
    pub drafts: Vec<DraftEntry>,
    pub clipboard: Clipboard,
    pub restore_hint: String,
}

/// 解析结果：快照 + 降级标记 + 注记（高版本降级时恢复提示里要注明）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParseOutcome {
    pub snap: Snapshot,
    pub degraded: bool,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// 错误面（B-207 的"优雅放弃"要有人话）
// ---------------------------------------------------------------------------

/// 快照读取失败的类别（人话 reason 必填）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SnapErr {
    pub code: SnapErrCode,
    pub reason: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapErrCode {
    /// 不是合法 UTF-8（bytes→str 之外的调用方前置检查也归这里）。
    NotUtf8,
    /// 没有 integrity 成员或格式不对——封条不在，整包拒收。
    NoIntegrity,
    /// 哈希不匹配——内容被动过或损坏，宁可放弃恢复也不解析半截（Q6）。
    IntegrityMismatch,
    /// JSON 语法错误。
    Syntax,
    /// 结构不符合 schema（类型错/缺必填字段/域枚举不识别）。
    Schema,
    /// 超出护栏（深度/长度/条目上限）。
    Limit,
}

impl SnapErr {
    fn new(code: SnapErrCode, reason: impl Into<String>) -> SnapErr {
        SnapErr { code, reason: reason.into() }
    }
}

// ---------------------------------------------------------------------------
// JSON 写面（确定性序列化：键序固定、紧凑无空白——integrity 剥除规则
// 依赖这份确定性）
// ---------------------------------------------------------------------------

/// JSON 字符串转义（控制字符合法化，非 ASCII 原样透传）。
pub fn json_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 写一个成员：`"key":value`（分隔由分层栈管——数组/对象嵌套不穿层）。
struct Writer {
    buf: String,
    /// 每一层（根值/对象/数组）是否还没写过成员或元素。
    first: Vec<bool>,
    /// 刚写完键名——下一个 sep 必须跳过（值紧跟键，成员间分隔已写）。
    after_key: bool,
}

impl Writer {
    fn new() -> Writer {
        Writer { buf: String::new(), first: alloc::vec![true], after_key: false }
    }
    /// 当前层写下分隔：首个成员/元素前无逗号，其余补逗号；键名后的值
    /// 不再补（键值对的分隔已由 key 写过）。
    fn sep(&mut self) {
        if self.after_key {
            self.after_key = false;
            return;
        }
        let d = self.first.len() - 1;
        if !self.first[d] {
            self.buf.push(',');
        }
        self.first[d] = false;
    }
    fn key(&mut self, k: &str) {
        self.sep();
        json_escape(k, &mut self.buf);
        self.buf.push(':');
        self.after_key = true;
    }
    fn raw(&mut self, v: &str) {
        self.sep();
        self.buf.push_str(v);
    }
    fn str_val(&mut self, v: &str) {
        self.sep();
        json_escape(v, &mut self.buf);
    }
    fn begin_obj(&mut self) {
        self.sep();
        self.buf.push('{');
        self.first.push(true);
    }
    fn end_obj(&mut self) {
        self.buf.push('}');
        self.first.pop();
    }
    fn begin_arr(&mut self) {
        self.sep();
        self.buf.push('[');
        self.first.push(true);
    }
    fn end_arr(&mut self) {
        self.buf.push(']');
        self.first.pop();
    }
}

fn write_geometry(g: &Geometry, w: &mut Writer) {
    w.begin_obj();
    w.key("x");
    w.raw(&g.x.to_string());
    w.key("y");
    w.raw(&g.y.to_string());
    w.key("w");
    w.raw(&g.w.to_string());
    w.key("h");
    w.raw(&g.h.to_string());
    w.key("output");
    w.raw(&g.output.to_string());
    w.end_obj();
}

/// 剪贴板文本的写面前处理：超出 256KB 在字符边界截断（写方职责——
/// schema 冻结的上限由写方结构性保证，读方对越界文本只认带 truncated
/// 标记的）。返回（实际写入文本, 是否发生截断）。
fn clip_text_for_write(t: &str) -> (&str, bool) {
    if t.len() > CLIP_TEXT_MAX {
        let mut cut = CLIP_TEXT_MAX;
        while cut > 0 && !t.is_char_boundary(cut) {
            cut -= 1;
        }
        (&t[..cut], true)
    } else {
        (t, false)
    }
}

fn write_clipboard(c: &Clipboard, w: &mut Writer) {
    w.begin_obj();
    w.key("kind");
    w.str_val(c.kind.as_str());
    w.key("content");
    let mut truncated = c.truncated;
    match &c.content {
        ClipContent::Text(t) => {
            let (text, cut) = clip_text_for_write(t);
            truncated = truncated || cut;
            w.str_val(text);
        }
        ClipContent::Files(paths) => {
            w.begin_arr();
            for p in paths.iter() {
                w.str_val(p);
            }
            w.end_arr();
        }
    }
    w.key("truncated");
    w.raw(if truncated { "true" } else { "false" });
    if let Some(reason) = &c.skipped_reason {
        w.key("clipboard_skipped_reason");
        w.str_val(reason);
    }
    w.end_obj();
}

/// 序列化出**不含 integrity 成员**的 body（以 `}` 收尾）。
pub fn serialize_body(snap: &Snapshot) -> String {
    let mut w = Writer::new();
    w.begin_obj();
    w.key("schema_version");
    w.raw(&snap.schema_version.to_string());
    w.key("created_at");
    w.str_val(&snap.created_at);
    w.key("source_domain");
    w.str_val(snap.source_domain.as_str());
    w.key("windows");
    w.begin_arr();
    for win in snap.windows.iter() {
        w.begin_obj();
        w.key("app_id");
        w.str_val(&win.app_id);
        w.key("title");
        w.str_val(&win.title);
        w.key("geometry");
        write_geometry(&win.geometry, &mut w);
        w.key("workspace");
        w.raw(&win.workspace.to_string());
        w.key("focused");
        w.raw(if win.focused { "true" } else { "false" });
        w.end_obj();
    }
    w.end_arr();
    w.key("drafts");
    w.begin_arr();
    for d in snap.drafts.iter() {
        w.begin_obj();
        w.key("path");
        w.str_val(&d.path);
        w.key("sha256");
        w.str_val(&d.sha256);
        w.key("app_id");
        w.str_val(&d.app_id);
        w.end_obj();
    }
    w.end_arr();
    w.key("clipboard");
    write_clipboard(&snap.clipboard, &mut w);
    w.key("restore_hint");
    w.str_val(&snap.restore_hint);
    w.end_obj();
    w.buf
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// 快照封皮：body + integrity 封条（最终落盘形态）。
pub fn serialize(snap: &Snapshot) -> String {
    let body = serialize_body(snap);
    let hash = crate::ksha256::sha256(body.as_bytes());
    let mut out = body;
    out.pop(); // 去掉末尾 }
    let _ = write!(out, ",\"integrity\":\"{}\"}}", hex_encode(&hash));
    out
}

// ---------------------------------------------------------------------------
// JSON 解析面（手写、有护栏、重复键拒绝）
// ---------------------------------------------------------------------------

/// 解析器护栏。
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;
const MAX_STRING: usize = 512 * 1024;

/// 通用 JSON 值（解析面只负责"读进来"，语义校验在 schema 映射层）。
#[derive(Clone, PartialEq, Debug)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<JsonValue>),
    Obj(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            JsonValue::Obj(m) => Some(m),
            _ => None,
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    depth: usize,
    nodes: usize,
}

fn syn(pos: usize, why: &str) -> SnapErr {
    SnapErr::new(SnapErrCode::Syntax, format!("JSON 语法错误（字节 {}）：{}", pos, why))
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.pos < self.b.len() && matches!(self.b[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn eat(&mut self, lit: &str) -> Result<(), SnapErr> {
        if self.b[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(syn(self.pos, "字面量不完整"))
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, SnapErr> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(SnapErr::new(SnapErrCode::Limit, "JSON 节点数超出护栏"));
        }
        self.ws();
        match self.peek() {
            Some(b'{') => self.parse_obj(),
            Some(b'[') => self.parse_arr(),
            Some(b'"') => Ok(JsonValue::Str(self.parse_string()?)),
            Some(b't') => self.eat("true").map(|_| JsonValue::Bool(true)),
            Some(b'f') => self.eat("false").map(|_| JsonValue::Bool(false)),
            Some(b'n') => self.eat("null").map(|_| JsonValue::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            _ => Err(syn(self.pos, "不是合法的值起点")),
        }
    }

    fn parse_obj(&mut self) -> Result<JsonValue, SnapErr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(SnapErr::new(SnapErrCode::Limit, "嵌套深度超出护栏"));
        }
        self.pos += 1; // {
        let mut members: Vec<(String, JsonValue)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(JsonValue::Obj(members));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(syn(self.pos, "对象键必须是字符串"));
            }
            let key = self.parse_string()?;
            if members.iter().any(|(k, _)| *k == key) {
                return Err(SnapErr::new(
                    SnapErrCode::Schema,
                    format!("重复键 \"{}\"——快照写方必须是确定性序列化", key),
                ));
            }
            self.ws();
            if self.peek() != Some(b':') {
                return Err(syn(self.pos, "键后必须是冒号"));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            members.push((key, val));
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Obj(members));
                }
                _ => return Err(syn(self.pos, "对象成员后必须是逗号或右花括号")),
            }
        }
    }

    fn parse_arr(&mut self) -> Result<JsonValue, SnapErr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(SnapErr::new(SnapErrCode::Limit, "嵌套深度超出护栏"));
        }
        self.pos += 1; // [
        let mut items: Vec<JsonValue> = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(JsonValue::Arr(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Arr(items));
                }
                _ => return Err(syn(self.pos, "数组元素后必须是逗号或右方括号")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, SnapErr> {
        self.pos += 1; // 开引号
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(syn(self.pos, "字符串未闭合"));
            };
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(esc) = self.peek() else {
                        return Err(syn(self.pos, "转义序列未闭合"));
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.parse_u16_hex()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                // 代理对：高代理后必须紧跟低代理。
                                if self.b[self.pos..].starts_with(b"\\u") {
                                    self.pos += 2;
                                } else {
                                    return Err(syn(self.pos, "高代理后缺少低代理"));
                                }
                                let lo = self.parse_u16_hex()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(syn(self.pos, "低代理范围非法"));
                                }
                                let cp = 0x1_0000 + ((hi as u32 - 0xD800) << 10)
                                    + (lo as u32 - 0xDC00);
                                char::from_u32(cp).ok_or_else(|| syn(self.pos, "码点非法"))?
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(syn(self.pos, "孤立低代理"));
                            } else {
                                char::from_u32(hi as u32).ok_or_else(|| syn(self.pos, "码点非法"))?
                            };
                            out.push(ch);
                        }
                        _ => return Err(syn(self.pos, "未知转义符")),
                    }
                    if out.len() > MAX_STRING {
                        return Err(SnapErr::new(SnapErrCode::Limit, "字符串长度超出护栏"));
                    }
                }
                c if c < 0x20 => return Err(syn(self.pos, "字符串里的裸控制字符")),
                c => {
                    // 多字节 UTF-8：首字节之后的所有连续字节原样搬运。
                    let start = self.pos - 1;
                    let mut end = self.pos;
                    while end < self.b.len() && (self.b[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    self.pos = end;
                    let chunk = core::str::from_utf8(&self.b[start..end])
                        .map_err(|_| syn(start, "非法 UTF-8 序列"))?;
                    out.push_str(chunk);
                    let _ = c;
                    if out.len() > MAX_STRING {
                        return Err(SnapErr::new(SnapErrCode::Limit, "字符串长度超出护栏"));
                    }
                }
            }
        }
    }

    fn parse_u16_hex(&mut self) -> Result<u16, SnapErr> {
        if self.pos + 4 > self.b.len() {
            return Err(syn(self.pos, "\\u 需要 4 位十六进制"));
        }
        let s = core::str::from_utf8(&self.b[self.pos..self.pos + 4])
            .map_err(|_| syn(self.pos, "\\u 编码非法"))?;
        let v = u16::from_str_radix(s, 16).map_err(|_| syn(self.pos, "\\u 编码非法"))?;
        self.pos += 4;
        Ok(v)
    }

    fn parse_number(&mut self) -> Result<JsonValue, SnapErr> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = core::str::from_utf8(&self.b[start..self.pos])
            .map_err(|_| syn(start, "数字解析失败"))?;
        if !float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok(JsonValue::Int(i));
            }
            // 整数溢出：按浮点收（能读多少读多少），schema 层拒绝即可。
        }
        text.parse::<f64>()
            .map(JsonValue::Float)
            .map_err(|_| syn(start, "数字格式非法"))
    }
}

/// 解析 JSON 文本（顶层必须是值；尾部不允许还有内容）。
pub fn json_parse(text: &str) -> Result<JsonValue, SnapErr> {
    if text.len() > SNAP_TEXT_MAX {
        return Err(SnapErr::new(SnapErrCode::Limit, "快照全文超出护栏"));
    }
    let mut p = Parser { b: text.as_bytes(), pos: 0, depth: 0, nodes: 0 };
    let v = p.parse_value()?;
    p.ws();
    if p.pos != p.b.len() {
        return Err(syn(p.pos, "JSON 值之后还有多余内容"));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// integrity 封条的验与剥（读方先验哈希再解析——Q6）
// ---------------------------------------------------------------------------

const INTEGRITY_MARK: &str = ",\"integrity\":\"";

/// 剥掉 integrity 封条，返回被哈希的 body。格式不对返回 None。
pub fn strip_integrity(text: &str) -> Option<(String, String)> {
    // 全文必须以 `"}` 收尾。
    if !text.ends_with("\"}") {
        return None;
    }
    // 取最后一次出现的封条标记（封条是写方最后写入的成员）。
    let idx = text.rfind(INTEGRITY_MARK)?;
    let hex_and_close = &text[idx + INTEGRITY_MARK.len()..];
    // 形如 "<64hex>"}"
    if hex_and_close.len() != 64 + 2 {
        return None;
    }
    let hex = &hex_and_close[..64];
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let body = format!("{}}}", &text[..idx]);
    Some((body, hex.to_ascii_lowercase()))
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// schema 映射层（JSON 树 → Snapshot；v1 严格 / 更高版本降级）
// ---------------------------------------------------------------------------

fn err_schema(why: impl Into<String>) -> SnapErr {
    SnapErr::new(SnapErrCode::Schema, why)
}

/// 读取一个成员；`required` 且缺失：严格模式拒收，降级模式出注记。
fn take<'v>(
    obj: &'v [(String, JsonValue)],
    key: &str,
    degraded: bool,
    notes: &mut Vec<String>,
) -> Result<Option<&'v JsonValue>, SnapErr> {
    match obj.iter().find(|(k, _)| k == key).map(|(_, v)| v) {
        Some(v) => Ok(Some(v)),
        None if degraded => {
            notes.push(format!("缺少字段 \"{}\"（更高版本 schema，按能读多少读多少降级）", key));
            Ok(None)
        }
        None => Err(err_schema(format!("缺少必填字段 \"{}\"", key))),
    }
}

fn need_str(v: Option<&JsonValue>, key: &str) -> Result<String, SnapErr> {
    v.and_then(JsonValue::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| err_schema(format!("字段 \"{}\" 必须是字符串", key)))
}

fn need_i64(v: Option<&JsonValue>, key: &str) -> Result<i64, SnapErr> {
    v.and_then(JsonValue::as_i64)
        .ok_or_else(|| err_schema(format!("字段 \"{}\" 必须是整数", key)))
}

fn need_bool(v: Option<&JsonValue>, key: &str) -> Result<bool, SnapErr> {
    v.and_then(JsonValue::as_bool)
        .ok_or_else(|| err_schema(format!("字段 \"{}\" 必须是布尔", key)))
}

fn parse_geometry(v: &JsonValue) -> Result<Geometry, SnapErr> {
    let m = v.as_obj().ok_or_else(|| err_schema("geometry 必须是对象"))?;
    let g = |k: &str| m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v);
    Ok(Geometry {
        x: need_i64(g("x"), "geometry.x")?,
        y: need_i64(g("y"), "geometry.y")?,
        w: need_i64(g("w"), "geometry.w")?,
        h: need_i64(g("h"), "geometry.h")?,
        output: need_i64(g("output"), "geometry.output")?,
    })
}

fn parse_clipboard(v: &JsonValue, degraded: bool, notes: &mut Vec<String>) -> Result<Clipboard, SnapErr> {
    let m = v.as_obj().ok_or_else(|| err_schema("clipboard 必须是对象"))?;
    let kind_v = take(m, "kind", degraded, notes)?;
    let kind_str = kind_v
        .and_then(JsonValue::as_str)
        .ok_or_else(|| err_schema("clipboard.kind 必须是字符串"))?;
    let kind = match kind_str {
        "text" => ClipKind::Text,
        "files" => ClipKind::Files,
        other => return Err(err_schema(format!("clipboard.kind \"{}\" 不在枚举内", other))),
    };
    let content_v = take(m, "content", degraded, notes)?;
    let content = match kind {
        ClipKind::Text => ClipContent::Text(match content_v {
            Some(v) => need_str(Some(v), "clipboard.content")?,
            None => String::new(),
        }),
        ClipKind::Files => {
            let arr = match content_v {
                Some(v) => v
                    .as_arr()
                    .ok_or_else(|| err_schema("clipboard.content 必须是路径数组"))?,
                None => &[],
            };
            if arr.len() > SNAP_LIST_MAX {
                return Err(SnapErr::new(SnapErrCode::Limit, "files 路径数超出护栏"));
            }
            let mut paths = Vec::new();
            for p in arr {
                paths.push(need_str(Some(p), "clipboard.content[]")?);
            }
            ClipContent::Files(paths)
        }
    };
    let truncated = match take(m, "truncated", degraded, notes)? {
        Some(v) => need_bool(Some(v), "clipboard.truncated")?,
        None => false,
    };
    // 写方职责的镜像校验：越界文本必须带 truncated 标记（写方负责截断，
    // 读方对"超限又不承认"的快照拒收）。
    if let ClipContent::Text(t) = &content {
        if t.len() > CLIP_TEXT_MAX && !truncated {
            return Err(SnapErr::new(
                SnapErrCode::Limit,
                "剪贴板文本超出 256KB 且未置 truncated 标记——写方违反 schema 上限",
            ));
        }
    }
    // 可选字段：只在采集被跳过时出现（Q13）——缺失是常态，不进降级注记。
    let skipped_reason = match m.iter().find(|(k, _)| k == "clipboard_skipped_reason").map(|(_, v)| v) {
        Some(v) => Some(need_str(Some(v), "clipboard_skipped_reason")?),
        None => None,
    };
    Ok(Clipboard { kind, content, truncated, skipped_reason })
}

/// 读方入口：**先验哈希再解析**，全过才出快照。任何失败都是优雅放弃
/// （返回带人话的 Err，原文由调用方保留供诊断——Q6）。
pub fn open_snapshot(text: &str) -> Result<ParseOutcome, SnapErr> {
    let (body, embedded) =
        strip_integrity(text).ok_or_else(|| {
            SnapErr::new(SnapErrCode::NoIntegrity, "快照没有 integrity 封条或封条格式不对")
        })?;
    let actual = hex_encode(&crate::ksha256::sha256(body.as_bytes()));
    if actual != embedded {
        return Err(SnapErr::new(
            SnapErrCode::IntegrityMismatch,
            format!("快照完整性校验失败（声明 {} 实算 {}）——宁可放弃恢复也不解析半截数据", embedded, actual),
        ));
    }
    let tree = json_parse(&body)?;
    let m = tree
        .as_obj()
        .ok_or_else(|| err_schema("快照顶层必须是对象"))?;
    let g = |k: &str| m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v);

    let version = need_i64(g("schema_version"), "schema_version")?;
    let (degraded, mut notes) = if version == SCHEMA_VERSION {
        (false, Vec::new())
    } else if version > SCHEMA_VERSION {
        (
            true,
            alloc::vec![format!(
                "快照 schema 版本 {} 高于本侧支持版本 {}——按能读多少读多少降级",
                version, SCHEMA_VERSION
            )],
        )
    } else {
        return Err(err_schema(format!("快照 schema 版本 {} 低于最低支持版本", version)));
    };

    let created_at = {
        let s = need_str(take(m, "created_at", degraded, &mut notes)?, "created_at")?;
        // 带时区标记：冻结格式以 Z 收尾（UTC）。
        if !s.ends_with('Z') {
            return Err(err_schema("created_at 必须是带时区标记的 UTC 时间戳（…Z）"));
        }
        s
    };
    let domain_v = take(m, "source_domain", degraded, &mut notes)?;
    let domain_str = domain_v
        .and_then(JsonValue::as_str)
        .ok_or_else(|| err_schema("source_domain 必须是字符串"))?;
    let source_domain = SourceDomain::parse(domain_str)
        .ok_or_else(|| err_schema(format!("source_domain \"{}\" 不在枚举内", domain_str)))?;

    let windows_v = take(m, "windows", degraded, &mut notes)?;
    let mut windows = Vec::new();
    if let Some(wv) = windows_v {
        let arr = wv.as_arr().ok_or_else(|| err_schema("windows 必须是数组"))?;
        if arr.len() > SNAP_LIST_MAX {
            return Err(SnapErr::new(SnapErrCode::Limit, "windows 条目数超出护栏"));
        }
        for w in arr {
            let wm = w.as_obj().ok_or_else(|| err_schema("windows[] 必须是对象"))?;
            let geo_v = take(wm, "geometry", degraded, &mut notes)?;
            let geometry = match geo_v {
                Some(gv) => parse_geometry(gv)?,
                None => Geometry { x: 0, y: 0, w: 0, h: 0, output: 0 },
            };
            windows.push(WinEntry {
                app_id: need_str(take(wm, "app_id", degraded, &mut notes)?, "windows[].app_id")?,
                title: match take(wm, "title", degraded, &mut notes)? {
                    Some(v) => need_str(Some(v), "windows[].title")?,
                    None => String::new(),
                },
                geometry,
                workspace: match take(wm, "workspace", degraded, &mut notes)? {
                    Some(v) => need_i64(Some(v), "windows[].workspace")?,
                    None => 0,
                },
                focused: match take(wm, "focused", degraded, &mut notes)? {
                    Some(v) => need_bool(Some(v), "windows[].focused")?,
                    None => false,
                },
            });
        }
    }

    let drafts_v = take(m, "drafts", degraded, &mut notes)?;
    let mut drafts = Vec::new();
    if let Some(dv) = drafts_v {
        let arr = dv.as_arr().ok_or_else(|| err_schema("drafts 必须是数组"))?;
        if arr.len() > SNAP_LIST_MAX {
            return Err(SnapErr::new(SnapErrCode::Limit, "drafts 条目数超出护栏"));
        }
        for d in arr {
            let dm = d.as_obj().ok_or_else(|| err_schema("drafts[] 必须是对象"))?;
            let sha = need_str(take(dm, "sha256", degraded, &mut notes)?, "drafts[].sha256")?;
            if !is_sha256_hex(&sha) {
                return Err(err_schema("drafts[].sha256 必须是 64 位十六进制"));
            }
            drafts.push(DraftEntry {
                path: need_str(take(dm, "path", degraded, &mut notes)?, "drafts[].path")?,
                sha256: sha,
                app_id: need_str(take(dm, "app_id", degraded, &mut notes)?, "drafts[].app_id")?,
            });
        }
    }

    let clip_v = take(m, "clipboard", degraded, &mut notes)?;
    let clipboard = match clip_v {
        Some(v) => parse_clipboard(v, degraded, &mut notes)?,
        None => Clipboard {
            kind: ClipKind::Text,
            content: ClipContent::Text(String::new()),
            truncated: false,
            skipped_reason: None,
        },
    };

    let restore_hint = match take(m, "restore_hint", degraded, &mut notes)? {
        Some(v) => need_str(Some(v), "restore_hint")?,
        None => String::new(),
    };

    Ok(ParseOutcome {
        snap: Snapshot {
            schema_version: version,
            created_at,
            source_domain,
            windows,
            drafts,
            clipboard,
            restore_hint,
        },
        degraded,
        notes,
    })
}

// ---------------------------------------------------------------------------
// 测试（B-202 字段对表 / B-206 互通 / B-207 损坏容错 / 版本协商）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 规范样本（黄金夹具的内存形态——fixture 与它逐字节对锁）。
    fn canonical() -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            created_at: "2026-09-24T08:30:00Z".to_string(),
            source_domain: SourceDomain::Varix,
            windows: alloc::vec![
                WinEntry {
                    app_id: "vx.editor".to_string(),
                    title: "README.md - Varix Editor".to_string(),
                    geometry: Geometry { x: 64, y: 48, w: 1152, h: 704, output: 0 },
                    workspace: 1,
                    focused: true,
                },
                WinEntry {
                    app_id: "Wine/default/TotalCommander".to_string(),
                    title: String::new(),
                    geometry: Geometry { x: 128, y: 96, w: 960, h: 600, output: 1 },
                    workspace: 2,
                    focused: false,
                },
            ],
            drafts: alloc::vec![DraftEntry {
                path: "/vx-drafts/notes-2026-09-24.md".to_string(),
                sha256: "1111111111111111111111111111111111111111111111111111111111111111"
                    .to_string(),
                app_id: "vx.editor".to_string(),
            }],
            clipboard: Clipboard {
                kind: ClipKind::Text,
                content: ClipContent::Text("交接协议测试剪贴板内容".to_string()),
                truncated: false,
                skipped_reason: None,
            },
            restore_hint: "回到 VARIX 时恢复 2 个窗口与 1 份草稿".to_string(),
        }
    }

    // B-202：结构体对表——写方产出的键集合与篇 2.2 字段表零差异。
    #[test]
    fn b202_writer_field_table_zero_diff() {
        let text = serialize(&canonical());
        let tree = json_parse(strip_integrity(&text).unwrap().0.as_str()).unwrap();
        let m = tree.as_obj().unwrap();
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        // 顶层键序 = schema 冻结的写方键序（integrity 是封皮，不在 body 里）。
        assert_eq!(
            keys,
            alloc::vec!["schema_version", "created_at", "source_domain", "windows", "drafts", "clipboard", "restore_hint"]
        );
        // windows[] 条目键集合逐字对表。
        let w0 = m.iter().find(|(k, _)| k == "windows").unwrap().1.as_arr().unwrap()[0]
            .as_obj()
            .unwrap();
        let wkeys: Vec<&str> = w0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(wkeys, alloc::vec!["app_id", "title", "geometry", "workspace", "focused"]);
        // geometry 整数组合五键。
        let g0 = w0.iter().find(|(k, _)| k == "geometry").unwrap().1.as_obj().unwrap();
        let gkeys: Vec<&str> = g0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(gkeys, alloc::vec!["x", "y", "w", "h", "output"]);
        // drafts[] 条目键集合。
        let d0 = m.iter().find(|(k, _)| k == "drafts").unwrap().1.as_arr().unwrap()[0]
            .as_obj()
            .unwrap();
        let dkeys: Vec<&str> = d0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(dkeys, alloc::vec!["path", "sha256", "app_id"]);
        // clipboard 键集合（无跳过原因时三键）。
        let c0 = m.iter().find(|(k, _)| k == "clipboard").unwrap().1.as_obj().unwrap();
        let ckeys: Vec<&str> = c0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(ckeys, alloc::vec!["kind", "content", "truncated"]);
    }

    #[test]
    fn b202_schema_constants_frozen() {
        // 篇 2.2：约定目录名在 schema 常量表里——值冻结，改值即破坏互通。
        assert_eq!(SNAP_DIR_VARIX, "/vx-snap");
        assert_eq!(SNAP_DIR_WINDOWS, "/var-snap");
        assert_eq!(SNAP_DIR_DIAG, "/diag");
        assert_eq!(DRAFT_DIR_VARIX, "/vx-drafts");
        assert_eq!(DRAFT_DIR_WINDOWS, "/var-drafts");
        assert_eq!(SCHEMA_VERSION, 1);
        assert_eq!(CLIP_TEXT_MAX, 256 * 1024, "剪贴板上限 256KB");
    }

    #[test]
    fn b202_integrity_excludes_itself_and_locks_bytes() {
        // 封条只盖 body：剥掉封条重算必须一致；改一个字节立刻现行。
        let text = serialize(&canonical());
        let (body, hex) = strip_integrity(&text).unwrap();
        let actual = hex_encode(&crate::ksha256::sha256(body.as_bytes()));
        assert_eq!(actual, hex);
        // 篡改 body 一个字符 → 校验失败。
        let mut tampered = text.clone();
        let pos = tampered.find("README").unwrap();
        tampered.replace_range(pos..pos + 1, "X");
        assert_eq!(
            open_snapshot(&tampered).unwrap_err().code,
            SnapErrCode::IntegrityMismatch
        );
        // 篡改 integrity 声明值本身 → 校验失败（封条自证不通过）。
        let mut lied = serialize(&canonical());
        let hpos = lied.rfind(INTEGRITY_MARK).unwrap() + INTEGRITY_MARK.len();
        let mut fake: String = lied[hpos..hpos + 64].to_string();
        fake.replace_range(0..1, if fake.starts_with('0') { "1" } else { "0" });
        lied.replace_range(hpos..hpos + 64, &fake);
        assert_eq!(open_snapshot(&lied).unwrap_err().code, SnapErrCode::IntegrityMismatch);
    }

    #[test]
    fn b202_roundtrip_and_byte_stability() {
        // 序列化 → 解析 → 再序列化：内容逐字段相等，字节逐位稳定。
        let snap = canonical();
        let text = serialize(&snap);
        let out = open_snapshot(&text).unwrap();
        assert!(!out.degraded);
        assert_eq!(out.snap, snap, "roundtrip 内容逐字段相等");
        assert_eq!(serialize(&out.snap), text, "再序列化字节逐位稳定");
    }

    #[test]
    fn b202_writer_escapes_and_truncates() {
        // 转义：引号/反斜杠/控制字符安全进 JSON，roundtrip 保真。
        let mut s = canonical();
        s.restore_hint = "quote:\" back\\slash\nnew\ttab".to_string();
        let out = open_snapshot(&serialize(&s)).unwrap();
        assert_eq!(out.snap.restore_hint, s.restore_hint);
        // 256KB 截断：超出截断并置 truncated（在字符边界上截）。
        let long = "字".repeat(CLIP_TEXT_MAX); // 每个 3 字节 → 远超上限
        s.clipboard.content = ClipContent::Text(long);
        let text = serialize(&s);
        let out = open_snapshot(&text).unwrap();
        match &out.snap.clipboard.content {
            ClipContent::Text(t) => {
                assert!(t.len() <= CLIP_TEXT_MAX, "截断后必须 ≤256KB");
                assert!(out.snap.clipboard.truncated, "截断必须置标记");
                assert!(t.chars().last().is_some(), "截断不得劈开 UTF-8 字符");
            }
            _ => panic!("kind 必须保持 text"),
        }
    }

    /// B-202 的字节级对锁：黄金夹具与写方产出逐字节一致（夹具同时是
    /// Python 参考实现的解析输入——两端同一个文件）。
    #[test]
    fn b202_golden_fixture_byte_locked() {
        let golden = include_str!("fixtures/handoff-varix.json");
        assert_eq!(serialize(&canonical()), golden, "写方产出必须与黄金夹具逐字节一致");
        let out = open_snapshot(golden).unwrap();
        assert_eq!(out.snap, canonical());
    }

    // B-206：双向互通——Windows 侧写面样本（篇 22.2 的产出形态）VARIX
    // 必须能读；VARIX 的黄金夹具参考实现必须能读（Python 侧跑同一断言）。
    #[test]
    fn b206_reads_windows_authored_snapshot() {
        let text = include_str!("fixtures/handoff-windows.json");
        let out = open_snapshot(text).unwrap();
        assert_eq!(out.snap.source_domain, SourceDomain::Windows);
        assert_eq!(out.snap.windows.len(), 1);
        assert_eq!(out.snap.windows[0].app_id, "com.variable.explorer");
        // files 类剪贴板 + Q13 跳过原因如实可读。
        match &out.snap.clipboard.content {
            ClipContent::Files(paths) => assert_eq!(paths.len(), 2),
            _ => panic!("Windows 夹具是 files 类"),
        }
        assert_eq!(
            out.snap.clipboard.skipped_reason.as_deref(),
            Some("password-manager-clipboard-skipped")
        );
        assert_eq!(out.snap.drafts[0].path, "/var-drafts/report-q4.xlsx");
    }

    #[test]
    fn b206_python_reference_reads_varix_golden() {
        // VARIX 黄金夹具必须能被参考实现解析——本测试守护夹具的
        // 机器可读性前提（格式冻结），真正的跨实现互读由
        // `portable/engine/handoff/vx_handoff_proto.py selftest` 实证。
        let golden = include_str!("fixtures/handoff-varix.json");
        let (body, _) = strip_integrity(golden).unwrap();
        assert!(json_parse(&body).is_ok(), "夹具必须是合法 JSON");
        // 助手侧写面（篇 22.2 临时文件+原子换名+全文件哈希）的最终形态
        // 与 VARIX 读面的契约一致性：同一套剥除规则。
        assert!(strip_integrity(include_str!("fixtures/handoff-windows.json")).is_some());
    }

    // B-207：注入损坏 ≥10 次，全部优雅放弃（人话、不 panic、原文保留）。
    #[test]
    fn b207_ten_corruptions_all_graceful() {
        let good = serialize(&canonical());
        let mut cases: Vec<(String, &str)> = Vec::new();
        // 1. body 中段翻字节。
        let mut c = good.clone();
        let p = c.find("README").unwrap();
        c.replace_range(p..p + 1, "X");
        cases.push((c, "body 翻字节"));
        // 2. 截尾 10 字节。
        cases.push((good[..good.len() - 10].to_string(), "截尾"));
        // 3. 整个 integrity 成员摘除。
        let idx = good.rfind(INTEGRITY_MARK).unwrap();
        cases.push((format!("{}]}}", &good[..idx]), "摘除封条"));
        // 4. 封条声明值作废（合法格式、错误数值）。
        let mut c = good.clone();
        let hp = c.rfind(INTEGRITY_MARK).unwrap() + INTEGRITY_MARK.len();
        c.replace_range(hp..hp + 64, &"0".repeat(64));
        cases.push((c, "封条声明值作废"));
        // 5. 字符串里塞裸控制字符。
        let mut c = good.clone();
        let p = c.find("README").unwrap();
        c.insert(p, '\u{1}');
        cases.push((c, "裸控制字符"));
        // 6. 类型错：schema_version 给成字符串。
        cases.push((good.replace("\"schema_version\":1", "\"schema_version\":\"1\""), "类型错"));
        // 7. 结构错：windows 给成对象。
        cases.push((good.replace("\"windows\":[{", "\"windows\":{"), "结构错"));
        // 8. 枚举错：kind 越界。
        cases.push((good.replace("\"kind\":\"text\"", "\"kind\":\"weird\""), "枚举错"));
        // 9. 整数字段给浮点。
        cases.push((good.replace("\"workspace\":1", "\"workspace\":1.5"), "整数给浮点"));
        // 10. 深度炸弹。
        let bomb = format!("{}1{}", "[".repeat(MAX_DEPTH + 8), "]".repeat(MAX_DEPTH + 8));
        cases.push((bomb, "深度炸弹"));
        // 11. 重复键。
        cases.push((
            good.replace("\"restore_hint\":", "\"created_at\":\"2026-01-01T00:00:00Z\",\"restore_hint\":"),
            "重复键",
        ));
        // 12. 非十六进制封条。
        let mut c = good.clone();
        let hp = c.rfind(INTEGRITY_MARK).unwrap() + INTEGRITY_MARK.len();
        c.replace_range(hp..hp + 64, &"z".repeat(64));
        cases.push((c, "封条非十六进制"));

        assert!(cases.len() >= 10, "至少十组损坏注入");
        for (i, (text, label)) in cases.iter().enumerate() {
            let r = open_snapshot(text);
            assert!(r.is_err(), "损坏注入 #{} ({}) 必须被拒", i + 1, label);
            let e = r.unwrap_err();
            assert!(!e.reason.is_empty(), "拒绝必须带人话 #{} ({})", i + 1, label);
            // 原文保留供诊断：调用方手里的 text 原样（这里断言长度未被动过）。
            assert!(!text.is_empty());
        }
    }

    // 版本协商（Q18 / 篇 22.3）：旧助手读新快照、新助手读旧快照都按
    // "能读多少读多少"降级。
    #[test]
    fn higher_version_reads_degraded_with_notes() {
        // v2：多一个未知字段 + 缺 restore_hint——能读多少读多少 + 注记。
        let mut snap = canonical();
        snap.schema_version = 2;
        snap.restore_hint = String::new();
        let mut body = serialize_body(&snap);
        // 真删掉 restore_hint 成员（模拟高版本 schema 的字段缺席）。
        let needle = ",\"restore_hint\":\"\"";
        let pos = body.find(needle).expect("restore_hint 为空串时必有该片段");
        body.replace_range(pos..pos + needle.len(), "");
        // 注入一个 v2 新字段（写方测试自产，模拟未来版本）。
        let pos = body.rfind('}').unwrap();
        body.insert_str(pos, ",\"theme\":\"dark\"");
        let hash = hex_encode(&crate::ksha256::sha256(body.as_bytes()));
        let text = format!(
            "{},\"integrity\":\"{}\"}}",
            &body[..body.len() - 1],
            hash
        );
        let out = open_snapshot(&text).unwrap();
        assert!(out.degraded, "更高版本必须降级读");
        assert!(out.notes.iter().any(|n| n.contains("降级")), "注记要说明降级");
        assert!(out.notes.iter().any(|n| n.contains("restore_hint")), "缺字段要注记");
        assert_eq!(out.snap.windows.len(), 2, "能读的部分要读出来");
    }

    #[test]
    fn lower_version_rejected() {
        let mut snap = canonical();
        snap.schema_version = 0;
        let text = serialize(&snap);
        assert_eq!(open_snapshot(&text).unwrap_err().code, SnapErrCode::Schema);
    }

    #[test]
    fn missing_required_field_rejected_at_v1() {
        // v1 严格模式：缺必填字段 = 拒收（不冒充降级）。
        let text = serialize(&canonical());
        let (body, _) = strip_integrity(&text).unwrap();
        let broken = body.replace(",\"created_at\":\"2026-09-24T08:30:00Z\"", "");
        let hash = hex_encode(&crate::ksha256::sha256(broken.as_bytes()));
        let text2 = format!("{},\"integrity\":\"{}\"}}", &broken[..broken.len() - 1], hash);
        assert_eq!(open_snapshot(&text2).unwrap_err().code, SnapErrCode::Schema);
    }

    #[test]
    fn parser_guardrails() {
        // 顶层非对象/尾随内容/坏转义各有归宿。
        assert!(json_parse("[] extra").is_err());
        assert!(json_parse("{\"a\":\\u00}").is_err());
        assert!(json_parse("{\"a\":1,\"a\":2}").is_err(), "重复键拒绝");
        // \uXXXX 与代理对。
        let v = json_parse("\"\\u4e2d\\u6587\"").unwrap();
        assert_eq!(v.as_str(), Some("中文"));
        let v = json_parse("\"\\ud83d\\ude00\"").unwrap();
        assert_eq!(v.as_str(), Some("😀"));
        assert!(json_parse("\"\\ud83d\"").is_err(), "孤立高代理拒绝");
    }
}
