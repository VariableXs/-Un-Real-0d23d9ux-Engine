//! UNX-F50101 PDF 直通判定器（打印机原生 PDF 支持探测）（0x438c0 段）
//!
//! 锚点：UNX-F50101-J1 探测 ×3 档打印机模型判定准确，不支持显式回退光栅化；
//! 判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x438c0`。诊断码分两个子区间，互不重叠：
//! * 探测/解析/会话错误 `SEG_BASE + 1 ..= SEG_BASE + 10`（见 [`PtError`]）；
//! * 回退原因 `SEG_BASE + 0x10 + 1 ..= SEG_BASE + 0x10 + 8`（见 [`RejectReason`]）。
//!
//! 两张表分开的理由：解析错误是「没问清」，回退原因是「问清了但不支持」，
//! 二者的处置完全不同（前者重试探测，后者换渲染路径）。混在一个枚举里，
//! 遥测侧就分不出「打印机坏了」还是「打印机就不吃 PDF」。
//!
//! ```text
//! 设备自述文本 --parse_report--> DeviceCaps
//!                                    |
//! JobProfile(Document) --decide()----+--> Route::PdfDirect
//!                                        \-> Route::Rasterize(RejectReason)
//! ```
//!
//! 判定优先级是**固定**的（越靠前越先报，保证同一文档永远得到同一原因）：
//! 无 PDF 通道 → 加密 → 版本过高 → 透明不支持 → CID 不支持 → 超尺寸 →
//! 超页数 → 字体不可嵌（许可禁嵌或字体风味不支持）。这条顺序既是排障顺序，
//! 也是「不支持必显式回退光栅化」的落点：没有 `Route::PdfDirect` 就是回退，
//! 且一定带一个具体原因，不存在「悄悄光栅化」。
//!
//! 结构清单：
//! * [`PdfVersion`] / [`Model`] / [`DeviceCaps`]：能力模型；
//! * [`parse_report`] / [`parse_u32`] / [`parse_fonts`]：设备自述解析器（键序无关）；
//! * [`ProbeSession`] / [`ProbeState`]：探测会话状态机（Idle→Probing→Ready/Failed）；
//! * [`Document`] / [`Route`] / [`RejectReason`] / [`decide`]：判定器；
//! * [`ModelTable`] / [`RouteLedger`]：三档模型表与路由账（守恒 + 摘要）；
//! * [`injection_core`]：10 类坏报告/坏调用注入核（逐类断言具体诊断码）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x438c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x438c0;
/// 回退原因子区间起点（与解析错误隔开 0x10）。
pub const REASON_BASE: u32 = SEG_BASE + 0x10;

const E_EMPTY: u16 = 1;
const E_KEY: u16 = 2;
const E_VALUE: u16 = 3;
const E_DUP_KEY: u16 = 4;
const E_MISSING: u16 = 5;
const E_MODEL: u16 = 6;
const E_VERSION: u16 = 7;
const E_RANGE: u16 = 8;
const E_STATE: u16 = 9;
const E_ATTEMPTS: u16 = 10;

const R_NO_CHANNEL: u16 = 1;
const R_VERSION: u16 = 2;
const R_TRANSPARENCY: u16 = 3;
const R_CID: u16 = 4;
const R_SIZE: u16 = 5;
const R_PAGES: u16 = 6;
const R_ENCRYPTED: u16 = 7;
const R_FONT: u16 = 8;

/// 探测/解析/会话错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtError {
    /// 自述文本为空。
    Empty,
    /// 键不认识（或整段没有 `=`）。
    Key,
    /// 值不可解析（yes/no、数字、字体风味表）。
    Value,
    /// 同一个键出现两次。
    DupKey,
    /// 缺少必需键（model / pdf，声明支持 PDF 时还缺 pdf-version）。
    Missing,
    /// 型号不在三档闭集内。
    Model,
    /// 版本串不是 `d.d` 或超出 0.0..=2.9。
    Version,
    /// 数值超出合法区间。
    Range,
    /// 会话状态不接受当前调用。
    State,
    /// 探测尝试预算用尽。
    Attempts,
}

impl PtError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            PtError::Empty => E_EMPTY,
            PtError::Key => E_KEY,
            PtError::Value => E_VALUE,
            PtError::DupKey => E_DUP_KEY,
            PtError::Missing => E_MISSING,
            PtError::Model => E_MODEL,
            PtError::Version => E_VERSION,
            PtError::Range => E_RANGE,
            PtError::State => E_STATE,
            PtError::Attempts => E_ATTEMPTS,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            PtError::Empty => "probe report empty",
            PtError::Key => "unknown key in probe report",
            PtError::Value => "probe value unparsable",
            PtError::DupKey => "duplicate key in probe report",
            PtError::Missing => "required key missing",
            PtError::Model => "printer model outside three-tier set",
            PtError::Version => "pdf version malformed or out of range",
            PtError::Range => "numeric value outside legal range",
            PtError::State => "probe session state rejects this call",
            PtError::Attempts => "probe attempt budget exhausted",
        }
    }
}

/// 回退原因。`code()` 返回 `REASON_BASE + n`（0x438d0 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 该机没有 PDF 直通通道。
    NoChannel,
    /// 文档 PDF 版本高于设备上限。
    VersionTooNew,
    /// 文档含透明对象而设备不支持。
    Transparency,
    /// 文档用 CID/CJK 字体而设备不支持 CID。
    CidFont,
    /// 首面或整份超设备尺寸上限。
    SizeLimit,
    /// 页数超设备上限。
    PageLimit,
    /// 文档加密且设备不接受。
    Encrypted,
    /// 字体不可嵌（许可禁嵌或风味不在设备表内）。
    FontForbidden,
}

impl RejectReason {
    /// 诊断码：`REASON_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            RejectReason::NoChannel => R_NO_CHANNEL,
            RejectReason::VersionTooNew => R_VERSION,
            RejectReason::Transparency => R_TRANSPARENCY,
            RejectReason::CidFont => R_CID,
            RejectReason::SizeLimit => R_SIZE,
            RejectReason::PageLimit => R_PAGES,
            RejectReason::Encrypted => R_ENCRYPTED,
            RejectReason::FontForbidden => R_FONT,
        };
        REASON_BASE + n as u32
    }

    /// 一行英文说明（日志与 UI 共用）。
    pub fn message(self) -> &'static str {
        match self {
            RejectReason::NoChannel => "printer has no native pdf channel",
            RejectReason::VersionTooNew => "pdf version above device ceiling",
            RejectReason::Transparency => "transparency unsupported",
            RejectReason::CidFont => "cid/cjk fonts unsupported",
            RejectReason::SizeLimit => "job exceeds device size limit",
            RejectReason::PageLimit => "job exceeds device page limit",
            RejectReason::Encrypted => "encrypted pdf rejected by device",
            RejectReason::FontForbidden => "fonts cannot be embedded",
        }
    }

    /// 全部原因（收口自检用）。
    pub fn all() -> [RejectReason; 8] {
        [
            RejectReason::NoChannel,
            RejectReason::VersionTooNew,
            RejectReason::Transparency,
            RejectReason::CidFont,
            RejectReason::SizeLimit,
            RejectReason::PageLimit,
            RejectReason::Encrypted,
            RejectReason::FontForbidden,
        ]
    }
}

/// PDF 版本（主.次）。只比较 `(major, minor)`，不比较 PDF 修订号。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PdfVersion {
    /// 主版本。
    pub major: u8,
    /// 次版本。
    pub minor: u8,
}

impl PdfVersion {
    /// 常量构造。
    pub const fn new(major: u8, minor: u8) -> PdfVersion {
        PdfVersion { major, minor }
    }

    /// 编码成 `major*10 + minor`（0.0 表示「不支持 PDF」）。
    pub fn code(self) -> u8 {
        self.major.saturating_mul(10).saturating_add(self.minor)
    }

    /// 解析 `d.d`。只认一位主版本与一位次版本——两段以上说明自述格式越界，
    /// 宁可报错也不猜（猜错会把不支持当支持）。
    pub fn parse(s: &str) -> Result<PdfVersion, PtError> {
        let bytes = s.as_bytes();
        if bytes.len() != 3 {
            return Err(PtError::Version);
        }
        let a = match bytes.get(0) {
            Some(v) => *v,
            None => return Err(PtError::Version),
        };
        let dot = match bytes.get(1) {
            Some(v) => *v,
            None => return Err(PtError::Version),
        };
        let b = match bytes.get(2) {
            Some(v) => *v,
            None => return Err(PtError::Version),
        };
        if dot != b'.' || !a.is_ascii_digit() || !b.is_ascii_digit() {
            return Err(PtError::Version);
        }
        let major = a - b'0';
        let minor = b - b'0';
        if major > 2 {
            return Err(PtError::Version);
        }
        Ok(PdfVersion { major, minor })
    }

    /// 是否比 `other` 新（严格大于）。
    pub fn is_newer_than(self, other: PdfVersion) -> bool {
        (self.major, self.minor) > (other.major, other.minor)
    }

    /// 静态标签（报告里不带堆分配）。
    pub fn label(self) -> &'static str {
        match self.code() {
            10 => "1.0",
            11 => "1.1",
            12 => "1.2",
            13 => "1.3",
            14 => "1.4",
            15 => "1.5",
            16 => "1.6",
            17 => "1.7",
            20 => "2.0",
            _ => "0.0",
        }
    }
}

/// 打印机的 PDF 直通档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    /// 一档：全功能（PDF 1.7 / CID / 透明 / 三种字体风味）。
    OfficeLaser,
    /// 二档：工作组机（PDF 1.4 / 无 CID / 无透明）。
    WorkgroupMono,
    /// 三档：老冲击式（无 PDF 通道，只能光栅）。
    LegacyImpact,
}

impl Model {
    /// 档位号（1..=3，越小越强）。
    pub fn tier(self) -> u8 {
        match self {
            Model::OfficeLaser => 1,
            Model::WorkgroupMono => 2,
            Model::LegacyImpact => 3,
        }
    }

    /// 自述文本里的型号名。
    pub fn label(self) -> &'static str {
        match self {
            Model::OfficeLaser => "officelaser",
            Model::WorkgroupMono => "workgroupmono",
            Model::LegacyImpact => "legacyimpact",
        }
    }

    /// 型号名反解。
    pub fn from_label(s: &str) -> Result<Model, PtError> {
        if s == "officelaser" {
            Ok(Model::OfficeLaser)
        } else if s == "workgroupmono" {
            Ok(Model::WorkgroupMono)
        } else if s == "legacyimpact" {
            Ok(Model::LegacyImpact)
        } else {
            Err(PtError::Model)
        }
    }

    /// 三档全表（报告与矩阵自检按此序）。
    pub fn all() -> [Model; 3] {
        [Model::OfficeLaser, Model::WorkgroupMono, Model::LegacyImpact]
    }
}

/// 设备 PDF 能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceCaps {
    /// 型号。
    pub model: Model,
    /// 是否有 PDF 直通通道。
    pub pdf_direct: bool,
    /// 可接受的最高 PDF 版本（无通道时为 0.0）。
    pub max_version: PdfVersion,
    /// 是否支持 CID/CJK 字体。
    pub cid: bool,
    /// 是否支持透明。
    pub transparency: bool,
    /// 单份上限（KiB）。
    pub max_kb: u32,
    /// 页数上限。
    pub max_pages: u32,
    /// 是否接受加密 PDF。
    pub encrypted_ok: bool,
    /// 可嵌入字体风味位图：1=TrueType，2=Type1，4=CFF。
    pub embed_types: u8,
}

/// 字体风味位：TrueType。
pub const FLAVOR_TRUETYPE: u8 = 1;
/// 字体风味位：Type1。
pub const FLAVOR_TYPE1: u8 = 2;
/// 字体风味位：CFF/OpenType。
pub const FLAVOR_CFF: u8 = 4;

/// 尺寸上限（KiB）。10 GiB，超过一定是自述文本被写坏。
pub const MAX_KB_LIMIT: u32 = 10_000_000;
/// 页数上限。一百万页，同理。
pub const MAX_PAGES_LIMIT: u32 = 1_000_000;
/// 探测尝试预算（含成功的那次）。
pub const MAX_ATTEMPTS: u8 = 3;

fn trim(s: &str) -> &str {
    s.trim()
}

/// 解析十进制无符号数。逐位 `checked_mul`，溢出即 `Range`，不回绕。
pub fn parse_u32(s: &str, limit: u32) -> Result<u32, PtError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(PtError::Value);
    }
    let mut acc: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(PtError::Value),
        };
        if !c.is_ascii_digit() {
            return Err(PtError::Value);
        }
        let d = (c - b'0') as u32;
        acc = match acc.checked_mul(10) {
            Some(v) => v,
            None => return Err(PtError::Range),
        };
        acc = match acc.checked_add(d) {
            Some(v) => v,
            None => return Err(PtError::Range),
        };
        i += 1;
    }
    if acc > limit {
        return Err(PtError::Range);
    }
    Ok(acc)
}

/// 解析字体风味表 `truetype,type1,cff`。空串表示「一个都不支持」（位图 0）。
pub fn parse_fonts(s: &str) -> Result<u8, PtError> {
    if s.is_empty() {
        return Ok(0);
    }
    let mut mask = 0u8;
    let mut start = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        let comma = match bytes.get(i) {
            Some(b) => *b == b',',
            None => false,
        };
        if at_end || comma {
            let seg = match s.get(start..i) {
                Some(v) => trim(v),
                None => "",
            };
            if seg == "truetype" {
                mask |= FLAVOR_TRUETYPE;
            } else if seg == "type1" {
                mask |= FLAVOR_TYPE1;
            } else if seg == "cff" {
                mask |= FLAVOR_CFF;
            } else {
                // 不认识的字体风味必须报错：忽略它会让「支持 CFF」被高估。
                return Err(PtError::Value);
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(mask)
}

fn parse_bool(s: &str) -> Result<bool, PtError> {
    if s == "yes" {
        Ok(true)
    } else if s == "no" {
        Ok(false)
    } else {
        Err(PtError::Value)
    }
}

fn key_index(key: &str) -> Option<usize> {
    if key == "model" {
        Some(0)
    } else if key == "pdf" {
        Some(1)
    } else if key == "pdf-version" {
        Some(2)
    } else if key == "cid" {
        Some(3)
    } else if key == "transparency" {
        Some(4)
    } else if key == "max-kb" {
        Some(5)
    } else if key == "max-pages" {
        Some(6)
    } else if key == "encrypted" {
        Some(7)
    } else if key == "fonts" {
        Some(8)
    } else {
        None
    }
}

/// 解析设备自述文本（`key=value;` 序列，键序无关）。
///
/// 缺省口径：`cid` / `transparency` / `encrypted` 缺省为 `no`，`max-kb` /
/// `max-pages` 缺省为 0，`fonts` 缺省为 0——**缺省一律按「不支持」**，绝不
/// 按「支持」兜底（这是整张卡「不虚报」的根）。
pub fn parse_report(text: &str) -> Result<DeviceCaps, PtError> {
    if trim(text).is_empty() {
        return Err(PtError::Empty);
    }
    let mut seen = [false; 9];
    let mut model: Option<Model> = None;
    let mut pdf: Option<bool> = None;
    let mut version: Option<PdfVersion> = None;
    let mut cid = false;
    let mut transparency = false;
    let mut max_kb = 0u32;
    let mut max_pages = 0u32;
    let mut encrypted_ok = false;
    let mut embed = 0u8;
    let mut pairs = 0usize;

    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        let semi = match bytes.get(i) {
            Some(b) => *b == b';',
            None => false,
        };
        if at_end || semi {
            let seg = match text.get(start..i) {
                Some(v) => trim(v),
                None => "",
            };
            start = i + 1;
            if !seg.is_empty() {
                pairs += 1;
                let eq = match seg.find('=') {
                    Some(v) => v,
                    None => return Err(PtError::Key),
                };
                let key = trim(match seg.get(..eq) {
                    Some(v) => v,
                    None => return Err(PtError::Key),
                });
                let val = trim(match seg.get(eq + 1..) {
                    Some(v) => v,
                    None => return Err(PtError::Value),
                });
                let idx = match key_index(key) {
                    Some(v) => v,
                    None => return Err(PtError::Key),
                };
                if let Some(flag) = seen.get_mut(idx) {
                    if *flag {
                        return Err(PtError::DupKey);
                    }
                    *flag = true;
                }
                match idx {
                    0 => model = Some(Model::from_label(val)?),
                    1 => pdf = Some(parse_bool(val)?),
                    2 => version = Some(PdfVersion::parse(val)?),
                    3 => cid = parse_bool(val)?,
                    4 => transparency = parse_bool(val)?,
                    5 => max_kb = parse_u32(val, MAX_KB_LIMIT)?,
                    6 => max_pages = parse_u32(val, MAX_PAGES_LIMIT)?,
                    7 => encrypted_ok = parse_bool(val)?,
                    8 => embed = parse_fonts(val)?,
                    _ => return Err(PtError::Key),
                }
            }
        }
        i += 1;
    }
    if pairs == 0 {
        return Err(PtError::Empty);
    }
    let model = match model {
        Some(m) => m,
        None => return Err(PtError::Missing),
    };
    let pdf_direct = match pdf {
        Some(v) => v,
        None => return Err(PtError::Missing),
    };
    let max_version = match version {
        Some(v) => v,
        None => {
            // 声称支持 PDF 却不报版本 = 自述不完整；不支持时 0.0 才是正解。
            if pdf_direct {
                return Err(PtError::Missing);
            }
            PdfVersion::new(0, 0)
        }
    };
    if pdf_direct && max_version.code() == 0 {
        return Err(PtError::Value);
    }
    if !pdf_direct && (cid || transparency || max_version.code() != 0) {
        // 无通道却声明 PDF 相关能力：自相矛盾的自述，宁可拒绝也不采信。
        return Err(PtError::Value);
    }
    Ok(DeviceCaps {
        model,
        pdf_direct,
        max_version,
        cid,
        transparency,
        max_kb: if pdf_direct { max_kb } else { 0 },
        max_pages: if pdf_direct { max_pages } else { 0 },
        encrypted_ok: if pdf_direct { encrypted_ok } else { false },
        embed_types: if pdf_direct { embed } else { 0 },
    })
}

/// 待打印文档的画像（只保留影响路由的字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Document {
    /// 文档 ID（台账用）。
    pub id: u8,
    /// PDF 版本。
    pub version: PdfVersion,
    /// 页数。
    pub pages: u32,
    /// 体积（KiB）。
    pub size_kb: u32,
    /// 是否含透明对象。
    pub transparency: bool,
    /// 是否使用 CID/CJK 字体。
    pub cid_fonts: bool,
    /// 是否加密。
    pub encrypted: bool,
    /// 字体许可是否禁止嵌入（禁嵌则直通不可能）。
    pub embed_forbidden: bool,
    /// 字体风味位图（0 = 无需嵌入字体）。
    pub font_flavor: u8,
}

/// 路由结果。`Rasterize` 必带具体原因——不存在「无原因回退」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 原生 PDF 直通。
    PdfDirect,
    /// 回退光栅化，并给出显式原因。
    Rasterize(RejectReason),
}

impl Route {
    /// 是否直通。
    pub fn is_direct(self) -> bool {
        matches!(self, Route::PdfDirect)
    }

    /// 回退原因（直通时为 None）。
    pub fn reason(self) -> Option<RejectReason> {
        match self {
            Route::PdfDirect => None,
            Route::Rasterize(r) => Some(r),
        }
    }

    /// 台账用的路由 tag：直通 0，回退 `0x10 + n`。
    pub fn tag(self) -> u32 {
        match self {
            Route::PdfDirect => 0,
            // 回退 tag 规范为 0x10 + n，n 为 RejectReason 的段内序号。
            Route::Rasterize(r) => r.code() - SEG_BASE,
        }
    }
}

/// 判定入口：按固定优先级给出路由（顺序见模块头）。
///
/// 为什么顺序不能乱：同一份「又透明又超页」的文档，如果先报页数，运维会去
/// 调页数上限，调完仍然失败；先报透明，才能一次改到点子上。
pub fn decide(doc: &Document, caps: &DeviceCaps) -> Route {
    if !caps.pdf_direct {
        return Route::Rasterize(RejectReason::NoChannel);
    }
    if doc.encrypted && !caps.encrypted_ok {
        return Route::Rasterize(RejectReason::Encrypted);
    }
    if doc.version.is_newer_than(caps.max_version) {
        return Route::Rasterize(RejectReason::VersionTooNew);
    }
    if doc.transparency && !caps.transparency {
        return Route::Rasterize(RejectReason::Transparency);
    }
    if doc.cid_fonts && !caps.cid {
        return Route::Rasterize(RejectReason::CidFont);
    }
    if doc.size_kb > caps.max_kb {
        return Route::Rasterize(RejectReason::SizeLimit);
    }
    if doc.pages > caps.max_pages {
        return Route::Rasterize(RejectReason::PageLimit);
    }
    if doc.embed_forbidden {
        return Route::Rasterize(RejectReason::FontForbidden);
    }
    if doc.font_flavor != 0 && caps.embed_types & doc.font_flavor != doc.font_flavor {
        return Route::Rasterize(RejectReason::FontForbidden);
    }
    Route::PdfDirect
}

/// 探测会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    /// 未开始。
    Idle,
    /// 已发起探测，等待自述。
    Probing,
    /// 已拿到能力。
    Ready,
    /// 尝试预算用尽。
    Failed,
}

/// 探测会话：Idle → Probing → Ready / Failed。
#[derive(Clone, Debug)]
pub struct ProbeSession {
    state: ProbeState,
    attempts: u8,
    caps: Option<DeviceCaps>,
}

impl ProbeSession {
    /// 新会话（Idle）。
    pub fn new() -> ProbeSession {
        ProbeSession { state: ProbeState::Idle, attempts: 0, caps: None }
    }

    /// 当前状态。
    pub fn state(&self) -> ProbeState {
        self.state
    }

    /// 已用尝试次数。
    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    /// 已签发的设备能力。
    pub fn caps(&self) -> Option<DeviceCaps> {
        self.caps
    }

    /// 发起探测。只有 Idle 能开；重复 open 是状态错误（防止把预算刷回去）。
    pub fn open(&mut self) -> Result<(), PtError> {
        if self.state != ProbeState::Idle {
            return if self.state == ProbeState::Failed {
                Err(PtError::Attempts)
            } else {
                Err(PtError::State)
            };
        }
        self.state = ProbeState::Probing;
        Ok(())
    }

    /// 提交自述文本。每次提交消耗一次预算（成功也算，预算是「尝试」不是「失败」）。
    pub fn submit(&mut self, text: &str) -> Result<DeviceCaps, PtError> {
        match self.state {
            ProbeState::Probing => {}
            ProbeState::Failed => return Err(PtError::Attempts),
            _ => return Err(PtError::State),
        }
        self.attempts = self.attempts.saturating_add(1);
        match parse_report(text) {
            Ok(c) => {
                self.state = ProbeState::Ready;
                self.caps = Some(c);
                Ok(c)
            }
            Err(e) => {
                if self.attempts >= MAX_ATTEMPTS {
                    self.state = ProbeState::Failed;
                }
                Err(e)
            }
        }
    }

    /// 一次无响应（超时）。返回消耗后的状态。
    pub fn timeout(&mut self) -> Result<ProbeState, PtError> {
        if self.state != ProbeState::Probing {
            return Err(PtError::State);
        }
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= MAX_ATTEMPTS {
            self.state = ProbeState::Failed;
        }
        Ok(self.state)
    }
}

impl Default for ProbeSession {
    fn default() -> ProbeSession {
        ProbeSession::new()
    }
}

/// 三档模型表：由三份自述文本解析而来（解析器与判定器共用同一条链）。
#[derive(Clone, Debug)]
pub struct ModelTable {
    caps: Vec<DeviceCaps>,
}

impl ModelTable {
    /// 从能力表构建（按档位升序插入，重复型号拒绝）。
    pub fn from_caps(caps: &[DeviceCaps]) -> Result<ModelTable, PtError> {
        if caps.is_empty() {
            return Err(PtError::Empty);
        }
        let mut out = Vec::with_capacity(caps.len());
        let tiers = Model::all();
        let mut t = 0usize;
        while t < tiers.len() {
            if let Some(want) = tiers.get(t) {
                let mut found = None;
                let mut i = 0usize;
                while i < caps.len() {
                    if let Some(c) = caps.get(i) {
                        if c.model == *want {
                            if found.is_some() {
                                return Err(PtError::Model);
                            }
                            found = Some(*c);
                        }
                    }
                    i += 1;
                }
                if let Some(c) = found {
                    out.push(c);
                }
            }
            t += 1;
        }
        if out.len() != caps.len() {
            // 出现表外型号或重复型号：两者都说明表被拼错了。
            return Err(PtError::Model);
        }
        Ok(ModelTable { caps: out })
    }

    /// 用内置三份自述构建。
    pub fn builtin() -> Result<ModelTable, PtError> {
        let mut caps = Vec::with_capacity(3);
        caps.push(parse_report(REPORT_OFFICE)?);
        caps.push(parse_report(REPORT_WORKGROUP)?);
        caps.push(parse_report(REPORT_LEGACY)?);
        ModelTable::from_caps(&caps)
    }

    /// 取某型号能力。
    pub fn get(&self, model: Model) -> Option<DeviceCaps> {
        let mut i = 0usize;
        while i < self.caps.len() {
            if let Some(c) = self.caps.get(i) {
                if c.model == model {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 表长。
    pub fn len(&self) -> usize {
        self.caps.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }

    /// 对某型号判一份文档。表里没有该型号 → `Model`。
    pub fn route(&self, model: Model, doc: &Document) -> Result<Route, PtError> {
        match self.get(model) {
            Some(c) => Ok(decide(doc, &c)),
            None => Err(PtError::Model),
        }
    }
}

/// 台账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteEntry {
    /// 型号。
    pub model: Model,
    /// 文档 ID。
    pub doc: u8,
    /// 路由。
    pub route: Route,
}

/// 路由账：守恒（total = direct + raster）+ 逐型号计数 + 顺序无关摘要。
#[derive(Clone, Debug)]
pub struct RouteLedger {
    entries: Vec<RouteEntry>,
}

impl RouteLedger {
    /// 空账。
    pub fn new() -> RouteLedger {
        RouteLedger { entries: Vec::with_capacity(16) }
    }

    /// 记一笔。
    pub fn note(&mut self, model: Model, doc: u8, route: Route) {
        self.entries.push(RouteEntry { model, doc, route });
    }

    /// 逐型号判定并记账（矩阵自检的主入口）。
    pub fn fill(&mut self, table: &ModelTable, docs: &[Document]) -> Result<usize, PtError> {
        if docs.is_empty() {
            return Err(PtError::Empty);
        }
        let models = Model::all();
        let mut m = 0usize;
        let mut n = 0usize;
        while m < models.len() {
            if let Some(model) = models.get(m) {
                let mut d = 0usize;
                while d < docs.len() {
                    if let Some(doc) = docs.get(d) {
                        let route = table.route(*model, doc)?;
                        self.note(*model, doc.id, route);
                        n += 1;
                    }
                    d += 1;
                }
            }
            m += 1;
        }
        Ok(n)
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 直通数。
    pub fn direct(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.route.is_direct() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 回退数。
    pub fn raster(&self) -> usize {
        self.total() - self.direct()
    }

    /// 某原因出现次数。
    pub fn reason_count(&self, reason: RejectReason) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.route.reason() == Some(reason) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某型号记账条数。
    pub fn count_for(&self, model: Model) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.model == model {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 读一笔。
    pub fn entry(&self, idx: usize) -> Option<RouteEntry> {
        self.entries.get(idx).copied()
    }

    /// 摘要：按 (型号档位, 文档 id, 路由 tag) 折叠。记账顺序即判定顺序，
    /// 故摘要同时是「判定确定」的指纹。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let bytes = [
                    e.model.tier(),
                    e.doc,
                    (e.route.tag() >> 8) as u8,
                    e.route.tag() as u8,
                ];
                let mut j = 0usize;
                while j < bytes.len() {
                    if let Some(b) = bytes.get(j) {
                        h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 一行报告（结果摘要非空，供软件写回）。
    pub fn report(&self) -> String {
        format!(
            "pdfpt total={} direct={} raster={} digest={:08x}",
            self.total(),
            self.direct(),
            self.raster(),
            self.digest()
        )
    }
}

impl Default for RouteLedger {
    fn default() -> RouteLedger {
        RouteLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 内置自述与语料
// ---------------------------------------------------------------------------

/// 一档机自述：PDF 1.7 + CID + 透明 + 三种字体风味。
pub const REPORT_OFFICE: &str = "model=officelaser;pdf=yes;pdf-version=1.7;cid=yes;transparency=yes;max-kb=51200;max-pages=2000;encrypted=yes;fonts=truetype,type1,cff";
/// 二档机自述：PDF 1.4，无 CID、无透明、不吃加密。
pub const REPORT_WORKGROUP: &str = "model=workgroupmono;pdf=yes;pdf-version=1.4;cid=no;transparency=no;max-kb=10240;max-pages=500;encrypted=no;fonts=truetype,type1";
/// 三档机自述：无 PDF 通道（其余键一律缺省为「不支持」）。
pub const REPORT_LEGACY: &str = "model=legacyimpact;pdf=no;max-kb=0;max-pages=0;encrypted=no;fonts=";

/// 现代文档：1.7 / 透明 / CID / CFF 字体。
pub const DOC_MODERN: Document = Document {
    id: 1,
    version: PdfVersion { major: 1, minor: 7 },
    pages: 120,
    size_kb: 8_000,
    transparency: true,
    cid_fonts: true,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: FLAVOR_CFF,
};

/// 保守文档：1.3 / 无透明 / 无 CID / TrueType。
pub const DOC_SAFE: Document = Document {
    id: 2,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 40,
    size_kb: 900,
    transparency: false,
    cid_fonts: false,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: FLAVOR_TRUETYPE,
};

/// CID 文档：1.4 但用 CJK 字体。
pub const DOC_CID: Document = Document {
    id: 3,
    version: PdfVersion { major: 1, minor: 4 },
    pages: 60,
    size_kb: 2_000,
    transparency: false,
    cid_fonts: true,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: FLAVOR_TRUE_BASE,
};

/// 加密文档。
pub const DOC_ENC: Document = Document {
    id: 4,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 12,
    size_kb: 400,
    transparency: false,
    cid_fonts: false,
    encrypted: true,
    embed_forbidden: false,
    font_flavor: FLAVOR_TRUETYPE,
};

/// 大体积文档（20,000 KiB）。
pub const DOC_BIG: Document = Document {
    id: 5,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 10,
    size_kb: 20_000,
    transparency: false,
    cid_fonts: false,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: 0,
};

/// 长文档（600 页）。
pub const DOC_LONG: Document = Document {
    id: 6,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 600,
    size_kb: 100,
    transparency: false,
    cid_fonts: false,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: 0,
};

/// 又透明又超页：用于验证优先级固定。
pub const DOC_TRANS_LONG: Document = Document {
    id: 7,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 900,
    size_kb: 50_000,
    transparency: true,
    cid_fonts: false,
    encrypted: false,
    embed_forbidden: false,
    font_flavor: 0,
};

/// 字体许可禁嵌。
pub const DOC_NOLIC: Document = Document {
    id: 8,
    version: PdfVersion { major: 1, minor: 3 },
    pages: 20,
    size_kb: 500,
    transparency: false,
    cid_fonts: false,
    encrypted: false,
    embed_forbidden: true,
    font_flavor: FLAVOR_TYPE1,
};

/// TrueType 风味别名（`DOC_CID` 用；单独起名避免与位常量混淆）。
pub const FLAVOR_TRUE_BASE: u8 = FLAVOR_TRUETYPE;

/// 判定矩阵的三份代表文档（J1「×3 档判定准确」的主体）。
pub fn matrix_docs() -> Vec<Document> {
    let mut v = Vec::with_capacity(3);
    v.push(DOC_MODERN);
    v.push(DOC_SAFE);
    v.push(DOC_CID);
    v
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn session_open() -> ProbeSession {
    let mut s = ProbeSession::new();
    let _ = s.open();
    s
}

/// 坏样本注入核：10 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 空自述
    if parse_report("").err() == Some(PtError::Empty) {
        caught += 1;
    }
    // 2 未知键
    if parse_report("model=officelaser;bogus=1;pdf=yes;pdf-version=1.7").err() == Some(PtError::Key) {
        caught += 1;
    }
    // 3 值不可解析
    if parse_report("model=officelaser;pdf=maybe").err() == Some(PtError::Value) {
        caught += 1;
    }
    // 4 键重复
    if parse_report("model=officelaser;model=workgroupmono;pdf=yes;pdf-version=1.7").err()
        == Some(PtError::DupKey)
    {
        caught += 1;
    }
    // 5 缺必需键（没有 model）
    if parse_report("pdf=yes;pdf-version=1.7").err() == Some(PtError::Missing) {
        caught += 1;
    }
    // 6 型号越界
    if parse_report("model=plotter9;pdf=yes;pdf-version=1.7").err() == Some(PtError::Model) {
        caught += 1;
    }
    // 7 版本串畸形
    if parse_report("model=officelaser;pdf=yes;pdf-version=17").err() == Some(PtError::Version) {
        caught += 1;
    }
    // 8 数值超出区间（页数上限一百万）
    if parse_report("model=officelaser;pdf=yes;pdf-version=1.7;max-pages=1000001").err()
        == Some(PtError::Range)
    {
        caught += 1;
    }
    // 9 会话状态错（未开就提交）
    let mut idle = ProbeSession::new();
    if idle.submit(REPORT_OFFICE).err() == Some(PtError::State) {
        caught += 1;
    }
    // 10 尝试预算用尽
    let mut s = session_open();
    let _ = s.timeout();
    let _ = s.timeout();
    let _ = s.timeout();
    if s.state() == ProbeState::Failed && s.submit(REPORT_OFFICE).err() == Some(PtError::Attempts) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50101 的 16 条判据（含 J1 三档判定准确与注入核）。
pub fn run_unx50101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50101");
    let table = ModelTable::builtin();

    // J1-1 三档模型表：3 条、档位 1/2/3、型号自洽、无通道者能力全零。
    let ok = match &table {
        Ok(t) => {
            let mut good = t.len() == 3 && !t.is_empty();
            let models = Model::all();
            let mut i = 0usize;
            while i < models.len() {
                if let Some(m) = models.get(i) {
                    match t.get(*m) {
                        Some(c) => {
                            if c.model != *m || m.tier() != (i as u8 + 1) {
                                good = false;
                            }
                            if !c.pdf_direct
                                && (c.max_version.code() != 0 || c.cid || c.transparency || c.embed_types != 0)
                            {
                                good = false;
                            }
                        }
                        None => good = false,
                    }
                }
                i += 1;
            }
            good
        }
        Err(_) => false,
    };
    let det = format!(
        "models={}",
        match &table {
            Ok(t) => t.len(),
            Err(_) => 0,
        }
    );
    s.add_dyn("50101-J1-三档模型表", ok, &det);

    // J1-2 解析器逐字段：一档自述 → 1.7 / CID / 透明 / 51200 / 2000 / 加密 / 三风味。
    let off = parse_report(REPORT_OFFICE);
    let ok = match &off {
        Ok(c) => {
            c.model == Model::OfficeLaser
                && c.pdf_direct
                && c.max_version == PdfVersion::new(1, 7)
                && c.cid
                && c.transparency
                && c.max_kb == 51_200
                && c.max_pages == 2_000
                && c.encrypted_ok
                && c.embed_types == (FLAVOR_TRUETYPE | FLAVOR_TYPE1 | FLAVOR_CFF)
        }
        Err(_) => false,
    };
    let det = match &off {
        Ok(c) => format!("v={} kb={} pages={}", c.max_version.label(), c.max_kb, c.max_pages),
        Err(e) => format!("err={}", e.code()),
    };
    s.add_dyn("50101-J1-解析逐字段", ok, &det);

    // J1-3 键序无关：把同一组键倒序重排，解析结果逐字段相同。
    let reordered = "fonts=cff,type1,truetype;encrypted=yes;max-pages=2000;max-kb=51200;transparency=yes;cid=yes;pdf-version=1.7;pdf=yes;model=officelaser";
    let same = parse_report(reordered);
    let ok = match (&off, &same) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    s.add("50101-J1-键序无关", ok, if ok { "" } else { "键序影响解析结果" });

    // J1-4 版本比较：1.7 > 1.4 > 1.3 > 1.0，同版本不 newer。
    let v17 = PdfVersion::new(1, 7);
    let v14 = PdfVersion::new(1, 4);
    let v13 = PdfVersion::new(1, 3);
    let v10 = PdfVersion::new(1, 0);
    let ok = v17.is_newer_than(v14)
        && v14.is_newer_than(v13)
        && v13.is_newer_than(v10)
        && !v14.is_newer_than(v14)
        && !v13.is_newer_than(v17)
        && PdfVersion::parse("1.7") == Ok(v17)
        && PdfVersion::parse("9.9").err() == Some(PtError::Version);
    s.add("50101-J1-版本比较", ok, if ok { "" } else { "版本序错误" });

    let t = match &table {
        Ok(v) => v,
        Err(_) => return s,
    };

    // J1-5 三档 × 现代文档：一档直通、二档版本过高、三档无通道。
    let r_off = t.route(Model::OfficeLaser, &DOC_MODERN);
    let r_wg = t.route(Model::WorkgroupMono, &DOC_MODERN);
    let r_lg = t.route(Model::LegacyImpact, &DOC_MODERN);
    let ok = r_off == Ok(Route::PdfDirect)
        && r_wg == Ok(Route::Rasterize(RejectReason::VersionTooNew))
        && r_lg == Ok(Route::Rasterize(RejectReason::NoChannel));
    let det = format!("o={:?} w={:?} l={:?}", r_off, r_wg, r_lg);
    s.add_dyn("50101-J1-现代文档三档", ok, &det);

    // J1-6 三档 × 保守文档：前两档直通，三档回退。
    let r_off = t.route(Model::OfficeLaser, &DOC_SAFE);
    let r_wg = t.route(Model::WorkgroupMono, &DOC_SAFE);
    let r_lg = t.route(Model::LegacyImpact, &DOC_SAFE);
    let ok = r_off == Ok(Route::PdfDirect)
        && r_wg == Ok(Route::PdfDirect)
        && r_lg == Ok(Route::Rasterize(RejectReason::NoChannel));
    let det = format!("o={:?} w={:?} l={:?}", r_off, r_wg, r_lg);
    s.add_dyn("50101-J1-保守文档三档", ok, &det);

    // J1-7 三档 × CID 文档：二档因 CID 回退。
    let r_off = t.route(Model::OfficeLaser, &DOC_CID);
    let r_wg = t.route(Model::WorkgroupMono, &DOC_CID);
    let r_lg = t.route(Model::LegacyImpact, &DOC_CID);
    let ok = r_off == Ok(Route::PdfDirect)
        && r_wg == Ok(Route::Rasterize(RejectReason::CidFont))
        && r_lg == Ok(Route::Rasterize(RejectReason::NoChannel));
    let det = format!("o={:?} w={:?} l={:?}", r_off, r_wg, r_lg);
    s.add_dyn("50101-J1-CID文档三档", ok, &det);

    // J1-8 判定矩阵 9 格全对：文档 × 型号的期望表逐格比对。
    let docs = matrix_docs();
    let expected: [[Route; 3]; 3] = [
        // DOC_MODERN
        [
            Route::PdfDirect,
            Route::Rasterize(RejectReason::VersionTooNew),
            Route::Rasterize(RejectReason::NoChannel),
        ],
        // DOC_SAFE
        [
            Route::PdfDirect,
            Route::PdfDirect,
            Route::Rasterize(RejectReason::NoChannel),
        ],
        // DOC_CID
        [
            Route::PdfDirect,
            Route::Rasterize(RejectReason::CidFont),
            Route::Rasterize(RejectReason::NoChannel),
        ],
    ];
    let mut cells = 0usize;
    let mut wrong = 0usize;
    let models = Model::all();
    let mut d = 0usize;
    while d < docs.len() {
        if let Some(doc) = docs.get(d) {
            let mut m = 0usize;
            while m < models.len() {
                if let Some(model) = models.get(m) {
                    let got = t.route(*model, doc);
                    let want = match expected.get(d).and_then(|row| row.get(m)) {
                        Some(v) => *v,
                        None => Route::PdfDirect,
                    };
                    if got != Ok(want) {
                        wrong += 1;
                    }
                    cells += 1;
                }
                m += 1;
            }
        }
        d += 1;
    }
    let det = format!("cells={} wrong={}", cells, wrong);
    s.add_dyn("50101-J1-矩阵九格", cells == 9 && wrong == 0, &det);

    // J1-9 不支持必显式回退：全语料矩阵里每一条回退都带非空原因，直通无原因。
    let mut docs_all = matrix_docs();
    docs_all.push(DOC_ENC);
    docs_all.push(DOC_BIG);
    docs_all.push(DOC_LONG);
    docs_all.push(DOC_TRANS_LONG);
    docs_all.push(DOC_NOLIC);
    let mut led = RouteLedger::new();
    let filled = led.fill(t, &docs_all);
    let mut direct_bad = 0usize;
    let mut raster_bad = 0usize;
    let mut i = 0usize;
    while i < led.total() {
        if let Some(e) = led.entry(i) {
            match e.route {
                Route::PdfDirect => {
                    if e.route.reason().is_some() {
                        direct_bad += 1;
                    }
                }
                Route::Rasterize(r) => {
                    if r.code() <= REASON_BASE || r.message().is_empty() {
                        raster_bad += 1;
                    }
                }
            }
        }
        i += 1;
    }
    let ok = filled == Ok(24)
        && led.total() == 24
        && direct_bad == 0
        && raster_bad == 0
        && led.raster() > 0;
    let det = format!("filled={:?} total={} raster={}", filled, led.total(), led.raster());
    s.add_dyn("50101-J1-显式回退", ok, &det);

    // J1-10 优先级固定：又透明又超页的文档，二档必须先报透明（不是页数）。
    let r_tl = t.route(Model::WorkgroupMono, &DOC_TRANS_LONG);
    let ok = r_tl == Ok(Route::Rasterize(RejectReason::Transparency));
    let det = format!("{:?}", r_tl);
    s.add_dyn("50101-J1-优先级固定", ok, &det);

    // J1-11 尺寸/页数边界：等于上限直通，超一即回退且原因正确。
    let caps_wg = match t.get(Model::WorkgroupMono) {
        Some(c) => c,
        None => DeviceCaps {
            model: Model::WorkgroupMono,
            pdf_direct: true,
            max_version: PdfVersion::new(1, 4),
            cid: false,
            transparency: false,
            max_kb: 0,
            max_pages: 0,
            encrypted_ok: false,
            embed_types: 0,
        },
    };
    let mut at_limit = DOC_SAFE;
    at_limit.size_kb = caps_wg.max_kb;
    at_limit.pages = caps_wg.max_pages;
    let mut over_kb = at_limit;
    over_kb.size_kb = caps_wg.max_kb + 1;
    let mut over_pages = at_limit;
    over_pages.pages = caps_wg.max_pages + 1;
    let ok = decide(&at_limit, &caps_wg) == Route::PdfDirect
        && decide(&over_kb, &caps_wg) == Route::Rasterize(RejectReason::SizeLimit)
        && decide(&over_pages, &caps_wg) == Route::Rasterize(RejectReason::PageLimit);
    let det = format!(
        "at={:?} kb={:?} pg={:?}",
        decide(&at_limit, &caps_wg).is_direct(),
        decide(&over_kb, &caps_wg),
        decide(&over_pages, &caps_wg)
    );
    s.add_dyn("50101-J1-尺寸页数边界", ok, &det);

    // J1-12 加密与字体禁嵌：二档拒加密、两档都拒禁嵌字体。
    let r_enc_wg = t.route(Model::WorkgroupMono, &DOC_ENC);
    let r_enc_off = t.route(Model::OfficeLaser, &DOC_ENC);
    let r_nolic_off = t.route(Model::OfficeLaser, &DOC_NOLIC);
    let r_nolic_wg = t.route(Model::WorkgroupMono, &DOC_NOLIC);
    let ok = r_enc_wg == Ok(Route::Rasterize(RejectReason::Encrypted))
        && r_enc_off == Ok(Route::PdfDirect)
        && r_nolic_off == Ok(Route::Rasterize(RejectReason::FontForbidden))
        && r_nolic_wg == Ok(Route::Rasterize(RejectReason::FontForbidden));
    let det = format!(
        "enc_w={:?} enc_o={:?} nol={:?}",
        r_enc_wg, r_enc_off, r_nolic_off
    );
    s.add_dyn("50101-J1-加密与禁嵌", ok, &det);

    // J1-13 会话状态机：未开拒提交、开后可提交、成功后 Ready 且能力可取。
    let mut ses = ProbeSession::new();
    let before = ses.submit(REPORT_OFFICE);
    let opened = ses.open();
    let twice = ses.open();
    let got = ses.submit(REPORT_OFFICE);
    let ok = before.err() == Some(PtError::State)
        && opened.is_ok()
        && twice.err() == Some(PtError::State)
        && got.is_ok()
        && ses.state() == ProbeState::Ready
        && ses.attempts() == 1
        && match ses.caps() {
            Some(c) => c.model == Model::OfficeLaser && c.max_version == PdfVersion::new(1, 7),
            None => false,
        };
    let det = format!("state={:?} attempts={}", ses.state(), ses.attempts());
    s.add_dyn("50101-J1-会话状态机", ok, &det);

    // J1-14 尝试预算：三次无响应 → Failed，之后提交报 Attempts 而不是 State。
    let mut dead = session_open();
    let a1 = dead.timeout();
    let a2 = dead.timeout();
    let a3 = dead.timeout();
    let after = dead.submit(REPORT_OFFICE);
    let ok = a1 == Ok(ProbeState::Probing)
        && a2 == Ok(ProbeState::Probing)
        && a3 == Ok(ProbeState::Failed)
        && dead.state() == ProbeState::Failed
        && after.err() == Some(PtError::Attempts);
    let det = format!("a={:?}/{:?}/{:?} after={:?}", a1, a2, a3, after.err().map(|e| e.code()));
    s.add_dyn("50101-J1-尝试预算", ok, &det);

    // J1-15 路由账守恒：total = direct + raster，逐型号计数之和 == total，
    // 摘要确定（再跑一遍同账同摘要），报告含 "pdfpt"。
    let mut again = RouteLedger::new();
    let refill = again.fill(t, &docs_all);
    let mut per_model = 0usize;
    let models = Model::all();
    let mut m = 0usize;
    while m < models.len() {
        if let Some(model) = models.get(m) {
            per_model += again.count_for(*model);
        }
        m += 1;
    }
    let ok = refill == Ok(24)
        && again.total() == again.direct() + again.raster()
        && per_model == again.total()
        && again.digest() == led.digest()
        && again.report().contains("pdfpt")
        && again.reason_count(RejectReason::NoChannel) > 0;
    let det = format!(
        "total={} direct={} raster={} dg={:08x}",
        again.total(),
        again.direct(),
        again.raster(),
        again.digest()
    );
    s.add_dyn("50101-J1-路由账守恒", ok, &det);

    // J1-16 八类回退原因全部可达（没有死原因，也没有未覆盖的分支）。
    let mut reached = 0usize;
    let reasons = RejectReason::all();
    let mut r = 0usize;
    while r < reasons.len() {
        if let Some(reason) = reasons.get(r) {
            if led.reason_count(*reason) > 0 {
                reached += 1;
            }
        }
        r += 1;
    }
    // NoChannel 只在三档机上（前两档 pdf_direct 为真），其余七类都要在前两档语料
    // 里出现过 —— 所以「八类全可达」即证明三档表与语料互相咬合，没有死原因。
    let ok = reached == 8
        && t.get(Model::LegacyImpact).map(|c| !c.pdf_direct) == Some(true)
        && t.get(Model::WorkgroupMono).map(|c| c.pdf_direct) == Some(true)
        && led.reason_count(RejectReason::NoChannel) > 0;
    let det = format!("reasons={}/8", reached);
    s.add_dyn("50101-J1-原因全覆盖", ok, &det);

    // 注入核：10 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50101-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50101_checks_green() {
        let set = run_unx50101_checks();
        assert!(set.all_passed(), "UNX-F50101 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50101_legacy_has_no_channel() {
        let caps = match parse_report(REPORT_LEGACY) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(!caps.pdf_direct);
        assert_eq!(caps.max_version.code(), 0);
        assert_eq!(decide(&DOC_SAFE, &caps), Route::Rasterize(RejectReason::NoChannel));
    }
}
