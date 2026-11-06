//! UNX-F9297 E 型判据可复测性声明（10/10 次检出） · 0x7B7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9297 逐字）：
//!   UNX-F9297-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky
//!   声明格过。
//!
//! 设计要点：
//!   ① 矩阵 12 行 × 10 轮。10 行「必检出」正例承载进程族 E 型错误语义，2 行
//!      「必不检出」陷阱例是反恒真对照：检测器若退化成恒真，陷阱行立刻从
//!      10/10 掉到 0/10。没有陷阱例的 10/10 只是同义反复，不构成可复测性。
//!   ② 十轮不是把同一常量算十遍：同一 errno 每轮以三种线上表示喂入——正值
//!      （测试桩直读）、负值 -errno（内核返回约定）、哨兵 -1 加旁路 errno
//!      （libc 对用户面约定）。解码必须落回同一数值，测的是表示不变性。
//!   ③ 轮序轮转：第 r 轮从 (r % 12) 行起绕圈，正序与轮转两次全跑逐格比对，
//!      任一格翻转即 flaky。判据若依赖遍历次序，顺序一换就露馅。
//!   ④ 统计格是结构体五元组（rows/rounds/hits/misses/perfect）而非散计数；
//!      perfect 必须等于 rows，misses 必须为零，hits 必须等于 120。
//!   ⑤ 注入账 injected == detected + escaped 且 escaped == 0，逐格记账。
//!   ⑥ 声明状态机 Blank→Injected→Measured→Declared，Revolted 只能回 Injected
//!      重测；写回空摘要回 EINVAL，完成后二次写回回 EBUSY。
//!   ⑦ 哨兵歧义显性化：负值表示下 -1 与 -EPERM 不可区分，解码器对 ret == -1
//!      一律回 None，绝不猜成 EPERM(1)。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9297 独占 0x7B7C0..=0x7B7FF）。
pub const SEG_BASE: u32 = 0x7B7C0;
/// 段窗口宽度（下一张卡 F9298 从 0x7B800 起）。
pub const SEG_SPAN: u32 = 0x40;
/// 矩阵行数：10 正例 + 2 陷阱例。
pub const ROWS: usize = 12;
/// 重复注入轮数。
pub const ROUNDS: usize = 10;
/// 矩阵总格数。
pub const CELLS: u32 = (ROWS as u32) * (ROUNDS as u32);
/// 正例行数。
pub const POSITIVE_ROWS: u32 = 10;
/// 陷阱例行数（负对照）。
pub const TRAP_ROWS: u32 = 2;
/// 每轮使用的线上表示种类数。
pub const WIRE_FORMS: u32 = 3;

// ── Linux errno 数值（正数口径，逐字对齐头文件） ─────────────────────
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
pub const ENOTTY: i32 = 25;
pub const EROFS: i32 = 30;
pub const ENOSYS: i32 = 38;

/// 以 errno 数值为位序构造掩码位；越界（<=0 或 >=64）一律 0，绝不回绕。
pub const fn errno_bit(errno: i32) -> u64 {
    if errno <= 0 || errno >= 64 { 0 } else { 1u64 << (errno as u32) }
}

// ── 家族 errno 归属表（检测器底座） ──────────────────────────────────
pub const FILE_MASK: u64 = errno_bit(EBADF) | errno_bit(ENOENT) | errno_bit(EACCES)
    | errno_bit(EEXIST) | errno_bit(EISDIR) | errno_bit(ENOTDIR) | errno_bit(EROFS)
    | errno_bit(EFAULT) | errno_bit(EINVAL);
pub const PROC_MASK: u64 = errno_bit(EAGAIN) | errno_bit(ENOMEM) | errno_bit(EINTR)
    | errno_bit(ECHILD) | errno_bit(ESRCH) | errno_bit(EINVAL) | errno_bit(EPERM);
pub const EXEC_MASK: u64 = errno_bit(ENOEXEC) | errno_bit(E2BIG) | errno_bit(EACCES)
    | errno_bit(ENOENT) | errno_bit(ENOMEM) | errno_bit(EFAULT);
pub const WAIT_MASK: u64 = errno_bit(ECHILD) | errno_bit(EINTR) | errno_bit(EINVAL) | errno_bit(ESRCH);
pub const MEM_MASK: u64 = errno_bit(ENOMEM) | errno_bit(EINVAL) | errno_bit(EFAULT);
pub const IO_MASK: u64 = errno_bit(EBADF) | errno_bit(EFAULT) | errno_bit(EINVAL)
    | errno_bit(EINTR) | errno_bit(ENOTTY) | errno_bit(EAGAIN);
pub const NET_MASK: u64 = errno_bit(EAGAIN) | errno_bit(EINTR) | errno_bit(ENOMEM)
    | errno_bit(EFAULT) | errno_bit(ENOSYS);
pub const TTY_MASK: u64 = errno_bit(ENOTTY) | errno_bit(EINVAL) | errno_bit(EFAULT) | errno_bit(EBADF);

/// 错误语义家族。矩阵每行绑定一个家族，检测器按家族闭集判 errno 归属。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    File,
    Proc,
    Exec,
    Wait,
    Mem,
    Io,
    Net,
    Tty,
}

impl Family {
    /// 家族编码（指纹用，0 保留给未知）。
    pub fn tag(self) -> u8 {
        match self {
            Family::File => 1, Family::Proc => 2, Family::Exec => 3, Family::Wait => 4,
            Family::Mem => 5, Family::Io => 6, Family::Net => 7, Family::Tty => 8,
        }
    }

    /// 闭集掩码。
    pub fn mask(self) -> u64 {
        match self {
            Family::File => FILE_MASK, Family::Proc => PROC_MASK,
            Family::Exec => EXEC_MASK, Family::Wait => WAIT_MASK,
            Family::Mem => MEM_MASK, Family::Io => IO_MASK,
            Family::Net => NET_MASK, Family::Tty => TTY_MASK,
        }
    }
}

/// 家族闭集判定：errno 是否落在该家族可回闭集内。
pub fn detect(family: Family, errno: i32) -> bool {
    family.mask() & errno_bit(errno) != 0
}

// ── 线上表示与解码 ────────────────────────────────────────────────────
/// errno 的线上表示。三轮一循环，故每行十轮里三种形式都被覆盖到。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireForm {
    /// 正值：测试桩直接返回 errno 数值。
    Positive,
    /// 负值：内核内部 -errno 返回约定。
    Negative,
    /// 哨兵：返回 -1，errno 走旁路（libc 对用户面约定）。
    RetOne,
}

/// 一次线上观测：返回槽 + 旁路 errno（仅哨兵形式使用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub form: WireForm,
    pub ret: i32,
    pub side_errno: i32,
}

/// 按形式编码一个 errno；负值形式的减法走 checked_sub，溢出退化为 0。
pub fn probe_for(form: WireForm, errno: i32) -> Probe {
    match form {
        WireForm::Positive => Probe { form, ret: errno, side_errno: 0 },
        WireForm::Negative => {
            let ret = match 0i32.checked_sub(errno) { Some(v) => v, None => 0 };
            Probe { form, ret, side_errno: 0 }
        }
        WireForm::RetOne => Probe { form, ret: -1, side_errno: errno },
    }
}

/// 解码回 errno。三种形式必须归一，否则十轮测的不是同一件事。
///
/// 负值形式对 ret == -1 一律回 None：-1 既是哨兵又是 -EPERM，语义上不可区分，
/// 猜成 EPERM 会把「哨兵」误判成「权限错」，是这类判据最经典的自欺路径。
pub fn decode(p: &Probe) -> Option<i32> {
    match p.form {
        WireForm::Positive => if p.ret > 0 { Some(p.ret) } else { None },
        WireForm::Negative => if p.ret < -1 { p.ret.checked_neg() } else { None },
        WireForm::RetOne => if p.ret == -1 && p.side_errno > 0 { Some(p.side_errno) } else { None },
    }
}

/// 第 round 轮使用的表示。
pub fn form_for_round(round: usize) -> WireForm {
    match round % (WIRE_FORMS as usize) {
        0 => WireForm::Positive,
        1 => WireForm::Negative,
        _ => WireForm::RetOne,
    }
}

// ── 矩阵行表 ──────────────────────────────────────────────────────────
/// 一行 E 型判据：家族 + 注入 errno + 是否必须检出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EtypeRow {
    /// 行号（1..=12，冻结，不得重排）
    pub id: u16,
    /// 错误语义家族
    pub family: Family,
    /// 注入的 errno
    pub injected: i32,
    /// 是否必须检出（false = 负对照陷阱）
    pub must_fire: bool,
}

/// 冻结行表。前 10 行为正例，后 2 行为陷阱：第 11 行把 ENOENT 塞进进程族
/// （ENOENT 不属进程闭集），第 12 行把 ENOTTY 塞进装载族（终端错不属装载闭集）。
pub const ETYPE_ROWS: [EtypeRow; ROWS] = [
    EtypeRow { id: 1, family: Family::File, injected: EBADF, must_fire: true },
    EtypeRow { id: 2, family: Family::File, injected: EISDIR, must_fire: true },
    EtypeRow { id: 3, family: Family::File, injected: ENOTDIR, must_fire: true },
    EtypeRow { id: 4, family: Family::File, injected: EROFS, must_fire: true },
    EtypeRow { id: 5, family: Family::Proc, injected: EAGAIN, must_fire: true },
    EtypeRow { id: 6, family: Family::Proc, injected: ECHILD, must_fire: true },
    EtypeRow { id: 7, family: Family::Exec, injected: ENOEXEC, must_fire: true },
    EtypeRow { id: 8, family: Family::Exec, injected: E2BIG, must_fire: true },
    EtypeRow { id: 9, family: Family::Wait, injected: ECHILD, must_fire: true },
    EtypeRow { id: 10, family: Family::Io, injected: ENOTTY, must_fire: true },
    EtypeRow { id: 11, family: Family::Proc, injected: ENOENT, must_fire: false },
    EtypeRow { id: 12, family: Family::Exec, injected: ENOTTY, must_fire: false },
];

/// 单行单轮判定：解码成功、数值不漂、归属与期望一致。
pub fn judge_row(row: &EtypeRow, round: usize) -> bool {
    match decode(&probe_for(form_for_round(round), row.injected)) {
        Some(errno) => errno == row.injected && detect(row.family, errno) == row.must_fire,
        None => false,
    }
}

/// 取行并按第 0 轮判定（清单自检与测试使用）。
pub fn row_matches(idx: usize) -> bool {
    match ETYPE_ROWS.get(idx) { Some(row) => judge_row(row, 0), None => false }
}

/// 正例行数。
pub fn positive_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ETYPE_ROWS.len() {
        if let Some(row) = ETYPE_ROWS.get(i) {
            if row.must_fire { n = n.saturating_add(1); }
        }
        i += 1;
    }
    n
}

/// 陷阱例行数。
pub fn trap_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ETYPE_ROWS.len() {
        if let Some(row) = ETYPE_ROWS.get(i) {
            if !row.must_fire { n = n.saturating_add(1); }
        }
        i += 1;
    }
    n
}

/// 行号是否连续 1..=ROWS 且无重复。
pub fn row_ids_contiguous() -> bool {
    let mut i = 0usize;
    while i < ETYPE_ROWS.len() {
        match ETYPE_ROWS.get(i) {
            Some(row) => if row.id as usize != i + 1 { return false; },
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 注入账与矩阵跑 ────────────────────────────────────────────────────
/// 注入账：injected == detected + escaped，escaped 必须为 0。
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

    /// 记一格。守恒被破坏时回 Err(EINVAL)——账本身也要被校验。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
        if self.detected.saturating_add(self.escaped) != self.injected { return Err(EINVAL); }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 一次全矩阵跑的产物。
#[derive(Clone, Copy, Debug)]
pub struct MatrixRun {
    /// 逐格命中位（1 = 与期望一致）
    pub grid: [[u8; ROUNDS]; ROWS],
    /// 逐行命中数（必须全为 ROUNDS）
    pub row_hits: [u8; ROWS],
    pub total_hits: u32,
    pub total_cells: u32,
    pub ledger: InjectLedger,
}

/// 全矩阵跑。`rotate = true` 时每轮轮转起始行，用于证明判定与次序无关。
pub fn run_matrix(rotate: bool) -> MatrixRun {
    let mut grid = [[0u8; ROUNDS]; ROWS];
    let mut ledger = InjectLedger::zero();
    let mut round = 0usize;
    while round < ROUNDS {
        let mut step = 0usize;
        while step < ROWS {
            let idx = if rotate { (round + step) % ROWS } else { step };
            let caught = match ETYPE_ROWS.get(idx) {
                Some(row) => judge_row(row, round),
                None => false,
            };
            if caught {
                if let Some(line) = grid.get_mut(idx) {
                    if let Some(cell) = line.get_mut(round) { *cell = 1; }
                }
            }
            // 记账失败只可能来自计数饱和：保留原账并原样返回，绝不重复计格。
            if ledger.record(caught).is_err() {
                return MatrixRun {
                    grid, row_hits: [0u8; ROWS], total_hits: 0,
                    total_cells: CELLS, ledger,
                };
            }
            step += 1;
        }
        round += 1;
    }

    let mut row_hits = [0u8; ROWS];
    let mut i = 0usize;
    while i < ROWS {
        let mut j = 0usize;
        let mut n = 0u8;
        while j < ROUNDS {
            if let Some(line) = grid.get(i) {
                if let Some(v) = line.get(j) {
                    if *v == 1 { n = n.saturating_add(1); }
                }
            }
            j += 1;
        }
        if let Some(slot) = row_hits.get_mut(i) { *slot = n; }
        i += 1;
    }

    let mut hits = 0u32;
    let mut k = 0usize;
    while k < row_hits.len() {
        if let Some(v) = row_hits.get(k) { hits = hits.saturating_add(*v as u32); }
        k += 1;
    }

    MatrixRun { grid, row_hits, total_hits: hits, total_cells: CELLS, ledger }
}

// ── 统计格与 flaky 比对 ───────────────────────────────────────────────
/// 10/10 检出统计格：五元组口径，不是散计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatsCell {
    pub rows: u32,
    pub rounds: u32,
    pub hits: u32,
    pub misses: u32,
    pub perfect: u32,
}

impl StatsCell {
    pub const fn zero() -> StatsCell {
        StatsCell { rows: 0, rounds: 0, hits: 0, misses: 0, perfect: 0 }
    }

    /// 从一次跑折叠出统计格。
    pub fn from_run(run: &MatrixRun) -> StatsCell {
        let rows = run.row_hits.len() as u32;
        let mut perfect = 0u32;
        let mut i = 0usize;
        while i < run.row_hits.len() {
            if let Some(v) = run.row_hits.get(i) {
                if *v == ROUNDS as u8 { perfect = perfect.saturating_add(1); }
            }
            i += 1;
        }
        let cells = rows.saturating_mul(ROUNDS as u32);
        let hits = run.total_hits.min(cells);
        StatsCell { rows, rounds: ROUNDS as u32, hits, misses: cells.saturating_sub(hits), perfect }
    }

    /// 10/10 统计格判据：行数、轮数、命中、满格行四项同时成立且零漏检。
    pub fn ten_of_ten(&self) -> bool {
        self.rows == ROWS as u32 && self.rounds == ROUNDS as u32
            && self.perfect == self.rows && self.misses == 0
    }

    /// 一行中文快照（动态明细用）。
    pub fn text(&self) -> &'static str {
        match (self.ten_of_ten(), self.misses) {
            (true, 0) => "十格全绿",
            (false, 0) => "有行未满十",
            _ => "存在漏检",
        }
    }
}

/// flaky 比对结果：比了多少格、几格翻转。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlakyCell {
    pub compared: u32,
    pub unstable: u32,
}

/// 逐格比较两次全跑。正序与轮转必须完全一致。
pub fn compare_runs(a: &MatrixRun, b: &MatrixRun) -> FlakyCell {
    let mut compared = 0u32;
    let mut unstable = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        let mut j = 0usize;
        while j < ROUNDS {
            let va = a.grid.get(i).and_then(|line| line.get(j)).copied().unwrap_or(9);
            let vb = b.grid.get(i).and_then(|line| line.get(j)).copied().unwrap_or(9);
            compared = compared.saturating_add(1);
            if va != vb { unstable = unstable.saturating_add(1); }
            j += 1;
        }
        i += 1;
    }
    FlakyCell { compared, unstable }
}

// ── 声明状态机 ────────────────────────────────────────────────────────
/// 声明阶段。只许逐级推进，Revolted 退回 Injected 重测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclPhase {
    Blank,
    Injected,
    Measured,
    Declared,
    Revolted,
}

impl DeclPhase {
    /// 阶段序（只能 +1 推进，撤销除外）。
    pub fn code(self) -> u8 {
        match self {
            DeclPhase::Blank => 0, DeclPhase::Injected => 1, DeclPhase::Measured => 2,
            DeclPhase::Declared => 3, DeclPhase::Revolted => 4,
        }
    }

    /// 是否允许重新注入（空白与撤销后可重来）。
    pub fn may_ingest(self) -> bool {
        matches!(self, DeclPhase::Blank | DeclPhase::Revolted)
    }
}

/// 声明记录：阶段 + 摘要长度 + 指纹 + 拒绝计数。
#[derive(Clone, Copy, Debug)]
pub struct Declaration {
    phase: DeclPhase,
    digest: u32,
    summary_len: usize,
    refused: u32,
}

impl Declaration {
    pub const fn new() -> Declaration {
        Declaration { phase: DeclPhase::Blank, digest: 0, summary_len: 0, refused: 0 }
    }

    pub fn phase(&self) -> DeclPhase { self.phase }
    pub fn digest(&self) -> u32 { self.digest }
    pub fn summary_len(&self) -> usize { self.summary_len }
    pub fn refused(&self) -> u32 { self.refused }

    /// 注入全矩阵。仅空白/撤销后允许；格数或命中不足回 EINVAL。
    pub fn ingest(&mut self, run: &MatrixRun) -> Result<(), i32> {
        if !self.phase.may_ingest() {
            self.refused = self.refused.saturating_add(1);
            return Err(EINVAL);
        }
        if run.total_cells != CELLS || run.total_hits != CELLS || !run.ledger.conserved() {
            self.refused = self.refused.saturating_add(1);
            return Err(EINVAL);
        }
        self.digest = run.total_hits;
        self.phase = DeclPhase::Injected;
        Ok(())
    }

    /// 测量统计格。未注入或统计格不满回 EINVAL。
    pub fn measure(&mut self, stats: &StatsCell) -> Result<(), i32> {
        if self.phase != DeclPhase::Injected || !stats.ten_of_ten() {
            self.refused = self.refused.saturating_add(1);
            return Err(EINVAL);
        }
        self.phase = DeclPhase::Measured;
        Ok(())
    }

    /// 声明。未测量或摘要为空回 EINVAL。
    pub fn declare(&mut self, summary: &str) -> Result<(), i32> {
        if self.phase != DeclPhase::Measured || summary.is_empty() {
            self.refused = self.refused.saturating_add(1);
            return Err(EINVAL);
        }
        self.summary_len = summary.len();
        self.phase = DeclPhase::Declared;
        Ok(())
    }

    /// 撤销。仅已声明可撤销；撤销后摘要清零、必须重测。
    pub fn revolt(&mut self) -> Result<(), i32> {
        if self.phase != DeclPhase::Declared {
            self.refused = self.refused.saturating_add(1);
            return Err(EINVAL);
        }
        self.phase = DeclPhase::Revolted;
        self.summary_len = 0;
        Ok(())
    }
}

// ── 写回记录 ──────────────────────────────────────────────────────────
/// 写回状态（API/软件写回：状态=已完成）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteState {
    Pending,
    Completed,
}

/// 写回记录：空摘要拒、完成后再写拒。
#[derive(Clone, Copy, Debug)]
pub struct WriteBack {
    state: WriteState,
    summary_len: usize,
    digest: u32,
    attempts: u32,
    refusals: u32,
}

impl WriteBack {
    pub const fn new() -> WriteBack {
        WriteBack { state: WriteState::Pending, summary_len: 0, digest: 0, attempts: 0, refusals: 0 }
    }

    pub fn state(&self) -> WriteState { self.state }
    pub fn is_completed(&self) -> bool { self.state == WriteState::Completed }
    pub fn summary_len(&self) -> usize { self.summary_len }
    pub fn digest(&self) -> u32 { self.digest }
    pub fn attempts(&self) -> u32 { self.attempts }
    pub fn refusals(&self) -> u32 { self.refusals }

    /// 提交写回。空摘要回 EINVAL 且状态不变；已完成再写回 EBUSY。
    pub fn submit(&mut self, summary: &str, digest: u32) -> Result<(), i32> {
        self.attempts = self.attempts.saturating_add(1);
        if self.state == WriteState::Completed {
            self.refusals = self.refusals.saturating_add(1);
            return Err(EBUSY);
        }
        if summary.is_empty() {
            self.refusals = self.refusals.saturating_add(1);
            return Err(EINVAL);
        }
        self.summary_len = summary.len();
        self.digest = digest;
        self.state = WriteState::Completed;
        Ok(())
    }
}

// ── 确定性指纹（FNV-1a 32 位，逐字节喂入） ───────────────────────────
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节喂入。
pub fn fnv1a_step(h: u32, byte: u8) -> u32 {
    (h ^ byte as u32).wrapping_mul(FNV_PRIME)
}

/// 矩阵 + 统计格指纹：逐格喂命中位，再喂行标签与统计五元组。
pub fn matrix_fingerprint(run: &MatrixRun, stats: &StatsCell) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < ROWS {
        if let Some(row) = ETYPE_ROWS.get(i) {
            h = fnv1a_step(h, row.family.tag());
            h = fnv1a_step(h, (row.id & 0xFF) as u8);
        }
        let mut j = 0usize;
        while j < ROUNDS {
            let v = run.grid.get(i).and_then(|line| line.get(j)).copied().unwrap_or(0);
            h = fnv1a_step(h, if v == 1 { 0xA5 } else { 0x5A });
            j += 1;
        }
        i += 1;
    }
    h = fnv1a_step(h, (stats.hits & 0xFF) as u8);
    h = fnv1a_step(h, ((stats.hits >> 8) & 0xFF) as u8);
    h = fnv1a_step(h, (stats.perfect & 0xFF) as u8);
    h = fnv1a_step(h, (stats.misses & 0xFF) as u8);
    h
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（序号 1..=14，段内码 = SEG_BASE + 序号）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecheckCode {
    GridShape = 1,
    RowIdGap = 2,
    BalanceBroken = 3,
    RowShort = 4,
    StatsNotFull = 5,
    Flaky = 6,
    DecodeLost = 7,
    SentinelAmbiguity = 8,
    FamilyMismatch = 9,
    LedgerBroken = 10,
    PhaseSkip = 11,
    EmptySummary = 12,
    SegmentViolation = 13,
    DigestDrift = 14,
}

impl RecheckCode {
    /// 序号。
    pub fn code(self) -> u16 { self as u16 }

    /// 段内码值（两两唯一）。
    pub fn seg_code(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RecheckCode::GridShape => "矩阵形状不合约：必须是 12 行 × 10 轮；请核对行表长度与轮常数",
            RecheckCode::RowIdGap => "行号不连续：行表 id 必须 1..=12 且不得重排；请恢复冻结次序",
            RecheckCode::BalanceBroken => "正负对照失衡：必须 10 正例 + 2 陷阱例；缺陷阱则 10/10 无判别力",
            RecheckCode::RowShort => "单行未满十：该行十轮里至少一轮判定与期望不符；请查该行的表示解码",
            RecheckCode::StatsNotFull => "统计格不满：perfect 未等于 rows 或 misses 非零；请回看逐行命中",
            RecheckCode::Flaky => "双跑翻转：正序与轮转存在不一致格；请排查遍历次序依赖",
            RecheckCode::DecodeLost => "解码失败：三种线上表示未归一为同一 errno；请检查返回值域判定",
            RecheckCode::SentinelAmbiguity => "哨兵歧义：负值 -1 被误读为 EPERM；请对 ret == -1 回 None",
            RecheckCode::FamilyMismatch => "家族归属错：errno 不在该家族闭集；请修掩码而非改期望",
            RecheckCode::LedgerBroken => "注入账不守恒：injected 不等于 detected+escaped；请查逐格记账",
            RecheckCode::PhaseSkip => "声明越级：未测量即声明或撤销后直接声明；请逐级推进",
            RecheckCode::EmptySummary => "结果摘要为空：写回必须携带非空摘要；请补摘要文本",
            RecheckCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B7C0 且 0x40 对齐",
            RecheckCode::DigestDrift => "指纹漂移：两次矩阵指纹不等；请排查隐藏状态或非确定遍历",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [RecheckCode] = &[
        RecheckCode::GridShape, RecheckCode::RowIdGap, RecheckCode::BalanceBroken,
        RecheckCode::RowShort, RecheckCode::StatsNotFull, RecheckCode::Flaky,
        RecheckCode::DecodeLost, RecheckCode::SentinelAmbiguity, RecheckCode::FamilyMismatch,
        RecheckCode::LedgerBroken, RecheckCode::PhaseSkip, RecheckCode::EmptySummary,
        RecheckCode::SegmentViolation, RecheckCode::DigestDrift,
    ];
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9297 判据集（静态 17 条 + 动态 3 条）。
pub fn run_unx9297_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9297");

    let seg_ok = SEG_BASE % SEG_SPAN == 0 && SEG_BASE == 0x7B7C0
        && SEG_BASE + SEG_SPAN <= 0x7B900;
    s.add("段位守卫：0x7B7C0 独占 0x40 窗口", seg_ok,
          "段基址须 0x40 对齐且落在本批窗口，不得与 F9296/F9298 窗口重叠");

    s.add("矩阵形状：12 行 × 10 轮 = 120 格",
          ETYPE_ROWS.len() == ROWS && ROWS.saturating_mul(ROUNDS) == CELLS as usize && CELLS == 120,
          "行数与轮数必须与统计格口径一致，否则 10/10 无从复算");

    s.add("正负对照：10 正例 + 2 陷阱例",
          positive_rows() == POSITIVE_ROWS && trap_rows() == TRAP_ROWS && row_ids_contiguous(),
          "陷阱例是反恒真对照；缺了它，10/10 只是同义反复");

    // 正序与轮转各跑一次：既是检出统计的输入，也是 flaky 比对的输入。
    let run_a = run_matrix(false);
    let run_b = run_matrix(true);
    let all_rows_full = run_a.row_hits.iter().all(|n| *n == ROUNDS as u8);
    s.add("全矩阵：10 次重复注入逐行 10/10 检出",
          all_rows_full && run_a.row_hits.len() == ROWS && run_a.total_hits == CELLS,
          "12 行 × 10 轮每格判定都必须与冻结期望一致，不允许某行靠别的行补分");

    let stats = StatsCell::from_run(&run_a);
    s.add("统计格：120 格全命中、零漏检、12 行满格",
          stats.ten_of_ten() && stats.hits == CELLS && stats.misses == 0
              && stats.perfect == ROWS as u32,
          "统计格是五元组口径：rows/rounds/hits/misses/perfect 必须同时成立");

    s.add("注入账：injected = detected + escaped 且逃逸为零",
          run_a.ledger.conserved() && run_a.ledger.escaped == 0
              && run_a.ledger.injected == CELLS && run_a.ledger.detected == CELLS,
          "逐格记账必须守恒；逃逸非零意味着有格没被判定就滑过去了");

    let flaky = compare_runs(&run_a, &run_b);
    s.add("零 flaky：正序与轮转两次全跑逐格一致",
          flaky.unstable == 0 && flaky.compared == CELLS,
          "轮序不得影响判定；任何一格翻转即 flaky，声明必须被拒");

    s.add("陷阱行：跨家族 errno 不被误检",
          !detect(Family::Proc, ENOENT) && !detect(Family::Exec, ENOTTY)
              && row_matches(10) && row_matches(11),
          "检测器不得恒真：陷阱行以「不检出」为期望命中，才证明它有判别力");

    s.add("表示不变性：三种线上形式解码回同一 errno",
          decode(&probe_for(WireForm::Positive, EBADF)) == Some(EBADF)
              && decode(&probe_for(WireForm::Negative, EBADF)) == Some(EBADF)
              && decode(&probe_for(WireForm::RetOne, EBADF)) == Some(EBADF)
              && decode(&probe_for(WireForm::Positive, E2BIG)) == Some(E2BIG),
          "正值/负值/哨兵三表示必须归一，否则十轮测的不是同一件事");

    s.add("哨兵歧义：负值 -1 不猜成 EPERM",
          decode(&probe_for(WireForm::Negative, EPERM)).is_none()
              && decode(&probe_for(WireForm::RetOne, EBADF)).map(|e| e == EBADF) == Some(true)
              && decode(&probe_for(WireForm::Positive, 0)).is_none(),
          "-1 与 -EPERM 在负值表示下不可区分，解码器必须回 None 而不是猜 1");

    s.add("家族掩码：EAGAIN 属进程与网络、不属文件",
          detect(Family::Proc, EAGAIN) && detect(Family::Net, EAGAIN)
              && !detect(Family::File, EAGAIN) && detect(Family::Wait, ECHILD)
              && !detect(Family::Mem, ECHILD) && Family::File.tag() != Family::Proc.tag(),
          "errno 归属表是判据底座，错一位就会把陷阱算成正例");

    s.add("errno 数值：与 Linux 头文件逐一相等",
          EBADF == 9 && ECHILD == 10 && EAGAIN == 11 && ENOMEM == 12 && EINVAL == 22
              && ENOTTY == 25 && ENOEXEC == 8 && E2BIG == 7 && EROFS == 30 && ENOSYS == 38,
          "码值不得自造；与内核 errno 常量不一致的判据无法与线上语义对齐");

    let mut decl = Declaration::new();
    let early = decl.declare("未测量就想声明");
    let ingested = decl.ingest(&run_a);
    let measured = decl.measure(&stats);
    let stated = decl.declare("12 行 10/10 检出，零 flaky");
    let stated_len = decl.summary_len();
    let revolted = decl.revolt();
    let after = decl.declare("撤销后直接重声明");
    s.add("声明状态机：越级拒绝、撤销后必须重测",
          early == Err(EINVAL) && ingested.is_ok() && measured.is_ok() && stated.is_ok()
              && stated_len > 0 && revolted.is_ok() && decl.phase() == DeclPhase::Revolted
              && after == Err(EINVAL),
          "Blank→Injected→Measured→Declared 只许逐级推进，Revolted 必须回到 Injected");

    let mut wb = WriteBack::new();
    let empty = wb.submit("", 0);
    let still_pending = !wb.is_completed() && wb.state() == WriteState::Pending;
    let ok = wb.submit("状态=已完成；摘要=12 行 10/10 零 flaky", run_a.total_hits);
    let dup = wb.submit("重复写回", 0);
    s.add("写回：空摘要 EINVAL、非空完成、二次写回 EBUSY",
          empty == Err(EINVAL) && still_pending && ok.is_ok() && wb.is_completed()
              && dup == Err(EBUSY) && wb.refusals() == 2 && wb.attempts() == 3,
          "写回语义：拒绝不得改状态，完成后再写必须显性拒绝并记账");

    s.add("写回：结果摘要非空且长度落账",
          wb.summary_len() == "状态=已完成；摘要=12 行 10/10 零 flaky".len()
              && wb.summary_len() > 0 && wb.digest() == run_a.total_hits,
          "结果摘要非空是写回判据的硬要求，长度为零即视同未写");

    let fp_a = matrix_fingerprint(&run_a, &stats);
    let fp_b = matrix_fingerprint(&run_b, &stats);
    s.add("双跑一致：矩阵指纹相等", fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相等；指纹漂移即隐藏状态");

    let mut uniq = true;
    let mut i = 0usize;
    while i < RecheckCode::ALL.len() {
        let mut j = i + 1;
        while j < RecheckCode::ALL.len() {
            if let (Some(a), Some(b)) = (RecheckCode::ALL.get(i), RecheckCode::ALL.get(j)) {
                if a.seg_code() == b.seg_code() || a.code() == b.code() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    let codes_ok = uniq && RecheckCode::ALL.len() == 14
        && RecheckCode::ALL.iter().all(|c| !c.detail().is_empty() && c.seg_code() > SEG_BASE);
    s.add("诊断码闭集：14 码、段内唯一、文案非空", codes_ok,
          "闭集码必须两两唯一且每码带可操作建议，否则红项无法定位");

    // ── 动态明细（3 条，不超过侧表 8 槽） ─────────────────────────────
    let stats_text = format!("统计格 {}/{} 行满格，命中 {}/{}，漏检 {}，态 {}",
                             stats.perfect, stats.rows, stats.hits, CELLS, stats.misses, stats.text());
    s.add_dyn("统计格快照", stats.ten_of_ten(), &stats_text);

    let flaky_text = format!("flaky 比对 {}/{} 格，翻转 {}（正序 vs 轮转）",
                             flaky.compared, CELLS, flaky.unstable);
    s.add_dyn("零 flaky 快照", flaky.unstable == 0, &flaky_text);

    let fp_text = format!("矩阵指纹 0x{:08X}，段内码基 0x{:05X}", fp_a, SEG_BASE);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_table_is_frozen_and_balanced() {
        assert_eq!(ETYPE_ROWS.len(), ROWS);
        assert_eq!(positive_rows(), 10);
        assert_eq!(trap_rows(), 2);
        assert_eq!(row_ids_contiguous(), true);
        assert_eq!(ETYPE_ROWS.get(0).map(|r| r.id), Some(1));
        assert_eq!(ETYPE_ROWS.get(11).map(|r| r.id), Some(12));
    }

    #[test]
    fn wire_forms_decode_to_same_errno() {
        let forms = [WireForm::Positive, WireForm::Negative, WireForm::RetOne];
        for f in forms.iter() {
            assert_eq!(decode(&probe_for(*f, EBADF)), Some(EBADF));
            assert_eq!(decode(&probe_for(*f, E2BIG)), Some(E2BIG));
        }
        assert_eq!(form_for_round(0), WireForm::Positive);
        assert_eq!(form_for_round(1), WireForm::Negative);
        assert_eq!(form_for_round(2), WireForm::RetOne);
        assert_eq!(form_for_round(9), WireForm::Positive);
    }

    #[test]
    fn sentinel_minus_one_is_not_eperm() {
        assert_eq!(decode(&probe_for(WireForm::Negative, EPERM)), None);
        assert_eq!(decode(&probe_for(WireForm::RetOne, EPERM)), Some(EPERM));
        assert_eq!(decode(&probe_for(WireForm::Positive, -9)), None);
    }

    #[test]
    fn family_masks_are_disjoint_where_expected() {
        assert_eq!(detect(Family::File, EBADF), true);
        assert_eq!(detect(Family::Proc, EBADF), false);
        assert_eq!(detect(Family::Proc, EAGAIN), true);
        assert_eq!(detect(Family::File, EAGAIN), false);
        assert_eq!(detect(Family::Exec, ENOTTY), false);
        assert_eq!(errno_bit(0), 0);
        assert_eq!(errno_bit(64), 0);
    }

    #[test]
    fn matrix_runs_ten_of_ten() {
        let run = run_matrix(false);
        assert_eq!(run.total_cells, 120);
        assert_eq!(run.total_hits, 120);
        for n in run.row_hits.iter() {
            assert_eq!(*n, ROUNDS as u8);
        }
        let st = StatsCell::from_run(&run);
        assert_eq!(st.ten_of_ten(), true);
        assert_eq!(st.misses, 0);
        assert_eq!(st.perfect, 12);
    }

    #[test]
    fn rotation_does_not_flake() {
        let a = run_matrix(false);
        let b = run_matrix(true);
        let f = compare_runs(&a, &b);
        assert_eq!(f.compared, 120);
        assert_eq!(f.unstable, 0);
        let sa = StatsCell::from_run(&a);
        let sb = StatsCell::from_run(&b);
        assert_eq!(matrix_fingerprint(&a, &sa), matrix_fingerprint(&b, &sb));
    }

    #[test]
    fn ledger_is_conserved() {
        let run = run_matrix(true);
        assert_eq!(run.ledger.conserved(), true);
        assert_eq!(run.ledger.escaped, 0);
        assert_eq!(run.ledger.injected, 120);
        assert_eq!(run.ledger.detected, 120);
    }

    #[test]
    fn declaration_rejects_phase_skips() {
        let run = run_matrix(false);
        let st = StatsCell::from_run(&run);
        let mut d = Declaration::new();
        assert_eq!(d.declare("x"), Err(EINVAL));
        assert_eq!(d.measure(&st), Err(EINVAL));
        assert_eq!(d.ingest(&run), Ok(()));
        assert_eq!(d.measure(&st), Ok(()));
        assert_eq!(d.declare(""), Err(EINVAL));
        assert_eq!(d.declare("ok"), Ok(()));
        assert_eq!(d.revolt(), Ok(()));
        assert_eq!(d.phase(), DeclPhase::Revolted);
        assert_eq!(d.ingest(&run), Ok(()));
    }

    #[test]
    fn writeback_requires_summary_once() {
        let mut wb = WriteBack::new();
        assert_eq!(wb.submit("", 7), Err(EINVAL));
        assert_eq!(wb.is_completed(), false);
        assert_eq!(wb.submit("已完成", 7), Ok(()));
        assert_eq!(wb.digest(), 7);
        assert_eq!(wb.submit("again", 8), Err(EBUSY));
        assert_eq!(wb.refusals(), 2);
    }

    #[test]
    fn codes_are_unique_and_documented() {
        assert_eq!(RecheckCode::ALL.len(), 14);
        assert_eq!(RecheckCode::GridShape.code(), 1);
        assert_eq!(RecheckCode::Flaky.seg_code(), SEG_BASE + 6);
        for c in RecheckCode::ALL.iter() {
            assert_eq!(c.detail().is_empty(), false);
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9297_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
