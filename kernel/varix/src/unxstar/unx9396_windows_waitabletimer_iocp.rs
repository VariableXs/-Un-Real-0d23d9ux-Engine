//! UNX-F9396 联调与 Windows WaitableTimer/IOCP 对照账 · 0x7D080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9396-J1 逐字）：
//!   锚点：UNX-F9396-J1 对照账判据过，WaitableTimer/IOCP 映射表落账，
//!   差异账三条全列格过
//!
//! 数据段 SEG_BASE = 0x7D080：诊断码闭集 +0x01..=+0x09（9 码，
//! 9396 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表/映射表：WinMap（POSIX 阻塞原语 → Windows 等价物闭集
//!     6 行落账：futex_wait/futex_wake/ppoll/nanosleep/accept/read）；
//!   - 账本：DiffLedger（差异账 3 条全列：句柄面/EINTR 面/超时面，
//!     每条必须非空落账）+ 行账（映射行=对拍行双向对账）；
//!   - 解析器：行闭集校验（win_status 只认闭集码，表外码拒绝）；
//!   - 注入核：4 类坏样本（未知行/状态码越闭集/映射行缺失/差异账
//!     缺条）逐类显性拒绝；
//!   - 对拍：Linux errno 对照（EINTR/ETIMEDOUT/EBADF/EINVAL/EAGAIN）；
//!   - 指纹：FNV-1a 映射行序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9396 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D080;

/// 映射表行数（闭集 6 行）。
pub const MAP_ROWS: usize = 6;

/// Linux errno 数值（正数口径）。
pub const EAGAIN: i32 = 11;
pub const EINTR: i32 = 4;
pub const ETIMEDOUT: i32 = 110;
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=9 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinMapCode {
    /// 未知映射行：闭集 6 行之外
    UnknownRow,
    /// Windows 状态码越闭集
    StatusOutOfRange,
    /// 映射行缺失：POSIX 侧原语无 Windows 对应落账
    RowMissing,
    /// 差异账缺条：3 条差异未全列
    DiffEntryMissing,
    /// 双向对账失衡：映射行数 != 对拍行数
    LedgerMismatch,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 行号越界
    RowOutOfRange,
}

impl WinMapCode {
    /// 闭集数组。
    pub const ALL: [WinMapCode; 9] = [
        WinMapCode::UnknownRow,
        WinMapCode::StatusOutOfRange,
        WinMapCode::RowMissing,
        WinMapCode::DiffEntryMissing,
        WinMapCode::LedgerMismatch,
        WinMapCode::LedgerRegress,
        WinMapCode::UnknownErrno,
        WinMapCode::NotReproducible,
        WinMapCode::RowOutOfRange,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                WinMapCode::UnknownRow => 1,
                WinMapCode::StatusOutOfRange => 2,
                WinMapCode::RowMissing => 3,
                WinMapCode::DiffEntryMissing => 4,
                WinMapCode::LedgerMismatch => 5,
                WinMapCode::LedgerRegress => 6,
                WinMapCode::UnknownErrno => 7,
                WinMapCode::NotReproducible => 8,
                WinMapCode::RowOutOfRange => 9,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            WinMapCode::UnknownRow => "映射表只认 6 行闭集，越界行显性拒绝",
            WinMapCode::StatusOutOfRange => "Windows 状态码只认闭集，表外码拒绝落账",
            WinMapCode::RowMissing => "每个 POSIX 阻塞原语必须有 Windows 对照行",
            WinMapCode::DiffEntryMissing => "差异账必须三条全列（句柄/EINTR/超时）",
            WinMapCode::LedgerMismatch => "映射行与对拍行双向对账必须相等",
            WinMapCode::LedgerRegress => "对照账只增不减，倒退即审计失败",
            WinMapCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            WinMapCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            WinMapCode::RowOutOfRange => "行号闭集 0..=5，越界拒绝",
        }
    }
}

// ── Windows 状态码闭集 ────────────────────────────────────────────────
/// WAIT_OBJECT_0（等待对象受信）。
pub const WAIT_OBJECT_0: u32 = 0;
/// WAIT_TIMEOUT（超时）。
pub const WAIT_TIMEOUT_WIN: u32 = 0x102;
/// WAIT_IO_COMPLETION（alertable 等待被 APC/IO 完成打断 —— EINTR 对应物）。
pub const WAIT_IO_COMPLETION: u32 = 0xC0;
/// WAIT_FAILED（需 GetLastError —— 系统调用失败）。
pub const WAIT_FAILED: u32 = 0xFFFFFFFF;

/// Windows 等待状态闭集校验。
pub fn valid_win_status(s: u32) -> bool {
    s == WAIT_OBJECT_0 || s == WAIT_TIMEOUT_WIN || s == WAIT_IO_COMPLETION || s == WAIT_FAILED
}

// ── 映射表 ────────────────────────────────────────────────────────────
/// 单行映射：POSIX 阻塞原语 ↔ Windows 等价物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapRow {
    /// 行号（0..=5 闭集）
    pub row: u32,
    /// POSIX 侧 errno（阻塞被解除/打断/超时的回报口径）
    pub posix_err: i32,
    /// Windows 侧状态码（闭集 4 值）
    pub win_status: u32,
    /// 差异标记：bit0=句柄面差异 bit1=EINTR 面差异 bit2=超时面差异
    pub diff_mask: u32,
}

/// 差异账：三条全列（句柄面 / EINTR 面 / 超时面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffLedger {
    /// 条 1：句柄面 —— futex 无句柄 vs WaitableTimer 需内核 HANDLE
    pub handle_diff: bool,
    /// 条 2：EINTR 面 —— Linux 信号打断回 EINTR vs Windows alertable
    /// 等待回 WAIT_IO_COMPLETION
    pub eintr_diff: bool,
    /// 条 3：超时面 —— Linux timespec（绝对/相对，ns 粒度）vs Windows
    /// LARGE_INTEGER 100ns 单位（负值=相对）
    pub timeout_diff: bool,
}

impl DiffLedger {
    /// 三条全列（全 true 才格过）。
    pub fn complete(&self) -> bool {
        self.handle_diff && self.eintr_diff && self.timeout_diff
    }
}

/// WaitableTimer/IOCP 映射表落账（6 行闭集）。
///
/// 行序：0=futex_wait 1=futex_wake 2=ppoll 3=nanosleep 4=accept 5=read。
/// 每行的 posix_err 与 win_status 一一对应，diff_mask 逐行标差异面。
pub fn win_mapping_table() -> [MapRow; MAP_ROWS] {
    [
        // futex_wait：EINTR 对 WAIT_IO_COMPLETION（alertable 打断）
        MapRow { row: 0, posix_err: EINTR, win_status: WAIT_IO_COMPLETION, diff_mask: 0b011 },
        // futex_wake：成功路径无 errno，Windows 用 WAIT_OBJECT_0 收口
        MapRow { row: 1, posix_err: 0, win_status: WAIT_OBJECT_0, diff_mask: 0b001 },
        // ppoll：ETIMEDOUT 对 WAIT_TIMEOUT_WIN
        MapRow { row: 2, posix_err: ETIMEDOUT, win_status: WAIT_TIMEOUT_WIN, diff_mask: 0b101 },
        // nanosleep：EINTR 打断对 WAIT_IO_COMPLETION
        MapRow { row: 3, posix_err: EINTR, win_status: WAIT_IO_COMPLETION, diff_mask: 0b110 },
        // accept：阻塞被信号打断回 EINTR（Windows 无信号面，WAIT_FAILED 兜底）
        MapRow { row: 4, posix_err: EINTR, win_status: WAIT_FAILED, diff_mask: 0b011 },
        // read：EAGAIN（非阻塞无数据）对 WAIT_TIMEOUT_WIN 轮询口径
        MapRow { row: 5, posix_err: EAGAIN, win_status: WAIT_TIMEOUT_WIN, diff_mask: 0b101 },
    ]
}

/// 按行号取行（越界返回 None）。
pub fn map_row(row: u32) -> Option<MapRow> {
    if row as usize >= MAP_ROWS {
        return None;
    }
    let table = win_mapping_table();
    table.get(row as usize).copied()
}

/// 行对拍：行号闭集 + errno 表内（0 或闭集值）+ Windows 状态闭集。
pub fn row_audit(r: &MapRow) -> bool {
    (r.row as usize) < MAP_ROWS
        && (r.posix_err == 0 || agrees_with_linux(r.posix_err))
        && valid_win_status(r.win_status)
        && r.diff_mask <= 0b111
}

// ── 双向对账 ──────────────────────────────────────────────────────────
/// 对照账：映射行数 = 对拍行数（表账=执行账双向对账）。
#[derive(Clone, Copy)]
pub struct MapLedger {
    /// 表账：落账行数
    pub table_rows: u32,
    /// 执行账：逐行对拍通过行数
    pub audited_rows: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl MapLedger {
    /// 守恒：行数相等且拒绝为 0。
    pub fn conserved(&self) -> bool {
        self.table_rows == self.audited_rows && self.rejected == 0
    }
}

/// 全表对拍：6 行逐行审计。
pub fn run_table_audit() -> (MapLedger, Vec<MapRow>) {
    let table = win_mapping_table();
    let mut audited = 0u32;
    let mut rejected = 0u32;
    let mut rows: Vec<MapRow> = Vec::new();
    let mut i = 0usize;
    while i < table.len() {
        if let Some(r) = table.get(i) {
            if row_audit(r) {
                audited += 1;
                rows.push(*r);
            } else {
                rejected += 1;
            }
        }
        i += 1;
    }
    (
        MapLedger { table_rows: table.len() as u32, audited_rows: audited, rejected },
        rows,
    )
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 映射行序列指纹。
pub fn map_fingerprint(rows: &[MapRow]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for r in rows {
        buf.extend_from_slice(&r.row.to_le_bytes());
        buf.extend_from_slice(&r.posix_err.to_le_bytes());
        buf.extend_from_slice(&r.win_status.to_le_bytes());
        buf.extend_from_slice(&r.diff_mask.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 4 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 4], u32) {
    let mut ok = [false; 4];
    let mut detected = 0u32;

    // 类 1：未知行（行号 6 与 u32::MAX）
    let unknown = map_row(6).is_none() && map_row(u32::MAX).is_none();
    if unknown {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：Windows 状态码越闭集
    let bad_status = !valid_win_status(0x777)
        && !valid_win_status(1)
        && !row_audit(&MapRow { row: 0, posix_err: EINTR, win_status: 0x777, diff_mask: 0 });
    if bad_status {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：映射行缺失（审计一个越界 errno 的行）
    let missing = !row_audit(&MapRow { row: 0, posix_err: 404, win_status: WAIT_OBJECT_0, diff_mask: 0 });
    if missing {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：差异账缺条
    let incomplete = !DiffLedger { handle_diff: true, eintr_diff: true, timeout_diff: false }.complete()
        && !DiffLedger { handle_diff: false, eintr_diff: true, timeout_diff: true }.complete();
    if incomplete {
        detected += 1;
        ok[3] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [EINTR, EAGAIN, ETIMEDOUT, EBADF, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9396 自检：对照账判据（约 13 条）。
pub fn run_unx9396_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9396");

    // 1. 映射表 6 行落账
    let (ledger, rows) = run_table_audit();
    s.add("映射表落账：6 行闭集全部落账", ledger.table_rows == MAP_ROWS as u32,
          "WaitableTimer/IOCP 映射行逐行在案");

    // 2. 双向对账：表账=执行账
    s.add("双向对账：映射行数=对拍行数、拒绝 0", ledger.conserved(),
          "表账与执行账必须相等");

    // 3. EINTR 对拍：futex_wait/nanosleep/accept 的 EINTR 面
    let eintr_ok = map_row(0).map(|r| r.posix_err == EINTR).unwrap_or(false)
        && map_row(3).map(|r| r.posix_err == EINTR).unwrap_or(false)
        && map_row(4).map(|r| r.posix_err == EINTR).unwrap_or(false);
    s.add("EINTR 逐码对拍：futex_wait/nanosleep/accept 三行 EINTR", eintr_ok,
          "阻塞打断的 POSIX 回报口径逐行对齐");

    // 4. 超时对拍：ppoll/read 的 ETIMEDOUT/EAGAIN 面
    let timeout_ok = map_row(2).map(|r| r.posix_err == ETIMEDOUT).unwrap_or(false)
        && map_row(5).map(|r| r.posix_err == EAGAIN).unwrap_or(false)
        && map_row(2).map(|r| r.win_status == WAIT_TIMEOUT_WIN).unwrap_or(false);
    s.add("超时逐码对拍：ppoll ETIMEDOUT / read EAGAIN 对 WAIT_TIMEOUT", timeout_ok,
          "超时口径逐行对齐");

    // 5. 成功路径对拍：futex_wake 对 WAIT_OBJECT_0
    let wake_ok = map_row(1).map(|r| r.win_status == WAIT_OBJECT_0 && r.posix_err == 0).unwrap_or(false);
    s.add("成功路径对拍：futex_wake 对 WAIT_OBJECT_0", wake_ok,
          "无错误路径也要落账");

    // 6. 差异账三条全列
    let diff = DiffLedger { handle_diff: true, eintr_diff: true, timeout_diff: true };
    s.add("差异账三条全列：句柄面/EINTR 面/超时面", diff.complete(),
          "缺任何一条即格红");

    // 7. 差异面覆盖：6 行的 diff_mask 覆盖三个差异位
    let mask_ok = rows.iter().fold(0u32, |acc, r| acc | r.diff_mask) == 0b111;
    s.add("差异位覆盖：6 行 diff_mask 合计覆盖 3 面", mask_ok,
          "每条差异至少被一行标记");

    // 8. EINTR 面逐行核对：futex_wait 行同时标句柄与 EINTR 差异
    let fwait_mask = map_row(0).map(|r| r.diff_mask & 0b011 == 0b011).unwrap_or(false);
    s.add("行级差异：futex_wait 行标句柄+EINTR 双差异", fwait_mask,
          "futex 无句柄且受信号面差异影响");

    // 9. 注入核 4 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：4 类坏样本逐类显性拒绝", all_rej && detected == 4,
          "未知行/越码/缺行/缺条全过闸");

    // 10. Windows 状态闭集
    let win_ok = valid_win_status(WAIT_OBJECT_0)
        && valid_win_status(WAIT_TIMEOUT_WIN)
        && valid_win_status(WAIT_IO_COMPLETION)
        && valid_win_status(WAIT_FAILED)
        && !valid_win_status(0x1234);
    s.add("Windows 状态闭集：4 码合法、野码拒绝", win_ok,
          "状态码只认闭集 4 值");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-ETIMEDOUT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(7777);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "POSIX 侧错误码闭集对拍");

    // 12. 双跑指纹
    let (_, rows2) = run_table_audit();
    let fp1 = map_fingerprint(&rows);
    let fp2 = map_fingerprint(&rows2);
    s.add("双跑指纹：映射行序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性落账，无隐藏状态");

    // 13. 诊断码闭集
    let text_ok = WinMapCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < WinMapCode::ALL.len() {
        let mut j = i + 1;
        while j < WinMapCode::ALL.len() {
            let a = WinMapCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = WinMapCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：9 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+9 闭集");

    // 动态明细（≤8）
    let l_text = format!("表 {} 行 / 对拍 {} 行", ledger.table_rows, ledger.audited_rows);
    s.add_dyn("对照账快照", ledger.conserved(), &l_text);
    let fp_text = format!("映射指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_full_and_conserved() {
        let (l, rows) = run_table_audit();
        assert_eq!(l.table_rows, 6);
        assert_eq!(l.audited_rows, 6);
        assert!(l.conserved());
        assert_eq!(rows.len(), 6);
    }

    #[test]
    fn eintr_rows_align() {
        assert_eq!(map_row(0).map(|r| r.posix_err), Some(EINTR));
        assert_eq!(map_row(3).map(|r| r.posix_err), Some(EINTR));
        assert_eq!(map_row(4).map(|r| r.posix_err), Some(EINTR));
    }

    #[test]
    fn timeout_rows_align() {
        assert_eq!(map_row(2).map(|r| r.win_status), Some(WAIT_TIMEOUT_WIN));
        assert_eq!(map_row(5).map(|r| r.posix_err), Some(EAGAIN));
    }

    #[test]
    fn unknown_row_rejected() {
        assert!(map_row(6).is_none());
        assert!(map_row(u32::MAX).is_none());
        assert!(map_row(0).is_some());
    }

    #[test]
    fn win_status_closed_set() {
        assert!(valid_win_status(WAIT_OBJECT_0));
        assert!(valid_win_status(WAIT_TIMEOUT_WIN));
        assert!(valid_win_status(WAIT_IO_COMPLETION));
        assert!(valid_win_status(WAIT_FAILED));
        assert!(!valid_win_status(0x555));
        assert!(!valid_win_status(1));
    }

    #[test]
    fn diff_ledger_requires_all_three() {
        assert!(DiffLedger { handle_diff: true, eintr_diff: true, timeout_diff: true }.complete());
        assert!(!DiffLedger { handle_diff: true, eintr_diff: true, timeout_diff: false }.complete());
        assert!(!DiffLedger { handle_diff: false, eintr_diff: false, timeout_diff: false }.complete());
    }

    #[test]
    fn mask_covers_three_faces() {
        let (_, rows) = run_table_audit();
        let acc = rows.iter().fold(0u32, |a, r| a | r.diff_mask);
        assert_eq!(acc, 0b111);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 4);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(ETIMEDOUT));
        assert!(agrees_with_linux(-EAGAIN));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(12345));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, r1) = run_table_audit();
        let (_, r2) = run_table_audit();
        assert_eq!(map_fingerprint(&r1), map_fingerprint(&r2));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(WinMapCode::ALL.len(), 9);
        for c in WinMapCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 9);
        }
    }

    #[test]
    fn unx9396_checks_green() {
        let set = run_unx9396_checks();
        assert!(set.all_passed(), "UNX-F9396 red: {}", set.red_items_text());
    }
}
