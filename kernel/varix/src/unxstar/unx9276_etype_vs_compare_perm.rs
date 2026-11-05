//! UNX-F9276 权限族 E 型与 Windows Access Denied 对照格 · 0x7B280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9276-J1 逐字）：
//!   ERROR_ACCESS_DENIED 映射判据过，差异账三条全列格过。
//!
//! 设计要点：
//!   ① 两段映射：粗档把 Linux EPERM(1)/EACCES(13) 一并折到 Windows
//!      ERROR_ACCESS_DENIED(5)，服务兼容层「是不是拒绝」的快速判定；细档
//!      把 EPERM 另挂 ERROR_PRIVILEGE_NOT_HELD(1314)，服务诊断文案与
//!      权限族分域。两档并存且都从同一张 MAP_TABLE 派生，禁止旁路硬编码。
//!   ② 差异账固定三条：粒度折叠（2→1 多对一）、判据来源（ACL/令牌 vs
//!      mode 三位 + 能力位）、共享/锁定语义（ERROR_SHARING_VIOLATION=32
//!      在 errno 侧无对等码）。缺一条即不满足「三条全列」。
//!   ③ 反向映射必须显式歧义：5 → {EPERM, EACCES} 两个候选，主项取 Exact
//!      级行（EACCES），候选数恒上报；把候选数抹成 1 属静默歧义，必抓。
//!   ④ 等级闭集 Exact/Approximate/NoCounterpart；未定级的行不得进表，
//!      NoCounterpart 行必须 coarse/refined 双空，禁止强行映射。
//!   ⑤ 判定次序：先查号段合法性（0/负值 → ZeroOrNegative），再查是否在
//!      表（表外 → UnknownErrno），最后才谈映射档位与等级。
//!   ⑥ 注入四类（未知号/行漂移/差异账缺条/静默歧义）各 10/10 必抓，
//!      注入账守恒 40 = 40 + 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9276 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B280;

/// Linux errno 正数码（权限族）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EBUSY: i32 = 16;
pub const EINVAL: i32 = 22;

/// Windows Win32 错误码（对照面只用这几个常量）。
pub const WIN_ACCESS_DENIED: u32 = 5;
pub const WIN_INVALID_HANDLE: u32 = 6;
pub const WIN_FILE_NOT_FOUND: u32 = 2;
pub const WIN_SHARING_VIOLATION: u32 = 32;
pub const WIN_INVALID_PARAMETER: u32 = 87;
pub const WIN_NOACCESS: u32 = 998;
pub const WIN_PRIVILEGE_NOT_HELD: u32 = 1314;

/// 对照表行数（7 行：六个可映射 + 一个 NoCounterpart）。
pub const MAP_ROWS: usize = 7;

/// 差异账固定条数（卡面「三条全列」）。
pub const DIFF_COUNT: usize = 3;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 反向映射冻结格数。
pub const REVERSE_ROWS: usize = 5;

// ── 等级闭集 ──────────────────────────────────────────────────────────
/// 对照等级：语义与码值对等程度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// 语义与码值双对等
    Exact = 0,
    /// 语义近似，需细档补正
    Approximate = 1,
    /// 对端无对等码
    NoCounterpart = 2,
}

impl Grade {
    /// 中文标签（闭集返回，禁止空串）。
    pub fn label(self) -> &'static str {
        match self { Grade::Exact => "Exact：语义与码值双对等，可无损互推", Grade::Approximate => "Approximate：语义近似，必须借细档补正", Grade::NoCounterpart => "NoCounterpart：对端无对等码，禁止映射", }
    }
}

// ── Windows 错误名闭集 ────────────────────────────────────────────────
/// 对照面用到的 Win32 错误名（闭集，码值为系统常量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinErr {
    /// ERROR_ACCESS_DENIED = 5
    AccessDenied,
    /// ERROR_INVALID_HANDLE = 6
    InvalidHandle,
    /// ERROR_FILE_NOT_FOUND = 2
    FileNotFound,
    /// ERROR_SHARING_VIOLATION = 32
    SharingViolation,
    /// ERROR_INVALID_PARAMETER = 87
    InvalidParameter,
    /// ERROR_NOACCESS = 998
    NoAccess,
    /// ERROR_PRIVILEGE_NOT_HELD = 1314
    PrivilegeNotHeld,
}

impl WinErr {
    /// 系统常量码。
    pub fn code(self) -> u32 {
        match self { WinErr::AccessDenied => WIN_ACCESS_DENIED, WinErr::InvalidHandle => WIN_INVALID_HANDLE, WinErr::FileNotFound => WIN_FILE_NOT_FOUND, WinErr::SharingViolation => WIN_SHARING_VIOLATION, WinErr::InvalidParameter => WIN_INVALID_PARAMETER, WinErr::NoAccess => WIN_NOACCESS, WinErr::PrivilegeNotHeld => WIN_PRIVILEGE_NOT_HELD, }
    }

    /// 符号名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self { WinErr::AccessDenied => "ERROR_ACCESS_DENIED", WinErr::InvalidHandle => "ERROR_INVALID_HANDLE", WinErr::FileNotFound => "ERROR_FILE_NOT_FOUND", WinErr::SharingViolation => "ERROR_SHARING_VIOLATION", WinErr::InvalidParameter => "ERROR_INVALID_PARAMETER", WinErr::NoAccess => "ERROR_NOACCESS", WinErr::PrivilegeNotHeld => "ERROR_PRIVILEGE_NOT_HELD", }
    }

    /// 闭集全量。
    pub const ALL: [WinErr; 7] = [WinErr::AccessDenied, WinErr::InvalidHandle, WinErr::FileNotFound, WinErr::SharingViolation, WinErr::InvalidParameter, WinErr::NoAccess, WinErr::PrivilegeNotHeld,];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareCode {
    /// 号段非法：0 或负值
    ZeroOrNegative,
    /// 表外 errno
    UnknownErrno,
    /// 该行无对等码
    NoCounterpart,
    /// 对照行与冻结账漂移
    RowDrift,
    /// 差异账缺条
    DiffMissing,
    /// 差异账字段为空
    DiffEmpty,
    /// 反向静默歧义（候选数被抹平）
    SilentAmbiguity,
    /// 等级未定或越集
    GradeUnset,
    /// 状态机回退/跳步
    StageSkip,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CompareCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self { CompareCode::ZeroOrNegative => "号段非法：errno/Win32 码必须为正；0 表示成功，\
                不得进入错误映射；请先按返回值符号分流",
            CompareCode::UnknownErrno => "表外 errno：目标号不在权限族对照表内；请先在 \
                MAP_TABLE 登记并定级，再发起映射",
            CompareCode::NoCounterpart => "该行无对等码：NoCounterpart 行禁止映射；\
                请改走差异账说明，不得强行取近似码",
            CompareCode::RowDrift => "对照行漂移：MAP_TABLE 与冻结账不一致；改映射表\
                必须先改冻结账，否则对照格失去可复算性",
            CompareCode::DiffMissing => "差异账缺条：三条差异必须全列（粒度/来源/共享）；\
                缺条即不能宣布映射收口",
            CompareCode::DiffEmpty => "差异账字段为空：标题/说明/引用号缺一不可；\
                空文案等于没有差异声明",
            CompareCode::SilentAmbiguity => "反向映射静默歧义：一个 Win32 码对应多个 \
                errno 时必须上报候选数，禁止默认取首项",
            CompareCode::GradeUnset => "等级未定或越集：只允许 Exact/Approximate/\
                NoCounterpart 三档，未定级行不得进表",
            CompareCode::StageSkip => "状态机回退或跳步：Map→Grid→Diff→Seal 只许单向\
                推进，回退会让对照读到半成品表",
            CompareCode::LedgerRegress => "注入账倒退：注入=检出+逃逸且只增不减；\
                请检查回滚路径误改累计值",
            CompareCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            CompareCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [CompareCode; 12] = [CompareCode::ZeroOrNegative, CompareCode::UnknownErrno, CompareCode::NoCounterpart, CompareCode::RowDrift, CompareCode::DiffMissing, CompareCode::DiffEmpty, CompareCode::SilentAmbiguity, CompareCode::GradeUnset, CompareCode::StageSkip, CompareCode::LedgerRegress, CompareCode::NotReproducible, CompareCode::SegmentViolation,];
}

// ── 对照表 ────────────────────────────────────────────────────────────
/// 单条对照行：errno → 粗档/细档 Win32 码 + 等级 + 说明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapRow {
    /// Linux errno 正码
    pub errno: i32,
    /// 粗档（兼容层快速判定）
    pub coarse: Option<WinErr>,
    /// 细档（权限族分域诊断）
    pub refined: Option<WinErr>,
    /// 对照等级
    pub grade: Grade,
    /// 语义说明
    pub note: &'static str,
}

/// 权限族对照表（7 行，冻结账见 FROZEN_ROWS）。
pub const MAP_TABLE: [MapRow; MAP_ROWS] = [MapRow { errno: EPERM, coarse: Some(WinErr::AccessDenied), refined: Some(WinErr::PrivilegeNotHeld), grade: Grade::Approximate, note: "操作特权缺失：粗档只能折到 5，细档才用 1314 分域", },
    MapRow { errno: EACCES, coarse: Some(WinErr::AccessDenied), refined: Some(WinErr::AccessDenied), grade: Grade::Exact, note: "对象权限不足：两端同名同码，可直接互推", },
    MapRow { errno: ENOENT, coarse: Some(WinErr::FileNotFound), refined: Some(WinErr::FileNotFound), grade: Grade::Exact, note: "路径不存在：两端码值同为 2", },
    MapRow { errno: EBADF, coarse: Some(WinErr::InvalidHandle), refined: Some(WinErr::InvalidHandle), grade: Grade::Exact, note: "坏句柄：fd 面与 HANDLE 面同构", },
    MapRow { errno: EFAULT, coarse: Some(WinErr::NoAccess), refined: Some(WinErr::NoAccess), grade: Grade::Approximate, note: "坏地址：Windows 不分用户/内核地址，998 覆盖面更宽", },
    MapRow { errno: EINVAL, coarse: Some(WinErr::InvalidParameter), refined: Some(WinErr::InvalidParameter), grade: Grade::Exact, note: "参数坏值：两端均为参数面错误", },
    MapRow { errno: EBUSY, coarse: None, refined: None, grade: Grade::NoCounterpart, note: "忙态：Windows ERROR_BUSY=170 属设备忙，语义不等价，禁止映射", },
];

/// 冻结账 (errno, 粗档码, 细档码, 等级序号)；0 表示该档为空。
pub const FROZEN_ROWS: [(i32, u32, u32, u8); MAP_ROWS] = [(EPERM, WIN_ACCESS_DENIED, WIN_PRIVILEGE_NOT_HELD, 1), (EACCES, WIN_ACCESS_DENIED, WIN_ACCESS_DENIED, 0), (ENOENT, WIN_FILE_NOT_FOUND, WIN_FILE_NOT_FOUND, 0), (EBADF, WIN_INVALID_HANDLE, WIN_INVALID_HANDLE, 0), (EFAULT, WIN_NOACCESS, WIN_NOACCESS, 1), (EINVAL, WIN_INVALID_PARAMETER, WIN_INVALID_PARAMETER, 0), (EBUSY, 0, 0, 2),];

/// 映射档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 粗档：兼容层「是否拒绝」
    Coarse,
    /// 细档：权限族分域诊断
    Refined,
}

/// 取某 errno 的对照行（表外返回 None）。
pub fn row_of(errno: i32) -> Option<MapRow> {
    let mut i = 0usize;
while i < MAP_TABLE.len() { if let Some(row) = MAP_TABLE.get(i) { if row.errno == errno { return Some(*row); } } i += 1; }
    None
}

/// 主映射：号段 → 表内 → 档位，三步短路，次序即判据。
pub fn map_errno(errno: i32, tier: Tier) -> Result<u32, CompareCode> {
if errno <= 0 { return Err(CompareCode::ZeroOrNegative); }
    let row = match row_of(errno) { Some(r) => r, None => return Err(CompareCode::UnknownErrno), };
    let pick = match tier { Tier::Coarse => row.coarse, Tier::Refined => row.refined, };
    match pick { Some(w) => Ok(w.code()), None => Err(CompareCode::NoCounterpart), }
}

/// 冻结账漂移行数（0 才算对照收口）。
pub fn drift_of(rows: &[MapRow]) -> u32 {
if rows.len() != FROZEN_ROWS.len() { return 1; }
    let mut drifted = 0u32;
    let mut i = 0usize;
while i < rows.len() { if let (Some(r), Some(f)) = (rows.get(i), FROZEN_ROWS.get(i)) { let coarse = match r.coarse { Some(w) => w.code(), None => 0, }; let refined = match r.refined { Some(w) => w.code(), None => 0, }; if r.errno != f.0 || coarse != f.1 || refined != f.2 || r.grade as u8 != f.3 { drifted = drifted.saturating_add(1); } } i += 1; }
    drifted
}

/// 当前表漂移行数。
pub fn row_drift() -> u32 {
    drift_of(&MAP_TABLE)
}

// ── 反向映射 ──────────────────────────────────────────────────────────
/// 反向命中：主项 + 候选数（候选数 >1 即歧义，必须显式上报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReverseHit {
    /// 主项 errno（优先 Exact 级行）
    pub primary: i32,
    /// 候选个数
    pub candidates: u8,
}

/// 反向：Win32 码 → errno。号段先判，表外拒绝。
pub fn reverse(win: u32) -> Result<ReverseHit, CompareCode> {
if win == 0 { return Err(CompareCode::ZeroOrNegative); }
    let mut candidates: u8 = 0;
    let mut primary: i32 = 0;
    let mut exact: i32 = 0;
    let mut i = 0usize;
while i < MAP_TABLE.len() { if let Some(row) = MAP_TABLE.get(i) { if let Some(w) = row.coarse { if w.code() == win { candidates = candidates.saturating_add(1); if primary == 0 { primary = row.errno; } if row.grade == Grade::Exact { exact = row.errno; } } } } i += 1; }
if candidates == 0 { return Err(CompareCode::UnknownErrno); }
if exact != 0 { primary = exact; }
    Ok(ReverseHit { primary, candidates })
}

/// 反向冻结账 (Win32 码, 主项, 候选数)。
pub const REVERSE_FROZEN: [(u32, i32, u8); REVERSE_ROWS] = [(WIN_ACCESS_DENIED, EACCES, 2), (WIN_INVALID_HANDLE, EBADF, 1), (WIN_FILE_NOT_FOUND, ENOENT, 1), (WIN_INVALID_PARAMETER, EINVAL, 1), (WIN_NOACCESS, EFAULT, 1),];

/// 反向冻结账漂移格数。
pub fn reverse_drift_of(frozen: &[(u32, i32, u8)]) -> u32 {
    let mut drifted = 0u32;
    let mut i = 0usize;
while i < frozen.len() { if let Some(f) = frozen.get(i).copied() { match reverse(f.0) { Ok(hit) => { if hit.primary != f.1 || hit.candidates != f.2 { drifted = drifted.saturating_add(1); } } Err(_) => drifted = drifted.saturating_add(1), } } i += 1; }
    drifted
}

/// 当前反向漂移格数。
pub fn reverse_drift() -> u32 {
    reverse_drift_of(&REVERSE_FROZEN)
}

/// 歧义是否显式：候选数必须与冻结账一致且 >1 时主项仍可确定。
pub fn ambiguity_explicit(win: u32) -> bool {
    match reverse(win) { Ok(hit) => hit.candidates > 1 && hit.primary != 0 && hit.candidates as usize <= MAP_ROWS, Err(_) => false, }
}

// ── 差异账三条 ────────────────────────────────────────────────────────
/// 单条差异：编号 + 标题 + 说明 + 引用对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    /// 编号（1..=3，唯一）
    pub id: u8,
    /// 差异标题
    pub title: &'static str,
    /// 差异说明（中文，非空）
    pub detail: &'static str,
    /// 引用的 Linux errno
    pub errno_ref: i32,
    /// 引用的 Win32 码
    pub win_ref: u32,
}

/// 差异账（固定三条，顺序即编号）。
pub const DIFFERENCES: [DiffEntry; DIFF_COUNT] = [DiffEntry { id: 1, title: "粒度折叠", detail: "Linux 用 EPERM(1)/EACCES(13) 两码区分「操作特权缺失」与「对象权限不足」，\
            Windows 粗档统一 ERROR_ACCESS_DENIED(5)，只有细档 1314 才能分域；多对一意味着\
            从 5 反推 Linux 判定域必然带歧义，禁止当作无损映射。",
        errno_ref: EACCES,
        win_ref: WIN_ACCESS_DENIED,
    },
    DiffEntry { id: 2, title: "判据来源", detail: "Windows 由 DACL 与访问令牌裁决，Linux 由 mode 三位叠加能力位\
            （CAP_DAC_OVERRIDE/CAP_DAC_READ_SEARCH）裁决；同一 EACCES 在两端可能由完全\
            不同的机制触发，故 13↔5 只能按 Approximate 对拍，不能做机制级等价声明。",
        errno_ref: EPERM,
        win_ref: WIN_PRIVILEGE_NOT_HELD,
    },
    DiffEntry { id: 3, title: "共享/锁定语义", detail: "Windows ERROR_SHARING_VIOLATION(32) 源于共享模式冲突，Linux errno 侧没有\
            对等码（就近的 EBUSY(16) 表示设备或挂载忙，语义不同）；本格必须列\
            NoCounterpart，禁止强行映射到 EBUSY 或 EAGAIN。",
        errno_ref: EBUSY,
        win_ref: WIN_SHARING_VIOLATION,
    },
];

/// 差异账完整性：三条、编号唯一、字段非空、引用号在表、Win32 码非零。
pub fn diff_complete(entries: &[DiffEntry]) -> bool {
if entries.len() != DIFF_COUNT { return false; }
    let mut i = 0usize;
while i < entries.len() { if let Some(e) = entries.get(i) { if e.id == 0 || e.title.is_empty() || e.detail.is_empty() { return false; } if e.win_ref == 0 || row_of(e.errno_ref).is_none() { return false; } let mut j = i + 1; while j < entries.len() { if let Some(o) = entries.get(j) { if o.id == e.id || o.title == e.title { return false; } } j += 1; } } i += 1; }
    true
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 对照推进阶段：Map→Grid→Diff→Seal 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareStage {
    /// 映射表就位
    Map,
    /// 对照格定级
    Grid,
    /// 差异账三条
    Diff,
    /// 封账
    Seal,
}

impl CompareStage {
    /// 下一阶段。
    pub fn next(self) -> Option<CompareStage> {
        match self { CompareStage::Map => Some(CompareStage::Grid), CompareStage::Grid => Some(CompareStage::Diff), CompareStage::Diff => Some(CompareStage::Seal), CompareStage::Seal => None, }
    }

    /// 单向推进；终点再推进即 StageSkip。
    pub fn advance(self) -> Result<CompareStage, CompareCode> {
        match self.next() { Some(n) => Ok(n), None => Err(CompareCode::StageSkip), }
    }
}

// ── 注入账与坏样本 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl CompareLedger {
    /// 空账。
    pub const fn zero() -> CompareLedger {
        CompareLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), CompareCode> {
        self.injected = self.injected.saturating_add(1);
if caught { self.detected = self.detected.saturating_add(1); } else { self.escaped = self.escaped.saturating_add(1); }
if self.detected.saturating_add(self.escaped) != self.injected { return Err(CompareCode::LedgerRegress); }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCompareCase {
    /// 表外 errno
    UnknownErrno,
    /// 对照行漂移（篡改粗档码）
    RowDrift,
    /// 差异账缺条（只列两条）
    DiffShort,
    /// 反向静默歧义（候选数抹成 1）
    SilentReverse,
}

impl BadCompareCase {
    /// 探针：返回是否被对照核抓到。
    pub fn probe(self) -> bool {
        match self { BadCompareCase::UnknownErrno => { map_errno(40_000, Tier::Coarse) == Err(CompareCode::UnknownErrno) } BadCompareCase::RowDrift => { let mut rows = MAP_TABLE; if let Some(row) = rows.get_mut(2) { row.coarse = Some(WinErr::InvalidHandle); } drift_of(&rows) > 0 } BadCompareCase::DiffShort => !diff_complete(&DIFFERENCES[..DIFF_COUNT - 1]), BadCompareCase::SilentReverse => { let mut frozen = REVERSE_FROZEN; if let Some(first) = frozen.get_mut(0) { first.2 = 1; } reverse_drift_of(&frozen) > 0 } }
    }

    /// 期望诊断码（供文案与账目定位）。
    pub fn code(self) -> CompareCode {
        match self { BadCompareCase::UnknownErrno => CompareCode::UnknownErrno, BadCompareCase::RowDrift => CompareCode::RowDrift, BadCompareCase::DiffShort => CompareCode::DiffMissing, BadCompareCase::SilentReverse => CompareCode::SilentAmbiguity, }
    }

    /// 闭集全量。
    pub const ALL: [BadCompareCase; 4] = [BadCompareCase::UnknownErrno, BadCompareCase::RowDrift, BadCompareCase::DiffShort, BadCompareCase::SilentReverse,];
}

/// 四类注入各 INJECT_ROUNDS 轮，返回账本与逐类检出数。
pub fn run_compare_matrix() -> (CompareLedger, [u32; 4]) {
    let mut ledger = CompareLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
while ci < BadCompareCase::ALL.len() { if let Some(case) = BadCompareCase::ALL.get(ci).copied() { let mut round = 0u32; while round < INJECT_ROUNDS { let caught = case.probe(); let _ = ledger.record(caught); if caught { if let Some(slot) = per_class.get_mut(ci) { *slot = slot.saturating_add(1); } } round += 1; } } ci += 1; }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 规范语料指纹：对照表 + 差异账 + 反向冻结账。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
while i < MAP_TABLE.len() { if let Some(r) = MAP_TABLE.get(i) { h = mix(h, r.errno as u32); h = mix(h, match r.coarse { Some(w) => w.code(), None => 0 }); h = mix(h, match r.refined { Some(w) => w.code(), None => 0 }); h = mix(h, r.grade as u32); } i += 1; }
    let mut k = 0usize;
while k < DIFFERENCES.len() { if let Some(d) = DIFFERENCES.get(k) { h = mix(h, d.id as u32); h = mix(h, d.errno_ref as u32); h = mix(h, d.win_ref); } k += 1; }
    let mut m = 0usize;
while m < REVERSE_FROZEN.len() { if let Some(f) = REVERSE_FROZEN.get(m) { h = mix(h, f.0); h = mix(h, f.1 as u32); h = mix(h, f.2 as u32); } m += 1; }
    h
}

/// 等级计数（某等级在表中出现次数）。
pub fn grade_count(g: Grade) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
while i < MAP_TABLE.len() { if let Some(r) = MAP_TABLE.get(i) { if r.grade == g { n = n.saturating_add(1); } } i += 1; }
    n
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9276 判据集（静态 18 条 + 动态 3 条）。
pub fn run_unx9276_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9276");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B280 0x40 对齐且在批窗口内", seg_ok, "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：EACCES 两档同为 5
    let acc_ok = map_errno(EACCES, Tier::Coarse) == Ok(WIN_ACCESS_DENIED)
        && map_errno(EACCES, Tier::Refined) == Ok(WIN_ACCESS_DENIED);
    s.add("正常流：EACCES 粗档/细档同为 ERROR_ACCESS_DENIED(5)", acc_ok, "对象权限不足在两端同名同码，属 Exact 级");

    // 3. 正常流：EPERM 粗档 5 / 细档 1314
    let eperm_ok = map_errno(EPERM, Tier::Coarse) == Ok(WIN_ACCESS_DENIED)
        && map_errno(EPERM, Tier::Refined) == Ok(WIN_PRIVILEGE_NOT_HELD);
    s.add("正常流：EPERM 粗档折 5、细档分到 1314（两档并存）", eperm_ok, "粗档服务兼容判定，细档服务权限族分域");

    // 4. 边界：其余可映射行逐码
    let others_ok = map_errno(ENOENT, Tier::Coarse) == Ok(WIN_FILE_NOT_FOUND)
        && map_errno(EBADF, Tier::Coarse) == Ok(WIN_INVALID_HANDLE)
        && map_errno(EFAULT, Tier::Refined) == Ok(WIN_NOACCESS)
        && map_errno(EINVAL, Tier::Coarse) == Ok(WIN_INVALID_PARAMETER);
    s.add("边界：ENOENT/EBADF/EFAULT/EINVAL 逐码对位", others_ok, "四个可映射行必须与冻结账逐码一致");

    // 5. 边界：NoCounterpart 行双空
    let busy_row = row_of(EBUSY);
    let busy_ok = match busy_row { Some(r) => r.grade == Grade::NoCounterpart && r.coarse.is_none() && r.refined.is_none(), None => false, } && map_errno(EBUSY, Tier::Coarse) == Err(CompareCode::NoCounterpart);
    s.add("边界：EBUSY 列 NoCounterpart 且两档双空、映射显性拒绝", busy_ok, "对端无对等码时禁止强行映射到 EBUSY/EAGAIN");

    // 6. 次序：号段先于表外
    let order_ok = map_errno(0, Tier::Coarse) == Err(CompareCode::ZeroOrNegative)
        && map_errno(-EACCES, Tier::Coarse) == Err(CompareCode::ZeroOrNegative)
        && map_errno(40_000, Tier::Coarse) == Err(CompareCode::UnknownErrno)
        && reverse(0) == Err(CompareCode::ZeroOrNegative);
    s.add("判序：0/负值先报号段非法，表外才报 UnknownErrno", order_ok, "号段合法性先于查表，次序颠倒会把 0 误报成表外号");

    // 7. 冻结：对照表零漂移
    s.add("冻结账：7 行对照零漂移", row_drift() == 0 && MAP_TABLE.len() == MAP_ROWS, "改映射表必须先改冻结账，否则对照格不可复算");

    // 8. 差异账三条全列
    s.add("差异账：三条全列（粒度/来源/共享）且编号唯一", diff_complete(&DIFFERENCES), "标题+说明+引用号缺一不可，缺条即不能收口");

    // 9. 差异账粒度可复算：5 对应两个 errno
    s.add("差异账：ERROR_ACCESS_DENIED 候选数=2 与粒度折叠条一致", match reverse(WIN_ACCESS_DENIED) { Ok(hit) => hit.candidates == 2, Err(_) => false, } && DIFFERENCES.len() == DIFF_COUNT, "多对一是差异账第一条的实证，不是文字声明");

    // 10. 反向显式歧义
    let amb_ok = reverse(WIN_ACCESS_DENIED) == Ok(ReverseHit { primary: EACCES, candidates: 2 })
        && ambiguity_explicit(WIN_ACCESS_DENIED);
    s.add("反向：5→{EPERM,EACCES}，主项取 Exact 级 EACCES 且候选数上报", amb_ok, "静默取首项会让兼容层把 EACCES 误判成特权缺失");

    // 11. 反向单候选行
    let single_ok = reverse(WIN_INVALID_HANDLE) == Ok(ReverseHit { primary: EBADF, candidates: 1 })
        && reverse(WIN_NOACCESS) == Ok(ReverseHit { primary: EFAULT, candidates: 1 })
        && reverse_drift() == 0;
    s.add("反向：单候选行逐格对位且全账零漂移", single_ok, "单候选行同样要过冻结账，防止主项被改写");

    // 12. 等级三档齐备
    let grades_ok = grade_count(Grade::Exact) >= 1
        && grade_count(Grade::Approximate) >= 1
        && grade_count(Grade::NoCounterpart) == 1;
    s.add("等级：Exact/Approximate/NoCounterpart 三档齐备", grades_ok, "NoCounterpart 恰好一行（EBUSY），其余行必须定级");

    // 13. 注入必抓：四类各 10/10
    let (ledger, per_class) = run_compare_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：未知号/行漂移/差异缺条/静默歧义各 10/10", all_caught, "四类违约样本必须全部被对照核抓到");

    // 14. 注入账守恒
    s.add("守恒：注入 40 = 检出 40 + 逃逸 0", ledger.conserved() && ledger.escaped == 0 && ledger.injected == BadCompareCase::ALL.len() as u32 * INJECT_ROUNDS, "40 次注入必须 40 次检出、0 逃逸");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹两次相等", a == b, "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 文案闭集：12 码连续、唯一、非空
    let mut text_ok = true;
    let mut i16 = 0usize;
while i16 < CompareCode::ALL.len() { if let Some(c) = CompareCode::ALL.get(i16) { if c.val() != SEG_BASE + i16 as u32 || c.detail().is_empty() { text_ok = false; } let mut j16 = i16 + 1; while j16 < CompareCode::ALL.len() { if let Some(o) = CompareCode::ALL.get(j16) { if c.val() == o.val() { text_ok = false; } } j16 += 1; } } i16 += 1; }
    s.add("文案闭集：12 个诊断码连续、唯一、detail 非空", text_ok, "每个码必须能取到非空且可操作的说明");

    // 17. 状态机单向
    let mut stage = CompareStage::Map;
    let mut steps = 0u32;
    let mut forward = true;
while let Ok(next) = stage.advance() { stage = next; steps = steps.saturating_add(1); if steps > 8 { forward = false; break; } }
    s.add("状态机：Map→Grid→Diff→Seal 单向三步", forward && steps == 3 && stage == CompareStage::Seal && stage.advance() == Err(CompareCode::StageSkip), "回退或跳步会让封账读到半成品对照格");

    // 18. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
while k < 4 { let _ = mono.record(true); k = k.saturating_add(1); }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4, "累计计数禁止回退或饱和到旧值");

    // 动态明细（≤8）
    s.add_dyn("行漂移快照", row_drift() == 0, &format!("drift={} rows={}", row_drift(), MAP_ROWS));
    s.add_dyn("反向快照", reverse_drift() == 0 && ambiguity_explicit(WIN_ACCESS_DENIED), &format!("cand5={} revdrift={}", match reverse(WIN_ACCESS_DENIED) { Ok(h) => h.candidates, Err(_) => 0, }, reverse_drift()));
    let ledger_text = format!("inj={} det={} esc={}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    s.add_dyn("指纹快照", a == b, &format!("fp=0x{:08X}", a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_denied_mapping_exact() {
        assert_eq!(map_errno(EACCES, Tier::Coarse), Ok(WIN_ACCESS_DENIED));
        assert_eq!(map_errno(EACCES, Tier::Refined), Ok(WIN_ACCESS_DENIED));
    }

    #[test]
    fn eperm_splits_between_tiers() {
        assert_eq!(map_errno(EPERM, Tier::Coarse), Ok(WIN_ACCESS_DENIED));
        assert_eq!(map_errno(EPERM, Tier::Refined), Ok(WIN_PRIVILEGE_NOT_HELD));
    }

    #[test]
    fn zero_and_negative_rejected_first() {
        assert_eq!(map_errno(0, Tier::Coarse), Err(CompareCode::ZeroOrNegative));
        assert_eq!(map_errno(-1, Tier::Coarse), Err(CompareCode::ZeroOrNegative));
        assert_eq!(map_errno(9999, Tier::Coarse), Err(CompareCode::UnknownErrno));
    }

    #[test]
    fn no_counterpart_row_is_double_empty() {
        let row = row_of(EBUSY).unwrap_or(MapRow { errno: 0, coarse: None, refined: None, grade: Grade::NoCounterpart, note: "", });
        assert!(row.coarse.is_none());
        assert!(row.refined.is_none());
        assert_eq!(row.grade, Grade::NoCounterpart);
        assert_eq!(map_errno(EBUSY, Tier::Refined), Err(CompareCode::NoCounterpart));
    }

    #[test]
    fn table_matches_frozen_ledger() {
        assert_eq!(row_drift(), 0);
        assert_eq!(MAP_TABLE.len(), MAP_ROWS);
    }

    #[test]
    fn diff_ledger_has_three_entries() {
        assert_eq!(DIFFERENCES.len(), 3);
        assert!(diff_complete(&DIFFERENCES));
        assert!(!diff_complete(&DIFFERENCES[..2]));
        assert!(DIFFERENCES.iter().all(|d| !d.title.is_empty() && !d.detail.is_empty()));
    }

    #[test]
    fn reverse_is_explicitly_ambiguous() {
        assert_eq!(reverse(WIN_ACCESS_DENIED), Ok(ReverseHit { primary: EACCES, candidates: 2 }));
        assert!(ambiguity_explicit(WIN_ACCESS_DENIED));
        assert_eq!(reverse(WIN_INVALID_HANDLE), Ok(ReverseHit { primary: EBADF, candidates: 1 }));
        assert_eq!(reverse(0), Err(CompareCode::ZeroOrNegative));
        assert_eq!(reverse(777), Err(CompareCode::UnknownErrno));
    }

    #[test]
    fn grades_cover_three_levels() {
        assert_eq!(grade_count(Grade::NoCounterpart), 1);
        assert!(grade_count(Grade::Exact) >= 1);
        assert!(grade_count(Grade::Approximate) >= 1);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_compare_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 40);
for n in per.iter() { assert_eq!(*n, INJECT_ROUNDS); }
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let mut stage = CompareStage::Map;
        let mut n = 0;
while let Ok(next) = stage.advance() { stage = next; n += 1; }
        assert_eq!(n, 3);
        assert_eq!(stage, CompareStage::Seal);
        assert_eq!(stage.advance(), Err(CompareCode::StageSkip));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9276_checks_green() {
        let set = run_unx9276_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
