//! UNX-F9223 pipe 系统调用错误矩阵（EFAULT/EMFILE/ENFILE） · 0x7A540段
//!
//! 判据原文（逐字摘录）：
//! ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9223-J1 三错误码注入矩阵判据过，
//! 坏 pipefd 指针全 EFAULT 格过，逐码与 Linux 一致；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. pipe2 的判定序按 Linux `__do_pipe_flags` 复刻：flags → 系统文件表（ENFILE）
//!    → 进程 fd 表（EMFILE）→ 用户缓冲（EFAULT）。系统表先于进程表，别按直觉写反。
//! 2. EFAULT 发生在资源已占之后，必须先回滚两只 fd 与两个 file 配额；
//!    回滚后同一对最低空槽必须能被下一次 pipe2 重新拿走。
//! 3. 两只 fd 是一对：读端在前写端在后，取自最低两个空槽，且必须互不相同。
//! 4. 坏 pipefd 指针不挑时机：哪怕 fd 表已将满，只要资源分配成功就先撞 EFAULT；
//!    只有分配失败时才轮到 EMFILE/ENFILE 抢先。
//! 5. 三类违约（坏指针/进程表满/系统表满）各注入 10 轮，检出 + 漏检守恒。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（派工表给定，0x7A540 独占 0x40 格）。
pub const SEG_BASE: u32 = 0x7A540;
/// 进程 fd 表容量。
pub const FD_CAP: usize = 16;
/// 系统文件表容量（以 file 对象计，pipe 每次吃 2 个）。
pub const FILE_CAP: u32 = 20;
/// 每类违约注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 3;
/// 参数非法。
pub const EINVAL: i32 = 22;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 进程 fd 表耗尽。
pub const EMFILE: i32 = 24;
/// 系统文件表耗尽。
pub const ENFILE: i32 = 23;

/// 诊断码闭集：pipe2 错误面被写错时的十三种形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeCode {
    FlagsAccepted,
    FaultSwallowed,
    FaultNoRollback,
    EmfileMisread,
    EnfileMisread,
    OrderViolation,
    FdLeak,
    FileLeak,
    PairMismatch,
    SlotNotReused,
    CountMismatch,
    NotReproducible,
    SegmentViolation,
}

impl PipeCode {
    /// 诊断码值：本段基址 + 变体序号。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            PipeCode::FlagsAccepted => "坏 flags 被放行：只认 O_CLOEXEC/O_NONBLOCK，其余 EINVAL",
            PipeCode::FaultSwallowed => "EFAULT 被吞：坏 pipefd 指针必须整调用失败，不得回半成品",
            PipeCode::FaultNoRollback => "EFAULT 未回滚：两只 fd 与两个 file 配额都要原样退回",
            PipeCode::EmfileMisread => "EMFILE 判错：进程 fd 表不足两只时必须回 EMFILE",
            PipeCode::EnfileMisread => "ENFILE 判错：系统文件表不足两个对象时必须回 ENFILE",
            PipeCode::OrderViolation => "判定次序倒置：flags → 系统表 → 进程表 → 用户缓冲，四关不许换位",
            PipeCode::FdLeak => "fd 泄漏：失败的 pipe2 不许留下占用的槽",
            PipeCode::FileLeak => "file 泄漏：失败的 pipe2 必须把 file 配额退回",
            PipeCode::PairMismatch => "fd 对错位：读端在前写端在后，且两号不得相同",
            PipeCode::SlotNotReused => "槽未复用：回滚掉的空槽必须能被下一次 pipe2 选中",
            PipeCode::CountMismatch => "账实不符：成功加二、失败归零，逐笔核对",
            PipeCode::NotReproducible => "双跑不一致：同脚本两次指纹必须相等",
            PipeCode::SegmentViolation => "段位越界：SEG_BASE 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }
    /// 全码表。
    pub const ALL: [PipeCode; 13] = [
        PipeCode::FlagsAccepted, PipeCode::FaultSwallowed, PipeCode::FaultNoRollback,
        PipeCode::EmfileMisread, PipeCode::EnfileMisread, PipeCode::OrderViolation,
        PipeCode::FdLeak, PipeCode::FileLeak, PipeCode::PairMismatch,
        PipeCode::SlotNotReused, PipeCode::CountMismatch, PipeCode::NotReproducible,
        PipeCode::SegmentViolation,
    ];
}

/// pipe2 的 flags 取值域：只两个合法位，其余一律非法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeFlags {
    None,
    Cloexec,
    Nonblock,
    Both,
    Unknown,
}

impl PipeFlags {
    /// 是否只含合法位。
    pub const fn valid(self) -> bool { !matches!(self, PipeFlags::Unknown) }
    /// 是否带 O_CLOEXEC。
    pub const fn cloexec(self) -> bool {
        matches!(self, PipeFlags::Cloexec | PipeFlags::Both)
    }
    /// 是否带 O_NONBLOCK。
    pub const fn nonblock(self) -> bool {
        matches!(self, PipeFlags::Nonblock | PipeFlags::Both)
    }
}

/// 调用失败原因闭集；None 仅供成功路径占位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeErr {
    None,
    Einval,
    Fault,
    Emfile,
    Enfile,
}

impl PipeErr {
    /// 映射 errno：成功为 0。
    pub const fn errno(self) -> i32 {
        match self {
            PipeErr::None => 0,
            PipeErr::Einval => EINVAL,
            PipeErr::Fault => EFAULT,
            PipeErr::Emfile => EMFILE,
            PipeErr::Enfile => ENFILE,
        }
    }
    /// 原因名。
    pub const fn label(self) -> &'static str {
        match self {
            PipeErr::None => "ok", PipeErr::Einval => "einval",
            PipeErr::Fault => "efault", PipeErr::Emfile => "emfile",
            PipeErr::Enfile => "enfile",
        }
    }
}

/// 用户侧 pipefd 指针：只有「有效/坏」两种，坏指针必落 EFAULT。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UserBuf {
    pub valid: bool,
}

impl UserBuf {
    pub const fn valid() -> UserBuf { UserBuf { valid: true } }
    pub const fn null() -> UserBuf { UserBuf { valid: false } }
}

/// 成功返回的一对 fd：读端在前，写端在后。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipeFd {
    pub rfd: i32,
    pub wfd: i32,
}

impl PipeFd {
    /// 两号必须不同。
    pub const fn distinct(self) -> bool { self.rfd != self.wfd }
    /// 读端必须小于写端（最低两空槽的自然次序）。
    pub const fn ordered(self) -> bool { self.rfd < self.wfd }
}

/// 进程侧状态：fd 位图 + 系统 file 配额 + 四本账。
#[derive(Clone, Copy, Debug)]
pub struct PipeSys {
    fds: [bool; FD_CAP],
    file_cap: u32,
    file_used: u32,
    successes: u32,
    faults: u32,
    rejects: u32,
    enfile: u32,
    emfile: u32,
}

impl PipeSys {
    pub const fn new() -> PipeSys {
        PipeSys {
            fds: [false; FD_CAP],
            file_cap: FILE_CAP,
            file_used: 0,
            successes: 0,
            faults: 0,
            rejects: 0,
            enfile: 0,
            emfile: 0,
        }
    }
    /// 已占 fd 数。
    pub fn fd_used(&self) -> u32 {
        (0..FD_CAP).filter_map(|i| self.fds.get(i)).filter(|open| **open).count() as u32
    }
    /// 已占 file 配额。
    pub const fn file_used(&self) -> u32 { self.file_used }
    /// 系统表容量。
    pub const fn file_cap(&self) -> u32 { self.file_cap }
    /// 成功次数。
    pub const fn successes(&self) -> u32 { self.successes }
    /// EFAULT 次数。
    pub const fn faults(&self) -> u32 { self.faults }
    /// 拒收总数。
    pub const fn rejects(&self) -> u32 { self.rejects }
    /// 预占 n 个最低 fd，模拟进程已有句柄。
    pub fn occupy_fds(&mut self, n: usize) {
        let mut i = 0usize;
        while i < n && i < FD_CAP {
            if let Some(slot) = self.fds.get_mut(i) { *slot = true; }
            i += 1;
        }
    }
    /// 预填系统 file 配额。
    pub fn fill_files(&mut self, n: u32) {
        self.file_used = if n < self.file_cap { n } else { self.file_cap };
    }
    /// 某 fd 是否占用。
    pub fn fd_open(&self, fd: i32) -> bool {
        if fd < 0 { return false; }
        match self.fds.get(fd as usize) { Some(open) => *open, None => false }
    }
    /// 取最低两只空槽并占位；不足两只回 None。
    fn alloc_pair(&mut self) -> Option<(i32, i32)> {
        let mut first = -1i32;
        let mut second = -1i32;
        let mut i = 0usize;
        while i < FD_CAP {
            let open = match self.fds.get(i) { Some(v) => *v, None => true };
            if !open {
                if first < 0 { first = i as i32; }
                else { second = i as i32; break; }
            }
            i += 1;
        }
        if first >= 0 && second >= 0 {
            if let Some(slot) = self.fds.get_mut(first as usize) { *slot = true; }
            if let Some(slot) = self.fds.get_mut(second as usize) { *slot = true; }
            Some((first, second))
        } else {
            None
        }
    }
    /// 退回一只 fd。
    fn release(&mut self, fd: i32) {
        if fd >= 0 {
            if let Some(slot) = self.fds.get_mut(fd as usize) { *slot = false; }
        }
    }
    /// pipe2：flags → 系统表 → 进程表 → 用户缓冲，四关固定。
    pub fn pipe2(&mut self, flags: PipeFlags, buf: UserBuf) -> Result<PipeFd, PipeErr> {
        if !flags.valid() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PipeErr::Einval);
        }
        if self.file_used.saturating_add(2) > self.file_cap {
            self.enfile = self.enfile.saturating_add(1);
            self.rejects = self.rejects.saturating_add(1);
            return Err(PipeErr::Enfile);
        }
        let pair = match self.alloc_pair() {
            Some(p) => p,
            None => {
                self.emfile = self.emfile.saturating_add(1);
                self.rejects = self.rejects.saturating_add(1);
                return Err(PipeErr::Emfile);
            }
        };
        self.file_used = self.file_used.saturating_add(2);
        if !buf.valid {
            self.release(pair.0);
            self.release(pair.1);
            self.file_used = self.file_used.saturating_sub(2);
            self.faults = self.faults.saturating_add(1);
            return Err(PipeErr::Fault);
        }
        self.successes = self.successes.saturating_add(1);
        Ok(PipeFd { rfd: pair.0, wfd: pair.1 })
    }
    /// 双账计数：(successes, faults, rejects)。
    pub fn totals(&self) -> (u32, u32, u32) { (self.successes, self.faults, self.rejects) }
    /// 状态指纹：fd 位图 + file 账 + 四本计数。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(open) = self.fds.get(i) {
                bytes.push(if *open { 1 } else { 0 });
            }
            i += 1;
        }
        bytes.push((self.file_used & 0xFF) as u8);
        bytes.push((self.successes & 0xFF) as u8);
        bytes.push((self.faults & 0xFF) as u8);
        bytes.push((self.rejects & 0xFF) as u8);
        fnv1a(&bytes)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("pipe2：成功 {} / EFAULT {} / ENFILE {} / EMFILE {} / fd 占 {} / file 占 {}/{}",
                self.successes, self.faults, self.enfile, self.emfile,
                self.fd_used(), self.file_used, self.file_cap)
    }
}

/// 结果归一：成功也压成原因闭集，便于逐格比较。
pub fn outcome(result: Result<PipeFd, PipeErr>) -> PipeErr {
    match result {
        Ok(_) => PipeErr::None,
        Err(e) => e,
    }
}

/// errno 归一。
pub fn errno_of(result: Result<PipeFd, PipeErr>) -> i32 {
    match result {
        Ok(_) => 0,
        Err(e) => e.errno(),
    }
}

/// 注入类别：三错误码各一类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeInject {
    BadPointer,
    FdFull,
    FileFull,
}

impl PipeInject {
    /// 该类别应落到的原因。
    pub const fn expected(self) -> PipeErr {
        match self {
            PipeInject::BadPointer => PipeErr::Fault,
            PipeInject::FdFull => PipeErr::Emfile,
            PipeInject::FileFull => PipeErr::Enfile,
        }
    }
    /// 三类全表。
    pub const ALL: [PipeInject; INJECT_CLASSES] = [
        PipeInject::BadPointer, PipeInject::FdFull, PipeInject::FileFull,
    ];
}

/// 注入账簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipeLedger {
    pub fault: u32,
    pub emfile: u32,
    pub enfile: u32,
    pub detects: u32,
    pub misses: u32,
}

impl PipeLedger {
    pub const fn empty() -> PipeLedger {
        PipeLedger { fault: 0, emfile: 0, enfile: 0, detects: 0, misses: 0 }
    }
    /// 记一笔：回码对且副作用干净才算检出。
    pub fn account(&mut self, expected: PipeErr, actual: PipeErr, hit: bool) {
        if hit && actual == expected {
            match actual {
                PipeErr::Fault => self.fault = self.fault.saturating_add(1),
                PipeErr::Emfile => self.emfile = self.emfile.saturating_add(1),
                PipeErr::Enfile => self.enfile = self.enfile.saturating_add(1),
                _ => {}
            }
            self.detects = self.detects.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
        }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detects.saturating_add(self.misses) == INJECT_ROUNDS * INJECT_CLASSES as u32
            && self.fault.saturating_add(self.emfile).saturating_add(self.enfile) == self.detects
    }
    /// 分类计数。
    pub fn per(&self, err: PipeErr) -> u32 {
        match err {
            PipeErr::Fault => self.fault,
            PipeErr::Emfile => self.emfile,
            PipeErr::Enfile => self.enfile,
            _ => 0,
        }
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("EFAULT {} / EMFILE {} / ENFILE {} / 检出 {} / 漏检 {}",
                self.fault, self.emfile, self.enfile, self.detects, self.misses)
    }
}

/// 单轮注入：坏指针还要验回滚，表满类只验回码。
pub fn inject_once(kind: PipeInject, round: u32) -> (PipeErr, bool) {
    let mut sys = PipeSys::new();
    match kind {
        PipeInject::BadPointer => {
            let pre = (round as usize) % 4;
            sys.occupy_fds(pre);
            let before = (sys.fd_used(), sys.file_used());
            let got = outcome(sys.pipe2(PipeFlags::None, UserBuf::null()));
            let clean = sys.fd_used() == before.0 && sys.file_used() == before.1;
            (got, clean)
        }
        PipeInject::FdFull => {
            sys.occupy_fds(FD_CAP - 1);
            (outcome(sys.pipe2(PipeFlags::Cloexec, UserBuf::valid())), true)
        }
        PipeInject::FileFull => {
            sys.fill_files(FILE_CAP);
            (outcome(sys.pipe2(PipeFlags::Nonblock, UserBuf::valid())), true)
        }
    }
}

/// 三类 × 10 轮矩阵。
pub fn run_pipe_matrix() -> (PipeLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = PipeLedger::empty();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < INJECT_CLASSES {
            let kind = match PipeInject::ALL.get(i) { Some(k) => *k, None => PipeInject::BadPointer };
            let (actual, clean) = inject_once(kind, round);
            let hit = clean && actual == kind.expected();
            ledger.account(kind.expected(), actual, hit);
            i += 1;
        }
        round += 1;
    }
    let per = [ledger.fault, ledger.emfile, ledger.enfile];
    (ledger, per)
}

/// 判定次序探针：flags > 系统表 > 进程表。
pub fn judge_order_probe() -> (PipeErr, PipeErr, PipeErr) {
    let mut both = PipeSys::new();
    both.occupy_fds(FD_CAP);
    both.fill_files(FILE_CAP);
    let first = outcome(both.pipe2(PipeFlags::Unknown, UserBuf::valid()));
    let second = outcome(both.pipe2(PipeFlags::None, UserBuf::valid()));
    let mut fd_only = PipeSys::new();
    fd_only.occupy_fds(FD_CAP);
    let third = outcome(fd_only.pipe2(PipeFlags::None, UserBuf::valid()));
    (first, second, third)
}

/// 对账行：调用式 / 期望 errno / 符号 / Windows 码。
#[derive(Clone, Copy, Debug)]
pub struct ReconRow {
    pub call: &'static str,
    pub errno: i32,
    pub name: &'static str,
    pub win: u16,
}

/// 对账表八行：两个成功、一个 flags、一个坏指针、两表各两格。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { call: "pipe2(buf,0)", errno: 0, name: "OK", win: 0 },
    ReconRow { call: "pipe2(buf,CLOEXEC|NONBLOCK)", errno: 0, name: "OK", win: 0 },
    ReconRow { call: "pipe2(buf,0x80)", errno: EINVAL, name: "EINVAL", win: 87 },
    ReconRow { call: "pipe2(null,0)", errno: EFAULT, name: "EFAULT", win: 998 },
    ReconRow { call: "pipe2(buf,0)[fd-full]", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { call: "pipe2(buf,0)[file-full]", errno: ENFILE, name: "ENFILE", win: 4 },
    ReconRow { call: "pipe2(null,0)[fd-full]", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { call: "pipe2(null,0)[file-full]", errno: ENFILE, name: "ENFILE", win: 4 },
];

/// 按行重放。
pub fn replay(case: usize) -> i32 {
    let mut sys = PipeSys::new();
    match case {
        0 => errno_of(sys.pipe2(PipeFlags::None, UserBuf::valid())),
        1 => errno_of(sys.pipe2(PipeFlags::Both, UserBuf::valid())),
        2 => errno_of(sys.pipe2(PipeFlags::Unknown, UserBuf::valid())),
        3 => errno_of(sys.pipe2(PipeFlags::None, UserBuf::null())),
        4 => {
            sys.occupy_fds(FD_CAP);
            errno_of(sys.pipe2(PipeFlags::None, UserBuf::valid()))
        }
        5 => {
            sys.fill_files(FILE_CAP);
            errno_of(sys.pipe2(PipeFlags::None, UserBuf::valid()))
        }
        6 => {
            sys.occupy_fds(FD_CAP);
            errno_of(sys.pipe2(PipeFlags::None, UserBuf::null()))
        }
        7 => {
            sys.fill_files(FILE_CAP);
            errno_of(sys.pipe2(PipeFlags::None, UserBuf::null()))
        }
        _ => -1,
    }
}

/// 逐行重算。
pub fn reconcile() -> (u32, u32) {
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            if replay(i) == row.errno { ok = ok.saturating_add(1); }
        }
        i += 1;
    }
    (ok, RECON_TABLE.len() as u32)
}

/// 失配明细。
pub fn recon_mismatch_text() -> String {
    let mut text = String::new();
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            let got = replay(i);
            if got != row.errno {
                text.push_str(&format!("第{}行 {} 表={} 实={}; ", i + 1, row.call, row.name, got));
            }
        }
        i += 1;
    }
    if text.is_empty() { text.push_str("8 行逐项一致"); }
    text
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(byte) = bytes.get(i) {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    hash
}

/// 巡逻脚本：成功一次、坏指针一次、进程表满一次，指纹定值。
pub fn patrol_fingerprint() -> u32 {
    let mut sys = PipeSys::new();
    let _ = sys.pipe2(PipeFlags::Cloexec, UserBuf::valid());
    let _ = sys.pipe2(PipeFlags::None, UserBuf::null());
    sys.occupy_fds(FD_CAP - 2);
    let _ = sys.pipe2(PipeFlags::Nonblock, UserBuf::valid());
    sys.fingerprint()
}

/// 写回摘要。
pub fn pipe_summary() -> String {
    let (ok, total) = reconcile();
    let (ledger, _) = run_pipe_matrix();
    format!("F9223 pipe2 矩阵：对账 {}/{} / 注入 检{} 漏{} / fd 表 {} / file 表 {}",
            ok, total, ledger.detects, ledger.misses, FD_CAP, FILE_CAP)
}

/// 本卡判据入口。
pub fn run_unx9223_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9223");

    s.add("段位：0x7A540 0x40 对齐且落在 0x7A000..=0x7D1C0 内",
          SEG_BASE == 0x7A540 && SEG_BASE % 0x40 == 0
              && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0,
          "段位独占，越界即整册返工");

    let mut basic = PipeSys::new();
    let first = basic.pipe2(PipeFlags::None, UserBuf::valid());
    s.add("正常流：pipe2 返回最低两空槽 (0,1) 且读端在前",
          first == Ok(PipeFd { rfd: 0, wfd: 1 })
              && basic.fd_used() == 2 && basic.file_used() == 2
              && basic.fd_open(0) && basic.fd_open(1) && basic.successes() == 1,
          "读端 0、写端 1，fd 与 file 两账各加二");

    let after = basic.pipe2(PipeFlags::Both, UserBuf::valid());
    s.add("正常流：第二次 pipe2 顺延到 (2,3)，flags 组合不改变次序",
          after == Ok(PipeFd { rfd: 2, wfd: 3 }) && basic.fd_used() == 4
              && basic.file_used() == 4 && basic.faults() == 0,
          "合法 flags 只影响属性，不影响最低空槽分配");

    let mut flags = PipeSys::new();
    let bad_flag = flags.pipe2(PipeFlags::Unknown, UserBuf::valid());
    s.add("边界：未知 flags → EINVAL 且 fd/file 账一动不动",
          bad_flag == Err(PipeErr::Einval) && flags.fd_used() == 0
              && flags.file_used() == 0 && flags.rejects() == 1,
          "参数关不过就不许碰任何资源，连预留都不行");

    let mut fault = PipeSys::new();
    let before = (fault.fd_used(), fault.file_used());
    let null_call = fault.pipe2(PipeFlags::None, UserBuf::null());
    s.add("坏指针：pipe2(null,0) → EFAULT 且 fd/file 全额回滚",
          null_call == Err(PipeErr::Fault) && fault.fd_used() == before.0
              && fault.file_used() == before.1 && fault.faults() == 1,
          "EFAULT 在资源分配之后，必须把两只 fd 与两个 file 配额退回");

    let mut pre = PipeSys::new();
    pre.occupy_fds(3);
    let pre_null = pre.pipe2(PipeFlags::Nonblock, UserBuf::null());
    s.add("坏指针：预占 3 槽时 EFAULT 只退自己那对，预占不受伤",
          pre_null == Err(PipeErr::Fault) && pre.fd_used() == 3
              && pre.fd_open(0) && pre.fd_open(1) && pre.fd_open(2)
              && !pre.fd_open(3),
          "回滚必须精确到本次分配的两槽，不许清别人的句柄");

    let mut emfile = PipeSys::new();
    emfile.occupy_fds(FD_CAP - 1);
    let one_left = emfile.pipe2(PipeFlags::None, UserBuf::valid());
    s.add("EMFILE：进程表只剩 1 槽 → EMFILE 且 file 账未增",
          one_left == Err(PipeErr::Emfile) && emfile.file_used() == 0
              && emfile.fd_used() == (FD_CAP - 1) as u32,
          "一对 fd 要两只，剩一只也必须整调用失败，不得降级成单端");

    let mut enfile = PipeSys::new();
    enfile.fill_files(FILE_CAP - 1);
    let one_file = enfile.pipe2(PipeFlags::None, UserBuf::valid());
    s.add("ENFILE：系统表只剩 1 个 file 配额 → ENFILE 且 fd 账未增",
          one_file == Err(PipeErr::Enfile) && enfile.fd_used() == 0
              && enfile.file_used() == FILE_CAP - 1,
          "pipe 一次吃两个 file 对象，剩一个也必须回 ENFILE");

    s.add("判定次序：双表皆满 → ENFILE（系统先于进程）",
          {
              let mut both = PipeSys::new();
              both.occupy_fds(FD_CAP);
              both.fill_files(FILE_CAP);
              both.pipe2(PipeFlags::None, UserBuf::valid()) == Err(PipeErr::Enfile)
          },
          "Linux 先建 file 对象再要 fd，系统表关必须排在进程表关之前");

    let (o1, o2, o3) = judge_order_probe();
    s.add("判定次序：flags > 系统表 > 进程表，三关不换位",
          o1 == PipeErr::Einval && o2 == PipeErr::Enfile && o3 == PipeErr::Emfile,
          "三关同时违约时按 flags、ENFILE、EMFILE 顺序回码");

    let mut order4 = PipeSys::new();
    order4.fill_files(FILE_CAP);
    s.add("判定次序：系统表满 + 坏指针 → ENFILE（分配先于取址）",
          order4.pipe2(PipeFlags::None, UserBuf::null()) == Err(PipeErr::Enfile),
          "资源分配都没过，谈不上 copy_to_user，EFAULT 不许抢跑");

    let mut edge = PipeSys::new();
    edge.occupy_fds(FD_CAP - 2);
    let last_pair = edge.pipe2(PipeFlags::None, UserBuf::valid());
    s.add("边界：进程表恰好剩 2 槽 → 成功用满，指针写回后账实相等",
          last_pair == Ok(PipeFd { rfd: (FD_CAP - 2) as i32, wfd: (FD_CAP - 1) as i32 })
              && edge.fd_used() == FD_CAP as u32 && edge.file_used() == 2,
          "剩两槽是最后可成格，少一槽即 EMFILE，边界必须实测");

    let mut reuse = PipeSys::new();
    let _ = reuse.pipe2(PipeFlags::None, UserBuf::null());
    let retry = reuse.pipe2(PipeFlags::None, UserBuf::valid());
    s.add("状态机：EFAULT 回滚后同一对最低空槽可被下次调用拿走",
          retry == Ok(PipeFd { rfd: 0, wfd: 1 }) && reuse.fd_used() == 2
              && reuse.file_used() == 2,
          "回滚不彻底会让槽永久泄漏，重试必须仍拿到 (0,1)");

    let (ledger, per) = run_pipe_matrix();
    s.add("注入必抓：坏指针/进程表满/系统表满三类各 10/10",
          matches!(per.get(0), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(1), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(2), Some(v) if *v == INJECT_ROUNDS),
          "三类违约各 10 轮全检出，坏指针那类还要验回滚干净");

    s.add("守恒：注入 30 次 = 检出 + 漏检，漏检为 0",
          ledger.detects == INJECT_ROUNDS * INJECT_CLASSES as u32
              && ledger.misses == 0 && ledger.conserved(),
          "检出与漏检之和恒等于注入轮数，任何漏检直接判红");

    let mut soak = PipeSys::new();
    let mut ok_count = 0u32;
    let mut call = 0u32;
    while call < 8 {
        if soak.pipe2(PipeFlags::None, UserBuf::valid()).is_ok() { ok_count += 1; }
        call += 1;
    }
    s.add("容量：连续 pipe2 至进程表耗尽，成功 8 次后转 EMFILE 且账不丢",
          ok_count == 8 && soak.fd_used() == FD_CAP as u32
              && soak.file_used() == FD_CAP as u32
              && soak.pipe2(PipeFlags::None, UserBuf::valid()) == Err(PipeErr::Emfile),
          "16 槽装 8 对管道，第 9 次必须回 EMFILE，已开的 8 对不许受损");

    s.add("守恒：成功加二、失败归零（成功 n 次即 fd_used == 2n）",
          soak.successes() == 8 && soak.fd_used() == soak.successes() * 2
              && soak.file_used() == soak.successes() * 2,
          "两本账都按成功次数乘二，逐笔可复算");

    let f1 = patrol_fingerprint();
    let f2 = patrol_fingerprint();
    s.add("双跑一致：同脚本两次指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同输入必须逐字段同指纹");

    let (rok, rtotal) = reconcile();
    s.add("对账：8 行 pipe2 回码与 Linux 对照表一致",
          rok == rtotal && rtotal == 8,
          "EINVAL/EFAULT/EMFILE/ENFILE 与 Windows 87/998/4 对表，逐行可复算");

    let mut text_closed = true;
    let mut d = 0usize;
    while let Some(code) = PipeCode::ALL.get(d) {
        if code.detail().is_empty() { text_closed = false; }
        if code.val() < SEG_BASE || code.val() > SEG_BASE + 0x3F { text_closed = false; }
        d += 1;
    }
    s.add("文案闭集：13 个诊断码均有非空中文建议且落本段", text_closed,
          "只报码不给路等于没报，诊断码必须自带处置建议");

    let summary = pipe_summary();
    s.add("收口：摘要非空且含对账/注入/两表容量三组数",
          !summary.is_empty() && summary.contains("对账")
              && summary.contains("注入") && summary.contains("fd 表"),
          "写回摘要必须一眼看出对账与注入结果");

    let mut dyn_sys = PipeSys::new();
    let dyn_before = dyn_sys.fd_used();
    let dyn_fault = dyn_sys.pipe2(PipeFlags::None, UserBuf::null());
    s.add_dyn("坏指针回滚明细", dyn_fault == Err(PipeErr::Fault),
              &format!("EFAULT 后 fd {}→{}，file 0，槽全空",
                       dyn_before, dyn_sys.fd_used()));
    let dyn_label = match dyn_fault { Err(e) => e.label(), Ok(_) => "ok" };
    s.add_dyn("回码明细", dyn_label == "efault", &format!("pipe2(null) → {}", dyn_label));
    let recon_text = recon_mismatch_text();
    s.add_dyn("对账失配明细", recon_text.starts_with("8 行"), &recon_text);
    s.add_dyn("注入账簿", ledger.conserved() && ledger.misses == 0, &ledger.summary());

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_success_lowest_pair() {
        let mut sys = PipeSys::new();
        assert_eq!(sys.pipe2(PipeFlags::None, UserBuf::valid()), Ok(PipeFd { rfd: 0, wfd: 1 }));
        assert_eq!(sys.pipe2(PipeFlags::Both, UserBuf::valid()), Ok(PipeFd { rfd: 2, wfd: 3 }));
        assert_eq!(sys.fd_used(), 4);
        assert_eq!(sys.file_used(), 4);
        let fd = sys.pipe2(PipeFlags::Cloexec, UserBuf::valid());
        assert_eq!(fd, Ok(PipeFd { rfd: 4, wfd: 5 }));
        match fd {
            Ok(p) => assert!(p.distinct() && p.ordered()),
            Err(_) => assert_eq!(sys.fd_used(), 4),
        }
    }

    #[test]
    fn bad_pointer_rollback() {
        let mut sys = PipeSys::new();
        sys.occupy_fds(3);
        assert_eq!(sys.pipe2(PipeFlags::None, UserBuf::null()), Err(PipeErr::Fault));
        assert_eq!(sys.fd_used(), 3);
        assert_eq!(sys.file_used(), 0);
        assert!(!sys.fd_open(3));
        assert_eq!(sys.faults(), 1);
        assert_eq!(sys.pipe2(PipeFlags::None, UserBuf::valid()), Ok(PipeFd { rfd: 3, wfd: 4 }));
    }

    #[test]
    fn emfile_and_enfile() {
        let mut sys = PipeSys::new();
        sys.occupy_fds(FD_CAP - 1);
        assert_eq!(sys.pipe2(PipeFlags::None, UserBuf::valid()), Err(PipeErr::Emfile));
        assert_eq!(sys.file_used(), 0);

        let mut full = PipeSys::new();
        full.fill_files(FILE_CAP - 1);
        assert_eq!(full.pipe2(PipeFlags::None, UserBuf::valid()), Err(PipeErr::Enfile));
        assert_eq!(full.fd_used(), 0);

        let mut both = PipeSys::new();
        both.occupy_fds(FD_CAP);
        both.fill_files(FILE_CAP);
        assert_eq!(both.pipe2(PipeFlags::None, UserBuf::valid()), Err(PipeErr::Enfile));
    }

    #[test]
    fn order_probe_and_flags() {
        assert_eq!(
            judge_order_probe(),
            (PipeErr::Einval, PipeErr::Enfile, PipeErr::Emfile)
        );
        let mut sys = PipeSys::new();
        assert_eq!(sys.pipe2(PipeFlags::Unknown, UserBuf::null()), Err(PipeErr::Einval));
        assert_eq!(sys.fd_used(), 0);
        assert_eq!(sys.file_used(), 0);
        assert!(PipeFlags::Both.valid() && PipeFlags::Both.cloexec() && PipeFlags::Both.nonblock());
        assert!(!PipeFlags::Unknown.valid());
    }

    #[test]
    fn full_table_edge_and_reuse() {
        let mut edge = PipeSys::new();
        edge.occupy_fds(FD_CAP - 2);
        assert_eq!(
            edge.pipe2(PipeFlags::None, UserBuf::valid()),
            Ok(PipeFd { rfd: (FD_CAP - 2) as i32, wfd: (FD_CAP - 1) as i32 })
        );
        assert_eq!(edge.fd_used(), FD_CAP as u32);

        let mut reuse = PipeSys::new();
        assert_eq!(reuse.pipe2(PipeFlags::None, UserBuf::null()), Err(PipeErr::Fault));
        assert_eq!(reuse.pipe2(PipeFlags::None, UserBuf::valid()), Ok(PipeFd { rfd: 0, wfd: 1 }));
    }

    #[test]
    fn table_exhaustion_soak() {
        let mut sys = PipeSys::new();
        let mut ok = 0u32;
        let mut i = 0u32;
        while i < 8 {
            if sys.pipe2(PipeFlags::None, UserBuf::valid()).is_ok() { ok += 1; }
            i += 1;
        }
        assert_eq!(ok, 8);
        assert_eq!(sys.fd_used(), FD_CAP as u32);
        assert_eq!(sys.pipe2(PipeFlags::None, UserBuf::valid()), Err(PipeErr::Emfile));
        assert_eq!(sys.fd_used(), FD_CAP as u32);
        assert_eq!(sys.file_used(), FD_CAP as u32);
    }

    #[test]
    fn injection_and_conservation() {
        let (ledger, per) = run_pipe_matrix();
        assert_eq!(ledger.detects, INJECT_ROUNDS * INJECT_CLASSES as u32);
        assert_eq!(ledger.misses, 0);
        assert!(ledger.conserved());
        assert_eq!(per, [INJECT_ROUNDS, INJECT_ROUNDS, INJECT_ROUNDS]);
        assert_eq!(ledger.per(PipeErr::Fault), INJECT_ROUNDS);
        assert_eq!(ledger.per(PipeErr::Emfile), INJECT_ROUNDS);
        assert_eq!(ledger.per(PipeErr::Enfile), INJECT_ROUNDS);
    }

    #[test]
    fn determinism_and_recon() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(patrol_fingerprint(), 0);
        let (ok, total) = reconcile();
        assert_eq!(ok, total);
        assert_eq!(total, 8);
        assert!(recon_mismatch_text().starts_with("8 行"));
        assert!(pipe_summary().contains("对账"));
    }

    #[test]
    fn text_and_segment() {
        assert_eq!(PipeCode::ALL.len(), 13);
        let mut i = 0usize;
        while let Some(code) = PipeCode::ALL.get(i) {
            assert!(!code.detail().is_empty());
            assert!(code.val() >= SEG_BASE && code.val() <= SEG_BASE + 0x3F);
            i += 1;
        }
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert!(PipeErr::None.errno() == 0);
    }
}
