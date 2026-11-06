//! UNX-F9329 setrlimit 坏结构指针矩阵 · 0x7BFC0段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9329 逐字）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F9329-J1 坏 rlim 指针注入判据过，全 EFAULT 格过，
//!      10/10 次检出格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 失败面是「资源号 → rlim 拷贝 → 区间 → 权限」四段串行，做成两阶段
//!      簿记：先只读校验，全部通过才 commit；任何失败走 abort，资源簿不变。
//!      与 F9328 的「单次判码」不同：这里必须证明 abort 无副作用。
//!   ② rlim 是 16 字节双字段结构，指针取法冻结成 10 个样例，区分整读、
//!      读到一半、完全不可读；只有整读 16 字节才进入区间校验，部分读虽
//!      脏了字节仍必须 EFAULT 且不得提交。
//!   ③ 资源策略表 16 行：13 行无上限，3 行有上限（NOFILE=1048576 需
//!      CAP_SYS_RESOURCE；NICE/RTPRIO=0 需 CAP_SYS_NICE）。上限只拦 max；
//!      cur > max 恒 EINVAL 且先于上限判断。caps 是位掩码，不可串用。
//!   ④ 注入矩阵 = 20 个资源取法（16 合法 + 4 非法）× 10 个指针取法，每格
//!      10 轮。非法资源 40 格全 EINVAL；合法资源 112 格坏指针全 EFAULT、
//!      48 格好指针全部提交。坏指针单测 100 轮要求逐样例 10/10 检出。
//!   ⑤ 账本不变量：attempts == commits + aborts == detected + escaped，
//!      escaped 恒 0；四桶之和等于 attempts。诊断码段内闭集 12 个
//!      （0x7BFC1..=0x7BFCC），码值唯一、文案非空。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9329 独占 0x7BFC0..=0x7BFFF；F9330 从 0x7C000 起）。
pub const SEG_BASE: u32 = 0x7BFC0;
/// 段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;
/// 成功。
pub const OK: i32 = 0;
/// 权限不足。
pub const EPERM: i32 = 1;
/// 指针不可读。
pub const EFAULT: i32 = 14;
/// 资源号或区间非法。
pub const EINVAL: i32 = 22;
/// 无上限哨兵。
pub const RLIM_INFINITY: u64 = u64::MAX;
/// 资源行数。
pub const RES_COUNT: usize = 16;
/// 指针样例数。
pub const GRID_PROBE: usize = 10;
/// 非法资源取法数。
pub const BAD_RES: usize = 4;
/// 矩阵行数。
pub const GRID_ROWS: usize = RES_COUNT + BAD_RES;
/// 矩阵格数。
pub const GRID_CELLS: usize = GRID_ROWS * GRID_PROBE;
/// 单格轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// struct rlimit 字节数。
pub const RLIM_BYTES: u64 = 16;
/// 用户页大小。
pub const PAGE: u64 = 4096;
/// 观察窗基址。
pub const WINDOW_BASE: u64 = 0x0000_7FF0_0000;
/// 观察窗上界（不含），三页。
pub const WINDOW_LIMIT: u64 = WINDOW_BASE + PAGE * 3;
/// 内核地址下界，落在其上的一律不可读。
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
/// CAP_SYS_RESOURCE 位。
pub const CAP_SYS_RESOURCE: u32 = 0x1;
/// CAP_SYS_NICE 位。
pub const CAP_SYS_NICE: u32 = 0x2;
/// 全部能力。
pub const CAPS_ALL: u32 = CAP_SYS_RESOURCE | CAP_SYS_NICE;

// ── 资源策略表 ────────────────────────────────────────────────────────
/// 一行资源策略。`ceiling` 是 max 的上限；`cap_bit` 是突破上限所需能力位，
/// 0 表示无需能力（此时 ceiling 恒为 RLIM_INFINITY，上限判断永不触发）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResSpec {
    pub res: i32,
    pub name: &'static str,
    pub ceiling: u64,
    pub cap_bit: u32,
}

/// 冻结行的构造函数（常量上下文用）。
pub const fn rs(res: i32, name: &'static str, ceiling: u64, cap_bit: u32) -> ResSpec {
    ResSpec { res, name, ceiling, cap_bit }
}

/// 16 行资源策略：0..=15 与 Linux 的 RLIMIT_* 号段对齐。
pub const RES_TABLE: [ResSpec; RES_COUNT] = [
    rs(0, "RLIMIT_CPU", RLIM_INFINITY, 0),
    rs(1, "RLIMIT_FSIZE", RLIM_INFINITY, 0),
    rs(2, "RLIMIT_DATA", RLIM_INFINITY, 0),
    rs(3, "RLIMIT_STACK", RLIM_INFINITY, 0),
    rs(4, "RLIMIT_CORE", RLIM_INFINITY, 0),
    rs(5, "RLIMIT_RSS", RLIM_INFINITY, 0),
    rs(6, "RLIMIT_NPROC", RLIM_INFINITY, 0),
    rs(7, "RLIMIT_NOFILE", 1_048_576, CAP_SYS_RESOURCE),
    rs(8, "RLIMIT_MEMLOCK", RLIM_INFINITY, 0),
    rs(9, "RLIMIT_AS", RLIM_INFINITY, 0),
    rs(10, "RLIMIT_LOCKS", RLIM_INFINITY, 0),
    rs(11, "RLIMIT_SIGPENDING", RLIM_INFINITY, 0),
    rs(12, "RLIMIT_MSGQUEUE", RLIM_INFINITY, 0),
    rs(13, "RLIMIT_NICE", 0, CAP_SYS_NICE),
    rs(14, "RLIMIT_RTPRIO", 0, CAP_SYS_NICE),
    rs(15, "RLIMIT_RTTIME", RLIM_INFINITY, 0),
];

/// 非法资源取法：上界越界、越界 +1、远越界、负数。
pub const BAD_RES_TABLE: [i32; BAD_RES] = [16, 17, 100, -1];

/// 按资源号线性查表；只有 0..=15 返回 Some。
pub fn lookup_res(res: i32) -> Option<ResSpec> {
    let mut i = 0usize;
    while i < RES_TABLE.len() {
        if let Some(row) = RES_TABLE.get(i).copied() {
            if row.res == res {
                return Some(row);
            }
        }
        i += 1;
    }
    None
}

/// 有上限的行数。
pub fn capped_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < RES_TABLE.len() {
        if let Some(row) = RES_TABLE.get(i) {
            if row.ceiling != RLIM_INFINITY {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 无上限的行数。
pub fn uncapped_count() -> u32 {
    RES_COUNT as u32 - capped_count()
}

/// 能力位是否满足；bit 为 0 表示不需要能力。
pub fn cap_ok(caps: u32, bit: u32) -> bool {
    bit == 0 || (caps & bit) != 0
}

// ── rlim 指针取法 ────────────────────────────────────────────────────
/// 一个指针样例：从 `addr` 起有多少字节可读（`readable`），以及期望结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCase {
    pub name: &'static str,
    pub addr: u64,
    pub readable: u32,
    pub want: i32,
    pub bytes: u64,
}

/// 冻结行构造函数。
pub const fn pc(name: &'static str, addr: u64, readable: u32, want: i32, bytes: u64) -> ProbeCase {
    ProbeCase { name, addr, readable, want, bytes }
}

/// 10 个指针样例：3 个整读、7 个坏读（部分读 3 个，零读 4 个）。
pub const PROBES: [ProbeCase; GRID_PROBE] = [
    pc("full-aligned", WINDOW_BASE, 16, OK, 16),
    pc("full-plus1", WINDOW_BASE + 1, 16, OK, 16),
    pc("full-cross-page", WINDOW_BASE + 4088, 16, OK, 16),
    pc("page-tail-fault", WINDOW_BASE + 4088, 8, EFAULT, 8),
    pc("second-field-fault", WINDOW_BASE + 64, 8, EFAULT, 8),
    pc("mid-partial", WINDOW_BASE + 128, 12, EFAULT, 12),
    pc("unmapped-hole", WINDOW_BASE + 512, 0, EFAULT, 0),
    pc("null", 0, 16, EFAULT, 0),
    pc("kernel", KERNEL_BASE, 16, EFAULT, 0),
    pc("wrapping", RLIM_INFINITY - 4, 16, EFAULT, 0),
];

/// 一次 rlim 读取的结果：状态与已落地字节数（脏字节记账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadOutcome {
    pub status: i32,
    pub bytes: u64,
}

impl ReadOutcome {
    /// 不可读，脏字节为 0。
    pub const fn fault0() -> ReadOutcome {
        ReadOutcome { status: EFAULT, bytes: 0 }
    }
}

/// 模拟 copy_from_user(struct rlimit)：越界/空/内核/回绕一律零读 EFAULT；
/// 窗内按 `readable` 与窗尾取小，满 16 字节才 OK。
pub fn read_rlim(addr: u64, readable: u32) -> ReadOutcome {
    if addr == 0 || addr >= KERNEL_BASE {
        return ReadOutcome::fault0();
    }
    let end = match addr.checked_add(RLIM_BYTES) {
        Some(e) => e,
        None => return ReadOutcome::fault0(),
    };
    if addr < WINDOW_BASE || end > WINDOW_LIMIT {
        return ReadOutcome::fault0();
    }
    let tail = (WINDOW_LIMIT - addr) as u32;
    let usable = if readable < tail { readable } else { tail };
    let bytes = if usable > RLIM_BYTES as u32 { RLIM_BYTES as u32 } else { usable };
    let status = if bytes == RLIM_BYTES as u32 { OK } else { EFAULT };
    ReadOutcome { status, bytes: bytes as u64 }
}

// ── 资源簿（被提交修改的状态） ────────────────────────────────────────
/// 16 槽资源簿；所有访问走 get/get_mut，无裸索引。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LimitBook {
    cur: [u64; RES_COUNT],
    max: [u64; RES_COUNT],
}

impl LimitBook {
    /// 初始态：cur 全 0，max 全无上限。
    pub const fn pristine() -> LimitBook {
        LimitBook { cur: [0u64; RES_COUNT], max: [RLIM_INFINITY; RES_COUNT] }
    }
    /// 读一槽。
    pub fn read(&self, idx: usize) -> Option<(u64, u64)> {
        let c = self.cur.get(idx).copied();
        let m = self.max.get(idx).copied();
        match (c, m) {
            (Some(cv), Some(mv)) => Some((cv, mv)),
            _ => None,
        }
    }
    /// 写一槽；槽号越界返回 false。
    pub fn store(&mut self, idx: usize, cur: u64, max: u64) -> bool {
        let c = self.cur.get_mut(idx);
        let m = self.max.get_mut(idx);
        match (c, m) {
            (Some(cv), Some(mv)) => {
                *cv = cur;
                *mv = max;
                true
            }
            _ => false,
        }
    }
    /// 两簿逐槽相等（用于证明 abort 无副作用）。
    pub fn same_as(&self, other: &LimitBook) -> bool {
        self.cur == other.cur && self.max == other.max
    }
}

// ── 两阶段事务 ────────────────────────────────────────────────────────
/// 事务相位。校验段只读；提交段才碰资源簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Validated,
    Probed,
    Checked,
    Committed,
    Aborted,
}

/// 单次事务计数与末态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Txn {
    pub phase: Phase,
    pub code: i32,
    pub attempts: u32,
    pub commits: u32,
    pub aborts: u32,
}

impl Txn {
    pub const fn new() -> Txn {
        Txn { phase: Phase::Idle, code: OK, attempts: 0, commits: 0, aborts: 0 }
    }
    /// 失败收尾：只记 abort，不回写任何状态。
    pub fn abort(&mut self, code: i32) {
        self.phase = Phase::Aborted;
        self.code = code;
        self.aborts = self.aborts.saturating_add(1);
    }
    /// 成功收尾。
    pub fn commit(&mut self) {
        self.phase = Phase::Committed;
        self.code = OK;
        self.commits = self.commits.saturating_add(1);
    }
}

/// 一次 setrlimit 请求：资源号、指针样例、待写值、能力掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetReq {
    pub res: i32,
    pub addr: u64,
    pub readable: u32,
    pub cur: u64,
    pub max: u64,
    pub caps: u32,
}

/// 四段串行判定。返回 errno；失败时资源簿逐槽不变。
pub fn attempt(book: &mut LimitBook, txn: &mut Txn, req: SetReq) -> i32 {
    txn.attempts = txn.attempts.saturating_add(1);
    txn.phase = Phase::Idle;

    let row = match lookup_res(req.res) {
        Some(r) => r,
        None => {
            txn.abort(EINVAL);
            return EINVAL;
        }
    };
    txn.phase = Phase::Validated;

    let read = read_rlim(req.addr, req.readable);
    if read.status != OK {
        txn.abort(EFAULT);
        return EFAULT;
    }
    txn.phase = Phase::Probed;

    if req.cur > req.max {
        txn.abort(EINVAL);
        return EINVAL;
    }

    if req.max > row.ceiling && !cap_ok(req.caps, row.cap_bit) {
        txn.abort(EPERM);
        return EPERM;
    }
    txn.phase = Phase::Checked;

    let idx = row.res as usize;
    if !book.store(idx, req.cur, req.max) {
        txn.abort(EINVAL);
        return EINVAL;
    }
    txn.commit();
    OK
}

/// 请求构造捷径（常量上下文可用）。
pub const fn mk(res: i32, addr: u64, readable: u32, cur: u64, max: u64, caps: u32) -> SetReq {
    SetReq { res, addr, readable, cur, max, caps }
}

/// 单发一试：新簿 + 新事务，返回 (errno, 事务)。
pub fn one(req: SetReq) -> (i32, Txn) {
    let mut book = LimitBook::pristine();
    let mut txn = Txn::new();
    let got = attempt(&mut book, &mut txn, req);
    (got, txn)
}

/// 矩阵一格的期望码：非法资源恒 EINVAL，否则等于指针读状态（好指针值域合法）。
pub fn cell_want(valid: bool, probe_status: i32) -> i32 {
    if valid {
        probe_status
    } else {
        EINVAL
    }
}

// ── 账本 ──────────────────────────────────────────────────────────────
/// 注入账：injected == detected + escaped，且等于四个码桶之和。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommitLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub commits: u32,
    pub aborts: u32,
    pub einval: u32,
    pub efault: u32,
    pub eperm: u32,
    pub ok: u32,
}

impl CommitLedger {
    pub const fn zero() -> CommitLedger {
        CommitLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            commits: 0,
            aborts: 0,
            einval: 0,
            efault: 0,
            eperm: 0,
            ok: 0,
        }
    }
    /// 归并一次事务，并记一笔记账（expected/got 决定检出与逃逸）。
    pub fn absorb(&mut self, txn: &Txn, expected: i32, got: i32) {
        self.injected = self.injected.saturating_add(txn.attempts);
        self.commits = self.commits.saturating_add(txn.commits);
        self.aborts = self.aborts.saturating_add(txn.aborts);
        if got == expected {
            self.detected = self.detected.saturating_add(txn.attempts);
        } else {
            self.escaped = self.escaped.saturating_add(txn.attempts);
        }
        if got == EINVAL {
            self.einval = self.einval.saturating_add(1);
        } else if got == EFAULT {
            self.efault = self.efault.saturating_add(1);
        } else if got == EPERM {
            self.eperm = self.eperm.saturating_add(1);
        } else if got == OK {
            self.ok = self.ok.saturating_add(1);
        }
    }
    /// 两阶段守恒：尝试 = 提交 + 中止；注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.injected == self.commits.saturating_add(self.aborts)
            && self.injected == self.detected.saturating_add(self.escaped)
    }
    /// 桶和。
    pub fn buckets(&self) -> u32 {
        self.einval
            .saturating_add(self.efault)
            .saturating_add(self.eperm)
            .saturating_add(self.ok)
    }
}

// ── 20 × 10 矩阵 ─────────────────────────────────────────────────────
/// 矩阵行取法：前 16 行按 RES_TABLE，后 4 行非法号。
pub fn row_res(i: usize) -> i32 {
    if i < RES_TABLE.len() {
        match RES_TABLE.get(i) {
            Some(r) => r.res,
            None => -1,
        }
    } else {
        let k = i.saturating_sub(RES_TABLE.len());
        match BAD_RES_TABLE.get(k) {
            Some(v) => *v,
            None => -1,
        }
    }
}

/// 逐格 10 轮注入，返回 (满命中格数, 账本)。
pub fn run_pointer_matrix() -> (u32, CommitLedger) {
    let mut hit_cells = 0u32;
    let mut ledger = CommitLedger::zero();
    let mut ri = 0usize;
    while ri < GRID_ROWS {
        let res = row_res(ri);
        let row = lookup_res(res);
        let mut pi = 0usize;
        while pi < PROBES.len() {
            if let Some(p) = PROBES.get(pi).copied() {
                let expected = cell_want(row.is_some(), p.want);
                let (mut hit, mut round) = (true, 0u32);
                while round < INJECT_ROUNDS {
                    let (got, txn) = one(mk(res, p.addr, p.readable, 0, 0, 0));
                    ledger.absorb(&txn, expected, got);
                    if got != expected {
                        hit = false;
                    }
                    round = round.saturating_add(1);
                }
                if hit {
                    hit_cells = hit_cells.saturating_add(1);
                }
            }
            pi += 1;
        }
        ri += 1;
    }
    (hit_cells, ledger)
}

/// 矩阵里期望码为 `code` 的格数。
pub fn count_cells(code: i32) -> u32 {
    let mut n = 0u32;
    let mut ri = 0usize;
    while ri < GRID_ROWS {
        let valid = lookup_res(row_res(ri)).is_some();
        let mut pi = 0usize;
        while pi < PROBES.len() {
            if let Some(p) = PROBES.get(pi).copied() {
                if cell_want(valid, p.want) == code {
                    n = n.saturating_add(1);
                }
            }
            pi += 1;
        }
        ri += 1;
    }
    n
}

// ── 能力网格：3 个有上限资源 × 2 个超限值 × 3 组掩码 ────────────────
/// 能力网格行：资源号与两个超限 max 值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapRow {
    pub res: i32,
    pub v1: u64,
    pub v2: u64,
}

pub const CAP_ROWS: [CapRow; 3] = [
    CapRow { res: 7, v1: 1_048_801, v2: 2_097_152 },
    CapRow { res: 13, v1: 1, v2: 39 },
    CapRow { res: 14, v1: 1, v2: 50 },
];

/// 三组掩码：无、仅资源能力、全部能力。
pub const CAP_MASKS: [u32; 3] = [0, CAP_SYS_RESOURCE, CAPS_ALL];

/// 单格期望：max 未超上限成功；超限时仅当所需能力位具备才成功。
pub fn cap_want(res: i32, max: u64, caps: u32) -> i32 {
    match lookup_res(res) {
        Some(row) => {
            if max > row.ceiling && !cap_ok(caps, row.cap_bit) {
                EPERM
            } else {
                OK
            }
        }
        None => EINVAL,
    }
}

/// 能力网格逐格 10 轮，返回 (满命中格数, 账本)。
pub fn run_capability_grid() -> (u32, CommitLedger) {
    let mut hit_cells = 0u32;
    let mut ledger = CommitLedger::zero();
    let mut ri = 0usize;
    while ri < CAP_ROWS.len() {
        if let Some(row) = CAP_ROWS.get(ri).copied() {
            let vals = [row.v1, row.v2];
            let mut vi = 0usize;
            while vi < vals.len() {
                let max = match vals.get(vi) { Some(v) => *v, None => 0 };
                let mut mi = 0usize;
                while mi < CAP_MASKS.len() {
                    let caps = match CAP_MASKS.get(mi) { Some(c) => *c, None => 0 };
                    let expected = cap_want(row.res, max, caps);
                    let (mut hit, mut round) = (true, 0u32);
                    while round < INJECT_ROUNDS {
                        let (got, txn) = one(mk(row.res, WINDOW_BASE, 16, 0, max, caps));
                        ledger.absorb(&txn, expected, got);
                        if got != expected {
                            hit = false;
                        }
                        round = round.saturating_add(1);
                    }
                    if hit {
                        hit_cells = hit_cells.saturating_add(1);
                    }
                    mi += 1;
                }
                vi += 1;
            }
        }
        ri += 1;
    }
    (hit_cells, ledger)
}

/// 能力网格格数。
pub fn cap_grid_cells() -> u32 {
    (CAP_ROWS.len() * 2 * CAP_MASKS.len()) as u32
}

// ── 倒置网格：cur > max 恒 EINVAL ────────────────────────────────────
/// 三组倒置对：(cur, max)。
pub const INVERTED: [(u64, u64); 3] = [(4096, 1024), (RLIM_INFINITY, 0), (1, 0)];

/// 倒置网格：16 资源 × 3 对 × 10 轮，全 EINVAL。
pub fn run_inversion_grid() -> (u32, CommitLedger) {
    let mut hit_cells = 0u32;
    let mut ledger = CommitLedger::zero();
    let mut ri = 0usize;
    while ri < RES_TABLE.len() {
        if let Some(row) = RES_TABLE.get(ri).copied() {
            let mut pi = 0usize;
            while pi < INVERTED.len() {
                if let Some((cur, max)) = INVERTED.get(pi).copied() {
                    let (mut hit, mut round) = (true, 0u32);
                    while round < INJECT_ROUNDS {
                        let (got, txn) = one(mk(row.res, WINDOW_BASE, 16, cur, max, CAPS_ALL));
                        ledger.absorb(&txn, EINVAL, got);
                        if got != EINVAL {
                            hit = false;
                        }
                        round = round.saturating_add(1);
                    }
                    if hit {
                        hit_cells = hit_cells.saturating_add(1);
                    }
                }
                pi += 1;
            }
        }
        ri += 1;
    }
    (hit_cells, ledger)
}

/// 倒置网格格数。
pub fn inversion_cells() -> u32 {
    (RES_TABLE.len() * INVERTED.len()) as u32
}

// ── errno 表 ─────────────────────────────────────────────────────────
/// errno 对照：Linux 号与 Windows 映射号。
pub struct ErrnoRow {
    pub name: &'static str,
    pub linux: i32,
    pub win: i32,
}

pub const ERRNO_TABLE: [ErrnoRow; 3] = [
    ErrnoRow { name: "EPERM", linux: EPERM, win: 5 },
    ErrnoRow { name: "EFAULT", linux: EFAULT, win: 998 },
    ErrnoRow { name: "EINVAL", linux: EINVAL, win: 87 },
];

/// errno 是否在表内且为正值。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.linux == code && code > 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 诊断码 ────────────────────────────────────────────────────────────
/// 12 个段内闭集诊断码。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetrlimitCode {
    BadResource,
    BadPointer,
    PartialRead,
    InvertedLimits,
    NoCapRaise,
    NoFileCeiling,
    NiceCeiling,
    RtprioCeiling,
    CommitOk,
    AbortClean,
    UnboundedMax,
    SlotOutOfRange,
}

impl SetrlimitCode {
    /// 低 16 位相对码（与相邻单的段基址高位对齐）。
    pub fn code(self) -> u16 {
        SEG_BASE as u16 + 1 + self as u16
    }
    /// 全宽码值，落在 SEG_BASE+1..=SEG_BASE+12。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 判据文案，不得为空。
    pub fn detail(self) -> &'static str {
        match self {
            SetrlimitCode::BadResource => "资源号不在 0..=15，先于指针返回 EINVAL",
            SetrlimitCode::BadPointer => "rlim 指针整体不可读，零脏字节 EFAULT",
            SetrlimitCode::PartialRead => "rlim 只读到一部分，脏字节非零仍 EFAULT",
            SetrlimitCode::InvertedLimits => "cur 大于 max，区间倒置 EINVAL",
            SetrlimitCode::NoCapRaise => "max 越过上限且缺所需能力，EPERM",
            SetrlimitCode::NoFileCeiling => "NOFILE 上限 1048576，需 CAP_SYS_RESOURCE",
            SetrlimitCode::NiceCeiling => "NICE 上限 0，需 CAP_SYS_NICE",
            SetrlimitCode::RtprioCeiling => "RTPRIO 上限 0，需 CAP_SYS_NICE",
            SetrlimitCode::CommitOk => "四段全过，提交资源簿并返回 0",
            SetrlimitCode::AbortClean => "任一阶段失败即中止，资源簿逐槽不变",
            SetrlimitCode::UnboundedMax => "13 行无上限，max 判断永不触发",
            SetrlimitCode::SlotOutOfRange => "资源簿槽号越界，拒绝提交",
        }
    }
    pub const ALL: [SetrlimitCode; 12] = [
        SetrlimitCode::BadResource,
        SetrlimitCode::BadPointer,
        SetrlimitCode::PartialRead,
        SetrlimitCode::InvertedLimits,
        SetrlimitCode::NoCapRaise,
        SetrlimitCode::NoFileCeiling,
        SetrlimitCode::NiceCeiling,
        SetrlimitCode::RtprioCeiling,
        SetrlimitCode::CommitOk,
        SetrlimitCode::AbortClean,
        SetrlimitCode::UnboundedMax,
        SetrlimitCode::SlotOutOfRange,
    ];
}

// ── 指纹 ──────────────────────────────────────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
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

fn push_i32(out: &mut Vec<u8>, v: i32) {
    let raw = (v as u32).to_le_bytes();
    let mut i = 0usize;
    while i < raw.len() {
        if let Some(b) = raw.get(i) {
            out.push(*b);
        }
        i += 1;
    }
}

/// 规范语料：20 行 × 10 指针 + 3 行 × 2 值 × 3 掩码 + 16 资源 × 3 倒置对。
pub fn corpus_fingerprint() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    let mut ri = 0usize;
    while ri < GRID_ROWS {
        let res = row_res(ri);
        let mut pi = 0usize;
        while pi < PROBES.len() {
            if let Some(p) = PROBES.get(pi).copied() {
                let (got, txn) = one(mk(res, p.addr, p.readable, 0, 0, 0));
                push_i32(&mut buf, got);
                push_i32(&mut buf, txn.commits as i32);
                push_i32(&mut buf, txn.aborts as i32);
            }
            pi += 1;
        }
        ri += 1;
    }
    let mut ci = 0usize;
    while ci < CAP_ROWS.len() {
        if let Some(row) = CAP_ROWS.get(ci).copied() {
            let vals = [row.v1, row.v2];
            let mut vi = 0usize;
            while vi < vals.len() {
                let max = match vals.get(vi) { Some(v) => *v, None => 0 };
                let mut mi = 0usize;
                while mi < CAP_MASKS.len() {
                    let caps = match CAP_MASKS.get(mi) { Some(c) => *c, None => 0 };
                    push_i32(&mut buf, cap_want(row.res, max, caps));
                    mi += 1;
                }
                vi += 1;
            }
        }
        ci += 1;
    }
    let mut ii = 0usize;
    while ii < RES_TABLE.len() {
        if let Some(row) = RES_TABLE.get(ii).copied() {
            let mut pi = 0usize;
            while pi < INVERTED.len() {
                if let Some((cur, max)) = INVERTED.get(pi).copied() {
                    let (got, _) = one(mk(row.res, WINDOW_BASE, 16, cur, max, CAPS_ALL));
                    push_i32(&mut buf, got);
                }
                pi += 1;
            }
        }
        ii += 1;
    }
    fnv1a(&buf)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9329 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9329_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9329");
    let (hit_cells, ledger) = run_pointer_matrix();
    let (cap_hit, cap_ledger) = run_capability_grid();
    let (inv_hit, inv_ledger) = run_inversion_grid();
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();

    let seg_ok = SEG_BASE == 0x7BFC0
        && SEG_BASE % 0x40 == 0
        && SEG_BASE >= 0x7B000
        && SEG_BASE + SEG_WINDOW <= 0x7C000;
    s.add("段位守卫：0x7BFC0 起 0x40 窗口不与 F9330 重叠", seg_ok,
          "段基址必须 0x40 对齐且本模块不得覆盖相邻单的段");

    let mut table_ok = RES_TABLE.len() == RES_COUNT;
    let mut ti = 0usize;
    while ti < RES_TABLE.len() {
        if let Some(row) = RES_TABLE.get(ti) {
            if row.name.is_empty() || row.res != ti as i32 {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("资源表：16 行 res==下标、名称非空、id 唯一", table_ok,
          "资源表错位会让 NOFILE 的上限套到别的资源上");

    s.add("策略分布：3 行有上限（NOFILE/NICE/RTPRIO）/ 13 行无上限",
          capped_count() == 3 && uncapped_count() == 13
              && lookup_res(7).map(|r| r.ceiling) == Some(1_048_576)
              && lookup_res(13).map(|r| r.cap_bit) == Some(CAP_SYS_NICE)
              && lookup_res(14).map(|r| r.cap_bit) == Some(CAP_SYS_NICE),
          "能力位与上限必须成对出现，缺一即策略失真");

    s.add("值域：0..=15 合法，16/17/100/-1 一律 EINVAL",
          lookup_res(15).is_some() && lookup_res(0).is_some()
              && lookup_res(16).is_none() && lookup_res(17).is_none()
              && lookup_res(100).is_none() && lookup_res(-1).is_none(),
          "资源号判定先于指针，坏号配坏指针报 EINVAL");

    let mut probe_ok = PROBES.len() == GRID_PROBE;
    let mut ok_probes = 0u32;
    let mut bad_probes = 0u32;
    let mut pi = 0usize;
    while pi < PROBES.len() {
        if let Some(p) = PROBES.get(pi).copied() {
            let got = read_rlim(p.addr, p.readable);
            if got.status != p.want || got.bytes != p.bytes {
                probe_ok = false;
            }
            if p.want == OK {
                ok_probes = ok_probes.saturating_add(1);
            } else {
                bad_probes = bad_probes.saturating_add(1);
            }
        }
        pi += 1;
    }
    s.add("指针表：10 样例 3 好 7 坏，逐样例码与脏字节复算一致", probe_ok
              && ok_probes == 3 && bad_probes == 7,
          "同一个 EFAULT 必须由脏字节数区分整读失败与部分读");

    s.add("读账：整读 16 字节 / 部分读 8 字节 / 零读 0 字节",
          read_rlim(WINDOW_BASE, 16).bytes == 16
              && read_rlim(WINDOW_BASE + 64, 8).bytes == 8
              && read_rlim(0, 16).bytes == 0
              && read_rlim(KERNEL_BASE, 16).bytes == 0
              && read_rlim(RLIM_INFINITY - 4, 16).bytes == 0,
          "空、内核、回绕三类越界必须零脏字节");

    let order_a = one(mk(16, 0, 0, 9, 1, 0)).0;
    let order_b = one(mk(0, 0, 0, 9, 1, 0)).0;
    s.add("次序①：坏资源先于坏指针（EINVAL），坏指针先于倒置（EFAULT）",
          order_a == EINVAL && order_b == EFAULT,
          "四段串行次序不可互换，否则 errno 口径整体漂移");

    let mut invert_ok = true;
    let mut ii = 0usize;
    while ii < INVERTED.len() {
        if let Some((cur, max)) = INVERTED.get(ii).copied() {
            if one(mk(0, WINDOW_BASE, 16, cur, max, CAPS_ALL)).0 != EINVAL {
                invert_ok = false;
            }
        }
        ii += 1;
    }
    s.add("区间倒置：3 组 cur>max 全 EINVAL（即使全能力）", invert_ok,
          "倒置是无条件拒绝，不能被能力位绕过");

    let eperm_a = one(mk(7, WINDOW_BASE, 16, 0, 2_097_152, 0)).0;
    let eperm_b = one(mk(7, WINDOW_BASE, 16, 0, 2_097_152, CAP_SYS_RESOURCE)).0;
    let nice_a = one(mk(13, WINDOW_BASE, 16, 0, 39, CAP_SYS_RESOURCE)).0;
    let nice_b = one(mk(13, WINDOW_BASE, 16, 0, 39, CAP_SYS_NICE)).0;
    s.add("能力分工：NOFILE 认 SYS_RESOURCE，NICE 认 SYS_NICE，互不顶替",
          eperm_a == EPERM && eperm_b == OK && nice_a == EPERM && nice_b == OK,
          "能力位串用会让越权改限被静默放行");

    let abort_clean = {
        let mut b = LimitBook::pristine();
        let before = b;
        let mut t = Txn::new();
        let got = attempt(&mut b, &mut t, mk(7, WINDOW_BASE, 16, 0, 2_097_152, 0));
        got == EPERM && b.same_as(&before) && t.aborts == 1 && t.commits == 0
            && t.phase == Phase::Aborted
    };
    s.add("两阶段：EPERM 中止后资源簿逐槽不变", abort_clean,
          "abort 必须无副作用，否则失败请求会留下脏限值");

    let commit_mutates = {
        let mut b = LimitBook::pristine();
        let before = b;
        let mut t = Txn::new();
        let got = attempt(&mut b, &mut t, mk(7, WINDOW_BASE, 16, 512, 1024, 0));
        got == OK && !b.same_as(&before) && b.read(7) == Some((512, 1024))
            && t.commits == 1 && t.aborts == 0 && t.phase == Phase::Committed
    };
    s.add("两阶段：提交后槽 7 = (512,1024)，commits 1 / aborts 0", commit_mutates,
          "提交段必须真的写回，且相位落在 Committed");

    s.add("矩阵格数：EINVAL 40 / EFAULT 112 / OK 48 = 200",
          count_cells(EINVAL) == 40 && count_cells(EFAULT) == 112
              && count_cells(OK) == 48
              && count_cells(EINVAL) + count_cells(EFAULT) + count_cells(OK)
                  == GRID_CELLS as u32,
          "非法资源 40 格即使指针可读也必须 EINVAL");

    s.add("矩阵注入：200 格 × 10 轮全命中（共 2000 次）",
          hit_cells == GRID_CELLS as u32
              && ledger.injected == (GRID_CELLS as u32) * INJECT_ROUNDS,
          "任一格漂移都说明资源/指针两轴次序脱钩");

    s.add("矩阵账：2000 = 480 提交 + 1520 中止，逃逸 0，四桶和 2000",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 2000 && ledger.commits == 480
              && ledger.aborts == 1520 && ledger.buckets() == ledger.injected
              && ledger.einval == 400 && ledger.efault == 1120 && ledger.ok == 480,
          "提交+中止必须覆盖全部尝试，分桶漏记立刻暴露");

    s.add("坏指针单测：10 样例 × 10 轮，70 EFAULT / 30 OK 逐样例 10/10",
          ledger.efault == 1120
              && bad_probes == 7 && ok_probes == 3
              && PROBES.len() * INJECT_ROUNDS as usize == 100,
          "每格 10/10 检出是 J1 判据的硬要求");

    s.add("能力网格：18 格 × 10 轮全命中，EPERM 100 / OK 80",
          cap_hit == cap_grid_cells() && cap_ledger.injected == 180
              && cap_ledger.eperm == 100 && cap_ledger.ok == 80
              && cap_ledger.escaped == 0 && cap_ledger.buckets() == 180,
          "超限请求在三种掩码下必须两两可区分");

    s.add("倒置网格：48 格 × 10 轮全 EINVAL（共 480 次）",
          inv_hit == inversion_cells() && inv_ledger.injected == 480
              && inv_ledger.einval == 480 && inv_ledger.commits == 0
              && inv_ledger.aborts == 480 && inv_ledger.escaped == 0,
          "倒置必须先于上限与能力判断，且绝不提交");

    s.add("errno 对照：EPERM=1 / EFAULT=14 / EINVAL=22 表内可复算",
          errno_known(EPERM) && errno_known(EFAULT) && errno_known(EINVAL)
              && !errno_known(OK) && !errno_known(-EFAULT) && ERRNO_TABLE.len() == 3,
          "错误码只认表内正值，负值口径由调用方统一取负");

    let mut code_ok = true;
    let mut window_ok = true;
    let mut text_ok = true;
    let mut ci = 0usize;
    while ci < SetrlimitCode::ALL.len() {
        if let Some(a) = SetrlimitCode::ALL.get(ci) {
            if a.detail().is_empty() {
                text_ok = false;
            }
            if a.val() <= SEG_BASE || a.val() > SEG_BASE + 0x0C {
                window_ok = false;
            }
            let mut cj = ci + 1;
            while cj < SetrlimitCode::ALL.len() {
                if let Some(b) = SetrlimitCode::ALL.get(cj) {
                    if a.val() == b.val() {
                        code_ok = false;
                    }
                }
                cj += 1;
            }
        }
        ci += 1;
    }
    s.add("诊断码：12 个码值唯一、全部落窗、文案非空",
          SetrlimitCode::ALL.len() == 12 && code_ok && window_ok && text_ok,
          "段内闭集越窗会与相邻单撞码，文案不得为空");

    s.add("双跑一致：266 次规范判定指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    let grid_text = format!("矩阵 {} 格 × {} 轮，命中 {} 格", GRID_CELLS, INJECT_ROUNDS, hit_cells);
    s.add_dyn("矩阵快照", hit_cells == GRID_CELLS as u32, &grid_text);

    let book_text = format!("注入 {} / 提交 {} / 中止 {} / 逃逸 {}",
                            ledger.injected, ledger.commits, ledger.aborts, ledger.escaped);
    s.add_dyn("账本快照", ledger.conserved() && ledger.escaped == 0, &book_text);

    let cap_text = format!("能力 {}/{} 格，倒置 {}/{} 格，指纹 0x{:08X}",
                           cap_hit, cap_grid_cells(), inv_hit, inversion_cells(), fp_a);
    s.add_dyn("网格快照", cap_hit == cap_grid_cells() && inv_hit == inversion_cells(),
              &cap_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_policy_is_frozen() {
        assert_eq!(RES_TABLE.len(), RES_COUNT);
        assert_eq!(capped_count(), 3);
        assert_eq!(uncapped_count(), 13);
        assert_eq!(lookup_res(7).map(|r| r.ceiling), Some(1_048_576));
        assert_eq!(lookup_res(13).map(|r| r.cap_bit), Some(CAP_SYS_NICE));
        assert_eq!(lookup_res(16).is_none(), true);
        assert_eq!(lookup_res(-1).is_none(), true);
    }

    #[test]
    fn probe_table_matches_read_model() {
        assert_eq!(PROBES.len(), GRID_PROBE);
        let mut i = 0usize;
        while i < PROBES.len() {
            if let Some(p) = PROBES.get(i).copied() {
                let got = read_rlim(p.addr, p.readable);
                assert_eq!((got.status, got.bytes), (p.want, p.bytes));
            }
            i += 1;
        }
        assert_eq!(read_rlim(WINDOW_BASE, 16), ReadOutcome { status: OK, bytes: 16 });
        assert_eq!(read_rlim(WINDOW_BASE + 64, 8), ReadOutcome { status: EFAULT, bytes: 8 });
        assert_eq!(read_rlim(WINDOW_BASE + 128, 12), ReadOutcome { status: EFAULT, bytes: 12 });
    }

    #[test]
    fn zero_read_faults() {
        assert_eq!(read_rlim(0, 16), ReadOutcome { status: EFAULT, bytes: 0 });
        assert_eq!(read_rlim(KERNEL_BASE, 16), ReadOutcome { status: EFAULT, bytes: 0 });
        assert_eq!(read_rlim(RLIM_INFINITY - 4, 16), ReadOutcome { status: EFAULT, bytes: 0 });
        assert_eq!(read_rlim(WINDOW_BASE + 512, 0), ReadOutcome { status: EFAULT, bytes: 0 });
    }

    #[test]
    fn resource_precedes_pointer_and_inversion() {
        let mut b = LimitBook::pristine();
        let mut t = Txn::new();
        let bad_res = SetReq { res: 16, addr: 0, readable: 0, cur: 9, max: 1, caps: 0 };
        assert_eq!(attempt(&mut b, &mut t, bad_res), EINVAL);
        let bad_ptr = SetReq { res: 0, addr: 0, readable: 0, cur: 9, max: 1, caps: 0 };
        assert_eq!(attempt(&mut b, &mut t, bad_ptr), EFAULT);
        let inverted = SetReq { res: 0, addr: WINDOW_BASE, readable: 16,
                                cur: 9, max: 1, caps: CAPS_ALL };
        assert_eq!(attempt(&mut b, &mut t, inverted), EINVAL);
    }

    #[test]
    fn inversion_never_commits() {
        let mut b = LimitBook::pristine();
        let before = b;
        let mut i = 0usize;
        while i < INVERTED.len() {
            if let Some((cur, max)) = INVERTED.get(i).copied() {
                let mut t = Txn::new();
                let req = SetReq { res: 3, addr: WINDOW_BASE, readable: 16,
                                   cur, max, caps: CAPS_ALL };
                assert_eq!(attempt(&mut b, &mut t, req), EINVAL);
                assert_eq!(t.commits, 0);
                assert_eq!(t.aborts, 1);
            }
            i += 1;
        }
        assert_eq!(b.same_as(&before), true);
    }

    #[test]
    fn capability_bits_do_not_cross() {
        let case = |res: i32, max: u64, caps: u32| {
            let mut b = LimitBook::pristine();
            let mut t = Txn::new();
            attempt(&mut b, &mut t,
                    SetReq { res, addr: WINDOW_BASE, readable: 16, cur: 0, max, caps })
        };
        assert_eq!(case(7, 2_097_152, 0), EPERM);
        assert_eq!(case(7, 2_097_152, CAP_SYS_RESOURCE), OK);
        assert_eq!(case(7, 2_097_152, CAP_SYS_NICE), EPERM);
        assert_eq!(case(13, 39, CAP_SYS_RESOURCE), EPERM);
        assert_eq!(case(13, 39, CAP_SYS_NICE), OK);
        assert_eq!(case(14, 50, CAP_SYS_NICE), OK);
        assert_eq!(case(0, RLIM_INFINITY, 0), OK);
    }

    #[test]
    fn abort_leaves_book_untouched() {
        let mut b = LimitBook::pristine();
        let before = b;
        let mut t = Txn::new();
        let req = SetReq { res: 7, addr: WINDOW_BASE, readable: 16,
                           cur: 0, max: 2_097_152, caps: 0 };
        assert_eq!(attempt(&mut b, &mut t, req), EPERM);
        assert_eq!(b.same_as(&before), true);
        assert_eq!(t.phase, Phase::Aborted);
        assert_eq!((t.commits, t.aborts), (0, 1));
    }

    #[test]
    fn commit_writes_slot() {
        let mut b = LimitBook::pristine();
        let mut t = Txn::new();
        let req = SetReq { res: 7, addr: WINDOW_BASE, readable: 16,
                           cur: 512, max: 1024, caps: 0 };
        assert_eq!(attempt(&mut b, &mut t, req), OK);
        assert_eq!(b.read(7), Some((512, 1024)));
        assert_eq!(b.read(6), Some((0, RLIM_INFINITY)));
        assert_eq!((t.commits, t.aborts, t.phase), (1, 0, Phase::Committed));
    }

    #[test]
    fn matrix_counts_and_hits() {
        assert_eq!(GRID_CELLS, 200);
        assert_eq!(count_cells(EINVAL), 40);
        assert_eq!(count_cells(EFAULT), 112);
        assert_eq!(count_cells(OK), 48);
        let (hit, ledger) = run_pointer_matrix();
        assert_eq!(hit, 200);
        assert_eq!(ledger.injected, 2000);
        assert_eq!(ledger.commits, 480);
        assert_eq!(ledger.aborts, 1520);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.buckets(), 2000);
    }

    #[test]
    fn capability_and_inversion_grids() {
        assert_eq!(cap_grid_cells(), 18);
        let (ch, cl) = run_capability_grid();
        assert_eq!(ch, 18);
        assert_eq!((cl.injected, cl.eperm, cl.ok), (180, 100, 80));
        assert_eq!(inversion_cells(), 48);
        let (ih, il) = run_inversion_grid();
        assert_eq!(ih, 48);
        assert_eq!((il.injected, il.einval, il.commits, il.aborts), (480, 480, 0, 480));
    }

    #[test]
    fn errno_and_codes_are_closed() {
        assert_eq!(errno_known(EPERM), true);
        assert_eq!(errno_known(EFAULT), true);
        assert_eq!(errno_known(EINVAL), true);
        assert_eq!(errno_known(0), false);
        assert_eq!(SetrlimitCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < SetrlimitCode::ALL.len() {
            if let Some(a) = SetrlimitCode::ALL.get(i) {
                assert_eq!(a.detail().is_empty(), false);
                assert_eq!(a.val() > SEG_BASE, true);
                assert_eq!(a.val() <= SEG_BASE + 0x0C, true);
                assert_eq!(a.code() > (SEG_BASE as u16), true);
                let mut j = i + 1;
                while j < SetrlimitCode::ALL.len() {
                    if let Some(b) = SetrlimitCode::ALL.get(j) {
                        assert_eq!(a.val() == b.val(), false);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9329_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
