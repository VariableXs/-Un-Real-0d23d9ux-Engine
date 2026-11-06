//! UNX-F50091 字体错误码族（15 条：坏字体/许可拒绝/下载超时等）（0x43780 段）
//!
//! 锚点：UNX-F50091-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43780`：
//! * 字体错误码族占 `SEG_BASE + 1 ..= SEG_BASE + 15`（15 条，闭集）；
//! * 注册表/账本自身的错误占 `SEG_BASE + 0x10 + 1 ..= + 8`，与族码不重叠。
//!
//! 「零漂移」不是一句口号：错误码经常被持久化到日志与遥测，任何一个码值
//! 改动都会让历史数据错位。本模块因此把码值、族、严重度、可重试性与处置
//! 全部写成**纯函数**，再由 [`ErrorDictionary`] 从表里加载后逐项复算比对
//! （[`ErrorDictionary::verify_no_drift`]）。表里任何一栏与纯函数不一致，
//! 加载阶段就按 [`RegistryError::Drift`] 拒绝，而不是等到现场误读。
//!
//! 结构清单：
//! * [`FontError`] 15 条错误码族（`code`/`message`/`family`/`severity`/`retryable`/`remedy`）；
//! * [`ErrFamily`]/[`Severity`]/[`Remedy`] 三维分类与处置闭集；
//! * [`ErrorDictionary`] 错误码字典：表加载、码值漂移检测、唯一性/顺序/完整性；
//! * [`ErrorLedger`] 频次账本：15 槽计数 + 族/严重度汇总 + 守恒审计；
//! * [`ErrorBudget`] 族级预算：超限即 Overflow，并显式记 trip；
//! * 注入核 [`injection_core`]：九类坏样本逐类断言具体诊断码。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43780 独占 0x20）。
pub const SEG_BASE: u32 = 0x43780;
/// 注册表错误子区间起点（族码占 1..=15，注册表码从 +0x10 起）。
pub const REG_BASE: u32 = SEG_BASE + 0x10;

/// 错误码族共 15 条。
pub const FONT_ERR_COUNT: usize = 15;
/// 族分类共 8 类。
pub const FAMILY_COUNT: usize = 8;
/// 字典表容量上限（15 条 + 1 条余量，多余即为漂移/灌表）。
pub const TABLE_CAP: usize = 16;

/// 字体错误码族（15 条）。`code()` 返回 `SEG_BASE + n`，n 即族内序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    BadFont,
    LicenseRefused,
    DownloadTimeout,
    DownloadHttp,
    SubsetFailed,
    MetricsMismatch,
    GdiCosignMissing,
    SubstituteMissing,
    SubstituteCycle,
    GlyphMissing,
    EmbedForbidden,
    Decompress,
    TableChecksum,
    CacheEvict,
    ResourceExhausted,
}

impl FontError {
    /// 族内序号 1..=15。码值与序号一一绑定，任何重排都是漂移。
    pub fn offset(self) -> u16 {
        match self {
            FontError::BadFont => 1,
            FontError::LicenseRefused => 2,
            FontError::DownloadTimeout => 3,
            FontError::DownloadHttp => 4,
            FontError::SubsetFailed => 5,
            FontError::MetricsMismatch => 6,
            FontError::GdiCosignMissing => 7,
            FontError::SubstituteMissing => 8,
            FontError::SubstituteCycle => 9,
            FontError::GlyphMissing => 10,
            FontError::EmbedForbidden => 11,
            FontError::Decompress => 12,
            FontError::TableChecksum => 13,
            FontError::CacheEvict => 14,
            FontError::ResourceExhausted => 15,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn from_code(code: u32) -> Result<FontError, RegistryError> {
        let mut i = 0usize;
        while i < ALL.len() {
            if let Some(e) = ALL.get(i) {
                if e.code() == code {
                    return Ok(*e);
                }
            }
            i += 1;
        }
        Err(RegistryError::BadCode)
    }

    pub fn message(self) -> &'static str {
        match self {
            FontError::BadFont => "font file is malformed",
            FontError::LicenseRefused => "font license refuses this use",
            FontError::DownloadTimeout => "font download timed out",
            FontError::DownloadHttp => "font download returned http error",
            FontError::SubsetFailed => "glyph subsetting failed",
            FontError::MetricsMismatch => "rendered metrics disagree with source",
            FontError::GdiCosignMissing => "font/gdi co-sign missing",
            FontError::SubstituteMissing => "no substitution registered",
            FontError::SubstituteCycle => "substitution chain loops",
            FontError::GlyphMissing => "requested glyph absent",
            FontError::EmbedForbidden => "embedding forbidden by license",
            FontError::Decompress => "font stream decompression failed",
            FontError::TableChecksum => "font table checksum mismatch",
            FontError::CacheEvict => "font cache entry evicted",
            FontError::ResourceExhausted => "font resource budget exhausted",
        }
    }

    pub fn family(self) -> ErrFamily {
        match self {
            FontError::BadFont | FontError::Decompress | FontError::TableChecksum => ErrFamily::Parse,
            FontError::LicenseRefused | FontError::EmbedForbidden => ErrFamily::License,
            FontError::DownloadTimeout | FontError::DownloadHttp => ErrFamily::Network,
            FontError::SubsetFailed | FontError::GlyphMissing => ErrFamily::Subset,
            FontError::MetricsMismatch => ErrFamily::Metrics,
            FontError::GdiCosignMissing => ErrFamily::Pipeline,
            FontError::SubstituteMissing | FontError::SubstituteCycle => ErrFamily::Substitution,
            FontError::CacheEvict | FontError::ResourceExhausted => ErrFamily::Storage,
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            FontError::CacheEvict => Severity::Warning,
            FontError::BadFont
            | FontError::Decompress
            | FontError::TableChecksum
            | FontError::ResourceExhausted => Severity::Fatal,
            _ => Severity::Error,
        }
    }

    /// 可重试集合固定为四条：网络超时/HTTP、缓存淘汰、资源暂满。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            FontError::DownloadTimeout
                | FontError::DownloadHttp
                | FontError::CacheEvict
                | FontError::ResourceExhausted
        )
    }

    pub fn remedy(self) -> Remedy {
        match self {
            FontError::BadFont => Remedy::Abort,
            FontError::LicenseRefused => Remedy::Substitute,
            FontError::DownloadTimeout | FontError::DownloadHttp => Remedy::Retry,
            FontError::SubsetFailed => Remedy::Reresolve,
            FontError::MetricsMismatch => Remedy::Reresolve,
            FontError::GdiCosignMissing => Remedy::Reresolve,
            FontError::SubstituteMissing => Remedy::BuiltinFallback,
            FontError::SubstituteCycle => Remedy::Abort,
            FontError::GlyphMissing => Remedy::Substitute,
            FontError::EmbedForbidden => Remedy::BuiltinFallback,
            FontError::Decompress => Remedy::Reresolve,
            FontError::TableChecksum => Remedy::Abort,
            FontError::CacheEvict => Remedy::Evict,
            FontError::ResourceExhausted => Remedy::Retry,
        }
    }
}

/// 全部 15 条错误码。顺序即码序，判据与字典加载都以它为准。
pub const ALL: [FontError; FONT_ERR_COUNT] = [
    FontError::BadFont,
    FontError::LicenseRefused,
    FontError::DownloadTimeout,
    FontError::DownloadHttp,
    FontError::SubsetFailed,
    FontError::MetricsMismatch,
    FontError::GdiCosignMissing,
    FontError::SubstituteMissing,
    FontError::SubstituteCycle,
    FontError::GlyphMissing,
    FontError::EmbedForbidden,
    FontError::Decompress,
    FontError::TableChecksum,
    FontError::CacheEvict,
    FontError::ResourceExhausted,
];

/// 错误族。族级预算与告警聚合按此分区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrFamily {
    Parse,
    License,
    Network,
    Subset,
    Metrics,
    Pipeline,
    Substitution,
    Storage,
}

impl ErrFamily {
    pub fn code(self) -> u8 {
        match self {
            ErrFamily::Parse => 0,
            ErrFamily::License => 1,
            ErrFamily::Network => 2,
            ErrFamily::Subset => 3,
            ErrFamily::Metrics => 4,
            ErrFamily::Pipeline => 5,
            ErrFamily::Substitution => 6,
            ErrFamily::Storage => 7,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ErrFamily::Parse => "parse",
            ErrFamily::License => "license",
            ErrFamily::Network => "network",
            ErrFamily::Subset => "subset",
            ErrFamily::Metrics => "metrics",
            ErrFamily::Pipeline => "pipeline",
            ErrFamily::Substitution => "substitution",
            ErrFamily::Storage => "storage",
        }
    }
}

/// 严重度三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
    Fatal,
}

impl Severity {
    pub fn code(self) -> u8 {
        match self {
            Severity::Warning => 0,
            Severity::Error => 1,
            Severity::Fatal => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }
}

/// 处置动作闭集。每条错误必须有且只有一个处置，不允许“未映射”。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    Retry,
    Reresolve,
    Substitute,
    BuiltinFallback,
    Abort,
    Evict,
}

impl Remedy {
    pub fn code(self) -> u8 {
        match self {
            Remedy::Retry => 0,
            Remedy::Reresolve => 1,
            Remedy::Substitute => 2,
            Remedy::BuiltinFallback => 3,
            Remedy::Abort => 4,
            Remedy::Evict => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Remedy::Retry => "retry",
            Remedy::Reresolve => "reresolve",
            Remedy::Substitute => "substitute",
            Remedy::BuiltinFallback => "builtin-fallback",
            Remedy::Abort => "abort",
            Remedy::Evict => "evict",
        }
    }
}

/// 注册表/账本错误。码值落在 `REG_BASE + 1 ..= REG_BASE + 8`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    Empty,
    Capacity,
    Duplicate,
    Drift,
    Unmapped,
    Order,
    BadCode,
    Overflow,
}

impl RegistryError {
    pub fn code(self) -> u32 {
        let n = match self {
            RegistryError::Empty => 1,
            RegistryError::Capacity => 2,
            RegistryError::Duplicate => 3,
            RegistryError::Drift => 4,
            RegistryError::Unmapped => 5,
            RegistryError::Order => 6,
            RegistryError::BadCode => 7,
            RegistryError::Overflow => 8,
        };
        REG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            RegistryError::Empty => "error table is empty",
            RegistryError::Capacity => "error table exceeds capacity",
            RegistryError::Duplicate => "duplicate error code entry",
            RegistryError::Drift => "code/field drifted from canonical mapping",
            RegistryError::Unmapped => "one or more error codes unmapped",
            RegistryError::Order => "error table not in ascending code order",
            RegistryError::BadCode => "code outside the font error family",
            RegistryError::Overflow => "family error budget exceeded",
        }
    }
}

/// 字典表的一行。持久化层只认这四列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub code: u32,
    pub error: FontError,
    pub remedy: Remedy,
}

/// 字典条目。严重度/可重试性由错误码纯函数复算，表里不另存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorEntry {
    pub error: FontError,
    pub remedy: Remedy,
    pub severity: Severity,
    pub retryable: bool,
}

/// 错误码字典。加载时就把漂移、重复、缺项、乱序全部挡在门外。
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorDictionary {
    entries: Vec<ErrorEntry>,
}

impl ErrorDictionary {
    /// 从持久化表加载。校验顺序：空 → 超容 → 码漂移 → 重复 → 乱序 → 缺项。
    pub fn from_table(rows: &[TableRow]) -> Result<ErrorDictionary, RegistryError> {
        if rows.is_empty() {
            return Err(RegistryError::Empty);
        }
        if rows.len() > TABLE_CAP {
            return Err(RegistryError::Capacity);
        }
        let mut entries: Vec<ErrorEntry> = Vec::with_capacity(rows.len());
        let mut prev: Option<u32> = None;
        let mut i = 0usize;
        while i < rows.len() {
            let row = match rows.get(i) {
                Some(r) => *r,
                None => return Err(RegistryError::Empty),
            };
            if row.code != row.error.code() {
                return Err(RegistryError::Drift);
            }
            let mut dup = false;
            let mut j = 0usize;
            while j < entries.len() {
                if let Some(seen) = entries.get(j) {
                    if seen.error == row.error {
                        dup = true;
                    }
                }
                j += 1;
            }
            if dup {
                return Err(RegistryError::Duplicate);
            }
            if let Some(p) = prev {
                if row.code <= p {
                    return Err(RegistryError::Order);
                }
            }
            entries.push(ErrorEntry {
                error: row.error,
                remedy: row.remedy,
                severity: row.error.severity(),
                retryable: row.error.retryable(),
            });
            prev = Some(row.code);
            i += 1;
        }
        if entries.len() != FONT_ERR_COUNT {
            return Err(RegistryError::Unmapped);
        }
        Ok(ErrorDictionary { entries })
    }

    /// 规范字典：由 [`FontError::remedy`] 纯函数生成，必然零漂移。
    pub fn canonical() -> Result<ErrorDictionary, RegistryError> {
        let mut rows: Vec<TableRow> = Vec::with_capacity(FONT_ERR_COUNT);
        let mut i = 0usize;
        while i < ALL.len() {
            if let Some(e) = ALL.get(i) {
                rows.push(TableRow {
                    code: e.code(),
                    error: *e,
                    remedy: e.remedy(),
                });
            }
            i += 1;
        }
        ErrorDictionary::from_table(&rows)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn lookup(&self, code: u32) -> Option<&ErrorEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.error.code() == code {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 零漂移校验：15 条齐全、码序连续、每一栏与纯函数复算一致。
    pub fn verify_no_drift(&self) -> bool {
        if self.entries.len() != FONT_ERR_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return false,
            };
            let expect_code = SEG_BASE + (i as u32) + 1;
            if e.error.code() != expect_code
                || e.error.offset() as usize != i + 1
                || e.remedy != e.error.remedy()
                || e.severity != e.error.severity()
                || e.retryable != e.error.retryable()
            {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn summary(&self) -> String {
        format!(
            "entries={} drift_free={}",
            self.entries.len(),
            self.verify_no_drift()
        )
    }
}

/// 按码查处置。未登记即 Unmapped，不做静默默认动作。
pub fn remedy_for(code: u32, dict: &ErrorDictionary) -> Result<Remedy, RegistryError> {
    match dict.lookup(code) {
        Some(e) => Ok(e.remedy),
        None => Err(RegistryError::Unmapped),
    }
}

/// 频次账本：15 槽 + 族/严重度二维汇总。
#[derive(Clone, Debug)]
pub struct ErrorLedger {
    counts: [u32; FONT_ERR_COUNT],
    family: [u32; FAMILY_COUNT],
    severity: [u32; 3],
    pub total: u32,
    pub retryable: u32,
    pub rejected: u32,
}

impl ErrorLedger {
    pub fn new() -> ErrorLedger {
        ErrorLedger {
            counts: [0; FONT_ERR_COUNT],
            family: [0; FAMILY_COUNT],
            severity: [0; 3],
            total: 0,
            retryable: 0,
            rejected: 0,
        }
    }

    /// 记一次错误。槽位全部由独立枚举派生，不存在越界写入。
    pub fn record(&mut self, err: FontError) {
        self.total += 1;
        if let Some(c) = self.counts.get_mut(err.offset() as usize - 1) {
            *c += 1;
        }
        if let Some(c) = self.family.get_mut(err.family().code() as usize) {
            *c += 1;
        }
        if let Some(c) = self.severity.get_mut(err.severity().code() as usize) {
            *c += 1;
        }
        if err.retryable() {
            self.retryable += 1;
        }
    }

    /// 按码记账。族外码值拒绝并计入 rejected，账本不被污染。
    pub fn record_code(&mut self, code: u32) -> Result<FontError, RegistryError> {
        match FontError::from_code(code) {
            Ok(e) => {
                self.record(e);
                Ok(e)
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }

    pub fn count_of(&self, err: FontError) -> u32 {
        match self.counts.get(err.offset() as usize - 1) {
            Some(c) => *c,
            None => 0,
        }
    }

    pub fn family_count(&self, fam: ErrFamily) -> u32 {
        match self.family.get(fam.code() as usize) {
            Some(c) => *c,
            None => 0,
        }
    }

    pub fn severity_count(&self, sev: Severity) -> u32 {
        match self.severity.get(sev.code() as usize) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 守恒：码槽之和 = 族之和 = 严重度之和 = total。
    pub fn conservation_ok(&self) -> bool {
        let mut code_sum = 0u32;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                code_sum += *c;
            }
            i += 1;
        }
        let mut fam_sum = 0u32;
        let mut j = 0usize;
        while j < self.family.len() {
            if let Some(c) = self.family.get(j) {
                fam_sum += *c;
            }
            j += 1;
        }
        let mut sev_sum = 0u32;
        let mut k = 0usize;
        while k < self.severity.len() {
            if let Some(c) = self.severity.get(k) {
                sev_sum += *c;
            }
            k += 1;
        }
        code_sum == self.total && fam_sum == self.total && sev_sum == self.total
    }

    pub fn summary(&self) -> String {
        format!(
            "total={} retryable={} rejected={} family_net={}",
            self.total,
            self.retryable,
            self.rejected,
            self.family_count(ErrFamily::Network)
        )
    }
}

/// 族级预算。limit 是允许次数，第 limit+1 次触发 Overflow 并记 trip。
#[derive(Clone, Debug)]
pub struct ErrorBudget {
    pub limit: u16,
    seen: [u16; FAMILY_COUNT],
    pub tripped: [bool; FAMILY_COUNT],
}

impl ErrorBudget {
    pub fn new(limit: u16) -> ErrorBudget {
        ErrorBudget {
            limit,
            seen: [0; FAMILY_COUNT],
            tripped: [false; FAMILY_COUNT],
        }
    }

    /// 观察一次错误。族内累计超过 limit 即 Overflow（显式 trip，不自动清除）。
    pub fn observe(&mut self, err: FontError) -> Result<(), RegistryError> {
        let idx = err.family().code() as usize;
        let cur = match self.seen.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if cur >= self.limit {
            if let Some(t) = self.tripped.get_mut(idx) {
                *t = true;
            }
            return Err(RegistryError::Overflow);
        }
        if let Some(c) = self.seen.get_mut(idx) {
            *c += 1;
        }
        Ok(())
    }

    pub fn seen_in(&self, fam: ErrFamily) -> u16 {
        match self.seen.get(fam.code() as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn tripped_families(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.tripped.len() {
            if let Some(t) = self.tripped.get(i) {
                if *t {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 注入核：九类坏样本
// ---------------------------------------------------------------------------

fn row(err: FontError) -> TableRow {
    TableRow {
        code: err.code(),
        error: err,
        remedy: err.remedy(),
    }
}

/// 注入核。返回（被抓数, 总数），每条对照具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1. 空表
    if ErrorDictionary::from_table(&[]).err() == Some(RegistryError::Empty) {
        caught += 1;
    }
    // 2. 超容（17 行）
    let mut over: Vec<TableRow> = Vec::with_capacity(17);
    let mut i = 0usize;
    while i < 17 {
        over.push(row(FontError::BadFont));
        i += 1;
    }
    if ErrorDictionary::from_table(&over).err() == Some(RegistryError::Capacity) {
        caught += 1;
    }
    // 3. 码值漂移（合法错误码被人为改了一位）
    let mut drifted: Vec<TableRow> = Vec::with_capacity(1);
    let mut d = row(FontError::DownloadTimeout);
    d.code = SEG_BASE + 14;
    drifted.push(d);
    if ErrorDictionary::from_table(&drifted).err() == Some(RegistryError::Drift) {
        caught += 1;
    }
    // 4. 重复条目
    let mut dup: Vec<TableRow> = Vec::with_capacity(2);
    dup.push(row(FontError::BadFont));
    dup.push(row(FontError::BadFont));
    if ErrorDictionary::from_table(&dup).err() == Some(RegistryError::Duplicate) {
        caught += 1;
    }
    // 5. 缺项（14 行）
    let mut short: Vec<TableRow> = Vec::with_capacity(FONT_ERR_COUNT);
    let mut k = 0usize;
    while k + 1 < ALL.len() {
        if let Some(e) = ALL.get(k) {
            short.push(row(*e));
        }
        k += 1;
    }
    if ErrorDictionary::from_table(&short).err() == Some(RegistryError::Unmapped) {
        caught += 1;
    }
    // 6. 乱序（3 号在 1 号之前）
    let mut unsorted: Vec<TableRow> = Vec::with_capacity(2);
    unsorted.push(row(FontError::DownloadTimeout));
    unsorted.push(row(FontError::BadFont));
    if ErrorDictionary::from_table(&unsorted).err() == Some(RegistryError::Order) {
        caught += 1;
    }
    // 7. 族外码值
    if FontError::from_code(SEG_BASE).err() == Some(RegistryError::BadCode) {
        caught += 1;
    }
    // 8. 预算超限
    let mut budget = ErrorBudget::new(1);
    let a = budget.observe(FontError::DownloadTimeout).is_ok();
    if a && budget.observe(FontError::DownloadHttp).err() == Some(RegistryError::Overflow) {
        caught += 1;
    }
    // 9. 未映射处置
    let dict = match ErrorDictionary::canonical() {
        Ok(d) => d,
        Err(_) => ErrorDictionary { entries: Vec::new() },
    };
    let unknown = remedy_for(SEG_BASE + 0x20, &dict);
    if unknown.err() == Some(RegistryError::Unmapped) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50091_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50091");

    // J1-1 错误码 ×15 零漂移：码值连续、序号绑定、from_code 往返
    let mut ok = true;
    let mut i = 0usize;
    while i < ALL.len() {
        if let Some(e) = ALL.get(i) {
            let expect = SEG_BASE + (i as u32) + 1;
            if e.code() != expect || e.offset() as usize != i + 1 {
                ok = false;
            }
            if FontError::from_code(expect) != Ok(*e) {
                ok = false;
            }
            if e.message().is_empty() {
                ok = false;
            }
        }
        i += 1;
    }
    let det = format!("codes={}..{}", SEG_BASE + 1, SEG_BASE + FONT_ERR_COUNT as u32);
    s.add_dyn("50091-J1-错误码×15零漂移", ok, &det);

    // J1-2 码值两两互异且恰好在段内
    let mut distinct = 0usize;
    let mut a = 0usize;
    while a < ALL.len() {
        if let Some(e) = ALL.get(a) {
            let mut collide = false;
            let mut b = 0usize;
            while b < ALL.len() {
                if b != a {
                    if let Some(o) = ALL.get(b) {
                        if o.code() == e.code() {
                            collide = true;
                        }
                    }
                }
                b += 1;
            }
            if !collide && e.code() > SEG_BASE && e.code() <= SEG_BASE + 0x0f {
                distinct += 1;
            }
        }
        a += 1;
    }
    let ok = distinct == FONT_ERR_COUNT;
    let det = format!("distinct={}", distinct);
    s.add_dyn("50091-J1-码值闭集", ok, &det);

    // J1-3 消息唯一且非空（现场日志靠它 grep，重复消息等于无信息）
    let mut unique = 0usize;
    let mut m = 0usize;
    while m < ALL.len() {
        if let Some(e) = ALL.get(m) {
            let mut dup = false;
            let mut n = 0usize;
            while n < ALL.len() {
                if n != m {
                    if let Some(o) = ALL.get(n) {
                        if o.message() == e.message() {
                            dup = true;
                        }
                    }
                }
                n += 1;
            }
            if !dup && !e.message().is_empty() {
                unique += 1;
            }
        }
        m += 1;
    }
    let ok = unique == FONT_ERR_COUNT;
    let det = format!("unique={}", unique);
    s.add_dyn("50091-J1-消息唯一", ok, &det);

    // J1-4 族分区：8 族齐全，族计数之和 = 15
    let mut fam_sum = 0usize;
    let mut fam_hit = 0usize;
    let mut f = 0usize;
    let fams = [
        ErrFamily::Parse,
        ErrFamily::License,
        ErrFamily::Network,
        ErrFamily::Subset,
        ErrFamily::Metrics,
        ErrFamily::Pipeline,
        ErrFamily::Substitution,
        ErrFamily::Storage,
    ];
    while f < fams.len() {
        let mut c = 0usize;
        let mut k = 0usize;
        while k < ALL.len() {
            if let Some(e) = ALL.get(k) {
                if e.family() == fams[f] {
                    c += 1;
                }
            }
            k += 1;
        }
        if c > 0 {
            fam_hit += 1;
        }
        fam_sum += c;
        f += 1;
    }
    let ok = fam_hit == FAMILY_COUNT
        && fam_sum == FONT_ERR_COUNT
        && FontError::LicenseRefused.family() == ErrFamily::License
        && FontError::DownloadTimeout.family() == ErrFamily::Network;
    let det = format!("families={} sum={}", fam_hit, fam_sum);
    s.add_dyn("50091-J1-族分区", ok, &det);

    // J1-5 严重度三档：总数守恒 + 三档各有代表
    let mut warn = 0usize;
    let mut err = 0usize;
    let mut fatal = 0usize;
    let mut k = 0usize;
    while k < ALL.len() {
        if let Some(e) = ALL.get(k) {
            match e.severity() {
                Severity::Warning => warn += 1,
                Severity::Error => err += 1,
                Severity::Fatal => fatal += 1,
            }
        }
        k += 1;
    }
    let ok = warn + err + fatal == FONT_ERR_COUNT
        && warn == 1
        && FontError::CacheEvict.severity() == Severity::Warning
        && FontError::BadFont.severity() == Severity::Fatal
        && FontError::DownloadTimeout.severity() == Severity::Error;
    let det = format!("w={} e={} f={}", warn, err, fatal);
    s.add_dyn("50091-J1-严重度分区", ok, &det);

    // J1-6 可重试集合精确为 4 条
    let mut retry = 0usize;
    let mut k = 0usize;
    while k < ALL.len() {
        if let Some(e) = ALL.get(k) {
            if e.retryable() {
                retry += 1;
            }
        }
        k += 1;
    }
    let ok = retry == 4
        && FontError::DownloadTimeout.retryable()
        && FontError::DownloadHttp.retryable()
        && FontError::CacheEvict.retryable()
        && FontError::ResourceExhausted.retryable()
        && !FontError::BadFont.retryable()
        && !FontError::LicenseRefused.retryable();
    let det = format!("retryable={}", retry);
    s.add_dyn("50091-J1-可重试集合", ok, &det);

    // J1-7 规范字典：15 条、零漂移、逐码可查
    let dict = ErrorDictionary::canonical();
    let mut all_lookup = true;
    let mut k = 0usize;
    while k < ALL.len() {
        if let Some(e) = ALL.get(k) {
            match &dict {
                Ok(d) => match d.lookup(e.code()) {
                    Some(entry) => {
                        if entry.error != *e || entry.remedy != e.remedy() {
                            all_lookup = false;
                        }
                    }
                    None => all_lookup = false,
                },
                Err(_) => all_lookup = false,
            }
        }
        k += 1;
    }
    let ok = match &dict {
        Ok(d) => d.len() == FONT_ERR_COUNT && d.verify_no_drift() && all_lookup,
        Err(_) => false,
    };
    let det = match &dict {
        Ok(d) => d.summary(),
        Err(_) => String::new(),
    };
    s.add_dyn("50091-J1-字典零漂移", ok, &det);

    // J1-8 处置完备：15 条各有处置，且字典与纯函数逐项一致
    let mut remedy_ok = true;
    let mut k = 0usize;
    while k < ALL.len() {
        if let Some(e) = ALL.get(k) {
            let r = e.remedy();
            if r.label().is_empty() {
                remedy_ok = false;
            }
            if let Ok(d) = &dict {
                if let Some(entry) = d.lookup(e.code()) {
                    if entry.remedy != r {
                        remedy_ok = false;
                    }
                }
            }
        }
        k += 1;
    }
    let ok = remedy_ok && FontError::BadFont.remedy() == Remedy::Abort;
    s.add("50091-J1-处置完备", ok, if ok { "" } else { "处置缺失或漂移" });

    // J1-9 字典加载拒绝漂移表
    let mut drift_rows: Vec<TableRow> = Vec::with_capacity(1);
    let mut drow = row(FontError::GlyphMissing);
    drow.code = SEG_BASE + 3;
    drift_rows.push(drow);
    let ok = ErrorDictionary::from_table(&drift_rows).err() == Some(RegistryError::Drift);
    s.add("50091-J1-漂移必拒", ok, if ok { "" } else { "漂移表被收" });

    // J1-10 重复/缺项/乱序/空/超容五类加载拒绝
    let mut dup: Vec<TableRow> = Vec::with_capacity(2);
    dup.push(row(FontError::BadFont));
    dup.push(row(FontError::BadFont));
    let mut short: Vec<TableRow> = Vec::with_capacity(FONT_ERR_COUNT);
    let mut q = 0usize;
    while q + 1 < ALL.len() {
        if let Some(e) = ALL.get(q) {
            short.push(row(*e));
        }
        q += 1;
    }
    let mut unsorted: Vec<TableRow> = Vec::with_capacity(2);
    unsorted.push(row(FontError::DownloadTimeout));
    unsorted.push(row(FontError::BadFont));
    let mut over: Vec<TableRow> = Vec::with_capacity(17);
    let mut o = 0usize;
    while o < 17 {
        over.push(row(FontError::BadFont));
        o += 1;
    }
    let ok = ErrorDictionary::from_table(&dup).err() == Some(RegistryError::Duplicate)
        && ErrorDictionary::from_table(&short).err() == Some(RegistryError::Unmapped)
        && ErrorDictionary::from_table(&unsorted).err() == Some(RegistryError::Order)
        && ErrorDictionary::from_table(&[]).err() == Some(RegistryError::Empty)
        && ErrorDictionary::from_table(&over).err() == Some(RegistryError::Capacity);
    s.add("50091-J1-表加载五拒", ok, if ok { "" } else { "坏表未被拒" });

    // J1-11 族外码值：段首、注册表码、超界全部 BadCode
    let dcheck = match &dict {
        Ok(d) => d.clone(),
        Err(_) => ErrorDictionary { entries: Vec::new() },
    };
    let ok = FontError::from_code(SEG_BASE).err() == Some(RegistryError::BadCode)
        && FontError::from_code(REG_BASE + 1).err() == Some(RegistryError::BadCode)
        && FontError::from_code(SEG_BASE + 0x20).err() == Some(RegistryError::BadCode)
        && remedy_for(SEG_BASE + 99, &dcheck).err() == Some(RegistryError::Unmapped);
    s.add("50091-J1-越界码必拒", ok, if ok { "" } else { "越界码被收" });

    // J1-12 频次账本 ×15：按 1..=15 次记录，总数 120 且三口径守恒
    let mut ledger = ErrorLedger::new();
    let mut k = 0usize;
    while k < ALL.len() {
        if let Some(e) = ALL.get(k) {
            let mut t = 0usize;
            while t <= k {
                ledger.record(*e);
                t += 1;
            }
        }
        k += 1;
    }
    let ok = ledger.total == 120
        && ledger.conservation_ok()
        && ledger.count_of(FontError::BadFont) == 1
        && ledger.count_of(FontError::ResourceExhausted) == 15
        && ledger.family_count(ErrFamily::Parse) == 1 + 12 + 13
        && ledger.severity_count(Severity::Fatal) == 1 + 12 + 13 + 15
        && ledger.retryable == 3 + 4 + 14 + 15;
    let det = ledger.summary();
    s.add_dyn("50091-J1-账本×15守恒", ok, &det);

    // J1-13 按码记账：合法累加、非法拒绝且不污染
    let mut lc = ErrorLedger::new();
    let ok1 = lc.record_code(FontError::DownloadTimeout.code()) == Ok(FontError::DownloadTimeout);
    let bad = lc.record_code(REG_BASE + 1).err();
    let ok = ok1
        && bad == Some(RegistryError::BadCode)
        && lc.total == 1
        && lc.rejected == 1
        && lc.count_of(FontError::DownloadTimeout) == 1
        && lc.conservation_ok();
    s.add("50091-J1-按码记账", ok, if ok { "" } else { "非法码被记账" });

    // J1-14 族预算：limit=2 时前两次放行、第三次 Overflow 并 trip
    let mut budget = ErrorBudget::new(2);
    let b1 = budget.observe(FontError::DownloadTimeout).is_ok();
    let b2 = budget.observe(FontError::DownloadHttp).is_ok();
    let b3 = budget.observe(FontError::DownloadHttp).err();
    let other = budget.observe(FontError::BadFont).is_ok();
    let ok = b1
        && b2
        && b3 == Some(RegistryError::Overflow)
        && other
        && budget.tripped_families() == 1
        && budget.tripped[ErrFamily::Network.code() as usize]
        && budget.seen_in(ErrFamily::Network) == 2;
    s.add("50091-J1-族预算", ok, if ok { "" } else { "预算未 trip" });

    // J1-15 确定性：两次规范字典逐条相等，摘要稳定
    let d1 = ErrorDictionary::canonical();
    let d2 = ErrorDictionary::canonical();
    let ok = d1 == d2
        && match (&d1, &d2) {
            (Ok(x), Ok(y)) => x.summary() == y.summary() && x.verify_no_drift() && y.verify_no_drift(),
            _ => false,
        };
    s.add("50091-J1-字典确定", ok, if ok { "" } else { "字典非确定" });

    // J1-16 注册表错误码闭集：8 条连续落在 REG 子段
    let regs = [
        RegistryError::Empty,
        RegistryError::Capacity,
        RegistryError::Duplicate,
        RegistryError::Drift,
        RegistryError::Unmapped,
        RegistryError::Order,
        RegistryError::BadCode,
        RegistryError::Overflow,
    ];
    let mut e = 0usize;
    let mut ok = true;
    while e < regs.len() {
        if let Some(err) = regs.get(e) {
            if err.code() != REG_BASE + (e as u32) + 1 || err.message().is_empty() {
                ok = false;
            }
        }
        e += 1;
    }
    s.add("50091-J1-注册码闭集", ok, if ok { "" } else { "注册码漂移" });

    // J1-17 注入核九类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50091-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50091_checks_green() {
        let set = run_unx50091_checks();
        assert!(set.all_passed(), "UNX-F50091 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50091_codes_roundtrip() {
        for e in ALL.iter() {
            assert_eq!(FontError::from_code(e.code()), Ok(*e));
            assert_eq!(e.code(), SEG_BASE + e.offset() as u32);
        }
        assert_eq!(FontError::from_code(SEG_BASE), Err(RegistryError::BadCode));
    }

    #[test]
    fn unx50091_dictionary_is_drift_free() {
        let d = ErrorDictionary::canonical();
        assert!(d.is_ok());
        match d {
            Ok(dict) => {
                assert_eq!(dict.len(), FONT_ERR_COUNT);
                assert!(dict.verify_no_drift());
            }
            Err(_) => panic!("canonical dictionary must load"),
        }
    }
}
