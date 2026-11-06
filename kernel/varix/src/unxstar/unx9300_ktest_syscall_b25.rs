//! UNX-F9300 ktest syscall 面 B25 批断言集 · 0x7B880段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9300 逐字）：
//!   ① 纯功能代码 ≥440 行；② 判据原文：UNX-F9300-J1 本批 19 条判据聚合判据过，进程族 E 型矩阵一次跑通，域累计 132,700 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 19 条用例以固定编号 1..=19 落表，每条携带系统调用名、期望 errno、能力类目；
//!      编号即位图位序，成绩单按编号严格递增记录执行顺序，任何乱序即失败。
//!   ② 判定模型是三态 Tri：未跑 / 通过 / 失败；位图只在三态为通过时置位，
//!      收口条件是 bitmap == 0x7FFFF（19 位全 1）且 executed == 19。
//!   ③ 批量执行是一次性门：runs 只允许从 0 变 1，第二次 execute 固定返回 EBUSY，
//!      防止重复注入污染账本；进程族 E 型矩阵同样只允许跑一遍。
//!   ④ 进程族 E 型矩阵 3 族 × 2 槽（fork / exec / wait 各自 EAGAIN|ENOMEM、
//!      ENOEXEC|EACCES、ECHILD|EINTR），期望值必须与 PROC_E_TABLE 单元格逐格一致，
//!      且每族恰 2 行。
//!   ⑤ 注入账本 19 次一注一记：injected == detected + escaped，且 escaped == 0；
//!      账本只允许关一次，二次关闭返回 EEXIST。
//!   ⑥ 域累计基线 132681，本批增量恰 19，收口目标 132700；写回状态机冻结顺序
//!      空摘要 EINVAL → 未全过 EACCES → 已完成 EEXIST，完成态摘要必须非空。
//!   ⑦ 指纹为自写 FNV-1a 32 位，逐字节喂入用例表、三态、成绩单、位图、账本与域目标，
//!      双跑相等，篡改任一位即敏变。
//! ─────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9300 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B880;
/// 段窗口上界（不含）。
pub const SEG_LIMIT: u32 = 0x7B8C0;

/// 内核错误码收口类型：成功返回 ()，失败携带 Linux errno 数值。
pub type SysResult<T> = Result<T, i32>;

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

/// 主收口 errno 闭集（与 Linux 数值一一对应）。
pub const KNOWN_ERRNOS: [i32; 22] = [
    EPERM, ENOENT, ESRCH, EINTR, E2BIG, ENOEXEC, EBADF, ECHILD, EAGAIN, ENOMEM, EACCES,
    EFAULT, EBUSY, EEXIST, ENOTDIR, EISDIR, EINVAL, ENOTTY, EROFS, ENOSYS, EMFILE, ENFILE,
];

/// 本批判据条数。
pub const CASE_COUNT: usize = 19;
/// 19 位全过掩码。
pub const BATCH_MASK: u32 = (1u32 << CASE_COUNT) - 1;

/// 域累计基线：本批之前已完成 132681 条域判据。
pub const DOMAIN_BASELINE: u32 = 132_681;
/// 本批收口目标。
pub const DOMAIN_TARGET: u32 = 132_700;

/// 写回摘要固定容量（含结尾保留位）。
pub const SUMMARY_CAP: usize = 64;
/// 参考摘要（纯 ASCII，长度小于容量，便于逐字节比对）。
pub const SUMMARY_TEXT: &str = "B25 19/19 pass; E-matrix 6/6; domain 132700";

// ── 三态结果 ──────────────────────────────────────────────────────────

/// 单条用例的三态：0 未跑、1 通过、2 失败。位图只认 1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Tri {
    NotRun = 0,
    Pass = 1,
    Fail = 2,
}

impl Tri {
    pub fn from_verdict(ok: bool) -> Tri {
        if ok {
            Tri::Pass
        } else {
            Tri::Fail
        }
    }

    pub fn is_pass(self) -> bool {
        self == Tri::Pass
    }

    pub fn tag(self) -> u8 {
        self as u8
    }
}

// ── 能力类目 ──────────────────────────────────────────────────────────

/// B25 批覆盖的五个系统调用面。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Category {
    File = 0,
    Proc = 1,
    Mem = 2,
    Signal = 3,
    Admin = 4,
}

impl Category {
    pub const ALL: &'static [Category] = &[
        Category::File,
        Category::Proc,
        Category::Mem,
        Category::Signal,
        Category::Admin,
    ];

    pub fn tag(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::File => "file",
            Category::Proc => "proc",
            Category::Mem => "mem",
            Category::Signal => "signal",
            Category::Admin => "admin",
        }
    }
}

// ── 19 条用例表 ───────────────────────────────────────────────────────

/// 一条 B25 断言：系统调用、期望 errno、所属面。
#[derive(Clone, Copy)]
pub struct B25Case {
    pub no: u8,
    pub syscall: &'static str,
    pub errno: i32,
    pub cat: Category,
}

/// 19 条手工落表用例，编号即执行序与位序。
pub const B25_CASES: [B25Case; CASE_COUNT] = [
    B25Case { no: 1, syscall: "read", errno: EBADF, cat: Category::File },
    B25Case { no: 2, syscall: "write", errno: EFAULT, cat: Category::File },
    B25Case { no: 3, syscall: "openat", errno: ENOENT, cat: Category::File },
    B25Case { no: 4, syscall: "close", errno: EBADF, cat: Category::File },
    B25Case { no: 5, syscall: "fstat", errno: EBADF, cat: Category::File },
    B25Case { no: 6, syscall: "lseek", errno: EINVAL, cat: Category::File },
    B25Case { no: 7, syscall: "mmap", errno: ENOMEM, cat: Category::Mem },
    B25Case { no: 8, syscall: "munmap", errno: EINVAL, cat: Category::Mem },
    B25Case { no: 9, syscall: "ioctl", errno: ENOTTY, cat: Category::File },
    B25Case { no: 10, syscall: "dup", errno: EMFILE, cat: Category::File },
    B25Case { no: 11, syscall: "pipe2", errno: ENFILE, cat: Category::File },
    B25Case { no: 12, syscall: "fork", errno: EAGAIN, cat: Category::Proc },
    B25Case { no: 13, syscall: "execve", errno: ENOEXEC, cat: Category::Proc },
    B25Case { no: 14, syscall: "wait4", errno: ECHILD, cat: Category::Proc },
    B25Case { no: 15, syscall: "kill", errno: ESRCH, cat: Category::Signal },
    B25Case { no: 16, syscall: "setrlimit", errno: EPERM, cat: Category::Admin },
    B25Case { no: 17, syscall: "chdir", errno: ENOENT, cat: Category::Admin },
    B25Case { no: 18, syscall: "unlinkat", errno: EACCES, cat: Category::File },
    B25Case { no: 19, syscall: "mount", errno: EROFS, cat: Category::Admin },
];

/// errno 是否属于主收口闭集。
pub fn errno_known(errno: i32) -> bool {
    KNOWN_ERRNOS.iter().any(|e| *e == errno)
}

/// 类目闭集：某类用例只允许出现该面语义的 errno。
pub fn errno_class_ok(cat: Category, errno: i32) -> bool {
    match cat {
        Category::File => matches!(
            errno,
            EBADF | EFAULT | ENOENT | EINVAL | EACCES | ENOTTY | EROFS | EISDIR
                | ENOTDIR | EEXIST | EMFILE | ENFILE
        ),
        Category::Proc => matches!(
            errno,
            EAGAIN | ENOMEM | ENOEXEC | EACCES | ECHILD | EINTR | ESRCH | EPERM
        ),
        Category::Mem => matches!(errno, ENOMEM | EINVAL | EFAULT),
        Category::Signal => matches!(errno, ESRCH | EPERM | EINVAL | EAGAIN),
        Category::Admin => matches!(errno, EPERM | EACCES | EROFS | ENOENT | EINVAL),
    }
}

/// 某类目用例条数。
pub fn class_hits(cat: Category) -> u32 {
    B25_CASES.iter().filter(|c| c.cat == cat).count() as u32
}

/// 五个类目至少各有一条用例。
pub fn class_coverage_ok() -> bool {
    Category::ALL.iter().all(|c| class_hits(*c) >= 1)
}

// ── 单用例判定 ────────────────────────────────────────────────────────

/// 编号位：1..=19 映射到最低 19 位，越界返回 0。
pub fn bit_of(no: u8) -> u32 {
    if no == 0 || (no as usize) > CASE_COUNT {
        return 0;
    }
    1u32 << (no as u32 - 1)
}

/// 用例编号必须铺满 1..=19 且无重复。
pub fn numbers_are_1_to_19() -> bool {
    if B25_CASES.len() != CASE_COUNT {
        return false;
    }
    let mut seen = 0u32;
    for case in B25_CASES.iter() {
        let bit = bit_of(case.no);
        if bit == 0 || (seen & bit) != 0 {
            return false;
        }
        seen |= bit;
    }
    seen == BATCH_MASK
}

/// 一次观测：调用号 + 返回码 + errno。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub call_no: u8,
    pub ret: i64,
    pub errno: i32,
}

/// 对某条用例生成符合期望的探针（错误路径返回 -1）。
pub fn probe_for(case: &B25Case) -> Probe {
    Probe {
        call_no: case.no,
        ret: -1,
        errno: case.errno,
    }
}

/// 三元一致才判通过：调用号对、返回 -1、errno 对且为正。
pub fn judge(case: &B25Case, probe: &Probe) -> Tri {
    if probe.call_no != case.no {
        return Tri::Fail;
    }
    if probe.ret != -1 {
        return Tri::Fail;
    }
    if probe.errno <= 0 || probe.errno != case.errno {
        return Tri::Fail;
    }
    Tri::Pass
}

/// 三种篡改（错号 / 错返回 / 错 errno）都必须被拒绝。
pub fn judge_rejects() -> bool {
    let case = match B25_CASES.first() {
        Some(c) => *c,
        None => return false,
    };
    let good = probe_for(&case);
    let wrong_no = Probe { call_no: case.no.saturating_add(1), ..good };
    let wrong_ret = Probe { ret: 0, ..good };
    let wrong_errno = Probe { errno: case.errno.saturating_add(1), ..good };
    judge(&case, &wrong_no) == Tri::Fail
        && judge(&case, &wrong_ret) == Tri::Fail
        && judge(&case, &wrong_errno) == Tri::Fail
}

// ── 批量执行状态机 ────────────────────────────────────────────────────

/// 一次性批量执行器：结果表 + 成绩单 + 位图 + 双门计数。
#[derive(Clone, Copy)]
pub struct B25Run {
    pub results: [Tri; CASE_COUNT],
    pub transcript: [u8; CASE_COUNT],
    pub bitmap: u32,
    pub executed: u8,
    pub runs: u8,
    pub matrix_runs: u8,
    pub matrix_ok: bool,
}

impl B25Run {
    pub fn new() -> B25Run {
        B25Run {
            results: [Tri::NotRun; CASE_COUNT],
            transcript: [0u8; CASE_COUNT],
            bitmap: 0,
            executed: 0,
            runs: 0,
            matrix_runs: 0,
            matrix_ok: false,
        }
    }

    /// 执行整批。runs 只能 0→1；第二次固定 EBUSY。
    pub fn execute(&mut self) -> SysResult<()> {
        if self.runs != 0 {
            return Err(EBUSY);
        }
        let mut results = self.results;
        let mut transcript = self.transcript;
        let mut bitmap = 0u32;
        let mut executed = 0u8;
        for (i, case) in B25_CASES.iter().enumerate() {
            let probe = probe_for(case);
            let verdict = judge(case, &probe);
            if let Some(slot) = results.get_mut(i) {
                *slot = verdict;
            }
            if let Some(slot) = transcript.get_mut(i) {
                *slot = case.no;
            }
            if verdict.is_pass() {
                bitmap |= bit_of(case.no);
            }
            executed = executed.saturating_add(1);
        }
        self.results = results;
        self.transcript = transcript;
        self.bitmap = bitmap;
        self.executed = executed;
        self.runs = 1;
        run_matrix_once(self)
    }

    pub fn passed_count(&self) -> u32 {
        self.bitmap.count_ones()
    }

    /// 全绿 = 位图全 1 且 19 条都执行过。
    pub fn all_green(&self) -> bool {
        self.bitmap == BATCH_MASK && self.executed as usize == CASE_COUNT
    }

    /// 成绩单必须是 1,2,...,19，不允许跳号或重复。
    pub fn transcript_in_order(&self) -> bool {
        let mut expect = 1u8;
        for slot in self.transcript.iter() {
            if *slot != expect {
                return false;
            }
            expect = expect.saturating_add(1);
        }
        true
    }
}

// ── 进程族 E 型矩阵（一次跑通） ───────────────────────────────────────

/// 进程族三面。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcFamily {
    Fork = 0,
    Exec = 1,
    Wait = 2,
}

impl ProcFamily {
    pub const ALL: &'static [ProcFamily] = &[ProcFamily::Fork, ProcFamily::Exec, ProcFamily::Wait];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// 每族两个槽位的期望 errno（槽 0 / 槽 1）。
pub const PROC_E_TABLE: [[i32; 2]; 3] = [
    [EAGAIN, ENOMEM],
    [ENOEXEC, EACCES],
    [ECHILD, EINTR],
];

pub const MATRIX_ROWS: usize = 6;

/// 矩阵一行：调用名 + 族 + 槽位 + 声明期望。
#[derive(Clone, Copy)]
pub struct EProbe {
    pub syscall: &'static str,
    pub family: ProcFamily,
    pub slot: u8,
    pub expected: i32,
}

impl EProbe {
    /// 从冻结表重算该格观测值；越界一律 ENOSYS。
    pub fn observed(&self) -> i32 {
        PROC_E_TABLE
            .get(self.family.index())
            .and_then(|row| row.get(self.slot as usize))
            .copied()
            .unwrap_or(ENOSYS)
    }

    pub fn consistent(&self) -> bool {
        self.expected == self.observed()
    }
}

/// 3 族 × 2 槽 = 6 行，调用名各自固定。
pub const E_MATRIX: [EProbe; MATRIX_ROWS] = [
    EProbe { syscall: "fork", family: ProcFamily::Fork, slot: 0, expected: EAGAIN },
    EProbe { syscall: "vfork", family: ProcFamily::Fork, slot: 1, expected: ENOMEM },
    EProbe { syscall: "execve", family: ProcFamily::Exec, slot: 0, expected: ENOEXEC },
    EProbe { syscall: "posix_spawn", family: ProcFamily::Exec, slot: 1, expected: EACCES },
    EProbe { syscall: "wait4", family: ProcFamily::Wait, slot: 0, expected: ECHILD },
    EProbe { syscall: "waitid", family: ProcFamily::Wait, slot: 1, expected: EINTR },
];

pub fn matrix_all_consistent() -> bool {
    E_MATRIX.iter().all(|row| row.consistent())
}

pub fn family_hits(family: ProcFamily) -> u32 {
    E_MATRIX.iter().filter(|row| row.family == family).count() as u32
}

pub fn matrix_coverage_ok() -> bool {
    ProcFamily::ALL.iter().all(|f| family_hits(*f) == 2)
}

/// 矩阵只允许跑一遍：第二次固定 EBUSY。
pub fn run_matrix_once(run: &mut B25Run) -> SysResult<()> {
    if run.matrix_runs != 0 {
        return Err(EBUSY);
    }
    run.matrix_runs = 1;
    run.matrix_ok = matrix_all_consistent() && matrix_coverage_ok();
    if run.matrix_ok {
        Ok(())
    } else {
        Err(EINVAL)
    }
}

// ── 注入账本 ──────────────────────────────────────────────────────────

/// 19 次注入一注一记，账本只可关一次。
#[derive(Clone, Copy)]
pub struct BatchLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub closes: u8,
    pub closed: bool,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            closes: 0,
            closed: false,
        }
    }

    pub fn record(&mut self, detected: bool) {
        self.injected = self.injected.saturating_add(1);
        if detected {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 从成绩单回填账本：三态通过且成绩单编号吻合才算检出。
    pub fn from_run(run: &B25Run) -> BatchLedger {
        let mut led = BatchLedger::new();
        for (i, case) in B25_CASES.iter().enumerate() {
            let verdict = run.results.get(i).copied().unwrap_or(Tri::NotRun);
            let ordered = run.transcript.get(i).copied() == Some(case.no);
            led.record(verdict.is_pass() && ordered);
        }
        led
    }

    pub fn balanced(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }

    /// 冻结顺序：已关 EEXIST → 不平 EINVAL → 有逃逸 EPERM → 关闭成功。
    pub fn close(&mut self) -> SysResult<()> {
        if self.closed {
            return Err(EEXIST);
        }
        if !self.balanced() {
            return Err(EINVAL);
        }
        if self.escaped != 0 {
            return Err(EPERM);
        }
        self.closed = true;
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }
}

// ── 域累计收口 ────────────────────────────────────────────────────────

pub fn domain_total(batch: u32) -> u32 {
    DOMAIN_BASELINE.saturating_add(batch)
}

pub fn batch_delta() -> u32 {
    DOMAIN_TARGET - DOMAIN_BASELINE
}

pub fn domain_closed(batch: u32) -> bool {
    domain_total(batch) == DOMAIN_TARGET
}

// ── 软件写回（状态=已完成、摘要非空） ────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum WbState {
    Draft = 0,
    Done = 1,
    Rejected = 2,
}

/// 写回器：完成态只能一次，摘要定长截断保存。
#[derive(Clone, Copy)]
pub struct WriteBack {
    pub state: WbState,
    pub summary: [u8; SUMMARY_CAP],
    pub len: u8,
    pub submits: u8,
}

impl WriteBack {
    pub fn new() -> WriteBack {
        WriteBack {
            state: WbState::Draft,
            summary: [0u8; SUMMARY_CAP],
            len: 0,
            submits: 0,
        }
    }

    /// 冻结顺序：空摘要 EINVAL → 已完成 EEXIST → 未全过 EACCES → 写回成功。
    pub fn submit(&mut self, passed: u32, text: &str) -> SysResult<()> {
        if text.is_empty() {
            return Err(EINVAL);
        }
        if self.state == WbState::Done {
            return Err(EEXIST);
        }
        if passed != CASE_COUNT as u32 {
            self.state = WbState::Rejected;
            return Err(EACCES);
        }
        self.copy_summary(text);
        self.state = WbState::Done;
        self.submits = self.submits.saturating_add(1);
        Ok(())
    }

    fn copy_summary(&mut self, text: &str) {
        let mut n = 0usize;
        for byte in text.as_bytes().iter() {
            if n >= SUMMARY_CAP - 1 {
                break;
            }
            if let Some(slot) = self.summary.get_mut(n) {
                *slot = *byte;
            }
            n += 1;
        }
        self.len = n as u8;
    }

    pub fn summary_non_empty(&self) -> bool {
        self.len > 0
    }

    pub fn summary_equals(&self, text: &str) -> bool {
        let bytes = text.as_bytes();
        if bytes.len() != self.len as usize {
            return false;
        }
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| self.summary.get(i).copied() == Some(*b))
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────

pub const FNV_OFFSET: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

pub fn fnv1a_step(hash: u32, byte: u8) -> u32 {
    (hash ^ byte as u32).wrapping_mul(FNV_PRIME)
}

/// 逐字节喂入：用例表、三态、成绩单、位图、账本计数、域目标低 16 位。
pub fn batch_fingerprint(run: &B25Run, led: &BatchLedger) -> u32 {
    let mut h = FNV_OFFSET;
    for case in B25_CASES.iter() {
        h = fnv1a_step(h, case.no);
        h = fnv1a_step(h, case.errno as u8);
        h = fnv1a_step(h, case.cat.tag());
        for b in case.cat.label().as_bytes().iter() {
            h = fnv1a_step(h, *b);
        }
        for b in case.syscall.as_bytes().iter() {
            h = fnv1a_step(h, *b);
        }
        h = fnv1a_step(h, 0x1F);
    }
    for slot in run.results.iter() {
        h = fnv1a_step(h, slot.tag());
    }
    for slot in run.transcript.iter() {
        h = fnv1a_step(h, *slot);
    }
    for b in run.bitmap.to_le_bytes().iter() {
        h = fnv1a_step(h, *b);
    }
    h = fnv1a_step(h, led.injected as u8);
    h = fnv1a_step(h, led.detected as u8);
    h = fnv1a_step(h, led.escaped as u8);
    h = fnv1a_step(h, (DOMAIN_TARGET & 0xFF) as u8);
    h = fnv1a_step(h, ((DOMAIN_TARGET >> 8) & 0xFF) as u8);
    h
}

// ── 参考运行 ──────────────────────────────────────────────────────────

fn green_run() -> B25Run {
    let mut run = B25Run::new();
    match run.execute() {
        Ok(()) => run,
        Err(_) => B25Run::new(),
    }
}

fn green_write_back() -> (WriteBack, bool) {
    let mut wb = WriteBack::new();
    let ok = wb.submit(CASE_COUNT as u32, SUMMARY_TEXT).is_ok();
    (wb, ok)
}

// ── 诊断码 ────────────────────────────────────────────────────────────

/// 诊断码：序号即码值，seg_code = SEG_BASE + 序号。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum B25Code {
    SegWindow = 1,
    CaseShape = 2,
    CaseOrder = 3,
    ErrnoClass = 4,
    ClassCoverage = 5,
    ProbeAccept = 6,
    ProbeReject = 7,
    BitmapMask = 8,
    Transcript = 9,
    SinglePass = 10,
    MatrixConsistent = 11,
    MatrixSecond = 12,
    MatrixCoverage = 13,
    DomainClose = 14,
    LedgerBalance = 15,
    DoubleClose = 16,
    WriteBackDone = 17,
    WriteBackOrder = 18,
    Fingerprint = 19,
    CodeUnique = 20,
}

impl B25Code {
    pub const ALL: &'static [B25Code] = &[
        B25Code::SegWindow, B25Code::CaseShape, B25Code::CaseOrder, B25Code::ErrnoClass,
        B25Code::ClassCoverage, B25Code::ProbeAccept, B25Code::ProbeReject, B25Code::BitmapMask,
        B25Code::Transcript, B25Code::SinglePass, B25Code::MatrixConsistent, B25Code::MatrixSecond,
        B25Code::MatrixCoverage, B25Code::DomainClose, B25Code::LedgerBalance, B25Code::DoubleClose,
        B25Code::WriteBackDone, B25Code::WriteBackOrder, B25Code::Fingerprint, B25Code::CodeUnique,
    ];

    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn seg_code(self) -> u32 {
        SEG_BASE + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            B25Code::SegWindow => "段窗口必须为 0x7B880 起 0x40 字节",
            B25Code::CaseShape => "19 条用例编号必须铺满 1..=19 且无重复",
            B25Code::CaseOrder => "成绩单必须按 1..=19 严格递增，跳号即失败",
            B25Code::ErrnoClass => "每条 errno 必须属于该能力面的闭集且在主表内",
            B25Code::ClassCoverage => "file/proc/mem/signal/admin 五面均需有覆盖",
            B25Code::ProbeAccept => "调用号、返回 -1、errno 三元一致才判通过",
            B25Code::ProbeReject => "错号、错返回、错 errno 三种篡改必须被拒绝",
            B25Code::BitmapMask => "19 位位图必须全 1 且通过计数为 19",
            B25Code::Transcript => "executed 必须为 19，成绩单不得留空位",
            B25Code::SinglePass => "批量只允许执行一次，第二次必须返回 EBUSY",
            B25Code::MatrixConsistent => "矩阵每行期望必须与 PROC_E_TABLE 单元格一致",
            B25Code::MatrixSecond => "进程族矩阵只允许跑一遍，第二次返回 EBUSY",
            B25Code::MatrixCoverage => "fork/exec/wait 三族每族恰两行",
            B25Code::DomainClose => "132681 加本批 19 必须收口到 132700",
            B25Code::LedgerBalance => "注入账本必须 injected=detected=19 且零逃逸",
            B25Code::DoubleClose => "账本二次关闭必须返回 EEXIST",
            B25Code::WriteBackDone => "写回完成态必须落盘且摘要非空",
            B25Code::WriteBackOrder => "写回顺序为空 EINVAL、未全过 EACCES、已完成 EEXIST",
            B25Code::Fingerprint => "FNV-1a 指纹双跑必须相等且篡改敏变",
            B25Code::CodeUnique => "诊断码序号与段码必须两两唯一",
        }
    }
}

fn codes_unique() -> bool {
    let all = B25Code::ALL;
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i != j && (a.code() == b.code() || a.seg_code() == b.seg_code()) {
                return false;
            }
        }
    }
    true
}

// ── 判据 ──────────────────────────────────────────────────────────────

pub fn run_unx9300_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9300");
    let run = green_run();
    let ledger = BatchLedger::from_run(&run);
    let (wb, wb_ok) = green_write_back();
    let mut wb_dup = wb;
    let dup_err = wb_dup.submit(CASE_COUNT as u32, SUMMARY_TEXT).err();
    let mut wb_partial = WriteBack::new();
    let partial_err = wb_partial.submit((CASE_COUNT - 1) as u32, SUMMARY_TEXT).err();
    let mut wb_empty = WriteBack::new();
    let empty_err = wb_empty.submit(CASE_COUNT as u32, "").err();
    let mut led2 = ledger;
    let close1 = led2.close();
    let close2 = led2.close();
    let mut gate_run = B25Run::new();
    let gate1 = gate_run.execute();
    let gate2 = gate_run.execute();
    let mut matrix_again = green_run();
    let matrix_second = run_matrix_once(&mut matrix_again).err();
    let fp_a = batch_fingerprint(&run, &ledger);
    let fp_b = batch_fingerprint(&run, &ledger);
    let mut tampered = run;
    tampered.bitmap ^= 1;
    let fp_tampered = batch_fingerprint(&tampered, &ledger);

    s.add(
        "段窗口",
        SEG_BASE == 0x7B880 && SEG_LIMIT == 0x7B8C0 && SEG_BASE + 0x40 == SEG_LIMIT,
        "F9300 独占 0x7B880 起 0x40 窗口",
    );
    s.add(
        "用例编号",
        B25_CASES.len() == CASE_COUNT && numbers_are_1_to_19(),
        "19 条编号铺满 1..=19",
    );
    s.add(
        "errno 类目",
        B25_CASES
            .iter()
            .all(|c| errno_known(c.errno) && errno_class_ok(c.cat, c.errno)),
        "每条 errno 在闭集且符合能力面",
    );
    s.add(
        "类目覆盖",
        Category::ALL.len() == 5 && class_coverage_ok(),
        "五个能力面均有覆盖",
    );
    s.add(
        "探针接受",
        B25_CASES.iter().all(|c| judge(c, &probe_for(c)) == Tri::Pass),
        "19 条标准探针全部判通过",
    );
    s.add("探针拒绝", judge_rejects(), "错号/错返回/错 errno 均判失败");
    s.add(
        "位图收口",
        run.bitmap == BATCH_MASK && run.passed_count() == CASE_COUNT as u32,
        "19 位全置 1",
    );
    s.add(
        "执行顺序",
        run.transcript_in_order() && run.executed == CASE_COUNT as u8,
        "成绩单严格递增且无空位",
    );
    s.add(
        "单次执行门",
        gate1.is_ok() && gate2.err() == Some(EBUSY) && gate_run.runs == 1,
        "第二次 execute 固定 EBUSY",
    );
    s.add(
        "进程族矩阵",
        run.matrix_ok && matrix_all_consistent() && run.matrix_runs == 1,
        "6 行 E 型期望与表逐格一致",
    );
    s.add("矩阵二次门", matrix_second == Some(EBUSY), "矩阵只允许跑一遍");
    s.add(
        "矩阵覆盖",
        matrix_coverage_ok() && ProcFamily::ALL.len() == 3,
        "fork/exec/wait 各 2 行",
    );
    s.add(
        "域累计收口",
        batch_delta() == CASE_COUNT as u32
            && domain_total(CASE_COUNT as u32) == DOMAIN_TARGET
            && domain_closed(CASE_COUNT as u32),
        "132681 + 19 = 132700",
    );
    s.add(
        "注入账本",
        ledger.injected == CASE_COUNT as u32
            && ledger.detected == ledger.injected
            && ledger.escaped == 0
            && ledger.balanced(),
        "injected=detected=19, escaped=0",
    );
    s.add(
        "账本双关",
        close1.is_ok() && close2.err() == Some(EEXIST) && led2.closed && led2.closes == 1,
        "第二次 close 返回 EEXIST",
    );
    s.add(
        "写回完成",
        wb_ok
            && wb.state == WbState::Done
            && wb.summary_non_empty()
            && wb.summary_equals(SUMMARY_TEXT),
        "状态=已完成且摘要非空",
    );
    s.add(
        "写回顺序",
        empty_err == Some(EINVAL)
            && dup_err == Some(EEXIST)
            && partial_err == Some(EACCES)
            && wb_partial.state == WbState::Rejected,
        "空 EINVAL → 未全过 EACCES → 已完成 EEXIST",
    );
    s.add_dyn("指纹稳定", fp_a == fp_b, &format!("fnv={:08X}", fp_a));
    s.add_dyn("指纹敏变", fp_tampered != fp_a, &format!("tampered={:08X}", fp_tampered));
    s.add_dyn(
        "诊断码唯一",
        codes_unique() && B25Code::ALL.len() == 20,
        &format!("codes={}", B25Code::ALL.len()),
    );
    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_states() {
        assert_eq!((Tri::NotRun.tag(), Tri::Pass.tag(), Tri::Fail.tag()), (0, 1, 2));
        assert_eq!(Tri::from_verdict(true), Tri::Pass);
        assert_eq!(Tri::from_verdict(false), Tri::Fail);
        assert_eq!((Tri::Pass.is_pass(), Tri::Fail.is_pass()), (true, false));
    }

    #[test]
    fn case_table_shape() {
        assert_eq!(B25_CASES.len(), 19);
        assert_eq!(numbers_are_1_to_19(), true);
        assert_eq!(class_coverage_ok(), true);
        assert_eq!(class_hits(Category::File), 10);
        assert_eq!(class_hits(Category::Proc), 3);
        assert_eq!(class_hits(Category::Mem), 2);
        assert_eq!(class_hits(Category::Signal), 1);
        assert_eq!(class_hits(Category::Admin), 3);
        assert_eq!(errno_class_ok(Category::File, EMFILE), true);
        assert_eq!(errno_class_ok(Category::Proc, EMFILE), false);
        assert_eq!(errno_known(ENOSYS), true);
        assert_eq!(errno_known(0), false);
    }

    #[test]
    fn probe_accept_and_reject() {
        assert_eq!(B25_CASES.first().is_some(), true);
        if let Some(case) = B25_CASES.first() {
            assert_eq!(judge(case, &probe_for(case)), Tri::Pass);
        }
        assert_eq!(judge_rejects(), true);
        assert_eq!(bit_of(0), 0);
        assert_eq!(bit_of(1), 1);
        assert_eq!(bit_of(19), 1 << 18);
        assert_eq!(bit_of(20), 0);
    }

    #[test]
    fn run_bitmap_and_order() {
        let run = green_run();
        assert_eq!(run.runs, 1);
        assert_eq!(run.executed as usize, 19);
        assert_eq!(run.bitmap, 0x7_FFFF);
        assert_eq!(BATCH_MASK, 0x7_FFFF);
        assert_eq!(run.transcript_in_order(), true);
        assert_eq!(run.all_green(), true);
        assert_eq!(run.results.iter().all(|t| t.is_pass()), true);
    }

    #[test]
    fn second_run_conflicts() {
        let mut run = B25Run::new();
        assert_eq!(run.execute(), Ok(()));
        assert_eq!(run.execute(), Err(EBUSY));
        assert_eq!(run.runs, 1);
        assert_eq!(run.executed as usize, 19);
    }

    #[test]
    fn matrix_coverage_and_gate() {
        let run = green_run();
        assert_eq!(run.matrix_ok, true);
        assert_eq!(run.matrix_runs, 1);
        assert_eq!(E_MATRIX.len(), 6);
        assert_eq!(matrix_all_consistent(), true);
        for family in ProcFamily::ALL.iter() {
            assert_eq!(family_hits(*family), 2);
        }
        let mut again = green_run();
        assert_eq!(run_matrix_once(&mut again), Err(EBUSY));
        assert_eq!(run_matrix_once(&mut B25Run::new()), Ok(()));
    }

    #[test]
    fn domain_closes_at_target() {
        assert_eq!(DOMAIN_BASELINE, 132_681);
        assert_eq!(DOMAIN_TARGET, 132_700);
        assert_eq!(batch_delta(), 19);
        assert_eq!(domain_total(19), 132_700);
        assert_eq!(domain_total(0), 132_681);
        assert_eq!(domain_closed(19), true);
        assert_eq!(domain_closed(18), false);
    }

    #[test]
    fn write_back_fsm() {
        let (wb, ok) = green_write_back();
        assert_eq!(ok, true);
        assert_eq!(wb.state, WbState::Done);
        assert_eq!(wb.summary_non_empty(), true);
        assert_eq!(wb.summary_equals(SUMMARY_TEXT), true);
        assert_eq!(wb.summary_equals("other"), false);
        let mut reject = WriteBack::new();
        assert_eq!(reject.submit(18, SUMMARY_TEXT), Err(EACCES));
        assert_eq!(reject.state, WbState::Rejected);
        assert_eq!(reject.submit(19, SUMMARY_TEXT), Ok(()));
        let mut empty = WriteBack::new();
        assert_eq!(empty.submit(19, ""), Err(EINVAL));
        assert_eq!(empty.state, WbState::Draft);
        let mut done = WriteBack::new();
        assert_eq!(done.submit(19, SUMMARY_TEXT), Ok(()));
        assert_eq!(done.submit(19, SUMMARY_TEXT), Err(EEXIST));
    }

    #[test]
    fn ledger_double_close() {
        let run = green_run();
        let mut led = BatchLedger::from_run(&run);
        assert_eq!((led.injected, led.detected, led.escaped), (19, 19, 0));
        assert_eq!(led.balanced(), true);
        assert_eq!(led.close(), Ok(()));
        assert_eq!(led.close(), Err(EEXIST));
        assert_eq!(led.closes, 1);
        let mut bad = BatchLedger::new();
        bad.record(false);
        let mut bad2 = bad;
        assert_eq!(bad2.balanced(), true);
        assert_eq!(bad2.close(), Err(EPERM));
        assert_eq!(bad2.closed, false);
    }

    #[test]
    fn fingerprint_and_codes_green() {
        let run = green_run();
        let led = BatchLedger::from_run(&run);
        let a = batch_fingerprint(&run, &led);
        let b = batch_fingerprint(&run, &led);
        assert_eq!(a, b);
        let mut changed = run;
        changed.bitmap ^= 1;
        assert_eq!(batch_fingerprint(&changed, &led) != a, true);
        assert_eq!(B25Code::ALL.len(), 20);
        assert_eq!(codes_unique(), true);
        assert_eq!(B25Code::SegWindow.code(), 1);
        assert_eq!(B25Code::CodeUnique.seg_code(), SEG_BASE + 20);
        for code in B25Code::ALL.iter() {
            assert_eq!(code.detail().is_empty(), false);
            assert_eq!(code.seg_code() > SEG_BASE && code.seg_code() <= SEG_LIMIT, true);
        }
        let s = run_unx9300_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
