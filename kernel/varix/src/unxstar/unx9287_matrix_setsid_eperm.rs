//! UNX-F9287 setsid EPERM 矩阵（会话首规则联 B18） · 0x7B540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9287 逐字）：
//!   UNX-F9287-J1 会话首注入判据过，全 EPERM 格过，与 B18 选举语义对账
//!   一致格过。
//!
//! 设计要点：
//!   ① setsid(2) 的硬拒绝只有一条来源：调用者已经是进程组首（pgid == pid）。
//!      内核在这一步直接回 EPERM，不做会话登记，也不碰控制终端；本模型把
//!      「组首」判定放在身份校验之后、容量判定之前，冻结次序为：
//!      零身份(EINVAL) → 组首(EPERM) → 已是会话首(EPERM) → 容量(ENOMEM) →
//!      登记成功。任何重排都会让「满表 + 组首」格返回 ENOMEM 而不是 EPERM，
//!      该格因此单独作为判序哨兵。
//!   ② 会话首（session leader）与进程组首（pgrp leader）在本模型里是两个
//!      独立字段：sid == pid 意味着调用者已是某会话首，它必然同时是组首，
//!      但反向不成立。矩阵把两类调用者分开取证，避免把「已是会话首」误并
//!      进普通组首格而掩盖 sid 判据。
//!   ③ 联 B18 选举：选举器只接受「愿意参选、pid 非零、且不是组首」的候选，
//!      多候选时取最小 pid。对账等式为
//!        setsid 返回 Ok  ⇔  elect(单候选集合) == Some(pid)
//!      逐格复算；只要有一格两侧不等价，就说明「会话首规则」在选举侧漂移。
//!   ④ 会话表容量 6 槽：插入只在成功路径写槽，失败只累加 rejects，绝不占用
//!      槽位；表满时后续调用回 ENOMEM，且不允许覆盖旧会话。
//!   ⑤ 注入 12 格 × 10 轮 = 120 次，每轮用全新空表，账本满足
//!      injected == detected + escaped 且 escaped == 0；单轮除错误码外还要
//!      复算选举对账和槽位不变量，三者全中才算检出。
//!   ⑥ 诊断码段内闭集 12 个（0x7B541..=0x7B54C），码值两两唯一；段基址按
//!      0x40 对齐且独占 0x7B540..=0x7B57F 窗口。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9287 独占 0x7B540..=0x7B57F）。
pub const SEG_BASE: u32 = 0x7B540;
/// 段窗口上界（含）。
pub const SEG_END: u32 = 0x7B57F;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;

/// 会话表容量：6 槽足够覆盖「满表 + 组首」判序哨兵。
pub const MAX_SESSIONS: usize = 6;
/// 矩阵格数。
pub const CELLS: usize = 12;
/// 每格注入轮数。
pub const ROUNDS: u32 = 10;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。码值 = SEG_BASE + 变体序号（1 起），落 0x7B541..=0x7B54C。
///
/// 用 u32 而非 u16：段基址 0x7B540 已超出 u16 值域，码值必须能表达
/// 「段基址 + 序号」，否则段内定位失效（合同里的段值优先于位宽字面）。
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetsidCode {
    GroupLeader = 1,
    AlreadyHead = 2,
    ZeroPid = 3,
    TableFull = 4,
    TtyNotDetached = 5,
    ElectionDrift = 6,
    LedgerLeak = 7,
    DoubleRun = 8,
    SegmentSkew = 9,
    MatrixDrift = 10,
    TicketMismatch = 11,
    OrderSwap = 12,
}

impl SetsidCode {
    /// 段内码值（SEG_BASE + 序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SetsidCode::GroupLeader => "组首被拒：调用者 pgid 等于 pid，setsid 必回 EPERM；请先 fork 子进程或改由非组首进程建会话",
            SetsidCode::AlreadyHead => "已是会话首被拒：sid 等于 pid 时不得再次 setsid；请检查调用点是否重复建会话",
            SetsidCode::ZeroPid => "零身份被拒：pid 为 0 的调用者不具备建会话资格；请先绑定真实进程标识",
            SetsidCode::TableFull => "会话表已满：6 个槽位全部占用；请先回收旧会话再申请",
            SetsidCode::TtyNotDetached => "控制终端未剥离：新会话必须无控制终端；请确认登记时清除了 tty 标志",
            SetsidCode::ElectionDrift => "选举对账漂移：setsid 结果与 B18 选举不等价；请核对组首排除规则",
            SetsidCode::LedgerLeak => "注入账不守恒：detected+escaped 不等于 injected；请检查漏记路径",
            SetsidCode::DoubleRun => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            SetsidCode::SegmentSkew => "段位越界：段基址必须 0x40 对齐且落在本批窗口",
            SetsidCode::MatrixDrift => "矩阵漂移：实测错误码与冻结表不符；请勿改动期望列",
            SetsidCode::TicketMismatch => "票号不符：EPERM 格计数与冻结值不一致；请核对格表",
            SetsidCode::OrderSwap => "判定次序被对调：组首 EPERM 必须先于容量 ENOMEM；请重排校验",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [SetsidCode; 12] = [
        SetsidCode::GroupLeader,
        SetsidCode::AlreadyHead,
        SetsidCode::ZeroPid,
        SetsidCode::TableFull,
        SetsidCode::TtyNotDetached,
        SetsidCode::ElectionDrift,
        SetsidCode::LedgerLeak,
        SetsidCode::DoubleRun,
        SetsidCode::SegmentSkew,
        SetsidCode::MatrixDrift,
        SetsidCode::TicketMismatch,
        SetsidCode::OrderSwap,
    ];
}

// ── 调用者模型 ────────────────────────────────────────────────────────
/// 调用者类别。仅用于格表标注与文案，真正的判定只用 pid/pgid/sid 三个数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallerKind {
    /// 普通组成员：pgid != pid，sid != pid。
    Ordinary,
    /// 进程组首：pgid == pid。
    PgrpLeader,
    /// 已是会话首：sid == pid（同时是组首）。
    SessionHead,
    /// 孤儿组成员：父组已消失但自身不是组首。
    OrphanMember,
    /// 线程组首：线程组首但不是进程组首。
    ThreadLeader,
    /// 零身份：pid == 0，模型内部的非法调用。
    NoIdentity,
}

impl CallerKind {
    /// 冻结编码（指纹与格表使用）。
    pub fn tag(self) -> u8 {
        match self {
            CallerKind::Ordinary => 1,
            CallerKind::PgrpLeader => 2,
            CallerKind::SessionHead => 3,
            CallerKind::OrphanMember => 4,
            CallerKind::ThreadLeader => 5,
            CallerKind::NoIdentity => 6,
        }
    }
}

/// 一次 setsid 调用者的可观测身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Caller {
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub kind: CallerKind,
    pub has_tty: bool,
}

impl Caller {
    /// 是否进程组首。pid 为 0 时不算（零身份另有 EINVAL 判据）。
    pub fn is_group_leader(&self) -> bool {
        self.pid != 0 && self.pgid == self.pid
    }

    /// 是否已是会话首。
    pub fn is_session_head(&self) -> bool {
        self.pid != 0 && self.sid == self.pid
    }
}

// ── 会话表 ────────────────────────────────────────────────────────────
/// 一个已建会话。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Session {
    pub sid: u32,
    pub leader: u32,
    pub pgid: u32,
    pub members: u32,
    pub has_tty: bool,
}

impl Session {
    /// 空槽哨兵（has_tty=false, sid=0 表示未用）。
    pub const fn empty() -> Session {
        Session { sid: 0, leader: 0, pgid: 0, members: 0, has_tty: false }
    }
}

/// 定容会话表：只增计数，失败不写槽。
#[derive(Clone, Copy, Debug)]
pub struct SessionTable {
    slots: [Session; MAX_SESSIONS],
    count: usize,
    rejects: u32,
}

impl SessionTable {
    pub const fn new() -> SessionTable {
        SessionTable { slots: [Session::empty(); MAX_SESSIONS], count: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn get(&self, i: usize) -> Option<&Session> {
        self.slots.get(i)
    }

    /// 记录一次拒绝（只动 rejects，不动槽位）。
    pub fn note_reject(&mut self) {
        self.rejects = self.rejects.saturating_add(1);
    }

    /// 是否已有该 sid（防止重复登记同一会话）。
    pub fn has_sid(&self, sid: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if let Some(s) = self.slots.get(i) {
                if s.sid == sid {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 插入会话；满表回 ENOMEM 且不覆盖旧槽。
    pub fn insert(&mut self, s: Session) -> Result<usize, i32> {
        let idx = self.count;
        if idx >= MAX_SESSIONS {
            self.note_reject();
            return Err(ENOMEM);
        }
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = s;
            self.count = idx.saturating_add(1);
            Ok(idx)
        } else {
            self.note_reject();
            Err(ENOMEM)
        }
    }
}

// ── B18 选举 ──────────────────────────────────────────────────────────
/// 参选候选。willing=false 表示该成员声明不参选（B18 票面语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Candidate {
    pub pid: u32,
    pub pgid: u32,
    pub willing: bool,
}

/// 空候选集哨兵：显式定长，避免 `&[]` 在调用点的类型推断歧义。
pub const EMPTY_CANDS: [Candidate; 0] = [];

/// 单候选是否具备被选举为会话首的资格。
///
/// 等价于 setsid 的成功条件：非零身份、非组首、且愿意参选。这样「选举」
/// 与「内核会话首规则」共用同一条排除律，对账才有意义。
pub fn eligible(c: Candidate) -> bool {
    c.willing && c.pid != 0 && c.pgid != c.pid
}

/// 选举：合格候选里取最小 pid；无合格候选返回 None（对应 EPERM）。
pub fn elect(cands: &[Candidate]) -> Option<u32> {
    let mut best: Option<u32> = None;
    let mut i = 0usize;
    while i < cands.len() {
        if let Some(c) = cands.get(i).copied() {
            if eligible(c) {
                best = match best {
                    None => Some(c.pid),
                    Some(b) => {
                        if c.pid < b {
                            Some(c.pid)
                        } else {
                            Some(b)
                        }
                    }
                };
            }
        }
        i += 1;
    }
    best
}

/// 由调用者构造单候选（B18 对账用）。
pub fn candidate_of(c: &Caller) -> Candidate {
    Candidate { pid: c.pid, pgid: c.pgid, willing: true }
}

// ── setsid 判定 ───────────────────────────────────────────────────────
/// setsid 的可执行模型。
///
/// 冻结次序：零身份 → 组首 → 已是会话首 → 已登记 → 容量 → 成功。
/// 返回 Ok(sid)（新会话首即 pid）或 Err(errno)。
pub fn sys_setsid(c: Caller, table: &mut SessionTable) -> Result<u32, i32> {
    // ① 零身份：模型内部非法调用，先于一切语义判定。
    if c.pid == 0 {
        table.note_reject();
        return Err(EINVAL);
    }
    // ② 组首硬拒绝：内核 setsid 的第一条语义判据。
    if c.is_group_leader() {
        table.note_reject();
        return Err(EPERM);
    }
    // ③ 已是会话首：sid==pid 必同时是组首，但仍单列取证，避免矩阵并格。
    if c.is_session_head() {
        table.note_reject();
        return Err(EPERM);
    }
    // ④ 同 sid 重复登记：防御性判据，正常调用不会命中。
    if table.has_sid(c.pid) {
        table.note_reject();
        return Err(EINVAL);
    }
    // ⑤ 容量：只有走到这里才允许看表深。
    if table.len() >= MAX_SESSIONS {
        table.note_reject();
        return Err(ENOMEM);
    }
    // ⑥ 建会话：sid/pgid 都收敛到调用者 pid，控制终端被剥离。
    let s = Session {
        sid: c.pid,
        leader: c.pid,
        pgid: c.pid,
        members: 1,
        has_tty: false,
    };
    table.insert(s).map(|_idx| c.pid)
}

// ── 冻结矩阵 ──────────────────────────────────────────────────────────
/// 一格：调用者身份 + 期望错误码（0 表示成功）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub kind: CallerKind,
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub tty: bool,
    pub expect: i32,
}

/// 12 格冻结矩阵（每格用全新空表复算）。
///
/// 期望分布：成功 6 格、EPERM 4 格、EINVAL 1 格（零身份）、
/// 第 12 格为「有控制终端的普通成员」成功格，专门验证 tty 剥离。
pub const MATRIX: [Cell; CELLS] = [
    Cell { kind: CallerKind::Ordinary, pid: 100, pgid: 90, sid: 90, tty: true, expect: 0 },
    Cell { kind: CallerKind::PgrpLeader, pid: 110, pgid: 110, sid: 90, tty: false, expect: EPERM },
    Cell { kind: CallerKind::SessionHead, pid: 120, pgid: 120, sid: 120, tty: false, expect: EPERM },
    Cell { kind: CallerKind::OrphanMember, pid: 210, pgid: 160, sid: 160, tty: false, expect: 0 },
    Cell { kind: CallerKind::Ordinary, pid: 300, pgid: 250, sid: 250, tty: false, expect: 0 },
    Cell { kind: CallerKind::NoIdentity, pid: 0, pgid: 0, sid: 0, tty: false, expect: EINVAL },
    Cell { kind: CallerKind::ThreadLeader, pid: 400, pgid: 350, sid: 350, tty: true, expect: 0 },
    Cell { kind: CallerKind::PgrpLeader, pid: 500, pgid: 500, sid: 450, tty: true, expect: EPERM },
    Cell { kind: CallerKind::SessionHead, pid: 600, pgid: 600, sid: 600, tty: true, expect: EPERM },
    Cell { kind: CallerKind::Ordinary, pid: 700, pgid: 650, sid: 650, tty: true, expect: 0 },
    Cell { kind: CallerKind::PgrpLeader, pid: 800, pgid: 800, sid: 700, tty: false, expect: EPERM },
    Cell { kind: CallerKind::OrphanMember, pid: 900, pgid: 850, sid: 850, tty: true, expect: 0 },
];

/// 由格构造调用者。
pub fn caller_of(cell: &Cell) -> Caller {
    Caller {
        pid: cell.pid,
        pgid: cell.pgid,
        sid: cell.sid,
        kind: cell.kind,
        has_tty: cell.tty,
    }
}

/// 错误码归一：Ok → 0，Err(e) → e。
pub fn code_of(r: Result<u32, i32>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔；守恒破坏时回 EINVAL（调用方必须把它当红）。
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

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 矩阵注入 ──────────────────────────────────────────────────────────
/// 矩阵运行结果。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRun {
    pub observed: [i32; CELLS],
    pub ledger: InjectLedger,
    pub eperm_cells: u32,
    pub eperm_hits: u32,
    pub parity_cells: u32,
}

impl MatrixRun {
    pub const fn empty() -> MatrixRun {
        MatrixRun {
            observed: [0i32; CELLS],
            ledger: InjectLedger::zero(),
            eperm_cells: 0,
            eperm_hits: 0,
            parity_cells: 0,
        }
    }
}

/// 复算一格的一条不变量：成功必须恰好落一槽且 sid/pgid 收敛、tty 剥离；
/// 失败必须槽数不变且 rejects 恰好 +1。
fn slot_invariant(code: i32, table: &SessionTable, pid: u32) -> bool {
    if code == 0 {
        match table.get(0) {
            Some(s) => {
                table.len() == 1
                    && s.sid == pid
                    && s.leader == pid
                    && s.pgid == pid
                    && s.members == 1
                    && !s.has_tty
            }
            None => false,
        }
    } else {
        table.len() == 0 && table.rejects() == 1
    }
}

/// 12 格 × 10 轮注入；每轮全新表，错误码 + 选举对账 + 槽位不变量三合一。
pub fn run_matrix() -> MatrixRun {
    let mut run = MatrixRun::empty();
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(cell) = MATRIX.get(i).copied() {
            if cell.expect == EPERM {
                run.eperm_cells = run.eperm_cells.saturating_add(1);
            }
            let mut round = 0u32;
            while round < ROUNDS {
                let mut table = SessionTable::new();
                let c = caller_of(&cell);
                let res = sys_setsid(c, &mut table);
                let code = code_of(res);
                // B18 对账：成功 ⇔ 单候选可当选。
                let parity = matches!(res, Ok(_)) == elect(&[candidate_of(&c)]).is_some();
                let slots_ok = slot_invariant(code, &table, cell.pid);
                let caught = code == cell.expect && parity && slots_ok;
                let rec = run.ledger.record(caught);
                if rec.is_err() {
                    // 守恒被破坏：不再记格，直接让账本保持可判红的状态。
                    run.ledger.escaped = run.ledger.escaped.saturating_add(1);
                }
                if round == 0 {
                    if let Some(slot) = run.observed.get_mut(i) {
                        *slot = code;
                    }
                    if parity {
                        run.parity_cells = run.parity_cells.saturating_add(1);
                    }
                    if code == EPERM && cell.expect == EPERM {
                        run.eperm_hits = run.eperm_hits.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        i += 1;
    }
    run
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

/// 编码一个错误码（0 表示成功）。
fn encode_code(out: &mut Vec<u8>, code: i32) {
    out.push(if code == 0 { 0u8 } else { 1u8 });
    out.push((code & 0x7F) as u8);
}

/// 规范语料：12 格结果 + 每格选举赢家 + 会话表深度。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(cell) = MATRIX.get(i).copied() {
            let c = caller_of(&cell);
            let mut table = SessionTable::new();
            let res = sys_setsid(c, &mut table);
            encode_code(&mut bytes, code_of(res));
            bytes.push(cell.kind.tag());
            match elect(&[candidate_of(&c)]) {
                Some(pid) => {
                    bytes.push(1);
                    bytes.push((pid & 0xFF) as u8);
                }
                None => bytes.push(0),
            }
            bytes.push((table.len() & 0xFF) as u8);
        }
        i += 1;
    }
    // 再喂一条选举语料：三候选、最小 pid 胜出。
    let cands = [
        Candidate { pid: 300, pgid: 250, willing: true },
        Candidate { pid: 200, pgid: 150, willing: true },
        Candidate { pid: 100, pgid: 100, willing: true },
    ];
    match elect(&cands) {
        Some(pid) => {
            bytes.push(2);
            bytes.push((pid & 0xFF) as u8);
        }
        None => bytes.push(3),
    }
    fnv1a(&bytes)
}

// ── 容量哨兵构造 ──────────────────────────────────────────────────────
/// 用 6 个互不相同的普通调用者填满会话表；返回填充成功数。
pub fn fill_table(table: &mut SessionTable) -> u32 {
    let mut n = 0u32;
    while n < MAX_SESSIONS as u32 {
        let pid = 1000u32.saturating_add(n.saturating_mul(10));
        let c = Caller {
            pid,
            pgid: pid.saturating_sub(1),
            sid: 1,
            kind: CallerKind::Ordinary,
            has_tty: false,
        };
        let res = sys_setsid(c, table);
        if res.is_err() {
            break;
        }
        n = n.saturating_add(1);
    }
    n
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9287 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9287_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9287");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7B540
        && SEG_BASE < SEG_END
        && SEG_END == SEG_BASE + 0x3F;
    s.add("段位守卫：0x40 对齐且窗口 0x7B540..=0x7B57F", seg_ok,
          "段基址必须落在本卡窗口且段间不得互相覆盖");

    let mut uniq = true;
    let mut i = 0usize;
    while i < SetsidCode::ALL.len() {
        let mut j = i + 1;
        while j < SetsidCode::ALL.len() {
            if let (Some(a), Some(b)) = (SetsidCode::ALL.get(i), SetsidCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
                if a.detail().is_empty() || b.detail().is_empty() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一且文案非空", uniq,
          "段内闭集码重复会让判据无法定位");

    let run = run_matrix();

    let mut matrix_ok = true;
    let mut k = 0usize;
    while k < MATRIX.len() {
        if let (Some(cell), Some(got)) = (MATRIX.get(k), run.observed.get(k)) {
            if *got != cell.expect {
                matrix_ok = false;
            }
        } else {
            matrix_ok = false;
        }
        k += 1;
    }
    s.add("矩阵：12 格实测错误码与冻结表逐格一致", matrix_ok,
          "期望列冻结，任何漂移都必须显性判红");

    s.add("注入：120 次全检出且逃逸为 0",
          run.ledger.conserved() && run.ledger.escaped == 0
              && run.ledger.injected == CELLS as u32 * ROUNDS
              && run.ledger.detected == run.ledger.injected,
          "每轮错误码 + 选举对账 + 槽位不变量必须同时成立");

    s.add("EPERM 格：5 格全过且计数吻合",
          run.eperm_cells == 5 && run.eperm_hits == 5,
          "组首/会话首两类调用者都必须被 EPERM 拦下");

    s.add("B18 对账：12 格 setsid 与选举语义一致", run.parity_cells == CELLS as u32,
          "成功 ⇔ 单候选可当选，任一侧漂移即判红");

    let normal = Caller { pid: 100, pgid: 90, sid: 90, kind: CallerKind::Ordinary, has_tty: true };
    let mut t1 = SessionTable::new();
    let r1 = sys_setsid(normal, &mut t1);
    let normal_ok = r1 == Ok(100)
        && t1.len() == 1
        && t1.get(0).map(|x| x.sid == 100 && x.pgid == 100 && x.leader == 100 && !x.has_tty) == Some(true);
    s.add("正常流：普通成员建会话且控制终端剥离", normal_ok,
          "新会话 sid/pgid 收敛到 pid，has_tty 必须清零");

    let leader = Caller { pid: 110, pgid: 110, sid: 90, kind: CallerKind::PgrpLeader, has_tty: false };
    let mut t2 = SessionTable::new();
    let r2 = sys_setsid(leader, &mut t2);
    s.add("组首：EPERM 且不占槽、rejects 恰 +1",
          r2 == Err(EPERM) && t2.len() == 0 && t2.rejects() == 1,
          "失败路径不得写槽，拒绝计数只增一笔");

    let head = Caller { pid: 120, pgid: 120, sid: 120, kind: CallerKind::SessionHead, has_tty: false };
    let mut t3 = SessionTable::new();
    let r3 = sys_setsid(head, &mut t3);
    s.add("会话首：sid==pid 再 setsid 回 EPERM", r3 == Err(EPERM) && t3.len() == 0,
          "重复建会话必须被拒，不得生成第二个 sid");

    let zero = Caller { pid: 0, pgid: 0, sid: 0, kind: CallerKind::NoIdentity, has_tty: false };
    let mut t4 = SessionTable::new();
    let r4 = sys_setsid(zero, &mut t4);
    s.add("零身份：EINVAL 先于组首 EPERM", r4 == Err(EINVAL) && t4.rejects() == 1,
          "pid==0 与 pgid==pid 同时成立时必须先回 EINVAL");

    let mut t5 = SessionTable::new();
    let filled = fill_table(&mut t5);
    let late_leader = Caller { pid: 5000, pgid: 5000, sid: 1, kind: CallerKind::PgrpLeader, has_tty: false };
    let r5 = sys_setsid(late_leader, &mut t5);
    s.add("判序：满表 + 组首仍回 EPERM 而非 ENOMEM",
          filled == MAX_SESSIONS as u32 && r5 == Err(EPERM) && t5.len() == MAX_SESSIONS,
          "语义拒绝必须先于容量拒绝，否则调用方会误判为资源不足");

    let mut t6 = SessionTable::new();
    let f6 = fill_table(&mut t6);
    let seventh = Caller { pid: 6000, pgid: 5999, sid: 1, kind: CallerKind::Ordinary, has_tty: false };
    let r6 = sys_setsid(seventh, &mut t6);
    s.add("容量：6 槽填满后第 7 个普通调用回 ENOMEM",
          f6 == MAX_SESSIONS as u32 && r6 == Err(ENOMEM) && t6.len() == MAX_SESSIONS,
          "定容表不得越界，也不得覆盖既有会话");

    let cands = [
        Candidate { pid: 300, pgid: 250, willing: true },
        Candidate { pid: 200, pgid: 150, willing: true },
        Candidate { pid: 400, pgid: 400, willing: true },
        Candidate { pid: 150, pgid: 140, willing: false },
    ];
    let win = elect(&cands);
    let only_leaders = [
        Candidate { pid: 10, pgid: 10, willing: true },
        Candidate { pid: 11, pgid: 11, willing: true },
    ];
    s.add("选举：取最小合格 pid、排除组首与不参选者",
          win == Some(200) && elect(&only_leaders).is_none() && elect(&EMPTY_CANDS).is_none(),
          "组首与不参选者不得进入决胜集合");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    let mut mono = run.ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _rec = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before.saturating_add(4),
          "累计计数禁止回退或被覆盖");

    let matrix_text = format!(
        "矩阵 {} 格 / EPERM {} 格 / 对账 {} 格",
        CELLS, run.eperm_hits, run.parity_cells
    );
    s.add_dyn("矩阵快照", matrix_ok && run.eperm_hits == 5, &matrix_text);

    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        run.ledger.injected, run.ledger.detected, run.ledger.escaped
    );
    s.add_dyn("注入账快照", run.ledger.conserved() && run.ledger.escaped == 0, &ledger_text);

    let fp_text = format!("会话指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary(pid: u32) -> Caller {
        Caller { pid, pgid: pid.saturating_sub(1), sid: 1, kind: CallerKind::Ordinary, has_tty: false }
    }

    #[test]
    fn ordinary_creates_session() {
        let mut t = SessionTable::new();
        assert_eq!(sys_setsid(ordinary(100), &mut t), Ok(100));
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(0).map(|s| s.sid), Some(100));
        assert_eq!(t.get(0).map(|s| s.has_tty), Some(false));
    }

    #[test]
    fn group_leader_denied() {
        let mut t = SessionTable::new();
        let c = Caller { pid: 110, pgid: 110, sid: 90, kind: CallerKind::PgrpLeader, has_tty: false };
        assert_eq!(sys_setsid(c, &mut t), Err(EPERM));
        assert_eq!(t.len(), 0);
        assert_eq!(t.rejects(), 1);
    }

    #[test]
    fn session_head_denied() {
        let mut t = SessionTable::new();
        let c = Caller { pid: 120, pgid: 120, sid: 120, kind: CallerKind::SessionHead, has_tty: false };
        assert_eq!(sys_setsid(c, &mut t), Err(EPERM));
        assert_eq!(t.rejects(), 1);
    }

    #[test]
    fn zero_pid_is_einval() {
        let mut t = SessionTable::new();
        let c = Caller { pid: 0, pgid: 0, sid: 0, kind: CallerKind::NoIdentity, has_tty: false };
        assert_eq!(sys_setsid(c, &mut t), Err(EINVAL));
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn capacity_full() {
        let mut t = SessionTable::new();
        assert_eq!(fill_table(&mut t), MAX_SESSIONS as u32);
        let c = Caller { pid: 9000, pgid: 8999, sid: 1, kind: CallerKind::Ordinary, has_tty: false };
        assert_eq!(sys_setsid(c, &mut t), Err(ENOMEM));
        assert_eq!(t.len(), MAX_SESSIONS);
    }

    #[test]
    fn leader_precedes_capacity() {
        let mut t = SessionTable::new();
        assert_eq!(fill_table(&mut t), MAX_SESSIONS as u32);
        let c = Caller { pid: 9100, pgid: 9100, sid: 1, kind: CallerKind::PgrpLeader, has_tty: false };
        assert_eq!(sys_setsid(c, &mut t), Err(EPERM));
    }

    #[test]
    fn election_picks_minimum() {
        let cands = [
            Candidate { pid: 300, pgid: 250, willing: true },
            Candidate { pid: 200, pgid: 150, willing: true },
            Candidate { pid: 100, pgid: 100, willing: true },
        ];
        assert_eq!(elect(&cands), Some(200));
        if let Some(leader) = cands.get(2) {
            assert_eq!(eligible(*leader), false);
        }
    }

    #[test]
    fn election_empty_and_leaders() {
        let only_leaders = [
            Candidate { pid: 10, pgid: 10, willing: true },
            Candidate { pid: 11, pgid: 11, willing: false },
        ];
        assert_eq!(elect(&EMPTY_CANDS), None);
        assert_eq!(elect(&only_leaders), None);
    }

    #[test]
    fn frozen_matrix_matches() {
        let run = run_matrix();
        assert_eq!(run.ledger.injected, 120);
        assert_eq!(run.ledger.escaped, 0);
        assert_eq!(run.eperm_cells, 5);
        assert_eq!(run.eperm_hits, 5);
        assert_eq!(run.parity_cells, CELLS as u32);
        for i in 0..MATRIX.len() {
            if let (Some(cell), Some(got)) = (MATRIX.get(i), run.observed.get(i)) {
                assert_eq!(*got, cell.expect);
            }
        }
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn codes_are_unique() {
        let mut i = 0;
        while i < SetsidCode::ALL.len() {
            let mut j = i + 1;
            while j < SetsidCode::ALL.len() {
                if let (Some(a), Some(b)) = (SetsidCode::ALL.get(i), SetsidCode::ALL.get(j)) {
                    assert_ne!(a.code(), b.code());
                    assert_eq!(a.detail().is_empty(), false);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9287_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
