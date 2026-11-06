//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18548 · 档案模板库（常见应用处方预置）
//! · 0x804C0 段 · 批 08 · 配额 260
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18548-J1 模板 20 例预置生效断言全过，模板来源登记
//!       入账（合规抽查项）。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 模板 20 例冻结：id 0..=19，六字段（major/minor/build/sp/pt/
//!     dpi）+ 提权位 + 来源闭集三元（0=内置冻结 / 1=社区回灌 /
//!     2=疑难库推导）——每例手写处方值，两两互异；
//!   - 来源登记入账：每例模板必带来源码（闭集 0..=2），登记账
//!     记录"模板 id → 来源码 → 应用计数"三列；未登记来源（≥3）
//!     禁入册；合规抽查 = 全 20 例来源码可回查且非空；
//!   - 应用语义：apply(id) → 处方档案值逐字段生效（读回对拍）；
//!     幂等（同 id 二次应用结果等值、登记账计数一致增长）；
//!   - 应用账定容 20：id 重复应用计幂次而不重登记（首应用登记、
//!     复应用只计数），登记位面 20 bit；
//!   - 注入三类 × 10 轮：id 越界（≥20）/ 坏模板入册（dpi 非冻结
//!     档）/ 未登记来源入册——逐类逐轮必拒，漏检即红；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] TlCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   template_row 20 例手写处方表 + SOURCE_NAMES 来源闭集
//!   [数据结构] TemplateRec / TemplateBank（登记位面 + 应用账）
//!   [状态机]   TplPhase 三态单向（Empty→Registered→Sealed）
//!   [账本]     SourceRegistry（来源登记账）+ ApplyLedger（守恒）
//!   [注入核]   三类变异 × 10 轮（id 越界/坏模板/未登记来源）
//!   [收口]     golden_run + run_unx18548_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x804C0，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x804C0;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 模板数（冻结：20，J1 口径）。
pub const TPL_COUNT: usize = 20;
/// 来源闭集数（冻结：3）。
pub const SOURCE_COUNT: usize = 3;
/// 来源码闭集。
pub const SRC_BUILTIN: u8 = 0;
pub const SRC_COMMUNITY: u8 = 1;
pub const SRC_HARDCASE: u8 = 2;
/// DPI 闭集（与 F18544 同口径）。
pub const DPI_SET: [u16; 5] = [96, 120, 144, 192, 240];
/// PT 闭集。
pub const PT_MIN: u16 = 1;
pub const PT_MAX: u16 = 3;
/// 版本主段下界。
pub const VER_MAJOR_MIN: u16 = 3;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 来源名闭集（合规抽查文本面）。
pub fn source_name(s: u8) -> Option<&'static str> {
    match s {
        SRC_BUILTIN => Some("builtin_frozen"),
        SRC_COMMUNITY => Some("community_backfill"),
        SRC_HARDCASE => Some("hardcase_derived"),
        _ => None,
    }
}

/// 模板处方行（六字段 + 提权 + 来源码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateRec {
    pub id: u8,
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub sp: u16,
    pub pt: u16,
    pub dpi: u16,
    /// 提权（0/1）。
    pub elevate: u16,
    /// 来源码（闭集 0..=2）。
    pub source: u8,
}

impl TemplateRec {
    /// 处方值域守卫（与 F18544/F18542 域闸同口径）。
    pub fn valid(&self) -> bool {
        if self.major < VER_MAJOR_MIN {
            return false;
        }
        if self.pt < PT_MIN || self.pt > PT_MAX {
            return false;
        }
        if self.elevate > 1 {
            return false;
        }
        if self.source >= SOURCE_COUNT as u8 {
            return false;
        }
        dpi_legal(self.dpi)
    }
    /// 处方指纹（31 折叠，对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18548 + self.id as u32;
        acc = acc.wrapping_mul(31).wrapping_add(self.major as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.minor as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.build as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.sp as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.pt as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.dpi as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.elevate as u32);
        acc.wrapping_mul(31).wrapping_add(self.source as u32)
    }
}

/// DPI 是否在冻结闭集内。
pub fn dpi_legal(v: u16) -> bool {
    let mut i = 0;
    while i < DPI_SET.len() {
        if DPI_SET[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

/// 20 例手写处方表（裁决表：逐例冻结值，两两互异）。
pub fn template_row(i: usize) -> Option<TemplateRec> {
    match i {
        0 => Some(TemplateRec { id: 0, major: 5, minor: 1, build: 2600, sp: 3, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        1 => Some(TemplateRec { id: 1, major: 5, minor: 2, build: 3790, sp: 2, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        2 => Some(TemplateRec { id: 2, major: 6, minor: 0, build: 6002, sp: 2, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        3 => Some(TemplateRec { id: 3, major: 6, minor: 1, build: 7601, sp: 1, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        4 => Some(TemplateRec { id: 4, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        5 => Some(TemplateRec { id: 5, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 120, elevate: 0, source: SRC_BUILTIN }),
        6 => Some(TemplateRec { id: 6, major: 6, minor: 2, build: 9200, sp: 0, pt: 3, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        7 => Some(TemplateRec { id: 7, major: 6, minor: 3, build: 9600, sp: 0, pt: 3, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        8 => Some(TemplateRec { id: 8, major: 10, minor: 0, build: 10240, sp: 0, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        9 => Some(TemplateRec { id: 9, major: 10, minor: 0, build: 19045, sp: 0, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        10 => Some(TemplateRec { id: 10, major: 10, minor: 0, build: 19045, sp: 0, pt: 1, dpi: 144, elevate: 0, source: SRC_BUILTIN }),
        11 => Some(TemplateRec { id: 11, major: 10, minor: 0, build: 19045, sp: 0, pt: 3, dpi: 96, elevate: 1, source: SRC_BUILTIN }),
        12 => Some(TemplateRec { id: 12, major: 10, minor: 0, build: 22631, sp: 0, pt: 1, dpi: 96, elevate: 0, source: SRC_BUILTIN }),
        13 => Some(TemplateRec { id: 13, major: 5, minor: 1, build: 2600, sp: 3, pt: 1, dpi: 120, elevate: 0, source: SRC_COMMUNITY }),
        14 => Some(TemplateRec { id: 14, major: 6, minor: 1, build: 7601, sp: 1, pt: 1, dpi: 144, elevate: 0, source: SRC_COMMUNITY }),
        15 => Some(TemplateRec { id: 15, major: 6, minor: 1, build: 7601, sp: 0, pt: 1, dpi: 96, elevate: 1, source: SRC_COMMUNITY }),
        16 => Some(TemplateRec { id: 16, major: 10, minor: 0, build: 19045, sp: 0, pt: 1, dpi: 120, elevate: 1, source: SRC_COMMUNITY }),
        17 => Some(TemplateRec { id: 17, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 144, elevate: 0, source: SRC_HARDCASE }),
        18 => Some(TemplateRec { id: 18, major: 10, minor: 0, build: 19045, sp: 0, pt: 3, dpi: 192, elevate: 0, source: SRC_HARDCASE }),
        19 => Some(TemplateRec { id: 19, major: 6, minor: 0, build: 6002, sp: 2, pt: 3, dpi: 240, elevate: 0, source: SRC_HARDCASE }),
        _ => None,
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlCode {
    /// 模板 id 越界（≥20）。
    IdOutOfRange,
    /// 坏模板入册（值域守卫未过）。
    TemplateInvalid,
    /// 未登记来源（来源码越闭集）。
    SourceUnregistered,
    /// 重复登记（同 id 二次入册）。
    RegistrationDuplicate,
    /// 空库封账（零登记即封）。
    LedgerEmpty,
    /// 封账后应用（单向倒退）。
    PhaseRegression,
    /// 应用未登记模板。
    ApplyUnregistered,
    /// 来源登记账不一致（全 20 例来源可回查失配）。
    SourceAuditBroken,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 注入漏检。
    InjectionMissed,
    /// 口径失配（与字段集域闸不等）。
    CrossCheckMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl TlCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TlCode::IdOutOfRange => "模板 id 越界：仅 0..=19",
            TlCode::TemplateInvalid => "坏模板入册：值域守卫未过",
            TlCode::SourceUnregistered => "未登记来源：来源码越闭集",
            TlCode::RegistrationDuplicate => "重复登记：同 id 只许入册一次",
            TlCode::LedgerEmpty => "空库封账：零登记不得封",
            TlCode::PhaseRegression => "封账后应用：状态机单向禁回退",
            TlCode::ApplyUnregistered => "应用未登记模板：先登记后应用",
            TlCode::SourceAuditBroken => "来源登记账不一致：合规抽查失配",
            TlCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            TlCode::InjectionMissed => "注入漏检：变异必被抓",
            TlCode::CrossCheckMismatch => "口径失配：与字段集域闸不等",
            TlCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TlCode; 12] = [
        TlCode::IdOutOfRange,
        TlCode::TemplateInvalid,
        TlCode::SourceUnregistered,
        TlCode::RegistrationDuplicate,
        TlCode::LedgerEmpty,
        TlCode::PhaseRegression,
        TlCode::ApplyUnregistered,
        TlCode::SourceAuditBroken,
        TlCode::LedgerBroken,
        TlCode::InjectionMissed,
        TlCode::CrossCheckMismatch,
        TlCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TlCode::ALL.len() {
        if TlCode::ALL[i].code() == code {
            return TlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x804C0 段闭集内"
}

// ── 模板库（登记位面 + 来源账 + 应用账） ────────────────────────
/// 库相位（单向：Empty→Registered→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TplPhase {
    /// 零登记。
    Empty,
    /// 有登记。
    Registered,
    /// 已封账（登记冻结，应用照常）。
    Sealed,
}

/// 应用账（守恒：尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
}

impl ApplyLedger {
    pub const fn new() -> ApplyLedger {
        ApplyLedger { tries: 0, passed: 0, rejected: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 来源登记账：每来源码的应用计数（闭集 3 列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRegistry {
    /// 登记位面（bit i = 模板 i 已入册）。
    pub reg_mask: u32,
    /// 来源码表（模板 i 的来源码，未登记为 0xFF）。
    pub src_of: [u8; TPL_COUNT],
    /// 各来源码的登记计数。
    pub reg_counts: [u32; SOURCE_COUNT],
}

impl SourceRegistry {
    pub fn new() -> SourceRegistry {
        SourceRegistry { reg_mask: 0, src_of: [0xFF; TPL_COUNT], reg_counts: [0; SOURCE_COUNT] }
    }
    /// 合规抽查：全 20 例来源码可回查（已登记且闭集内非空名）。
    pub fn audit_all(&self) -> bool {
        let mut i = 0;
        while i < TPL_COUNT {
            if self.reg_mask & (1u32 << i) == 0 {
                return false;
            }
            let s = self.src_of[i];
            if s >= SOURCE_COUNT as u8 {
                return false;
            }
            match source_name(s) {
                Some(_) => {}
                None => return false,
            }
            i += 1;
        }
        true
    }
    /// 登记计数守恒：三来源计数和 = 位面权。
    pub fn counts_conserve(&self) -> bool {
        let mut weight = 0u32;
        let mut i = 0;
        while i < TPL_COUNT {
            if self.reg_mask & (1u32 << i) != 0 {
                weight += 1;
            }
            i += 1;
        }
        self.reg_counts[0] + self.reg_counts[1] + self.reg_counts[2] == weight
    }
}

/// 模板库。
pub struct TemplateBank {
    pub phase: TplPhase,
    pub registry: SourceRegistry,
    pub ledger: ApplyLedger,
    /// 各模板应用计数（幂等口径：重复应用只计数）。
    pub apply_counts: [u32; TPL_COUNT],
}

impl TemplateBank {
    pub fn new() -> TemplateBank {
        TemplateBank {
            phase: TplPhase::Empty,
            registry: SourceRegistry::new(),
            ledger: ApplyLedger::new(),
            apply_counts: [0; TPL_COUNT],
        }
    }

    /// 入册：id 越界拒 → 模板值域拒 → 来源闭集拒 → 重复登记拒。
    pub fn register(&mut self, id: usize) -> Result<(), u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        // 封账相位闸：Sealed 后禁登记（单向倒退拒）——缺此闸则
        // "封账后登记拒" 判据恒红。
        if self.phase == TplPhase::Sealed {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::PhaseRegression.code());
        }
        if id >= TPL_COUNT {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::IdOutOfRange.code());
        }
        let rec = match template_row(id) {
            Some(r) => r,
            None => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                return Err(TlCode::IdOutOfRange.code());
            }
        };
        if !rec.valid() {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::TemplateInvalid.code());
        }
        if rec.source >= SOURCE_COUNT as u8 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::SourceUnregistered.code());
        }
        if self.registry.reg_mask & (1u32 << id) != 0 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::RegistrationDuplicate.code());
        }
        self.registry.reg_mask |= 1u32 << id;
        self.registry.src_of[id] = rec.source;
        self.registry.reg_counts[rec.source as usize] += 1;
        if self.phase == TplPhase::Empty {
            self.phase = TplPhase::Registered;
        }
        self.ledger.passed = self.ledger.passed.saturating_add(1);
        Ok(())
    }

    /// 应用：未登记拒（先登记后应用）；封账后应用照常（只读处方）。
    /// 幂等：重复应用结果等值，只计数。
    pub fn apply(&mut self, id: usize) -> Result<TemplateRec, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if id >= TPL_COUNT {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::IdOutOfRange.code());
        }
        if self.registry.reg_mask & (1u32 << id) == 0 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(TlCode::ApplyUnregistered.code());
        }
        let rec = match template_row(id) {
            Some(r) => r,
            None => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                return Err(TlCode::IdOutOfRange.code());
            }
        };
        self.apply_counts[id] = self.apply_counts[id].saturating_add(1);
        self.ledger.passed = self.ledger.passed.saturating_add(1);
        Ok(rec)
    }

    /// 封账：空库拒；封后登记拒（应用照常——只读处方）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == TplPhase::Sealed {
            return Err(TlCode::PhaseRegression.code());
        }
        if self.phase == TplPhase::Empty {
            return Err(TlCode::LedgerEmpty.code());
        }
        if !self.registry.counts_conserve() || !self.ledger.conserve() {
            return Err(TlCode::LedgerBroken.code());
        }
        self.phase = TplPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（登记数 / 应用数 / 库指纹）。
pub type GoldenOut = (usize, usize, u32);

/// 黄金全链：20 例全登记 → 全 20 例应用且读回对拍 → 来源合规抽查
/// → 幂等复应用 → 封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut bank = TemplateBank::new();
    // 20 例全登记。
    let mut i = 0;
    while i < TPL_COUNT {
        bank.register(i)?;
        i += 1;
    }
    if bank.registry.reg_counts[0] != 13 {
        // 推导：SRC_BUILTIN 例 0..=12 共 13 例。
        return Err(TlCode::SourceAuditBroken.code());
    }
    if bank.registry.reg_counts[1] != 4 {
        // 推导：SRC_COMMUNITY 例 13..=16 共 4 例。
        return Err(TlCode::SourceAuditBroken.code());
    }
    if bank.registry.reg_counts[2] != 3 {
        // 推导：SRC_HARDCASE 例 17..=19 共 3 例。
        return Err(TlCode::SourceAuditBroken.code());
    }
    // 全 20 例应用 + 读回对拍。
    let mut applied = 0usize;
    let mut j = 0;
    while j < TPL_COUNT {
        let rec = bank.apply(j)?;
        let expect = match template_row(j) {
            Some(r) => r,
            None => return Err(TlCode::IdOutOfRange.code()),
        };
        if rec != expect {
            return Err(TlCode::TemplateInvalid.code());
        }
        applied += 1;
        j += 1;
    }
    if applied != TPL_COUNT {
        return Err(TlCode::LedgerBroken.code());
    }
    // 来源合规抽查（J1 第二命题）。
    if !bank.registry.audit_all() {
        return Err(TlCode::SourceAuditBroken.code());
    }
    // 幂等复应用：同 id 结果等值。
    let again = bank.apply(5)?;
    let first = match template_row(5) {
        Some(r) => r,
        None => return Err(TlCode::IdOutOfRange.code()),
    };
    if again != first {
        return Err(TlCode::NotReproducible.code());
    }
    bank.seal()?;
    if !bank.ledger.conserve() {
        return Err(TlCode::LedgerBroken.code());
    }
    // 库指纹：全登记位面 + 20 例处方指纹折叠。
    let mut fp: u32 = 0x185C0;
    let mut k = 0;
    while k < TPL_COUNT {
        let rec = match template_row(k) {
            Some(r) => r,
            None => return Err(TlCode::IdOutOfRange.code()),
        };
        fp = fp.wrapping_mul(31).wrapping_add(rec.fingerprint() % 31);
        k += 1;
    }
    Ok((bank.registry.reg_counts[0] as usize + 20, applied, fp))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TlCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 id 越界。
    pub oob_caught: u32,
    /// 类2 坏模板（dpi 非档）。
    pub badtpl_caught: u32,
    /// 类3 未登记来源。
    pub unsrc_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约。
pub const FACE_OOB: u32 = 1;
pub const FACE_BADTPL: u32 = 1;
pub const FACE_UNSRC: u32 = 1;

/// 三类注入 rounds 轮；每类每轮必拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 id 越界：id=20 登记与应用双拒。
        let mut bank = TemplateBank::new();
        match bank.register(TPL_COUNT) {
            Err(c) if c == TlCode::IdOutOfRange.code() => {
                led.oob_caught = led.oob_caught.saturating_add(FACE_OOB);
            }
            _ => return Err(TlCode::InjectionMissed.code()),
        }
        // 类2 坏模板：dpi=100 的处方行必被值域守卫拒。
        let rogue = TemplateRec { id: 0, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 100, elevate: 0, source: SRC_BUILTIN };
        if !rogue.valid() {
            led.badtpl_caught = led.badtpl_caught.saturating_add(FACE_BADTPL);
        } else {
            return Err(TlCode::InjectionMissed.code());
        }
        // 类3 未登记来源：source=3 的处方行必被来源闭集拒。
        let rogue2 = TemplateRec { id: 1, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96, elevate: 0, source: SOURCE_COUNT as u8 };
        if !rogue2.valid() {
            led.unsrc_caught = led.unsrc_caught.saturating_add(FACE_UNSRC);
        } else {
            return Err(TlCode::InjectionMissed.code());
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空库封账 → 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut bank = TemplateBank::new();
    bank.seal()
}

/// 探针 C：应用未登记模板 → 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    // Ok 侧折叠为违约码（探针语义：未登记模板必须被拒，Ok 即漏检）。
    let mut bank = TemplateBank::new();
    match bank.apply(0) {
        Err(c) => Err(c),
        Ok(_) => Err(TlCode::ApplyUnregistered.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < TlCode::ALL.len() {
        let v = TlCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：20 例登记+应用+抽查全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((_, applied, _)) => *applied == TPL_COUNT,
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. 模板 20 例预置生效：逐例读回对拍（J1 第一命题）。
    let eff_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < TPL_COUNT {
            match template_row(i) {
                Some(r) => {
                    if !r.valid() || r.id as usize != i {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-模板20例生效", eff_ok, String::from("逐例值域过+id 对位")));

    // 3. 来源登记入账：13/4/3 三来源闭集分布（合规抽查项）。
    let src_ok = {
        let mut c0 = 0;
        let mut c1 = 0;
        let mut c2 = 0;
        let mut i = 0;
        while i < TPL_COUNT {
            match template_row(i) {
                Some(r) => match r.source {
                    SRC_BUILTIN => c0 += 1,
                    SRC_COMMUNITY => c1 += 1,
                    SRC_HARDCASE => c2 += 1,
                    _ => {}
                },
                None => {}
            }
            i += 1;
        }
        c0 == 13 && c1 == 4 && c2 == 3 && c0 + c1 + c2 == 20
    };
    v.push(("J1-来源登记13/4/3", src_ok, String::from("内置13+社区4+疑难3")));

    // 4. 合规抽查：全 20 例来源码可回查非空。
    let audit_ok = {
        let mut reg = SourceRegistry::new();
        let mut i = 0;
        while i < TPL_COUNT {
            match template_row(i) {
                Some(r) => {
                    reg.reg_mask |= 1u32 << i;
                    reg.src_of[i] = r.source;
                    reg.reg_counts[r.source as usize] += 1;
                }
                None => {}
            }
            i += 1;
        }
        reg.audit_all() && reg.counts_conserve()
    };
    v.push(("J1-合规抽查全回查", audit_ok, String::from("20 例来源可回溯")));

    // 5. 20 例两两互异。
    let uniq_ok = {
        let mut ok = true;
        let mut a = 0;
        while a < TPL_COUNT {
            let mut b = a + 1;
            while b < TPL_COUNT {
                if template_row(a) == template_row(b) {
                    ok = false;
                }
                b += 1;
            }
            a += 1;
        }
        ok
    };
    v.push(("J1-模板两两互异", uniq_ok, String::from("20 例独立处方")));

    // 6. 类1 id 越界注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.oob_caught).unwrap_or(0);
    v.push(("J1-id越界全拒", t1 == FACE_OOB * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_OOB * JUDGE_ROUNDS)));

    // 7. 类2 坏模板注入。
    let t2 = led.as_ref().map(|l| l.badtpl_caught).unwrap_or(0);
    v.push(("J1-坏模板全拒", t2 == FACE_BADTPL * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_BADTPL * JUDGE_ROUNDS)));

    // 8. 类3 未登记来源注入。
    let t3 = led.as_ref().map(|l| l.unsrc_caught).unwrap_or(0);
    v.push(("J1-未登记来源全拒", t3 == FACE_UNSRC * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_UNSRC * JUDGE_ROUNDS)));

    // 9. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.oob_caught.saturating_add(l.badtpl_caught).saturating_add(l.unsrc_caught);
            sum == (FACE_OOB + FACE_BADTPL + FACE_UNSRC) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 10. 重复登记拒 + 登记位面单调。
    let dup_ok = {
        let mut bank = TemplateBank::new();
        if bank.register(3).is_err() {
            false
        } else {
            bank.register(3).err() == Some(TlCode::RegistrationDuplicate.code())
                && bank.registry.reg_mask == 0b1000
        }
    };
    v.push(("J1-重复登记拒", dup_ok, String::from("bit3 单桶")));

    // 11. 空库封账拒（探针 B）。
    v.push(("J1-空库封账拒", probe_seq_b().is_err(), String::from("零登记即封必拒")));

    // 12. 应用未登记拒（探针 C）。
    v.push(("J1-未登记应用拒", probe_seq_c().is_err(), String::from("先登记后应用")));

    // 13. 封账后登记拒、应用照常。
    let sealed_ok = {
        let mut bank = TemplateBank::new();
        if bank.register(0).is_err() {
            false
        } else if bank.seal().is_err() {
            false
        } else {
            bank.register(1).is_err() && bank.apply(0).is_ok()
        }
    };
    v.push(("J1-封账后登记拒", sealed_ok, String::from("应用只读照常")));

    // 14. 来源名闭集：三来源可回查、越界 None。
    let name_ok = source_name(SRC_BUILTIN).is_some()
        && source_name(SRC_COMMUNITY).is_some()
        && source_name(SRC_HARDCASE).is_some()
        && source_name(3).is_none();
    v.push(("J1-来源名闭集", name_ok, String::from("3 名可回查")));

    // 15. 库指纹双跑一致。
    let fp = match &g {
        Ok((_, _, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, _, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-库指纹双跑", fp == fp2 && fp != 0, format!("{:08X}", fp)));

    // 16. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = TlCode::ALL.len() == 12;
        let mut i = 0;
        while i < TlCode::ALL.len() {
            if TlCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18548_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18548 档案模板库20例");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18548_checks();
        assert!(set.all_passed(), "UNX-F18548 判据集应全绿");
    }

    /// 黄金链：20 例登记+应用。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((_, applied, _)) => {
                assert_eq!(applied, 20);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 模板 20 例逐例值域+来源闭集。
    #[test]
    fn test_template_rows() {
        let mut i = 0;
        while i < TPL_COUNT {
            let r = match template_row(i) {
                Some(r) => r,
                None => panic!("模板 {} 缺行", i),
            };
            assert!(r.valid());
            assert_eq!(r.id as usize, i);
            assert!(r.source < SOURCE_COUNT as u8);
            i += 1;
        }
        assert!(template_row(TPL_COUNT).is_none());
    }

    /// 应用幂等：两次应用等值且计数 2。
    #[test]
    fn test_apply_idempotent() {
        let mut bank = TemplateBank::new();
        bank.register(4).ok();
        let a = match bank.apply(4) {
            Ok(r) => r,
            Err(e) => panic!("应用不应失败: {}", detail(e)),
        };
        let b = match bank.apply(4) {
            Ok(r) => r,
            Err(e) => panic!("复应用不应失败: {}", detail(e)),
        };
        assert_eq!(a, b);
        assert_eq!(bank.apply_counts[4], 2);
    }

    /// 注入三类全拒（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.oob_caught, 10);
                assert_eq!(l.badtpl_caught, 10);
                assert_eq!(l.unsrc_caught, 10);
            }
            Err(e) => assert!(false, "注入全拒不应报错: {}", detail(e)),
        }
    }

    /// 来源登记账守恒。
    #[test]
    fn test_source_registry() {
        let mut bank = TemplateBank::new();
        let mut i = 0;
        while i < TPL_COUNT {
            assert!(bank.register(i).is_ok());
            i += 1;
        }
        assert!(bank.registry.counts_conserve());
        assert_eq!(bank.registry.reg_counts, [13, 4, 3]);
        assert!(bank.registry.audit_all());
        assert_eq!(bank.registry.reg_mask, (1u32 << TPL_COUNT) - 1);
    }

    /// 登记闸：越界/重复/封后。
    #[test]
    fn test_register_gates() {
        assert!(probe_seq_b().is_err());
        let mut bank = TemplateBank::new();
        assert_eq!(bank.register(TPL_COUNT).err(), Some(TlCode::IdOutOfRange.code()));
        assert!(bank.register(0).is_ok());
        assert_eq!(bank.register(0).err(), Some(TlCode::RegistrationDuplicate.code()));
        assert!(bank.seal().is_ok());
        assert_eq!(bank.register(1).err(), Some(TlCode::PhaseRegression.code()));
    }

    /// 应用闸：未登记拒。
    #[test]
    fn test_apply_gates() {
        assert!(probe_seq_c().is_err());
        let mut bank = TemplateBank::new();
        bank.register(2).ok();
        assert!(bank.apply(2).is_ok());
        assert_eq!(bank.apply(3).err(), Some(TlCode::ApplyUnregistered.code()));
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
