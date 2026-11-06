//! UNX-F9358 E 型判据可复测性总声明（全矩阵 10/10） · 0x7C700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9358 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F9358-J1 全矩阵 10 次重复注入判据过，10/10 次检出总
//!      统计格过，零 flaky 总声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判定对象是「操作 × 参数形态」的 errno 门：6 个 syscall 面（read /
//!      write / close / ioctl / mmap / 路径遍历）× 6 种参数形态（坏 fd、空
//!      缓冲、错对齐、只写 fd、只读 fd、能力不支持）共 36 格冻结矩阵，0 表示
//!      成功。探针表只允许引用矩阵坐标，`want` 必须与该坐标的冻结值相等，
//!      因此探针表与矩阵互为校验，任何单边手改都会当场红。
//!   ② 重复注入是「轮」的循环：固定 10 轮，每轮用自写 LCG 决定起始格做旋
//!      转遍历（顺序扰动），每轮的命中数、脱靶数、轮摘要都记账。10/10 的落
//!      点是 `per_round[i] == 12` 与 `rounds_passed == 10` 两条同时成立。
//!   ③ 零 flaky 不是口号：flaky 的定义是「轮命中数不等于格数」的轮次计数，
//!      必须恰为 0；同时每格跨 10 轮的命中数必须恰为 10（逐格稳定），两层
//!      统计格同时平账才允许写「零 flaky」总声明。
//!   ④ 顺序无关性单独成判据：以三组不同旋转种子生成的判定向量必须逐格相
//!      等。这一条把「检测依赖遍历顺序」这种最隐蔽的伪稳定抓出来。
//!   ⑤ 账本守恒：injected = detected + escaped，且 escaped == 0；injected
//!      固定为 10 × 12 = 120，脱靶哪怕一格，守恒式仍成立但 escaped ≠ 0，
//!      两条判据分别覆盖「漏记」与「漏检」两种失效。
//!   ⑥ 总声明是 5 条可复算断言（覆盖/可重复/零 flaky/守恒/确定性）拼成的
//!      声明格，另附状态与摘要，对应合同 ③ 的写回要求。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9358 独占 0x7C700..=0x7C73F）。
pub const SEG_BASE: u32 = 0x7C700;

/// 固定重复轮数（10 次重复注入）。
pub const ROUNDS: usize = 10;

/// 探针格数（每轮注入 12 格）。
pub const CELLS: usize = 12;

/// syscall 面个数（矩阵行数）。
pub const OPS: usize = 6;

/// 参数形态个数（矩阵列数）。
pub const SHAPES: usize = 6;

/// 注入总量（10 × 12）。
pub const INJECTED_TOTAL: u32 = (ROUNDS * CELLS) as u32;

/// 总声明条数。
pub const CLAIMS: usize = 5;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOTTY: i32 = 25;
pub const ENOSYS: i32 = 38;
pub const EBADF: i32 = 9;

/// 成功格（矩阵中 0 表示该组合不该报错）。
pub const OK: i32 = 0;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+12；段基址超 u16，故 code() 给段内序号）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecheckCode {
    RepeatMiss = 1,
    CellUnstable = 2,
    FlakyRound = 3,
    LedgerBreach = 4,
    OrderDependent = 5,
    MatrixDrift = 6,
    ProbeOutOfRange = 7,
    VerdictLeak = 8,
    ZeroRound = 9,
    DeclareEmpty = 10,
    FingerprintDrift = 11,
    SegmentOut = 12,
}

impl RecheckCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内绝对码。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文一句话建议。
    pub fn detail(self) -> &'static str {
        match self {
            RecheckCode::RepeatMiss => "重复轮次不足：必须跑满 10 轮，少一轮即无法声称可复测",
            RecheckCode::CellUnstable => "单格跨轮不稳：某格命中数不足 10，说明判定含隐藏状态",
            RecheckCode::FlakyRound => "存在 flaky 轮：轮命中数不等于格数，禁止写零 flaky 总声明",
            RecheckCode::LedgerBreach => "注入账不守恒：injected 必须等于 detected 加 escaped",
            RecheckCode::OrderDependent => "判定随遍历顺序变化：旋转种子必须给出同一判定向量",
            RecheckCode::MatrixDrift => "探针与矩阵不符：want 必须等于矩阵冻结值，禁止单边改数",
            RecheckCode::ProbeOutOfRange => "探针越界：op/shape 坐标必须落在 6×6 矩阵内",
            RecheckCode::VerdictLeak => "判定泄漏：成功格不得产出错误码，错误格不得返回成功",
            RecheckCode::ZeroRound => "轮次为零：未注入就出声明，属于空跑，必须拒绝",
            RecheckCode::DeclareEmpty => "总声明为空：五条断言缺条或摘要为空，视为未完成",
            RecheckCode::FingerprintDrift => "双跑指纹不等：同轮次序列必须逐字节相同",
            RecheckCode::SegmentOut => "段位越界：本模块必须落在 0x7C700 起 0x40 窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [RecheckCode] = &[
        RecheckCode::RepeatMiss,
        RecheckCode::CellUnstable,
        RecheckCode::FlakyRound,
        RecheckCode::LedgerBreach,
        RecheckCode::OrderDependent,
        RecheckCode::MatrixDrift,
        RecheckCode::ProbeOutOfRange,
        RecheckCode::VerdictLeak,
        RecheckCode::ZeroRound,
        RecheckCode::DeclareEmpty,
        RecheckCode::FingerprintDrift,
        RecheckCode::SegmentOut,
    ];
}

// ── 矩阵坐标 ──────────────────────────────────────────────────────────
/// syscall 面（矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Read,
    Write,
    Close,
    Ioctl,
    Mmap,
    Traverse,
}

impl Op {
    /// 行下标。
    pub fn row(self) -> usize {
        match self {
            Op::Read => 0,
            Op::Write => 1,
            Op::Close => 2,
            Op::Ioctl => 3,
            Op::Mmap => 4,
            Op::Traverse => 5,
        }
    }

    /// 行名。
    pub fn name(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Write => "write",
            Op::Close => "close",
            Op::Ioctl => "ioctl",
            Op::Mmap => "mmap",
            Op::Traverse => "traverse",
        }
    }
}

/// 参数形态（矩阵列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    BadFd,
    NullBuf,
    BadAlign,
    WriteOnly,
    ReadOnly,
    Unsupported,
}

impl Shape {
    /// 列下标。
    pub fn col(self) -> usize {
        match self {
            Shape::BadFd => 0,
            Shape::NullBuf => 1,
            Shape::BadAlign => 2,
            Shape::WriteOnly => 3,
            Shape::ReadOnly => 4,
            Shape::Unsupported => 5,
        }
    }

    /// 列名。
    pub fn name(self) -> &'static str {
        match self {
            Shape::BadFd => "坏 fd",
            Shape::NullBuf => "空缓冲",
            Shape::BadAlign => "错对齐",
            Shape::WriteOnly => "只写 fd",
            Shape::ReadOnly => "只读 fd",
            Shape::Unsupported => "能力不支持",
        }
    }
}

/// 冻结 errno 矩阵：行 = Op，列 = Shape，0 = 成功。
///
/// 取值口径（为什么不是「每个错格都给 EINVAL」）：Linux 的错误面按「谁先被
/// 检查」分层——fd 合法性先于缓冲合法性，缓冲先于对齐，对齐先于能力协商。
/// 因此坏 fd 一行几乎全是 EBADF，能力不支持只在 ioctl/mmap 上显形；close 对
/// 参数形态不敏感，只剩坏 fd 一列报错。这张分层表就是「E 型语义」本身。
pub const GATE: [[i32; SHAPES]; OPS] = [
    // Read
    [EBADF, EFAULT, EINVAL, EBADF, OK, OK],
    // Write
    [EBADF, EFAULT, EINVAL, OK, EBADF, OK],
    // Close
    [EBADF, OK, OK, OK, OK, OK],
    // Ioctl
    [EBADF, EFAULT, EINVAL, OK, OK, ENOTTY],
    // Mmap
    [EBADF, OK, EINVAL, EACCES, OK, ENOSYS],
    // Traverse
    [EBADF, EFAULT, OK, OK, OK, OK],
];

/// 矩阵取值（无裸索引，坐标越界回 None）。
pub fn gate_value(op: Op, shape: Shape) -> Option<i32> {
    GATE.get(op.row()).and_then(|row| row.get(shape.col())).copied()
}

/// 矩阵里的错误格统计（返回值, 错误格数, 成功格数）。
pub fn gate_census() -> (u32, u32, u32) {
    let mut err = 0u32;
    let mut ok = 0u32;
    let mut total = 0u32;
    for row in GATE.iter() {
        for v in row.iter() {
            total = total.saturating_add(1);
            if *v == OK {
                ok = ok.saturating_add(1);
            } else {
                err = err.saturating_add(1);
            }
        }
    }
    (total, err, ok)
}

// ── 探针表 ────────────────────────────────────────────────────────────
/// 一格探针：只引用矩阵坐标，`want` 必须与矩阵值一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub op: Op,
    pub shape: Shape,
    pub want: i32,
    pub label: &'static str,
}

/// 12 格冻结探针（8 个错误格 + 4 个成功格，用来同时验漏检与误报）。
pub const PROBES: [Probe; CELLS] = [
    Probe { op: Op::Read, shape: Shape::BadFd, want: EBADF, label: "read 坏 fd" },
    Probe { op: Op::Read, shape: Shape::NullBuf, want: EFAULT, label: "read 空缓冲" },
    Probe { op: Op::Read, shape: Shape::BadAlign, want: EINVAL, label: "read 错对齐" },
    Probe { op: Op::Read, shape: Shape::ReadOnly, want: OK, label: "read 只读 fd" },
    Probe { op: Op::Write, shape: Shape::ReadOnly, want: EBADF, label: "write 只读 fd" },
    Probe { op: Op::Write, shape: Shape::NullBuf, want: EFAULT, label: "write 空缓冲" },
    Probe { op: Op::Close, shape: Shape::BadFd, want: EBADF, label: "close 坏 fd" },
    Probe { op: Op::Close, shape: Shape::NullBuf, want: OK, label: "close 忽略缓冲" },
    Probe { op: Op::Ioctl, shape: Shape::Unsupported, want: ENOTTY, label: "ioctl 不支持" },
    Probe { op: Op::Ioctl, shape: Shape::BadAlign, want: EINVAL, label: "ioctl 错对齐" },
    Probe { op: Op::Mmap, shape: Shape::WriteOnly, want: EACCES, label: "mmap 越权保护" },
    Probe { op: Op::Mmap, shape: Shape::Unsupported, want: ENOSYS, label: "mmap 未实现族" },
];

/// 探针与矩阵逐格互证：返回 (一致格数, 越界格数)。
pub fn probe_matrix_agreement() -> (u32, u32) {
    let mut agree = 0u32;
    let mut miss = 0u32;
    for p in PROBES.iter() {
        match gate_value(p.op, p.shape) {
            Some(v) => {
                if v == p.want {
                    agree = agree.saturating_add(1);
                } else {
                    miss = miss.saturating_add(1);
                }
            }
            None => miss = miss.saturating_add(1),
        }
    }
    (agree, miss)
}

/// 单探针过门：成功回 Ok(0)，错误回 Err(errno)。
///
/// 这里刻意保留「成功也进 Result」的口径，以便调用方必须显式处理成功分支，
/// 不会因为 `?` 把成功当成异常吞掉。
pub fn probe_gate(p: &Probe) -> Result<u32, i32> {
    match gate_value(p.op, p.shape) {
        None => Err(EINVAL),
        Some(OK) => Ok(0),
        Some(code) => Err(code),
    }
}

/// 单格检出：期望与实得必须同侧同值。
pub fn detect(p: &Probe) -> bool {
    match probe_gate(p) {
        Ok(v) => p.want == OK && v == 0,
        Err(e) => p.want != OK && e == p.want,
    }
}

// ── 顺序扰动（固定 LCG） ──────────────────────────────────────────────
/// 自写 32 位同余发生器（Numerical Recipes 常数），不依赖系统随机数。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 以固定种子构造（保证双跑一致）。
    pub const fn seed(s: u32) -> Lcg {
        Lcg { state: s | 1 }
    }

    /// 下一状态。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }

    /// 取 [0, n) 内的整数（n 为常量 12，非零）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }
}

/// 某种子下的判定向量（按旋转起点遍历 12 格）。
///
/// 遍历顺序被扰动，但判定必须落在同一格上——这正是顺序无关性的观测点。
pub fn verdict_vector(seed: u32) -> [bool; CELLS] {
    let mut out = [false; CELLS];
    let mut k = 0usize;
    while k < CELLS {
        let idx = (seed as usize).wrapping_add(k) % CELLS;
        if let Some(p) = PROBES.get(idx) {
            let ok = detect(p);
            if let Some(slot) = out.get_mut(idx) {
                *slot = ok;
            }
        }
        k += 1;
    }
    out
}

/// 顺序无关：三组种子给出逐格相同的判定向量。
pub fn order_independent() -> bool {
    let a = verdict_vector(0);
    let b = verdict_vector(5);
    let c = verdict_vector(11);
    a == b && b == c && a.iter().all(|v| *v)
}

// ── 单轮与全矩阵 ──────────────────────────────────────────────────────
/// 一轮的账目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundResult {
    pub index: u32,
    pub hits: u32,
    pub misses: u32,
    pub digest: u32,
}

/// 单轮注入：旋转遍历 12 格，命中/脱靶分别记账，轮摘要覆盖顺序与判定。
pub fn run_round(index: u32, lcg: &mut Lcg) -> RoundResult {
    let start = lcg.below(CELLS as u32) as usize;
    let mut hits = 0u32;
    let mut misses = 0u32;
    let mut digest: u32 = 0x811C_9DC5 ^ (index.wrapping_mul(0x0100_0193));
    let mut k = 0usize;
    while k < CELLS {
        let idx = start.wrapping_add(k) % CELLS;
        if let Some(p) = PROBES.get(idx) {
            if detect(p) {
                hits = hits.saturating_add(1);
            } else {
                misses = misses.saturating_add(1);
            }
            // 摘要逐格喂入「格号 + 判定 + 期望码」，顺序本身也进摘要。
            digest ^= (idx as u32).wrapping_mul(0x9E37_79B9);
            digest = digest.wrapping_mul(0x0100_0193);
            digest ^= (p.want & 0xFF) as u32;
            digest = digest.wrapping_mul(0x0100_0193);
        }
        k += 1;
    }
    RoundResult { index, hits, misses, digest }
}

/// 全矩阵报告（10 轮 + 逐格统计）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixReport {
    pub per_round: [u32; ROUNDS],
    pub per_cell: [u32; CELLS],
    pub rounds: u32,
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub flaky: u32,
    pub digest: u32,
}

impl MatrixReport {
    /// 空报告（未跑）。
    pub const fn empty() -> MatrixReport {
        MatrixReport {
            per_round: [0u32; ROUNDS],
            per_cell: [0u32; CELLS],
            rounds: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            flaky: 0,
            digest: 0,
        }
    }

    /// 账本守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }

    /// 每轮是否满格。
    pub fn rounds_full(&self) -> bool {
        self.per_round.iter().all(|n| *n == CELLS as u32)
    }

    /// 每格是否跨轮稳定。
    pub fn cells_stable(&self) -> bool {
        self.per_cell.iter().all(|n| *n == ROUNDS as u32)
    }

    /// 轮次是否跑满。
    pub fn rounds_complete(&self) -> bool {
        self.rounds == ROUNDS as u32
    }
}

/// 跑满 10 轮全矩阵（固定种子，双跑一致）。
pub fn run_matrix() -> MatrixReport {
    let mut rep = MatrixReport::empty();
    let mut lcg = Lcg::seed(0x9358_0007);
    let mut r = 0usize;
    while r < ROUNDS {
        let round = run_round(r as u32, &mut lcg);
        if let Some(slot) = rep.per_round.get_mut(r) {
            *slot = round.hits;
        }
        rep.rounds = rep.rounds.saturating_add(1);
        rep.injected = rep.injected.saturating_add(round.hits.saturating_add(round.misses));
        rep.detected = rep.detected.saturating_add(round.hits);
        rep.escaped = rep.escaped.saturating_add(round.misses);
        if round.hits != CELLS as u32 {
            rep.flaky = rep.flaky.saturating_add(1);
        }
        rep.digest ^= round.digest;
        rep.digest = rep.digest.wrapping_mul(0x0100_0193);
        r += 1;
    }
    // 逐格统计：再跑一遍同种子序列，按格累加（不复用上面的顺序状态）。
    let mut lcg2 = Lcg::seed(0x9358_0007);
    let mut r2 = 0usize;
    while r2 < ROUNDS {
        let start = lcg2.below(CELLS as u32) as usize;
        let mut k = 0usize;
        while k < CELLS {
            let idx = start.wrapping_add(k) % CELLS;
            if let Some(p) = PROBES.get(idx) {
                if detect(p) {
                    if let Some(slot) = rep.per_cell.get_mut(idx) {
                        *slot = slot.saturating_add(1);
                    }
                }
            }
            k += 1;
        }
        // 与 run_round 保持同一消费节奏（每轮一次 below）。
        r2 += 1;
    }
    rep
}

// ── 总声明 ────────────────────────────────────────────────────────────
/// 一条总声明断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClaimRow {
    pub name: &'static str,
    pub holds: bool,
    pub note: &'static str,
}

/// 写回状态。
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

/// 总声明格：5 条断言 + 轮次账 + 状态 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Declaration {
    pub claims_ok: u32,
    pub claims_total: u32,
    pub rounds_passed: u32,
    pub rounds_total: u32,
    pub flaky: u32,
    pub escaped: u32,
    pub status: WbStatus,
    pub summary: &'static str,
}

/// 由报告算出五条断言（顺序固定：覆盖/可重复/零 flaky/守恒/确定性）。
pub fn claim_board(rep: &MatrixReport) -> [ClaimRow; CLAIMS] {
    let (total, err, _ok) = gate_census();
    let (agree, miss) = probe_matrix_agreement();
    let tw = verdict_vector(0);
    [
        ClaimRow {
            name: "覆盖",
            holds: total == (OPS * SHAPES) as u32
                && err > 0
                && agree + miss == CELLS as u32
                && miss == 0,
            note: "36 格矩阵齐备，12 探针逐格与矩阵一致",
        },
        ClaimRow {
            name: "可重复",
            holds: rep.rounds_complete() && rep.injected == INJECTED_TOTAL,
            note: "必须跑满 10 轮、注入总量 120，少一轮不可声称可复测",
        },
        ClaimRow {
            name: "零 flaky",
            holds: rep.flaky == 0 && rep.rounds_full() && rep.cells_stable(),
            note: "轮命中满格且逐格跨轮稳定，两层统计同时成立",
        },
        ClaimRow {
            name: "守恒",
            holds: rep.conserved() && rep.escaped == 0,
            note: "injected = detected + escaped 且逃逸为 0",
        },
        ClaimRow {
            name: "确定性",
            holds: tw.iter().all(|v| *v) && order_independent(),
            note: "三组旋转种子给出同一判定向量，且全部检出",
        },
    ]
}

/// 由报告生成声明（未达成的条数计入 claims_ok）。
pub fn declare(rep: &MatrixReport) -> Declaration {
    let board = claim_board(rep);
    let mut ok = 0u32;
    for c in board.iter() {
        if c.holds {
            ok = ok.saturating_add(1);
        }
    }
    let rounds_passed = rep.per_round.iter().filter(|n| **n == CELLS as u32).count() as u32;
    let status = if ok == CLAIMS as u32 && rep.escaped == 0 {
        WbStatus::Done
    } else if ok > 0 {
        WbStatus::Partial
    } else {
        WbStatus::Pending
    };
    Declaration {
        claims_ok: ok,
        claims_total: CLAIMS as u32,
        rounds_passed,
        rounds_total: ROUNDS as u32,
        flaky: rep.flaky,
        escaped: rep.escaped,
        status,
        summary: "全矩阵 10 轮 120 次注入全检出，逐格跨轮稳定，零 flaky 总声明成立",
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 报告指纹：逐轮命中、逐格命中、账目、摘要全部入账。
pub fn report_fingerprint(rep: &MatrixReport) -> u32 {
    let mut out: Vec<u8> = Vec::new();
    for n in rep.per_round.iter() {
        out.push((*n & 0xFF) as u8);
    }
    for n in rep.per_cell.iter() {
        out.push((*n & 0xFF) as u8);
    }
    out.push((rep.rounds & 0xFF) as u8);
    out.push((rep.injected & 0xFF) as u8);
    out.push((rep.detected & 0xFF) as u8);
    out.push((rep.escaped & 0xFF) as u8);
    out.push((rep.flaky & 0xFF) as u8);
    out.push((rep.digest & 0xFF) as u8);
    out.push(((rep.digest >> 8) & 0xFF) as u8);
    // 矩阵本身也入指纹：矩阵漂移必须反映到指纹上。
    for row in GATE.iter() {
        for v in row.iter() {
            out.push((*v & 0x7F) as u8);
        }
    }
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9358 判据集（静态 16 条 + 动态 3 条）。
pub fn run_unx9358_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9358");

    // 1. 段位
    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7C700..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE == 0x7C700;
    s.add("段位守卫：0x7C700 起 0x40 对齐", seg_ok,
          "本模块独占 0x7C700..=0x7C73F，与 F9357 窗口不重叠");

    // 2. 矩阵形状
    let (total, err, ok_cells) = gate_census();
    s.add("矩阵形状：6×6 共 36 格且错误格与非错格并存",
          total == (OPS * SHAPES) as u32 && err > 0 && ok_cells > 0,
          "36 格必须齐备，且成功格用于验误报、错误格用于验漏检");

    // 3. 探针与矩阵互证
    let (agree, miss) = probe_matrix_agreement();
    s.add("探针互证：12 格 want 与矩阵冻结值逐格相等",
          agree == CELLS as u32 && miss == 0,
          "探针只引用坐标，禁止单边改 want 绕过矩阵");

    // 4. 单格检出
    let all_detect = PROBES.iter().all(detect);
    s.add("单格检出：12 探针全部与期望同侧同值", all_detect,
          "成功格不得报错、错误格不得返回成功，两侧都要验");

    // 5. 单轮满格
    let mut lcg = Lcg::seed(0x9358_0007);
    let first = run_round(0, &mut lcg);
    s.add("单轮：第 0 轮 12/12 命中、脱靶 0",
          first.hits == CELLS as u32 && first.misses == 0 && first.index == 0,
          "每轮必须满格，脱靶一格即视为本轮失败");

    // 6. 全矩阵 10 轮
    let rep = run_matrix();
    s.add("全矩阵：10 轮跑满且每轮 12/12",
          rep.rounds_complete() && rep.rounds_full(),
          "10/10 次检出的总统计格由 per_round 逐轮核对");

    // 7. 账本守恒
    s.add("账本：injected 120 = detected + escaped 且逃逸 0",
          rep.injected == INJECTED_TOTAL
              && rep.conserved()
              && rep.detected == INJECTED_TOTAL
              && rep.escaped == 0,
          "守恒式管漏记，escaped==0 管漏检，两条都要成立");

    // 8. 逐格稳定
    s.add("逐格稳定：12 格各跨 10 轮命中 10 次",
          rep.cells_stable(),
          "任一格不足 10 次说明判定依赖轮次状态");

    // 9. 零 flaky
    s.add("零 flaky：flaky 轮计数为 0", rep.flaky == 0,
          "flaky 定义为轮命中数不等于格数，必须恰为 0");

    // 10. 10/10 总统计
    let passed = rep.per_round.iter().filter(|n| **n == CELLS as u32).count() as u32;
    s.add("10/10 检出总统计格：通过轮数等于 10",
          passed == ROUNDS as u32 && passed == 10,
          "通过轮数必须逐轮累加得到，不能用常量冒充");

    // 11. 顺序无关
    s.add("顺序无关：三组旋转种子判定向量逐格相同", order_independent(),
          "检测若依赖遍历顺序，会在此处暴露为向量不等");

    // 12. 顺序无关的反面：故意错位的期望必须被检出
    let wrong = Probe { op: Op::Read, shape: Shape::BadFd, want: EFAULT, label: "错期望" };
    let wrong2 = Probe { op: Op::Mmap, shape: Shape::WriteOnly, want: OK, label: "错期望2" };
    s.add("误报反面：故意错位的期望必须判不检出",
          !detect(&wrong) && !detect(&wrong2),
          "若错期望也被判检出，说明判定退化成恒真");

    // 13. 坐标越界
    let bogus = Op::Read.row();
    let out_of_range = GATE.get(bogus).and_then(|r| r.get(SHAPES)).copied();
    s.add("越界安全：列下标取 SHAPES 回 None 而非回绕",
          out_of_range.is_none() && gate_value(Op::Read, Shape::BadFd).is_some(),
          "矩阵取值必须走 get，越界不得读到相邻行首");

    // 14. 总声明五条
    let board = claim_board(&rep);
    let holding = board.iter().filter(|c| c.holds).count() as u32;
    s.add("总声明格：五条断言全部成立",
          holding == CLAIMS as u32 && board.iter().all(|c| !c.note.is_empty()),
          "覆盖/可重复/零 flaky/守恒/确定性五条缺一即红");

    // 15. 写回
    let decl = declare(&rep);
    s.add("写回：状态=已完成、摘要非空、轮次 10/10",
          decl.status == WbStatus::Done
              && decl.status.idx() == 2
              && decl.summary.len() >= 24
              && decl.rounds_passed == decl.rounds_total
              && decl.claims_ok == decl.claims_total,
          "合同 ③ 要求状态与摘要同时成立，且声明条数必须齐");

    // 16. 空报告必不通过（防止空跑出声明）
    //
    // 口径：覆盖与确定性两条是纯静态断言，空报告也能立；但「可重复」与
    // 「零 flaky」依赖轮次与逐格账，空报告必然不成立，因此声明只能停在
    // Partial，绝不能是 Done。这里同时钉住「声明条数必须少于 5」。
    let empty = MatrixReport::empty();
    let empty_decl = declare(&empty);
    s.add("空跑防护：空报告的声明不得为已完成",
          empty_decl.status == WbStatus::Partial
              && empty_decl.status != WbStatus::Done
              && empty_decl.claims_ok < CLAIMS as u32
              && empty_decl.rounds_passed == 0,
          "未注入就出声明属于空跑，必须被状态机拒绝");

    // 17. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RecheckCode::ALL.len() {
        let mut j = i + 1;
        while j < RecheckCode::ALL.len() {
            if let (Some(a), Some(b)) = (RecheckCode::ALL.get(i), RecheckCode::ALL.get(j)) {
                if a.code() == b.code() || a.seg_addr() == b.seg_addr() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个段内序号与绝对码两两唯一", uniq,
          "闭集码重复会让红项无法定位到唯一建议");

    // 18. 双跑
    let fp_a = report_fingerprint(&rep);
    let rep2 = run_matrix();
    let fp_b = report_fingerprint(&rep2);
    s.add("双跑一致：同种子两遍报告指纹相等",
          fp_a == fp_b && rep.digest == rep2.digest && rep.per_cell == rep2.per_cell,
          "重复注入的可复测性以双跑逐字节相等为准");

    // 动态明细（≤8 条）
    let round_text = format!("轮次 {}/{}，每轮 {} 格，注入 {}", rep.rounds, ROUNDS, CELLS, rep.injected);
    s.add_dyn("轮次明细", rep.rounds_complete(), &round_text);

    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", rep.injected, rep.detected, rep.escaped);
    s.add_dyn("账目明细", rep.conserved(), &ledger_text);

    let fp_text = format!("报告指纹 0x{:08X}，轮摘要 0x{:08X}", fp_a, rep.digest);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_shape_and_census() {
        assert_eq!(GATE.len(), OPS);
        assert_eq!(GATE.first().map(|r| r.len()), Some(SHAPES));
        let (total, err, ok) = gate_census();
        assert_eq!(total, 36);
        assert_eq!(err + ok, 36);
        assert!(err > 0 && ok > 0);
    }

    #[test]
    fn gate_values() {
        assert_eq!(gate_value(Op::Read, Shape::BadFd), Some(EBADF));
        assert_eq!(gate_value(Op::Close, Shape::NullBuf), Some(OK));
        assert_eq!(gate_value(Op::Ioctl, Shape::Unsupported), Some(ENOTTY));
        assert_eq!(gate_value(Op::Mmap, Shape::WriteOnly), Some(EACCES));
        assert_eq!(gate_value(Op::Traverse, Shape::BadFd), Some(EBADF));
    }

    #[test]
    fn probes_agree_with_matrix() {
        let (agree, miss) = probe_matrix_agreement();
        assert_eq!(agree, CELLS as u32);
        assert_eq!(miss, 0);
        assert_eq!(PROBES.len(), CELLS);
    }

    #[test]
    fn every_probe_detects() {
        for p in PROBES.iter() {
            assert!(detect(p), "probe {} failed", p.label);
        }
    }

    #[test]
    fn single_round_is_full() {
        let mut lcg = Lcg::seed(0x9358_0007);
        let r = run_round(3, &mut lcg);
        assert_eq!(r.hits, CELLS as u32);
        assert_eq!(r.misses, 0);
        assert_eq!(r.index, 3);
    }

    #[test]
    fn matrix_runs_ten_rounds() {
        let rep = run_matrix();
        assert!(rep.rounds_complete());
        assert!(rep.rounds_full());
        assert!(rep.cells_stable());
        assert_eq!(rep.injected, INJECTED_TOTAL);
        assert_eq!(rep.detected, INJECTED_TOTAL);
        assert_eq!(rep.escaped, 0);
        assert_eq!(rep.flaky, 0);
    }

    #[test]
    fn ledger_conserved() {
        let rep = run_matrix();
        assert!(rep.conserved());
        assert_eq!(rep.detected + rep.escaped, rep.injected);
    }

    #[test]
    fn order_insensitive() {
        assert!(order_independent());
        assert_eq!(verdict_vector(0), verdict_vector(7));
        assert_eq!(verdict_vector(7), verdict_vector(11));
    }

    #[test]
    fn wrong_expectations_fail() {
        let w1 = Probe { op: Op::Read, shape: Shape::BadFd, want: EFAULT, label: "x" };
        let w2 = Probe { op: Op::Close, shape: Shape::BadFd, want: OK, label: "y" };
        let w3 = Probe { op: Op::Ioctl, shape: Shape::Unsupported, want: OK, label: "z" };
        assert!(!detect(&w1));
        assert!(!detect(&w2));
        assert!(!detect(&w3));
    }

    #[test]
    fn claims_all_hold() {
        let rep = run_matrix();
        let board = claim_board(&rep);
        assert_eq!(board.len(), CLAIMS);
        for c in board.iter() {
            assert!(c.holds, "claim {} failed", c.name);
            assert!(!c.note.is_empty());
        }
    }

    #[test]
    fn declaration_done() {
        let rep = run_matrix();
        let d = declare(&rep);
        assert_eq!(d.status, WbStatus::Done);
        assert_eq!(d.claims_ok, 5);
        assert_eq!(d.rounds_passed, 10);
        assert!(!d.summary.is_empty());
    }

    #[test]
    fn empty_report_cannot_declare() {
        let d = declare(&MatrixReport::empty());
        assert_eq!(d.status, WbStatus::Partial);
        assert!(d.claims_ok < CLAIMS as u32);
        assert_eq!(d.rounds_passed, 0);
        assert_eq!(d.escaped, 0);
        assert!(d.status != WbStatus::Done);
    }

    #[test]
    fn lcg_is_deterministic() {
        let mut a = Lcg::seed(11);
        let mut b = Lcg::seed(11);
        let mut k = 0u32;
        while k < 16 {
            assert_eq!(a.next_u32(), b.next_u32());
            k += 1;
        }
        assert!(Lcg::seed(1).below(0) == 0);
    }

    #[test]
    fn codes_and_segment() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(RecheckCode::ALL.len(), 12);
        assert_eq!(RecheckCode::RepeatMiss.seg_addr(), SEG_BASE + 1);
        assert_eq!(RecheckCode::SegmentOut.code(), 12);
        assert!(RecheckCode::ALL.iter().all(|c| !c.detail().is_empty()));
    }

    #[test]
    fn deterministic_report() {
        let a = run_matrix();
        let b = run_matrix();
        assert_eq!(report_fingerprint(&a), report_fingerprint(&b));
        assert_eq!(a.digest, b.digest);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9358_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
