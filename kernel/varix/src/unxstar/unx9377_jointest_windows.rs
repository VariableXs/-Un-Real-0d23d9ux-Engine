//! UNX-F9377 联调与 Windows 对照账联测面 · 0x7CBC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9377 逐字）：
//!   UNX-F9377-J1 对照账联测判据过，Windows 对照格与联测面对接可观测，
//!   差异账复用格过。
//!
//! 设计要点：
//!   ① 对照表 18 格（syscall 面 × Linux errno × Win32 码 × 分类）。分类由
//!      「Windows 是否有对应码 / 一 errno 对多码 / 多 errno 归一码」三条结构
//!      判据派生：Exact 9、Split 2、Approx 4、Merged 1、Absent 2，合计 18；
//!      分区总和必须等于表长，不接受未分类格。
//!   ② 「可观测」是可回读，不是"表里写着"：每格由 probe 从 errno 反查 Win
//!      码回读。Absent 格的可观测形态是**显性回 None**——盲格（返回 Some 但
//!      值不符、或 Absent 却回了码）一律判 CellBlind，不允许静默缺失。
//!   ③ 差异账只收非 Exact 格（9 条），append-only；同 (errno,win) 二次登记
//!      回 EEXIST，Exact 误入回 EINVAL，满 12 槽回 ENOSPC。登记即冻结指纹
//!      （FNV-1a 32 位），复用前重算，漂移即 ReuseDrift。
//!   ④ 复用格 6 条：Split/Approx/Merged 可复用（4 条放行），Absent 没有
//!      Windows 码可复用，回 ENOSYS（既非静默放行，也不与参数错误 EINVAL
//!      混同）；复用账 hits+denied 必须等于格数。
//!   ⑤ 联测面 14 步冻结脚本（6 Probe + 4 Diff + 4 Reuse）单向前进，越序
//!      EINVAL，每步只跑一次；面账 steps 必须等于脚本长度。
//!   ⑥ 写回：状态 Done + 结果摘要非空；空摘要 EINVAL，重复写回 EBUSY。
//!   ⑦ 注入四类（篡改 Win 码 / 差异账重号 / 复用指纹陈旧 / 联测越序）各 10
//!      轮，账本满足 injected == detected + escaped，且 escaped 恒为 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9377 独占 0x7CBC0..=0x7CBFF）。
pub const SEG_BASE: u32 = 0x7CBC0;
/// 段窗口上界（不含）。
pub const SEG_LIMIT: u32 = SEG_BASE + 0x40;
/// 对照表行数（18 格冻结）。
pub const TABLE_ROWS: usize = 18;
/// 差异账槽位（12 槽，9 条有效 + 3 槽余量）。
pub const DIFF_CAP: usize = 12;
/// 复用格数。
pub const REUSE_CELLS: usize = 6;
/// 联测脚本步数。
pub const JOINT_STEPS: usize = 14;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 4;
/// 注入总笔数。
pub const INJECT_TOTAL: u32 = INJECT_ROUNDS * INJECT_CLASSES;

// ── Linux errno（正数口径，与内核 errno.h 数值一致） ─────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
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
pub const EMFILE: i32 = 24;
pub const ENOTTY: i32 = 25;
pub const ENOSPC: i32 = 28;
pub const EROFS: i32 = 30;
pub const EPIPE: i32 = 32;
pub const ENOSYS: i32 = 38;

// ── Win32 错误码（GetLastError 口径） ────────────────────────────────
pub const WIN_INVALID_FUNCTION: u32 = 1;
pub const WIN_FILE_NOT_FOUND: u32 = 2;
pub const WIN_PATH_NOT_FOUND: u32 = 3;
pub const WIN_TOO_MANY_OPEN_FILES: u32 = 4;
pub const WIN_ACCESS_DENIED: u32 = 5;
pub const WIN_INVALID_HANDLE: u32 = 6;
pub const WIN_NOT_ENOUGH_MEMORY: u32 = 8;
pub const WIN_WRITE_PROTECT: u32 = 19;
pub const WIN_SHARING_VIOLATION: u32 = 32;
pub const WIN_FILE_EXISTS: u32 = 80;
pub const WIN_INVALID_PARAMETER: u32 = 87;
pub const WIN_BROKEN_PIPE: u32 = 109;
pub const WIN_DISK_FULL: u32 = 112;
pub const WIN_CALL_NOT_IMPLEMENTED: u32 = 120;
pub const WIN_ALREADY_EXISTS: u32 = 183;
pub const WIN_DIRECTORY: u32 = 267;
pub const WIN_OPERATION_ABORTED: u32 = 995;
pub const WIN_IO_PENDING: u32 = 997;
pub const WIN_NOT_FOUND: u32 = 1168;

/// Linux errno 名称表（数值唯一、名称唯一；表外 errno 不得入对照格）。
pub const ERRNO_NAMES: [(&str, i32); 18] = [
    ("EPERM", EPERM), ("ENOENT", ENOENT), ("ESRCH", ESRCH), ("EINTR", EINTR),
    ("EBADF", EBADF), ("ECHILD", ECHILD), ("EAGAIN", EAGAIN), ("ENOMEM", ENOMEM),
    ("EACCES", EACCES), ("EBUSY", EBUSY), ("EEXIST", EEXIST), ("EISDIR", EISDIR),
    ("EINVAL", EINVAL), ("EMFILE", EMFILE), ("ENOTTY", ENOTTY), ("EROFS", EROFS),
    ("EPIPE", EPIPE), ("ENOSYS", ENOSYS),
];

/// 按 errno 取 Linux 名称；表外 None。
pub fn errno_name(code: i32) -> Option<&'static str> {
    let mut i = 0usize;
    while i < ERRNO_NAMES.len() {
        if let Some(row) = ERRNO_NAMES.get(i) {
            if row.1 == code {
                return Some(row.0);
            }
        }
        i += 1;
    }
    None
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内序号 0..=13，绝对码值 SEG_BASE+1+序号）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinCode {
    CellBlind,      // 对照格不可观测（盲格/回读不符）
    DiffDup,        // 差异账重号
    ReuseDenied,    // 复用被拒（Absent 无 Windows 码）
    ReuseDrift,     // 复用指纹漂移
    OrderViolation, // 联测越序
    LedgerRegress,  // 账目倒退
    NotReproducible,// 双跑不一致
    SegmentViolation,// 段位越界
    EmptyName,      // 名称/摘要为空
    BadClass,       // 分类非法或分区不闭合
    WritebackBusy,  // 写回重复
    WritebackEmpty, // 写回摘要为空
    ProbeMismatch,  // 回读值与冻结码不符
    SurfaceShort,   // 联测步数不足
}

impl WinCode {
    /// 段内序号（u16 口径，与段基址无关）。
    pub fn code(self) -> u16 { self as u16 }
    /// 绝对诊断码值：段基址 + 1 + 序号（0x7CBC1..=0x7CBCE）。
    pub fn val(self) -> u32 { SEG_BASE + 1 + self as u32 }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WinCode::CellBlind => "Windows 对照格不可观测：probe 未回读到冻结码，或 Absent 格回了码；请先修对照表再联测",
            WinCode::DiffDup => "差异账重号：同一 (errno,win) 只许登记一次；请先查账再去重，重复登记会破坏差异计数",
            WinCode::ReuseDenied => "复用被拒：Absent 格没有 Windows 码可复用；请改引 Split/Approx/Merged 条目，回 ENOSYS",
            WinCode::ReuseDrift => "复用指纹漂移：条目冻结指纹与重算不一致；请检查是否在登记后被改写",
            WinCode::OrderViolation => "联测越序：14 步脚本只许单向前进；请从断点续跑，禁止跳步或重跑",
            WinCode::LedgerRegress => "账目倒退：差异账/注入账只增不减；请排查回滚路径是否误改累计值",
            WinCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            WinCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
            WinCode::EmptyName => "名称为空：对照格的 op/lname/wname 与写回摘要都必须非空",
            WinCode::BadClass => "分类非法：五类分区之和必须等于表长 18，且每类计数与冻结值一致",
            WinCode::WritebackBusy => "写回重复：状态已是已完成，二次写回被拒；请勿覆盖已封账的结果摘要",
            WinCode::WritebackEmpty => "写回摘要为空：完成态必须携带非空结果摘要；请写明对照格数与差异复用结论",
            WinCode::ProbeMismatch => "回读不符：probe 返回的 Win 码与冻结格不一致；请核对 errno→Win 映射表",
            WinCode::SurfaceShort => "联测面不足：14 步未走满；请补齐 Probe/Diff/Reuse 三类步数后再收口",
        }
    }
    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: &'static [WinCode] = &[
        WinCode::CellBlind, WinCode::DiffDup, WinCode::ReuseDenied, WinCode::ReuseDrift,
        WinCode::OrderViolation, WinCode::LedgerRegress, WinCode::NotReproducible,
        WinCode::SegmentViolation, WinCode::EmptyName, WinCode::BadClass,
        WinCode::WritebackBusy, WinCode::WritebackEmpty, WinCode::ProbeMismatch,
        WinCode::SurfaceShort,
    ];
}

// ── 差异分类 ──────────────────────────────────────────────────────────
/// Linux errno 与 Win32 码的结构关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffClass {
    Exact,  // 一码对一码，语义等价
    Split,  // 一 errno 对多码（ENOENT→FILE/PATH_NOT_FOUND）
    Approx, // 语义近似但归属子系统不同（ENOTTY→INVALID_FUNCTION）
    Merged, // 多 errno 归一码（EPERM/EACCES→ACCESS_DENIED）
    Absent, // Windows 侧无对应码，必须由调用方合成
}

impl DiffClass {
    /// 冻结编码（分区计数与指纹使用）。
    pub fn code(self) -> u8 {
        match self {
            DiffClass::Exact => 0,
            DiffClass::Split => 1,
            DiffClass::Approx => 2,
            DiffClass::Merged => 3,
            DiffClass::Absent => 4,
        }
    }
    /// 该分类的条目是否可被后续联测复用。
    ///
    /// 不变量：Absent 没有 Windows 码，复用等于凭空造码，必须拒绝；其余四类
    /// 都冻结了至少一个实码，可安全复用。
    pub fn reusable(self) -> bool { !matches!(self, DiffClass::Absent) }
}

// ── 对照表 ────────────────────────────────────────────────────────────
/// 一格 Windows 对照账：syscall 面 + Linux errno + Win32 主码/副码 + 分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinCell {
    pub op: &'static str,
    pub errno: i32,
    pub lname: &'static str,
    pub win: u32,
    pub wname: &'static str,
    pub alt: u32,
    pub class: DiffClass,
}

/// 18 格冻结对照表。
///
/// 为什么 EBADF 出现两次：read 与 close 两条路径的 Win 码同源（都落
/// ERROR_INVALID_HANDLE=6），但它们是两条独立 syscall 面，联测必须分格
/// 观测；去重是差异账的事，不是对照表的事。
pub const WIN_TABLE: [WinCell; TABLE_ROWS] = [
    WinCell { op: "read", errno: EBADF, lname: "EBADF", win: WIN_INVALID_HANDLE, wname: "ERROR_INVALID_HANDLE", alt: 0, class: DiffClass::Exact },
    WinCell { op: "write", errno: EPIPE, lname: "EPIPE", win: WIN_BROKEN_PIPE, wname: "ERROR_BROKEN_PIPE", alt: 0, class: DiffClass::Exact },
    WinCell { op: "open", errno: ENOENT, lname: "ENOENT", win: WIN_FILE_NOT_FOUND, wname: "ERROR_FILE_NOT_FOUND", alt: WIN_PATH_NOT_FOUND, class: DiffClass::Split },
    WinCell { op: "open", errno: EACCES, lname: "EACCES", win: WIN_ACCESS_DENIED, wname: "ERROR_ACCESS_DENIED", alt: 0, class: DiffClass::Exact },
    WinCell { op: "open", errno: EEXIST, lname: "EEXIST", win: WIN_FILE_EXISTS, wname: "ERROR_FILE_EXISTS", alt: WIN_ALREADY_EXISTS, class: DiffClass::Split },
    WinCell { op: "open", errno: EISDIR, lname: "EISDIR", win: 0, wname: "NONE", alt: 0, class: DiffClass::Absent },
    WinCell { op: "close", errno: EBADF, lname: "EBADF", win: WIN_INVALID_HANDLE, wname: "ERROR_INVALID_HANDLE", alt: 0, class: DiffClass::Exact },
    WinCell { op: "lseek", errno: EINVAL, lname: "EINVAL", win: WIN_INVALID_PARAMETER, wname: "ERROR_INVALID_PARAMETER", alt: 0, class: DiffClass::Exact },
    WinCell { op: "mmap", errno: ENOMEM, lname: "ENOMEM", win: WIN_NOT_ENOUGH_MEMORY, wname: "ERROR_NOT_ENOUGH_MEMORY", alt: 0, class: DiffClass::Exact },
    WinCell { op: "mmap", errno: EPERM, lname: "EPERM", win: WIN_ACCESS_DENIED, wname: "ERROR_ACCESS_DENIED", alt: 0, class: DiffClass::Merged },
    WinCell { op: "ioctl", errno: ENOTTY, lname: "ENOTTY", win: WIN_INVALID_FUNCTION, wname: "ERROR_INVALID_FUNCTION", alt: 0, class: DiffClass::Approx },
    WinCell { op: "dup2", errno: EMFILE, lname: "EMFILE", win: WIN_TOO_MANY_OPEN_FILES, wname: "ERROR_TOO_MANY_OPEN_FILES", alt: 0, class: DiffClass::Exact },
    WinCell { op: "fsync", errno: EROFS, lname: "EROFS", win: WIN_WRITE_PROTECT, wname: "ERROR_WRITE_PROTECT", alt: 0, class: DiffClass::Exact },
    WinCell { op: "unlink", errno: EBUSY, lname: "EBUSY", win: WIN_SHARING_VIOLATION, wname: "ERROR_SHARING_VIOLATION", alt: 0, class: DiffClass::Approx },
    WinCell { op: "wait4", errno: ECHILD, lname: "ECHILD", win: 0, wname: "NONE", alt: 0, class: DiffClass::Absent },
    WinCell { op: "futex", errno: EAGAIN, lname: "EAGAIN", win: WIN_IO_PENDING, wname: "ERROR_IO_PENDING", alt: 0, class: DiffClass::Approx },
    WinCell { op: "kill", errno: ESRCH, lname: "ESRCH", win: WIN_NOT_FOUND, wname: "ERROR_NOT_FOUND", alt: 0, class: DiffClass::Approx },
    WinCell { op: "syscall", errno: ENOSYS, lname: "ENOSYS", win: WIN_CALL_NOT_IMPLEMENTED, wname: "ERROR_CALL_NOT_IMPLEMENTED", alt: 0, class: DiffClass::Exact },
];

/// Linux errno → Win32 主码（表内首个非零匹配；表外 None）。
pub fn win_of(errno: i32) -> Option<u32> {
    let mut i = 0usize;
    while i < WIN_TABLE.len() {
        if let Some(cell) = WIN_TABLE.get(i) {
            if cell.errno == errno && cell.win != 0 {
                return Some(cell.win);
            }
        }
        i += 1;
    }
    None
}

/// Win32 码（主码或副码）→ Linux errno；0 码与表外码 None。
pub fn errno_of(win: u32) -> Option<i32> {
    if win == 0 {
        return None;
    }
    let mut i = 0usize;
    while i < WIN_TABLE.len() {
        if let Some(cell) = WIN_TABLE.get(i) {
            if cell.win == win || cell.alt == win {
                return Some(cell.errno);
            }
        }
        i += 1;
    }
    None
}

/// 单格回读：Absent 格显性回 None，其余按 errno 反查主码。
pub fn probe_win(cell: &WinCell) -> Option<u32> {
    if cell.class == DiffClass::Absent {
        return None;
    }
    win_of(cell.errno)
}

/// 单格可观测判定：回读形态与该格分类必须严格匹配。
pub fn observable(cell: &WinCell) -> bool {
    match probe_win(cell) {
        Some(w) => cell.class != DiffClass::Absent && cell.win != 0 && w == cell.win,
        None => cell.class == DiffClass::Absent && cell.win == 0,
    }
}

/// 按分类计数。
pub fn class_count(class: DiffClass) -> u32 {
    let mut n = 0u32;
    for cell in WIN_TABLE.iter() {
        if cell.class == class {
            n = n.saturating_add(1);
        }
    }
    n
}

/// 可观测格数（联测面观测量的真源）。
pub fn observable_count() -> u32 {
    let mut n = 0u32;
    for cell in WIN_TABLE.iter() {
        if observable(cell) {
            n = n.saturating_add(1);
        }
    }
    n
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 逐字节喂入 u32（小端，定长 4 字节）。
fn feed_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 差异条目冻结指纹：errno + 主码 + 副码 + 分类编码。
pub fn diff_fp(errno: i32, win: u32, alt: u32, class: DiffClass) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    feed_u32(&mut bytes, errno as u32);
    feed_u32(&mut bytes, win);
    feed_u32(&mut bytes, alt);
    bytes.push(class.code());
    fnv1a(&bytes)
}

// ── 差异账 ────────────────────────────────────────────────────────────
/// 差异账条目（append-only，登记即冻结指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    pub used: bool,
    pub errno: i32,
    pub win: u32,
    pub alt: u32,
    pub class: DiffClass,
    pub fp: u32,
}

impl DiffEntry {
    /// 空槽常量（`[EMPTY; N]` 要求 Copy）。
    pub const EMPTY: DiffEntry = DiffEntry {
        used: false, errno: 0, win: 0, alt: 0, class: DiffClass::Exact, fp: 0,
    };
}

/// 定容差异账：12 槽 + 单调 reject 计数。
#[derive(Clone, Copy, Debug)]
pub struct DiffLedger {
    entries: [DiffEntry; DIFF_CAP],
    count: u32,
    rejected: u32,
}

impl DiffLedger {
    /// 空账。
    pub const fn new() -> DiffLedger {
        DiffLedger { entries: [DiffEntry::EMPTY; DIFF_CAP], count: 0, rejected: 0 }
    }
    /// 已登记条数。
    pub fn count(&self) -> u32 { self.count }
    /// 被拒笔数（重号 / Exact 误入 / 满账）。
    pub fn rejected(&self) -> u32 { self.rejected }
    /// 取条目视图（空槽视同不存在）。
    pub fn entry(&self, idx: usize) -> Option<&DiffEntry> {
        match self.entries.get(idx) {
            Some(e) => if e.used { Some(e) } else { None },
            None => None,
        }
    }
    /// 查重：同 (errno, win) 只认一条。
    pub fn find(&self, errno: i32, win: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < DIFF_CAP {
            if let Some(e) = self.entries.get(i) {
                if e.used && e.errno == errno && e.win == win {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 登记一条差异。只收非 Exact 格。
    ///
    /// 判定次序：分类（EINVAL）→ 重号（EEXIST）→ 容量（ENOSPC）。次序不可
    /// 交换：Exact 误入必须报分类错误，不能被"重号"掩盖。
    pub fn register(&mut self, cell: &WinCell) -> Result<usize, i32> {
        if cell.class == DiffClass::Exact {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EINVAL);
        }
        if self.find(cell.errno, cell.win).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EEXIST);
        }
        let mut i = 0usize;
        while i < DIFF_CAP {
            let free = match self.entries.get(i) {
                Some(e) => !e.used,
                None => false,
            };
            if free {
                let fp = diff_fp(cell.errno, cell.win, cell.alt, cell.class);
                if let Some(slot) = self.entries.get_mut(i) {
                    slot.used = true;
                    slot.errno = cell.errno;
                    slot.win = cell.win;
                    slot.alt = cell.alt;
                    slot.class = cell.class;
                    slot.fp = fp;
                }
                self.count = self.count.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        self.rejected = self.rejected.saturating_add(1);
        Err(ENOSPC)
    }
    /// 复算指纹并与冻结值比对（只读，不改账）。
    pub fn reseal(&self, idx: usize) -> Result<u32, i32> {
        let e = match self.entry(idx) {
            Some(v) => *v,
            None => return Err(ENOENT),
        };
        let now = diff_fp(e.errno, e.win, e.alt, e.class);
        if now != e.fp {
            return Err(EINVAL);
        }
        Ok(now)
    }
}

// ── 复用格 ────────────────────────────────────────────────────────────
/// 一条复用格：指向差异账下标 + 期望是否放行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseCell {
    pub diff_idx: usize,
    pub expect_ok: bool,
}

/// 6 条复用格：前 4 条指向可复用分类，后 2 条指向 Absent。
pub const REUSE_GRID: [ReuseCell; REUSE_CELLS] = [
    ReuseCell { diff_idx: 0, expect_ok: true },
    ReuseCell { diff_idx: 1, expect_ok: true },
    ReuseCell { diff_idx: 3, expect_ok: true },
    ReuseCell { diff_idx: 4, expect_ok: true },
    ReuseCell { diff_idx: 2, expect_ok: false },
    ReuseCell { diff_idx: 6, expect_ok: false },
];

/// 按给定指纹复用：指纹必须与冻结值相等。
pub fn reuse_with_fp(ledger: &DiffLedger, idx: usize, fp: u32) -> Result<u32, i32> {
    let e = match ledger.entry(idx) {
        Some(v) => *v,
        None => return Err(ENOENT),
    };
    if !e.class.reusable() {
        return Err(ENOSYS);
    }
    let now = diff_fp(e.errno, e.win, e.alt, e.class);
    if now != e.fp || now != fp {
        return Err(EINVAL);
    }
    Ok(now)
}

/// 复用一格（用账内冻结指纹）。
pub fn reuse_one(ledger: &DiffLedger, idx: usize) -> Result<u32, i32> {
    let fp = match ledger.entry(idx) {
        Some(e) => e.fp,
        None => return Err(ENOENT),
    };
    reuse_with_fp(ledger, idx, fp)
}

/// 复用格复算结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseReport {
    pub hits: u32,
    pub denied: u32,
    pub drift: u32,
    pub mismatched: u32,
}

/// 逐格复算复用面。
pub fn run_reuse_grid(ledger: &DiffLedger) -> ReuseReport {
    let mut r = ReuseReport { hits: 0, denied: 0, drift: 0, mismatched: 0 };
    for cell in REUSE_GRID.iter() {
        match reuse_one(ledger, cell.diff_idx) {
            Ok(_) => {
                r.hits = r.hits.saturating_add(1);
                if !cell.expect_ok {
                    r.mismatched = r.mismatched.saturating_add(1);
                }
            }
            Err(c) => {
                if c == ENOSYS {
                    r.denied = r.denied.saturating_add(1);
                } else if c == EINVAL {
                    r.drift = r.drift.saturating_add(1);
                }
                if cell.expect_ok {
                    r.mismatched = r.mismatched.saturating_add(1);
                }
            }
        }
    }
    r
}

// ── 联测面（14 步冻结脚本） ──────────────────────────────────────────
/// 联测步类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepKind {
    Probe, // 对照格可观测回读
    Diff,  // 差异账条目复算
    Reuse, // 复用格复算
}

/// 一步联测：类型 + 表内下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointStep {
    pub kind: StepKind,
    pub arg: u8,
}

/// 14 步冻结脚本：三类步交错，且 Probe 与 Reuse 都覆盖 Absent 行。
pub const JOINT_SCRIPT: [JointStep; JOINT_STEPS] = [
    JointStep { kind: StepKind::Probe, arg: 0 },
    JointStep { kind: StepKind::Probe, arg: 2 },
    JointStep { kind: StepKind::Diff, arg: 0 },
    JointStep { kind: StepKind::Reuse, arg: 0 },
    JointStep { kind: StepKind::Probe, arg: 9 },
    JointStep { kind: StepKind::Diff, arg: 3 },
    JointStep { kind: StepKind::Reuse, arg: 2 },
    JointStep { kind: StepKind::Probe, arg: 5 },
    JointStep { kind: StepKind::Reuse, arg: 4 },
    JointStep { kind: StepKind::Diff, arg: 8 },
    JointStep { kind: StepKind::Probe, arg: 17 },
    JointStep { kind: StepKind::Reuse, arg: 1 },
    JointStep { kind: StepKind::Diff, arg: 6 },
    JointStep { kind: StepKind::Probe, arg: 11 },
];

/// 单向前进的联测游标：只认「下一步」，重跑与跳步都拒。
#[derive(Clone, Copy, Debug)]
pub struct JointRunner {
    next: usize,
    executed: u16,
    rejects: u32,
}

impl JointRunner {
    /// 空游标。
    pub const fn new() -> JointRunner { JointRunner { next: 0, executed: 0, rejects: 0 } }
    /// 已执行步数。
    pub fn executed(&self) -> u32 { self.executed.count_ones() }
    /// 被拒步数。
    pub fn rejects(&self) -> u32 { self.rejects }
    /// 推进一步：`idx` 必须等于游标，否则 EINVAL；同一步二次推进 EEXIST。
    pub fn step(&mut self, idx: usize) -> Result<(), i32> {
        if idx != self.next || idx >= JOINT_STEPS {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if let Some(bit) = 1u16.checked_shl(idx as u32) {
            if self.executed & bit != 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(EEXIST);
            }
            self.executed |= bit;
        }
        self.next = self.next.saturating_add(1);
        Ok(())
    }
}

/// 联测面报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointReport {
    pub registered: u32,
    pub steps: u32,
    pub ordered: bool,
    pub observed: u32,
    pub diffs: u32,
    pub reuse_hits: u32,
    pub reuse_denied: u32,
}

/// 跑一遍联测面（先立差异账，再按脚本单向前进）。
pub fn run_joint_surface() -> JointReport {
    let mut ledger = DiffLedger::new();
    let mut registered = 0u32;
    for cell in WIN_TABLE.iter() {
        if cell.class != DiffClass::Exact {
            match ledger.register(cell) {
                Ok(_) => registered = registered.saturating_add(1),
                Err(_) => {}
            }
        }
    }
    let mut runner = JointRunner::new();
    let mut report = JointReport {
        registered, steps: 0, ordered: true, observed: 0, diffs: 0,
        reuse_hits: 0, reuse_denied: 0,
    };
    let mut i = 0usize;
    while i < JOINT_SCRIPT.len() {
        if let Some(step) = JOINT_SCRIPT.get(i) {
            if runner.step(i).is_err() {
                report.ordered = false;
            }
            match step.kind {
                StepKind::Probe => {
                    if let Some(cell) = WIN_TABLE.get(step.arg as usize) {
                        if observable(cell) {
                            report.observed = report.observed.saturating_add(1);
                        }
                    }
                }
                StepKind::Diff => {
                    if ledger.reseal(step.arg as usize).is_ok() {
                        report.diffs = report.diffs.saturating_add(1);
                    }
                }
                StepKind::Reuse => {
                    if let Some(cell) = REUSE_GRID.get(step.arg as usize) {
                        match reuse_one(&ledger, cell.diff_idx) {
                            Ok(_) => report.reuse_hits = report.reuse_hits.saturating_add(1),
                            Err(_) => report.reuse_denied = report.reuse_denied.saturating_add(1),
                        }
                    }
                }
            }
            report.steps = report.steps.saturating_add(1);
        }
        i += 1;
    }
    report
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger { InjectLedger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔注入；守恒破坏回 EINVAL。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }
    /// 守恒：检出 + 逃逸 == 注入。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类注入各 10 轮。
///
/// 类 0：篡改 Win 主码 → 该格必须变成不可观测；
/// 类 1：差异账同 (errno,win) 二次登记 → 必须 EEXIST；
/// 类 2：复用指纹陈旧 → 必须 EINVAL（不能算正常复用）；
/// 类 3：联测跳步/重跑 → 必须 EINVAL。
pub fn run_injection() -> InjectLedger {
    let mut ledger = InjectLedger::zero();
    let mut base = DiffLedger::new();
    for cell in WIN_TABLE.iter() {
        if cell.class != DiffClass::Exact {
            match base.register(cell) {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let tampered = WinCell {
            op: "tamper", errno: EBADF, lname: "EBADF",
            win: WIN_INVALID_HANDLE + 1, wname: "TAMPER", alt: 0,
            class: DiffClass::Exact,
        };
        let caught0 = !observable(&tampered);
        match ledger.record(caught0) { Ok(_) => {} Err(_) => {} }

        let mut dup = DiffLedger::new();
        let mut first_ok = false;
        if let Some(cell) = WIN_TABLE.get(2) {
            first_ok = dup.register(cell).is_ok();
        }
        let mut second_dup = false;
        if let Some(cell) = WIN_TABLE.get(2) {
            second_dup = dup.register(cell) == Err(EEXIST);
        }
        let caught1 = first_ok && second_dup && dup.count() == 1;
        match ledger.record(caught1) { Ok(_) => {} Err(_) => {} }

        let mut stale = false;
        if let Some(entry) = base.entry(0) {
            stale = reuse_with_fp(&base, 0, entry.fp ^ 0x5A5A_A5A5) == Err(EINVAL);
        }
        match ledger.record(stale) { Ok(_) => {} Err(_) => {} }

        let mut runner = JointRunner::new();
        let skip = runner.step(1) == Err(EINVAL);
        let first = runner.step(0).is_ok();
        let again = runner.step(0) == Err(EINVAL);
        match ledger.record(skip && first && again) { Ok(_) => {} Err(_) => {} }

        round = round.saturating_add(1);
    }
    ledger
}

// ── 写回面 ────────────────────────────────────────────────────────────
/// 写回状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbStatus {
    Pending,
    Done,
}

/// 写回记录：状态 + 结果摘要长度。
#[derive(Clone, Copy, Debug)]
pub struct Writeback {
    status: WbStatus,
    summary_len: u8,
}

impl Writeback {
    /// 待写回。
    pub const fn new() -> Writeback { Writeback { status: WbStatus::Pending, summary_len: 0 } }
    /// 当前状态。
    pub fn status(&self) -> WbStatus { self.status }
    /// 摘要长度。
    pub fn summary_len(&self) -> u8 { self.summary_len }
    /// 写回完成态：空摘要 EINVAL，重复写回 EBUSY。
    pub fn complete(&mut self, summary: &str) -> Result<(), i32> {
        if self.status == WbStatus::Done {
            return Err(EBUSY);
        }
        if summary.is_empty() {
            return Err(EINVAL);
        }
        let n = summary.len();
        self.summary_len = if n > 255 { 255 } else { n as u8 };
        self.status = WbStatus::Done;
        Ok(())
    }
}

// ── 规范语料指纹 ──────────────────────────────────────────────────────
/// 语料指纹：对照表 + 差异账指纹 + 复用/联测计数逐字节喂入。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    for cell in WIN_TABLE.iter() {
        feed_u32(&mut bytes, cell.errno as u32);
        feed_u32(&mut bytes, cell.win);
        feed_u32(&mut bytes, cell.alt);
        bytes.push(cell.class.code());
    }
    let mut ledger = DiffLedger::new();
    for cell in WIN_TABLE.iter() {
        if cell.class != DiffClass::Exact {
            match ledger.register(cell) {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }
    let mut i = 0usize;
    while i < DIFF_CAP {
        if let Some(e) = ledger.entry(i) {
            feed_u32(&mut bytes, e.fp);
        }
        i += 1;
    }
    let reuse = run_reuse_grid(&ledger);
    feed_u32(&mut bytes, reuse.hits);
    feed_u32(&mut bytes, reuse.denied);
    feed_u32(&mut bytes, reuse.drift);
    let joint = run_joint_surface();
    feed_u32(&mut bytes, joint.steps);
    feed_u32(&mut bytes, joint.observed);
    feed_u32(&mut bytes, joint.diffs);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9377 判据集（add 16 + add_dyn 3）。
pub fn run_unx9377_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9377");

    let seg_ok = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0
        && SEG_BASE > 0x7CB80 && SEG_LIMIT == SEG_BASE + 0x40;
    s.add("段位守卫：0x7CBC0 独占 0x40 窗口", seg_ok,
          "段基址 0x40 对齐、落在 0x7A000..=0x7D1C0，且窗口不与邻段重叠");

    let mut names_ok = WIN_TABLE.len() == TABLE_ROWS;
    for cell in WIN_TABLE.iter() {
        match errno_name(cell.errno) {
            Some(n) => if n != cell.lname { names_ok = false; },
            None => names_ok = false,
        }
    }
    s.add("对照表：18 格 errno 与数值表逐格同名", names_ok,
          "errno 名称必须与冻结数值表一致，表外 errno 不得入格");

    let exact = class_count(DiffClass::Exact);
    let split = class_count(DiffClass::Split);
    let approx = class_count(DiffClass::Approx);
    let merged = class_count(DiffClass::Merged);
    let absent = class_count(DiffClass::Absent);
    let partition_ok = exact == 9 && split == 2 && approx == 4 && merged == 1
        && absent == 2 && exact + split + approx + merged + absent == TABLE_ROWS as u32;
    s.add("分类分区：Exact9/Split2/Approx4/Merged1/Absent2", partition_ok,
          "五类计数之和必须等于表长 18，不允许未分类格");

    s.add("可观测：18/18 对照格回读一致", observable_count() == TABLE_ROWS as u32,
          "Absent 格必须显性回 None，其余格回读码必须等于冻结码");

    let mut rt_ok = true;
    for cell in WIN_TABLE.iter() {
        if cell.win != 0 && (win_of(cell.errno) != Some(cell.win) || errno_of(cell.win).is_none()) {
            rt_ok = false;
        }
    }
    s.add("双向映射：errno→Win 与 Win→errno 均可回读", rt_ok,
          "非 Absent 格两个方向都必须有落点，单向映射即断链");

    let mut split_ok = true;
    for cell in WIN_TABLE.iter() {
        match cell.class {
            DiffClass::Split => {
                if cell.alt == 0 || cell.alt == cell.win || errno_of(cell.alt) != Some(cell.errno) {
                    split_ok = false;
                }
            }
            _ => if cell.alt != 0 { split_ok = false; },
        }
    }
    s.add("Split 行：副码非零且回读同一 errno", split_ok,
          "ENOENT 的 FILE/PATH_NOT_FOUND 与 EEXIST 的 FILE/ALREADY_EXISTS 必须双码可回读");

    let mut ledger = DiffLedger::new();
    let mut reg = 0u32;
    for cell in WIN_TABLE.iter() {
        if cell.class != DiffClass::Exact {
            match ledger.register(cell) {
                Ok(_) => reg = reg.saturating_add(1),
                Err(_) => {}
            }
        }
    }
    let mut dup_ok = false;
    if let Some(cell) = WIN_TABLE.get(2) { dup_ok = ledger.register(cell) == Err(EEXIST); }
    let mut exact_ok = false;
    if let Some(cell) = WIN_TABLE.get(0) { exact_ok = ledger.register(cell) == Err(EINVAL); }
    let before = ledger.count();
    let mut later_rejected = false;
    if let Some(cell) = WIN_TABLE.get(7) { later_rejected = ledger.register(cell) == Err(EINVAL); }
    // 本格共显性拒绝三笔：重号（cell2→EEXIST）、Exact 误入（cell0→EINVAL）、
    // Exact 误入（cell7→EINVAL）；rejects 是单调累计计数，故判 3 而非 2。
    s.add("差异账：9 条登记、重号 EEXIST、Exact 误入 EINVAL",
          reg == 9 && ledger.count() == 9 && dup_ok && exact_ok && ledger.rejected() == 3
              && later_rejected && ledger.count() == before,
          "差异账只收非 Exact；二次登记/误入都显性拒绝且失败路径不占槽");

    let reuse = run_reuse_grid(&ledger);
    s.add("复用格：4 放行 + 2 拒绝 + 0 漂移",
          reuse.hits == 4 && reuse.denied == 2 && reuse.drift == 0 && reuse.mismatched == 0
              && reuse.hits + reuse.denied == REUSE_CELLS as u32,
          "Absent 条目复用必须回 ENOSYS，复用账合计必须等于 6 格");

    let mut drift_ok = false;
    if let Some(entry) = ledger.entry(0) {
        drift_ok = reuse_with_fp(&ledger, 0, entry.fp ^ 0x1234_5678) == Err(EINVAL);
    }
    s.add("复用指纹：陈旧指纹必须 EINVAL", drift_ok,
          "冻结指纹与重算不一致时不得放行，避免复用已漂移的差异结论");

    let joint = run_joint_surface();
    s.add("联测面：14 步按序跑通，6 Probe/4 Diff/4 Reuse",
          joint.steps == JOINT_STEPS as u32 && joint.ordered && joint.registered == 9
              && joint.observed == 6 && joint.diffs == 4
              && joint.reuse_hits == 3 && joint.reuse_denied == 1,
          "脚本必须单向前进一次跑通，三类型步数与冻结值一致");

    let mut runner = JointRunner::new();
    let out_of_order = runner.step(3) == Err(EINVAL);
    let first_ok = runner.step(0).is_ok();
    let replay = runner.step(0) == Err(EINVAL);
    s.add("顺序：越序与重跑均 EINVAL，首步放行",
          out_of_order && first_ok && replay && runner.rejects() == 2,
          "游标只认下一步；跳步与重跑必须拒绝且计入 rejects");

    let mut wb = Writeback::new();
    let mut wb_ok = wb.complete("windows 对照账 18/18 可观测，差异 9 条，复用 6 格").is_ok();
    if wb.status() != WbStatus::Done || wb.summary_len() == 0 { wb_ok = false; }
    if wb.complete("二次写回").err() != Some(EBUSY) { wb_ok = false; }
    if Writeback::new().complete("").err() != Some(EINVAL) { wb_ok = false; }
    s.add("写回：Done + 摘要非空；重复 EBUSY、空摘要 EINVAL", wb_ok,
          "完成态必须携带非空摘要，且不允许覆盖已封账结果");

    let inj = run_injection();
    s.add("注入：四类 ×10 全检出，逃逸 0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == INJECT_TOTAL && inj.detected == INJECT_TOTAL,
          "篡改码/重号/陈旧指纹/越序四类各 10 轮必须 40/40 检出");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：对照账语料指纹相等", fp_a == fp_b,
          "同输入两次指纹必须逐字节相等，隐藏状态即失败");

    let mut uniq = true;
    let mut i = 0usize;
    while i < WinCode::ALL.len() {
        let mut j = i + 1;
        while j < WinCode::ALL.len() {
            if let (Some(a), Some(b)) = (WinCode::ALL.get(i), WinCode::ALL.get(j)) {
                if a.val() == b.val() || a.code() == b.code() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    let mut text_ok = true;
    for c in WinCode::ALL.iter() {
        if c.detail().is_empty() { text_ok = false; }
    }
    s.add("诊断码：14 个码值与序号两两唯一且文案非空",
          uniq && WinCode::ALL.len() == 14 && text_ok,
          "段内闭集码不得重复，且每码必须能取到非空可操作说明");

    let mut field_ok = true;
    for cell in WIN_TABLE.iter() {
        if cell.op.is_empty() || cell.lname.is_empty() || cell.wname.is_empty() {
            field_ok = false;
        }
        if cell.class != DiffClass::Absent && (cell.win == 0 || cell.wname == "NONE") {
            field_ok = false;
        }
    }
    s.add("格字段：op/lname/wname 非空，非 Absent 必有实码", field_ok,
          "对照格缺字段会让联测无法定位，Absent 以外的格不得留空码");

    let diff_text = format!("差异账 {} 条 / 拒绝 {} 笔；复用 {}/{}/漂移 {}",
        ledger.count(), ledger.rejected(), reuse.hits, reuse.denied, reuse.drift);
    s.add_dyn("差异账快照", ledger.count() == 9, &diff_text);

    let joint_text = format!("联测 {} 步，观测 {}，差异复算 {}，复用 {}+{}",
        joint.steps, joint.observed, joint.diffs, joint.reuse_hits, joint.reuse_denied);
    s.add_dyn("联测面快照", joint.ordered && joint.steps == 14, &joint_text);

    let fp_text = format!("对照账指纹 0x{:08X}，注入 {}/{}", fp_a, inj.detected, inj.injected);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_window_is_fixed() {
        assert_eq!(SEG_BASE, 0x7CBC0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_LIMIT, 0x7CC00);
    }

    #[test]
    fn table_partition_and_names() {
        let mut names = 0u32;
        for c in WIN_TABLE.iter() {
            if errno_name(c.errno) == Some(c.lname) { names = names + 1; }
        }
        assert_eq!(names, TABLE_ROWS as u32);
        assert_eq!(class_count(DiffClass::Exact), 9);
        assert_eq!(class_count(DiffClass::Split), 2);
        assert_eq!(class_count(DiffClass::Approx), 4);
        assert_eq!(class_count(DiffClass::Merged), 1);
        assert_eq!(class_count(DiffClass::Absent), 2);
    }

    #[test]
    fn every_cell_is_observable() {
        assert_eq!(observable_count(), 18);
        for c in WIN_TABLE.iter() { assert_eq!(observable(c), true); }
    }

    #[test]
    fn roundtrip_and_split_codes() {
        assert_eq!(win_of(EBADF), Some(WIN_INVALID_HANDLE));
        assert_eq!(errno_of(WIN_PATH_NOT_FOUND), Some(ENOENT));
        assert_eq!(win_of(ENOTDIR), None);
        assert_eq!(errno_of(0), None);
        let mut split = 0u32;
        for c in WIN_TABLE.iter() {
            if c.class == DiffClass::Split && errno_of(c.alt) == Some(c.errno) { split = split + 1; }
        }
        assert_eq!(split, 2);
        assert_eq!(DiffClass::Absent.reusable(), false);
    }

    #[test]
    fn diff_ledger_rejects_duplicates() {
        let mut l = DiffLedger::new();
        for c in WIN_TABLE.iter() {
            if c.class != DiffClass::Exact { assert_eq!(l.register(c).is_ok(), true); }
        }
        if let Some(c) = WIN_TABLE.get(2) { assert_eq!(l.register(c), Err(EEXIST)); }
        if let Some(c) = WIN_TABLE.get(0) { assert_eq!(l.register(c), Err(EINVAL)); }
        assert_eq!(l.count(), 9);
        assert_eq!(l.rejected(), 2);
    }

    #[test]
    fn reuse_grid_and_stale_fingerprint() {
        let mut l = DiffLedger::new();
        for c in WIN_TABLE.iter() {
            if c.class != DiffClass::Exact { let _slot = l.register(c); }
        }
        let r = run_reuse_grid(&l);
        assert_eq!(r.hits, 4);
        assert_eq!(r.denied, 2);
        assert_eq!(r.drift + r.mismatched, 0);
        if let Some(e) = l.entry(0) {
            assert_eq!(reuse_with_fp(&l, 0, e.fp ^ 1), Err(EINVAL));
            assert_eq!(reuse_with_fp(&l, 0, e.fp).is_ok(), true);
        }
    }

    #[test]
    fn joint_runner_enforces_order() {
        let mut r = JointRunner::new();
        assert_eq!(r.step(1), Err(EINVAL));
        assert_eq!(r.step(0).is_ok(), true);
        assert_eq!(r.step(0), Err(EINVAL));
        assert_eq!(r.executed(), 1);
        assert_eq!(r.rejects(), 2);
    }

    #[test]
    fn joint_surface_matches_frozen_counts() {
        let j = run_joint_surface();
        assert_eq!(j.steps, 14);
        assert_eq!(j.ordered, true);
        assert_eq!(j.registered, 9);
        assert_eq!(j.observed, 6);
        assert_eq!(j.diffs, 4);
        assert_eq!(j.reuse_hits, 3);
        assert_eq!(j.reuse_denied, 1);
    }

    #[test]
    fn writeback_is_single_shot() {
        let mut wb = Writeback::new();
        assert_eq!(wb.complete("18 格可观测"), Ok(()));
        assert_eq!(wb.status(), WbStatus::Done);
        assert_eq!(wb.summary_len() > 0, true);
        assert_eq!(wb.complete("again"), Err(EBUSY));
        assert_eq!(Writeback::new().complete(""), Err(EINVAL));
    }

    #[test]
    fn injection_and_fingerprint() {
        let l = run_injection();
        assert_eq!(l.conserved(), true);
        assert_eq!(l.injected, 40);
        assert_eq!(l.detected, 40);
        assert_eq!(l.escaped, 0);
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9377_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
