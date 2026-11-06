//! UNX-F50109 直通失败码族（10 条：加密/损坏/不支持等）（0x439c0 段）
//!
//! 锚点：UNX-F50109-J1 错误码 ×10 零漂移；判据锚定 §AI-63 §2 专题二。
//!
//! 直通面只对**入件结构**做判定，不做内容修复：码族把「为什么不能直通」切成
//! 10 个互斥、可复算的类别，每类绑定一个固定的段内偏移与一条恢复策略。
//! 「零漂移」= 数值、符号、类别、策略四张表逐行对齐，任何一行改动都会被
//! 判据当场抓住。
//!
//! 数据段：`SEG_BASE = 0x439c0`；诊断码闭集 = `SEG_BASE + 1..=10`
//! （10 个码全部由 `injection_core` 真实构造）。
//!
//! 结构清单：
//! * `PtFail` —— 10 条失败码（唯一错误枚举，`code()` = SEG_BASE + n）；
//! * `FaultRow` + `FAULT_TABLE` —— 符号/类别/重试位/恢复策略注册表；
//! * `diagnose` —— 按**固定优先级**扫描入件头的判定器（PDF/PS 双格式）；
//! * `FaultLedger` —— 定容码值分布账（计数 + 出现序 + 溢出记账）；
//! * `injection_core` —— 10 类坏入件逐类断码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（本模块独占 0x439c0）。
pub const SEG_BASE: u32 = 0x439c0;

/// 单件上限（1 MiB）：超过即判资源类超限，绝不整载后再失败。
pub const MAX_BYTES: usize = 1024 * 1024;

/// 直通失败码族（10 条闭集）。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtFail {
    /// 空入件。
    Empty,
    /// 加密（`/Encrypt` 存在）：直通必须原样交设备，无权解密。
    Encrypted,
    /// 结构损坏（缺 `%%EOF`）。
    Damaged,
    /// 格式不在直通白名单（非 PDF/PS）。
    Unsupported,
    /// 截断（短于最小头，或 `/Length` 超过实际字节）。
    Truncated,
    /// 交叉引用表缺失（PDF 缺 `startxref`）。
    XrefBroken,
    /// 版本超直通白名单（PDF > 2.x / PS > 3.x）。
    VersionBeyond,
    /// 字体内嵌缺失（有 `/BaseFont` 无 `/FontFile`）。
    FontUnembedded,
    /// 超过单件上限。
    Oversize,
    /// 媒体尺寸与票据目标不符。
    MediaMismatch,
}

impl PtFail {
    /// 段内偏移（1..=10），同时是注册表行号 + 1。
    pub fn n(self) -> u16 {
        match self {
            PtFail::Empty => 1,
            PtFail::Encrypted => 2,
            PtFail::Damaged => 3,
            PtFail::Unsupported => 4,
            PtFail::Truncated => 5,
            PtFail::XrefBroken => 6,
            PtFail::VersionBeyond => 7,
            PtFail::FontUnembedded => 8,
            PtFail::Oversize => 9,
            PtFail::MediaMismatch => 10,
        }
    }

    /// 全仓唯一诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.n() as u32
    }

    /// 账本下标（0..=9）。
    pub fn index(self) -> usize {
        (self.n() - 1) as usize
    }

    /// 由账本下标反解。
    pub fn from_index(i: usize) -> Option<PtFail> {
        match i {
            0 => Some(PtFail::Empty),
            1 => Some(PtFail::Encrypted),
            2 => Some(PtFail::Damaged),
            3 => Some(PtFail::Unsupported),
            4 => Some(PtFail::Truncated),
            5 => Some(PtFail::XrefBroken),
            6 => Some(PtFail::VersionBeyond),
            7 => Some(PtFail::FontUnembedded),
            8 => Some(PtFail::Oversize),
            9 => Some(PtFail::MediaMismatch),
            _ => None,
        }
    }

    /// 码值反解（零漂移判据）。
    pub fn from_code(code: u32) -> Option<PtFail> {
        if code <= SEG_BASE {
            return None;
        }
        PtFail::from_index((code - SEG_BASE - 1) as usize)
    }

    pub fn message(self) -> &'static str {
        match self {
            PtFail::Empty => "input is empty",
            PtFail::Encrypted => "document is encrypted",
            PtFail::Damaged => "document structure damaged",
            PtFail::Unsupported => "format not in passthrough whitelist",
            PtFail::Truncated => "document truncated",
            PtFail::XrefBroken => "cross-reference table missing",
            PtFail::VersionBeyond => "format version beyond whitelist",
            PtFail::FontUnembedded => "font not embedded",
            PtFail::Oversize => "input exceeds per-job limit",
            PtFail::MediaMismatch => "media box mismatches ticket",
        }
    }
}

/// 失败类别：决定谁能重试、谁只能回退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultClass {
    Input,
    Structure,
    Policy,
    Resource,
}

impl FaultClass {
    pub fn code(self) -> u8 {
        match self {
            FaultClass::Input => 0,
            FaultClass::Structure => 1,
            FaultClass::Policy => 2,
            FaultClass::Resource => 3,
        }
    }
}

/// 恢复策略（直通失败后的下一步，显式登记不靠调用方猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// 回退光栅化链（F50104 的入口）。
    Fallback,
    /// 重排队（入件本身可修复，例如重传/重切）。
    Requeue,
    /// 终态拒绝（重试无意义）。
    Abort,
}

impl Recovery {
    pub fn code(self) -> u8 {
        match self {
            Recovery::Fallback => 0,
            Recovery::Requeue => 1,
            Recovery::Abort => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Recovery::Fallback => "fallback",
            Recovery::Requeue => "requeue",
            Recovery::Abort => "abort",
        }
    }
}

/// 注册表一行：码 → 符号/类别/重试位/策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRow {
    pub kind: PtFail,
    pub symbol: &'static str,
    pub class: FaultClass,
    pub retryable: bool,
    pub recovery: Recovery,
}

/// 冻结注册表。行序 = 码序（第 i 行的码必须是 SEG_BASE + i + 1）。
pub const FAULT_TABLE: [FaultRow; 10] = [
    FaultRow {
        kind: PtFail::Empty,
        symbol: "PT_EMPTY",
        class: FaultClass::Input,
        retryable: false,
        recovery: Recovery::Abort,
    },
    FaultRow {
        kind: PtFail::Encrypted,
        symbol: "PT_ENCRYPTED",
        class: FaultClass::Policy,
        retryable: false,
        recovery: Recovery::Abort,
    },
    FaultRow {
        kind: PtFail::Damaged,
        symbol: "PT_DAMAGED",
        class: FaultClass::Structure,
        retryable: false,
        recovery: Recovery::Fallback,
    },
    FaultRow {
        kind: PtFail::Unsupported,
        symbol: "PT_UNSUPPORTED",
        class: FaultClass::Policy,
        retryable: false,
        recovery: Recovery::Abort,
    },
    FaultRow {
        kind: PtFail::Truncated,
        symbol: "PT_TRUNCATED",
        class: FaultClass::Input,
        retryable: true,
        recovery: Recovery::Requeue,
    },
    FaultRow {
        kind: PtFail::XrefBroken,
        symbol: "PT_XREF_BROKEN",
        class: FaultClass::Structure,
        retryable: false,
        recovery: Recovery::Fallback,
    },
    FaultRow {
        kind: PtFail::VersionBeyond,
        symbol: "PT_VERSION_BEYOND",
        class: FaultClass::Policy,
        retryable: false,
        recovery: Recovery::Abort,
    },
    FaultRow {
        kind: PtFail::FontUnembedded,
        symbol: "PT_FONT_UNEMBEDDED",
        class: FaultClass::Structure,
        retryable: false,
        recovery: Recovery::Fallback,
    },
    FaultRow {
        kind: PtFail::Oversize,
        symbol: "PT_OVERSIZE",
        class: FaultClass::Resource,
        retryable: true,
        recovery: Recovery::Requeue,
    },
    FaultRow {
        kind: PtFail::MediaMismatch,
        symbol: "PT_MEDIA_MISMATCH",
        class: FaultClass::Input,
        retryable: false,
        recovery: Recovery::Fallback,
    },
];

/// 按码查注册表行。
pub fn row_of(kind: PtFail) -> Option<FaultRow> {
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        if let Some(r) = FAULT_TABLE.get(i) {
            if r.kind == kind {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 按符号查注册表行（日志/验收文本回查用）。
pub fn row_by_symbol(sym: &str) -> Option<FaultRow> {
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        if let Some(r) = FAULT_TABLE.get(i) {
            if r.symbol == sym {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 按全仓诊断码查注册表行。
pub fn row_by_code(code: u32) -> Option<FaultRow> {
    let kind = PtFail::from_code(code)?;
    row_of(kind)
}

// ---------------------------------------------------------------------------
// 入件判定器（固定优先级，单次线性扫描）
// ---------------------------------------------------------------------------

/// 直通白名单内的两种格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocFormat {
    Pdf,
    Ps,
}

impl DocFormat {
    pub fn code(self) -> u8 {
        match self {
            DocFormat::Pdf => 0,
            DocFormat::Ps => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DocFormat::Pdf => "pdf",
            DocFormat::Ps => "ps",
        }
    }
}

/// 票据目标媒体（只支持两种常用规格；尺寸单位 1/72 inch）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSize {
    A4,
    Letter,
}

impl MediaSize {
    pub fn dims(self) -> (usize, usize) {
        match self {
            MediaSize::A4 => (595, 842),
            MediaSize::Letter => (612, 792),
        }
    }
}

/// 判定通过的入件轮廓（直通层只带回这些字段，不驻留正文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobShape {
    pub format: DocFormat,
    pub version_major: u16,
    pub version_minor: u16,
    pub bytes: usize,
    pub media: Option<(usize, usize)>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        let mut k = 0usize;
        let mut hit = true;
        while k < needle.len() {
            let a = match hay.get(i + k) {
                Some(&v) => v,
                None => {
                    hit = false;
                    break;
                }
            };
            let b = match needle.get(k) {
                Some(&v) => v,
                None => {
                    hit = false;
                    break;
                }
            };
            if a != b {
                hit = false;
                break;
            }
            k += 1;
        }
        if hit {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | b'\x0c' | b'\x00')
}

fn read_version(doc: &[u8], at: usize) -> Option<(u16, u16)> {
    let a = *doc.get(at)?;
    let dot = *doc.get(at + 1)?;
    let b = *doc.get(at + 2)?;
    if a < b'0' || a > b'9' || dot != b'.' || b < b'0' || b > b'9' {
        return None;
    }
    Some(((a - b'0') as u16, (b - b'0') as u16))
}

/// 读一个十进制整数（饱和到 `usize::MAX`，绝不因超长数字溢出）。
fn read_uint(doc: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut i = at;
    while i < doc.len() {
        match doc.get(i) {
            Some(&c) if is_space(c) => i += 1,
            _ => break,
        }
    }
    let mut v = 0usize;
    let mut digits = 0usize;
    while i < doc.len() {
        let c = match doc.get(i) {
            Some(&c) => c,
            None => break,
        };
        if c < b'0' || c > b'9' {
            break;
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as usize);
        digits += 1;
        i += 1;
    }
    if digits == 0 {
        None
    } else {
        Some((v, i))
    }
}

fn declared_length(doc: &[u8]) -> Option<usize> {
    let at = find(doc, b"/Length")?;
    match read_uint(doc, at + 7) {
        Some((v, _)) => Some(v),
        None => None,
    }
}

fn media_box(doc: &[u8]) -> Option<(usize, usize)> {
    let at = find(doc, b"/MediaBox")?;
    let mut i = at + 9;
    while i < doc.len() {
        match doc.get(i) {
            Some(&b'[') => {
                i += 1;
                break;
            }
            Some(&c) if is_space(c) => i += 1,
            _ => return None,
        }
    }
    let mut nums = [0usize; 4];
    let mut n = 0usize;
    while n < 4 {
        let (v, next) = read_uint(doc, i)?;
        if let Some(slot) = nums.get_mut(n) {
            *slot = v;
        }
        i = next;
        n += 1;
    }
    let x1 = nums[0];
    let y1 = nums[1];
    let x2 = nums[2];
    let y2 = nums[3];
    let w = if x2 > x1 { x2 - x1 } else { x1 - x2 };
    let h = if y2 > y1 { y2 - y1 } else { y1 - y2 };
    Some((w, h))
}

/// 判定入件能否直通。
///
/// 优先级固定（先命中的码即结果，保证同输入多次判定逐位一致）：
/// 空 → 超限 → 短 → 头/版本 → 加密 → 交叉引用 → 结束标记 → 长度 → 字体 → 媒体。
/// 加密排在损坏之前：`/Encrypt` 存在时文档内容本就不可解析，报「损坏」会把
/// 用户引向错误的修复动作。
pub fn diagnose(doc: &[u8], target: MediaSize) -> Result<JobShape, PtFail> {
    if doc.is_empty() {
        return Err(PtFail::Empty);
    }
    if doc.len() > MAX_BYTES {
        return Err(PtFail::Oversize);
    }
    if doc.len() < 8 {
        return Err(PtFail::Truncated);
    }
    let (format, ver_at) = if find(doc, b"%PDF-") == Some(0) {
        (DocFormat::Pdf, 5usize)
    } else if find(doc, b"%!PS-Adobe-") == Some(0) {
        (DocFormat::Ps, 11usize)
    } else if find(doc, b"%!PS") == Some(0) {
        return Err(PtFail::Unsupported);
    } else {
        return Err(PtFail::Unsupported);
    };
    let (major, minor) = match read_version(doc, ver_at) {
        Some(v) => v,
        None => return Err(PtFail::Unsupported),
    };
    let beyond = match format {
        DocFormat::Pdf => major > 2,
        DocFormat::Ps => major != 3 && major != 2,
    };
    if beyond {
        return Err(PtFail::VersionBeyond);
    }
    if find(doc, b"/Encrypt").is_some() {
        return Err(PtFail::Encrypted);
    }
    if format == DocFormat::Pdf && find(doc, b"startxref").is_none() {
        return Err(PtFail::XrefBroken);
    }
    if find(doc, b"%%EOF").is_none() {
        return Err(PtFail::Damaged);
    }
    if let Some(len) = declared_length(doc) {
        if len > doc.len() {
            return Err(PtFail::Truncated);
        }
    }
    if find(doc, b"/BaseFont").is_some() && find(doc, b"/FontFile").is_none() {
        return Err(PtFail::FontUnembedded);
    }
    let media = media_box(doc);
    if let Some((w, h)) = media {
        let (tw, th) = target.dims();
        let upright = w == tw && h == th;
        let rotated = w == th && h == tw;
        if !upright && !rotated {
            return Err(PtFail::MediaMismatch);
        }
    }
    Ok(JobShape {
        format,
        version_major: major,
        version_minor: minor,
        bytes: doc.len(),
        media,
    })
}

// ---------------------------------------------------------------------------
// 码值分布账
// ---------------------------------------------------------------------------

/// 定容失败账：每码计数 + 出现序 + 溢出显式记账。
pub struct FaultLedger {
    counts: [u32; 10],
    order: Vec<u16>,
    cap: usize,
    total: u32,
    accepted: u32,
    overflow: u32,
}

impl FaultLedger {
    /// `cap` 只约束出现序长度（诊断计数恒不丢）。
    pub fn new(cap: usize) -> FaultLedger {
        FaultLedger {
            counts: [0u32; 10],
            order: Vec::new(),
            cap,
            total: 0,
            accepted: 0,
            overflow: 0,
        }
    }

    pub fn count(&self, f: PtFail) -> u32 {
        match self.counts.get(f.index()) {
            Some(&c) => c,
            None => 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn overflow(&self) -> u32 {
        self.overflow
    }

    pub fn order(&self) -> &[u16] {
        &self.order
    }

    /// 出现过的不同码个数（×10 覆盖度）。
    pub fn distinct(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(&c) = self.counts.get(i) {
                if c > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn distribution(&self) -> [u32; 10] {
        self.counts
    }

    /// 记一次失败：计数恒加，出现序按容量决定是否记账。
    pub fn record(&mut self, f: PtFail) {
        self.total = self.total.saturating_add(1);
        let idx = f.index();
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        if self.order.len() < self.cap {
            self.order.push(f.n());
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// 扫一批入件：通过的计 accepted，失败的按码入账。返回通过数。
    pub fn sweep(&mut self, docs: &[&[u8]], target: MediaSize) -> usize {
        let mut ok = 0usize;
        let mut i = 0usize;
        while i < docs.len() {
            if let Some(d) = docs.get(i) {
                match diagnose(d, target) {
                    Ok(_) => {
                        self.accepted = self.accepted.saturating_add(1);
                        self.total = self.total.saturating_add(1);
                        ok += 1;
                    }
                    Err(f) => self.record(f),
                }
            }
            i += 1;
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// 注入核：10 类坏入件，逐类断言具体码值
// ---------------------------------------------------------------------------

fn oversize_doc() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(MAX_BYTES + 1);
    v.resize(MAX_BYTES + 1, b'x');
    let head = b"%PDF-1.7";
    let mut i = 0usize;
    while i < head.len() {
        if let Some(slot) = v.get_mut(i) {
            *slot = head[i];
        }
        i += 1;
    }
    v
}

type BadSample = (&'static [u8], PtFail);

/// 9 类静态坏入件（超限件由 `oversize_doc()` 单独构造，避免 1 MiB 常量）。
fn bad_samples() -> Vec<BadSample> {
    let mut v: Vec<BadSample> = Vec::new();
    v.push((&b""[..], PtFail::Empty));
    v.push((
        &b"%PDF-1.7\n/Encrypt 4 0 R\nstartxref\n0\n%%EOF\n"[..],
        PtFail::Encrypted,
    ));
    v.push((&b"%PDF-1.7\nstartxref\n0\n"[..], PtFail::Damaged));
    v.push((&b"GIF89a-not-a-print-file"[..], PtFail::Unsupported));
    v.push((&b"%PD"[..], PtFail::Truncated));
    v.push((
        &b"%PDF-1.7\nno xref table\n%%EOF\n"[..],
        PtFail::XrefBroken,
    ));
    v.push((&b"%PDF-3.0\nstartxref\n0\n%%EOF\n"[..], PtFail::VersionBeyond));
    v.push((
        &b"%PDF-1.7\n/BaseFont /Helvetica\nstartxref\n0\n%%EOF\n"[..],
        PtFail::FontUnembedded,
    ));
    v.push((
        &b"%PDF-1.7\n/MediaBox [0 0 612 792]\nstartxref\n0\n%%EOF\n"[..],
        PtFail::MediaMismatch,
    ));
    v
}

/// 10 类坏入件：9 类静态件 + 1 类真实超限件，逐类断言具体码值。
pub fn injection_core() -> (usize, usize) {
    let total = 10usize;
    let mut caught = 0usize;
    let samples = bad_samples();
    let mut i = 0usize;
    while i < samples.len() {
        if let Some((doc, want)) = samples.get(i) {
            if diagnose(doc, MediaSize::A4).err() == Some(*want) {
                caught += 1;
            }
        }
        i += 1;
    }
    let big = oversize_doc();
    if diagnose(&big, MediaSize::A4).err() == Some(PtFail::Oversize) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

const PDF_OK: &[u8] = b"%PDF-1.7\n/MediaBox [0 0 595 842]\nstartxref\n9\n%%EOF\n";
const PS_OK: &[u8] = b"%!PS-Adobe-3.0\nshowpage\n%%EOF\n";

pub fn run_unx50109_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50109");

    // J1-1 码值 ×10：连续、互异、可反解
    let mut ok = true;
    let mut seen = [false; 10];
    let mut i = 0usize;
    while i < 10 {
        let kind = match PtFail::from_index(i) {
            Some(k) => k,
            None => {
                ok = false;
                i += 1;
                continue;
            }
        };
        if kind.n() as usize != i + 1 || seen[i] {
            ok = false;
        }
        seen[i] = true;
        if kind.code() != SEG_BASE + (i as u32) + 1 {
            ok = false;
        }
        if PtFail::from_code(kind.code()) != Some(kind) {
            ok = false;
        }
        i += 1;
    }
    ok = ok && PtFail::from_code(SEG_BASE).is_none()
        && PtFail::from_code(SEG_BASE + 11).is_none();
    let det = format!("codes={}..{}", SEG_BASE + 1, SEG_BASE + 10);
    s.add_dyn("50109-J1-码值连续", ok, &det);

    // J1-2 注册表行序 = 码序（零漂移的硬约束：第 i 行必须是第 i+1 个码）
    let mut ok = true;
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        match FAULT_TABLE.get(i) {
            Some(r) => {
                if r.kind.n() as usize != i + 1 {
                    ok = false;
                }
                if r.kind.code() != SEG_BASE + (i as u32) + 1 {
                    ok = false;
                }
                if row_of(r.kind) != Some(*r) {
                    ok = false;
                }
                if row_by_code(r.kind.code()) != Some(*r) {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    let det = format!("rows={}", FAULT_TABLE.len());
    s.add_dyn("50109-J1-表行对齐", ok, &det);

    // J1-3 符号唯一且可反查
    let mut ok = true;
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        if let Some(r) = FAULT_TABLE.get(i) {
            if row_by_symbol(r.symbol) != Some(*r) {
                ok = false;
            }
            let mut j = i + 1;
            while j < FAULT_TABLE.len() {
                if let Some(o) = FAULT_TABLE.get(j) {
                    if o.symbol == r.symbol || o.kind == r.kind {
                        ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50109-J1-符号唯一", ok, if ok { "" } else { "符号/枚举重复" });

    // J1-4 策略自洽：Requeue ⇔ retryable；Policy 一律 Abort；Structure 一律 Fallback
    let mut ok = true;
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        if let Some(r) = FAULT_TABLE.get(i) {
            if r.retryable != (r.recovery == Recovery::Requeue) {
                ok = false;
            }
            if r.class == FaultClass::Policy && r.recovery != Recovery::Abort {
                ok = false;
            }
            if r.class == FaultClass::Structure && r.recovery != Recovery::Fallback {
                ok = false;
            }
        }
        i += 1;
    }
    // 四类必须都真实出现（空类别说明分类表退化成摆设）
    let mut classes = [false; 4];
    let mut i = 0usize;
    while i < FAULT_TABLE.len() {
        if let Some(r) = FAULT_TABLE.get(i) {
            let k = r.class.code() as usize;
            if k < 4 {
                classes[k] = true;
            }
        }
        i += 1;
    }
    ok = ok && classes[0] && classes[1] && classes[2] && classes[3];
    s.add("50109-J1-策略自洽", ok, if ok { "" } else { "策略/类别失配" });

    // J1-5 好样本：PDF 与 PS 都在白名单内被接收，版本字段如实带回
    let pdf = diagnose(PDF_OK, MediaSize::A4);
    let ps = diagnose(PS_OK, MediaSize::A4);
    let ok = match (pdf, ps) {
        (Ok(a), Ok(b)) => {
            a.format == DocFormat::Pdf
                && a.version_major == 1
                && a.version_minor == 7
                && a.media == Some((595, 842))
                && b.format == DocFormat::Ps
                && b.version_major == 3
        }
        _ => false,
    };
    s.add("50109-J1-白名单接收", ok, if ok { "" } else { "好样本被拒" });

    // J1-6 空/短：Empty 与 Truncated 分得开（都是 Input 类，但不能混报）
    let e = diagnose(&[], MediaSize::A4);
    let t = diagnose(b"%PD", MediaSize::A4);
    let ok = e.err() == Some(PtFail::Empty) && t.err() == Some(PtFail::Truncated);
    s.add("50109-J1-空短分档", ok, if ok { "" } else { "空/短混报" });

    // J1-7 加密优先于损坏：同时缺 %%EOF 时报 Encrypted
    let enc = diagnose(
        b"%PDF-1.7\n/Encrypt 4 0 R\nstartxref\n0\n",
        MediaSize::A4,
    );
    let ok = enc.err() == Some(PtFail::Encrypted);
    let det = format!("got={:?}", enc.err());
    s.add_dyn("50109-J1-加密优先", ok, &det);

    // J1-8 交叉引用与结束标记分开报
    let x = diagnose(b"%PDF-1.7\n%%EOF\n", MediaSize::A4);
    let d = diagnose(b"%PDF-1.7\nstartxref\n0\n", MediaSize::A4);
    let ok = x.err() == Some(PtFail::XrefBroken) && d.err() == Some(PtFail::Damaged);
    s.add("50109-J1-引用与结束", ok, if ok { "" } else { "两类结构错混报" });

    // J1-9 版本超界：PDF-3.0 与 PS-Adobe-9.0 都判 VersionBeyond
    let v1 = diagnose(b"%PDF-3.0\nstartxref\n0\n%%EOF\n", MediaSize::A4);
    let v2 = diagnose(b"%!PS-Adobe-9.0\n%%EOF\n", MediaSize::A4);
    let ok = v1.err() == Some(PtFail::VersionBeyond) && v2.err() == Some(PtFail::VersionBeyond);
    s.add("50109-J1-版本超界", ok, if ok { "" } else { "版本闸失守" });

    // J1-10 长度炸弹：/Length 超实际字节 → Truncated（与结构损坏区分）
    let lb = diagnose(
        b"%PDF-1.7\n/Length 999999\nstartxref\n0\n%%EOF\n",
        MediaSize::A4,
    );
    let ok = lb.err() == Some(PtFail::Truncated);
    s.add("50109-J1-长度炸弹", ok, if ok { "" } else { "长度谎言未拦" });

    // J1-11 字体内嵌缺失：有 BaseFont 无 FontFile → FontUnembedded
    let f1 = diagnose(
        b"%PDF-1.7\n/BaseFont /Helvetica\nstartxref\n0\n%%EOF\n",
        MediaSize::A4,
    );
    let f2 = diagnose(
        b"%PDF-1.7\n/BaseFont /Helvetica\n/FontFile 9 0 R\nstartxref\n0\n%%EOF\n",
        MediaSize::A4,
    );
    let ok = f1.err() == Some(PtFail::FontUnembedded) && f2.is_ok();
    s.add("50109-J1-字体内嵌", ok, if ok { "" } else { "内嵌判定错" });

    // J1-12 媒体不符：Letter 件投 A4 票据必拒；旋转 90° 视为可用
    let mm = diagnose(
        b"%PDF-1.7\n/MediaBox [0 0 612 792]\nstartxref\n0\n%%EOF\n",
        MediaSize::A4,
    );
    let rot = diagnose(
        b"%PDF-1.7\n/MediaBox [0 0 842 595]\nstartxref\n0\n%%EOF\n",
        MediaSize::A4,
    );
    let ok = mm.err() == Some(PtFail::MediaMismatch) && rot.is_ok();
    s.add("50109-J1-媒体校验", ok, if ok { "" } else { "媒体校验错" });

    // J1-13 超限：1 MiB + 1 字节 → Oversize（不是 Truncated、不是 Unsupported）
    let big = oversize_doc();
    let ob = diagnose(&big, MediaSize::A4);
    let ok = ob.err() == Some(PtFail::Oversize);
    let det = format!("len={}", big.len());
    s.add_dyn("50109-J1-超限先判", ok, &det);

    // J1-14 注入核：10 类坏入件全部命中各自码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50109-J1-注入必抓", ok, &det);

    // J1-15 账本守恒：9 类静态坏件 + 超限件 → total = 10、distinct = 10、order = 10
    let mut ledger = FaultLedger::new(16);
    let samples = bad_samples();
    let mut i = 0usize;
    while i < samples.len() {
        if let Some((_, want)) = samples.get(i) {
            ledger.record(*want);
        }
        i += 1;
    }
    ledger.record(PtFail::Oversize);
    let dist = ledger.distribution();
    let mut sum = 0u32;
    let mut k = 0usize;
    while k < 10 {
        if let Some(&c) = dist.get(k) {
            sum = sum.saturating_add(c);
        }
        k += 1;
    }
    let ok = sum == ledger.total() && ledger.total() == 10 && ledger.distinct() == 10
        && ledger.order().len() == 10;
    let det = format!("sum={} total={} d={}", sum, ledger.total(), ledger.distinct());
    s.add_dyn("50109-J1-账本守恒", ok, &det);

    // J1-16 账本容量：出现序 cap=3，记 5 次 → overflow=2，但计数仍是 5
    let mut l = FaultLedger::new(3);
    let mut i = 0usize;
    while i < 5 {
        l.record(PtFail::Damaged);
        i += 1;
    }
    let ok = l.total() == 5 && l.count(PtFail::Damaged) == 5 && l.order().len() == 3 && l.overflow() == 2;
    let det = format!("ord={} ov={}", l.order().len(), l.overflow());
    s.add_dyn("50109-J1-容量显式", ok, &det);

    // J1-17 批量扫描：好/坏混批，通过数 + 失败计数 = 批长
    let docs: [&[u8]; 4] = [
        PDF_OK,
        b"%PDF-1.7\nno xref\n%%EOF\n",
        PS_OK,
        b"%PDF-1.7\nstartxref\n0\n",
    ];
    let mut l = FaultLedger::new(8);
    let passed = l.sweep(&docs, MediaSize::A4);
    let ok = passed == 2 && l.total() == 4 && l.count(PtFail::XrefBroken) == 1
        && l.count(PtFail::Damaged) == 1;
    let det = format!("pass={} total={}", passed, l.total());
    s.add_dyn("50109-J1-混批记账", ok, &det);

    // J1-18 判定确定性：同件两次判定结果逐位一致
    let a = diagnose(PDF_OK, MediaSize::A4);
    let b = diagnose(PDF_OK, MediaSize::A4);
    let c = diagnose(b"%PDF-1.7\n%%EOF\n", MediaSize::A4);
    let d = diagnose(b"%PDF-1.7\n%%EOF\n", MediaSize::A4);
    let ok = a == b && c.err() == d.err() && c.err() == Some(PtFail::XrefBroken);
    s.add("50109-J1-判定确定", ok, if ok { "" } else { "非确定" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50109_checks_green() {
        let set = run_unx50109_checks();
        assert!(set.all_passed(), "UNX-F50109 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50109_codes_are_frozen() {
        let mut i = 0u16;
        while i < 10 {
            let k = PtFail::from_index(i as usize).expect("row");
            assert_eq!(k.code(), SEG_BASE + i as u32 + 1);
            assert_eq!(PtFail::from_code(k.code()), Some(k));
            i += 1;
        }
        assert_eq!(PtFail::from_code(SEG_BASE), None);
        assert_eq!(PtFail::from_code(SEG_BASE + 11), None);
    }

    #[test]
    fn unx50109_good_pdf_passes_and_bad_pdf_is_classified() {
        assert!(diagnose(PDF_OK, MediaSize::A4).is_ok());
        assert_eq!(
            diagnose(b"%PDF-1.7\n/Encrypt 4 0 R\nstartxref\n0\n%%EOF\n", MediaSize::A4).err(),
            Some(PtFail::Encrypted)
        );
        assert_eq!(
            diagnose(b"%PDF-1.7\nstartxref\n0\n%%EOF\n", MediaSize::Letter).err(),
            None
        );
    }
}
