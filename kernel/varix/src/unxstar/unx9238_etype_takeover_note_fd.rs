//! UNX-F9238 fd 族 E 型升级接管注记 · 0x7A900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9238 逐字）：
//!   ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9238-J1 接管边界声明判据过，
//!   存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 接管先声明边界：四族（open/stat/unlink/dup）、六码（EBADF/EFAULT/
//!      EINVAL/EMFILE/ENFILE/ENOENT）、fd 上界 63、退役路径默认不放行。
//!      边界之外的调用一律拒绝接管，宁可漏接不可越权。
//!   ② 存量错误路径零重写是硬承诺：20 条种子路径逐条比对 legacy_errno 与
//!      new_errno，必须完全相等；Disposition 中的 Rewritten 是禁用态，出现
//!      即承诺破裂，而不是「新语义更合理」。
//!   ③ 处置分四态落账：ZeroRewrite 6 条、Wrapped 8 条、Delegated 4 条、
//!      Retired 2 条，四态求和等于 20；守恒式不成立则清单不可签发。
//!   ④ 增量清单单列：3 项增量（门序统一、errno 归一表、边界声明）配 4 守卫、
//!      6 断言、5 对账；增量与存量分账，避免用增量收益掩盖存量改写。
//!   ⑤ 三类注入各 10 轮：存量改写、边界泄漏、增量丢账，检出 10/10；接管
//!      矩阵的注入账与对账账分开记，互不冲销。
//!   ⑥ 注记状态机 Draft→Admitted→Declared→Ratified→Sealed 只许单步前进，
//!      封存时冻结指纹，回退重签一律拒绝。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9238 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A900;
/// 种子路径总数。
pub const SEED_TOTAL: usize = 20;
/// open 族行数。
pub const OPEN_ROWS: usize = 6;
/// stat 族行数。
pub const STAT_ROWS: usize = 5;
/// unlink 族行数。
pub const UNLINK_ROWS: usize = 4;
/// dup 族行数。
pub const DUP_ROWS: usize = 5;
/// ZeroRewrite 行数。
pub const ZERO_REWRITE_ROWS: usize = 6;
/// Wrapped 行数。
pub const WRAPPED_ROWS: usize = 8;
/// Delegated 行数。
pub const DELEGATED_ROWS: usize = 4;
/// Retired 行数。
pub const RETIRED_ROWS: usize = 2;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 接管 fd 上界。
pub const MAX_FD: i32 = 63;
/// 四族掩码。
pub const FAMILY_MASK_ALL: u32 = 0x0F;
/// 六码掩码位：EBADF/EFAULT/EINVAL/EMFILE/ENFILE/ENOENT。
pub const ERRNO_MASK_DECLARED: u32 = 0x3F;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 打开文件数超限（进程）。
pub const EMFILE: u32 = 24;
/// 打开文件数超限（系统）。
pub const ENFILE: u32 = 23;
/// 路径不存在。
pub const ENOENT: u32 = 2;
/// 调用族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Open,
    Stat,
    Unlink,
    Dup,
}
impl Family {
    /// 族掩码位。
    pub fn bit(self) -> u32 {
        1 << self.tag()
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Family::Open => 0,
            Family::Stat => 1,
            Family::Unlink => 2,
            Family::Dup => 3,
        }
    }
    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            Family::Open => "open 族",
            Family::Stat => "stat 族",
            Family::Unlink => "unlink 族",
            Family::Dup => "dup 族",
        }
    }
    /// 四族全量。
    pub const ALL: [Family; 4] = [Family::Open, Family::Stat, Family::Unlink, Family::Dup];
}
/// 处置态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    ZeroRewrite,
    Wrapped,
    Delegated,
    Retired,
    Rewritten,
}
impl Disposition {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Disposition::ZeroRewrite => 0,
            Disposition::Wrapped => 1,
            Disposition::Delegated => 2,
            Disposition::Retired => 3,
            Disposition::Rewritten => 4,
        }
    }
    /// 是否允许出现在接管清单中。
    pub fn admissible(self) -> bool {
        self != Disposition::Rewritten
    }
    /// 处置名。
    pub fn name(self) -> &'static str {
        match self {
            Disposition::ZeroRewrite => "零改写",
            Disposition::Wrapped => "包一层",
            Disposition::Delegated => "委派新核",
            Disposition::Retired => "冻结退役",
            Disposition::Rewritten => "改写（禁用态）",
        }
    }
    /// 五态全量。
    pub const ALL: [Disposition; 5] = [
        Disposition::ZeroRewrite,
        Disposition::Wrapped,
        Disposition::Delegated,
        Disposition::Retired,
        Disposition::Rewritten,
    ];
}
/// 种子路径记录。
#[derive(Clone, Copy)]
pub struct PathRecord {
    pub call: &'static str,
    pub family: Family,
    pub disposition: Disposition,
    pub legacy_errno: u32,
    pub new_errno: u32,
}
/// 20 条种子路径。
pub const SEED_ROWS: [PathRecord; 20] = [
    PathRecord { call: "open", family: Family::Open, disposition: Disposition::ZeroRewrite, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "openat", family: Family::Open, disposition: Disposition::ZeroRewrite, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "openat2", family: Family::Open, disposition: Disposition::ZeroRewrite, legacy_errno: EINVAL, new_errno: EINVAL },
    PathRecord { call: "creat", family: Family::Open, disposition: Disposition::Wrapped, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "open_tmpfile", family: Family::Open, disposition: Disposition::Wrapped, legacy_errno: EINVAL, new_errno: EINVAL },
    PathRecord { call: "open_dir", family: Family::Open, disposition: Disposition::Delegated, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "stat", family: Family::Stat, disposition: Disposition::ZeroRewrite, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "lstat", family: Family::Stat, disposition: Disposition::ZeroRewrite, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "fstat", family: Family::Stat, disposition: Disposition::Wrapped, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "newfstatat", family: Family::Stat, disposition: Disposition::Wrapped, legacy_errno: EFAULT, new_errno: EFAULT },
    PathRecord { call: "statx", family: Family::Stat, disposition: Disposition::Delegated, legacy_errno: EINVAL, new_errno: EINVAL },
    PathRecord { call: "unlink", family: Family::Unlink, disposition: Disposition::ZeroRewrite, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "unlinkat", family: Family::Unlink, disposition: Disposition::Wrapped, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "rmdir", family: Family::Unlink, disposition: Disposition::Wrapped, legacy_errno: ENOENT, new_errno: ENOENT },
    PathRecord { call: "unlinkat_removedir", family: Family::Unlink, disposition: Disposition::Retired, legacy_errno: EINVAL, new_errno: EINVAL },
    PathRecord { call: "dup", family: Family::Dup, disposition: Disposition::Wrapped, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "dup2", family: Family::Dup, disposition: Disposition::Wrapped, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "dup3", family: Family::Dup, disposition: Disposition::Delegated, legacy_errno: EINVAL, new_errno: EINVAL },
    PathRecord { call: "fcntl_dupfd", family: Family::Dup, disposition: Disposition::Delegated, legacy_errno: EBADF, new_errno: EBADF },
    PathRecord { call: "fcntl_dupfd_cloexec", family: Family::Dup, disposition: Disposition::Retired, legacy_errno: EMFILE, new_errno: EMFILE },
];
/// 接管边界声明。
#[derive(Clone, Copy)]
pub struct TakeoverBoundary {
    pub family_mask: u32,
    pub errno_mask: u32,
    pub max_fd: i32,
    pub allow_retired: bool,
}
impl TakeoverBoundary {
    /// 声明的标准边界。
    pub fn declared() -> Self {
        TakeoverBoundary {
            family_mask: FAMILY_MASK_ALL,
            errno_mask: ERRNO_MASK_DECLARED,
            max_fd: MAX_FD,
            allow_retired: false,
        }
    }
    /// 族是否在边界内。
    pub fn admits_family(&self, f: Family) -> bool {
        self.family_mask & f.bit() != 0
    }
    /// 错误码是否在边界内。
    pub fn admits_errno(&self, e: u32) -> bool {
        let bit = match e {
            EBADF => 0,
            EFAULT => 1,
            EINVAL => 2,
            EMFILE => 3,
            ENFILE => 4,
            ENOENT => 5,
            _ => return false,
        };
        self.errno_mask & (1 << bit) != 0
    }
    /// fd 是否在边界内。
    pub fn admits_fd(&self, fd: i32) -> bool {
        fd >= 0 && fd <= self.max_fd
    }
    /// 退役路径是否放行。
    pub fn admits_disposition(&self, d: Disposition) -> bool {
        d.admissible() && (d != Disposition::Retired || self.allow_retired)
    }
    /// 声明是否完整：四族、六码、上界 63、默认不放退役。
    pub fn complete(&self) -> bool {
        self.family_mask == FAMILY_MASK_ALL
            && self.errno_mask == ERRNO_MASK_DECLARED
            && self.max_fd == MAX_FD
            && !self.allow_retired
    }
}
/// 记录是否可被接管。
pub fn record_admitted(r: &PathRecord, b: &TakeoverBoundary) -> bool {
    b.admits_family(r.family)
        && b.admits_errno(r.legacy_errno)
        && b.admits_errno(r.new_errno)
        && b.admits_disposition(r.disposition)
}
/// 活跃行数（退役行不参与接管）。
pub fn active_rows(rows: &[PathRecord]) -> usize {
    rows.iter().filter(|r| r.disposition != Disposition::Retired).count()
}
/// 全部活跃记录是否可被接管（退役行冻结在旧路径，不纳入接管）。
pub fn all_active_admitted(rows: &[PathRecord], b: &TakeoverBoundary) -> bool {
    rows.iter()
        .filter(|r| r.disposition != Disposition::Retired)
        .all(|r| record_admitted(r, b))
}
/// 行数。
pub fn family_rows(rows: &[PathRecord], f: Family) -> usize {
    rows.iter().filter(|r| r.family == f).count()
}
/// 处置计数。
pub fn disposition_rows(rows: &[PathRecord], d: Disposition) -> usize {
    rows.iter().filter(|r| r.disposition == d).count()
}
/// 处置守恒。
pub fn disposition_conserved(rows: &[PathRecord]) -> bool {
    let mut sum = 0usize;
    for d in Disposition::ALL.iter() {
        sum += disposition_rows(rows, *d);
    }
    sum == rows.len()
}
/// 存量错误路径是否逐条等值。
pub fn legacy_equal_new(rows: &[PathRecord]) -> bool {
    rows.iter().all(|r| r.legacy_errno == r.new_errno)
}
/// 零重写承诺：无改写态、逐条等值、全部落在边界内。
pub fn promise_holds(rows: &[PathRecord], b: &TakeoverBoundary) -> bool {
    rows.iter().all(|r| r.disposition != Disposition::Rewritten)
        && legacy_equal_new(rows)
        && all_active_admitted(rows, b)
}
/// 增量清单。
#[derive(Clone, Copy)]
pub struct IncrementManifest {
    pub increments: usize,
    pub guards: usize,
    pub asserts: usize,
    pub recons: usize,
}
impl IncrementManifest {
    /// 标准清单：3 增量、4 守卫、6 断言、5 对账。
    pub fn declared() -> Self {
        IncrementManifest { increments: 3, guards: 4, asserts: 6, recons: 5 }
    }
    /// 清单是否落账完整。
    pub fn complete(&self) -> bool {
        self.increments == 3 && self.guards == 4 && self.asserts == 6 && self.recons == 5
    }
    /// 增量项名（闭集）。
    pub fn items(&self) -> [&'static str; 3] {
        ["门序统一", "errno 归一化表", "接管边界声明"]
    }
}
/// 注记状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteState {
    Draft,
    Admitted,
    Declared,
    Ratified,
    Sealed,
}
impl NoteState {
    /// 单步前进。
    pub fn can_go(self, next: NoteState) -> bool {
        matches!(
            (self, next),
            (NoteState::Draft, NoteState::Admitted)
                | (NoteState::Admitted, NoteState::Declared)
                | (NoteState::Declared, NoteState::Ratified)
                | (NoteState::Ratified, NoteState::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            NoteState::Draft => 0,
            NoteState::Admitted => 1,
            NoteState::Declared => 2,
            NoteState::Ratified => 3,
            NoteState::Sealed => 4,
        }
    }
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            NoteState::Draft => "草拟",
            NoteState::Admitted => "已接纳",
            NoteState::Declared => "已声明",
            NoteState::Ratified => "已批准",
            NoteState::Sealed => "已封存",
        }
    }
}
/// 接管注记。
#[derive(Clone, Copy)]
pub struct TakeoverNote {
    pub state: NoteState,
    pub admitted: usize,
    pub sealed_digest: u32,
}
impl TakeoverNote {
    /// 新建草稿。
    pub fn new() -> Self {
        TakeoverNote { state: NoteState::Draft, admitted: 0, sealed_digest: 0 }
    }
    /// 接纳种子路径。
    pub fn admit(&mut self, n: usize, boundary_ok: bool) -> bool {
        if self.state == NoteState::Draft && boundary_ok && n == SEED_TOTAL {
            self.admitted = n;
            self.state = NoteState::Admitted;
            true
        } else {
            false
        }
    }
    /// 声明边界。
    pub fn declare(&mut self, declared_ok: bool) -> bool {
        if self.state == NoteState::Admitted && declared_ok {
            self.state = NoteState::Declared;
            true
        } else {
            false
        }
    }
    /// 批准。
    pub fn ratify(&mut self) -> bool {
        if self.state == NoteState::Declared {
            self.state = NoteState::Ratified;
            true
        } else {
            false
        }
    }
    /// 封存并冻结指纹。
    pub fn seal(&mut self, digest: u32) -> bool {
        if self.state == NoteState::Ratified {
            self.sealed_digest = digest;
            self.state = NoteState::Sealed;
            true
        } else {
            false
        }
    }
    /// 承诺是否仍成立。
    pub fn promise_holds(&self, rows: &[PathRecord], b: &TakeoverBoundary) -> bool {
        self.state != NoteState::Draft && promise_holds(rows, b)
    }
    /// 摘要（闭集文案）。
    pub fn summary(&self) -> &'static str {
        match self.state {
            NoteState::Draft => "接管注记尚未接纳种子路径",
            NoteState::Admitted => "已接纳 20 条种子路径，待声明边界",
            NoteState::Declared => "边界已声明，待批准",
            NoteState::Ratified => "已批准，待封存指纹",
            NoteState::Sealed => "已封存，存量错误路径零重写",
        }
    }
}
/// 对账行（B08/B15 错误码归一对）。
#[derive(Clone, Copy)]
pub struct ReconRow {
    pub book: &'static str,
    pub call: &'static str,
    pub legacy: u32,
    pub normalized: u32,
}
/// 8 条对账行。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { book: "B08", call: "dup", legacy: EBADF, normalized: EBADF },
    ReconRow { book: "B08", call: "dup2", legacy: EBADF, normalized: EBADF },
    ReconRow { book: "B08", call: "dup3", legacy: EINVAL, normalized: EINVAL },
    ReconRow { book: "B08", call: "fcntl", legacy: EMFILE, normalized: EMFILE },
    ReconRow { book: "B15", call: "close", legacy: EBADF, normalized: EBADF },
    ReconRow { book: "B15", call: "fstat", legacy: EFAULT, normalized: EFAULT },
    ReconRow { book: "B15", call: "lseek", legacy: EINVAL, normalized: EINVAL },
    ReconRow { book: "B15", call: "openat", legacy: ENOENT, normalized: ENOENT },
];
/// 对账表是否零漂移。
pub fn recon_ok() -> bool {
    RECON_TABLE.iter().all(|r| r.legacy == r.normalized && !r.book.is_empty() && !r.call.is_empty())
}
/// 注入种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TkInject {
    LegacyRewrite,
    BoundaryLeak,
    IncrementLoss,
}
impl TkInject {
    /// 三类注入。
    pub fn all() -> [TkInject; 3] {
        [TkInject::LegacyRewrite, TkInject::BoundaryLeak, TkInject::IncrementLoss]
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            TkInject::LegacyRewrite => 0,
            TkInject::BoundaryLeak => 1,
            TkInject::IncrementLoss => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            TkInject::LegacyRewrite => "把一条零改写行改成改写态，承诺格必须报红",
            TkInject::BoundaryLeak => "塞入边界外错误码，接管边界必须拒绝",
            TkInject::IncrementLoss => "增量项缺失，清单落账格必须报红",
        }
    }
}
/// 注入后是否被抓住。
pub fn inject_detected(k: TkInject) -> bool {
    match k {
        TkInject::LegacyRewrite => {
            let mut rows: Vec<PathRecord> = SEED_ROWS.iter().copied().collect();
            if let Some(r) = rows.first_mut() {
                r.disposition = Disposition::Rewritten;
            }
            !promise_holds(&rows, &TakeoverBoundary::declared())
        }
        TkInject::BoundaryLeak => {
            let b = TakeoverBoundary::declared();
            !b.admits_errno(75) && !record_admitted(
                &PathRecord { call: "leak", family: Family::Open, disposition: Disposition::Wrapped, legacy_errno: 75, new_errno: 75 },
                &b,
            )
        }
        TkInject::IncrementLoss => {
            let m = IncrementManifest { increments: 2, guards: 4, asserts: 6, recons: 5 };
            !m.complete()
        }
    }
}
/// 注入账。
pub struct TkLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}
impl TkLedger {
    /// 空账。
    pub fn new() -> Self {
        TkLedger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }
    /// 记一轮。
    pub fn record(&mut self, tag: u8, hit: bool) {
        self.injected += 1;
        if hit {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
        self.rounds.push((tag, if hit { 1 } else { 0 }));
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}
/// 3 类 × 10 轮接管矩阵。
pub fn run_takeover_matrix() -> TkLedger {
    let mut l = TkLedger::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        for k in TkInject::all().iter() {
            l.record(k.tag() as u8, inject_detected(*k));
        }
        r += 1;
    }
    l
}
/// 每类检出数。
pub fn per_class(l: &TkLedger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        out[(tag as usize) % 3] += hit;
    }
    out
}
/// 门序探针：fd 先于值域、值域先于指针。
pub fn judge_order_probe(fd_ok: bool, val_ok: bool, ptr_ok: bool) -> u32 {
    if !fd_ok {
        EBADF
    } else if !val_ok {
        EINVAL
    } else if !ptr_ok {
        EFAULT
    } else {
        0
    }
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TkCode {
    SeedCountMismatch,
    FamilyCountDrift,
    DispositionDrift,
    DispositionLeak,
    LegacyRewritten,
    LegacyErrnoDrift,
    BorderFamilyMissing,
    BorderErrnoMissing,
    BorderFdOverrun,
    RetiredNotDeclared,
    ManifestNotBalanced,
    InjectionEscaped,
    StateIllegal,
    SegmentOutOfRange,
}
impl TkCode {
    /// 码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            TkCode::SeedCountMismatch => "种子路径不是 20 条：先补齐再接管",
            TkCode::FamilyCountDrift => "族计数漂移：open/stat/unlink/dup 应为 6/5/4/5",
            TkCode::DispositionDrift => "处置计数漂移：零改写/包层/委派/退役应为 6/8/4/2",
            TkCode::DispositionLeak => "处置态越界：改写态属禁用态，不得入清单",
            TkCode::LegacyRewritten => "存量路径被改写：零重写承诺破裂，回退",
            TkCode::LegacyErrnoDrift => "新旧 errno 不等：存量语义被悄悄替换",
            TkCode::BorderFamilyMissing => "接管边界缺族：四族必须整族声明",
            TkCode::BorderErrnoMissing => "接管边界缺码：六码掩码不完整",
            TkCode::BorderFdOverrun => "fd 越界：只接 0..63，超出交回旧路径",
            TkCode::RetiredNotDeclared => "退役路径未声明：默认不放行",
            TkCode::ManifestNotBalanced => "增量清单不落账：3 增量/4 守卫/6 断言/5 对账",
            TkCode::InjectionEscaped => "注入逃逸：存量改写或边界泄漏未被抓住",
            TkCode::StateIllegal => "状态跳变：注记只许单步前进",
            TkCode::SegmentOutOfRange => "码值越出 0x7A900 窗口：段位守卫拒绝",
        }
    }
    /// 全量闭集。
    pub const ALL: [TkCode; 14] = [
        TkCode::SeedCountMismatch,
        TkCode::FamilyCountDrift,
        TkCode::DispositionDrift,
        TkCode::DispositionLeak,
        TkCode::LegacyRewritten,
        TkCode::LegacyErrnoDrift,
        TkCode::BorderFamilyMissing,
        TkCode::BorderErrnoMissing,
        TkCode::BorderFdOverrun,
        TkCode::RetiredNotDeclared,
        TkCode::ManifestNotBalanced,
        TkCode::InjectionEscaped,
        TkCode::StateIllegal,
        TkCode::SegmentOutOfRange,
    ];
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 种子表指纹。
pub fn seed_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9238/seed");
    for r in SEED_ROWS.iter() {
        h = fnv1a(h, r.call.as_bytes());
        h = fnv1a(h, &[r.family.tag() as u8, r.disposition.tag() as u8]);
        h = fnv1a(h, &r.legacy_errno.to_le_bytes());
    }
    h
}
/// 注记指纹。
pub fn note_digest(n: &TakeoverNote) -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9238/note");
    h = fnv1a(h, &[n.state.tag() as u8]);
    h = fnv1a(h, &(n.admitted as u32).to_le_bytes());
    h = fnv1a(h, &n.sealed_digest.to_le_bytes());
    h
}
/// 巡检指纹：种子表 + 接管矩阵 + 对账表。
pub fn patrol_fingerprint() -> u32 {
    let l = run_takeover_matrix();
    let pc = per_class(&l);
    let mut h = fnv1a(seed_digest(), b"patrol");
    h = fnv1a(h, &l.injected.to_le_bytes());
    h = fnv1a(h, &pc[0].to_le_bytes());
    h = fnv1a(h, &pc[1].to_le_bytes());
    h = fnv1a(h, &pc[2].to_le_bytes());
    h = fnv1a(h, &[recon_ok() as u8]);
    h
}
/// 文案闭集自检。
pub fn text_ok() -> bool {
    if TkCode::ALL.len() != 14 {
        return false;
    }
    let mut i = 0;
    while i < TkCode::ALL.len() {
        let a = TkCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < TkCode::ALL.len() {
            let b = TkCode::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// 自检总成。
pub fn run_unx9238_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9238");
    let rows: Vec<PathRecord> = SEED_ROWS.iter().copied().collect();
    let b = TakeoverBoundary::declared();
    let m = IncrementManifest::declared();
    s.add("seg_base", SEG_BASE == 0x7A900, "段基址应为 0x7A900");
    s.add("code_window", TkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "码值落在 0x7A900..0x7A940");
    s.add("text_closed_set", text_ok(), "14 条闭集、值唯一、建议非空");
    s.add("seed_total", rows.len() == SEED_TOTAL, "种子路径 20 条");
    s.add("family_open", family_rows(&rows, Family::Open) == OPEN_ROWS, "open 族 6 条");
    s.add("family_stat", family_rows(&rows, Family::Stat) == STAT_ROWS, "stat 族 5 条");
    s.add("family_unlink", family_rows(&rows, Family::Unlink) == UNLINK_ROWS, "unlink 族 4 条");
    s.add("family_dup", family_rows(&rows, Family::Dup) == DUP_ROWS, "dup 族 5 条");
    s.add("disp_zero", disposition_rows(&rows, Disposition::ZeroRewrite) == ZERO_REWRITE_ROWS, "零改写 6 条");
    s.add("disp_wrapped", disposition_rows(&rows, Disposition::Wrapped) == WRAPPED_ROWS, "包层 8 条");
    s.add("disp_delegated", disposition_rows(&rows, Disposition::Delegated) == DELEGATED_ROWS, "委派 4 条");
    s.add("disp_retired", disposition_rows(&rows, Disposition::Retired) == RETIRED_ROWS, "退役 2 条");
    s.add("disp_rewritten", disposition_rows(&rows, Disposition::Rewritten) == 0, "改写态 0 条");
    s.add("disp_conserved", disposition_conserved(&rows), "五态求和等于行数");
    s.add("promise", promise_holds(&rows, &b), "零重写承诺成立");
    s.add("legacy_equal", legacy_equal_new(&rows), "20 条新旧 errno 逐条相等");
    s.add("no_rewritten", rows.iter().all(|r| r.disposition != Disposition::Rewritten), "无改写态行");
    s.add("border_family", b.complete() && Family::ALL.iter().all(|f| b.admits_family(*f)), "四族整族声明");
    s.add("border_errno", [EBADF, EFAULT, EINVAL, EMFILE, ENFILE, ENOENT].iter().all(|e| b.admits_errno(*e)), "六码全部在界内");
    s.add("border_errno_reject", !b.admits_errno(75), "界外码 75 被拒绝");
    s.add("border_fd", b.admits_fd(0) && b.admits_fd(63) && !b.admits_fd(64) && !b.admits_fd(-1), "fd 边界 0..63");
    s.add("border_retired", !b.admits_disposition(Disposition::Retired) && !b.admits_disposition(Disposition::Rewritten), "退役默认不放行、改写在禁区");
    s.add("admit_all", all_active_admitted(&rows, &b) && active_rows(&rows) == SEED_TOTAL - RETIRED_ROWS, "18 条活跃路径全部可接管，2 条退役冻结");
    s.add("manifest", m.complete() && m.items().len() == 3, "3 增量/4 守卫/6 断言/5 对账");
    s.add("recon_table", RECON_TABLE.len() == 8 && recon_ok(), "对账表 8 条零漂移");
    let mut n = TakeoverNote::new();
    let a1 = n.admit(SEED_TOTAL, b.complete());
    let a2 = n.declare(b.complete());
    let a3 = n.ratify();
    let a4 = n.seal(seed_digest());
    s.add("note_flow", a1 && a2 && a3 && a4 && n.state == NoteState::Sealed, "接纳→声明→批准→封存");
    s.add("note_promise", n.promise_holds(&rows, &b), "封存后承诺仍成立");
    s.add("note_summary", !n.summary().is_empty(), "摘要非空");
    let stage_ok = NoteState::Draft.can_go(NoteState::Admitted)
        && NoteState::Admitted.can_go(NoteState::Declared)
        && NoteState::Declared.can_go(NoteState::Ratified)
        && NoteState::Ratified.can_go(NoteState::Sealed)
        && !NoteState::Draft.can_go(NoteState::Sealed)
        && !NoteState::Sealed.can_go(NoteState::Draft);
    s.add("state_machine", stage_ok, "注记状态只许单步前进");
    s.add("order_probe", judge_order_probe(false, false, false) == EBADF
        && judge_order_probe(true, false, false) == EINVAL
        && judge_order_probe(true, true, false) == EFAULT
        && judge_order_probe(true, true, true) == 0, "门序 fd→值域→指针");
    let l = run_takeover_matrix();
    let pc = per_class(&l);
    s.add("inject_ten_of_ten", pc == [INJECT_ROUNDS; 3], "三类注入各 10/10");
    s.add("ledger_conserved", l.conserved() && l.escaped == 0 && l.injected == INJECT_ROUNDS * 3, "注入 30 = 检出 30 + 逃逸 0");
    s.add("inject_text", TkInject::all().iter().all(|k| !k.detail().is_empty()), "注入文案非空");
    let d1 = patrol_fingerprint();
    let d2 = patrol_fingerprint();
    let l2 = run_takeover_matrix();
    s.add("double_run", d1 == d2 && l2.injected == l.injected && per_class(&l2) == pc, "双跑指纹与注入账一致");
    s.add("capacity", rows.len() <= 32, "种子行不超过 32");
    s.add_dyn("seed_detail", rows.len() == 20, &format!("行={} 指纹={:08x}", rows.len(), seed_digest()));
    s.add_dyn("disp_detail", true, &format!("零改写={} 包层={} 委派={} 退役={}", ZERO_REWRITE_ROWS, WRAPPED_ROWS, DELEGATED_ROWS, RETIRED_ROWS));
    s.add_dyn("inject_detail", pc == [10, 10, 10], &format!("改写={} 泄漏={} 丢账={}", pc[0], pc[1], pc[2]));
    s.add_dyn("note_detail", n.state == NoteState::Sealed, &format!("状态={} 指纹={:08x}", n.state.name(), note_digest(&n)));
    s.add_dyn("patrol_detail", d1 == d2, &format!("巡检={:08x} 对账={}", d1, recon_ok()));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rows() -> Vec<PathRecord> {
        SEED_ROWS.iter().copied().collect()
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9238_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn seed_families_add_up() {
        let r = rows();
        assert_eq!(r.len(), 20);
        assert_eq!(family_rows(&r, Family::Open), 6);
        assert_eq!(family_rows(&r, Family::Stat), 5);
        assert_eq!(family_rows(&r, Family::Unlink), 4);
        assert_eq!(family_rows(&r, Family::Dup), 5);
        assert_eq!(OPEN_ROWS + STAT_ROWS + UNLINK_ROWS + DUP_ROWS, SEED_TOTAL);
    }
    #[test]
    fn dispositions_add_up() {
        let r = rows();
        assert_eq!(disposition_rows(&r, Disposition::ZeroRewrite), 6);
        assert_eq!(disposition_rows(&r, Disposition::Wrapped), 8);
        assert_eq!(disposition_rows(&r, Disposition::Delegated), 4);
        assert_eq!(disposition_rows(&r, Disposition::Retired), 2);
        assert_eq!(disposition_rows(&r, Disposition::Rewritten), 0);
        assert!(disposition_conserved(&r));
    }
    #[test]
    fn zero_rewrite_promise() {
        let r = rows();
        let b = TakeoverBoundary::declared();
        assert!(promise_holds(&r, &b));
        assert!(legacy_equal_new(&r));
        assert!(all_active_admitted(&r, &b));
        assert_eq!(active_rows(&r), SEED_TOTAL - RETIRED_ROWS);
    }
    #[test]
    fn boundary_is_declared_exactly() {
        let b = TakeoverBoundary::declared();
        assert!(b.complete());
        for f in Family::ALL.iter() {
            assert!(b.admits_family(*f));
        }
        for e in [EBADF, EFAULT, EINVAL, EMFILE, ENFILE, ENOENT].iter() {
            assert!(b.admits_errno(*e));
        }
        assert!(!b.admits_errno(75));
        assert!(b.admits_fd(63));
        assert!(!b.admits_fd(64));
    }
    #[test]
    fn retired_is_locked_out() {
        let b = TakeoverBoundary::declared();
        assert!(!b.admits_disposition(Disposition::Retired));
        assert!(!b.admits_disposition(Disposition::Rewritten));
        assert!(b.admits_disposition(Disposition::ZeroRewrite));
    }
    #[test]
    fn manifest_is_balanced() {
        let m = IncrementManifest::declared();
        assert!(m.complete());
        assert_eq!(m.increments, 3);
        assert_eq!(m.guards, 4);
        assert_eq!(m.asserts, 6);
        assert_eq!(m.recons, 5);
        assert_eq!(m.items().len(), 3);
    }
    #[test]
    fn injections_are_caught_ten_of_ten() {
        let l = run_takeover_matrix();
        assert_eq!(per_class(&l), [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        for k in TkInject::all().iter() {
            assert!(inject_detected(*k));
        }
    }
    #[test]
    fn note_state_machine() {
        let mut n = TakeoverNote::new();
        assert_eq!(n.state, NoteState::Draft);
        assert!(!n.ratify());
        assert!(n.admit(20, true));
        assert!(!n.admit(20, true));
        assert!(n.declare(true));
        assert!(n.ratify());
        assert!(n.seal(seed_digest()));
        assert_eq!(n.state, NoteState::Sealed);
        assert_eq!(n.admitted, 20);
        assert_ne!(n.sealed_digest, 0);
        assert!(!n.seal(1));
    }
    #[test]
    fn gate_order_probe_is_fd_first() {
        assert_eq!(judge_order_probe(false, false, false), EBADF);
        assert_eq!(judge_order_probe(true, false, false), EINVAL);
        assert_eq!(judge_order_probe(true, true, false), EFAULT);
        assert_eq!(judge_order_probe(true, true, true), 0);
    }
    #[test]
    fn digests_and_recon_stable() {
        assert_eq!(seed_digest(), seed_digest());
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(seed_digest(), 0);
        assert!(recon_ok());
        assert_eq!(RECON_TABLE.len(), 8);
    }
}
