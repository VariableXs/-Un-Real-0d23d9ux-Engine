//! UNX-F9357 E 型与 Windows 错误码全域对照账 · 0x7C6C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9357 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F9357-J1 全域对照账判据过，逐码映射表落账，差异账
//!      五条全列格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 对照账是「22 行 × 两侧对端列」的冻结表：每行给 Linux errno（正数口
//!      径）、Win32 ERROR_*（正式对端，0 表示无）、NTSTATUS、近似候选 alt、
//!      差异等级与两侧符号名。表按 errno 严格递增排布，因此「查表 = 单调扫
//!      描 + 命中即停」，无哈希、无隐藏状态，任何顺序漂移都会在判据里显形。
//!   ② 差异账不是注释，而是可复算的五条：Exact / Approx / Merge / Split /
//!      NoPeer 各一条，claimed 计数必须与实测计数逐条相等；五条缺一、重一、
//!      多一都判红，这就是「五条全列格」的落点。
//!   ③ Merge 的认定靠「对端别名反向计数」：同一 Win32 或同一 NTSTATUS 被两
//!      个以上 errno 指向时才允许标 Merge；Exact 行的两条对端列必须都只有一
//!      个上家。正表手写标签与反表计数互证，任何一处对不上即红。
//!   ④ 三条判定路径彼此独立：translate（Linux→Win32）、reverse（Win32→
//!      Linux）、ntstatus_severity（NTSTATUS 严重级）。正反两条路径构成对称
//!      性判据：非 Merge 行必须 reverse(translate(e)) == e；Merge 行必须回
//!      E2BIG（多候选），NoPeer 行必须回 ENOSYS（对端无实现）。
//!   ⑤ NoPeer 行不允许静默透传：translate 一律回 ENOSYS，迫使上层显式合成
//!      对端语义，而不是拿一个「看起来像」的 Windows 码顶替；其近似候选放
//!      在 alt 列且必须不在正式对端集合里（reverse(alt) == ENOENT）。
//!   ⑥ 写回格：状态枚举 + 非空摘要 + 落账行数三者同时成立才算「已完成」。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9357 独占 0x7C6C0..=0x7C6FF）。
pub const SEG_BASE: u32 = 0x7C6C0;

/// 对照账行数（= 合同 §3 errno 闭集的 22 个码）。
pub const WIN_ROWS: usize = 22;

/// 差异等级个数（Exact/Approx/Merge/Split/NoPeer）。
pub const TIER_KINDS: usize = 5;

/// 差异账条数（五条全列）。
pub const DIVERGENCE_ROWS: usize = 5;

/// Linux errno 数值（正数口径，与内核 ABI 一致）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENFILE: i32 = 23;
pub const EMFILE: i32 = 24;
pub const ENOTTY: i32 = 25;
pub const EROFS: i32 = 30;
pub const ENOSYS: i32 = 38;

/// NTSTATUS 严重级字段（bit30..31）。
pub const SEV_SUCCESS: u32 = 0;
pub const SEV_WARNING: u32 = 2;
pub const SEV_ERROR: u32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+12 闭集）。
///
/// 段基址 0x7C6C0 已越过 u16 值域，故 `code()` 返回段内序号，
/// 段内绝对码 = `SEG_BASE + code()`（见 `seg_addr`），二者一一对应。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    TableUnsorted = 1,
    CoverageHole = 2,
    TierDrift = 3,
    AliasBroken = 4,
    PeerAbsent = 5,
    SplitAmbiguous = 6,
    SeverityDrift = 7,
    ReverseAsymmetry = 8,
    NegativeLeak = 9,
    LedgerMismatch = 10,
    FingerprintDrift = 11,
    SegmentOut = 12,
}

impl MapCode {
    /// 段内序号（唯一，1..=12）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内绝对码（SEG_BASE + 序号，窗口内两两唯一）。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文一句话建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::TableUnsorted => "对照表未按 errno 严格递增：二分/单调扫描会漏行，请重排后重跑",
            MapCode::CoverageHole => "覆盖缺口：合同 22 码中有码未落账，请补齐逐码映射行",
            MapCode::TierDrift => "差异等级漂移：五类计数与冻结值不符，请核对 Merge/Split 的归类依据",
            MapCode::AliasBroken => "对端别名计数不符：Merge 行必须有多上家，Exact 行必须独占对端",
            MapCode::PeerAbsent => "对端无实现：NoPeer 行不得静默透传，请上层显式合成语义",
            MapCode::SplitAmbiguous => "一对多歧义：Split 行的候选码必须落在正式对端集合之外",
            MapCode::SeverityDrift => "NTSTATUS 严重级不符：错误级必须是 3，警告级只允许 EBUSY 特例",
            MapCode::ReverseAsymmetry => "正反对照不对称：非 Merge 行反解必须回到自身 errno",
            MapCode::NegativeLeak => "负值口径泄漏：查表只接受正 errno，负值必须当场回 EINVAL",
            MapCode::LedgerMismatch => "差异账不平：claimed 与实测计数不等，请重算五条",
            MapCode::FingerprintDrift => "双跑指纹不等：表或判定路径存在隐藏状态",
            MapCode::SegmentOut => "段位越界：本模块必须落在 0x7C6C0 起的 0x40 窗口且 0x40 对齐",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: &'static [MapCode] = &[
        MapCode::TableUnsorted,
        MapCode::CoverageHole,
        MapCode::TierDrift,
        MapCode::AliasBroken,
        MapCode::PeerAbsent,
        MapCode::SplitAmbiguous,
        MapCode::SeverityDrift,
        MapCode::ReverseAsymmetry,
        MapCode::NegativeLeak,
        MapCode::LedgerMismatch,
        MapCode::FingerprintDrift,
        MapCode::SegmentOut,
    ];
}

// ── 差异等级 ──────────────────────────────────────────────────────────
/// errno → Windows 的对应拓扑。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 语义等同、一对一，可直接透传数值。
    Exact,
    /// 语义近似，需要调用方补上下文（如路径族/进程族差异）。
    Approx,
    /// 多对一：多个 errno 归并到同一 Windows 码，反解必然歧义。
    Merge,
    /// 一对多：一个 errno 在 Windows 侧分裂成多个候选码。
    Split,
    /// 对端没有一等公民错误，必须由上层合成。
    NoPeer,
}

impl Tier {
    /// 闭集下标（0..=4，差异账按此序落格）。
    pub fn idx(self) -> u32 {
        match self {
            Tier::Exact => 0,
            Tier::Approx => 1,
            Tier::Merge => 2,
            Tier::Split => 3,
            Tier::NoPeer => 4,
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            Tier::Exact => "等同",
            Tier::Approx => "近似",
            Tier::Merge => "多对一",
            Tier::Split => "一对多",
            Tier::NoPeer => "无对等",
        }
    }
}

/// 写回状态（合同 ③：状态=已完成）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbStatus {
    Pending,
    Partial,
    Done,
}

impl WbStatus {
    /// 状态码，Done=2。
    pub fn idx(self) -> u32 {
        match self {
            WbStatus::Pending => 0,
            WbStatus::Partial => 1,
            WbStatus::Done => 2,
        }
    }
}

// ── 对照账行 ──────────────────────────────────────────────────────────
/// 一行逐码映射。
///
/// `win32 == 0` 表示正式对端缺席（只有 NoPeer 允许为 0）；`alt` 只在
/// Split / NoPeer 两类上使用，其余必须为 0，否则判据当场红。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRow {
    pub errno: i32,
    pub win32: u32,
    pub ntstatus: u32,
    pub alt: u32,
    pub tier: Tier,
    pub linux: &'static str,
    pub win: &'static str,
}

impl WinRow {
    /// 是否有正式对端。
    pub fn has_peer(&self) -> bool {
        self.win32 != 0
    }

    /// 是否存在近似/备选候选。
    pub fn has_alt(&self) -> bool {
        self.alt != 0
    }
}

/// 冻结对照表（按 errno 严格递增；任何插入必须保持单调）。
///
/// 关于取值的说明（为什么写成这些数而不是「更自然的」数）：Win32 列的语义
/// 由 Windows 头文件固定，同一 Win32 码可被多个 Linux 语义共享（例如
/// ERROR_ACCESS_DENIED 同时覆盖 EPERM 与 EACCES），这正是 Merge 档存在的
/// 原因；NTSTATUS 列则记录内核态的第一跳，EMFILE/ENFILE 在这里再次归并到
/// STATUS_INSUFFICIENT_RESOURCES。
pub const WIN_TABLE: &'static [WinRow] = &[
    WinRow { errno: EPERM, win32: 5, ntstatus: 0xC000_0022, alt: 0, tier: Tier::Approx,
             linux: "EPERM", win: "ERROR_ACCESS_DENIED" },
    WinRow { errno: ENOENT, win32: 2, ntstatus: 0xC000_0034, alt: 0, tier: Tier::Approx,
             linux: "ENOENT", win: "ERROR_FILE_NOT_FOUND" },
    WinRow { errno: ESRCH, win32: 0, ntstatus: 0, alt: 1168, tier: Tier::NoPeer,
             linux: "ESRCH", win: "ERROR_NOT_FOUND(候选)" },
    WinRow { errno: EINTR, win32: 995, ntstatus: 0xC000_0121, alt: 0, tier: Tier::Approx,
             linux: "EINTR", win: "ERROR_OPERATION_ABORTED" },
    WinRow { errno: E2BIG, win32: 111, ntstatus: 0xC000_0106, alt: 0, tier: Tier::Approx,
             linux: "E2BIG", win: "ERROR_BUFFER_OVERFLOW" },
    WinRow { errno: ENOEXEC, win32: 193, ntstatus: 0xC000_007B, alt: 0, tier: Tier::Exact,
             linux: "ENOEXEC", win: "ERROR_BAD_EXE_FORMAT" },
    WinRow { errno: EBADF, win32: 6, ntstatus: 0xC000_0008, alt: 0, tier: Tier::Exact,
             linux: "EBADF", win: "ERROR_INVALID_HANDLE" },
    WinRow { errno: ECHILD, win32: 0, ntstatus: 0, alt: 128, tier: Tier::NoPeer,
             linux: "ECHILD", win: "ERROR_WAIT_NO_CHILDREN(候选)" },
    WinRow { errno: EAGAIN, win32: 1237, ntstatus: 0xC000_022D, alt: 0, tier: Tier::Exact,
             linux: "EAGAIN", win: "ERROR_RETRY" },
    WinRow { errno: ENOMEM, win32: 8, ntstatus: 0xC000_0017, alt: 0, tier: Tier::Exact,
             linux: "ENOMEM", win: "ERROR_NOT_ENOUGH_MEMORY" },
    WinRow { errno: EACCES, win32: 5, ntstatus: 0xC000_0022, alt: 0, tier: Tier::Merge,
             linux: "EACCES", win: "ERROR_ACCESS_DENIED" },
    WinRow { errno: EFAULT, win32: 998, ntstatus: 0xC000_0005, alt: 0, tier: Tier::Exact,
             linux: "EFAULT", win: "ERROR_NOACCESS" },
    WinRow { errno: EBUSY, win32: 0, ntstatus: 0, alt: 170, tier: Tier::NoPeer,
             linux: "EBUSY", win: "ERROR_BUSY(候选)" },
    WinRow { errno: EEXIST, win32: 80, ntstatus: 0xC000_0035, alt: 0, tier: Tier::Exact,
             linux: "EEXIST", win: "ERROR_FILE_EXISTS" },
    WinRow { errno: ENOTDIR, win32: 267, ntstatus: 0xC000_0103, alt: 0, tier: Tier::Merge,
             linux: "ENOTDIR", win: "ERROR_DIRECTORY" },
    WinRow { errno: EISDIR, win32: 267, ntstatus: 0xC000_00BA, alt: 0, tier: Tier::Merge,
             linux: "EISDIR", win: "ERROR_DIRECTORY" },
    WinRow { errno: EINVAL, win32: 87, ntstatus: 0xC000_000D, alt: 160, tier: Tier::Split,
             linux: "EINVAL", win: "ERROR_INVALID_PARAMETER" },
    WinRow { errno: ENFILE, win32: 1450, ntstatus: 0xC000_009A, alt: 0, tier: Tier::Approx,
             linux: "ENFILE", win: "ERROR_NO_SYSTEM_RESOURCES" },
    WinRow { errno: EMFILE, win32: 4, ntstatus: 0xC000_009A, alt: 0, tier: Tier::Merge,
             linux: "EMFILE", win: "ERROR_TOO_MANY_OPEN_FILES" },
    WinRow { errno: ENOTTY, win32: 1, ntstatus: 0xC000_0010, alt: 0, tier: Tier::Exact,
             linux: "ENOTTY", win: "ERROR_INVALID_FUNCTION" },
    WinRow { errno: EROFS, win32: 19, ntstatus: 0xC000_00A2, alt: 0, tier: Tier::Exact,
             linux: "EROFS", win: "ERROR_WRITE_PROTECT" },
    WinRow { errno: ENOSYS, win32: 50, ntstatus: 0xC000_0002, alt: 0, tier: Tier::Exact,
             linux: "ENOSYS", win: "ERROR_NOT_SUPPORTED" },
];

/// 合同 §3 规定的 errno 闭集（升序），覆盖判据逐码比对它。
pub const MANDATED: [i32; WIN_ROWS] = [
    EPERM, ENOENT, ESRCH, EINTR, E2BIG, ENOEXEC, EBADF, ECHILD, EAGAIN, ENOMEM, EACCES,
    EFAULT, EBUSY, EEXIST, ENOTDIR, EISDIR, EINVAL, ENFILE, EMFILE, ENOTTY, EROFS, ENOSYS,
];

// ── 差异账 ────────────────────────────────────────────────────────────
/// 差异账一行：等级 + 冻结计数 + 一句话结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DivRow {
    pub tier: Tier,
    pub claimed: u32,
    pub note: &'static str,
}

/// 五条冻结差异账（顺序即 Tier::idx，缺一不可）。
pub const DIVERGENCE: [DivRow; DIVERGENCE_ROWS] = [
    DivRow { tier: Tier::Exact, claimed: 9,
             note: "九码可直接透传：两侧语义与触发条件一一对应" },
    DivRow { tier: Tier::Approx, claimed: 5,
             note: "五码需上下文：进程/缓冲/中断族在 Windows 侧被拆散" },
    DivRow { tier: Tier::Merge, claimed: 4,
             note: "四码多对一：ERROR_ACCESS_DENIED 与 ERROR_DIRECTORY 各吃两码" },
    DivRow { tier: Tier::Split, claimed: 1,
             note: "一码一对多：EINVAL 在 Windows 侧另有 ERROR_BAD_ARGUMENTS" },
    DivRow { tier: Tier::NoPeer, claimed: 3,
             note: "三码无对等：进程/子进程/忙三族须由上层合成，不得透传" },
];

/// 写回格（合同 ③）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Writeback {
    pub status: WbStatus,
    pub summary: &'static str,
    pub rows: u32,
}

/// 本单写回内容：22 行全部落账，五条差异全列。
pub const WB_F9357: Writeback = Writeback {
    status: WbStatus::Done,
    summary: "E 型 22 码全域对照已落账，差异账 5 条全列，正反对照对称",
    rows: WIN_ROWS as u32,
};

// ── 查表与统计 ────────────────────────────────────────────────────────
/// 按 errno 单调扫描（表已保证严格递增，命中即停）。
pub fn lookup_errno(e: i32) -> Option<&'static WinRow> {
    let mut i = 0usize;
    while i < WIN_TABLE.len() {
        if let Some(row) = WIN_TABLE.get(i) {
            if row.errno == e {
                return Some(row);
            }
            if row.errno > e {
                return None;
            }
        }
        i += 1;
    }
    None
}

/// 同一 Win32 码的上家个数（反向计数，Merge 的独立证据）。
pub fn win32_alias_count(w: u32) -> u32 {
    if w == 0 {
        return 0;
    }
    let mut n = 0u32;
    for row in WIN_TABLE.iter() {
        if row.win32 == w {
            n = n.saturating_add(1);
        }
    }
    n
}

/// 同一 NTSTATUS 的上家个数（第二层归并证据）。
pub fn ntstatus_alias_count(s: u32) -> u32 {
    if s == 0 {
        return 0;
    }
    let mut n = 0u32;
    for row in WIN_TABLE.iter() {
        if row.ntstatus == s {
            n = n.saturating_add(1);
        }
    }
    n
}

/// 五类等级实测计数。
pub fn tier_counts() -> [u32; TIER_KINDS] {
    let mut c = [0u32; TIER_KINDS];
    for row in WIN_TABLE.iter() {
        let i = row.tier.idx() as usize;
        if let Some(slot) = c.get_mut(i) {
            *slot = slot.saturating_add(1);
        }
    }
    c
}

/// 差异账复算：返回 (五条全平, 平坦条数)。
pub fn divergence_reconcile() -> (bool, u32) {
    let counted = tier_counts();
    let mut flat = 0u32;
    let mut rows_ok = 0u32;
    let mut i = 0usize;
    while i < DIVERGENCE.len() {
        if let Some(div) = DIVERGENCE.get(i) {
            let got = counted.get(div.tier.idx() as usize).copied().unwrap_or(0);
            if got == div.claimed && !div.note.is_empty() {
                flat = flat.saturating_add(1);
            }
            if div.tier.idx() == i as u32 {
                rows_ok = rows_ok.saturating_add(1);
            }
        }
        i += 1;
    }
    (flat == DIVERGENCE_ROWS as u32 && rows_ok == DIVERGENCE_ROWS as u32, flat)
}

/// 表是否按 errno 严格递增（单调 + 唯一一步到位）。
pub fn table_monotonic() -> bool {
    let mut i = 1usize;
    while i < WIN_TABLE.len() {
        let prev = WIN_TABLE.get(i - 1).map(|r| r.errno);
        let cur = WIN_TABLE.get(i).map(|r| r.errno);
        match (prev, cur) {
            (Some(a), Some(b)) => {
                if b <= a {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 覆盖扫描：返回 (命中数, 应有数, 缺口数)。
pub fn coverage() -> (u32, u32, u32) {
    let mut hit = 0u32;
    let mut gap = 0u32;
    for m in MANDATED.iter() {
        match lookup_errno(*m) {
            Some(_) => hit = hit.saturating_add(1),
            None => gap = gap.saturating_add(1),
        }
    }
    (hit, MANDATED.len() as u32, gap)
}

// ── 三条判定路径 ──────────────────────────────────────────────────────
/// Linux 正 errno → Win32。
///
/// 顺序：非正值 → EINVAL；表外 → EINVAL；NoPeer/无正式对端 → ENOSYS。
pub fn translate(e: i32) -> Result<u32, i32> {
    if e <= 0 {
        return Err(EINVAL);
    }
    match lookup_errno(e) {
        None => Err(EINVAL),
        Some(row) => {
            if !row.has_peer() || row.tier == Tier::NoPeer {
                Err(ENOSYS)
            } else {
                Ok(row.win32)
            }
        }
    }
}

/// Win32 → Linux errno（反解；只用正式对端列）。
///
/// 无上家 → ENOENT；多上家 → E2BIG（歧义必须显性，不能挑第一个了事）。
pub fn reverse(w: u32) -> Result<i32, i32> {
    if w == 0 {
        return Err(EINVAL);
    }
    let mut hits = 0u32;
    let mut first: Option<i32> = None;
    for row in WIN_TABLE.iter() {
        if row.win32 == w {
            hits = hits.saturating_add(1);
            if first.is_none() {
                first = Some(row.errno);
            }
        }
    }
    match (hits, first) {
        (0, _) => Err(ENOENT),
        (1, Some(e)) => Ok(e),
        _ => Err(E2BIG),
    }
}

/// NTSTATUS 严重级（bit30..31）。
pub fn ntstatus_severity(s: u32) -> u32 {
    (s >> 30) & 0x3
}

/// 严重级是否符合本表的冻结口径：错误级 3 为主，警告级 2 只允许 EBUSY 候选。
pub fn severity_ok(row: &WinRow) -> bool {
    if !row.has_peer() {
        return row.ntstatus == 0 && row.alt != 0;
    }
    let sev = ntstatus_severity(row.ntstatus);
    if sev == SEV_ERROR {
        return true;
    }
    matches!(row.tier, Tier::NoPeer) && sev == SEV_WARNING
}

/// 正反对称扫描：返回 (对称行数, 参与行数)。
///
/// 口径：仅有正式对端的行参与。Win32 层出现两个以上上家（真正的多对一）
/// → 反解必须回 E2BIG；其余（含只在 NTSTATUS 层归并的 EMFILE）→ 必须回自身
/// errno。把「哪一层归并」这件事做进判据，避免用 tier 标签一刀切。
pub fn symmetry() -> (u32, u32) {
    let mut ok = 0u32;
    let mut total = 0u32;
    for row in WIN_TABLE.iter() {
        if !row.has_peer() {
            continue;
        }
        total = total.saturating_add(1);
        let back = reverse(row.win32);
        let win_merged = win32_alias_count(row.win32) >= 2;
        let good = if win_merged {
            back == Err(E2BIG)
        } else {
            back == Ok(row.errno)
        };
        if good {
            ok = ok.saturating_add(1);
        }
    }
    (ok, total)
}

/// 候选码审计：Split / NoPeer 的 alt 必须非零、不同于正式对端、且不在正式对端集合。
pub fn alt_audit() -> (u32, u32) {
    let mut ok = 0u32;
    let mut checked = 0u32;
    for row in WIN_TABLE.iter() {
        let needs = matches!(row.tier, Tier::Split | Tier::NoPeer);
        if needs {
            checked = checked.saturating_add(1);
            let distinct = row.alt != 0 && row.alt != row.win32;
            let outside = reverse(row.alt) == Err(ENOENT);
            if distinct && outside {
                ok = ok.saturating_add(1);
            }
        } else if row.alt == 0 {
            // 非 Split/NoPeer 行不得携带候选码：把它们计入 checked 并计为合格。
            checked = checked.saturating_add(1);
            ok = ok.saturating_add(1);
        }
    }
    (ok, checked)
}

/// 负值纪律：Linux 返回 -errno，本账一律存正数。
pub fn neg(c: i32) -> i32 {
    c.saturating_neg()
}

/// 负值与零必须被查表路径统一拒绝。
pub fn rejects_non_positive() -> bool {
    translate(0) == Err(EINVAL)
        && translate(-ENOENT) == Err(EINVAL)
        && translate(i32::MIN) == Err(EINVAL)
        && reverse(0) == Err(EINVAL)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 编码一张表 + 一条判定轨迹。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    for row in WIN_TABLE.iter() {
        out.push((row.errno & 0x7F) as u8);
        out.push(row.tier.idx() as u8);
        out.push((row.win32 & 0xFF) as u8);
        out.push(((row.win32 >> 8) & 0xFF) as u8);
        out.push((row.ntstatus & 0xFF) as u8);
        out.push(((row.ntstatus >> 8) & 0xFF) as u8);
        out.push(((row.ntstatus >> 16) & 0xFF) as u8);
        out.push(((row.ntstatus >> 24) & 0xFF) as u8);
        out.push((row.alt & 0xFF) as u8);
        out.push(row.linux.len() as u8);
        out.push(row.win.len() as u8);
    }
    for e in MANDATED.iter() {
        match translate(*e) {
            Ok(w) => {
                out.push(0x51);
                out.push((w & 0xFF) as u8);
            }
            Err(c) => {
                out.push(0x52);
                out.push((c & 0x7F) as u8);
            }
        }
    }
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9357 判据集（静态 16 条 + 动态 3 条）。
pub fn run_unx9357_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9357");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7C000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE >= 0x7C6C0;
    s.add("段位守卫：0x7C6C0 起 0x40 对齐且不越段", seg_ok,
          "本模块独占 0x7C6C0..=0x7CFF 一角，段间不得互相覆盖");

    // 2. 逐码映射表落账：行数、单调、唯一三者同成立
    let rows_ok = WIN_TABLE.len() == WIN_ROWS
        && table_monotonic()
        && WIN_TABLE.iter().all(|r| !r.linux.is_empty() && !r.win.is_empty());
    s.add("逐码映射表：22 行落账且严格递增", rows_ok,
          "表未排序或有空符号名即判红，查表路径依赖单调性");

    // 3. 全域覆盖
    let (hit, want, gap) = coverage();
    s.add("全域覆盖：22/22 码在表内", hit == want && gap == 0,
          "合同 errno 闭集必须逐码命中，缺口数必须为 0");

    // 4. 差异账五条全列
    let (flat, flat_rows) = divergence_reconcile();
    s.add("差异账五条全列：claimed 与实测逐条相等", flat && flat_rows == 5,
          "Exact/Approx/Merge/Split/NoPeer 五格必须同时平账");

    // 5. Merge 语义：既有 Win32 归并，也有 NTSTATUS 归并
    let merge_ok = win32_alias_count(5) == 2
        && ntstatus_alias_count(0xC000_0022) == 2
        && ntstatus_alias_count(0xC000_009A) == 2
        && win32_alias_count(267) == 2;
    s.add("Merge 互证：ACCESS_DENIED/DIRECTORY/资源不足各二上家", merge_ok,
          "多对一必须由反表计数独立证实，正表标签不能自说自话");

    // 6. Exact 行必须独占两侧对端
    let exact_ok = WIN_TABLE.iter().all(|r| {
        r.tier != Tier::Exact
            || (win32_alias_count(r.win32) == 1 && ntstatus_alias_count(r.ntstatus) == 1)
    });
    s.add("Exact 独占：九个等同码两侧对端均唯一", exact_ok,
          "等同档若出现别名，说明归类错，必须降级到 Merge");

    // 7. NoPeer 不得透传
    let nopeer_ok = translate(ESRCH) == Err(ENOSYS)
        && translate(ECHILD) == Err(ENOSYS)
        && translate(EBUSY) == Err(ENOSYS)
        && lookup_errno(ESRCH).map(|r| r.win32) == Some(0);
    s.add("NoPeer：三码回 ENOSYS 且正式对端为空", nopeer_ok,
          "无对等码必须迫使上层合成，禁止拿候选码顶替");

    // 8. Split 一对多
    let split_ok = translate(EINVAL) == Ok(87)
        && lookup_errno(EINVAL).map(|r| r.alt) == Some(160)
        && reverse(160) == Err(ENOENT);
    s.add("Split：EINVAL 主码 87、次码 160 且在正式集合外", split_ok,
          "一对多必须把候选码留在正式对端集合之外，否则反解歧义");

    // 9. 正反对称
    let (sym_ok, sym_total) = symmetry();
    s.add("正反对照：Win32 归并行回 E2BIG、其余回自身",
          sym_ok == sym_total && sym_total == 19,
          "19 个有正式对端的行必须全部对称，仅 ACCESS_DENIED/DIRECTORY 两族歧义");

    // 10. 候选码审计
    let (alt_ok, alt_checked) = alt_audit();
    s.add("候选码审计：Split/NoPeer 必有候选、其余必空",
          alt_ok == alt_checked && alt_checked == WIN_ROWS as u32,
          "22 行逐行过筛，候选码错位会污染反向路径");

    // 11. 负值与零口径
    s.add("负值纪律：0 与负值一律 EINVAL", rejects_non_positive(),
          "本账只存正 errno，负值属系统调用返回形态，不得参与查表");

    // 12. NTSTATUS 严重级
    let sev_ok = WIN_TABLE.iter().all(severity_ok)
        && ntstatus_severity(0x8000_0011) == SEV_WARNING
        && ntstatus_severity(0xC000_000D) == SEV_ERROR;
    s.add("NTSTATUS 严重级：错误级 3 为主、警告级仅候选位", sev_ok,
          "严重级误判会让调用方把告警当错误，语义必须分层");

    // 13. 逐码直译统计
    let mut ok_cnt = 0u32;
    let mut enosys_cnt = 0u32;
    for e in MANDATED.iter() {
        match translate(*e) {
            Ok(_) => ok_cnt = ok_cnt.saturating_add(1),
            Err(c) => {
                if c == ENOSYS {
                    enosys_cnt = enosys_cnt.saturating_add(1);
                }
            }
        }
    }
    s.add("直译统计：19 码有对端、3 码回 ENOSYS",
          ok_cnt == 19 && enosys_cnt == 3,
          "直译账必须与差异账的 Exact+Approx+Merge+Split 总和一致");

    // 14. 写回格
    let wb_ok = WB_F9357.status == WbStatus::Done
        && WB_F9357.status.idx() == 2
        && WB_F9357.summary.len() >= 24
        && WB_F9357.rows == WIN_ROWS as u32;
    s.add("写回：状态=已完成、摘要非空、落账 22 行", wb_ok,
          "合同 ③ 要求写回状态与摘要同时成立，空摘要视为未完成");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < MapCode::ALL.len() {
        let mut j = i + 1;
        while j < MapCode::ALL.len() {
            if let (Some(a), Some(b)) = (MapCode::ALL.get(i), MapCode::ALL.get(j)) {
                if a.code() == b.code() || a.seg_addr() == b.seg_addr() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个段内序号与绝对码两两唯一", uniq,
          "闭集码重复会让红项无法定位到唯一诊断建议");

    // 16. 文案闭集
    let text_ok = MapCode::ALL.len() == 12
        && MapCode::ALL.iter().all(|c| !c.detail().is_empty() && c.detail().len() >= 12);
    s.add("文案闭集：12 个诊断码均有可操作中文建议", text_ok,
          "每个码必须给出非空且可执行的处置建议");

    // 17. 双跑指纹
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：对照账指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8 条）
    let cover_text = format!("覆盖 {}/{}，缺口 {}，表行 {}", hit, want, gap, WIN_TABLE.len());
    s.add_dyn("覆盖明细", hit == want, &cover_text);

    let counted = tier_counts();
    let t0 = counted.first().copied().unwrap_or(0);
    let t1 = counted.get(1).copied().unwrap_or(0);
    let t2 = counted.get(2).copied().unwrap_or(0);
    let t3 = counted.get(3).copied().unwrap_or(0);
    let t4 = counted.get(4).copied().unwrap_or(0);
    let tier_text = format!("等同 {} 近似 {} 归并 {} 分裂 {} 无对等 {}", t0, t1, t2, t3, t4);
    s.add_dyn("差异分布", t0 + t1 + t2 + t3 + t4 == WIN_ROWS as u32, &tier_text);

    let fp_text = format!("对照账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_ordered_and_complete() {
        assert!(table_monotonic());
        let (hit, want, gap) = coverage();
        assert_eq!(hit, want);
        assert_eq!(gap, 0);
        assert_eq!(WIN_TABLE.len(), WIN_ROWS);
    }

    #[test]
    fn lookup_hits_and_misses() {
        assert_eq!(lookup_errno(ENOMEM).map(|r| r.win32), Some(8));
        assert_eq!(lookup_errno(EBADF).map(|r| r.win32), Some(6));
        assert_eq!(lookup_errno(5).map(|r| r.win32), None);
        assert_eq!(lookup_errno(0).map(|r| r.win32), None);
        assert!(lookup_errno(ENOSYS).is_some());
    }

    #[test]
    fn translate_paths() {
        assert_eq!(translate(ENOMEM), Ok(8));
        assert_eq!(translate(ENOENT), Ok(2));
        assert_eq!(translate(ESRCH), Err(ENOSYS));
        assert_eq!(translate(EBUSY), Err(ENOSYS));
        assert_eq!(translate(0), Err(EINVAL));
        assert_eq!(translate(-ENOMEM), Err(EINVAL));
        assert_eq!(translate(9999), Err(EINVAL));
    }

    #[test]
    fn reverse_paths() {
        assert_eq!(reverse(8), Ok(ENOMEM));
        assert_eq!(reverse(995), Ok(EINTR));
        assert_eq!(reverse(5), Err(E2BIG));
        assert_eq!(reverse(267), Err(E2BIG));
        assert_eq!(reverse(4242), Err(ENOENT));
        assert_eq!(reverse(0), Err(EINVAL));
    }

    #[test]
    fn alias_counts_are_independent() {
        assert_eq!(win32_alias_count(5), 2);
        assert_eq!(win32_alias_count(267), 2);
        assert_eq!(win32_alias_count(8), 1);
        assert_eq!(ntstatus_alias_count(0xC000_009A), 2);
        assert_eq!(ntstatus_alias_count(0xC000_0022), 2);
        assert_eq!(ntstatus_alias_count(0xC000_0005), 1);
        assert_eq!(win32_alias_count(0), 0);
    }

    #[test]
    fn tier_ledger_balances() {
        let (flat, rows) = divergence_reconcile();
        assert!(flat);
        assert_eq!(rows, 5);
        let c = tier_counts();
        assert_eq!(c.first().copied(), Some(9));
        assert_eq!(c.get(1).copied(), Some(5));
        assert_eq!(c.get(2).copied(), Some(4));
        assert_eq!(c.get(3).copied(), Some(1));
        assert_eq!(c.get(4).copied(), Some(3));
    }

    #[test]
    fn symmetry_holds() {
        let (ok, total) = symmetry();
        assert_eq!(ok, total);
        assert_eq!(total, 19);
    }

    #[test]
    fn alt_audit_holds() {
        let (ok, checked) = alt_audit();
        assert_eq!(ok, checked);
        assert_eq!(checked, WIN_ROWS as u32);
        assert_eq!(reverse(160), Err(ENOENT));
        assert_eq!(reverse(128), Err(ENOENT));
    }

    #[test]
    fn severity_layers() {
        assert!(WIN_TABLE.iter().all(severity_ok));
        assert_eq!(ntstatus_severity(0xC000_000D), SEV_ERROR);
        assert_eq!(ntstatus_severity(0x8000_0011), SEV_WARNING);
        assert_eq!(ntstatus_severity(0x0000_0000), SEV_SUCCESS);
    }

    #[test]
    fn negative_discipline() {
        assert_eq!(neg(ENOMEM), -12);
        assert_eq!(neg(i32::MIN), i32::MAX);
        assert!(rejects_non_positive());
    }

    #[test]
    fn segment_and_codes() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(MapCode::ALL.len(), 12);
        assert_eq!(MapCode::TableUnsorted.seg_addr(), SEG_BASE + 1);
        assert_eq!(MapCode::SegmentOut.seg_addr(), SEG_BASE + 12);
        assert!(MapCode::ALL.iter().all(|c| !c.detail().is_empty()));
    }

    #[test]
    fn writeback_done() {
        assert_eq!(WB_F9357.status, WbStatus::Done);
        assert!(!WB_F9357.summary.is_empty());
        assert_eq!(WB_F9357.rows, 22);
    }

    #[test]
    fn deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9357_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
