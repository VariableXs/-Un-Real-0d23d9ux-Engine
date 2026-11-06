//! UNX-F10108 ioctl 正反双判据（控制请求达成与非法请求误用受控） · 0x92b00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10108（ioctl 正反双判据：J1 合法请求分发达成，J2 非法
//!       请求误用受控）。
//! 锚点：UNX-F10109-J1 pipe 双端读写达成（LTP pipe01/pipe02 面 10/10
//!       次）；UNX-F10109-J2 断端写误用受控（预期 EPIPE 精确复现 10/10
//!       次，SIGPIPE 处置按 L1 条款注记）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92b00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `IoctlCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ReqTable`：请求闭集（TCGETS/TCSETS/TIOCGWINSZ/TIOCSWINSZ/
//!     FIOCLEX），出表请求即 J2 注入对象，按 ENOTTY 受控；
//!   - 解析器 `parse_ioctl_call`："ioctl|fd|req|arg" 四段解析；
//!   - 状态机 `TermModel`：终端模型（echo 位 + 行规尺寸 rows/cols），
//!     TCGETS/TCSETS 回显位往返、TIOCGWINSZ/TIOCSWINSZ 尺寸往返、
//!     FIOCLEX 置 CLOEXEC；
//!   - 分发核 `do_ioctl`：fd 必检（EBADF）→ 非 tty fd 按 ENOTTY 拒 →
//!     请求分发（出表按 ENOTTY，J2）→ 参数域检（arg>1 / 尺寸>255 按
//!     EINVAL 拒）；
//!   - 账本 `ReqLedger`：分发计数账，声称/实际漂移显性入账；
//!   - 注入核 `BadIoctlCase`：非法请求 / 非 tty fd / 负 fd / 越界参数 /
//!     账漂移 五类坏样本逐类 10/10 次拒绝，逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10108 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92b00;

/// 段窗上界（下一卡 0x92b40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92b40;

/// fd 表容量。
pub const FD_CAP: usize = 16;

/// TCGETS 请求码（读终端态）。
pub const TCGETS: u32 = 1;
/// TCSETS 请求码（写终端态：arg 为 echo 位）。
pub const TCSETS: u32 = 2;
/// TIOCGWINSZ 请求码（读窗口尺寸：返回 rows<<8|cols）。
pub const TIOCGWINSZ: u32 = 3;
/// TIOCSWINSZ 请求码（写窗口尺寸：arg 为 rows<<8|cols）。
pub const TIOCSWINSZ: u32 = 4;
/// FIOCLEX 请求码（置 FD_CLOEXEC）。
pub const FIOCLEX: u32 = 5;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（越界参数族）。
pub const EINVAL: i32 = 22;
/// 不适当 ioctl（非法请求/非 tty 族，J2 口径）。
pub const ENOTTY: i32 = 25;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoctlCode {
    /// 缺操作成分（首段空或非 ioctl）
    MissingOp,
    /// fd 段不可解析
    BadFdForm,
    /// req 段不可解析
    BadReqForm,
    /// arg 段不可解析
    BadArgForm,
    /// 负 fd（EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// 关闭后 ioctl（空槽，EBADF）
    FdClosed,
    /// 非 tty fd（常规对象上 ioctl，ENOTTY）
    NotTty,
    /// 非法请求（出 ReqTable 闭集，ENOTTY；J2 主样本）
    IllegalReq,
    /// 越界参数（arg>1 / 尺寸>255，EINVAL）
    BadArg,
    /// 账漂移（分发计数账与实数不符）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl IoctlCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            IoctlCode::MissingOp => "缺操作成分被拒：首段必须是 ioctl；请核对调用行",
            IoctlCode::BadFdForm => "fd 段不可解析被拒：fd 必须是十进制整数（可带负号）",
            IoctlCode::BadReqForm => "req 段不可解析被拒：req 必须是十进制整数",
            IoctlCode::BadArgForm => "arg 段不可解析被拒：arg 必须是十进制整数",
            IoctlCode::FdNegative => "负 fd 被拒：ioctl(-1,…) 按 EBADF 受控复现",
            IoctlCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            IoctlCode::FdClosed => "关闭后 ioctl 被拒：槽位已空；请先重开",
            IoctlCode::NotTty => "非 tty fd 被拒：常规对象不收终端请求；按 ENOTTY 受控复现",
            IoctlCode::IllegalReq => "非法请求被拒：req 出闭集；按 ENOTTY 受控复现",
            IoctlCode::BadArg => "越界参数被拒：arg>1 或尺寸>255；按 EINVAL 受控复现",
            IoctlCode::LedgerDrift => "账漂移被拒：分发计数账≠实数；请回查账面",
            IoctlCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92b00..=0x92b40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [IoctlCode; 12] = [
        IoctlCode::MissingOp,
        IoctlCode::BadFdForm,
        IoctlCode::BadReqForm,
        IoctlCode::BadArgForm,
        IoctlCode::FdNegative,
        IoctlCode::FdRange,
        IoctlCode::FdClosed,
        IoctlCode::NotTty,
        IoctlCode::IllegalReq,
        IoctlCode::BadArg,
        IoctlCode::LedgerDrift,
        IoctlCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: IoctlCode) -> i32 {
    match code {
        IoctlCode::FdNegative | IoctlCode::FdRange | IoctlCode::FdClosed => EBADF,
        IoctlCode::NotTty | IoctlCode::IllegalReq => ENOTTY,
        IoctlCode::BadArg => EINVAL,
        _ => EINVAL,
    }
}

// ── 请求注册表与解析器 ────────────────────────────────────────────────
/// 请求闭集表。
pub const REQ_TABLE: &[u32] = &[TCGETS, TCSETS, TIOCGWINSZ, TIOCSWINSZ, FIOCLEX];

/// 查请求表：出表请求返回 false（非法请求族）。
pub fn req_known(req: u32) -> bool {
    let mut i = 0usize;
    while i < REQ_TABLE.len() {
        if let Some(r) = REQ_TABLE.get(i) {
            if *r == req {
                return true;
            }
        }
        i += 1;
    }
    false
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

/// 调用行解析："ioctl|fd|req|arg" → (fd, req, arg)。
pub fn parse_ioctl_call(line: &str) -> Result<(i32, u32, u32), IoctlCode> {
    let cuts = match split4(line) {
        Some(c) => c,
        None => return Err(IoctlCode::MissingOp),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let fd_seg = line.get(cuts[1]..cuts[2] - 1).unwrap_or("");
    let req_seg = line.get(cuts[2]..cuts[3] - 1).unwrap_or("");
    let arg_seg = line.get(cuts[3]..cuts[4]).unwrap_or("");
    if op_seg != "ioctl" {
        return Err(IoctlCode::MissingOp);
    }
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(IoctlCode::BadFdForm),
    };
    let req = match req_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(IoctlCode::BadReqForm),
    };
    let arg = match arg_seg.parse::<u32>() {
        Ok(v) => v,
        Err(_) => return Err(IoctlCode::BadArgForm),
    };
    Ok((fd, req, arg))
}

// ── fd 表与终端状态机 ────────────────────────────────────────────────
/// 一个 fd 槽位：是否 tty + 终端态（echo/rows/cols/cloexec）。
#[derive(Clone, Copy, Debug)]
pub struct TermCell {
    /// 是否 tty（false 即常规对象，ENOTTY 族）
    pub is_tty: bool,
    /// 回显位
    pub echo: bool,
    /// 窗口行数（0..=255）
    pub rows: u32,
    /// 窗口列数（0..=255）
    pub cols: u32,
    /// CLOEXEC 位（FIOCLEX 置位）
    pub cloexec: bool,
}

/// fd 表：16 槽终端态。
#[derive(Clone, Debug)]
pub struct FdTable {
    /// 槽位数组（None=空槽）
    pub slots: [Option<TermCell>; FD_CAP],
}

impl FdTable {
    /// 构造空表。
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP] }
    }

    /// 打开一个 tty 槽位（默认 echo=1、尺寸 24×80）。
    pub fn open_tty(&mut self) -> Result<usize, IoctlCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(TermCell { is_tty: true, echo: true, rows: 24, cols: 80, cloexec: false });
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(IoctlCode::FdRange)
    }

    /// 打开一个非 tty 槽位（常规对象）。
    pub fn open_plain(&mut self) -> Result<usize, IoctlCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(TermCell { is_tty: false, echo: false, rows: 0, cols: 0, cloexec: false });
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(IoctlCode::FdRange)
    }

    /// 关闭 fd。
    pub fn close(&mut self, fd: usize) -> Result<(), IoctlCode> {
        match self.slots.get(fd) {
            Some(Some(_)) => {
                if let Some(s) = self.slots.get_mut(fd) {
                    *s = None;
                }
                Ok(())
            }
            _ => Err(IoctlCode::FdClosed),
        }
    }
}

/// ioctl 分发核：按请求闭集分发。
///
/// fd 必检（EBADF）→ 非 tty 按 ENOTTY 拒 → 请求必检（出表按 ENOTTY，
/// J2）→ 参数域检（TCSETS arg>1 / TIOCSWINSZ 尺寸>255 按 EINVAL 拒）。
pub fn do_ioctl(
    table: &mut FdTable,
    fd: i32,
    req: u32,
    arg: u32,
) -> Result<u32, IoctlCode> {
    if fd < 0 {
        return Err(IoctlCode::FdNegative);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(IoctlCode::FdRange);
    }
    let cell = match table.slots.get(fd_u).copied() {
        Some(Some(c)) => c,
        _ => return Err(IoctlCode::FdClosed),
    };
    if !cell.is_tty {
        return Err(IoctlCode::NotTty);
    }
    if !req_known(req) {
        return Err(IoctlCode::IllegalReq);
    }
    match req {
        TCGETS => Ok(if cell.echo { 1 } else { 0 }),
        TCSETS => {
            if arg > 1 {
                return Err(IoctlCode::BadArg);
            }
            if let Some(Some(t)) = table.slots.get_mut(fd_u) {
                t.echo = arg == 1;
            }
            Ok(0)
        }
        TIOCGWINSZ => Ok((cell.rows << 8) | cell.cols),
        TIOCSWINSZ => {
            let rows = arg >> 8;
            let cols = arg & 0xFF;
            if rows > 255 || cols > 255 {
                return Err(IoctlCode::BadArg);
            }
            if let Some(Some(t)) = table.slots.get_mut(fd_u) {
                t.rows = rows;
                t.cols = cols;
            }
            Ok(0)
        }
        FIOCLEX => {
            if let Some(Some(t)) = table.slots.get_mut(fd_u) {
                t.cloexec = true;
            }
            Ok(0)
        }
        _ => Err(IoctlCode::IllegalReq),
    }
}

// ── 分发账本 ──────────────────────────────────────────────────────────
/// 分发账本：逐请求成功次数账，声称/实际漂移显性入账。
#[derive(Clone, Debug)]
pub struct ReqLedger {
    /// 逐请求成功计数（下标即 REQ_TABLE 位序）
    pub counts: [u32; 5],
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl ReqLedger {
    /// 构造零账。
    pub const fn new() -> ReqLedger {
        ReqLedger { counts: [0, 0, 0, 0, 0], drifts: 0, escapes: 0 }
    }

    /// 按请求码定位账位（出表返回 None）。
    pub fn idx_of(req: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < REQ_TABLE.len() {
            if let Some(r) = REQ_TABLE.get(i) {
                if *r == req {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一次成功分发。
    pub fn record(&mut self, req: u32) -> Result<(), IoctlCode> {
        match ReqLedger::idx_of(req) {
            Some(i) => {
                if let Some(c) = self.counts.get_mut(i) {
                    *c = c.saturating_add(1);
                }
                Ok(())
            }
            None => Err(IoctlCode::IllegalReq),
        }
    }

    /// 守恒核：声称合计与逐项合计比对。
    pub fn audit(&mut self, claimed_total: u32) -> Result<(), IoctlCode> {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                total = total.saturating_add(*c);
            }
            i += 1;
        }
        if total != claimed_total {
            self.drifts = self.drifts.saturating_add(1);
            return Err(IoctlCode::LedgerDrift);
        }
        Ok(())
    }
}

/// 标准语料：开 tty 槽 → 五请求各分发 10 轮（TCSETS 0/1 交替回读、
/// TIOCSWINSZ 尺寸往返、FIOCLEX 置位核验）→ 逐次入账。
pub fn standard_corpus(
    table: &mut FdTable,
    ledger: &mut ReqLedger,
) -> Result<(), IoctlCode> {
    let f = table.open_tty()?;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // TCSETS/TCGETS 回显位往返
        let want_echo = round % 2 == 0;
        let _ = do_ioctl(table, f as i32, TCSETS, if want_echo { 1 } else { 0 })?;
        if do_ioctl(table, f as i32, TCGETS, 0)? != if want_echo { 1 } else { 0 } {
            return Err(IoctlCode::LedgerDrift);
        }
        ledger.record(TCSETS)?;
        ledger.record(TCGETS)?;
        // TIOCSWINSZ/TIOCGWINSZ 尺寸往返
        let rows = 20 + round;
        let cols = 90 + round;
        let _ = do_ioctl(table, f as i32, TIOCSWINSZ, (rows << 8) | cols)?;
        let got = do_ioctl(table, f as i32, TIOCGWINSZ, 0)?;
        if got != (rows << 8) | cols {
            return Err(IoctlCode::LedgerDrift);
        }
        ledger.record(TIOCSWINSZ)?;
        ledger.record(TIOCGWINSZ)?;
        // FIOCLEX 置位
        let _ = do_ioctl(table, f as i32, FIOCLEX, 0)?;
        ledger.record(FIOCLEX)?;
        round += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 ioctl 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadIoctlCase {
    /// 非法请求（J2 主样本：req=99 → ENOTTY）
    IllegalReq,
    /// 非 tty fd
    NotTty,
    /// 负 fd
    NegativeFd,
    /// 越界参数（TCSETS arg=7）
    BadArg,
    /// 账漂移
    LedgerDrift,
}

impl BadIoctlCase {
    /// 闭集全量。
    pub const ALL: [BadIoctlCase; 5] = [
        BadIoctlCase::IllegalReq,
        BadIoctlCase::NotTty,
        BadIoctlCase::NegativeFd,
        BadIoctlCase::BadArg,
        BadIoctlCase::LedgerDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self, table: &mut FdTable, ledger: &mut ReqLedger) -> bool {
        match self {
            BadIoctlCase::IllegalReq => {
                let f = match table.open_tty() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_ioctl(table, f as i32, 99, 0);
                let _ = table.close(f);
                r.err() == Some(IoctlCode::IllegalReq)
                    && errno_of(IoctlCode::IllegalReq) == ENOTTY
            }
            BadIoctlCase::NotTty => {
                let f = match table.open_plain() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_ioctl(table, f as i32, TCGETS, 0);
                let _ = table.close(f);
                r.err() == Some(IoctlCode::NotTty)
            }
            BadIoctlCase::NegativeFd => {
                do_ioctl(table, -1, TCGETS, 0).err() == Some(IoctlCode::FdNegative)
                    && errno_of(IoctlCode::FdNegative) == EBADF
            }
            BadIoctlCase::BadArg => {
                let f = match table.open_tty() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let r = do_ioctl(table, f as i32, TCSETS, 7);
                let _ = table.close(f);
                r.err() == Some(IoctlCode::BadArg)
            }
            BadIoctlCase::LedgerDrift => {
                let mut lg = ReqLedger::new();
                lg.audit(9).err() == Some(IoctlCode::LedgerDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_ioctl_injection() -> (ReqLedger, [u32; 5]) {
    let mut table = FdTable::new();
    let mut ledger = ReqLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadIoctlCase::ALL.len() {
        if let Some(case) = BadIoctlCase::ALL.get(ci).copied() {
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/关闭后 ioctl 族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；非法请求/非 tty 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界参数族"),
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

/// 规范语料双跑指纹：五请求 10 轮分发全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut table = FdTable::new();
    let mut lg = ReqLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    let mut i = 0usize;
    while i < lg.counts.len() {
        if let Some(c) = lg.counts.get(i) {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        i += 1;
    }
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10108 判据集（19 条静态 + 3 条动态明细）。
pub fn run_unx10108_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10108");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92b00..=0x92b40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料：五请求 10 轮分发全过
    let mut table = FdTable::new();
    let mut lg = ReqLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut lg).is_ok();
    s.add("请求分发达成：五请求各 10 轮全过", corpus_ok,
          "J1 正判据：ioctl01 面请求分发逐轮落账");

    // 3. TCGETS 默认回显位：echo=1 回读 10/10
    let mut def_hits = 0u32;
    let mut dr = 0u32;
    while dr < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap_or(0);
        if do_ioctl(&mut t, f as i32, TCGETS, 0) == Ok(1) {
            def_hits += 1;
        }
        dr += 1;
    }
    s.add("TCGETS 默认态：echo=1 回读 10/10", def_hits == INJECT_ROUNDS,
          "终端默认回显开，未设置不得漂移");

    // 4. TCSETS→TCGETS 往返 10/10（0/1 两态）
    let mut echo_hits = 0u32;
    let mut er = 0u32;
    while er < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap_or(0);
        let want = er % 2;
        let _ = do_ioctl(&mut t, f as i32, TCSETS, want);
        if do_ioctl(&mut t, f as i32, TCGETS, 0) == Ok(want) {
            echo_hits += 1;
        }
        er += 1;
    }
    s.add("TCSETS/TCGETS：回显位往返 10/10", echo_hits == INJECT_ROUNDS,
          "写后回读逐位一致，两态交替可复算");

    // 5. TIOCSWINSZ→TIOCGWINSZ 往返 10/10
    let mut win_hits = 0u32;
    let mut wr = 0u32;
    while wr < INJECT_ROUNDS {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap_or(0);
        let rows = 30 + wr;
        let cols = 100 + wr;
        let _ = do_ioctl(&mut t, f as i32, TIOCSWINSZ, (rows << 8) | cols);
        if do_ioctl(&mut t, f as i32, TIOCGWINSZ, 0) == Ok((rows << 8) | cols) {
            win_hits += 1;
        }
        wr += 1;
    }
    s.add("TIOCSWINSZ/TIOCGWINSZ：尺寸往返 10/10", win_hits == INJECT_ROUNDS,
          "rows<<8|cols 编码往返一致");

    // 6. 非法请求 10/10 ENOTTY（J2 反判据）
    let mut req_hits = 0u32;
    let mut rt = FdTable::new();
    let rf = rt.open_tty().ok().unwrap_or(0);
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        if do_ioctl(&mut rt, rf as i32, 99, 0).err() == Some(IoctlCode::IllegalReq)
            && !req_known(99)
            && errno_of(IoctlCode::IllegalReq) == ENOTTY
        {
            req_hits += 1;
        }
        rr += 1;
    }
    s.add("非法请求：req=99 出表 10/10 次 ENOTTY 精确复现", req_hits == INJECT_ROUNDS,
          "J2 反判据：出 ReqTable 闭集请求逐次按 ENOTTY 受控复现");

    // 7. 非 tty fd → ENOTTY
    let mut pt = FdTable::new();
    let pf = pt.open_plain().ok().unwrap_or(0);
    let notty_rej = do_ioctl(&mut pt, pf as i32, TCGETS, 0).err() == Some(IoctlCode::NotTty);
    s.add("非 tty fd：常规对象 ioctl 拒收（ENOTTY）", notty_rej,
          "终端请求只落在 tty 槽位，常规对象显性拒绝");

    // 8. 越界参数拒：TCSETS arg=7 / TIOCSWINSZ 尺寸>255
    let mut at = FdTable::new();
    let af = at.open_tty().ok().unwrap_or(0);
    let arg_rej = do_ioctl(&mut at, af as i32, TCSETS, 7).err() == Some(IoctlCode::BadArg);
    let win_rej = do_ioctl(&mut at, af as i32, TIOCSWINSZ, (300 << 8) | 80).err()
        == Some(IoctlCode::BadArg);
    s.add("越界参数：TCSETS 7 拒、rows>255 拒", arg_rej && win_rej,
          "参数域闭集，越界按 EINVAL 受控");

    // 9. FIOCLEX 置位：置后状态核验
    let mut xt = FdTable::new();
    let xf = xt.open_tty().ok().unwrap_or(0);
    let _ = do_ioctl(&mut xt, xf as i32, FIOCLEX, 0);
    let cloexec_set = match xt.slots.get(xf).copied() {
        Some(Some(c)) => c.cloexec,
        Some(None) => false,
        None => false,
    };
    s.add("FIOCLEX：CLOEXEC 置位留痕", cloexec_set,
          "请求效应落账，置位状态可查");

    // 10. 负 fd 10/10 EBADF
    let mut neg_hits = 0u32;
    let mut nt = FdTable::new();
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if do_ioctl(&mut nt, -1, TCGETS, 0).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("负 fd：10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "ioctl(-1,…) 逐次按 EBADF 受控复现");

    // 11. 越界 fd / 关闭后 ioctl 拒
    let range_rej = do_ioctl(&mut nt, FD_CAP as i32, TCGETS, 0).err()
        == Some(IoctlCode::FdRange);
    let f2 = nt.open_tty().ok().unwrap_or(0);
    let _ = nt.close(f2);
    let closed_rej = do_ioctl(&mut nt, f2 as i32, TCGETS, 0).err()
        == Some(IoctlCode::FdClosed);
    s.add("fd 守卫：越界拒、关闭后拒", range_rej && closed_rej,
          "fd 必须在 0..16 且槽位在用");

    // 12. 解析器：四段/非数字/残形
    let p_ok = parse_ioctl_call("ioctl|3|3|20480") == Ok((3, TIOCGWINSZ, 20480));
    let p_bad_req = parse_ioctl_call("ioctl|3|x|0").err() == Some(IoctlCode::BadReqForm);
    let p_bad_arg = parse_ioctl_call("ioctl|3|3|x").err() == Some(IoctlCode::BadArgForm);
    let p_seg = parse_ioctl_call("ioctl|3|3").err() == Some(IoctlCode::MissingOp);
    s.add("协议解析：四段/非数字 req/arg/残形全拒", p_ok && p_bad_req && p_bad_arg && p_seg,
          "调用行恰四段 ioctl|fd|req|arg，逐段显性核");

    // 13. 分发账守恒：逐项合计 == 声称合计
    let mut total = 0u32;
    let mut ti = 0usize;
    while ti < lg.counts.len() {
        if let Some(c) = lg.counts.get(ti) {
            total = total.saturating_add(*c);
        }
        ti += 1;
    }
    let cons_ok = lg.audit(total).is_ok() && total == INJECT_ROUNDS * 5;
    let mut dl = ReqLedger::new();
    let drift_rej = dl.audit(9).err() == Some(IoctlCode::LedgerDrift);
    s.add("分发账守恒：合计 50 零漂移、失配逐次检出", cons_ok && drift_rej && dl.drifts == 1,
          "逐请求计数账与实数对核，漂移显性入账");

    // 14. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_ioctl_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "非法请求/非 tty/负 fd/越界参数/账漂移全被显式拒收");

    // 15. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 16. 文案闭集 + 诊断码唯一落段
    let text_ok = IoctlCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < IoctlCode::ALL.len() {
        let mut j = i2 + 1;
        while j < IoctlCode::ALL.len() {
            if let (Some(x), Some(y)) = (IoctlCode::ALL.get(i2), IoctlCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = IoctlCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOTTY)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 19. 请求闭集完备：REQ_TABLE 恰 5 请求
    s.add("请求闭集：REQ_TABLE 恰 5 请求", REQ_TABLE.len() == 5,
          "TCGETS/TCSETS/TIOCGWINSZ/TIOCSWINSZ/FIOCLEX 闭集齐备");

    // 动态明细（≤8）：分发快照 + 账快照 + 指纹快照
    let req_text = format!("五请求分发合计 {} 次", total);
    s.add_dyn("分发快照", total == INJECT_ROUNDS * 5, &req_text);
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
    fn parse_ioctl_ok() {
        assert_eq!(parse_ioctl_call("ioctl|3|1|0"), Ok((3, TCGETS, 0)));
        assert_eq!(parse_ioctl_call("ioctl|-1|99|0"), Ok((-1, 99, 0)));
    }

    #[test]
    fn parse_ioctl_rejects() {
        assert_eq!(parse_ioctl_call("iow|3|1|0"), Err(IoctlCode::MissingOp));
        assert_eq!(parse_ioctl_call("ioctl|x|1|0"), Err(IoctlCode::BadFdForm));
        assert_eq!(parse_ioctl_call("ioctl|3|x|0"), Err(IoctlCode::BadReqForm));
        assert_eq!(parse_ioctl_call("ioctl|3|1|x"), Err(IoctlCode::BadArgForm));
        assert_eq!(parse_ioctl_call("ioctl|3|1"), Err(IoctlCode::MissingOp));
    }

    #[test]
    fn req_table_closed_set() {
        assert!(req_known(TCGETS));
        assert!(req_known(FIOCLEX));
        assert!(!req_known(99));
        assert!(!req_known(0));
    }

    #[test]
    fn echo_roundtrip() {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap();
        assert!(do_ioctl(&mut t, f as i32, TCSETS, 0).is_ok());
        assert_eq!(do_ioctl(&mut t, f as i32, TCGETS, 0), Ok(0));
        assert!(do_ioctl(&mut t, f as i32, TCSETS, 1).is_ok());
        assert_eq!(do_ioctl(&mut t, f as i32, TCGETS, 0), Ok(1));
    }

    #[test]
    fn winsize_roundtrip() {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap();
        assert!(do_ioctl(&mut t, f as i32, TIOCSWINSZ, (40 << 8) | 120).is_ok());
        assert_eq!(do_ioctl(&mut t, f as i32, TIOCGWINSZ, 0), Ok((40 << 8) | 120));
    }

    #[test]
    fn illegal_req_is_enotty() {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap();
        assert_eq!(do_ioctl(&mut t, f as i32, 99, 0), Err(IoctlCode::IllegalReq));
        assert_eq!(errno_of(IoctlCode::IllegalReq), ENOTTY);
    }

    #[test]
    fn not_tty_is_enotty() {
        let mut t = FdTable::new();
        let f = t.open_plain().ok().unwrap();
        assert_eq!(do_ioctl(&mut t, f as i32, TCGETS, 0), Err(IoctlCode::NotTty));
    }

    #[test]
    fn bad_args_rejected() {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap();
        assert_eq!(do_ioctl(&mut t, f as i32, TCSETS, 7), Err(IoctlCode::BadArg));
        assert_eq!(do_ioctl(&mut t, f as i32, TIOCSWINSZ, (300 << 8) | 80), Err(IoctlCode::BadArg));
    }

    #[test]
    fn fioclex_sets_cloexec() {
        let mut t = FdTable::new();
        let f = t.open_tty().ok().unwrap();
        assert!(do_ioctl(&mut t, f as i32, FIOCLEX, 0).is_ok());
        assert!(t.slots[f].unwrap().cloexec);
    }

    #[test]
    fn negative_fd_is_ebadf() {
        let mut t = FdTable::new();
        assert_eq!(do_ioctl(&mut t, -1, TCGETS, 0), Err(IoctlCode::FdNegative));
        assert_eq!(errno_of(IoctlCode::FdNegative), EBADF);
    }

    #[test]
    fn standard_corpus_green() {
        let mut t = FdTable::new();
        let mut lg = ReqLedger::new();
        assert!(standard_corpus(&mut t, &mut lg).is_ok());
        assert!(lg.audit(INJECT_ROUNDS * 5).is_ok());
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_ioctl_injection();
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
    fn unx10108_checks_green() {
        let s = run_unx10108_checks();
        assert!(s.all_passed(), "UNX-F10108 red: {}", s.red_items_text());
    }
}
