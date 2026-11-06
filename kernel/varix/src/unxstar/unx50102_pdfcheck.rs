//! UNX-F50102 PDF 直通透传校验（页数/加密/损坏三检）（0x438e0 段）
//!
//! 锚点：UNX-F50102-J1 三检 ×30 注入全中，加密件显式拒绝+三要素告警；
//! 判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x438e0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 12`，
//! 逐项见 [`PdfError`]。
//!
//! 直通透传（Send as PDF）把字节原样交给打印机固件，驱动只能**看不能改**。
//! 因此放行前必须回答三个互不替代的问题，任何一个答错都会在设备侧变成
//! 卡纸/丢页/整单失败：
//! 1. **页数**——trailer 页面树的 `/Count` 声明值必须等于实际叶子页对象
//!    `/Type /Page` 的个数；声明大于实际 = 静默丢页，小于实际 = 多打。
//! 2. **加密**——带 `/Encrypt` 的文件在固件侧必然解不开；必须显式拒绝，
//!    并留下「位置 + 键 + 值」三要素，便于运维定位是哪个 handler/修订版。
//! 3. **损坏**——`%%EOF`、`startxref` 指针、经典 xref 表、trailer 字典、
//!    `obj`/`endobj` 配平，任一不成立都说明字节流不完整，不能直通。
//!
//! 结构清单：
//! * [`PdfHeader`] / [`XrefTable`] / [`EncryptionEvidence`] / [`PdfReport`]：
//!   PDF 结构解析结果与可审计中间量；
//! * [`PdfSpec`] + [`build_fixture`]：可复算的最小 PDF 构造器（判据底座）；
//! * [`validate`] / [`PdfTicket`] / [`Rejection`]：三检主入口与放行票据；
//! * [`injection_core`]：30 例注入核（页数 10 + 加密 10 + 损坏 10）。
//!
//! 错误纪律：内核路径零 panic——所有切片先经 `get`/`Some` 校验，偏移一律
//! 用饱和加法，解析器任何越界都返回显式诊断码，不做静默兜底。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x438e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x438e0;

const E_EMPTY: u16 = 1;
const E_SHORT: u16 = 2;
const E_MAGIC: u16 = 3;
const E_VERSION: u16 = 4;
const E_EOF: u16 = 5;
const E_XREF: u16 = 6;
const E_TRAILER: u16 = 7;
const E_SIZE: u16 = 8;
const E_ENCRYPTED: u16 = 9;
const E_PAGE_COUNT: u16 = 10;
const E_PAGE_MISMATCH: u16 = 11;
const E_CORRUPT: u16 = 12;

/// 页数上限：超过 65535 页的「文档」在办公直通场景必然是坏件
/// （真正的画册也不会走 PDF 直通），提前挡住病态分配。
pub const MAX_PAGES: i64 = 65_535;
/// 分类号表的对象数上限，防伪造 `/Size` 撑爆计数。
pub const MAX_SIZE: i64 = 10_000_000;

/// PDF 直通诊断。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfError {
    /// 输入为空。
    Empty,
    /// 短于最小 PDF 头（16 字节）。
    Short,
    /// 头不是 `%PDF-`。
    Magic,
    /// 版本号不在 1.0–1.7 / 2.0 闭集内。
    Version,
    /// 文件尾 2KiB 内没有 `%%EOF`。
    Eof,
    /// `startxref` 缺失、非数字、越界或指向非 xref 区。
    Xref,
    /// trailer 关键字/字典缺失，或 `/Root` 缺失。
    Trailer,
    /// `/Size` 缺失、非正或大得不可信。
    Size,
    /// 文件带 `/Encrypt`：显式拒绝直通。
    Encrypted,
    /// 页数声明为 0 / 负数 / 超过上限，或无叶子页。
    PageCount,
    /// `/Count` 声明值与实际叶子页数不一致。
    PageMismatch,
    /// 字节流结构性损坏（`obj`/`endobj` 不配平）。
    Corrupt,
}

impl PdfError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PdfError::Empty => E_EMPTY,
            PdfError::Short => E_SHORT,
            PdfError::Magic => E_MAGIC,
            PdfError::Version => E_VERSION,
            PdfError::Eof => E_EOF,
            PdfError::Xref => E_XREF,
            PdfError::Trailer => E_TRAILER,
            PdfError::Size => E_SIZE,
            PdfError::Encrypted => E_ENCRYPTED,
            PdfError::PageCount => E_PAGE_COUNT,
            PdfError::PageMismatch => E_PAGE_MISMATCH,
            PdfError::Corrupt => E_CORRUPT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PdfError::Empty => "empty pdf input",
            PdfError::Short => "shorter than pdf header",
            PdfError::Magic => "%PDF- magic missing",
            PdfError::Version => "pdf version outside supported set",
            PdfError::Eof => "%%EOF missing near end of file",
            PdfError::Xref => "classic xref table broken",
            PdfError::Trailer => "trailer dictionary incomplete",
            PdfError::Size => "trailer /Size invalid",
            PdfError::Encrypted => "pdf is encrypted, passthrough refused",
            PdfError::PageCount => "declared page count unusable",
            PdfError::PageMismatch => "declared /Count differs from leaf pages",
            PdfError::Corrupt => "object stream not balanced",
        }
    }
}

// ---------------------------------------------------------------------------
// 结构解析结果
// ---------------------------------------------------------------------------

/// PDF 头版本（`%PDF-1.7` → 1/7）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfHeader {
    /// 主版本。
    pub major: u8,
    /// 次版本。
    pub minor: u8,
}

impl PdfHeader {
    /// 解析文件头；主版本只接受 1/2，次版本按 PDF 规范闭集校验。
    pub fn parse(d: &[u8]) -> Result<PdfHeader, PdfError> {
        if !at_is(d, 0, b"%PDF-") {
            return Err(PdfError::Magic);
        }
        let major = match d.get(5) {
            Some(v) => *v,
            None => return Err(PdfError::Magic),
        };
        let dot = match d.get(6) {
            Some(v) => *v,
            None => return Err(PdfError::Magic),
        };
        let minor = match d.get(7) {
            Some(v) => *v,
            None => return Err(PdfError::Magic),
        };
        if !major.is_ascii_digit() || dot != b'.' || !minor.is_ascii_digit() {
            return Err(PdfError::Magic);
        }
        let maj = major - b'0';
        let min = minor - b'0';
        // PDF 1.0–1.7 与 2.0；其余都是伪造/损坏头。
        if maj < 1 || maj > 2 {
            return Err(PdfError::Version);
        }
        if maj == 1 && min > 7 {
            return Err(PdfError::Version);
        }
        if maj == 2 && min > 0 {
            return Err(PdfError::Version);
        }
        Ok(PdfHeader {
            major: maj,
            minor: min,
        })
    }

    /// `1.7` 形式的标签。
    pub fn label(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

/// 加密三要素：位置、键（固定 `/Encrypt`）、字典值（handler/V/R）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncryptionEvidence {
    /// 加密字典对象的字节偏移（找不到对象时退化为 trailer 中键的偏移）。
    pub at: usize,
    /// 加密处理器名（`/Filter`），缺省记 `unknown`。
    pub filter: String,
    /// `/V` 算法版本。
    pub version: i64,
    /// `/R` 修订版。
    pub revision: i64,
}

impl EncryptionEvidence {
    /// 三要素告警文本：位置 + 键 + 值，三者缺一不可。
    pub fn warning(&self) -> String {
        format!(
            "encrypted pdf refused: offset={} key=/Encrypt filter={} V={} R={}",
            self.at, self.filter, self.version, self.revision
        )
    }

    /// 修订版是否落在 PDF 规范已知闭集 2..=6。
    pub fn known_revision(&self) -> bool {
        self.revision >= 2 && self.revision <= 6
    }
}

/// 经典 xref 表解析摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XrefTable {
    /// 表起始偏移。
    pub offset: usize,
    /// 有效条目数（含空闲项）。
    pub entries: u32,
    /// 子区段数。
    pub subsections: u32,
    /// 表体结束位置（trailer 关键字之前）。
    pub end: usize,
}

/// 一次结构解析的完整画像。
#[derive(Clone, Debug, PartialEq)]
pub struct PdfReport {
    /// 头版本。
    pub header: PdfHeader,
    /// 页面树 `/Count` 声明值。
    pub page_count: u32,
    /// 实际叶子页对象数。
    pub leaf_pages: u32,
    /// 是否带 `/Encrypt`。
    pub encrypted: bool,
    /// 加密三要素。
    pub encryption: Option<EncryptionEvidence>,
    /// xref 条目数。
    pub xref_entries: u32,
    /// `obj` 对象数。
    pub objects: u32,
    /// `%%EOF` 位置。
    pub eof_at: usize,
    /// trailer 关键字位置。
    pub trailer_at: usize,
    /// 文件字节数。
    pub size_bytes: usize,
    /// 是否线性化（首屏优化标记，仅供审计）。
    pub linearized: bool,
}

impl PdfReport {
    /// 三检是否全部通过（不含加密拒绝语义）。
    pub fn structurally_ok(&self) -> bool {
        self.page_count > 0
            && self.page_count == self.leaf_pages
            && !self.encrypted
            && self.objects > 0
            && self.xref_entries > 0
    }

    /// 一行可写回软件面的摘要（结果摘要必须非空）。
    pub fn summary(&self) -> String {
        format!(
            "pdf {} pages={}/{} enc={} xref={} objs={} bytes={} linearized={}",
            self.header.label(),
            self.page_count,
            self.leaf_pages,
            self.encrypted,
            self.xref_entries,
            self.objects,
            self.size_bytes,
            self.linearized
        )
    }
}

/// 放行票据：直通链下游只认这张票，不再重复解析字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfTicket {
    /// 头版本。
    pub header: PdfHeader,
    /// 页数（= 叶子页数）。
    pub pages: u32,
    /// 对象数。
    pub objects: u32,
    /// xref 条目数。
    pub xref_entries: u32,
    /// 字节数。
    pub size_bytes: u32,
    /// FNV-1a 内容指纹，用于回退链对账。
    pub digest: u32,
}

/// 拒绝记录：诊断码 + 偏移 + 可选的加密三要素。
#[derive(Clone, Debug, PartialEq)]
pub struct Rejection {
    /// 具体诊断。
    pub error: PdfError,
    /// 相关字节偏移（0 表示文件级判定）。
    pub at: usize,
    /// 加密件必带三要素。
    pub alert: Option<EncryptionEvidence>,
}

impl Rejection {
    /// 诊断码透传。
    pub fn code(&self) -> u32 {
        self.error.code()
    }

    /// 告警文本：有加密三要素时优先输出三要素。
    pub fn warning(&self) -> String {
        match &self.alert {
            Some(a) => a.warning(),
            None => format!(
                "pdf check failed: code=0x{:x} at={} msg={}",
                self.error.code(),
                self.at,
                self.error.message()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 字节级工具：全部先判界再切片
// ---------------------------------------------------------------------------

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0c || b == 0
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'#'
}

fn at_is(d: &[u8], at: usize, pat: &[u8]) -> bool {
    if pat.is_empty() {
        return false;
    }
    match d.get(at..at.saturating_add(pat.len())) {
        Some(s) => s == pat,
        None => false,
    }
}

fn find_from(d: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    if pat.is_empty() || d.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i.saturating_add(pat.len()) <= d.len() {
        if at_is(d, i, pat) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn rfind(d: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || d.len() < pat.len() {
        return None;
    }
    let mut i = d.len() - pat.len();
    loop {
        if at_is(d, i, pat) {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn skip_ws(d: &[u8], mut at: usize) -> usize {
    while at < d.len() && is_ws(d[at]) {
        at += 1;
    }
    at
}

fn parse_u32(d: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut i = at;
    let mut v: u64 = 0;
    let mut n = 0usize;
    while i < d.len() && d[i].is_ascii_digit() {
        v = v * 10 + (d[i] - b'0') as u64;
        if v > u32::MAX as u64 {
            return None;
        }
        i += 1;
        n += 1;
    }
    if n == 0 {
        None
    } else {
        Some((v as u32, i))
    }
}

fn parse_i64(d: &[u8], at: usize) -> Option<(i64, usize)> {
    let mut i = at;
    let mut neg = false;
    if d.get(i) == Some(&b'-') {
        neg = true;
        i += 1;
    }
    let mut v: i64 = 0;
    let mut n = 0usize;
    while i < d.len() && d[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add((d[i] - b'0') as i64);
        i += 1;
        n += 1;
    }
    if n == 0 {
        None
    } else if neg {
        Some((-v, i))
    } else {
        Some((v, i))
    }
}

/// 在字典中找 `name` 后的整数值；名字后必须是分隔符，避免 `/V` 命中 `/Version`。
fn find_value_i64(d: &[u8], name: &[u8]) -> Option<i64> {
    let mut at = 0usize;
    while let Some(p) = find_from(d, name, at) {
        let q = p + name.len();
        let bounded = q >= d.len() || is_ws(d[q]) || is_delim(d[q]);
        if bounded {
            let v = skip_ws(d, q);
            if let Some((n, _)) = parse_i64(d, v) {
                return Some(n);
            }
        }
        at = q;
    }
    None
}

fn count_occurrences(d: &[u8], pat: &[u8]) -> usize {
    let mut n = 0usize;
    let mut at = 0usize;
    while let Some(p) = find_from(d, pat, at) {
        n += 1;
        at = p + pat.len();
    }
    n
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 解析经典 xref 表：只接受 `xref` 关键字起始的经典表；交叉引用流在
/// 直通场景极少且需额外解码，显式归入 `Xref` 不支持，避免半解析假绿。
fn parse_xref(d: &[u8], off: usize) -> Result<XrefTable, PdfError> {
    if off >= d.len() || !at_is(d, off, b"xref") {
        return Err(PdfError::Xref);
    }
    let mut at = off + 4;
    let mut entries = 0u32;
    let mut subsections = 0u32;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 100_000 {
            return Err(PdfError::Xref);
        }
        at = skip_ws(d, at);
        if at >= d.len() {
            return Err(PdfError::Xref);
        }
        if at_is(d, at, b"trailer") {
            return Ok(XrefTable {
                offset: off,
                entries,
                subsections,
                end: at,
            });
        }
        // 表体在非数字处结束（例如 trailer 关键字被拼坏）：正常收口，
        // 由上层区分「trailer 缺失」与「xref 结构坏」，避免码位串扰。
        if !d[at].is_ascii_digit() {
            return Ok(XrefTable {
                offset: off,
                entries,
                subsections,
                end: at,
            });
        }
        let (start, a1) = match parse_u32(d, at) {
            Some(v) => v,
            None => return Err(PdfError::Xref),
        };
        let _ = start;
        let a2 = skip_ws(d, a1);
        let (count, mut a) = match parse_u32(d, a2) {
            Some(v) => v,
            None => return Err(PdfError::Xref),
        };
        if count > MAX_PAGES as u32 {
            return Err(PdfError::Xref);
        }
        let mut i = 0u32;
        while i < count {
            a = skip_ws(d, a);
            let (val, na) = match parse_u32(d, a) {
                Some(v) => v,
                None => return Err(PdfError::Xref),
            };
            let b1 = skip_ws(d, na);
            let (_gen, nb) = match parse_u32(d, b1) {
                Some(v) => v,
                None => return Err(PdfError::Xref),
            };
            let b2 = skip_ws(d, nb);
            let kind = match d.get(b2) {
                Some(v) => *v,
                None => return Err(PdfError::Xref),
            };
            if kind != b'n' && kind != b'f' {
                return Err(PdfError::Xref);
            }
            // 在用条目必须指向文件内；空闲条目 (f) 的偏移无意义。
            if kind == b'n' && val as usize >= d.len() {
                return Err(PdfError::Xref);
            }
            let mut c = b2 + 1;
            while c < d.len() && d[c] != b'\n' && d[c] != b'\r' {
                c += 1;
            }
            if c < d.len() && d[c] == b'\r' {
                c += 1;
            }
            if c < d.len() && d[c] == b'\n' {
                c += 1;
            }
            a = c;
            i += 1;
            entries = entries.saturating_add(1);
        }
        subsections = subsections.saturating_add(1);
        if subsections > 64 {
            return Err(PdfError::Xref);
        }
        if a <= at {
            return Err(PdfError::Xref);
        }
        at = a;
    }
}

/// 统计 `/Type /Page` 叶子对象；`/Pages`、`/PageMode` 因后随名字字符被排除。
fn scan_leaf_pages(d: &[u8]) -> u32 {
    let mut n = 0u32;
    let mut at = 0usize;
    while let Some(p) = find_from(d, b"/Page", at) {
        let after = p + 5;
        let ok_after = after >= d.len() || !is_name_char(d[after]);
        let ok_before = p == 0 || d[p - 1] != b'/';
        if ok_after && ok_before {
            n = n.saturating_add(1);
        }
        at = after;
    }
    n
}

/// 从 `/Type /Pages` 字典里取 `/Count`；窗口 256 字节覆盖 Kids 数组。
fn find_pages_count(d: &[u8]) -> Option<i64> {
    let mut at = 0usize;
    while let Some(p) = find_from(d, b"/Type", at) {
        let q = skip_ws(d, p + 5);
        if at_is(d, q, b"/Pages") {
            let after = q + 6;
            let ok = after >= d.len() || !is_name_char(d[after]);
            if ok {
                // 窗口取到所在字典的 `>>` 为止：Kids 数组可能很长（100 页
                // 就有近 1KiB 引用），固定 320 字节窗口会漏掉 /Count。
                let end = match find_from(d, b">>", q) {
                    Some(p) => p,
                    None => {
                        if q.saturating_add(65_536) < d.len() {
                            q + 65_536
                        } else {
                            d.len()
                        }
                    }
                };
                if let Some(win) = d.get(q..end) {
                    if let Some(v) = find_value_i64(win, b"/Count") {
                        return Some(v);
                    }
                }
            }
        }
        at = p + 5;
    }
    None
}

/// 统计 `obj` 与 `endobj`；`endobj` 里的 `obj` 不重复计数。
fn count_objects(d: &[u8]) -> (u32, u32) {
    let endobj = count_occurrences(d, b"endobj");
    let mut objs = 0u32;
    let mut at = 0usize;
    while let Some(p) = find_from(d, b"obj", at) {
        let inside_end = p >= 3 && at_is(d, p - 3, b"end");
        if !inside_end {
            let before_ok = p == 0 || is_ws(d[p - 1]) || d[p - 1] == b'>' || d[p - 1] == b']';
            let after = p + 3;
            let after_ok =
                after >= d.len() || is_ws(d[after]) || d[after] == b'<' || d[after] == b'/';
            if before_ok && after_ok {
                objs = objs.saturating_add(1);
            }
        }
        at = p + 3;
    }
    (objs, endobj as u32)
}

/// 解析加密字典：先按 trailer 中的引用号定位对象，再读 /Filter /V /R。
fn parse_encryption(data: &[u8], key_at: usize) -> EncryptionEvidence {
    // trailer 里 /Encrypt 后是对象引用 "N 0 R"。
    let mut obj_at = key_at;
    let mut filter = "unknown".to_string();
    let mut version = 0i64;
    let mut revision = 0i64;
    // 跳过 "/Encrypt"（8 字节）后才是对象引用。
    let mut q = skip_ws(data, key_at.saturating_add(8));
    if let Some((num, after)) = parse_u32(data, q) {
        q = skip_ws(data, after);
        // 跳过代号 "0 R"
        if let Some((_gen, a2)) = parse_u32(data, q) {
            let r = skip_ws(data, a2);
            if at_is(data, r, b"R") {
                let needle = format!("{} 0 obj", num);
                if let Some(off) = find_from(data, needle.as_bytes(), 0) {
                    obj_at = off;
                }
            }
        }
    }
    let end = if obj_at.saturating_add(512) < data.len() {
        obj_at + 512
    } else {
        data.len()
    };
    let win = match data.get(obj_at..end) {
        Some(w) => w,
        None => data,
    };
    if let Some(fp) = find_from(win, b"/Filter", 0) {
        let mut s = skip_ws(win, fp + 7);
        if win.get(s) == Some(&b'/') {
            s += 1;
            let mut e = s;
            while e < win.len() && !is_ws(win[e]) && !is_delim(win[e]) {
                e += 1;
            }
            if e > s {
                let mut txt = String::new();
                let mut i = s;
                while i < e {
                    if let Some(b) = win.get(i) {
                        if *b < 0x80 {
                            txt.push(*b as char);
                        } else {
                            txt.push('?');
                        }
                    }
                    i += 1;
                }
                if !txt.is_empty() {
                    filter = txt;
                }
            }
        }
    }
    if let Some(v) = find_value_i64(win, b"/V") {
        version = v;
    }
    if let Some(v) = find_value_i64(win, b"/R") {
        revision = v;
    }
    EncryptionEvidence {
        at: obj_at,
        filter,
        version,
        revision,
    }
}

/// 结构画像主入口；加密件在这里**不**报错，由 [`validate`] 做显式拒绝，
/// 这样上层可以先拿到三要素再决定告警通道。
pub fn inspect(data: &[u8]) -> Result<PdfReport, Rejection> {
    inspect_inner(data).map_err(|e| Rejection {
        error: e,
        at: 0,
        alert: None,
    })
}

fn inspect_inner(data: &[u8]) -> Result<PdfReport, PdfError> {
    if data.is_empty() {
        return Err(PdfError::Empty);
    }
    if data.len() < 16 {
        return Err(PdfError::Short);
    }
    let header = PdfHeader::parse(data)?;

    // %%EOF 必须在尾部 2KiB 内：PDF 允许尾部附加少量垃圾，但不允许缺失。
    let eof_at = match rfind(data, b"%%EOF") {
        Some(p) => p,
        None => return Err(PdfError::Eof),
    };
    if eof_at.saturating_add(2048) < data.len() {
        return Err(PdfError::Eof);
    }

    let sx = match rfind(data, b"startxref") {
        Some(p) => p,
        None => return Err(PdfError::Xref),
    };
    let (ptr, _) = match parse_u32(data, skip_ws(data, sx + 9)) {
        Some(v) => v,
        None => return Err(PdfError::Xref),
    };
    if ptr as usize >= data.len() {
        return Err(PdfError::Xref);
    }
    let xref = parse_xref(data, ptr as usize)?;
    if xref.entries == 0 {
        return Err(PdfError::Xref);
    }

    // trailer 关键字必须落在 xref 表体之后、startxref 之前。
    if xref.end > sx {
        return Err(PdfError::Trailer);
    }
    let trailer_at = match find_from(data, b"trailer", xref.end) {
        Some(p) if p < sx => p,
        _ => return Err(PdfError::Trailer),
    };
    let dict_start = match find_from(data, b"<<", trailer_at + 7) {
        Some(p) if p < sx => p,
        _ => return Err(PdfError::Trailer),
    };
    let dict_end = match find_from(data, b">>", dict_start + 2) {
        Some(p) if p < sx => p,
        _ => return Err(PdfError::Trailer),
    };
    let dict = match data.get(dict_start..dict_end) {
        Some(v) => v,
        None => return Err(PdfError::Trailer),
    };

    // /Root 是直通后设备找页面树的唯一入口，缺失即拒。
    if find_from(dict, b"/Root", 0).is_none() {
        return Err(PdfError::Trailer);
    }
    match find_value_i64(dict, b"/Size") {
        Some(v) if v > 0 && v <= MAX_SIZE => {}
        _ => return Err(PdfError::Size),
    }

    let encrypt_rel = find_from(dict, b"/Encrypt", 0);
    let encrypted = encrypt_rel.is_some();
    let encryption = match encrypt_rel {
        Some(rel) => Some(parse_encryption(data, dict_start + rel)),
        None => None,
    };

    // 页数三检的第一检：声明值与叶子页数必须同时可用且相等。
    let declared = match find_pages_count(data) {
        Some(v) => v,
        None => return Err(PdfError::PageCount),
    };
    if declared <= 0 || declared > MAX_PAGES {
        return Err(PdfError::PageCount);
    }
    let leaf = scan_leaf_pages(data);
    if leaf == 0 {
        return Err(PdfError::PageCount);
    }
    if leaf as i64 != declared {
        return Err(PdfError::PageMismatch);
    }

    // 损坏检：对象开始/结束必须配平；一个都不配平视为流被截断。
    let (objects, endobjs) = count_objects(data);
    if objects == 0 || objects != endobjs {
        return Err(PdfError::Corrupt);
    }

    let linearized = find_from(data, b"/Linearized", 0).is_some();
    Ok(PdfReport {
        header,
        page_count: declared as u32,
        leaf_pages: leaf,
        encrypted,
        encryption,
        xref_entries: xref.entries,
        objects,
        eof_at,
        trailer_at,
        size_bytes: data.len(),
        linearized,
    })
}

fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < data.len() {
        if let Some(b) = data.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 三检主入口：结构 → 加密显式拒绝 → 放行票据。
pub fn validate(data: &[u8]) -> Result<PdfTicket, Rejection> {
    let report = inspect(data)?;
    let digest = fnv1a(data);
    let pages = report.page_count;
    let objects = report.objects;
    let xref_entries = report.xref_entries;
    let size_bytes = report.size_bytes as u32;
    let header = report.header;
    if report.encrypted {
        return Err(Rejection {
            error: PdfError::Encrypted,
            at: report.encryption.as_ref().map(|e| e.at).unwrap_or(0),
            alert: report.encryption,
        });
    }
    Ok(PdfTicket {
        header,
        pages,
        objects,
        xref_entries,
        size_bytes,
        digest,
    })
}

// ---------------------------------------------------------------------------
// 最小 PDF 构造器：判据与注入核共用，保证「同一算式在两边各算一遍」
// ---------------------------------------------------------------------------

/// 加密字典参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncSpec {
    /// `/V`。
    pub v: u8,
    /// `/R`。
    pub r: u8,
    /// `/Filter` 名字；`None` 表示故意不写。
    pub filter: Option<&'static str>,
    /// trailer 中 `/Encrypt` 的引用文本覆盖（如 `0 0 R`）。
    pub ref_text: Option<&'static str>,
}

/// 可复算的最小 PDF 规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfSpec {
    /// 叶子页数。
    pub pages: u16,
    /// 加密字典（`None` = 明文）。
    pub encrypt: Option<EncSpec>,
    /// 页面树 `/Count` 文本覆盖（`-1` 等坏值）。
    pub count_text: Option<&'static str>,
    /// trailer `/Size` 文本覆盖。
    pub size_text: Option<&'static str>,
    /// 是否省略 `/Size`。
    pub omit_size: bool,
    /// 是否省略 `/Root`。
    pub omit_root: bool,
    /// 是否省略 `%%EOF`。
    pub eof: bool,
    /// `startxref` 指针额外偏移（造越界）。
    pub startxref_extra: u32,
    /// `startxref` 指向第一个对象体（造非 xref 指针）。
    pub startxref_to_first_obj: bool,
    /// xref 子区段计数虚增（造表体不完整）。
    pub subs_extra: u32,
    /// 第一条在用 xref 条目偏移写成 4e9（造越界条目）。
    pub bad_entry: bool,
    /// 页面树类型写成 `/Paggs`（造缺 `/Type /Pages`）。
    pub break_pages_type: bool,
    /// trailer 关键字拼写覆盖。
    pub trailer_key: &'static str,
    /// 构造后替换一个 `endobj`（造对象不配平）。
    pub unbalance_obj: bool,
    /// 文件头覆盖。
    pub header: &'static str,
}

impl Default for PdfSpec {
    fn default() -> PdfSpec {
        PdfSpec {
            pages: 3,
            encrypt: None,
            count_text: None,
            size_text: None,
            omit_size: false,
            omit_root: false,
            eof: true,
            startxref_extra: 0,
            startxref_to_first_obj: false,
            subs_extra: 0,
            bad_entry: false,
            break_pages_type: false,
            trailer_key: "trailer",
            unbalance_obj: false,
            header: "%PDF-1.4\n",
        }
    }
}

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn replace_first(d: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(d.len() + to.len());
    match find_from(d, from, 0) {
        Some(p) => {
            out.extend_from_slice(&d[..p]);
            out.extend_from_slice(to);
            out.extend_from_slice(&d[p + from.len()..]);
        }
        None => out.extend_from_slice(d),
    }
    out
}

/// 按规格构造最小 PDF；xref 偏移全部由 `out.len()` 现算，因此任何参数
/// 变化都不会出现「偏移与内容不同步」的伪失败。
pub fn build_fixture(spec: &PdfSpec) -> Vec<u8> {
    let pages = spec.pages;
    let enc_num = 3u32 + pages as u32;
    let mut out: Vec<u8> = Vec::new();
    put(&mut out, spec.header);
    out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

    let mut offsets: Vec<usize> = Vec::new();
    offsets.push(0); // 对象 0 是空闲项占位

    // 1: catalog
    offsets.push(out.len());
    put(
        &mut out,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );

    // 2: pages
    offsets.push(out.len());
    let mut kids = String::new();
    let mut i = 0u16;
    while i < pages {
        if i > 0 {
            kids.push(' ');
        }
        kids.push_str(&format!("{} 0 R", 3 + i as u32));
        i += 1;
    }
    let count_s = match spec.count_text {
        Some(t) => t.to_string(),
        None => format!("{}", pages),
    };
    let pages_type = if spec.break_pages_type { "Paggs" } else { "Pages" };
    put(
        &mut out,
        &format!(
            "2 0 obj\n<< /Type /{} /Kids [{}] /Count {} >>\nendobj\n",
            pages_type, kids, count_s
        ),
    );

    // 3..: 叶子页
    let mut p = 0u16;
    while p < pages {
        offsets.push(out.len());
        put(
            &mut out,
            &format!(
                "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>\nendobj\n",
                3 + p as u32
            ),
        );
        p += 1;
    }

    // 可选加密字典
    if let Some(enc) = spec.encrypt {
        offsets.push(out.len());
        let mut dict = String::new();
        dict.push_str("<< ");
        if let Some(f) = enc.filter {
            dict.push_str("/Filter /");
            dict.push_str(f);
            dict.push(' ');
        }
        dict.push_str(&format!("/V {} /R {} /Length 128 >>", enc.v, enc.r));
        put(
            &mut out,
            &format!("{} 0 obj\n{}\nendobj\n", enc_num, dict),
        );
    }

    // xref：偏移即当前字节数
    let xref_off = out.len();
    let entries_all = offsets.len() as u32;
    put(&mut out, "xref\n");
    put(
        &mut out,
        &format!("0 {}\n", entries_all.saturating_add(spec.subs_extra)),
    );
    put(&mut out, "0000000000 65535 f \n");
    let mut k = 1usize;
    while k < offsets.len() {
        let off = if spec.bad_entry && k == 1 {
            4_000_000_000usize
        } else {
            match offsets.get(k) {
                Some(v) => *v,
                None => 0,
            }
        };
        put(&mut out, &format!("{:010} 00000 n \n", off));
        k += 1;
    }

    // trailer
    put(&mut out, spec.trailer_key);
    put(&mut out, "\n<< ");
    if !spec.omit_size {
        let size_s = match spec.size_text {
            Some(t) => t.to_string(),
            None => format!("{}", entries_all),
        };
        put(&mut out, &format!("/Size {} ", size_s));
    }
    if !spec.omit_root {
        put(&mut out, "/Root 1 0 R ");
    }
    if let Some(enc) = spec.encrypt {
        let ref_s = match enc.ref_text {
            Some(t) => t.to_string(),
            None => format!("{} 0 R", enc_num),
        };
        put(&mut out, &format!("/Encrypt {} ", ref_s));
    }
    put(&mut out, ">>\n");

    let base_ptr = if spec.startxref_to_first_obj {
        match offsets.get(1) {
            Some(v) => *v,
            None => 0,
        }
    } else {
        xref_off
    };
    put(&mut out, "startxref\n");
    put(
        &mut out,
        &format!("{}\n", base_ptr.saturating_add(spec.startxref_extra as usize)),
    );
    if spec.eof {
        put(&mut out, "%%EOF\n");
    }

    if spec.unbalance_obj {
        out = replace_first(&out, b"endobj", b"endobx");
    }
    out
}

// ---------------------------------------------------------------------------
// 注入核：页数 10 + 加密 10 + 损坏 10，共 30 例
// ---------------------------------------------------------------------------

fn outcome(data: &[u8]) -> Option<PdfError> {
    match validate(data) {
        Ok(_) => None,
        Err(r) => Some(r.error),
    }
}

/// 30 例分类注入核。返回 (正确分类数, 总数)；每一项都断言**具体**诊断码，
/// 而不是只断言「失败」——诊断码错位同样算漏检。
pub fn injection_core() -> (usize, usize) {
    let cases = injection_cases();
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((data, want)) = cases.get(i) {
            if outcome(data) == *want {
                caught += 1;
            }
        }
        i += 1;
    }
    (caught, cases.len())
}

/// 注入病例清单（供注入核与诊断详情共用，保证两边样本完全一致）。
fn injection_cases() -> Vec<(Vec<u8>, Option<PdfError>)> {
    let mut cases: Vec<(Vec<u8>, Option<PdfError>)> = Vec::new();

    // ---- 页数检 10 例 ----
    let mut sp = PdfSpec::default();
    sp.count_text = Some("0");
    cases.push((build_fixture(&sp), Some(PdfError::PageCount)));

    let mut sp = PdfSpec::default();
    sp.count_text = Some("9");
    cases.push((build_fixture(&sp), Some(PdfError::PageMismatch)));

    let mut sp = PdfSpec::default();
    sp.count_text = Some("99999");
    cases.push((build_fixture(&sp), Some(PdfError::PageCount)));

    let mut sp = PdfSpec::default();
    sp.count_text = Some("-1");
    cases.push((build_fixture(&sp), Some(PdfError::PageCount)));

    let mut sp = PdfSpec::default();
    sp.break_pages_type = true;
    cases.push((build_fixture(&sp), Some(PdfError::PageCount)));

    let mut sp = PdfSpec::default();
    sp.pages = 0;
    sp.count_text = Some("0");
    cases.push((build_fixture(&sp), Some(PdfError::PageCount)));

    // 声明 3 页，但把一个叶子页类型改坏 → 实际 2 页
    let base3 = build_fixture(&PdfSpec::default());
    let leaf_broken = replace_first(&base3, b"/Type /Page ", b"/Type /PagX ");
    cases.push((leaf_broken, Some(PdfError::PageMismatch)));

    let mut sp = PdfSpec::default();
    sp.count_text = Some("1");
    cases.push((build_fixture(&sp), Some(PdfError::PageMismatch)));

    // 负例：100 页合法件必须放行
    let mut sp = PdfSpec::default();
    sp.pages = 100;
    cases.push((build_fixture(&sp), None));

    // 负例：5 页合法件必须放行
    let mut sp = PdfSpec::default();
    sp.pages = 5;
    cases.push((build_fixture(&sp), None));

    // ---- 加密检 10 例 ----
    let enc_variants: [(u8, u8, Option<&'static str>, Option<&'static str>); 10] = [
        (1, 2, Some("Standard"), None),
        (2, 3, Some("Standard"), None),
        (4, 4, Some("Standard"), None),
        (5, 5, Some("Standard"), None),
        (5, 6, Some("Standard"), None),
        (0, 0, Some("Standard"), None),
        (255, 255, Some("Standard"), None),
        (4, 4, None, None),
        (4, 4, Some("Standard"), Some("0 0 R")),
        (2, 3, Some("Standard"), None),
    ];
    let mut e = 0usize;
    while e < enc_variants.len() {
        if let Some(v) = enc_variants.get(e) {
            let mut sp = PdfSpec::default();
            sp.pages = if e == 7 { 1 } else { 2 };
            sp.encrypt = Some(EncSpec {
                v: v.0,
                r: v.1,
                filter: v.2,
                ref_text: v.3,
            });
            cases.push((build_fixture(&sp), Some(PdfError::Encrypted)));
        }
        e += 1;
    }

    // ---- 损坏检 10 例 ----
    cases.push((Vec::new(), Some(PdfError::Empty)));
    cases.push((b"%PDF-1.4\nx".to_vec(), Some(PdfError::Short)));

    let mut sp = PdfSpec::default();
    sp.header = "%PDX-1.4\n";
    cases.push((build_fixture(&sp), Some(PdfError::Magic)));

    let mut sp = PdfSpec::default();
    sp.header = "%PDF-9.9\n";
    cases.push((build_fixture(&sp), Some(PdfError::Version)));

    let mut sp = PdfSpec::default();
    sp.eof = false;
    cases.push((build_fixture(&sp), Some(PdfError::Eof)));

    let mut sp = PdfSpec::default();
    sp.startxref_extra = 100_000;
    cases.push((build_fixture(&sp), Some(PdfError::Xref)));

    let mut sp = PdfSpec::default();
    sp.trailer_key = "trailor";
    cases.push((build_fixture(&sp), Some(PdfError::Trailer)));

    let mut sp = PdfSpec::default();
    sp.size_text = Some("0");
    cases.push((build_fixture(&sp), Some(PdfError::Size)));

    let mut sp = PdfSpec::default();
    sp.unbalance_obj = true;
    cases.push((build_fixture(&sp), Some(PdfError::Corrupt)));

    let mut sp = PdfSpec::default();
    sp.omit_root = true;
    cases.push((build_fixture(&sp), Some(PdfError::Trailer)));

    cases
}

/// 诊断详情：列出分类错误的样本序号与期望/实际码（仅供判据落红时定位）。
fn injection_mismatch_detail() -> String {
    let cases = injection_cases();
    let mut out = String::new();
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((data, want)) = cases.get(i) {
            let got = outcome(data);
            if got != *want {
                let w = match want {
                    Some(e) => e.code(),
                    None => 0,
                };
                let g = match got {
                    Some(e) => e.code(),
                    None => 0,
                };
                out.push_str(&format!("[{} want=0x{:x} got=0x{:x}] ", i, w, g));
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50102 自检：16 条判据。
pub fn run_unx50102_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50102");
    let plain = build_fixture(&PdfSpec::default());

    // J1-1 头版本解析：合法 1.4 通过，9.9 显式 Version。
    let h = PdfHeader::parse(&plain);
    let ok = match h {
        Ok(v) => v.major == 1 && v.minor == 4 && v.label() == "1.4",
        Err(_) => false,
    } && PdfHeader::parse(b"%PDF-9.9\nrest").err() == Some(PdfError::Version);
    s.add("50102-J1-版本头", ok, if ok { "" } else { "版本闭集失效" });

    // J1-2 合法 3 页件放行，票据三要素守恒
    let t = validate(&plain);
    let ok = match &t {
        Ok(v) => v.pages == 3 && v.objects == 5 && v.xref_entries == 6 && v.size_bytes as usize == plain.len(),
        Err(_) => false,
    };
    s.add("50102-J1-放行票据", ok, if ok { "" } else { "票据字段不守恒" });

    // J1-3 叶子页扫描：3 页件得 3，/Pages 不被误计
    let leaf = scan_leaf_pages(&plain);
    let ok = leaf == 3;
    let det = format!("leaf={}", leaf);
    s.add_dyn("50102-J1-叶子页计数", ok, &det);

    // J1-4 页数声明零 → PageCount；声明 9 实际 3 → PageMismatch（码值必须区分）
    let mut sp = PdfSpec::default();
    sp.count_text = Some("0");
    let zero = build_fixture(&sp);
    let mut sp2 = PdfSpec::default();
    sp2.count_text = Some("9");
    let over = build_fixture(&sp2);
    let ok = outcome(&zero) == Some(PdfError::PageCount)
        && outcome(&over) == Some(PdfError::PageMismatch);
    s.add("50102-J1-页数两码", ok, if ok { "" } else { "页数码位混淆" });

    // J1-5 加密件显式拒绝：码 = Encrypted，且 revision 取到真实值
    let enc_spec = PdfSpec {
        pages: 2,
        encrypt: Some(EncSpec {
            v: 4,
            r: 4,
            filter: Some("Standard"),
            ref_text: None,
        }),
        ..PdfSpec::default()
    };
    let enc = build_fixture(&enc_spec);
    let rej = validate(&enc).err();
    let ok = match &rej {
        Some(r) => {
            r.error == PdfError::Encrypted
                && r.alert.is_some()
                && r.alert.as_ref().map(|a| a.revision) == Some(4)
        }
        None => false,
    };
    s.add("50102-J1-加密拒绝", ok, if ok { "" } else { "加密件未拒" });

    // J1-6 三要素告警：位置 + /Encrypt + filter/V/R 全部在场
    let warn = match &rej {
        Some(r) => r.warning(),
        None => String::new(),
    };
    let ok = warn.contains("offset=")
        && warn.contains("key=/Encrypt")
        && warn.contains("filter=Standard")
        && warn.contains("V=4")
        && warn.contains("R=4");
    s.add_dyn("50102-J1-三要素告警", ok, &warn);

    // J1-7 明文件不得被误标加密
    let report = inspect(&plain);
    let ok = match &report {
        Ok(r) => !r.encrypted && r.encryption.is_none(),
        Err(_) => false,
    };
    s.add("50102-J1-误报加密", ok, if ok { "" } else { "明文被标加密" });

    // J1-8 xref 守恒：条目数 = 对象数 + 1（空闲项），三档页数都成立
    let mut cons = true;
    let pages = [1u16, 3u16, 17u16];
    let mut i = 0usize;
    while i < pages.len() {
        let p = match pages.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let mut sp = PdfSpec::default();
        sp.pages = p;
        let d = build_fixture(&sp);
        match inspect(&d) {
            Ok(r) => {
                if r.xref_entries != r.objects + 1 {
                    cons = false;
                }
                if r.page_count != p as u32 || r.leaf_pages != p as u32 {
                    cons = false;
                }
            }
            Err(_) => cons = false,
        }
        i += 1;
    }
    s.add("50102-J1-xref守恒", cons, if cons { "" } else { "xref/对象不守恒" });

    // J1-9 确定性：同字节两次放行逐字段一致，摘要非空
    let t2 = validate(&plain);
    let ok = match (&t, &t2) {
        (Ok(a), Ok(b)) => a == b && a.digest != 0 && !report.as_ref().map(|r| r.summary()).unwrap_or_default().is_empty(),
        _ => false,
    };
    s.add("50102-J1-确定摘要", ok, if ok { "" } else { "非确定或摘要空" });

    // J1-10 指纹区分：3 页件与 17 页件指纹不同
    let mut sp = PdfSpec::default();
    sp.pages = 17;
    let big = build_fixture(&sp);
    let ok = match (validate(&plain), validate(&big)) {
        (Ok(a), Ok(b)) => a.digest != b.digest,
        _ => false,
    };
    s.add("50102-J1-指纹区分", ok, if ok { "" } else { "指纹碰撞" });

    // J1-11 缺 %%EOF → Eof
    let mut sp = PdfSpec::default();
    sp.eof = false;
    let no_eof = build_fixture(&sp);
    let ok = outcome(&no_eof) == Some(PdfError::Eof);
    s.add("50102-J1-EOF必检", ok, if ok { "" } else { "缺 EOF 未拒" });

    // J1-12 startxref 越界 → Xref；指向对象体也必须拒
    let mut sp = PdfSpec::default();
    sp.startxref_extra = 100_000;
    let far = build_fixture(&sp);
    let mut sp2 = PdfSpec::default();
    sp2.startxref_to_first_obj = true;
    let to_obj = build_fixture(&sp2);
    let ok = outcome(&far) == Some(PdfError::Xref) && outcome(&to_obj) == Some(PdfError::Xref);
    s.add("50102-J1-指针必检", ok, if ok { "" } else { "startxref 漏检" });

    // J1-13 trailer /Size 两路：0 与缺失都归 Size，不与 Trailer 混淆
    let mut sp = PdfSpec::default();
    sp.size_text = Some("0");
    let zero_size = build_fixture(&sp);
    let mut sp2 = PdfSpec::default();
    sp2.omit_size = true;
    let no_size = build_fixture(&sp2);
    let ok = outcome(&zero_size) == Some(PdfError::Size) && outcome(&no_size) == Some(PdfError::Size);
    s.add("50102-J1-Size必检", ok, if ok { "" } else { "/Size 码位错" });

    // J1-14 对象配平：合法件 objs==endobjs，坏件归 Corrupt
    let (o, e) = count_objects(&plain);
    let mut sp = PdfSpec::default();
    sp.unbalance_obj = true;
    let unbal = build_fixture(&sp);
    let ok = o == e && o == 5 && outcome(&unbal) == Some(PdfError::Corrupt);
    let det = format!("objs={} endobjs={}", o, e);
    s.add_dyn("50102-J1-对象配平", ok, &det);

    // J1-15 结构画像与放行票据互证：structurally_ok 与 validate Ok 同真
    let mut agree = true;
    let pages = [1u16, 3u16, 42u16];
    let mut i = 0usize;
    while i < pages.len() {
        let p = match pages.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let mut sp = PdfSpec::default();
        sp.pages = p;
        let d = build_fixture(&sp);
        match inspect(&d) {
            Ok(r) => {
                if !r.structurally_ok() || validate(&d).is_err() {
                    agree = false;
                }
            }
            Err(_) => agree = false,
        }
        i += 1;
    }
    s.add("50102-J1-画像互证", agree, if agree { "" } else { "画像与票据不一致" });

    // J1-16 注入核 30 例全中
    let (caught, total) = injection_core();
    let ok = caught == total && total == 30;
    let det = format!("注入 {}/{} {}", caught, total, injection_mismatch_detail());
    s.add_dyn("50102-J1-注入30", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50102_checks_green() {
        let set = run_unx50102_checks();
        assert!(set.all_passed(), "UNX-F50102 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50102_valid_roundtrip() {
        let d = build_fixture(&PdfSpec::default());
        let t = validate(&d);
        assert!(t.is_ok());
        if let Ok(v) = t {
            assert_eq!(v.pages, 3);
            assert_eq!(v.xref_entries, v.objects + 1);
            assert!(v.digest != 0);
        }
    }

    #[test]
    fn unx50102_encrypted_evidence() {
        let mut sp = PdfSpec::default();
        sp.encrypt = Some(EncSpec {
            v: 4,
            r: 4,
            filter: Some("Standard"),
            ref_text: None,
        });
        let d = build_fixture(&sp);
        let r = validate(&d).err();
        assert!(r.is_some());
        if let Some(x) = r {
            assert_eq!(x.error, PdfError::Encrypted);
            assert!(x.warning().contains("/Encrypt"));
            assert!(x.alert.is_some());
        }
    }

    #[test]
    fn unx50102_injection_30() {
        let (caught, total) = injection_core();
        assert_eq!(total, 30);
        assert_eq!(caught, total);
    }
}
