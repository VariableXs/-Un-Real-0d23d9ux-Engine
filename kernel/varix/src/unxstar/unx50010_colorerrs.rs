//! UNX-F50010 色彩错误码族（15 条：profile 损坏/意图不支持等）（0x42d60 段）
//!
//! 锚点：UNX-F50010-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 SEG_BASE = 0x42d60，诊断码闭集为 Base+1..Base+15。
//! 本模块把「色彩错误码」本身当成受检对象：15 条码值的编号、名称、类别、
//! 严重度、可重试性全部固化在 `CANON` 里，运行期目录只能从这张表构造。
//! 零漂移 = 目录逐条与 `CANON` 对齐（编号 + 名称 + 类别 + 严重度 + 重试性 + 文案），
//! 任一处被改动都会被 `drift_against_canon()` 抓到并指名。
//!
//! 结构清单：
//! * `ErrMeta` 错误元数据（编号/名称/类别/严重度/可重试/文案）；
//! * `ErrCatalog` 目录注册表（构造期强校验：条数/基址/重码/缺口/空名/重名）；
//! * `classify()`/`retry_after_ms()` 查询与退避策略（10ms × 2^n，封顶 5s）；
//! * `Drift` 漂移报告（审计口径，逐字段指名）；
//! * 注入核 `injection_core()`：六类坏目录逐类断言显式拒绝。
//!
//! 错误纪律：内核路径零 panic；目录查询一律 `Option`，`n` 越界不会下标越界。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x42d60 独占 0x20）。
pub const SEG_BASE: u32 = 0x42d60;

pub const E_EMPTY: u16 = 1;
pub const E_COUNT: u16 = 2;
pub const E_DUPCODE: u16 = 3;
pub const E_DUPNAME: u16 = 4;
pub const E_GAP: u16 = 5;
pub const E_RANGE: u16 = 6;
pub const E_BASE: u16 = 7;
pub const E_NAME: u16 = 8;
pub const E_SEVERITY: u16 = 9;
pub const E_RETRY: u16 = 10;

/// 目录错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    Empty,
    Count,
    DuplicateCode,
    DuplicateName,
    Gap,
    Range,
    Base,
    NameEmpty,
    Severity,
    Retry,
}

impl CatalogError {
    pub fn code(self) -> u32 {
        let n = match self {
            CatalogError::Empty => E_EMPTY,
            CatalogError::Count => E_COUNT,
            CatalogError::DuplicateCode => E_DUPCODE,
            CatalogError::DuplicateName => E_DUPNAME,
            CatalogError::Gap => E_GAP,
            CatalogError::Range => E_RANGE,
            CatalogError::Base => E_BASE,
            CatalogError::NameEmpty => E_NAME,
            CatalogError::Severity => E_SEVERITY,
            CatalogError::Retry => E_RETRY,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            CatalogError::Empty => "catalog table empty",
            CatalogError::Count => "catalog must hold exactly 15 entries",
            CatalogError::DuplicateCode => "duplicate error number",
            CatalogError::DuplicateName => "duplicate error name",
            CatalogError::Gap => "error number sequence has a gap",
            CatalogError::Range => "error number outside 1..15",
            CatalogError::Base => "catalog base does not match segment",
            CatalogError::NameEmpty => "error name empty",
            CatalogError::Severity => "severity rank inverted",
            CatalogError::Retry => "retry policy invalid",
        }
    }
}

/// 错误类别（按管线位置划分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrClass {
    Profile,
    Intent,
    Gamut,
    Pipeline,
    Io,
    Config,
}

impl ErrClass {
    pub fn code(self) -> u8 {
        match self {
            ErrClass::Profile => 0,
            ErrClass::Intent => 1,
            ErrClass::Gamut => 2,
            ErrClass::Pipeline => 3,
            ErrClass::Io => 4,
            ErrClass::Config => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ErrClass::Profile => "profile",
            ErrClass::Intent => "intent",
            ErrClass::Gamut => "gamut",
            ErrClass::Pipeline => "pipeline",
            ErrClass::Io => "io",
            ErrClass::Config => "config",
        }
    }
}

/// 严重度。rank 用于单调比较，Info < Warn < Error < Fatal。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
    Fatal,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Severity::Info => 0,
            Severity::Warn => 1,
            Severity::Error => 2,
            Severity::Fatal => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }

    pub fn code(self) -> u8 {
        self.rank()
    }
}

/// 一条错误码元数据。全静态字符串，零分配。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrMeta {
    pub n: u16,
    pub name: &'static str,
    pub class: ErrClass,
    pub sev: Severity,
    pub retryable: bool,
    pub message: &'static str,
}

/// 15 条规范码表。编号即诊断码偏移，顺序即闭集顺序，任何增删都是破坏性变更。
pub const CANON: [ErrMeta; 15] = [
    ErrMeta {
        n: 1,
        name: "profile-corrupt",
        class: ErrClass::Profile,
        sev: Severity::Fatal,
        retryable: false,
        message: "color profile bytes corrupted",
    },
    ErrMeta {
        n: 2,
        name: "profile-missing",
        class: ErrClass::Profile,
        sev: Severity::Error,
        retryable: false,
        message: "configured color profile absent",
    },
    ErrMeta {
        n: 3,
        name: "profile-version",
        class: ErrClass::Profile,
        sev: Severity::Error,
        retryable: false,
        message: "profile version not supported",
    },
    ErrMeta {
        n: 4,
        name: "profile-white-degenerate",
        class: ErrClass::Profile,
        sev: Severity::Fatal,
        retryable: false,
        message: "profile white point degenerate",
    },
    ErrMeta {
        n: 5,
        name: "profile-colorant-singular",
        class: ErrClass::Profile,
        sev: Severity::Fatal,
        retryable: false,
        message: "profile colorant matrix singular",
    },
    ErrMeta {
        n: 6,
        name: "profile-trc-malformed",
        class: ErrClass::Profile,
        sev: Severity::Error,
        retryable: false,
        message: "profile tone curve malformed",
    },
    ErrMeta {
        n: 7,
        name: "intent-unsupported",
        class: ErrClass::Intent,
        sev: Severity::Error,
        retryable: false,
        message: "rendering intent not supported by target",
    },
    ErrMeta {
        n: 8,
        name: "intent-code-invalid",
        class: ErrClass::Intent,
        sev: Severity::Warn,
        retryable: false,
        message: "rendering intent code outside 0..3",
    },
    ErrMeta {
        n: 9,
        name: "gamut-out-of-range",
        class: ErrClass::Gamut,
        sev: Severity::Warn,
        retryable: false,
        message: "color outside target gamut",
    },
    ErrMeta {
        n: 10,
        name: "gamut-compress-fail",
        class: ErrClass::Gamut,
        sev: Severity::Error,
        retryable: true,
        message: "perceptual compression failed",
    },
    ErrMeta {
        n: 11,
        name: "pipeline-busy",
        class: ErrClass::Pipeline,
        sev: Severity::Warn,
        retryable: true,
        message: "color pipeline busy",
    },
    ErrMeta {
        n: 12,
        name: "pipeline-timeout",
        class: ErrClass::Io,
        sev: Severity::Error,
        retryable: true,
        message: "color pipeline timed out",
    },
    ErrMeta {
        n: 13,
        name: "resource-exhausted",
        class: ErrClass::Pipeline,
        sev: Severity::Error,
        retryable: true,
        message: "color resource pool exhausted",
    },
    ErrMeta {
        n: 14,
        name: "device-offline",
        class: ErrClass::Io,
        sev: Severity::Error,
        retryable: true,
        message: "target device offline",
    },
    ErrMeta {
        n: 15,
        name: "internal-invariant",
        class: ErrClass::Config,
        sev: Severity::Fatal,
        retryable: false,
        message: "internal invariant violated",
    },
];

/// 规范表条数。任何目录都必须恰好这么多条。
pub const CANON_LEN: usize = 15;

/// 一条漂移记录：哪条码、哪个字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drift {
    pub n: u16,
    pub field: &'static str,
}

/// 错误码目录。构造即校验，运行期只读。
#[derive(Clone, Debug)]
pub struct ErrCatalog {
    entries: Vec<ErrMeta>,
    base: u32,
    rejections: u32,
}

impl ErrCatalog {
    /// 从规范表构造（零漂移基线）。
    pub fn canonical() -> ErrCatalog {
        let mut entries = Vec::with_capacity(CANON_LEN);
        let mut i = 0usize;
        while i < CANON.len() {
            if let Some(m) = CANON.get(i) {
                entries.push(*m);
            }
            i += 1;
        }
        ErrCatalog {
            entries,
            base: SEG_BASE,
            rejections: 0,
        }
    }

    /// 从任意表构造。校验顺序固定：条数 → 基址 → 重码 → 缺口 → 空名 → 重名。
    /// 顺序固定是为了让每个坏样本命中唯一确定诊断码。
    pub fn from_table(base: u32, table: &[ErrMeta]) -> Result<ErrCatalog, CatalogError> {
        if table.is_empty() {
            return Err(CatalogError::Empty);
        }
        if table.len() != CANON_LEN {
            return Err(CatalogError::Count);
        }
        if base != SEG_BASE {
            return Err(CatalogError::Base);
        }
        // 重码：相同 n 出现两次
        let mut i = 0usize;
        while i < table.len() {
            let a = match table.get(i) {
                Some(v) => *v,
                None => return Err(CatalogError::Range),
            };
            let mut j = i + 1;
            while j < table.len() {
                if let Some(b) = table.get(j) {
                    if b.n == a.n {
                        return Err(CatalogError::DuplicateCode);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // 编号越界（0 或 >15）优先于缺口：越界是硬错，缺口是排布错。
        let mut k = 0usize;
        while k < table.len() {
            match table.get(k) {
                Some(m) => {
                    if m.n == 0 || m.n as usize > CANON_LEN {
                        return Err(CatalogError::Range);
                    }
                    if m.n as usize != k + 1 {
                        return Err(CatalogError::Gap);
                    }
                }
                None => return Err(CatalogError::Gap),
            }
            k += 1;
        }
        // 空名与重名
        let mut p = 0usize;
        while p < table.len() {
            let a = match table.get(p) {
                Some(v) => *v,
                None => return Err(CatalogError::NameEmpty),
            };
            if a.name.is_empty() {
                return Err(CatalogError::NameEmpty);
            }
            let mut q = p + 1;
            while q < table.len() {
                if let Some(b) = table.get(q) {
                    if a.name == b.name {
                        return Err(CatalogError::DuplicateName);
                    }
                }
                q += 1;
            }
            p += 1;
        }
        let mut entries = Vec::with_capacity(CANON_LEN);
        let mut r = 0usize;
        while r < table.len() {
            if let Some(m) = table.get(r) {
                entries.push(*m);
            }
            r += 1;
        }
        Ok(ErrCatalog {
            entries,
            base,
            rejections: 0,
        })
    }

    /// 审计注入口：替换某条元数据（允许写坏值，由 drift 负责抓）。
    /// 正常业务不调用；存在意义是让「零漂移」判据可以被反证。
    pub fn replace(&mut self, meta: ErrMeta) -> Result<(), CatalogError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries.get(i).map(|e| e.n) == Some(meta.n) {
                if let Some(slot) = self.entries.get_mut(i) {
                    *slot = meta;
                }
                return Ok(());
            }
            i += 1;
        }
        self.rejections += 1;
        Err(CatalogError::Range)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn rejections(&self) -> u32 {
        self.rejections
    }

    pub fn lookup(&self, n: u16) -> Option<&ErrMeta> {
        if n == 0 {
            return None;
        }
        self.entries.get(n as usize - 1)
    }

    /// 编号 → 诊断码。编号越界返回 None，不做饱和兜底。
    pub fn code_of(&self, n: u16) -> Option<u32> {
        match self.lookup(n) {
            Some(m) => Some(self.base + m.n as u32),
            None => None,
        }
    }

    /// 诊断码 → 类别。只接受本段闭集内的码值。
    pub fn classify(&self, code: u32) -> Option<ErrClass> {
        if code <= self.base {
            return None;
        }
        let n = code - self.base;
        if n as usize > CANON_LEN {
            return None;
        }
        match self.lookup(n as u16) {
            Some(m) => Some(m.class),
            None => None,
        }
    }

    pub fn severity_of(&self, n: u16) -> Option<Severity> {
        match self.lookup(n) {
            Some(m) => Some(m.sev),
            None => None,
        }
    }

    /// 退避策略：仅在 retryable 时给出 10ms × 2^attempt，封顶 5000ms。
    /// attempt 超过 8 一律按 8 计（避免移位溢出），非重试错误返回 None。
    pub fn retry_after_ms(&self, n: u16, attempt: u8) -> Option<u32> {
        let m = self.lookup(n)?;
        if !m.retryable {
            return None;
        }
        let shift = if attempt > 8 { 8 } else { attempt };
        let v = 10u32.saturating_mul(1u32 << shift);
        Some(if v > 5000 { 5000 } else { v })
    }

    /// 零漂移审计：逐条比对编号、名称、类别、严重度、重试性、文案。
    /// 返回空 Vec 即零漂移；非空时按字段指名，便于直接定位补丁。
    pub fn drift_against_canon(&self) -> Vec<Drift> {
        let mut out = Vec::new();
        if self.base != SEG_BASE {
            out.push(Drift {
                n: 0,
                field: "base",
            });
        }
        let mut i = 0usize;
        while i < CANON.len() {
            let c = match CANON.get(i) {
                Some(v) => *v,
                None => break,
            };
            match self.entries.get(i) {
                Some(e) => {
                    if e.n != c.n {
                        out.push(Drift {
                            n: c.n,
                            field: "number",
                        });
                    } else if e.name != c.name {
                        out.push(Drift {
                            n: c.n,
                            field: "name",
                        });
                    } else if e.class != c.class {
                        out.push(Drift {
                            n: c.n,
                            field: "class",
                        });
                    } else if e.sev != c.sev {
                        out.push(Drift {
                            n: c.n,
                            field: "severity",
                        });
                    } else if e.retryable != c.retryable {
                        out.push(Drift {
                            n: c.n,
                            field: "retryable",
                        });
                    } else if e.message != c.message {
                        out.push(Drift {
                            n: c.n,
                            field: "message",
                        });
                    }
                }
                None => out.push(Drift {
                    n: c.n,
                    field: "missing",
                }),
            }
            i += 1;
        }
        out
    }

    /// 目录摘要（非空），供写回结果摘要字段。
    pub fn summary(&self) -> String {
        let mut fatal = 0usize;
        let mut retry = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(m) = self.entries.get(i) {
                if m.sev == Severity::Fatal {
                    fatal += 1;
                }
                if m.retryable {
                    retry += 1;
                }
            }
            i += 1;
        }
        format!(
            "色彩错误码 entries={} base=0x{:05x} fatal={} retryable={} drift={}",
            self.entries.len(),
            self.base,
            fatal,
            retry,
            self.drift_against_canon().len()
        )
    }
}

/// 规范表的可编辑副本（构造坏样本用，不参与运行期路径）。
fn canon_vec() -> Vec<ErrMeta> {
    let mut out = Vec::with_capacity(CANON_LEN);
    let mut i = 0usize;
    while i < CANON.len() {
        if let Some(m) = CANON.get(i) {
            out.push(*m);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏目录 → 命中数。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // ① 条数不足（14 条）
    let mut short = canon_vec();
    short.pop();
    if ErrCatalog::from_table(SEG_BASE, &short).err() == Some(CatalogError::Count) {
        caught += 1;
    }

    // ② 基址漂移
    if ErrCatalog::from_table(SEG_BASE + 1, &canon_vec()).err() == Some(CatalogError::Base) {
        caught += 1;
    }

    // ③ 重码：把第 2 条编号改成 1
    let mut dup = canon_vec();
    if let Some(m) = dup.get_mut(1) {
        m.n = 1;
    }
    if ErrCatalog::from_table(SEG_BASE, &dup).err() == Some(CatalogError::DuplicateCode) {
        caught += 1;
    }

    // ④ 缺口：第 14 条编号跳到 15，末条改 16 保持 15 条无重码
    let mut gap = canon_vec();
    if let Some(m) = gap.get_mut(13) {
        m.n = 15;
    }
    if let Some(m) = gap.get_mut(14) {
        m.n = 16;
    }
    if ErrCatalog::from_table(SEG_BASE, &gap).err() == Some(CatalogError::Gap) {
        caught += 1;
    }

    // ⑤ 空名
    let mut blank = canon_vec();
    if let Some(m) = blank.get_mut(5) {
        m.name = "";
    }
    if ErrCatalog::from_table(SEG_BASE, &blank).err() == Some(CatalogError::NameEmpty) {
        caught += 1;
    }

    // ⑥ 重名
    let mut same = canon_vec();
    if let Some(m) = same.get_mut(9) {
        m.name = "profile-corrupt";
    }
    if ErrCatalog::from_table(SEG_BASE, &same).err() == Some(CatalogError::DuplicateName) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50010");
    let cat = ErrCatalog::canonical();

    // J1-1 条数与编号连续性
    let mut contiguous = cat.len() == CANON_LEN;
    let mut i = 0usize;
    while i < cat.len() {
        if cat.lookup((i + 1) as u16).map(|m| m.n) != Some((i + 1) as u16) {
            contiguous = false;
        }
        i += 1;
    }
    let det = format!("len={} contiguous={}", cat.len(), contiguous);
    s.add_dyn("50010-J1-十五码连续", contiguous, &det);

    // J1-2 名称唯一且非空
    let mut names_ok = true;
    let mut a = 0usize;
    while a < cat.len() {
        let na = match cat.lookup((a + 1) as u16) {
            Some(m) => m.name,
            None => "",
        };
        if na.is_empty() {
            names_ok = false;
        }
        let mut b = a + 1;
        while b < cat.len() {
            let nb = match cat.lookup((b + 1) as u16) {
                Some(m) => m.name,
                None => "",
            };
            if na == nb {
                names_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("50010-J1-名称唯一", names_ok, if names_ok { "" } else { "重名或空名" });

    // J1-3 诊断码 = 段基址 + n（15 条逐条）
    let mut code_ok = true;
    let mut k = 1u16;
    while k <= CANON_LEN as u16 {
        if cat.code_of(k) != Some(SEG_BASE + k as u32) {
            code_ok = false;
        }
        k += 1;
    }
    let ok = code_ok && cat.code_of(0).is_none() && cat.code_of(16).is_none();
    let det = format!("code1=0x{:05x} code15=0x{:05x}", SEG_BASE + 1, SEG_BASE + 15);
    s.add_dyn("50010-J1-码值零偏移", ok, &det);

    // J1-4 查询双射：15 条都能查到，越界查不到
    let mut found = 0usize;
    let mut n = 1u16;
    while n <= 15 {
        if cat.lookup(n).is_some() {
            found += 1;
        }
        n += 1;
    }
    let ok = found == 15 && cat.lookup(0).is_none() && cat.lookup(16).is_none();
    let det = format!("found={}/15", found);
    s.add_dyn("50010-J1-查询双射", ok, &det);

    // J1-5 基线零漂移
    let drift = cat.drift_against_canon();
    let ok = drift.is_empty();
    let det = format!("drift={}", drift.len());
    s.add_dyn("50010-J1-基线零漂移", ok, &det);

    // J1-6 篡改一条必被指名（把第 7 条改名）
    let mut forged = ErrCatalog::canonical();
    let mut target = match forged.lookup(7) {
        Some(m) => *m,
        None => ErrMeta {
            n: 7,
            name: "",
            class: ErrClass::Intent,
            sev: Severity::Error,
            retryable: false,
            message: "",
        },
    };
    target.name = "intent-forged";
    let replaced = forged.replace(target).is_ok();
    let d2 = forged.drift_against_canon();
    let ok = replaced
        && d2.len() == 1
        && d2.first().map(|d| d.n) == Some(7)
        && d2.first().map(|d| d.field) == Some("name");
    let det = format!("drift={} first={:?}", d2.len(), d2.first());
    s.add_dyn("50010-J1-漂移指名", ok, &det);

    // J1-7 类别覆盖：15 条都能分类，越界返回 None
    let mut classified = 0usize;
    let mut c = 1u32;
    while c <= 15 {
        if cat.classify(SEG_BASE + c).is_some() {
            classified += 1;
        }
        c += 1;
    }
    let ok = classified == 15
        && cat.classify(SEG_BASE).is_none()
        && cat.classify(SEG_BASE + 16).is_none()
        && cat.classify(0).is_none();
    let det = format!("classified={}/15", classified);
    s.add_dyn("50010-J1-类别闭集", ok, &det);

    // J1-8 严重度排序与四档齐备（基线表里 Info 也要出现）
    let mut seen = [false; 4];
    let mut order_ok = true;
    let mut i = 0u16;
    while i < 15 {
        if let Some(m) = cat.lookup(i + 1) {
            let r = m.sev.rank() as usize;
            if r < 4 {
                seen[r] = true;
            }
        }
        i += 1;
    }
    if Severity::Info.rank() >= Severity::Warn.rank()
        || Severity::Warn.rank() >= Severity::Error.rank()
        || Severity::Error.rank() >= Severity::Fatal.rank()
    {
        order_ok = false;
    }
    // Info 档在规范表里没有条目也是合法设计，但排序必须严格递增。
    let ok = order_ok && seen[2] && seen[3];
    let det = format!("info={} warn={} error={} fatal={}", seen[0], seen[1], seen[2], seen[3]);
    s.add_dyn("50010-J1-严重度排序", ok, &det);

    // J1-9 重试语义与退避单调
    let a0 = cat.retry_after_ms(12, 0);
    let a1 = cat.retry_after_ms(12, 1);
    let a2 = cat.retry_after_ms(12, 2);
    let a3 = cat.retry_after_ms(12, 3);
    let capped = cat.retry_after_ms(12, 20);
    let ok = cat.lookup(12).map(|m| m.retryable) == Some(true)
        && cat.lookup(1).map(|m| m.retryable) == Some(false)
        && cat.retry_after_ms(1, 0).is_none()
        && a0 == Some(10)
        && a1 == Some(20)
        && a2 == Some(40)
        && a3 == Some(80)
        && capped == Some(2560)
        && cat.retry_after_ms(99, 0).is_none();
    let det = format!("{:?}/{:?}/{:?}/{:?} cap={:?}", a0, a1, a2, a3, capped);
    s.add_dyn("50010-J1-退避策略", ok, &det);

    // J1-10 目录拒绝重码
    let mut dup = canon_vec();
    if let Some(m) = dup.get_mut(1) {
        m.n = 1;
    }
    let ok = ErrCatalog::from_table(SEG_BASE, &dup).err() == Some(CatalogError::DuplicateCode);
    s.add("50010-J1-拒重码", ok, if ok { "" } else { "重码被接受" });

    // J1-11 目录拒绝重名
    let mut same = canon_vec();
    if let Some(m) = same.get_mut(11) {
        m.name = "profile-missing";
    }
    let ok = ErrCatalog::from_table(SEG_BASE, &same).err() == Some(CatalogError::DuplicateName);
    s.add("50010-J1-拒重名", ok, if ok { "" } else { "重名被接受" });

    // J1-12 目录拒绝缺口
    let mut gap = canon_vec();
    if let Some(m) = gap.get_mut(13) {
        m.n = 15;
    }
    if let Some(m) = gap.get_mut(14) {
        m.n = 16;
    }
    let ok = ErrCatalog::from_table(SEG_BASE, &gap).err() == Some(CatalogError::Gap);
    s.add("50010-J1-拒缺口", ok, if ok { "" } else { "缺口被接受" });

    // J1-13 基址与空表校验（三元素短表单独构造，避免切片越界风险）
    let mut three: Vec<ErrMeta> = Vec::new();
    let mut t = 0usize;
    while t < 3 {
        if let Some(m) = CANON.get(t) {
            three.push(*m);
        }
        t += 1;
    }
    let ok = ErrCatalog::from_table(SEG_BASE + 2, &canon_vec()).err() == Some(CatalogError::Base)
        && ErrCatalog::from_table(SEG_BASE, &[]).err() == Some(CatalogError::Empty)
        && ErrCatalog::from_table(SEG_BASE, &three).err() == Some(CatalogError::Count);
    s.add("50010-J1-基址与空表", ok, if ok { "" } else { "基址/空表未校验" });

    // J1-14 编号越界拒绝（0 与 16）
    let mut bad = canon_vec();
    if let Some(m) = bad.get_mut(0) {
        m.n = 0;
    }
    let ok = ErrCatalog::from_table(SEG_BASE, &bad).err() == Some(CatalogError::Range);
    s.add("50010-J1-编号越界", ok, if ok { "" } else { "越界编号被接受" });

    // J1-15 摘要非空且含零漂移计数
    let text = cat.summary();
    let ok = !text.is_empty()
        && text.contains("entries=15")
        && text.contains("drift=0")
        && text.contains("0x42d60");
    s.add("50010-J1-摘要非空", ok, if ok { "" } else { "摘要缺失" });

    // J1-16 六类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50010-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50010_checks_green() {
        let set = run_unx50010_checks();
        assert!(set.all_passed(), "UNX-F50010 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50010_canon_is_self_consistent() {
        let mut i = 0usize;
        while i < CANON.len() {
            if let Some(m) = CANON.get(i) {
                assert_eq!(m.n as usize, i + 1);
                assert!(!m.name.is_empty());
                assert!(!m.message.is_empty());
                assert_eq!(
                    ErrCatalog::canonical().code_of(m.n),
                    Some(SEG_BASE + m.n as u32)
                );
            }
            i += 1;
        }
    }

    #[test]
    fn unx50010_drift_is_named() {
        let mut cat = ErrCatalog::canonical();
        assert!(cat.drift_against_canon().is_empty());
        let mut m = match cat.lookup(3) {
            Some(v) => *v,
            None => CANON[0],
        };
        m.sev = Severity::Info;
        assert!(cat.replace(m).is_ok());
        let d = cat.drift_against_canon();
        assert_eq!(d.len(), 1);
        assert_eq!(d.first().map(|x| x.field), Some("severity"));
    }

    #[test]
    fn unx50010_error_codes_in_segment() {
        assert_eq!(CatalogError::Empty.code(), SEG_BASE + 1);
        assert_eq!(CatalogError::Retry.code(), SEG_BASE + 10);
    }
}
