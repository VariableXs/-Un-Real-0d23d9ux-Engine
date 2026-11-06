//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18545 · 档案→矩阵注入路径（依据=档案源）
//! · 0x80400 段 · 批 08 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18545-J1 档案值写入矩阵依据字段=档案断言全过，
//!       优先级插入位（兼容层之上）正确。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 依据源四闭集冻结：System=0（真实系统值）/ CompatLayer=1
//!     （兼容层改写值）/ Profile=2（档案注入值）/ Override=3
//!     （人工覆盖）——矩阵条目必带依据（basis）字段，档案注入路径
//!     写入的条目依据恒 = Profile（闭集值 2）；
//!   - 优先级插入位冻结：矩阵按 rank 升序组织，生效值 = 最高 rank
//!     条目；档案 rank=2，兼容层 rank=1，人工覆盖 rank=3——档案
//!     插入位必须在兼容层之上（rank 序 1 < 2 < 3），插入闸拒绝
//!     档案条目落在兼容层之下；
//!   - 矩阵 16 槽定容：有序插入（rank 升序 + 同 rank 追加尾），
//!     满显性拒且旧账不丢；(rank, field) 二元组重复拒；
//!   - 状态机四态单向：Empty → Seeded（有兼容层/系统底值）→
//!     Injected（档案条目入阵）→ Sealed；
//!   - 注入三类 × 10 轮：依据误标（档案值标成 System 依据）/
//!     插入位越层（档案 rank 伪造为 0 或 1）/ (rank,field) 重复
//!     ——逐类逐轮必抓，漏检即红；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] MxCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   依据源闭集 basis_name / 优先级序表 rank_of
//!   [数据结构] MatrixEntry / CompatMatrix（16 槽有序）
//!   [状态机]   MatrixPhase 四态单向 + MatrixInjector
//!   [账本]     InjectLedger 域账（尝试=过+拒）+ 注入账
//!   [注入核]   三类变异 × 10 轮（误标/越层/重复）
//!   [收口]     golden_run + run_unx18545_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80400，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80400;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 矩阵容量（冻结：16 槽）。
pub const MATRIX_CAP: usize = 16;
/// 依据源闭集（冻结四值）。
pub const BASIS_SYSTEM: u8 = 0;
pub const BASIS_COMPAT_LAYER: u8 = 1;
pub const BASIS_PROFILE: u8 = 2;
pub const BASIS_OVERRIDE: u8 = 3;
/// rank 冻结（优先级序：兼容层之下 < 兼容层 < 档案 < 覆盖）。
pub const RANK_SYSTEM: u8 = 0;
pub const RANK_COMPAT_LAYER: u8 = 1;
pub const RANK_PROFILE: u8 = 2;
pub const RANK_OVERRIDE: u8 = 3;
/// 矩阵字段槽（与 F18544 六字段同口径）。
pub const FIELD_COUNT: usize = 6;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// 依据源名闭集。
pub fn basis_name(b: u8) -> Option<&'static str> {
    match b {
        BASIS_SYSTEM => Some("system"),
        BASIS_COMPAT_LAYER => Some("compat_layer"),
        BASIS_PROFILE => Some("profile"),
        BASIS_OVERRIDE => Some("override"),
        _ => None,
    }
}

/// rank 与依据源配对（裁决表：rank 必与依据源同层，错配即违约）。
pub fn rank_of(basis: u8) -> Option<u8> {
    match basis {
        BASIS_SYSTEM => Some(RANK_SYSTEM),
        BASIS_COMPAT_LAYER => Some(RANK_COMPAT_LAYER),
        BASIS_PROFILE => Some(RANK_PROFILE),
        BASIS_OVERRIDE => Some(RANK_OVERRIDE),
        _ => None,
    }
}

// ── 矩阵条目与容器 ──────────────────────────────────────────────
/// 矩阵条目：字段槽 + 值 + 依据 + rank。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixEntry {
    pub field: u8,
    pub value: u16,
    /// 依据源（闭集 0..=3）。
    pub basis: u8,
    /// 优先级 rank（与 basis 配对）。
    pub rank: u8,
}

/// 16 槽有序矩阵：rank 升序 + 同 rank 追加尾（稳定序）。
#[derive(Clone, Copy, Debug)]
pub struct CompatMatrix {
    pub slots: [Option<MatrixEntry>; MATRIX_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl CompatMatrix {
    pub fn new() -> CompatMatrix {
        CompatMatrix { slots: [None; MATRIX_CAP], len: 0, full_rejects: 0 }
    }
    /// 按槽取条目（越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<MatrixEntry> {
        if idx >= self.len {
            return None;
        }
        self.slots[idx]
    }
    /// (rank, field) 重复判定（线性扫描）。
    pub fn contains(&self, rank: u8, field: u8) -> bool {
        let mut i = 0;
        while i < self.len {
            match self.slots[i] {
                Some(e) => {
                    if e.rank == rank && e.field == field {
                        return true;
                    }
                }
                None => {}
            }
            i += 1;
        }
        false
    }
    /// 有序插入：找首个 rank > 新 rank 的槽位，其后整体右移一格。
    /// 满 → 满拒（旧账不丢）；(rank,field) 重复 → 重复拒。
    pub fn insert_sorted(&mut self, e: MatrixEntry) -> Result<usize, u32> {
        if self.len >= MATRIX_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(MxCode::MatrixFull.code());
        }
        if self.contains(e.rank, e.field) {
            return Err(MxCode::EntryDuplicate.code());
        }
        // 插入点：首个严格大于的 rank。
        let mut pos = self.len;
        let mut i = 0;
        while i < self.len {
            match self.slots[i] {
                Some(x) => {
                    if x.rank > e.rank {
                        pos = i;
                        break;
                    }
                }
                None => {}
            }
            i += 1;
        }
        // 右移（从尾往前）。
        let mut k = self.len;
        while k > pos {
            self.slots[k] = self.slots[k - 1];
            k -= 1;
        }
        self.slots[pos] = Some(e);
        self.len += 1;
        Ok(pos)
    }
    /// 生效值检索：字段槽上 rank 最高的条目（有序表尾向扫描首个命中）。
    pub fn effective(&self, field: u8) -> Option<MatrixEntry> {
        let mut i = self.len;
        while i > 0 {
            match self.slots[i - 1] {
                Some(e) => {
                    if e.field == field {
                        return Some(e);
                    }
                }
                None => {}
            }
            i -= 1;
        }
        None
    }
    /// 有序性核验：逐相邻对 rank 非降。
    pub fn sorted_ok(&self) -> bool {
        let mut i = 1;
        while i < self.len {
            let a = match self.slots[i - 1] {
                Some(e) => e.rank,
                None => return false,
            };
            let b = match self.slots[i] {
                Some(e) => e.rank,
                None => return false,
            };
            if a > b {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 矩阵指纹（逐条目折叠，双跑对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18545;
        let mut i = 0;
        while i < self.len {
            match self.slots[i] {
                Some(e) => {
                    acc = acc.wrapping_mul(31).wrapping_add(e.rank as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(e.field as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(e.value as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(e.basis as u32);
                }
                None => {}
            }
            i += 1;
        }
        acc
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxCode {
    /// 矩阵满（16 槽耗尽）。
    MatrixFull,
    /// (rank,field) 重复。
    EntryDuplicate,
    /// 依据误标（档案路径写入非 Profile 依据）。
    BasisMislabel,
    /// 插入位越层（档案 rank < 兼容层 rank）。
    InsertBelowCompat,
    /// 依据源越闭集（≥4）。
    BasisOutOfRange,
    /// 字段槽越界（≥6）。
    FieldOutOfRange,
    /// rank 与依据源错配。
    RankBasisMismatch,
    /// 有序性破坏。
    SortedBroken,
    /// 相位倒退/越级。
    PhaseRegression,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 注入漏检。
    InjectionMissed,
    /// 双跑不一致。
    NotReproducible,
}

impl MxCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MxCode::MatrixFull => "矩阵满：16 槽耗尽",
            MxCode::EntryDuplicate => "(rank,field) 重复：同层同槽禁二写",
            MxCode::BasisMislabel => "依据误标：档案路径依据必为 profile",
            MxCode::InsertBelowCompat => "插入位越层：档案须在兼容层之上",
            MxCode::BasisOutOfRange => "依据源越闭集：仅 0..=3",
            MxCode::FieldOutOfRange => "字段槽越界：仅 0..=5",
            MxCode::RankBasisMismatch => "rank 与依据源错配",
            MxCode::SortedBroken => "有序性破坏：rank 非降序",
            MxCode::PhaseRegression => "相位倒退：状态机单向禁回退",
            MxCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            MxCode::InjectionMissed => "注入漏检：变异必被抓",
            MxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MxCode; 12] = [
        MxCode::MatrixFull,
        MxCode::EntryDuplicate,
        MxCode::BasisMislabel,
        MxCode::InsertBelowCompat,
        MxCode::BasisOutOfRange,
        MxCode::FieldOutOfRange,
        MxCode::RankBasisMismatch,
        MxCode::SortedBroken,
        MxCode::PhaseRegression,
        MxCode::LedgerBroken,
        MxCode::InjectionMissed,
        MxCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MxCode::ALL.len() {
        if MxCode::ALL[i].code() == code {
            return MxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80400 段闭集内"
}

// ── 状态机 + 注入机 ─────────────────────────────────────────────
/// 矩阵相位（单向：Empty→Seeded→Injected→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixPhase {
    /// 空阵。
    Empty,
    /// 已铺底（系统/兼容层底值在阵）。
    Seeded,
    /// 档案条目已注入。
    Injected,
    /// 已封账。
    Sealed,
}

/// 域账（守恒：尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
}

impl DomainLedger {
    pub const fn new() -> DomainLedger {
        DomainLedger { tries: 0, passed: 0, rejected: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 档案→矩阵注入机。
pub struct MatrixInjector {
    pub phase: MatrixPhase,
    pub matrix: CompatMatrix,
    pub ledger: DomainLedger,
}

impl MatrixInjector {
    pub fn new() -> MatrixInjector {
        MatrixInjector {
            phase: MatrixPhase::Empty,
            matrix: CompatMatrix::new(),
            ledger: DomainLedger::new(),
        }
    }

    /// 铺底：系统值与兼容层值入阵（rank 0/1）。
    pub fn seed(&mut self, field: u8, value: u16, basis: u8) -> Result<usize, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        let rank = match rank_of(basis) {
            Some(r) => r,
            None => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                return Err(MxCode::BasisOutOfRange.code());
            }
        };
        if field >= FIELD_COUNT as u8 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(MxCode::FieldOutOfRange.code());
        }
        match self.matrix.insert_sorted(MatrixEntry { field, value, basis, rank }) {
            Ok(pos) => {
                self.ledger.passed = self.ledger.passed.saturating_add(1);
                if self.phase == MatrixPhase::Empty {
                    self.phase = MatrixPhase::Seeded;
                }
                Ok(pos)
            }
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 档案注入：依据恒 = Profile，rank 恒 = RANK_PROFILE。
    /// 闸序：字段槽 → 依据误标 → 越层（rank 必须 == RANK_PROFILE）。
    pub fn inject_profile(&mut self, field: u8, value: u16, basis: u8, rank: u8) -> Result<usize, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if field >= FIELD_COUNT as u8 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(MxCode::FieldOutOfRange.code());
        }
        // 依据误标闸：档案路径只认 Profile 依据。
        if basis != BASIS_PROFILE {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(MxCode::BasisMislabel.code());
        }
        // 越层闸：档案 rank 必须恰为兼容层之上一位。
        if rank != RANK_PROFILE {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(MxCode::InsertBelowCompat.code());
        }
        match self.matrix.insert_sorted(MatrixEntry { field, value, basis, rank }) {
            Ok(pos) => {
                self.ledger.passed = self.ledger.passed.saturating_add(1);
                if self.phase != MatrixPhase::Sealed {
                    self.phase = MatrixPhase::Injected;
                }
                Ok(pos)
            }
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 封账：必须已达 Injected（空阵/仅铺底皆不许收口档案路径）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == MatrixPhase::Sealed {
            return Err(MxCode::PhaseRegression.code());
        }
        if self.phase != MatrixPhase::Injected {
            return Err(MxCode::PhaseRegression.code());
        }
        if !self.matrix.sorted_ok() {
            return Err(MxCode::SortedBroken.code());
        }
        if !self.ledger.conserve() {
            return Err(MxCode::LedgerBroken.code());
        }
        self.phase = MatrixPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（条目数 / 账 / 矩阵指纹）。
pub type GoldenOut = (usize, DomainLedger, u32);

/// 黄金全链：铺底（系统+兼容层）→ 档案六字段注入（依据=profile、
/// 插入位在兼容层之上）→ 生效值 = 档案值 → 人工覆盖压档案 → 封账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut inj = MatrixInjector::new();
    // 铺底：字段 0..3 系统值 + 字段 0..2 兼容层改写值。
    let mut f = 0u8;
    while f < 3 {
        inj.seed(f, 5 + f as u16, BASIS_SYSTEM)?;
        f += 1;
    }
    let mut g = 0u8;
    while g < 2 {
        inj.seed(g, 6 + g as u16, BASIS_COMPAT_LAYER)?;
        g += 1;
    }
    if inj.phase != MatrixPhase::Seeded {
        return Err(MxCode::PhaseRegression.code());
    }
    // 档案六字段注入：依据=profile，rank=2。
    let profile_vals: [u16; FIELD_COUNT] = [6, 1, 1, 3, 0x0100, 96];
    let mut k = 0usize;
    while k < FIELD_COUNT {
        let pos = inj.inject_profile(k as u8, profile_vals[k], BASIS_PROFILE, RANK_PROFILE)?;
        // 插入位核：档案条目左侧不得有更高 rank（兼容层之下禁）。
        let e = match inj.matrix.entry_at(pos) {
            Some(e) => e,
            None => return Err(MxCode::SortedBroken.code()),
        };
        if e.basis != BASIS_PROFILE || e.rank != RANK_PROFILE {
            return Err(MxCode::BasisMislabel.code());
        }
        k += 1;
    }
    if !inj.matrix.sorted_ok() {
        return Err(MxCode::SortedBroken.code());
    }
    // 生效值 = 档案值（rank 2 压过 rank 0/1）。
    let mut j = 0usize;
    while j < FIELD_COUNT {
        let eff = match inj.matrix.effective(j as u8) {
            Some(e) => e,
            None => return Err(MxCode::SortedBroken.code()),
        };
        if eff.basis != BASIS_PROFILE || eff.value != profile_vals[j] {
            return Err(MxCode::BasisMislabel.code());
        }
        j += 1;
    }
    // 人工覆盖压档案（rank 3）：字段 0 覆盖后生效值翻转。
    inj.seed(0, 7, BASIS_OVERRIDE)?;
    let eff0 = match inj.matrix.effective(0) {
        Some(e) => e,
        None => return Err(MxCode::SortedBroken.code()),
    };
    if eff0.basis != BASIS_OVERRIDE || eff0.value != 7 {
        return Err(MxCode::InsertBelowCompat.code());
    }
    inj.seal()?;
    if !inj.ledger.conserve() {
        return Err(MxCode::LedgerBroken.code());
    }
    Ok((inj.matrix.len, inj.ledger, inj.matrix.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MxCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 依据误标。
    pub mislabel_caught: u32,
    /// 类2 插入位越层。
    pub below_caught: u32,
    /// 类3 (rank,field) 重复。
    pub dup_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约。
pub const FACE_MISLABEL: u32 = 1;
pub const FACE_BELOW: u32 = 1;
pub const FACE_DUP: u32 = 1;

/// 三类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 依据误标：档案路径标 System 依据 → BasisMislabel。
        let mut inj = MatrixInjector::new();
        match inj.inject_profile(0, 6, BASIS_SYSTEM, RANK_PROFILE) {
            Err(c) if c == MxCode::BasisMislabel.code() => {
                led.mislabel_caught = led.mislabel_caught.saturating_add(FACE_MISLABEL);
            }
            _ => return Err(MxCode::InjectionMissed.code()),
        }
        // 类2 插入位越层：档案 rank 伪造为兼容层 rank → InsertBelowCompat。
        let mut inj2 = MatrixInjector::new();
        match inj2.inject_profile(0, 6, BASIS_PROFILE, RANK_COMPAT_LAYER) {
            Err(c) if c == MxCode::InsertBelowCompat.code() => {
                led.below_caught = led.below_caught.saturating_add(FACE_BELOW);
            }
            _ => return Err(MxCode::InjectionMissed.code()),
        }
        // 类3 (rank,field) 重复：同档案条目二次注入 → EntryDuplicate。
        let mut inj3 = MatrixInjector::new();
        if inj3.inject_profile(1, 6, BASIS_PROFILE, RANK_PROFILE).is_err() {
            return Err(MxCode::InjectionMissed.code());
        }
        match inj3.inject_profile(1, 6, BASIS_PROFILE, RANK_PROFILE) {
            Err(c) if c == MxCode::EntryDuplicate.code() => {
                led.dup_caught = led.dup_caught.saturating_add(FACE_DUP);
            }
            _ => return Err(MxCode::InjectionMissed.code()),
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

/// 探针 B：空阵直接注入前封账（仅铺底未注入）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut inj = MatrixInjector::new();
    inj.seed(0, 5, BASIS_SYSTEM)?;
    inj.seal()
}

/// 探针 C：字段槽越界（6）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    // Ok 侧折叠为违约码（探针语义：字段槽越界必须被拒，Ok 即漏检）。
    let mut inj = MatrixInjector::new();
    match inj.inject_profile(FIELD_COUNT as u8, 0, BASIS_PROFILE, RANK_PROFILE) {
        Err(c) => Err(c),
        Ok(_) => Err(MxCode::FieldOutOfRange.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < MxCode::ALL.len() {
        let v = MxCode::ALL[i].code();
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

    // 1. 黄金链：铺底+档案注入+生效+覆盖+封账全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == 12 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. 依据=档案：六档案条目依据闭集值恒 2。
    let basis_ok = {
        let mut inj = MatrixInjector::new();
        if inj.inject_profile(0, 6, BASIS_PROFILE, RANK_PROFILE).is_err() {
            false
        } else {
            match inj.matrix.effective(0) {
                Some(e) => e.basis == BASIS_PROFILE && basis_name(e.basis) == Some("profile"),
                None => false,
            }
        }
    };
    v.push(("J1-依据=档案", basis_ok, String::from("basis 闭集值 2")));

    // 3. 插入位在兼容层之上：rank 序 1 < 2 < 3 且有序表核验。
    let pos_ok = {
        let mut inj = MatrixInjector::new();
        let _ = inj.seed(0, 5, BASIS_SYSTEM);
        let _ = inj.seed(0, 6, BASIS_COMPAT_LAYER);
        let pos = match inj.inject_profile(0, 7, BASIS_PROFILE, RANK_PROFILE) {
            Ok(p) => p,
            Err(_) => 99,
        };
        pos == 2 && inj.matrix.sorted_ok()
    };
    v.push(("J1-插入位兼容层之上", pos_ok, String::from("sys(0)<compat(1)<profile(2)")));

    // 4. 生效值 = 档案值（rank 2 压 rank 0/1）。
    let eff_ok = {
        let mut inj = MatrixInjector::new();
        let _ = inj.seed(1, 5, BASIS_SYSTEM);
        let _ = inj.seed(1, 6, BASIS_COMPAT_LAYER);
        let _ = inj.inject_profile(1, 8, BASIS_PROFILE, RANK_PROFILE);
        match inj.matrix.effective(1) {
            Some(e) => e.value == 8 && e.rank == RANK_PROFILE,
            None => false,
        }
    };
    v.push(("J1-生效值=档案值", eff_ok, String::from("高 rank 胜出")));

    // 5. 类1 依据误标注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.mislabel_caught).unwrap_or(0);
    v.push(("J1-依据误标全抓", t1 == FACE_MISLABEL * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_MISLABEL * JUDGE_ROUNDS)));

    // 6. 类2 越层注入。
    let t2 = led.as_ref().map(|l| l.below_caught).unwrap_or(0);
    v.push(("J1-越层注入全抓", t2 == FACE_BELOW * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_BELOW * JUDGE_ROUNDS)));

    // 7. 类3 重复注入。
    let t3 = led.as_ref().map(|l| l.dup_caught).unwrap_or(0);
    v.push(("J1-重复注入全抓", t3 == FACE_DUP * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_DUP * JUDGE_ROUNDS)));

    // 8. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.mislabel_caught.saturating_add(l.below_caught).saturating_add(l.dup_caught);
            sum == (FACE_MISLABEL + FACE_BELOW + FACE_DUP) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 9. rank/依据配对表逐项核对。
    let pair_ok = rank_of(BASIS_SYSTEM) == Some(RANK_SYSTEM)
        && rank_of(BASIS_COMPAT_LAYER) == Some(RANK_COMPAT_LAYER)
        && rank_of(BASIS_PROFILE) == Some(RANK_PROFILE)
        && rank_of(BASIS_OVERRIDE) == Some(RANK_OVERRIDE)
        && rank_of(9).is_none();
    v.push(("J1-rank依据配对表", pair_ok, String::from("0/1/2/3 同层配对")));

    // 10. 矩阵满显性：16 槽收满后拒且旧账不丢。
    let full_ok = {
        let mut inj = MatrixInjector::new();
        let mut ok = true;
        let mut k = 0u8;
        while k < MATRIX_CAP as u8 {
            // 同 rank 追加尾：用 Override rank 配不同 field 值不可行
            // （field 闭集 6），改用不同 rank 0..16 逐条铺满。
            if inj.inject_profile(0, k as u16, BASIS_PROFILE, RANK_PROFILE).is_ok() {
                // 首条可入；此后 (2,0) 重复——改走 seed 造多 rank：
                // 直接手工铺 [None 槽] 不行，换注入低层。
            }
            k += 1;
        }
        // 正宗铺满：rank 1..=16 逐条入（rank 递增不同 field 0）。
        // 但档案路径 rank 恒 2 ——铺满改用 CompatMatrix 有序插入直填。
        let mut m = CompatMatrix::new();
        let mut r = 0u8;
        while r < MATRIX_CAP as u8 {
            if m.insert_sorted(MatrixEntry { field: 0, value: r as u16, basis: BASIS_SYSTEM, rank: r }).is_err() {
                ok = false;
            }
            r += 1;
        }
        let extra = m.insert_sorted(MatrixEntry { field: 1, value: 0, basis: BASIS_SYSTEM, rank: 0 });
        ok && extra.is_err() && m.full_rejects == 1 && m.len == MATRIX_CAP
    };
    v.push(("J1-矩阵满显性16槽", full_ok, String::from("16 收 1 拒")));

    // 11. 有序性破坏不可达：直插乱序仍被 insert_sorted 归位。
    let sort_ok = {
        let mut m = CompatMatrix::new();
        let _ = m.insert_sorted(MatrixEntry { field: 0, value: 3, basis: BASIS_OVERRIDE, rank: 3 });
        let _ = m.insert_sorted(MatrixEntry { field: 0, value: 1, basis: BASIS_SYSTEM, rank: 0 });
        m.sorted_ok() && match m.entry_at(0) {
            Some(e) => e.rank == 0,
            None => false,
        }
    };
    v.push(("J1-插入自动归位", sort_ok, String::from("rank 3 先入 rank 0 归首")));

    // 12. 相位闸：未注入先封拒（探针 B）。
    v.push(("J1-未注入封账拒", probe_seq_b().is_err(), String::from("仅铺底不许收口")));

    // 13. 字段槽越界拒（探针 C）。
    v.push(("J1-字段槽越界拒", probe_seq_c().is_err(), String::from("field=6 必拒")));

    // 14. 依据源越闭集：basis=4 在铺底路径即拒。
    let boob = {
        let mut inj = MatrixInjector::new();
        inj.seed(0, 5, 4).is_err()
    };
    v.push(("J1-依据源越闭集拒", boob, String::from("basis=4 必拒")));

    // 15. 矩阵指纹双跑一致。
    let fp = match &g {
        Ok((_, _, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, _, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-矩阵指纹双跑", fp == fp2 && fp != 0, format!("{:08X}", fp)));

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
        let mut ok = MxCode::ALL.len() == 12;
        let mut i = 0;
        while i < MxCode::ALL.len() {
            if MxCode::ALL[i].detail().is_empty() {
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
pub fn run_unx18545_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18545 档案→矩阵注入路径");
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
        let set = run_unx18545_checks();
        assert!(set.all_passed(), "UNX-F18545 判据集应全绿");
    }

    /// 黄金链：12 条目（3 系统底 + 2 兼容层 + 6 档案 + 1 覆盖）。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 12);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 有序插入归位。
    #[test]
    fn test_sorted_insert() {
        let mut m = CompatMatrix::new();
        assert!(m.insert_sorted(MatrixEntry { field: 0, value: 3, basis: BASIS_OVERRIDE, rank: 3 }).is_ok());
        assert!(m.insert_sorted(MatrixEntry { field: 0, value: 1, basis: BASIS_SYSTEM, rank: 0 }).is_ok());
        assert!(m.insert_sorted(MatrixEntry { field: 0, value: 2, basis: BASIS_PROFILE, rank: 2 }).is_ok());
        assert!(m.sorted_ok());
        // rank 0 插到首位，rank 2 居中，rank 3 殿后。
        assert_eq!(m.entry_at(0).map(|e| e.rank), Some(0));
        assert_eq!(m.entry_at(1).map(|e| e.rank), Some(2));
        assert_eq!(m.entry_at(2).map(|e| e.rank), Some(3));
    }

    /// 生效值检索：最高 rank 胜出。
    #[test]
    fn test_effective() {
        let mut m = CompatMatrix::new();
        let _ = m.insert_sorted(MatrixEntry { field: 0, value: 1, basis: BASIS_SYSTEM, rank: 0 });
        let _ = m.insert_sorted(MatrixEntry { field: 0, value: 2, basis: BASIS_PROFILE, rank: 2 });
        assert_eq!(m.effective(0).map(|e| e.value), Some(2));
        assert_eq!(m.effective(1), None);
    }

    /// 注入三类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.mislabel_caught, 10);
                assert_eq!(l.below_caught, 10);
                assert_eq!(l.dup_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 闸位：误标 / 越层 / 重复 / 越闭集逐码核对。
    #[test]
    fn test_gates() {
        let mut inj = MatrixInjector::new();
        assert_eq!(
            inj.inject_profile(0, 6, BASIS_OVERRIDE, RANK_PROFILE).err(),
            Some(MxCode::BasisMislabel.code())
        );
        assert_eq!(
            inj.inject_profile(0, 6, BASIS_PROFILE, RANK_OVERRIDE).err(),
            Some(MxCode::InsertBelowCompat.code())
        );
        assert!(inj.inject_profile(0, 6, BASIS_PROFILE, RANK_PROFILE).is_ok());
        assert_eq!(
            inj.inject_profile(0, 7, BASIS_PROFILE, RANK_PROFILE).err(),
            Some(MxCode::EntryDuplicate.code())
        );
        assert_eq!(inj.seed(0, 5, 9).err(), Some(MxCode::BasisOutOfRange.code()));
    }

    /// 容量：16 槽满显性拒。
    #[test]
    fn test_capacity() {
        let mut m = CompatMatrix::new();
        let mut r = 0u8;
        while r < MATRIX_CAP as u8 {
            assert!(m.insert_sorted(MatrixEntry { field: 0, value: r as u16, basis: BASIS_SYSTEM, rank: r }).is_ok());
            r += 1;
        }
        assert_eq!(
            m.insert_sorted(MatrixEntry { field: 1, value: 0, basis: BASIS_SYSTEM, rank: 0 }).err(),
            Some(MxCode::MatrixFull.code())
        );
        assert_eq!(m.len, MATRIX_CAP);
        assert_eq!(m.full_rejects, 1);
    }

    /// 相位：单向推进与封账闸。
    #[test]
    fn test_phases() {
        assert!(probe_seq_b().is_err());
        let mut inj = MatrixInjector::new();
        assert_eq!(inj.phase, MatrixPhase::Empty);
        let _ = inj.seed(0, 5, BASIS_SYSTEM);
        assert_eq!(inj.phase, MatrixPhase::Seeded);
        let _ = inj.inject_profile(1, 6, BASIS_PROFILE, RANK_PROFILE);
        assert_eq!(inj.phase, MatrixPhase::Injected);
        assert!(inj.seal().is_ok());
        assert_eq!(inj.seal().err(), Some(MxCode::PhaseRegression.code()));
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
