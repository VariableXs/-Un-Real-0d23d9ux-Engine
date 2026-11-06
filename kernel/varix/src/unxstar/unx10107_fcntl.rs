//! UNX-F10107 fcntl 正反双判据（描述符控制达成与非法命令误用受控） · 0x92ac0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10107（fcntl 正反双判据：J1 F_GETFD/F_SETFD/F_GETFL 达成，
//!       J2 非法命令误用受控）。
//! 锚点：UNX-F10108-J1 ioctl 合法请求达成（LTP ioctl01 面请求分发正确
//!       10/10 次）；UNX-F10108-J2 非法请求误用受控（预期 ENOTTY 精确
//!       复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92ac0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `FcntlCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `CmdTable`：命令闭集（F_GETFD/F_SETFD/F_GETFL/F_SETFL/
//!     F_DUPFD），出表命令即 J2 注入对象，按 EINVAL 受控；
//!   - 解析器 `parse_fcntl_call`："fcntl|fd|cmd|arg" 四段解析，
//!     cmd/arg 必为十进制，fd 允许负号；
//!   - 状态机 `FdTable`：16 槽双旗标（fdflags 含 FD_CLOEXEC 位、
//!     stflags 含 O_APPEND/O_NONBLOCK 位），SETFD/SETFL 只允许闭集位，
//!     越界位按 EINVAL 拒；
//!   - 语义核 `do_fcntl`：GETFD/GETFL 回读、SETFD/SETFL 置位、
//!     F_DUPFD 按 ≥arg 最低空槽复制；
//!   - 账本 `FlagLedger`：旗标账与表实数逐槽对核（Σcloexec == 账面），
//!     漂移显性入账；
//!   - 注入核 `BadFcntlCase`：非法命令 / 越界 SETFD 位 / 越界 SETFL 位 /
//!     负 fd / 账漂移 五类坏样本逐类 10/10 次拒绝，逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10107 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92ac0;

/// 段窗上界（下一卡 0x92b00 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92b00;

/// fd 表容量。
pub const FD_CAP: usize = 16;

/// FD_CLOEXEC 位（exec 后关闭）。
pub const FD_CLOEXEC: u32 = 1;
/// O_APPEND 追加位。
pub const O_APPEND: u32 = 0o2000;
/// O_NONBLOCK 非阻塞位。
pub const O_NONBLOCK: u32 = 0o4000;

/// F_GETFD 命令码。
pub const F_GETFD: u32 = 1;
/// F_SETFD 命令码。
pub const F_SETFD: u32 = 2;
/// F_GETFL 命令码。
pub const F_GETFL: u32 = 3;
/// F_SETFL 命令码。
pub const F_SETFL: u32 = 4;
/// F_DUPFD 命令码。
pub const F_DUPFD: u32 = 5;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（非法命令/越界位族，J2 口径）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FcntlCode {
    /// 缺操作成分（首段空或非 fcntl）
    MissingOp,
    /// fd 段不可解析
    BadFdForm,
    /// cmd 段不可解析
    BadCmdForm,
    /// arg 段不可解析
    BadArgForm,
    /// 负 fd（EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// 关闭后 fcntl（空槽，EBADF）
    FdClosed,
    /// 非法命令（出 CmdTable 闭集，EINVAL；J2 主样本）
    IllegalCmd,
    /// 越界旗标位（SETFD/SETFL 位出闭集，EINVAL）
    BadFlagBits,
    /// F_DUPFD 复制失守（未按 ≥arg 最低空槽分配）
    DupFdMissed,
    /// 账漂移（旗标账与表实数不符）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl FcntlCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FcntlCode::MissingOp => "缺操作成分被拒：首段必须是 fcntl；请核对调用行",
            FcntlCode::BadFdForm => "fd 段不可解析被拒：fd 必须是十进制整数（可带负号）",
            FcntlCode::BadCmdForm => "cmd 段不可解析被拒：cmd 必须是十进制整数",
            FcntlCode::BadArgForm => "arg 段不可解析被拒：arg 必须是十进制整数",
            FcntlCode::FdNegative => "负 fd 被拒：fcntl(-1,…) 按 EBADF 受控复现",
            FcntlCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            FcntlCode::FdClosed => "关闭后 fcntl 被拒：槽位已空；请先重开",
            FcntlCode::IllegalCmd => "非法命令被拒：cmd 出闭集；按 EINVAL 受控复现",
            FcntlCode::BadFlagBits => "越界旗标位被拒：SETFD/SETFL 位出闭集；按 EINVAL 受控复现",
            FcntlCode::DupFdMissed => "F_DUPFD 失守被拒：必须按 ≥arg 最低空槽复制；请核分配器",
            FcntlCode::LedgerDrift => "账漂移被拒：旗标账≠表实数；请逐槽对核",
            FcntlCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92ac0..=0x92b00",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FcntlCode; 12] = [
        FcntlCode::MissingOp,
        FcntlCode::BadFdForm,
        FcntlCode::BadCmdForm,
        FcntlCode::BadArgForm,
        FcntlCode::FdNegative,
        FcntlCode::FdRange,
        FcntlCode::FdClosed,
        FcntlCode::IllegalCmd,
        FcntlCode::BadFlagBits,
        FcntlCode::DupFdMissed,
        FcntlCode::LedgerDrift,
        FcntlCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: FcntlCode) -> i32 {
    match code {
        FcntlCode::FdNegative | FcntlCode::FdRange | FcntlCode::FdClosed => EBADF,
        _ => EINVAL,
    }
}

// ── 命令注册表与解析器 ────────────────────────────────────────────────
/// 命令闭集表：(码, 是否需要 arg)。
pub const CMD_TABLE: &[(u32, bool)] = &[
    (F_GETFD, false),
    (F_SETFD, true),
    (F_GETFL, false),
    (F_SETFL, true),
    (F_DUPFD, true),
];

/// 查命令表：出表命令返回 None（非法命令族）。
pub fn cmd_known(cmd: u32) -> Option<bool> {
    let mut i = 0usize;
    while i < CMD_TABLE.len() {
        if let Some((c, needs)) = CMD_TABLE.get(i) {
            if *c == cmd {
                return Some(*needs);
            }
        }
        i += 1;
    }
    None
}

/// 分段器：按 '|' 切四段，段数≠4 返回 None。
fn split4(line: &str) -> Option<[usize; 5]> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 5];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 3 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n != 3 {
        return None;
    }
    cuts[4] = b.len();
    Some(cuts)
}

/// 调用行解析："fcntl|fd|cmd|arg" → (fd, cmd, arg)。
///
/// fd 允许负号；cmd/arg 必为十进制（无 arg 命令约定 arg=0）。
pub fn parse_fcntl_call(line: &str) -> Result<(i32, u32, u32), FcntlCode> {
    let cuts = match split4(line) {
        Some(c) => c,
        None => return Err(FcntlCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let cmd_seg = line.get(cuts[2]..cuts[3] - 1).unwrap_or("");
    let arg_seg = line.get(cuts[3]..cuts[4]).unwrap_or("");
    if op_seg != "fcntl" {
        return Err(FcntlCode::MissingOp);
    }
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(FcntlCode::BadFdForm),
    };
    let cmd = match cmd_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(FcntlCode::BadCmdForm),
    };
    let arg = match arg_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(FcntlCode::BadArgForm),
    };
    Ok((fd, cmd, arg))
}

// ── fd 表（双旗标） ───────────────────────────────────────────────────
/// 一个 fd 槽位的双旗标：fdflags（CLOEXEC）+ stflags（APPEND/NONBLOCK）。
#[derive(Clone, Copy, Debug)]
pub struct FlagCell {
    /// fd 旗标（FD_CLOEXEC 位）
    pub fdflags: u32,
    /// 状态旗标（O_APPEND/O_NONBLOCK 位）
    pub stflags: u32,
}

/// fd 表：16 槽双旗标。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<FlagCell>; FD_CAP],
}

impl FdTable {
    /// 构造空表。
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP] }
    }

    /// 打开一个槽位（默认双旗标 0，模拟 O_RDWR 常规文件）。
    pub fn open(&mut self) -> Result<usize, FcntlCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(FlagCell { fdflags: 0, stflags: 0 });
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(FcntlCode::FdRange)
    }

    /// 关闭 fd。
    pub fn close(&mut self, fd: usize) -> Result<(), FcntlCode> {
        match self.slots.get(fd) {
            Some(Some(_)) => {
                if let Some(s) = self.slots.get_mut(fd) {
                    *s = None;
                }
                Ok(())
            }
            _ => Err(FcntlCode::FdClosed),
        }
    }

    /// 在用槽数。
    pub fn used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(Some(_)) = self.slots.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// fcntl 语义核：按命令分发。
///
/// fd 必检（负/越界/空槽按 EBADF）→ 命令必检（出闭集按 EINVAL，J2）→
/// SETFD 只收 FD_CLOEXEC 位、SETFL 只收 O_APPEND|O_NONBLOCK 位，
/// 越界位按 EINVAL 拒 → F_DUPFD 按 ≥arg 最低空槽复制并回引用 +1 语义
/// （本模型建新槽同旗标）。
pub fn do_fcntl(
    table: &mut FdTable,
    fd: i32,
    cmd: u32,
    arg: u32,
) -> Result<u32, FcntlCode> {
    if fd < 0 {
        return Err(FcntlCode::FdNegative);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(FcntlCode::FdRange);
    }
    let cell = match table.slots.get(fd_u).copied() {
        Some(Some(c)) => c,
        _ => return Err(FcntlCode::FdClosed),
    };
    match cmd {
        F_GETFD => Ok(cell.fdflags),
        F_SETFD => {
            if arg & !FD_CLOEXEC != 0 {
                return Err(FcntlCode::BadFlagBits);
            }
            if let Some(Some(t)) = table.slots.get_mut(fd_u) {
                t.fdflags = arg;
            }
            Ok(0)
        }
        F_GETFL => Ok(cell.stflags),
        F_SETFL => {
            if arg & !(O_APPEND | O_NONBLOCK) != 0 {
                return Err(FcntlCode::BadFlagBits);
            }
            if let Some(Some(t)) = table.slots.get_mut(fd_u) {
                t.stflags = arg;
            }
            Ok(0)
        }
        F_DUPFD => {
            if arg as usize >= FD_CAP {
                return Err(FcntlCode::FdRange);
            }
            let mut hit = None;
            let mut i = arg as usize;
            while i < FD_CAP {
                if let Some(None) = table.slots.get(i) {
                    hit = Some(i);
                    i = FD_CAP;
                } else {
                    i += 1;
                }
            }
            match hit {
                None => Err(FcntlCode::FdRange),
                Some(t) => {
                    if let Some(s) = table.slots.get_mut(t) {
                        *s = Some(FlagCell { fdflags: 0, stflags: cell.stflags });
                    }
                    Ok(t as u32)
                }
            }
        }
        _ => Err(FcntlCode::IllegalCmd),
    }
}

// ── 旗标账本 ──────────────────────────────────────────────────────────
/// 旗标账本：Σcloexec 位与表实数对核，漂移显性入账。
#[derive(Clone, Debug)]
pub struct FlagLedger {
    /// 账面 cloexec 置位槽数
    pub claimed_cloexec: u32,
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl FlagLedger {
    /// 构造零账。
    pub const fn new() -> FlagLedger {
        FlagLedger { claimed_cloexec: 0, drifts: 0, escapes: 0 }
    }

    /// 守恒核：账面与表逐槽对核。
    pub fn audit(&mut self, table: &FdTable) -> Result<(), FcntlCode> {
        let mut actual = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(Some(c)) = table.slots.get(i) {
                if c.fdflags & FD_CLOEXEC != 0 {
                    actual += 1;
                }
            }
            i += 1;
        }
        if actual != self.claimed_cloexec {
            self.drifts = self.drifts.saturating_add(1);
            return Err(FcntlCode::LedgerDrift);
        }
        Ok(())
    }
}

/// 标准语料：开 2 槽 → SETFD 置 CLOEXEC → GETFD 回读 → SETFL 置
/// APPEND|NONBLOCK → GETFL 回读 → F_DUPFD 复制 → 逐项账核。
pub fn standard_corpus(
    table: &mut FdTable,
    ledger: &mut FlagLedger,
) -> Result<(), FcntlCode> {
    let f0 = table.open()?;
    let f1 = table.open()?;
    let _ = do_fcntl(table, f0 as i32, F_SETFD, FD_CLOEXEC)?;
    if do_fcntl(table, f0 as i32, F_GETFD, 0)? != FD_CLOEXEC {
        return Err(FcntlCode::DupFdMissed);
    }
    ledger.claimed_cloexec = 1;
    let _ = do_fcntl(table, f1 as i32, F_SETFL, O_APPEND | O_NONBLOCK)?;
    if do_fcntl(table, f1 as i32, F_GETFL, 0)? != O_APPEND | O_NONBLOCK {
        return Err(FcntlCode::LedgerDrift);
    }
    let dup = do_fcntl(table, f1 as i32, F_DUPFD, f1 as u32 + 1)?;
    if dup != f1 as u32 + 1 {
        return Err(FcntlCode::DupFdMissed);
    }
    ledger.audit(table)?;
    table.close(f0)?;
    ledger.claimed_cloexec = 0;
    ledger.audit(table)?;
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 fcntl 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFcntlCase {
    /// 非法命令（J2 主样本：cmd=99 → EINVAL）
    IllegalCmd,
    /// 越界 SETFD 位
    BadSetFdBits,
    /// 越界 SETFL 位
    BadSetFlBits,
    /// 负 fd
    NegativeFd,
    /// 账漂移
    LedgerDrift,
}

impl BadFcntlCase {
    /// 闭集全量。
    pub const ALL: [BadFcntlCase; 5] = [
        BadFcntlCase::IllegalCmd,
        BadFcntlCase::BadSetFdBits,
        BadFcntlCase::BadSetFlBits,
        BadFcntlCase::NegativeFd,
        BadFcntlCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self, table: &mut FdTable, ledger: &mut FlagLedger) -> bool {
        match self {
            BadFcntlCase::IllegalCmd => {
                let f = match table.open() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_fcntl(table, f as i32, 99, 0);
                let _ = table.close(f);
                r.err() == Some(FcntlCode::IllegalCmd)
                    && errno_of(FcntlCode::IllegalCmd) == EINVAL
            }
            BadFcntlCase::BadSetFdBits => {
                let f = match table.open() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_fcntl(table, f as i32, F_SETFD, 0xFE);
                let _ = table.close(f);
                r.err() == Some(FcntlCode::BadFlagBits)
            }
            BadFcntlCase::BadSetFlBits => {
                let f = match table.open() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_fcntl(table, f as i32, F_SETFL, 0xFFFF);
                let _ = table.close(f);
                r.err() == Some(FcntlCode::BadFlagBits)
            }
            BadFcntlCase::NegativeFd => {
                do_fcntl(table, -1, F_GETFD, 0).err() == Some(FcntlCode::FdNegative)
                    && errno_of(FcntlCode::FdNegative) == EBADF
            }
            BadFcntlCase::LedgerDrift => {
                let mut lg = FlagLedger::new();
                lg.claimed_cloexec = 9;
                lg.audit(table).err() == Some(FcntlCode::LedgerDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_fcntl_injection() -> (FlagLedger, [u32; 5]) {
    let mut table = FdTable::new();
    let mut ledger = FlagLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadFcntlCase::ALL.len() {
        if let Some(case) = BadFcntlCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut ledger);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/关闭后 fcntl 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法命令/越界旗标位族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：SETFD/SETFL/DUPFD/关闭全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = FdTable::new();
    let mut lg = FlagLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.used().to_le_bytes());
    bytes.extend_from_slice(&lg.claimed_cloexec.to_le_bytes());
    bytes.push(match lg.audit(&table) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10107 判据集（19 条静态 + 3 条动态明细）。
pub fn run_unx10107_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10107");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92ac0..=0x92b00", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料全链成功
    let mut table = FdTable::new();
    let mut lg = FlagLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut lg).is_ok();
    s.add("描述符控制达成：SETFD/GETFL/DUPFD 全链成功", corpus_ok,
          "J1 正判据：fcntl02/fcntl05 面语义逐环落账");

    // 3. SETFD→GETFD 往返 10/10
    let mut fd_hits = 0u32;
    let mut fr = 0u32;
    while fr < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap_or(0);
        let _ = do_fcntl(&mut t, f as i32, F_SETFD, FD_CLOEXEC);
        if do_fcntl(&mut t, f as i32, F_GETFD, 0) == Ok(FD_CLOEXEC) {
            fd_hits += 1;
        }
        fr += 1;
    }
    s.add("F_SETFD/F_GETFD：CLOEXEC 位往返 10/10 一致", fd_hits == INJECT_ROUNDS,
          "置位后回读逐位一致（fcntl05 面）");

    // 4. SETFL→GETFL 往返 10/10（双位组合）
    let mut fl_hits = 0u32;
    let mut flr = 0u32;
    while flr < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap_or(0);
        let _ = do_fcntl(&mut t, f as i32, F_SETFL, O_APPEND | O_NONBLOCK);
        if do_fcntl(&mut t, f as i32, F_GETFL, 0) == Ok(O_APPEND | O_NONBLOCK) {
            fl_hits += 1;
        }
        flr += 1;
    }
    s.add("F_SETFL/F_GETFL：APPEND|NONBLOCK 往返 10/10", fl_hits == INJECT_ROUNDS,
          "状态旗标双位组合置位回读一致");

    // 5. GETFD 默认值：新开槽 fdflags=0
    let mut gt = FdTable::new();
    let gf = gt.open().ok().unwrap_or(0);
    let default_ok = do_fcntl(&mut gt, gf as i32, F_GETFD, 0) == Ok(0)
        && do_fcntl(&mut gt, gf as i32, F_GETFL, 0) == Ok(0);
    s.add("默认旗标：新开槽 GETFD/GETFL 均 0", default_ok,
          "未置位时回读必须为 0，无隐藏状态");

    // 6. 非法命令 10/10 EINVAL（J2 反判据）
    let mut cmd_hits = 0u32;
    let mut ct = FdTable::new();
    let cf = ct.open().ok().unwrap_or(0);
    let mut cr = 0u32;
    while cr < INJECT_ROUNDS {
        if do_fcntl(&mut ct, cf as i32, 99, 0).err() == Some(FcntlCode::IllegalCmd)
            && cmd_known(99).is_none()
            && errno_of(FcntlCode::IllegalCmd) == EINVAL
        {
            cmd_hits += 1;
        }
        cr += 1;
    }
    s.add("非法命令：cmd=99 出表 10/10 次 EINVAL 精确复现", cmd_hits == INJECT_ROUNDS,
          "J2 反判据：出 CmdTable 闭集命令逐次按 EINVAL 受控复现");

    // 7. 越界 SETFD 位拒
    let mut bt = FdTable::new();
    let bf = bt.open().ok().unwrap_or(0);
    let setfd_rej = do_fcntl(&mut bt, bf as i32, F_SETFD, 0xFE).err()
        == Some(FcntlCode::BadFlagBits);
    let setfd_keep = do_fcntl(&mut bt, bf as i32, F_GETFD, 0) == Ok(0);
    s.add("SETFD 越界位：0xFE 拒且原位不被污染", setfd_rej && setfd_keep,
          "只收 FD_CLOEXEC 位，越界位拒绝且旗标保持不变");

    // 8. 越界 SETFL 位拒
    let setfl_rej = do_fcntl(&mut bt, bf as i32, F_SETFL, 0xFFFF).err()
        == Some(FcntlCode::BadFlagBits);
    s.add("SETFL 越界位：0xFFFF 拒收", setfl_rej,
          "只收 O_APPEND|O_NONBLOCK 位，越界位拒绝");

    // 9. 负 fd 10/10 EBADF
    let mut neg_hits = 0u32;
    let mut nt = FdTable::new();
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if do_fcntl(&mut nt, -1, F_GETFD, 0).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "fcntl(-1,…) 逐次按 EBADF 受控复现");

    // 10. 越界 fd / 关闭后 fcntl 拒
    let range_rej = do_fcntl(&mut nt, FD_CAP as i32, F_GETFD, 0).err()
        == Some(FcntlCode::FdRange);
    let f_nt2 = nt.open().ok().unwrap_or(0);
    let _ = nt.close(f_nt2);
    let closed_rej = do_fcntl(&mut nt, f_nt2 as i32, F_GETFD, 0).err()
        == Some(FcntlCode::FdClosed);
    s.add("fd 守卫：越界拒、关闭后拒", range_rej && closed_rej,
          "fd 必须在 0..16 且槽位在用，越界/空槽显性拒绝");

    // 11. F_DUPFD：≥arg 最低空槽
    let mut dt = FdTable::new();
    let d0 = dt.open().ok().unwrap_or(0);
    let d1 = dt.open().ok().unwrap_or(1);
    let _ = dt.close(d0);
    let dupfd = do_fcntl(&mut dt, d1 as i32, F_DUPFD, 0);
    s.add("F_DUPFD：按 ≥arg 最低空槽复制（命中槽 0）", dupfd == Ok(0),
          "arg=0 时最低空槽 0 命中，DupFdMissed 兜底");

    // 12. F_DUPFD arg 越界拒
    let dupfd_rej = do_fcntl(&mut dt, d1 as i32, F_DUPFD, FD_CAP as u32).err()
        == Some(FcntlCode::FdRange);
    s.add("F_DUPFD 越界：arg≥16 拒收", dupfd_rej,
          "arg 必须落在 0..16 窗，越界按 EBADF 族受控");

    // 13. 解析器：四段/非法段/残形
    let p_ok = parse_fcntl_call("fcntl|3|2|1") == Ok((3, F_SETFD, 1));
    let p_bad_cmd = parse_fcntl_call("fcntl|3|x|1").err() == Some(FcntlCode::BadCmdForm);
    let p_bad_arg = parse_fcntl_call("fcntl|3|2|x").err() == Some(FcntlCode::BadArgForm);
    let p_seg = parse_fcntl_call("fcntl|3|2").err() == Some(FcntlCode::MissingOp);
    s.add("协议解析：四段/非数字 cmd/arg/残形全拒", p_ok && p_bad_cmd && p_bad_arg && p_seg,
          "调用行恰四段 fcntl|fd|cmd|arg，逐段显性核");

    // 14. 旗标账守恒：账面 == 表实数
    let mut kt = FdTable::new();
    let mut kl = FlagLedger::new();
    let k0 = kt.open().ok().unwrap_or(0);
    let _ = do_fcntl(&mut kt, k0 as i32, F_SETFD, FD_CLOEXEC);
    kl.claimed_cloexec = 1;
    let cons_ok = kl.audit(&kt).is_ok();
    kl.claimed_cloexec = 5;
    let drift_rej = kl.audit(&kt).err() == Some(FcntlCode::LedgerDrift);
    s.add("旗标账守恒：账面==表实数、漂移逐次检出", cons_ok && drift_rej && kl.drifts == 1,
          "cloexec 账逐槽对核，漂移显性入账");

    // 15. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_fcntl_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "非法命令/越界 SETFD/越界 SETFL/负 fd/账漂移全被显式拒收");

    // 16. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 17. 文案闭集 + 诊断码唯一落段
    let text_ok = FcntlCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < FcntlCode::ALL.len() {
        let mut j = i2 + 1;
        while j < FcntlCode::ALL.len() {
            if let (Some(x), Some(y)) = (FcntlCode::ALL.get(i2), FcntlCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = FcntlCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 18. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 19. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：旗标快照 + 账快照 + 指纹快照
    let used = table.used();
    let fd_text = format!("fd 在用 {}/16，cloexec 账 {} 槽", used, lg.claimed_cloexec);
    s.add_dyn("fd 快照", used <= FD_CAP as u32, &fd_text);
    let lg_text = format!("漂移={}，逃逸={}", lg.drifts, lg.escapes);
    s.add_dyn("账快照", lg.drifts == 0, &lg_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fcntl_ok() {
        assert_eq!(parse_fcntl_call("fcntl|3|2|1"), Ok((3, F_SETFD, 1)));
        assert_eq!(parse_fcntl_call("fcntl|-1|1|0"), Ok((-1, F_GETFD, 0)));
    }

    #[test]
    fn parse_fcntl_rejects() {
        assert_eq!(parse_fcntl_call("flock|3|2|1"), Err(FcntlCode::MissingOp));
        assert_eq!(parse_fcntl_call("fcntl|x|2|1"), Err(FcntlCode::BadFdForm));
        assert_eq!(parse_fcntl_call("fcntl|3|x|1"), Err(FcntlCode::BadCmdForm));
        assert_eq!(parse_fcntl_call("fcntl|3|2|x"), Err(FcntlCode::BadArgForm));
        assert_eq!(parse_fcntl_call("fcntl|3|2"), Err(FcntlCode::MissingOp));
    }

    #[test]
    fn cmd_table_closed_set() {
        assert!(cmd_known(F_GETFD).is_some());
        assert!(cmd_known(F_DUPFD).is_some());
        assert!(cmd_known(99).is_none());
        assert!(cmd_known(0).is_none());
    }

    #[test]
    fn setfd_getfd_roundtrip() {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap();
        assert!(do_fcntl(&mut t, f as i32, F_SETFD, FD_CLOEXEC).is_ok());
        assert_eq!(do_fcntl(&mut t, f as i32, F_GETFD, 0), Ok(FD_CLOEXEC));
    }

    #[test]
    fn setfl_getfl_roundtrip() {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap();
        assert!(do_fcntl(&mut t, f as i32, F_SETFL, O_APPEND | O_NONBLOCK).is_ok());
        assert_eq!(do_fcntl(&mut t, f as i32, F_GETFL, 0), Ok(O_APPEND | O_NONBLOCK));
    }

    #[test]
    fn illegal_cmd_is_einval() {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap();
        assert_eq!(do_fcntl(&mut t, f as i32, 99, 0), Err(FcntlCode::IllegalCmd));
        assert_eq!(errno_of(FcntlCode::IllegalCmd), EINVAL);
    }

    #[test]
    fn bad_flag_bits_rejected() {
        let mut t = FdTable::new();
        let f = t.open().ok().unwrap();
        assert_eq!(do_fcntl(&mut t, f as i32, F_SETFD, 0xFE), Err(FcntlCode::BadFlagBits));
        assert_eq!(do_fcntl(&mut t, f as i32, F_SETFL, 0xFFFF), Err(FcntlCode::BadFlagBits));
        assert_eq!(do_fcntl(&mut t, f as i32, F_GETFD, 0), Ok(0));
    }

    #[test]
    fn dupfd_lowest_from_arg() {
        let mut t = FdTable::new();
        let _ = t.open();
        let d1 = t.open().ok().unwrap();
        let _ = t.close(0);
        assert_eq!(do_fcntl(&mut t, d1 as i32, F_DUPFD, 0), Ok(0));
    }

    #[test]
    fn negative_fd_is_ebadf() {
        let mut t = FdTable::new();
        assert_eq!(do_fcntl(&mut t, -1, F_GETFD, 0), Err(FcntlCode::FdNegative));
        assert_eq!(errno_of(FcntlCode::FdNegative), EBADF);
    }

    #[test]
    fn standard_corpus_green() {
        let mut t = FdTable::new();
        let mut lg = FlagLedger::new();
        assert!(standard_corpus(&mut t, &mut lg).is_ok());
        assert!(lg.audit(&t).is_ok());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_fcntl_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10107_checks_green() {
        let s = run_unx10107_checks();
        assert!(s.all_passed(), "UNX-F10107 red: {}", s.red_items_text());
    }
}
