//! F220 纯文本粘贴（Ctrl+Shift+V）· 判据实装（H 基础通用域 · AI-H1）。
//!
//! **判据锚**：主册 F220「纯文本粘贴（Ctrl+Shift+V）」。
//!
//! **验收标准（主册第一句）**：富文本→纯文本格式剥离完整性（HTML/RTF/
//! 图片三种来源用例）；自动纯文本目标清单审计；撤销联动验证；双快捷键
//! 并存不冲突（20 次混按）。
//!
//! **设计要点**：
//! - Ctrl+V 按来源格式栈取最优（F017 剪贴板格式栈语义）、Ctrl+Shift+V
//!   强制纯文本——去掉字体/颜色/链接样式只留文字；
//! - 剥离器三分支：HTML 去标签留文本（基础实体解码）、RTF 去控制字留
//!   文本、图片**诚实产出空文本**（图里没字就是不产字，绝不编）；
//! - 自动纯文本目标（记事本/搜索框/重命名框）双保险：不管哪个快捷键
//!   一律纯文本（目标本来就没有富文本格式）；
//! - 撤销联动：粘贴返回 [`PasteEdit`]（before/after），F202 撤销栈
//!   直接入栈；双快捷键 20 次混按各自独立解析、零冲突。
//!
//! **依赖锚点**：F017 格式栈、F202 撤销框架。
//! 时间纪律：不持时钟（本项无时序判据）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 混按验收次数——主册 F220：「双快捷键并存不冲突（20 次混按）」。
pub const MIX_PRESS_CASES: usize = 20;

// ---------------------------------------------------------------------------
// 剪贴板载荷（格式栈）
// ---------------------------------------------------------------------------

/// 剪贴板格式（F017 格式栈的 H1 侧最小承载）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipFormat {
    /// HTML 富文本。
    Html(String),
    /// RTF 富文本。
    Rtf(String),
    /// 图片（字节长度占位——判定面只关心「非文本」）。
    Image(usize),
    /// 纯文本。
    Plain(String),
}

/// 剪贴板载荷：格式栈（序 = 优先级，前为优）。
#[derive(Clone, Debug, Default)]
pub struct ClipPayload {
    pub formats: Vec<ClipFormat>,
}

impl ClipPayload {
    /// Ctrl+V 的取用：格式栈中最优的可显示格式。
    pub fn preferred(&self) -> Option<&ClipFormat> {
        self.formats.first()
    }

    /// Ctrl+Shift+V 的取用：强制纯文本（剥到只剩文字）。
    pub fn forced_plain(&self) -> String {
        match self.preferred() {
            Some(ClipFormat::Html(s)) => strip_html(s),
            Some(ClipFormat::Rtf(s)) => strip_rtf(s),
            Some(ClipFormat::Plain(s)) => s.clone(),
            Some(ClipFormat::Image(_)) | None => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// 剥离器（三分支，一处一事实）
// ---------------------------------------------------------------------------

/// HTML → 纯文本：去标签 + 基础实体解码（&amp; &lt; &gt; &quot; &#39;）。
/// `<style>/<script>` 整块剔除（样式脚本不是内容）。
pub fn strip_html(s: &str) -> String {
    const DROP_BLOCKS: [&str; 2] = ["style", "script"];
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    // 当前剔除块名（style/script）——String 承载避免借用越界。
    let mut dropping: Option<String> = None;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // 注释整块剔除。
            if s[i..].starts_with("<!--") {
                i += 4;
                while i < bytes.len() && !s[i..].starts_with("-->") {
                    i += 1;
                }
                i = (i + 3).min(bytes.len());
                continue;
            }
            let Some(close) = s[i..].find('>') else { break };
            let tag = &s[i + 1..i + close];
            let t = tag.trim().to_ascii_lowercase();
            if let Some(name) = t.strip_prefix('/') {
                if dropping.as_deref() == Some(name.trim()) {
                    dropping = None;
                }
            } else {
                let name = t.split_whitespace().next().unwrap_or("");
                if DROP_BLOCKS.contains(&name) && !t.ends_with('/') {
                    dropping = Some(name.to_string());
                }
            }
            i += close + 1;
            continue;
        }
        if dropping.is_some() {
            i += 1;
            continue;
        }
        // 实体解码。
        if bytes[i] == b'&' {
            let rest = &s[i..];
            let entity = ["&amp;", "&lt;", "&gt;", "&quot;", "&#39;"]
                .iter()
                .find(|e| rest.starts_with(**e));
            if let Some(e) = entity {
                out.push_str(match *e {
                    "&amp;" => "&",
                    "&lt;" => "<",
                    "&gt;" => ">",
                    "&quot;" => "\"",
                    _ => "'",
                });
                i += e.len();
                continue;
            }
        }
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_len(b0: u8) -> usize {
    if b0 < 0x80 { 1 } else if b0 >= 0xF0 { 4 } else if b0 >= 0xE0 { 3 } else { 2 }
}

/// RTF → 纯文本：控制字/控制符号剔除、组剥壳、`\'hh` 十六进制转字节、
/// `\par` 换行。目标字符集之外的转义如实丢弃（不编造）。
pub fn strip_rtf(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                if i + 1 >= bytes.len() {
                    break;
                }
                match bytes[i + 1] {
                    b'\\' | b'{' | b'}' => {
                        out.push(bytes[i + 1]);
                        i += 2;
                    }
                    b'\'' => {
                        // \'hh → 单字节（Latin-1 语义；多字节如实丢弃）。
                        if i + 3 < bytes.len() {
                            let hex = &s[i + 2..i + 4];
                            if let Ok(v) = u8::from_str_radix(hex, 16) {
                                out.push(v);
                            }
                            i += 4;
                        } else {
                            i += 2;
                        }
                    }
                    b'p' if s[i..].starts_with("\\par") => {
                        out.push(b'\n');
                        i += 4;
                        if bytes.get(i) == Some(&b' ') {
                            i += 1;
                        }
                    }
                    _ => {
                        // 控制字：\word[ ][可选数值][空格]。
                        let mut j = i + 1;
                        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b'-' {
                            j += 1;
                        }
                        while j < bytes.len() && bytes[j].is_ascii_digit() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b' ' {
                            j += 1;
                        }
                        i = j;
                    }
                }
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// 粘贴解析（双快捷键 + 自动纯文本目标）
// ---------------------------------------------------------------------------

/// 粘贴目标（自动纯文本清单的成员）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteTarget {
    /// 富文本面（可用 Ctrl+V 带格式）。
    RichSurface,
    /// 记事本（F097）——自动纯文本。
    Notepad,
    /// 搜索框——自动纯文本。
    SearchBox,
    /// 重命名框——自动纯文本。
    RenameBox,
}

/// 自动纯文本目标清单（主册：「往记事本、搜索框、重命名框粘贴时自动
/// 纯文本」——清单审计的唯一实现点）。
pub fn is_auto_plain_target(t: PasteTarget) -> bool {
    matches!(t, PasteTarget::Notepad | PasteTarget::SearchBox | PasteTarget::RenameBox)
}

/// 粘贴快捷键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteShortcut {
    /// Ctrl+V：按来源格式。
    WithFormat,
    /// Ctrl+Shift+V：强制纯文本。
    PlainOnly,
}

/// 粘贴产出（文本 + 撤销联动记录）。
pub struct PasteEdit {
    pub text: String,
    /// 撤销联动：粘贴前文本 → 粘贴后文本（F202 入栈用）。
    pub before: String,
    pub after: String,
}

/// 解析一次粘贴：目标自动纯文本优先于快捷键语义（双保险）。
pub fn resolve_paste(payload: &ClipPayload, target: PasteTarget, shortcut: PasteShortcut, before: &str) -> PasteEdit {
    let text = if is_auto_plain_target(target) || shortcut == PasteShortcut::PlainOnly {
        payload.forced_plain()
    } else {
        match payload.preferred() {
            Some(ClipFormat::Plain(s)) | Some(ClipFormat::Html(s)) | Some(ClipFormat::Rtf(s)) => s.clone(),
            Some(ClipFormat::Image(_)) | None => String::new(),
        }
    };
    let mut after = before.to_string();
    after.push_str(&text);
    PasteEdit { text, before: before.to_string(), after }
}

/// 双快捷键并存不冲突：20 次混按（两键交替）每次解析独立、结果互不
/// 污染（Ctrl+V 恒带格式语义、Ctrl+Shift+V 恒纯文本）。
pub fn dual_shortcut_mix_ok(payload: &ClipPayload) -> bool {
    for i in 0..MIX_PRESS_CASES {
        let shortcut = if i % 2 == 0 { PasteShortcut::WithFormat } else { PasteShortcut::PlainOnly };
        let edit = resolve_paste(payload, PasteTarget::RichSurface, shortcut, "");
        let expect_plain = shortcut == PasteShortcut::PlainOnly;
        let rich_has_markup = edit.text.contains('<') || edit.text.contains('\\');
        if expect_plain && rich_has_markup {
            return false;
        }
        if !expect_plain && edit.text != rich_source_text(payload) {
            return false;
        }
    }
    true
}

/// 富文本来源的原文（Ctrl+V 应取到的内容——格式栈最优原样）。
fn rich_source_text(p: &ClipPayload) -> String {
    match p.preferred() {
        Some(ClipFormat::Html(s)) | Some(ClipFormat::Rtf(s)) | Some(ClipFormat::Plain(s)) => s.clone(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F220 自检（判据面：三来源剥离 + 自动清单 + 撤销联动 + 20 次混按）。
pub fn run_pasteplain_checks() -> CheckSet {
    let mut set = CheckSet::new("F220-pasteplain");

    // 1. HTML 剥离：标签去净、实体解码、style/script 整块剔除。
    set.add(
        "html stripped to text",
        strip_html("<p style=\"x\">你好 <b>world</b></p>") == "你好 world"
            && strip_html("a&lt;b&gt;c&amp;d") == "a<b>c&d"
            && strip_html("<style>p{}</style>正文<script>evil()</script>") == "正文",
        "",
    );

    // 2. RTF 剥离：控制字剔除、\'hh 转字节、\par 换行、组剥壳。
    set.add(
        "rtf stripped to text",
        strip_rtf("{\\rtf1\\ansi Hi \\'41\\'42\\par World}") == "Hi AB\nWorld",
        "",
    );

    // 3. 图片来源诚实产空（图里没字就不产字）。
    let img = ClipPayload { formats: vec![ClipFormat::Image(2048)] };
    set.add("image source yields empty text", img.forced_plain().is_empty(), "");

    // 4. 自动纯文本目标清单审计（三个目标 + 富文本面不自动剥）。
    set.add(
        "auto plain target list audited",
        is_auto_plain_target(PasteTarget::Notepad)
            && is_auto_plain_target(PasteTarget::SearchBox)
            && is_auto_plain_target(PasteTarget::RenameBox)
            && !is_auto_plain_target(PasteTarget::RichSurface),
        "",
    );

    // 5. 自动纯文本双保险：记事本上 Ctrl+V 也只得纯文本。
    let rich = ClipPayload {
        formats: vec![ClipFormat::Html("<b>加粗</b>".into()), ClipFormat::Plain("加粗".into())],
    };
    let edit = resolve_paste(&rich, PasteTarget::Notepad, PasteShortcut::WithFormat, "");
    set.add("auto target forces plain even ctrl+v", edit.text == "加粗", "");

    // 6. Ctrl+V 带格式、Ctrl+Shift+V 强制纯文本（富文本面）。
    let e1 = resolve_paste(&rich, PasteTarget::RichSurface, PasteShortcut::WithFormat, "");
    let e2 = resolve_paste(&rich, PasteTarget::RichSurface, PasteShortcut::PlainOnly, "");
    set.add(
        "dual shortcuts distinct semantics",
        e1.text == "<b>加粗</b>" && e2.text == "加粗",
        "",
    );

    // 7. 撤销联动：PasteEdit.before/after 可直接入 F202 栈（往返一致）。
    set.add(
        "undo linkage before/after",
        e2.before.is_empty() && e2.after == "加粗",
        "",
    );

    // 8. 双快捷键 20 次混按零冲突。
    set.add("20 mixed presses no conflict", dual_shortcut_mix_ok(&rich), "");

    // 9. 格式栈取优：栈首优先（HTML 在 Plain 前）。
    set.add(
        "format stack prefers head",
        matches!(rich.preferred(), Some(ClipFormat::Html(_))),
        "",
    );

    // 10. 剥离完整性：来源三格式往返文本一致（HTML/RTF/Plain 同文）。
    let same = "混合文本 mixed";
    let p_html = ClipPayload { formats: vec![ClipFormat::Html(format!("<p>{}</p>", same))] };
    let p_rtf = ClipPayload { formats: vec![ClipFormat::Rtf(format!("{{\\rtf1{}}}", same))] };
    let p_plain = ClipPayload { formats: vec![ClipFormat::Plain(same.into())] };
    set.add(
        "three sources same plain text",
        p_html.forced_plain() == same && p_rtf.forced_plain() == same && p_plain.forced_plain() == same,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_edge_cases() {
        // 未闭合标签：尽力剥到尾部不崩。
        assert_eq!(strip_html("<b>粗体"), "粗体");
        // 嵌套标签。
        assert_eq!(strip_html("<div><span>inner</span></div>"), "inner");
        // 纯文本直通。
        assert_eq!(strip_html("no markup"), "no markup");
    }

    #[test]
    fn rtf_brace_escape() {
        assert_eq!(strip_rtf("a\\{b\\}c"), "a{b}c");
        assert_eq!(strip_rtf("\\\\backslash"), "\\backslash");
    }

    #[test]
    fn undo_roundtrip_semantics() {
        let p = ClipPayload { formats: vec![ClipFormat::Plain("X".into())] };
        let e = resolve_paste(&p, PasteTarget::SearchBox, PasteShortcut::WithFormat, "abc");
        // 撤销 = 回到 before；重做 = 回到 after。
        assert_eq!(e.before, "abc");
        assert_eq!(e.after, "abcX");
    }

    #[test]
    fn pasteplain_selfcheck_all_green() {
        let set = run_pasteplain_checks();
        assert!(set.all_passed(), "F220 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F220 验收定义的实装细化，非新立项）：持久化面 = 剥离
// 结果审计记录（来源类型+剥离位图+文本长度）的 VXH1 定长记录；壳接线面 =
// 格式探测纯函数（魔数嗅探 HTML/RTF/图片头）+ 双快捷键路由判定；判定面 =
// run_pasteplain_v2_checks。零堆定长缓冲。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 审计槽位容量（8 条剥离审计封顶）与来源类型编码（与 ClipFormat 三分支
/// 同源：HTML/RTF/图片，外加纯文本）。
pub const V2_AUDIT_CAP: usize = 8;
pub const SRC_HTML: u8 = 0;
pub const SRC_RTF: u8 = 1;
pub const SRC_IMAGE: u8 = 2;
pub const SRC_PLAIN: u8 = 3;
/// 记录定长：4 魔数 + 1 版本 + 33 载荷（计数 1 + 8 × (来源 1 + 剥离 1 + 长度 2)）+ 4 校验。
pub const V2_RECORD_BYTES: usize = 42;

/// v2 持久化错误：损坏输入显性拒绝（四类 + 字段域越界一类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength, BadField }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 剥离结果审计记录（持久化面）：判据「富文本→纯文本格式剥离完整性
/// （HTML/RTF/图片三种来源用例）」的存档载体——逐条记来源、剥离是否
/// 成功（1 = 剥净）、产物文本长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripAuditRecord {
    pub count: u8,
    pub source: [u8; V2_AUDIT_CAP],
    pub stripped_ok: [u8; V2_AUDIT_CAP],
    pub text_len: [u16; V2_AUDIT_CAP],
}

impl StripAuditRecord {
    /// 空记录（count=0 起步）。
    pub fn new() -> StripAuditRecord {
        StripAuditRecord { count: 0, source: [0; V2_AUDIT_CAP], stripped_ok: [0; V2_AUDIT_CAP], text_len: [0; V2_AUDIT_CAP] }
    }

    /// 登记一条剥离结果（超容量如实返回 false——不静默吞）。
    pub fn push(&mut self, source: u8, stripped_ok: bool, text_len: u16) -> bool {
        if self.count as usize >= V2_AUDIT_CAP {
            return false;
        }
        let i = self.count as usize;
        self.source[i] = source;
        self.stripped_ok[i] = stripped_ok as u8;
        self.text_len[i] = text_len;
        self.count += 1;
        true
    }

    /// 编码：VXH1 + 版本 + 33 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.count;
        for i in 0..V2_AUDIT_CAP {
            out[6 + i] = self.source[i];
            out[14 + i] = self.stripped_ok[i];
            out[22 + i * 2..24 + i * 2].copy_from_slice(&self.text_len[i].to_le_bytes());
        }
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门 + 计数与来源值域校验。
    pub fn from_bytes(b: &[u8]) -> Result<StripAuditRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[38], b[39], b[40], b[41]]);
        if v2_fnv1a(&b[..38]) != sum { return Err(V2PersistError::BadChecksum); }
        if b[5] as usize > V2_AUDIT_CAP {
            return Err(V2PersistError::BadLength);
        }
        for i in 0..V2_AUDIT_CAP {
            if b[6 + i] > SRC_PLAIN {
                return Err(V2PersistError::BadField);
            }
        }
        let mut rec = StripAuditRecord::new();
        rec.count = b[5];
        rec.source.copy_from_slice(&b[6..14]);
        rec.stripped_ok.copy_from_slice(&b[14..22]);
        for i in 0..V2_AUDIT_CAP {
            rec.text_len[i] = u16::from_le_bytes([b[22 + i * 2], b[23 + i * 2]]);
        }
        Ok(rec)
    }
}

impl Default for StripAuditRecord {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：格式探测纯函数 + 双快捷键路由判定
// ---------------------------------------------------------------------------

/// 来源类型（魔数嗅探结果面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind { Html, Rtf, Png, Jpeg, Gif, Bmp, Plain }

/// 格式探测纯函数（魔数嗅探——F017 格式栈入栈前定序的判定面）：先嗅
/// RTF/图片二进制头，HTML 以 '<' 开头兜底，其余按纯文本。文本嗅探是
/// 最后手段（"BM…" 开头的纯文本会被认作 BMP——二进制头判据优先的取舍）。
pub fn sniff_format(b: &[u8]) -> SourceKind {
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1; // 跳过前导空白
    }
    let r = &b[i..];
    if r.starts_with(b"{\\rtf") {
        return SourceKind::Rtf;
    }
    if r.starts_with(b"\x89PNG\r\n\x1a\n") {
        return SourceKind::Png;
    }
    if r.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return SourceKind::Jpeg;
    }
    if r.starts_with(b"GIF87a") || r.starts_with(b"GIF89a") {
        return SourceKind::Gif;
    }
    if r.starts_with(b"BM") {
        return SourceKind::Bmp;
    }
    if r.starts_with(b"<") {
        return SourceKind::Html;
    }
    SourceKind::Plain
}

/// 双快捷键路由判定：Ctrl+Shift+V → 强制纯文本；Ctrl+V → 按来源格式；
/// 无 Ctrl 按下 → 非粘贴路由（None——显性区分，不静默当 Ctrl+V）。
pub fn route_shortcut(ctrl: bool, shift: bool) -> Option<PasteShortcut> {
    if !ctrl {
        return None;
    }
    Some(if shift { PasteShortcut::PlainOnly } else { PasteShortcut::WithFormat })
}

/// F220 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_pasteplain_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F220-pasteplain-v2");
    let mut rec = StripAuditRecord::new();
    let _ = rec.push(SRC_HTML, true, 9);
    let _ = rec.push(SRC_RTF, true, 9);
    let _ = rec.push(SRC_IMAGE, true, 0);
    let blob = rec.to_bytes();
    // 1. round-trip：审计记录逐条登记→编码→解码逐字段相等（v2 记录纪律
    //    + 「剥离完整性」存档面）。
    set.add("v2 record round-trip strip audit", StripAuditRecord::from_bytes(&blob) == Ok(rec), "");
    // 2. 四类损坏输入全部拒绝 + 字段域越界（魔数/版本/校验/长度/来源）。
    // 缺陷账本：现象=「corruption five-way rejected」红；根因=bad_field 翻
    // 载荷字节 [6] 后未重算校验和（字节 6 在校验覆盖区内），校验门先行返回
    // BadChecksum，BadField 分支不可达；修法=翻位后按同一 FNV-1a 口径重算
    // 校验和再送入，使来源域门被真实测到（五类损坏须逐一显性拒绝），不改实现。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    let mut bad_field = blob;
    bad_field[6] = 9; // 来源码越界（重算校验和——只测字段门这一分支）
    let fs = v2_fnv1a(&bad_field[..V2_RECORD_BYTES - 4]);
    bad_field[V2_RECORD_BYTES - 4..].copy_from_slice(&fs.to_le_bytes());
    set.add(
        "corruption five-way rejected",
        StripAuditRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && StripAuditRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && StripAuditRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && StripAuditRecord::from_bytes(&blob[..41]) == Err(V2PersistError::BadLength)
            && StripAuditRecord::from_bytes(&bad_field) == Err(V2PersistError::BadField),
        "",
    );
    // 3. 验容量在册纪律：8 条封顶，第 9 条登记如实拒绝（不静默吞）。
    let mut full = StripAuditRecord::new();
    let mut all_in = true;
    for _ in 0..V2_AUDIT_CAP + 1 {
        all_in = full.push(SRC_PLAIN, true, 1) && all_in;
    }
    set.add("audit cap 8 honest overflow", !all_in && full.count == V2_AUDIT_CAP as u8, "");
    // 4. 验「HTML/RTF/图片三种来源」探测面：七类魔数头逐一嗅对。
    set.add(
        "format sniff seven headers",
        sniff_format(b"<p>hi</p>") == SourceKind::Html
            && sniff_format(b"  <div>x</div>") == SourceKind::Html
            && sniff_format(b"{\\rtf1\\ansi}") == SourceKind::Rtf
            && sniff_format(b"\x89PNG\r\n\x1a\n\rest") == SourceKind::Png
            && sniff_format(&[0xFF, 0xD8, 0xFF, 0xE0]) == SourceKind::Jpeg
            && sniff_format(b"GIF89a\x01\x00") == SourceKind::Gif
            && sniff_format(b"BM\x36\x00") == SourceKind::Bmp
            && sniff_format(b"plain text") == SourceKind::Plain,
        "",
    );
    // 5. 验「双快捷键并存不冲突」路由面：嗅探喂剥离 + 无 Ctrl 显性 None、
    //    Ctrl+Shift 恒纯文本。
    let html = "<b>粗体</b>";
    let sniffed_html = sniff_format(html.as_bytes()) == SourceKind::Html && !strip_html(html).contains('<');
    set.add(
        "sniff feeds stripper + route matrix",
        sniffed_html
            && route_shortcut(false, false).is_none()
            && route_shortcut(true, false) == Some(PasteShortcut::WithFormat)
            && route_shortcut(true, true) == Some(PasteShortcut::PlainOnly),
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn strip_audit_round_trip_and_reject() {
        let mut rec = StripAuditRecord::new();
        assert!(rec.push(SRC_HTML, true, 5));
        assert!(rec.push(SRC_RTF, false, 0));
        let blob = rec.to_bytes();
        assert_eq!(StripAuditRecord::from_bytes(&blob), Ok(rec));
        assert!(StripAuditRecord::from_bytes(&vec![0u8; 4]).is_err());
    }

    #[test]
    fn sniff_and_route_edges() {
        assert_eq!(sniff_format(b""), SourceKind::Plain);
        assert_eq!(sniff_format(b"\t\r\n{\\rtf"), SourceKind::Rtf); // 前导空白后嗅探
        assert_eq!(route_shortcut(false, true), None);
    }

    #[test]
    fn pasteplain_v2_selfcheck_all_green() {
        let set = run_pasteplain_v2_checks();
        assert!(set.all_passed(), "F220 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
