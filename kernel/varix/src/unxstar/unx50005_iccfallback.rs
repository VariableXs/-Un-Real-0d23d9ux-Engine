//! UNX-F50005 ICC profile 缺失降级档（内置 sRGB 兜底 + 显式标记）（0x42cc0 段）
//!
//! 锚点：UNX-F50005-J1 降级 ×20 显式标记零静默；判据锚定 体验九 错误三要素。
//!
//! 数据段 `SEG_BASE = 0x42cc0`（独占 0x20）。诊断码闭集 = 本段 `E_EMPTY_NAME..=E_RANGE`
//! （1..=8），`FallbackError::code()` 恒为 `0x42cc0 + n`；降级**理由码**另占本段高半区
//! （`0x42cd1` 缺档 / `0x42cd2` 空文件），损坏类理由则原样保留 0x42c40 段的上游码。
//!
//! 为什么「兜底」本身要一张卡：兜底是最容易静默的路径——profile 读不出来时
//! 程序往往 `unwrap_or(sRGB)` 一句就过去了，作业照常出片，但**没人知道这一页
//! 用的是假色彩**。本模块把降级做成一条带三要素的事件（现象 / 原因 / 处置）：
//! 决定了就得落账，落了账就得能查出原始诊断码。
//!
//! 结构清单：
//! * `FallbackError`：8 个显式错误（本段 1..=8）；
//! * `DegradeReason` / `ProfileFault`：降级理由闭集，损坏类**携带上游 ColorError**；
//! * `Marker`：错误三要素（symptom/cause/action），空要素 = 静默，判据直接钉；
//! * `FallbackDecision`：一次解析的完整结论（层号 / 来源 / profile / 理由 / 标记）；
//! * `FallbackRecord` / `FallbackLedger`：降级账本，容量 20（与 J1 的 ×20 对齐），
//!   第 21 次显式 `LedgerFull`；
//! * `ProfileResolver`：解析器 + 缓存（容量 8，同名覆盖、异名满则拒绝）；
//! * `injection_core`：5 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx50001_gamutmap::{parse_profile, ColorError, IccProfile};

/// 诊断段：0x42cc0 独占 0x20（同批其余模块从 0x42ce0 起顺序占用）。
pub const SEG_BASE: u32 = 0x42cc0;

pub const E_EMPTY_NAME: u16 = 1;
pub const E_LEDGER_FULL: u16 = 2;
pub const E_CACHE_FULL: u16 = 3;
pub const E_EMPTY_SOURCE: u16 = 4;
pub const E_MARKER: u16 = 5;
pub const E_NO_BUILTIN: u16 = 6;
pub const E_BAD_FIXTURE: u16 = 7;
pub const E_RANGE: u16 = 8;

/// 降级理由码（本段高半区，与上面 1..=8 的错误码不重叠）。
pub const R_MISSING: u16 = 0x11;
pub const R_EMPTY_FILE: u16 = 0x12;

/// 降级路径的显式错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackError {
    /// 设备名/作业名为空（无法归档，拒绝解析）。
    EmptyName,
    /// 降级账本已满（容量 20）。
    LedgerFull,
    /// 缓存已满（容量 8）且是新名字。
    CacheFull,
    /// 既没有字节流也没有缓存条目，且不允许在内置档之外编造。
    EmptySource,
    /// 标记三要素不完整（静默标记）——构造期就该拦住。
    MarkerIncomplete,
    /// 内置 sRGB 兜底不可用（理论上不可达，但显式建模）。
    NoBuiltin,
    /// 测试夹具非法。
    BadFixture,
    /// 数值/下标越界。
    Range,
}

impl FallbackError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            FallbackError::EmptyName => E_EMPTY_NAME,
            FallbackError::LedgerFull => E_LEDGER_FULL,
            FallbackError::CacheFull => E_CACHE_FULL,
            FallbackError::EmptySource => E_EMPTY_SOURCE,
            FallbackError::MarkerIncomplete => E_MARKER,
            FallbackError::NoBuiltin => E_NO_BUILTIN,
            FallbackError::BadFixture => E_BAD_FIXTURE,
            FallbackError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FallbackError::EmptyName => "empty device name",
            FallbackError::LedgerFull => "fallback ledger full",
            FallbackError::CacheFull => "profile cache full",
            FallbackError::EmptySource => "no bytes and no cached profile",
            FallbackError::MarkerIncomplete => "degradation marker incomplete",
            FallbackError::NoBuiltin => "builtin sRGB unavailable",
            FallbackError::BadFixture => "malformed test fixture",
            FallbackError::Range => "index outside range",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 降级理由（携带上游诊断）
// ---------------------------------------------------------------------------

/// 降级理由闭集。`Unreadable` 原样携带 F50001 解析器的 `ColorError`——
/// 降级不等于丢诊断，页面上要说清楚「为什么没读成」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeReason {
    /// 作业根本没带 profile。
    Missing,
    /// 带了但字节数为 0（读到一半被截断在文件头之前）。
    EmptyFile,
    /// 带了解析失败（携带上游诊断）。
    Unreadable(ColorError),
}

impl DegradeReason {
    /// 理由码：损坏类保留上游码（0x42c40 段），缺档/空文件用本段高半区。
    pub fn code(self) -> u32 {
        match self {
            DegradeReason::Missing => SEG_BASE + R_MISSING as u32,
            DegradeReason::EmptyFile => SEG_BASE + R_EMPTY_FILE as u32,
            DegradeReason::Unreadable(e) => e.code(),
        }
    }

    /// 一行理由文本（进日志与标记的 `cause` 要素）。
    pub fn text(self) -> &'static str {
        match self {
            DegradeReason::Missing => "no profile supplied with the job",
            DegradeReason::EmptyFile => "profile resource is zero bytes",
            DegradeReason::Unreadable(e) => e.message(),
        }
    }

    /// 是否来自上游解析器（判据据此核对「原码保留」）。
    pub fn upstream(self) -> Option<ColorError> {
        match self {
            DegradeReason::Unreadable(e) => Some(e),
            _ => None,
        }
    }
}

/// 错误三要素（体验九口径）：现象 / 原因 / 处置。
///
/// 三要素齐全才算「显式」；任一为空即视为静默——`FallbackLedger::silent_count`
/// 数的就是这种。这种结构强制每个降级点用**人类能读**的三句话解释清楚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    /// 现象：用户/运维看到什么。
    pub symptom: &'static str,
    /// 原因：为什么走到这条路。
    pub cause: &'static str,
    /// 处置：接下来该做什么。
    pub action: &'static str,
}

impl Marker {
    /// 空标记（非降级路径用）。
    pub const fn none() -> Marker {
        Marker {
            symptom: "",
            cause: "",
            action: "",
        }
    }

    /// 三要素是否齐全。
    pub fn is_complete(&self) -> bool {
        !self.symptom.is_empty() && !self.cause.is_empty() && !self.action.is_empty()
    }

    /// 三要素是否互不相同（同一句话复用三遍等于没说）。
    pub fn is_distinct(&self) -> bool {
        self.symptom != self.cause && self.cause != self.action && self.symptom != self.action
    }
}

/// 按理由给出三要素。每个损坏类别都有自己的「处置」——把短文件和大文件
/// 混成一句「profile 坏了」是运维最讨厌的那种报错。
pub fn marker_for(reason: DegradeReason) -> Marker {
    match reason {
        DegradeReason::Missing => Marker {
            symptom: "page rendered with builtin sRGB instead of the job profile",
            cause: "job carried no ICC profile for this device",
            action: "re-embed the profile or fix the RIP resource path, then re-send",
        },
        DegradeReason::EmptyFile => Marker {
            symptom: "page rendered with builtin sRGB on a 0-byte profile",
            cause: "profile resource was truncated before the 128-byte header",
            action: "re-copy the profile from the colour server and verify its size",
        },
        DegradeReason::Unreadable(ColorError::Short) => Marker {
            symptom: "page rendered with builtin sRGB; profile shorter than header",
            cause: "file is smaller than the 132-byte ICC header plus tag count",
            action: "export the profile again from the colour server",
        },
        DegradeReason::Unreadable(ColorError::Truncated) => Marker {
            symptom: "page rendered with builtin sRGB; declared size exceeds file",
            cause: "profile size field points past the end of the resource",
            action: "re-copy the whole file; a partial download is the usual cause",
        },
        DegradeReason::Unreadable(ColorError::Magic) => Marker {
            symptom: "page rendered with builtin sRGB; acsp signature missing",
            cause: "resource is not an ICC profile or the header is damaged",
            action: "check the file type; re-export if it is really a profile",
        },
        DegradeReason::Unreadable(ColorError::TagCount) => Marker {
            symptom: "page rendered with builtin sRGB; tag table count out of range",
            cause: "tag count field is corrupt or the table runs past the file",
            action: "re-export the profile; do not hand-edit tag counts",
        },
        DegradeReason::Unreadable(ColorError::TagOffset) => Marker {
            symptom: "page rendered with builtin sRGB; a tag points outside the file",
            cause: "tag offset/size pair violates the profile bounds",
            action: "re-export the profile from the colour server",
        },
        DegradeReason::Unreadable(ColorError::NoColorant) => Marker {
            symptom: "page rendered with builtin sRGB; colorant tags missing",
            cause: "rXYZ/gXYZ/bXYZ are absent (LUT-only or device-link profile)",
            action: "supply a matrix/TRC output profile for this device",
        },
        DegradeReason::Unreadable(ColorError::Singular) => Marker {
            symptom: "page rendered with builtin sRGB; colorant matrix singular",
            cause: "the three colorant columns are linearly dependent",
            action: "re-measure the device or use the vendor profile",
        },
        DegradeReason::Unreadable(ColorError::White) => Marker {
            symptom: "page rendered with builtin sRGB; white point degenerate",
            cause: "header illuminant has a non-positive component",
            action: "re-export the profile; check the measured white",
        },
        DegradeReason::Unreadable(ColorError::Trc) => Marker {
            symptom: "page rendered with builtin sRGB; tone curve malformed",
            cause: "curv tag is shorter than its declared table",
            action: "re-export the profile with a plain gamma/sRGB TRC",
        },
        DegradeReason::Unreadable(ColorError::Intent) => Marker {
            symptom: "page rendered with builtin sRGB; header intent code illegal",
            cause: "rendering intent field is outside 0..=3",
            action: "re-export with a legal default intent",
        },
        DegradeReason::Unreadable(ColorError::Inversion) => Marker {
            symptom: "page rendered with builtin sRGB; matrix inversion unstable",
            cause: "colorant matrix is near-singular for f32 inversion",
            action: "re-measure the device or use the vendor profile",
        },
        DegradeReason::Unreadable(ColorError::Range) => Marker {
            symptom: "page rendered with builtin sRGB; a field is out of range",
            cause: "profile carries a value outside its legal interval",
            action: "re-export the profile and re-check the file checksum",
        },
        DegradeReason::Unreadable(ColorError::Empty) => Marker {
            symptom: "page rendered with builtin sRGB; parser saw no bytes",
            cause: "the resource handle returned an empty slice",
            action: "re-copy the profile and re-send the job",
        },
    }
}

// ---------------------------------------------------------------------------
// 2. 决策与账本
// ---------------------------------------------------------------------------

/// profile 的来源层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// 本次作业自带且解析成功。
    External,
    /// 命中解析缓存。
    Cached,
    /// 内置 sRGB 兜底。
    Builtin,
}

impl SourceKind {
    pub fn layer(self) -> u8 {
        match self {
            SourceKind::External => 0,
            SourceKind::Cached => 1,
            SourceKind::Builtin => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SourceKind::External => "external",
            SourceKind::Cached => "cached",
            SourceKind::Builtin => "builtin",
        }
    }
}

/// 一次解析的完整结论。
pub struct FallbackDecision {
    pub source: SourceKind,
    pub profile: IccProfile,
    /// 是否发生降级。
    pub fallback: bool,
    pub reason: Option<DegradeReason>,
    pub marker: Marker,
}

impl FallbackDecision {
    /// 层号（0 外部 / 1 缓存 / 2 兜底）。
    pub fn layer(&self) -> u8 {
        self.source.layer()
    }

    /// 是否静默：降级了却没有完整三要素。
    pub fn is_silent(&self) -> bool {
        self.fallback && !self.marker.is_complete()
    }

    /// 是否显式降级（降级 + 三要素齐 + 有理由）。
    pub fn is_explicit(&self) -> bool {
        self.fallback && self.marker.is_complete() && self.reason.is_some()
    }
}

/// 一条降级账目（不含 profile 本体，只留可审计字段）。
#[derive(Clone, Debug, PartialEq)]
pub struct FallbackRecord {
    pub slot: u8,
    pub source: SourceKind,
    pub device: String,
    pub reason: DegradeReason,
    pub marker: Marker,
}

pub const LEDGER_CAP: usize = 20;

/// 降级账本。容量 20 = J1 的「×20」，第 21 次显式 `LedgerFull`（不静默丢）。
#[derive(Clone, Debug)]
pub struct FallbackLedger {
    recs: Vec<FallbackRecord>,
    silent: usize,
}

impl FallbackLedger {
    pub fn new() -> FallbackLedger {
        FallbackLedger {
            recs: Vec::new(),
            silent: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn capacity_left(&self) -> usize {
        if self.recs.len() >= LEDGER_CAP {
            0
        } else {
            LEDGER_CAP - self.recs.len()
        }
    }

    /// 追加一条降级事件。满 → `LedgerFull`；三要素不全 → `MarkerIncomplete`
    /// （**先**拒收，而不是记账后才发现是静默）。
    pub fn push(&mut self, rec: FallbackRecord) -> Result<(), FallbackError> {
        if !rec.marker.is_complete() {
            self.silent += 1;
            return Err(FallbackError::MarkerIncomplete);
        }
        if self.recs.len() >= LEDGER_CAP {
            return Err(FallbackError::LedgerFull);
        }
        self.recs.push(rec);
        Ok(())
    }

    pub fn get(&self, i: usize) -> Option<&FallbackRecord> {
        self.recs.get(i)
    }

    /// 被拒收后再计数的静默条数（恒为 0 才算「零静默」）。
    pub fn silent_rejected(&self) -> usize {
        self.silent
    }

    /// 账内静默条数（三要素不全的已记账条目）。
    pub fn silent_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if !r.marker.is_complete() || !r.marker.is_distinct() {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 账内**显式**条数（三要素齐且三句不重复）。
    pub fn explicit_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.marker.is_complete() && r.marker.is_distinct() {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 出现过的不同理由码个数（证明诊断没被塌成一条）。
    pub fn distinct_reason_codes(&self) -> usize {
        let mut seen: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                let c = r.reason.code();
                let mut dup = false;
                let mut j = 0usize;
                while j < seen.len() {
                    if let Some(v) = seen.get(j) {
                        if *v == c {
                            dup = true;
                        }
                    }
                    j += 1;
                }
                if !dup {
                    seen.push(c);
                }
            }
            i += 1;
        }
        seen.len()
    }

    /// 所有理由码（按记账序）。
    pub fn reason_codes(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.recs.len());
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                out.push(r.reason.code());
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 3. 解析器 + 缓存
// ---------------------------------------------------------------------------

pub const CACHE_CAP: usize = 8;

#[derive(Clone)]
struct CacheEntry {
    device: String,
    profile: IccProfile,
    fallback: bool,
    reason: Option<DegradeReason>,
    marker: Marker,
}

/// profile 解析器：外部优先、缓存兜底、内置 sRGB 最后。
///
/// 缓存语义（显式写死，避免「命中就静默」）：
/// * 同名再解析**覆盖**旧条目（新文件必须能替换旧结论）；
/// * 异名且缓存满 → `CacheFull`（不 LRU 淘汰：淘汰会让审计链断掉）；
/// * 命中缓存里的**降级**条目时，返回的决策仍带原标记（标记跟着 profile 走）。
pub struct ProfileResolver {
    cache: Vec<CacheEntry>,
    ledger: FallbackLedger,
    resolutions: u32,
}

impl ProfileResolver {
    pub fn new() -> ProfileResolver {
        ProfileResolver {
            cache: Vec::new(),
            ledger: FallbackLedger::new(),
            resolutions: 0,
        }
    }

    pub fn ledger(&self) -> &FallbackLedger {
        &self.ledger
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn resolutions(&self) -> u32 {
        self.resolutions
    }

    fn cache_find(&self, device: &str) -> Option<&CacheEntry> {
        let mut i = 0usize;
        while i < self.cache.len() {
            if let Some(e) = self.cache.get(i) {
                if e.device.as_str() == device {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 写入/覆盖缓存。异名且满 → `CacheFull`。
    fn cache_put(&mut self, entry: CacheEntry) -> Result<(), FallbackError> {
        let mut i = 0usize;
        while i < self.cache.len() {
            let same = match self.cache.get(i) {
                Some(e) => e.device.as_str() == entry.device.as_str(),
                None => false,
            };
            if same {
                if let Some(slot) = self.cache.get_mut(i) {
                    *slot = entry;
                }
                return Ok(());
            }
            i += 1;
        }
        if self.cache.len() >= CACHE_CAP {
            return Err(FallbackError::CacheFull);
        }
        self.cache.push(entry);
        Ok(())
    }

    /// 解析一台设备的 profile。
    ///
    /// `bytes == Some(..)`：**总是**尝试解析（新文件不能被缓存掩盖）；
    /// 失败则降级到内置 sRGB 并落账。
    /// `bytes == None`：命中缓存则复用（降级条目仍带标记，不重复记账）；
    /// 否则按「缺档」降级并落账。
    pub fn resolve(
        &mut self,
        device: &str,
        bytes: Option<&[u8]>,
    ) -> Result<FallbackDecision, FallbackError> {
        if device.is_empty() {
            return Err(FallbackError::EmptyName);
        }
        self.resolutions = self.resolutions.saturating_add(1);
        match bytes {
            Some(raw) => {
                if raw.is_empty() {
                    let reason = DegradeReason::EmptyFile;
                    return self.degrade(device, reason, SourceKind::Builtin);
                }
                match parse_profile(raw) {
                    Ok(p) => {
                        self.cache_put(CacheEntry {
                            device: device.to_string(),
                            profile: p.clone(),
                            fallback: false,
                            reason: None,
                            marker: Marker::none(),
                        })?;
                        Ok(FallbackDecision {
                            source: SourceKind::External,
                            profile: p,
                            fallback: false,
                            reason: None,
                            marker: Marker::none(),
                        })
                    }
                    Err(e) => self.degrade(device, DegradeReason::Unreadable(e), SourceKind::Builtin),
                }
            }
            None => {
                if let Some(entry) = self.cache_find(device) {
                    // 命中即复用结论；降级条目原样带标记（不重复记账）。
                    let d = FallbackDecision {
                        source: SourceKind::Cached,
                        profile: entry.profile.clone(),
                        fallback: entry.fallback,
                        reason: entry.reason,
                        marker: entry.marker,
                    };
                    return Ok(d);
                }
                self.degrade(device, DegradeReason::Missing, SourceKind::Builtin)
            }
        }
    }

    /// 统一降级出口：内置 sRGB + 三要素 + 落账 + 覆盖缓存。
    fn degrade(
        &mut self,
        device: &str,
        reason: DegradeReason,
        source: SourceKind,
    ) -> Result<FallbackDecision, FallbackError> {
        let marker = marker_for(reason);
        if !marker.is_complete() {
            // 不可达（marker_for 覆盖全部 ColorError 变体），但保留显式失败路径。
            return Err(FallbackError::MarkerIncomplete);
        }
        let slot = self.ledger.len() as u8;
        self.ledger.push(FallbackRecord {
            slot,
            source,
            device: device.to_string(),
            reason,
            marker,
        })?;
        let profile = IccProfile::builtin_srgb();
        self.cache_put(CacheEntry {
            device: device.to_string(),
            profile: profile.clone(),
            fallback: true,
            reason: Some(reason),
            marker,
        })?;
        Ok(FallbackDecision {
            source,
            profile,
            fallback: true,
            reason: Some(reason),
            marker,
        })
    }
}

impl Default for ProfileResolver {
    fn default() -> ProfileResolver {
        ProfileResolver::new()
    }
}

// ---------------------------------------------------------------------------
// 4. 测试夹具：最小合法 ICC v2 矩阵 profile
// ---------------------------------------------------------------------------

fn zeros(len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        v.push(0u8);
        i += 1;
    }
    v
}

fn put_u32(v: &mut Vec<u8>, at: usize, val: u32) {
    if at + 4 <= v.len() {
        v[at] = (val >> 24) as u8;
        v[at + 1] = (val >> 16) as u8;
        v[at + 2] = (val >> 8) as u8;
        v[at + 3] = val as u8;
    }
}

fn put_tag(v: &mut Vec<u8>, at: usize, sig: &[u8; 4]) {
    let mut i = 0usize;
    while i < 4 {
        if at + i < v.len() {
            v[at + i] = sig[i];
        }
        i += 1;
    }
}

fn put_xyz(v: &mut Vec<u8>, at: usize, x: f32, y: f32, z: f32) {
    let vals = [x, y, z];
    let mut i = 0usize;
    while i < 3 {
        let raw = (vals[i] * 65536.0) as i32;
        put_u32(v, at + i * 4, raw as u32);
        i += 1;
    }
}

/// 构造一个 256 字节的最小合法 profile（sRGB 色料 + desc）。
/// 只用于判据的「正常路径」对照，不参与任何生产路径。
pub fn valid_profile_bytes() -> Vec<u8> {
    let total = 256usize;
    let mut v = zeros(total);
    put_u32(&mut v, 0, total as u32);
    v[8] = 0x02; // v2
    put_tag(&mut v, 12, b"mntr");
    put_tag(&mut v, 16, b"RGB ");
    put_tag(&mut v, 20, b"XYZ ");
    put_tag(&mut v, 36, b"acsp");
    put_u32(&mut v, 64, 1); // 相对比色
    put_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
    put_u32(&mut v, 128, 4);
    let colors = [
        (b'r', 168usize, 0.4360747_f32, 0.2225045_f32, 0.0139322_f32),
        (b'g', 188, 0.3850649, 0.7168786, 0.0971045),
        (b'b', 208, 0.1430804, 0.0606169, 0.7141733),
    ];
    let mut i = 0usize;
    while i < 3 {
        let e = 132 + i * 12;
        let (sig, off, x, y, z) = colors[i];
        let tag = [sig, b'X', b'Y', b'Z'];
        put_tag(&mut v, e, &tag);
        put_u32(&mut v, e + 4, off as u32);
        put_u32(&mut v, e + 8, 20);
        put_tag(&mut v, off, b"XYZ ");
        put_xyz(&mut v, off + 8, x, y, z);
        i += 1;
    }
    let e = 132 + 3 * 12;
    put_tag(&mut v, e, b"desc");
    put_u32(&mut v, e + 4, 228);
    put_u32(&mut v, e + 8, 28);
    put_tag(&mut v, 228, b"desc");
    put_u32(&mut v, 236, 13);
    let text = b"vx11 fixture";
    let mut k = 0usize;
    while k < text.len() {
        if 240 + k < v.len() {
            v[240 + k] = text[k];
        }
        k += 1;
    }
    v
}

/// 20 条坏样本（5 类 × 4 变体）。返回值保证每条都能进入降级路径。
pub fn bad_sample_bank() -> Vec<Vec<u8>> {
    let mut bank: Vec<Vec<u8>> = Vec::with_capacity(20);

    // 类 1：空 / 极短（1、64、131 字节 —— 全部短于 132 字节头部）
    bank.push(Vec::new());
    bank.push(zeros(1));
    bank.push(zeros(64));
    bank.push(zeros(131));

    // 类 2：magic 缺失（尺寸字段合法，但 36..40 不是 acsp）
    let mut i = 0usize;
    while i < 4 {
        let len = 200 + i * 8;
        let mut v = zeros(len);
        put_u32(&mut v, 0, len as u32);
        put_u32(&mut v, 128, 0);
        put_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
        bank.push(v);
        i += 1;
    }

    // 类 3：尺寸字段越界（声明 4000 但只有 200 字节）
    let mut i = 0usize;
    while i < 4 {
        let mut v = zeros(200 + i);
        put_u32(&mut v, 0, 4000);
        put_tag(&mut v, 36, b"acsp");
        put_u32(&mut v, 128, 0);
        put_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
        bank.push(v);
        i += 1;
    }

    // 类 4：标签数越界（acsp 合法，count = 数千）
    let mut i = 0usize;
    while i < 4 {
        let mut v = zeros(200);
        put_u32(&mut v, 0, 200);
        put_tag(&mut v, 36, b"acsp");
        put_u32(&mut v, 128, 999 + i as u32);
        put_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
        bank.push(v);
        i += 1;
    }

    // 类 5：无 colorant（acsp 合法、标签数 0 → NoColorant）
    let mut i = 0usize;
    while i < 4 {
        let mut v = zeros(200);
        put_u32(&mut v, 0, 200);
        put_tag(&mut v, 36, b"acsp");
        put_u32(&mut v, 128, 0);
        put_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
        bank.push(v);
        i += 1;
    }

    bank
}

// ---------------------------------------------------------------------------
// 5. 注入核：5 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 5 类坏样本 → 逐类断言显式降级。返回 (抓住数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 空设备名 → EmptyName
    let mut r = ProfileResolver::new();
    if r.resolve("", None).err() == Some(FallbackError::EmptyName) {
        caught += 1;
    }

    // 2) 空字节流 → EmptyFile 理由 + 完整三要素
    let mut r2 = ProfileResolver::new();
    let d2 = r2.resolve("press-empty", Some(&[]));
    let ok2 = match d2 {
        Ok(d) => d.fallback && d.reason == Some(DegradeReason::EmptyFile) && d.marker.is_complete(),
        Err(_) => false,
    };
    if ok2 && r2.ledger().len() == 1 {
        caught += 1;
    }

    // 3) 坏 magic → 降级且理由码 = 上游 Magic 码
    let mut bad_magic = zeros(200);
    put_u32(&mut bad_magic, 0, 200);
    put_xyz(&mut bad_magic, 68, 0.9642, 1.0, 0.8252);
    let mut r3 = ProfileResolver::new();
    let d3 = r3.resolve("press-magic", Some(&bad_magic));
    let ok3 = match d3 {
        Ok(d) => {
            d.is_explicit()
                && d.reason == Some(DegradeReason::Unreadable(ColorError::Magic))
                && d.reason.map(|x| x.code()) == Some(ColorError::Magic.code())
        }
        Err(_) => false,
    };
    if ok3 {
        caught += 1;
    }

    // 4) 账本容量封口 → LedgerFull（第 21 条），长度不变
    let mut r4 = ProfileResolver::new();
    let bank = bad_sample_bank();
    let mut k = 0usize;
    while k < LEDGER_CAP {
        if let Some(b) = bank.get(k) {
            let _ = r4.resolve("press-loop", Some(b.as_slice()));
        }
        k += 1;
    }
    let before = r4.ledger().len();
    let over = r4.resolve("press-loop", Some(bad_magic.as_slice()));
    if over.err() == Some(FallbackError::LedgerFull) && r4.ledger().len() == before {
        caught += 1;
    }

    // 5) 缓存异名封口 → CacheFull（8 个名字之后第 9 个）
    let mut r5 = ProfileResolver::new();
    let good = valid_profile_bytes();
    let mut i = 0usize;
    let mut filled = 0usize;
    while i < CACHE_CAP {
        let name = format!("dev-{}", i);
        if r5.resolve(name.as_str(), Some(good.as_slice())).is_ok() {
            filled += 1;
        }
        i += 1;
    }
    let over5 = r5.resolve("dev-over", Some(good.as_slice()));
    if filled == CACHE_CAP && over5.err() == Some(FallbackError::CacheFull) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 6. 判据
// ---------------------------------------------------------------------------

/// UNX-F50005 判据集（16 条）。锚点：降级 ×20 显式标记零静默。
pub fn run_unx50005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50005");
    let bank = bad_sample_bank();

    // J1-1 样本规模：20 条坏样本（5 类 × 4 变体）
    let ok = bank.len() == 20;
    s.add("50005-J1-样本20", ok, "坏样本表长度不是 20");

    // J1-2 锚点：20 次降级全部落账、全部 fallback=true
    let mut r = ProfileResolver::new();
    let mut all_fallback = true;
    let mut i = 0usize;
    while i < bank.len() {
        if let Some(b) = bank.get(i) {
            match r.resolve("press-a", Some(b.as_slice())) {
                Ok(d) => {
                    if !d.fallback {
                        all_fallback = false;
                    }
                }
                Err(_) => all_fallback = false,
            }
        }
        i += 1;
    }
    let ok = all_fallback && r.ledger().len() == 20;
    let det = format!("records={} all_fallback={}", r.ledger().len(), all_fallback);
    s.add_dyn("50005-J1-降级20全落账", ok, &det);

    // J1-3 零静默：静默条数 = 0，显式条数 = 20，且没有因缺要素被拒收的
    let ok = r.ledger().silent_count() == 0
        && r.ledger().explicit_count() == 20
        && r.ledger().silent_rejected() == 0;
    let det = format!(
        "silent={} explicit={} rejected={}",
        r.ledger().silent_count(),
        r.ledger().explicit_count(),
        r.ledger().silent_rejected()
    );
    s.add_dyn("50005-J1-零静默", ok, &det);

    // J1-4 原诊断保留：20 条里至少 4 个不同理由码，且每条码都落在合法段
    let codes = r.ledger().reason_codes();
    let mut bad_code = 0usize;
    let mut k = 0usize;
    while k < codes.len() {
        if let Some(c) = codes.get(k) {
            // 合法段：上游 0x42c40 或本段高半区 0x42cd1/0x42cd2
            let upstream = *c >= 0x42c40 && *c < 0x42c40 + 0x20;
            let local = *c == SEG_BASE + R_MISSING as u32 || *c == SEG_BASE + R_EMPTY_FILE as u32;
            if !upstream && !local {
                bad_code += 1;
            }
        }
        k += 1;
    }
    let distinct = r.ledger().distinct_reason_codes();
    let ok = distinct >= 4 && bad_code == 0;
    let det = format!("distinct={} bad_code={}", distinct, bad_code);
    s.add_dyn("50005-J1-原码保留", ok, &det);

    // J1-5 兜底档就是内置 sRGB（矩阵逐位相等，不是「另一个 sRGB」）
    let builtin = IccProfile::builtin_srgb();
    let mut same_matrix = true;
    let mut m = 0usize;
    while m < 20 {
        if let Some(rec) = r.ledger().get(m) {
            if !rec.marker.is_complete() {
                same_matrix = false;
            }
        }
        m += 1;
    }
    // 用独立解析器探测：r 的账本此刻已满 20 条，再借它探测会先撞 LedgerFull，
    // 测到的就不是「兜底 profile 是什么」而是「账本还能不能写」了。
    let mut r_builtin = ProfileResolver::new();
    let probe = r_builtin.resolve("press-b", None);
    let matrix_ok = match probe {
        Ok(d) => d.profile.rgb_to_xyz == builtin.rgb_to_xyz && d.profile.xyz_to_rgb == builtin.xyz_to_rgb,
        Err(_) => false,
    };
    let ok = same_matrix && matrix_ok;
    s.add("50005-J1-兜底即内置", ok, "兜底 profile 与内置档不一致");

    // J1-6 合法 profile 不标记降级：External / fallback=false / 空标记
    let good = valid_profile_bytes();
    let mut r2 = ProfileResolver::new();
    let d6 = r2.resolve("press-good", Some(good.as_slice()));
    let ok = match d6 {
        Ok(d) => {
            !d.fallback
                && d.source == SourceKind::External
                && d.layer() == 0
                && !d.marker.is_complete()
                && !d.is_silent()
                && d.reason.is_none()
        }
        Err(_) => false,
    };
    s.add("50005-J1-合法不降级", ok, "合法 profile 被误判降级");

    // J1-7 缓存命中：重复解析不新增账目；降级条目的标记仍随结果返回
    let before = r2.ledger().len();
    let hit_fallback = r.resolve("press-a", None);
    let hit_good = r2.resolve("press-good", None);
    let ok = match (hit_fallback, hit_good) {
        (Ok(a), Ok(b)) => {
            a.fallback
                && a.source == SourceKind::Cached
                && a.layer() == 1
                && a.marker.is_complete()
                && a.reason.is_some()
                && !b.fallback
                && b.source == SourceKind::Cached
                && r2.ledger().len() == before
        }
        _ => false,
    };
    s.add("50005-J1-缓存保持标记", ok, "缓存命中丢了降级标记");

    // J1-8 缺档事件：无字节流且无缓存 → Missing 理由 + 落账
    let mut r3 = ProfileResolver::new();
    let d8 = r3.resolve("press-ghost", None);
    let ok = match d8 {
        Ok(d) => {
            d.fallback
                && d.reason == Some(DegradeReason::Missing)
                && d.marker.is_complete()
                && d.reason.map(|x| x.code()) == Some(SEG_BASE + R_MISSING as u32)
        }
        Err(_) => false,
    };
    let ok = ok && r3.ledger().len() == 1;
    s.add("50005-J1-缺档必记", ok, "缺档未产生显式事件");

    // J1-9 账本封口：第 21 条显式 LedgerFull，长度与静默计数不变
    let len20 = r.ledger().len();
    let over = r.resolve("press-a", Some(bank.first().map(|v| v.as_slice()).unwrap_or(&[])));
    let ok = len20 == 20
        && over.err() == Some(FallbackError::LedgerFull)
        && r.ledger().len() == 20
        && r.ledger().silent_rejected() == 0;
    s.add("50005-J1-账本封口", ok, "第 21 条降级被静默吞掉");

    // J1-10 缓存异名封口：8 个名字之后第 9 个 → CacheFull
    let mut r4 = ProfileResolver::new();
    let mut filled = 0usize;
    let mut n = 0usize;
    while n < CACHE_CAP {
        let name = format!("d{}", n);
        if r4.resolve(name.as_str(), Some(good.as_slice())).is_ok() {
            filled += 1;
        }
        n += 1;
    }
    let over4 = r4.resolve("d-over", Some(good.as_slice()));
    let ok = filled == CACHE_CAP && over4.err() == Some(FallbackError::CacheFull) && r4.cache_len() == CACHE_CAP;
    s.add("50005-J1-缓存封口", ok, "缓存容量未封口");

    // J1-11 同名覆盖：同名再解析成功会把降级条目换成正常条目（新文件有效）
    let mut r5 = ProfileResolver::new();
    let _ = r5.resolve("press-x", Some(bank.first().map(|v| v.as_slice()).unwrap_or(&[])));
    let d11 = r5.resolve("press-x", Some(good.as_slice()));
    let after = r5.resolve("press-x", None);
    let ok = match (d11, after) {
        (Ok(a), Ok(b)) => !a.fallback && a.source == SourceKind::External && !b.fallback && r5.cache_len() == 1,
        _ => false,
    };
    s.add("50005-J1-同名覆盖", ok, "同名覆盖未生效（旧降级结论粘住）");

    // J1-12 三要素质量：每条标记三句互不相同（不是一句话复读三遍）
    let mut distinct_triples = 0usize;
    let mut q = 0usize;
    while q < r.ledger().len() {
        if let Some(rec) = r.ledger().get(q) {
            if rec.marker.is_distinct() {
                distinct_triples += 1;
            }
        }
        q += 1;
    }
    let ok = distinct_triples == 20;
    let det = format!("distinct_triples={}/20", distinct_triples);
    s.add_dyn("50005-J1-三要素质量", ok, &det);

    // J1-13 决策层号与来源一致（0 External / 1 Cached / 2 Builtin）
    let mut layer_ok = true;
    let mut z = 0usize;
    while z < r.ledger().len() {
        if let Some(rec) = r.ledger().get(z) {
            let expect = match rec.source {
                SourceKind::External => 0,
                SourceKind::Cached => 1,
                SourceKind::Builtin => 2,
            };
            if rec.source.layer() != expect {
                layer_ok = false;
            }
            // 记账的降级事件必然是兜底层（外部/缓存层不产生新事件）。
            if rec.source != SourceKind::Builtin {
                layer_ok = false;
            }
        }
        z += 1;
    }
    s.add("50005-J1-层号一致", layer_ok, "来源与层号错配");

    // J1-14 确定性：同一坏样本两次降级得到同一理由码与同一标记
    let mut ra = ProfileResolver::new();
    let mut rb = ProfileResolver::new();
    let b14 = bank.get(10).map(|v| v.as_slice()).unwrap_or(&[]);
    let da = ra.resolve("det", Some(b14));
    let db = rb.resolve("det", Some(b14));
    let ok = match (da, db) {
        (Ok(x), Ok(y)) => {
            x.reason.map(|r0| r0.code()) == y.reason.map(|r1| r1.code())
                && x.marker == y.marker
                && x.profile.rgb_to_xyz == y.profile.rgb_to_xyz
        }
        _ => false,
    };
    s.add("50005-J1-确定性", ok, "同输入两次降级结论不同");

    // J1-15 空名拒绝且不产生事件（拒绝在解析之前，账本保持干净）
    let mut r6 = ProfileResolver::new();
    let e = r6.resolve("", Some(good.as_slice()));
    let ok = e.err() == Some(FallbackError::EmptyName) && r6.ledger().len() == 0 && r6.resolutions() == 0;
    s.add("50005-J1-空名不记账", ok, "空名产生了账目或计数");

    // 注入核：5 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50005-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50005_checks_green() {
        let set = run_unx50005_checks();
        assert!(set.all_passed(), "UNX-F50005 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50005_twenty_degradations_are_all_marked() {
        let mut r = ProfileResolver::new();
        for b in bad_sample_bank() {
            let d = r.resolve("press-a", Some(b.as_slice())).unwrap();
            assert!(d.fallback);
            assert!(d.marker.is_complete());
        }
        assert_eq!(r.ledger().len(), 20);
        assert_eq!(r.ledger().silent_count(), 0);
    }

    #[test]
    fn unx50005_valid_profile_is_not_degraded() {
        let good = valid_profile_bytes();
        let mut r = ProfileResolver::new();
        let d = r.resolve("press", Some(good.as_slice())).unwrap();
        assert!(!d.fallback);
        assert_eq!(d.source, SourceKind::External);
        assert_eq!(r.ledger().len(), 0);
    }

    #[test]
    fn unx50005_codes_stay_in_segment() {
        let all = [
            FallbackError::EmptyName,
            FallbackError::LedgerFull,
            FallbackError::CacheFull,
            FallbackError::EmptySource,
            FallbackError::MarkerIncomplete,
            FallbackError::NoBuiltin,
            FallbackError::BadFixture,
            FallbackError::Range,
        ];
        for e in all {
            let c = e.code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x20, "code {:#x}", c);
            assert!(!e.message().is_empty());
        }
        // 段内码必须两两不同（同段复用会毁掉审计定位）。
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                assert!(all[i].code() != all[j].code(), "dup code {:#x}", all[i].code());
                j += 1;
            }
            i += 1;
        }
    }
}
