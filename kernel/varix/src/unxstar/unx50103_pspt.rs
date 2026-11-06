//! UNX-F50103 PS 直通判定器与版本档（PS2/PS3 探测）（0x43900 段）
//!
//! 锚点：UNX-F50103-J1 版本档 ×2 探测 ×20 准确；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43900`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 12`，
//! 逐项见 [`PsError`]。
//!
//! PostScript 直通（Send as PS）比 PDF 更依赖 DSC 注释：设备靠
//! `%!PS-Adobe-x.y`、`%%LanguageLevel:`、`%%Pages:`、`%%BoundingBox:` 这些
//! 行决定解释器档位与纸张。判定的难点不在「是不是 PS」，而在**版本档**：
//! * PS2 档（LanguageLevel 2）不吃 `%%BeginBinary` 的二进制段，必须回退；
//! * PS3 档（LanguageLevel 3）允许二进制，且能向下兼容 Level 2 的活件；
//! * 头声明版本与 `%%LanguageLevel` 冲突（LL3 挂在 2.0 头下）说明文件被
//!   反复转存过，直通给任何一档都可能半途报错，显式拒绝。
//!
//! 结构清单：
//! * [`PsLevel`] / [`Tier`] / [`TierPolicy`]：版本档与两档能力表；
//! * [`DscHeader`] / [`BBox`]：DSC 扫描结果（解析器）；
//! * [`DecisionLedger`] / [`DecisionRecord`] / [`Decider`]：判定账本 + 注册器；
//! * [`Passport`] / [`Rejection`]：放行票据与带行号的拒绝三要素；
//! * [`probe_accuracy`]：2 档 × 20 样本探测核；
//! * [`injection_core`]：12 类坏样本注入核。
//!
//! 错误纪律：所有行扫描先取 `get` 再切片，行号随诊断一起上报；内核路径
//! 无 panic、无 `std`，浮点不参与判定（PS 档位是纯整数语义）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43900 独占 0x20）。
pub const SEG_BASE: u32 = 0x43900;

const E_EMPTY: u16 = 1;
const E_SHORT: u16 = 2;
const E_MAGIC: u16 = 3;
const E_VERSION: u16 = 4;
const E_LEVEL: u16 = 5;
const E_CONFLICT: u16 = 6;
const E_BBOX: u16 = 7;
const E_PAGES: u16 = 8;
const E_BINARY: u16 = 9;
const E_EOF: u16 = 10;
const E_EPS: u16 = 11;
const E_CAPACITY: u16 = 12;

/// PS 直通诊断。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsError {
    /// 输入为空。
    Empty,
    /// 短于 `%!`（2 字节）。
    Short,
    /// 不是 `%!` 开头。
    Magic,
    /// `%!PS-Adobe-x.y` 的版本号不在 1..=3。
    Version,
    /// `%%LanguageLevel` 不是 1..=3。
    Level,
    /// 头版本与 LanguageLevel 冲突（LL 高于头声明）。
    Conflict,
    /// BoundingBox 缺字段或四至倒置。
    BBox,
    /// 页数缺失、为 0、`atend` 或超过档位上限。
    Pages,
    /// 档位不支持二进制段。
    Binary,
    /// 文件尾 2KiB 内没有 `%%EOF`。
    Eof,
    /// EPS 缺 BoundingBox（裁切框是 EPS 的硬前提）。
    Eps,
    /// 判定账本已满。
    Capacity,
}

impl PsError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PsError::Empty => E_EMPTY,
            PsError::Short => E_SHORT,
            PsError::Magic => E_MAGIC,
            PsError::Version => E_VERSION,
            PsError::Level => E_LEVEL,
            PsError::Conflict => E_CONFLICT,
            PsError::BBox => E_BBOX,
            PsError::Pages => E_PAGES,
            PsError::Binary => E_BINARY,
            PsError::Eof => E_EOF,
            PsError::Eps => E_EPS,
            PsError::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PsError::Empty => "empty postscript input",
            PsError::Short => "shorter than %!",
            PsError::Magic => "%! magic missing",
            PsError::Version => "dsc header version unsupported",
            PsError::Level => "language level outside 1..=3",
            PsError::Conflict => "language level above header version",
            PsError::BBox => "bounding box malformed",
            PsError::Pages => "page count unusable for tier",
            PsError::Binary => "tier does not accept binary sections",
            PsError::Eof => "%%EOF missing near end of file",
            PsError::Eps => "eps without bounding box",
            PsError::Capacity => "decision ledger full",
        }
    }
}

/// 拒绝记录：诊断 + 行号 + 指令名（三要素，便于在 DSC 里定位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// 具体诊断。
    pub error: PsError,
    /// 行号（1 起；0 表示文件级判定）。
    pub line: u32,
    /// 触发指令（如 `%%LanguageLevel`）。
    pub directive: String,
}

impl Rejection {
    /// 构造拒绝记录。
    pub fn new(error: PsError, line: u32, directive: &str) -> Rejection {
        Rejection {
            error,
            line,
            directive: directive.to_string(),
        }
    }

    /// 三要素告警文本：行号 + 指令 + 诊断码/消息。
    pub fn warning(&self) -> String {
        format!(
            "ps check failed: line={} directive={} code=0x{:x} msg={}",
            self.line,
            self.directive,
            self.error.code(),
            self.error.message()
        )
    }
}

/// PostScript 语言档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PsLevel {
    /// LanguageLevel 1。
    One,
    /// LanguageLevel 2。
    Two,
    /// LanguageLevel 3。
    Three,
}

impl PsLevel {
    /// 从数字解析；0 与 4.. 都是伪造/损坏。
    pub fn from_number(n: u8) -> Result<PsLevel, PsError> {
        match n {
            1 => Ok(PsLevel::One),
            2 => Ok(PsLevel::Two),
            3 => Ok(PsLevel::Three),
            _ => Err(PsError::Level),
        }
    }

    /// 数字形式。
    pub fn number(self) -> u8 {
        match self {
            PsLevel::One => 1,
            PsLevel::Two => 2,
            PsLevel::Three => 3,
        }
    }

    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            PsLevel::One => "LL1",
            PsLevel::Two => "LL2",
            PsLevel::Three => "LL3",
        }
    }

    /// 头注释版本（`%!PS-Adobe-x.y` 的 x）。
    pub fn header_version(self) -> u8 {
        self.number()
    }
}

/// 直通输出档：PS2 打印机 / PS3 打印机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// PS2 档。
    Ps2,
    /// PS3 档。
    Ps3,
}

impl Tier {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            Tier::Ps2 => "PS2",
            Tier::Ps3 => "PS3",
        }
    }

    /// 该档默认策略。
    pub fn policy(self) -> TierPolicy {
        match self {
            Tier::Ps2 => TierPolicy::ps2(),
            Tier::Ps3 => TierPolicy::ps3(),
        }
    }
}

/// 档位能力表。数字来自 DSC 3.0 与常见 RIP 实测：
/// PS2 不支持 `%%BeginBinary`；PS3 支持且页数上限更高。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierPolicy {
    /// 输出档。
    pub tier: Tier,
    /// 可接受的最低语言档。
    pub min_level: PsLevel,
    /// 可接受的最高语言档。
    pub max_level: PsLevel,
    /// 是否接受二进制段。
    pub allow_binary: bool,
    /// 页数上限。
    pub max_pages: u32,
    /// EPS 是否强制要求 BoundingBox。
    pub require_bbox_for_eps: bool,
}

impl TierPolicy {
    /// PS2 档：只吃 Level 2 及以下的文本流，上限 999 页。
    pub fn ps2() -> TierPolicy {
        TierPolicy {
            tier: Tier::Ps2,
            min_level: PsLevel::Two,
            max_level: PsLevel::Two,
            allow_binary: false,
            max_pages: 999,
            require_bbox_for_eps: true,
        }
    }

    /// PS3 档：向下兼容 Level 2，允许二进制，上限 5000 页。
    pub fn ps3() -> TierPolicy {
        TierPolicy {
            tier: Tier::Ps3,
            min_level: PsLevel::Two,
            max_level: PsLevel::Three,
            allow_binary: true,
            max_pages: 5000,
            require_bbox_for_eps: true,
        }
    }

    /// 档位是否接受该语言档（纯表查询，判据里作为单调性基准）。
    pub fn accepts_level(&self, l: PsLevel) -> bool {
        l >= self.min_level && l <= self.max_level
    }
}

/// 四至框（1/72 英寸，DSC 约定 llx lly urx ury）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BBox {
    /// 左下 x。
    pub llx: i32,
    /// 左下 y。
    pub lly: i32,
    /// 右上 x。
    pub urx: i32,
    /// 右上 y。
    pub ury: i32,
}

impl BBox {
    /// 构造。
    pub const fn new(llx: i32, lly: i32, urx: i32, ury: i32) -> BBox {
        BBox { llx, lly, urx, ury }
    }

    /// 四至是否非退化。
    pub fn valid(&self) -> bool {
        self.urx > self.llx && self.ury > self.lly
    }

    /// 宽。
    pub fn width(&self) -> i32 {
        self.urx.saturating_sub(self.llx)
    }

    /// 高。
    pub fn height(&self) -> i32 {
        self.ury.saturating_sub(self.lly)
    }

    /// 面积（用于日志与零面积判断）。
    pub fn area(&self) -> i64 {
        self.width() as i64 * self.height() as i64
    }

    /// `llx lly urx ury` 文本。
    pub fn label(&self) -> String {
        format!("{} {} {} {}", self.llx, self.lly, self.urx, self.ury)
    }
}

/// DSC 头扫描结果。字段刻意保留「原样」而不是折算后的结果，
/// 便于判定器按档位策略二次解释（同一文件在不同档位结论可以不同）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DscHeader {
    /// 生效语言档（`%%LanguageLevel` 与头版本的较小者）。
    pub level: PsLevel,
    /// 头声明版本。
    pub version: u8,
    /// `%%LanguageLevel` 原值。
    pub language_level: Option<u8>,
    /// 创作者（截断 64 字节）。
    pub creator: String,
    /// 四至框。
    pub bounding_box: Option<BBox>,
    /// 页数（`atend`/缺失为 `None`）。
    pub pages: Option<u32>,
    /// 是否 EPSF。
    pub eps: bool,
    /// 是否含二进制段标记。
    pub binary: bool,
    /// 是否见到 `%%EndComments`。
    pub end_comments: bool,
    /// 扫描到的注释行数。
    pub comment_lines: u32,
    /// 头扫描消耗的字节数。
    pub header_bytes: usize,
}

impl DscHeader {
    /// 一行摘要（写回软件面用，必须非空）。
    pub fn summary(&self) -> String {
        let pages = match self.pages {
            Some(n) => format!("{}", n),
            None => "atend".to_string(),
        };
        let bbox = match &self.bounding_box {
            Some(b) => b.label(),
            None => "none".to_string(),
        };
        format!(
            "ps v{} level={} pages={} eps={} bin={} bbox={}",
            self.version,
            self.level.label(),
            pages,
            self.eps,
            self.binary,
            bbox
        )
    }
}

/// 放行票据：判定器产出的不可变结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Passport {
    /// 目标档。
    pub tier: Tier,
    /// 生效语言档。
    pub level: PsLevel,
    /// 页数。
    pub pages: u32,
    /// 是否二进制。
    pub binary: bool,
    /// 四至框。
    pub bbox: Option<BBox>,
    /// 头内容指纹（判定确定性对账）。
    pub digest: u32,
}

impl Passport {
    /// `PS3/LL3 pages=12 bin=true` 形式摘要。
    pub fn summary(&self) -> String {
        format!(
            "{}/{} pages={} bin={} bbox={}",
            self.tier.label(),
            self.level.label(),
            self.pages,
            self.binary,
            match &self.bbox {
                Some(b) => b.label(),
                None => "none".to_string(),
            }
        )
    }
}

// ---------------------------------------------------------------------------
// DSC 解析
// ---------------------------------------------------------------------------

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

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0c || b == 0
}

fn skip_ws(d: &[u8], mut at: usize) -> usize {
    while at < d.len() && is_ws(d[at]) {
        at += 1;
    }
    at
}

/// 一行：返回 (行尾, 下一行起点)。
fn next_line(d: &[u8], start: usize) -> Option<(usize, usize)> {
    if start >= d.len() {
        return None;
    }
    let mut i = start;
    while i < d.len() && d[i] != b'\n' && d[i] != b'\r' {
        i += 1;
    }
    let end = i;
    let mut n = i;
    if n < d.len() && d[n] == b'\r' {
        n += 1;
    }
    if n < d.len() && d[n] == b'\n' {
        n += 1;
    }
    Some((end, n))
}

fn first_line(d: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < d.len() && d[i] != b'\n' && d[i] != b'\r' {
        i += 1;
    }
    match d.get(..i) {
        Some(v) => v,
        None => d,
    }
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

fn parse_i32(d: &[u8], at: usize) -> Option<(i32, usize)> {
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
    } else {
        let x = if neg { -v } else { v };
        if x > i32::MAX as i64 || x < i32::MIN as i64 {
            None
        } else {
            Some((x as i32, i))
        }
    }
}

/// 解析 `%%BoundingBox:` 行（`(atend)` 视为缺省）。
fn parse_bbox(line: &[u8]) -> Result<Option<BBox>, PsError> {
    // "%%BoundingBox:" 共 14 字节，之后才是 llx。
    let mut at = skip_ws(line, 14);
    if at_is(line, at, b"(atend)") {
        return Ok(None);
    }
    let mut v = [0i32; 4];
    let mut i = 0usize;
    while i < 4 {
        at = skip_ws(line, at);
        let (x, next) = match parse_i32(line, at) {
            Some(t) => t,
            None => return Err(PsError::BBox),
        };
        if let Some(slot) = v.get_mut(i) {
            *slot = x;
        }
        at = next;
        i += 1;
    }
    // 行尾只允许空白，防止 "0 0 10 10 junk" 这类半截数据被当成合法。
    at = skip_ws(line, at);
    if at < line.len() {
        return Err(PsError::BBox);
    }
    let b = BBox::new(v[0], v[1], v[2], v[3]);
    if !b.valid() {
        return Err(PsError::BBox);
    }
    Ok(Some(b))
}

fn parse_pages(line: &[u8]) -> Result<Option<u32>, PsError> {
    let at = skip_ws(line, 8);
    if at_is(line, at, b"atend") {
        return Ok(None);
    }
    match parse_u32(line, at) {
        Some((v, next)) => {
            let tail = skip_ws(line, next);
            if tail < line.len() {
                return Err(PsError::Pages);
            }
            Ok(Some(v))
        }
        None => Err(PsError::Pages),
    }
}

fn parse_language(line: &[u8]) -> Result<u8, PsError> {
    let at = skip_ws(line, 16);
    match parse_u32(line, at) {
        Some((v, next)) => {
            let tail = skip_ws(line, next);
            if tail < line.len() || v == 0 || v > 3 {
                return Err(PsError::Level);
            }
            Ok(v as u8)
        }
        None => Err(PsError::Level),
    }
}

fn take_text(line: &[u8], skip: usize) -> String {
    let mut s = String::new();
    let mut i = skip_ws(line, skip);
    let mut n = 0usize;
    while i < line.len() && n < 64 {
        if let Some(b) = line.get(i) {
            if *b < 0x80 {
                s.push(*b as char);
            } else {
                s.push('?');
            }
        }
        i += 1;
        n += 1;
    }
    s
}

fn head_digest(h: &DscHeader) -> u32 {
    let mut hsh: u32 = 0x811c_9dc5;
    let feed = |hsh: &mut u32, v: u32| {
        *hsh ^= v;
        *hsh = (*hsh).wrapping_mul(0x0100_0193);
    };
    feed(&mut hsh, h.level.number() as u32);
    feed(&mut hsh, h.version as u32);
    feed(&mut hsh, h.pages.unwrap_or(0));
    feed(&mut hsh, h.binary as u32);
    feed(&mut hsh, h.eps as u32);
    if let Some(b) = &h.bounding_box {
        feed(&mut hsh, b.llx as u32);
        feed(&mut hsh, b.lly as u32);
        feed(&mut hsh, b.urx as u32);
        feed(&mut hsh, b.ury as u32);
    }
    hsh
}

/// 扫描 DSC 头：只读注释到 `%%EndComments`，正文不再解析（直通不需要）。
pub fn scan_dsc(data: &[u8]) -> Result<DscHeader, Rejection> {
    if data.is_empty() {
        return Err(Rejection::new(PsError::Empty, 0, "file"));
    }
    if data.len() < 2 {
        return Err(Rejection::new(PsError::Short, 0, "%!"));
    }
    if !at_is(data, 0, b"%!") {
        return Err(Rejection::new(PsError::Magic, 0, "%!"));
    }
    let line = first_line(data);
    let eps = find_from(line, b"EPSF", 0).is_some();
    let mut version = 1u8;
    if at_is(line, 0, b"%!PS-Adobe-") {
        let v = match line.get(11) {
            Some(b @ b'1'..=b'3') => *b - b'0',
            _ => return Err(Rejection::new(PsError::Version, 1, "%!PS-Adobe-")),
        };
        version = v;
    }
    let mut header = DscHeader {
        level: PsLevel::from_number(version).map_err(|e| Rejection::new(e, 1, "%!PS-Adobe-"))?,
        version,
        language_level: None,
        creator: String::new(),
        bounding_box: None,
        pages: None,
        eps,
        binary: false,
        end_comments: false,
        comment_lines: 0,
        header_bytes: 0,
    };

    // 第一行之后逐行扫描；DSC 头不会超过 64KiB，超出即停（防病态输入）。
    let mut at = match next_line(data, 0) {
        Some((_, next)) => next,
        None => return Err(Rejection::new(PsError::Short, 1, "%!")),
    };
    let mut line_no = 1u32;
    while at < data.len() && line_no < 4096 {
        let (end, next) = match next_line(data, at) {
            Some(v) => v,
            None => break,
        };
        line_no = line_no.saturating_add(1);
        header.comment_lines = header.comment_lines.saturating_add(1);
        let l = match data.get(at..end) {
            Some(v) => v,
            None => break,
        };
        // 二进制探测：头部出现高位字节即视为二进制混流（文本 DSC 只有 ASCII）。
        let mut high = false;
        let mut k = 0usize;
        while k < l.len() {
            if let Some(b) = l.get(k) {
                if *b >= 0x80 {
                    high = true;
                }
            }
            k += 1;
        }
        if high {
            header.binary = true;
        }
        if at_is(l, 0, b"%%EndComments") {
            header.end_comments = true;
            header.header_bytes = next;
            break;
        }
        if at_is(l, 0, b"%%BeginBinary") {
            header.binary = true;
        } else if at_is(l, 0, b"%%LanguageLevel:") {
            let lv = parse_language(l).map_err(|e| Rejection::new(e, line_no, "%%LanguageLevel"))?;
            header.language_level = Some(lv);
        } else if at_is(l, 0, b"%%Pages:") {
            header.pages =
                parse_pages(l).map_err(|e| Rejection::new(e, line_no, "%%Pages"))?;
        } else if at_is(l, 0, b"%%BoundingBox:") {
            header.bounding_box =
                parse_bbox(l).map_err(|e| Rejection::new(e, line_no, "%%BoundingBox"))?;
        } else if at_is(l, 0, b"%%Creator:") {
            if header.creator.is_empty() {
                header.creator = take_text(l, 10);
            }
        }
        if end >= 65_536 {
            header.header_bytes = end;
            break;
        }
        at = next;
    }
    if at_is(data, at, b"%%EndComments") {
        header.end_comments = true;
    }
    if header.header_bytes == 0 {
        header.header_bytes = at.min(data.len());
    }

    // LanguageLevel 高于头版本 = 转存时注释没同步，直通必炸。
    if let Some(lv) = header.language_level {
        if lv > version {
            return Err(Rejection::new(PsError::Conflict, line_no, "%%LanguageLevel"));
        }
        header.level = match PsLevel::from_number(lv) {
            Ok(v) => v,
            Err(e) => return Err(Rejection::new(e, line_no, "%%LanguageLevel")),
        };
    }

    // %%EOF 必须在尾部 2KiB 内；缺失说明传输被截断。
    let eof_at = match rfind(data, b"%%EOF") {
        Some(p) => p,
        None => return Err(Rejection::new(PsError::Eof, line_no, "%%EOF")),
    };
    if eof_at.saturating_add(2048) < data.len() {
        return Err(Rejection::new(PsError::Eof, line_no, "%%EOF"));
    }

    Ok(header)
}

/// 判定入口：按档位策略把扫描画像折算成放行票据。
pub fn decide(h: &DscHeader, policy: &TierPolicy) -> Result<Passport, Rejection> {
    if !policy.accepts_level(h.level) {
        return Err(Rejection::new(PsError::Level, 1, "%!PS-Adobe-"));
    }
    if h.binary && !policy.allow_binary {
        return Err(Rejection::new(PsError::Binary, 1, "%%BeginBinary"));
    }
    if h.eps && h.bounding_box.is_none() && policy.require_bbox_for_eps {
        return Err(Rejection::new(PsError::Eps, 1, "%%BoundingBox"));
    }
    if let Some(b) = &h.bounding_box {
        if !b.valid() {
            return Err(Rejection::new(PsError::BBox, 1, "%%BoundingBox"));
        }
    }
    let pages = match h.pages {
        Some(n) if n > 0 && n <= policy.max_pages => n,
        _ => return Err(Rejection::new(PsError::Pages, 1, "%%Pages")),
    };
    Ok(Passport {
        tier: policy.tier,
        level: h.level,
        pages,
        binary: h.binary,
        bbox: h.bounding_box,
        digest: head_digest(h),
    })
}

// ---------------------------------------------------------------------------
// 判定账本与注册器
// ---------------------------------------------------------------------------

/// 一条判定记录（append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionRecord {
    /// 单调序号（1 起）。
    pub seq: u32,
    /// 生效语言档。
    pub level: PsLevel,
    /// 是否放行。
    pub accepted: bool,
    /// 诊断码（放行为 0）。
    pub code: u32,
    /// 内容指纹。
    pub digest: u32,
}

/// 定容判定账本：满即拒，不覆盖旧记录（审计面不允许丢证据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionLedger {
    /// 容量。
    pub capacity: usize,
    /// 记录。
    pub entries: Vec<DecisionRecord>,
}

impl DecisionLedger {
    /// 新建。
    pub fn new(capacity: usize) -> DecisionLedger {
        DecisionLedger {
            capacity,
            entries: Vec::new(),
        }
    }

    /// 追加；满账返回 `Capacity`。
    pub fn append(&mut self, r: DecisionRecord) -> Result<(), PsError> {
        if self.entries.len() >= self.capacity {
            return Err(PsError::Capacity);
        }
        self.entries.push(r);
        Ok(())
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 封账指纹：链式 FNV，对记录顺序敏感（重排必变）。
    pub fn seal(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.seq;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.level.number() as u32;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.code;
                h = h.wrapping_mul(0x0100_0193);
                h ^= e.digest;
                h = h.wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        h
    }

    /// 最近一条记录。
    pub fn last(&self) -> Option<&DecisionRecord> {
        let n = self.entries.len();
        if n == 0 {
            None
        } else {
            self.entries.get(n - 1)
        }
    }
}

/// 判定器：策略 + 账本。所有提交（不论成败）都落账，保证可复盘。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decider {
    /// 档位策略。
    pub policy: TierPolicy,
    /// 判定账本。
    pub ledger: DecisionLedger,
    /// 内部序号。
    pub seq: u32,
}

impl Decider {
    /// 按档位新建（账本容量 64，足以覆盖一次打印会话）。
    pub fn new(tier: Tier) -> Decider {
        Decider {
            policy: tier.policy(),
            ledger: DecisionLedger::new(64),
            seq: 0,
        }
    }

    /// 提交一份 PS 字节流：扫描 → 判定 → 落账。账本先于结果更新，
    /// 即使策略拒绝也留下拒绝证据。
    pub fn submit(&mut self, data: &[u8]) -> Result<Passport, Rejection> {
        let result = scan_dsc(data).and_then(|h| decide(&h, &self.policy));
        let (accepted, level, code, digest) = match &result {
            Ok(p) => (true, p.level, 0u32, p.digest),
            Err(r) => (false, PsLevel::One, r.error.code(), fnv1a(data)),
        };
        self.seq = self.seq.saturating_add(1);
        let rec = DecisionRecord {
            seq: self.seq,
            level,
            accepted,
            code,
            digest,
        };
        if let Err(e) = self.ledger.append(rec) {
            return Err(Rejection::new(e, 0, "ledger"));
        }
        result
    }
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

// ---------------------------------------------------------------------------
// 样本构造器（判据与探测共用）
// ---------------------------------------------------------------------------

/// PS 样本规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsSpec {
    /// `%!PS-Adobe-x.0` 的版本号。
    pub version: u8,
    /// `%%LanguageLevel` 覆盖。
    pub lang: Option<u8>,
    /// `%%Pages` 文本。
    pub pages: &'static str,
    /// 四至框。
    pub bbox: Option<(i32, i32, i32, i32)>,
    /// 是否 EPSF。
    pub eps: bool,
    /// 是否写 `%%BeginBinary`。
    pub binary: bool,
    /// 是否写 `%%EOF`。
    pub eof: bool,
}

impl Default for PsSpec {
    fn default() -> PsSpec {
        PsSpec {
            version: 2,
            lang: Some(2),
            pages: "1",
            bbox: Some((0, 0, 595, 842)),
            eps: false,
            binary: false,
            eof: true,
        }
    }
}

/// 构造最小 PS 文档；所有行都以 `\n` 结束，便于行号断言。
pub fn build_ps(spec: &PsSpec) -> Vec<u8> {
    let mut s = String::new();
    if spec.eps {
        s.push_str(&format!("%!PS-Adobe-{}.0 EPSF-{}.0\n", spec.version, spec.version));
    } else {
        s.push_str(&format!("%!PS-Adobe-{}.0\n", spec.version));
    }
    s.push_str("%%Creator: unx50103 probe\n");
    if let Some(l) = spec.lang {
        s.push_str(&format!("%%LanguageLevel: {}\n", l));
    }
    s.push_str(&format!("%%Pages: {}\n", spec.pages));
    if let Some(b) = spec.bbox {
        s.push_str(&format!("%%BoundingBox: {} {} {} {}\n", b.0, b.1, b.2, b.3));
    }
    if spec.binary {
        s.push_str("%%BeginBinary: 64\n");
    }
    s.push_str("%%EndComments\n");
    s.push_str("%%Page: 1 1\nshowpage\n");
    if spec.eof {
        s.push_str("%%EOF\n");
    }
    s.into_bytes()
}

// ---------------------------------------------------------------------------
// 探测核：2 档 × 20 样本
// ---------------------------------------------------------------------------

fn probe_cases() -> Vec<(Vec<u8>, PsLevel, Option<PsError>, Tier)> {
    let mut v: Vec<(Vec<u8>, PsLevel, Option<PsError>, Tier)> = Vec::new();

    // ---- PS2 档 10 例 ----
    v.push((build_ps(&PsSpec::default()), PsLevel::Two, None, Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.pages = "12";
    v.push((build_ps(&sp), PsLevel::Two, None, Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.bbox = Some((-10, -10, 10, 10));
    v.push((build_ps(&sp), PsLevel::Two, None, Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.binary = true;
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Binary), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.lang = Some(3);
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Conflict), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.pages = "0";
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Pages), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.bbox = Some((10, 10, 0, 0));
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::BBox), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.eps = true;
    sp.bbox = None;
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Eps), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.eof = false;
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Eof), Tier::Ps2));

    let mut sp = PsSpec::default();
    sp.pages = "atend";
    v.push((build_ps(&sp), PsLevel::Two, Some(PsError::Pages), Tier::Ps2));

    // ---- PS3 档 10 例 ----
    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    v.push((build_ps(&sp), PsLevel::Three, None, Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    sp.binary = true;
    v.push((build_ps(&sp), PsLevel::Three, None, Tier::Ps3));

    // 3.0 头 + LL2：向下兼容，生效档是 LL2
    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(2);
    v.push((build_ps(&sp), PsLevel::Two, None, Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    sp.eps = true;
    sp.bbox = Some((0, 0, 400, 400));
    v.push((build_ps(&sp), PsLevel::Three, None, Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    sp.pages = "2000";
    v.push((build_ps(&sp), PsLevel::Three, None, Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    sp.pages = "9999";
    v.push((build_ps(&sp), PsLevel::Three, Some(PsError::Pages), Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(9);
    v.push((build_ps(&sp), PsLevel::Three, Some(PsError::Level), Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    v.push((build_ps(&sp), PsLevel::Three, None, Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 4;
    sp.lang = Some(3);
    v.push((build_ps(&sp), PsLevel::Three, Some(PsError::Version), Tier::Ps3));

    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(3);
    sp.bbox = Some((5, 5, 5, 40));
    v.push((build_ps(&sp), PsLevel::Three, Some(PsError::BBox), Tier::Ps3));

    v
}

/// 2 档 × 20 样本探测：同时校验语言档与判定结果（含拒绝码）。
pub fn probe_accuracy() -> (usize, usize) {
    let cases = probe_cases();
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((data, want_level, want_code, tier)) = cases.get(i) {
            let scan = scan_dsc(data);
            let ok = match &scan {
                Ok(h) => {
                    h.level == *want_level
                        && match decide(h, &tier.policy()) {
                            Ok(_) => want_code.is_none(),
                            Err(r) => Some(r.error) == *want_code,
                        }
                }
                Err(r) => Some(r.error) == *want_code,
            };
            if ok {
                hits += 1;
            }
        }
        i += 1;
    }
    (hits, cases.len())
}

/// 探测错位详情（判据落红时定位用）。
fn probe_mismatch_detail() -> String {
    let cases = probe_cases();
    let mut out = String::new();
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((data, want_level, want_code, tier)) = cases.get(i) {
            let scan = scan_dsc(data);
            let ok = match &scan {
                Ok(h) => {
                    h.level == *want_level
                        && match decide(h, &tier.policy()) {
                            Ok(_) => want_code.is_none(),
                            Err(r) => Some(r.error) == *want_code,
                        }
                }
                Err(r) => Some(r.error) == *want_code,
            };
            if !ok {
                out.push_str(&format!("[{} want={:?}/{:?}] ", i, want_level, want_code));
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 注入核：12 类坏样本
// ---------------------------------------------------------------------------

/// 12 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut cases: Vec<(Vec<u8>, PsError)> = Vec::new();

    cases.push((Vec::new(), PsError::Empty));
    cases.push((b"%".to_vec(), PsError::Short));
    cases.push((b"hello world\n%%EOF\n".to_vec(), PsError::Magic));

    let mut sp = PsSpec::default();
    sp.version = 4;
    cases.push((build_ps(&sp), PsError::Version));

    let mut sp = PsSpec::default();
    sp.lang = Some(9);
    cases.push((build_ps(&sp), PsError::Level));

    let mut sp = PsSpec::default();
    sp.lang = Some(3);
    cases.push((build_ps(&sp), PsError::Conflict));

    let mut sp = PsSpec::default();
    sp.bbox = Some((20, 0, 10, 10));
    cases.push((build_ps(&sp), PsError::BBox));

    let mut sp = PsSpec::default();
    sp.eps = true;
    sp.bbox = None;
    cases.push((build_ps(&sp), PsError::Eps));

    let mut sp = PsSpec::default();
    sp.pages = "0";
    cases.push((build_ps(&sp), PsError::Pages));

    let mut sp = PsSpec::default();
    sp.binary = true;
    cases.push((build_ps(&sp), PsError::Binary));

    let mut sp = PsSpec::default();
    sp.eof = false;
    cases.push((build_ps(&sp), PsError::Eof));

    let mut sp = PsSpec::default();
    sp.pages = "atend";
    cases.push((build_ps(&sp), PsError::Pages));

    let pol2 = TierPolicy::ps2();
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((data, want)) = cases.get(i) {
            let got = match scan_dsc(data).and_then(|h| decide(&h, &pol2)) {
                Ok(_) => None,
                Err(r) => Some(r.error),
            };
            if got == Some(*want) {
                caught += 1;
            }
        }
        i += 1;
    }
    (caught, cases.len())
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50103 自检：16 条判据。
pub fn run_unx50103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50103");

    // J1-1 三档头版本识别：1.0/2.0/3.0
    let l1 = scan_dsc(&build_ps(&PsSpec {
        version: 1,
        lang: Some(1),
        ..PsSpec::default()
    }));
    let l2 = scan_dsc(&build_ps(&PsSpec::default()));
    let l3 = scan_dsc(&build_ps(&PsSpec {
        version: 3,
        lang: Some(3),
        ..PsSpec::default()
    }));
    let ok = match (&l1, &l2, &l3) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.level == PsLevel::One && b.level == PsLevel::Two && c.level == PsLevel::Three
        }
        _ => false,
    };
    s.add("50103-J1-三档识别", ok, if ok { "" } else { "档位识别错位" });

    // J1-2 纯 `%!` 视为 Level 1，且必须带 EOF
    let plain = b"%!PS\n0 0 moveto\nshowpage\n%%EOF\n".to_vec();
    let ok = match scan_dsc(&plain) {
        Ok(h) => h.level == PsLevel::One && h.version == 1 && h.end_comments == false,
        Err(_) => false,
    };
    s.add("50103-J1-纯PS", ok, if ok { "" } else { "纯 PS 头未识别" });

    // J1-3 LanguageLevel 向下覆盖：3.0 头 + LL2 → 生效 LL2
    let mut sp = PsSpec::default();
    sp.version = 3;
    sp.lang = Some(2);
    let ok = match scan_dsc(&build_ps(&sp)) {
        Ok(h) => h.level == PsLevel::Two && h.language_level == Some(2),
        Err(_) => false,
    };
    s.add("50103-J1-语言档下探", ok, if ok { "" } else { "LL2 未生效" });

    // J1-4 LL 高于头版本 → Conflict（不静默取大）
    let mut sp = PsSpec::default();
    sp.lang = Some(3);
    let ok = match scan_dsc(&build_ps(&sp)) {
        Err(r) => r.error == PsError::Conflict && r.line > 1,
        Ok(_) => false,
    };
    s.add("50103-J1-版本冲突", ok, if ok { "" } else { "冲突未报" });

    // J1-5 四至解析：负坐标合法，倒置归 BBox
    let mut sp = PsSpec::default();
    sp.bbox = Some((-100, -50, 100, 50));
    let good = scan_dsc(&build_ps(&sp));
    let mut bad = PsSpec::default();
    bad.bbox = Some((100, 50, -100, -50));
    let badr = scan_dsc(&build_ps(&bad));
    let ok = match (&good, &badr) {
        (Ok(h), Err(r)) => {
            h.bounding_box == Some(BBox::new(-100, -50, 100, 50))
                && r.error == PsError::BBox
        }
        _ => false,
    };
    s.add("50103-J1-四至解析", ok, if ok { "" } else { "BBox 判定错" });

    // J1-6 EPS 无 BoundingBox → Eps（裁切框是硬前提）
    let mut sp = PsSpec::default();
    sp.eps = true;
    sp.bbox = None;
    let h = scan_dsc(&build_ps(&sp));
    let ok = match &h {
        Ok(v) => v.eps && decide(v, &TierPolicy::ps3()).err().map(|r| r.error) == Some(PsError::Eps),
        Err(_) => false,
    };
    s.add("50103-J1-EPS裁切框", ok, if ok { "" } else { "EPS 缺框未拒" });

    // J1-7 页数解析：数字生效、atend 归 None（档位再拒）
    let mut sp = PsSpec::default();
    sp.pages = "42";
    let num = scan_dsc(&build_ps(&sp));
    let mut sp2 = PsSpec::default();
    sp2.pages = "atend";
    let end = scan_dsc(&build_ps(&sp2));
    let ok = match (&num, &end) {
        (Ok(a), Ok(b)) => a.pages == Some(42) && b.pages.is_none(),
        _ => false,
    };
    s.add("50103-J1-页数解析", ok, if ok { "" } else { "Pages 解析错" });

    // J1-8 二进制识别：标记行必须被看见
    let mut sp = PsSpec::default();
    sp.binary = true;
    let ok = match scan_dsc(&build_ps(&sp)) {
        Ok(h) => h.binary,
        Err(_) => false,
    };
    s.add("50103-J1-二进制识别", ok, if ok { "" } else { "二进制漏检" });

    // J1-9 两档能力差：同一 LL2 二进制件 PS2 拒、PS3 收
    let mut sp = PsSpec::default();
    sp.version = 2;
    sp.lang = Some(2);
    sp.binary = true;
    let data = build_ps(&sp);
    let ok = match scan_dsc(&data) {
        Ok(h) => {
            decide(&h, &TierPolicy::ps2()).err().map(|r| r.error) == Some(PsError::Binary)
                && decide(&h, &TierPolicy::ps3()).is_ok()
        }
        Err(_) => false,
    };
    s.add("50103-J1-两档能力差", ok, if ok { "" } else { "档位差失效" });

    // J1-10 档位表单调：PS3 的 level 区间与页数上限都不窄于 PS2
    let p2 = TierPolicy::ps2();
    let p3 = TierPolicy::ps3();
    let ok = p3.min_level <= p2.min_level
        && p3.max_level >= p2.max_level
        && p3.max_pages >= p2.max_pages
        && !p2.allow_binary
        && p3.allow_binary;
    let det = format!(
        "ps2={}..{} p3={}..{} pages={}/{}",
        p2.min_level.label(),
        p2.max_level.label(),
        p3.min_level.label(),
        p3.max_level.label(),
        p2.max_pages,
        p3.max_pages
    );
    s.add_dyn("50103-J1-档位单调", ok, &det);

    // J1-11 判定确定性：同头两次 decide 票据逐字段一致
    let data = build_ps(&PsSpec::default());
    let ok = match scan_dsc(&data) {
        Ok(h) => {
            let a = decide(&h, &TierPolicy::ps2());
            let b = decide(&h, &TierPolicy::ps2());
            match (a, b) {
                (Ok(x), Ok(y)) => x == y && x.digest != 0,
                _ => false,
            }
        }
        Err(_) => false,
    };
    s.add("50103-J1-判定确定", ok, if ok { "" } else { "判定非确定" });

    // J1-12 账本：序号单调、成功/失败都留痕、封账指纹随记录变化
    let mut d = Decider::new(Tier::Ps2);
    let good = build_ps(&PsSpec::default());
    let mut bad = PsSpec::default();
    bad.binary = true;
    let bad_data = build_ps(&bad);
    let r1 = d.submit(&good);
    let r2 = d.submit(&bad_data);
    let seal_a = d.ledger.seal();
    let r3 = d.submit(&good);
    let seal_b = d.ledger.seal();
    let seq_ok = match (d.ledger.entries.get(0), d.ledger.entries.get(1), d.ledger.entries.get(2)) {
        (Some(a), Some(b), Some(c)) => a.seq == 1 && b.seq == 2 && c.seq == 3,
        _ => false,
    };
    let ok = r1.is_ok()
        && r2.is_err()
        && r3.is_ok()
        && seq_ok
        && d.ledger.len() == 3
        && seal_a != seal_b
        && d.ledger.last().map(|e| e.accepted) == Some(true);
    let det = format!("len={} seal={:x}/{:x}", d.ledger.len(), seal_a, seal_b);
    s.add_dyn("50103-J1-判定留痕", ok, &det);

    // J1-13 账本满账显式 Capacity：容量 1 时第二条被拒
    let mut d = Decider::new(Tier::Ps2);
    d.ledger.capacity = 1;
    let _ = d.submit(&good);
    let second = d.submit(&good);
    let ok = second.err().map(|r| r.error) == Some(PsError::Capacity);
    s.add("50103-J1-满账显式", ok, if ok { "" } else { "满账未拒" });

    // J1-14 三要素告警：行号 + 指令 + 码值全部在场
    let mut sp = PsSpec::default();
    sp.lang = Some(3);
    let rej = scan_dsc(&build_ps(&sp)).err();
    let warn = match &rej {
        Some(r) => r.warning(),
        None => String::new(),
    };
    let ok = warn.contains("line=")
        && warn.contains("directive=%%LanguageLevel")
        && warn.contains("code=0x");
    s.add_dyn("50103-J1-三要素", ok, &warn);

    // J1-15 摘要非空（软件面写回）
    let h = scan_dsc(&build_ps(&PsSpec::default()));
    let ok = match &h {
        Ok(v) => {
            let sum = v.summary();
            !sum.is_empty() && sum.contains("level=LL2") && sum.contains("pages=1")
        }
        Err(_) => false,
    };
    s.add("50103-J1-摘要非空", ok, if ok { "" } else { "摘要空或缺字段" });

    // J1-16 2 档 × 20 探测全中
    let (hits, total) = probe_accuracy();
    let ok = hits == total && total == 20;
    let det = format!("探测 {}/{} {}", hits, total, probe_mismatch_detail());
    s.add_dyn("50103-J1-探测20", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50103_checks_green() {
        let set = run_unx50103_checks();
        assert!(set.all_passed(), "UNX-F50103 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50103_probe_20() {
        let (hits, total) = probe_accuracy();
        assert_eq!(total, 20);
        assert_eq!(hits, total);
    }

    #[test]
    fn unx50103_injection_12() {
        let (caught, total) = injection_core();
        assert_eq!(total, 12);
        assert_eq!(caught, total);
    }

    #[test]
    fn unx50103_ledger_seal_stable() {
        let mut d = Decider::new(Tier::Ps3);
        let data = build_ps(&PsSpec::default());
        assert!(d.submit(&data).is_ok());
        let a = d.ledger.seal();
        let b = d.ledger.seal();
        assert_eq!(a, b);
    }
}
