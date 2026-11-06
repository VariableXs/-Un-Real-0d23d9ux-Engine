//! UNX-F9225 系统级 ENFILE 矩阵（文件表上限） · 0x7A5C0段
//!
//! 判据原文（逐字摘录）：
//! ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9225-J1 系统级上限注入判据过，
//! 全 ENFILE 格过，与进程级 EMFILE 判序（先进程后系统）格过；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 两级容量：每个进程有自己的 fd 表（EMFILE），所有进程共享系统文件表（ENFILE）；
//!    系统表是真正的稀缺资源，进程表只是本地闸门。
//! 2. 判序按卡面钉死：open 路径先查进程表再查系统表，双满时回 EMFILE；
//!    进程有空但系统满才回 ENFILE。
//! 3. 与 F9223 的差异要写明白：pipe2 先建两个 file 对象再要 fd，顺序是
//!    ENFILE → EMFILE；open 先要 fd 再开文件，顺序是 EMFILE → ENFILE。
//!    两条路径的 hook 都入册，禁止用一句「都一样」糊过去。
//! 4. ENFILE 失败不消费进程 fd 槽：系统表满时反复重试，进程占用数必须纹丝不动。
//! 5. 释放一个系统槽后，原本 ENFILE 的调用必须立刻成功并拿到最低空 fd。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（派工表给定，0x7A5C0 独占 0x40 格）。
pub const SEG_BASE: u32 = 0x7A5C0;
/// 每进程 fd 表容量。
pub const PROC_CAP: usize = 8;
/// 进程数（用来演示系统表被多进程共享）。
pub const PROC_COUNT: usize = 3;
/// 系统文件表容量。
pub const SYS_CAP: u32 = 12;
/// 每类违约注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 3;
/// 进程 fd 表耗尽。
pub const EMFILE: i32 = 24;
/// 系统文件表耗尽。
pub const ENFILE: i32 = 23;
/// 坏 pid / 坏 fd。
pub const EBADF: i32 = 9;

/// 诊断码闭集：ENFILE 面被写错时的十三种形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnfileCode {
    SysCapIgnored,
    PrecedenceFlipped,
    ProcMisjudged,
    LeakOnFailure,
    CommitBeforeCheck,
    GridHole,
    CrossProcLost,
    RecoveryFailed,
    CountMismatch,
    OrderNoteMissing,
    NotReproducible,
    SummaryEmpty,
    SegmentViolation,
}

impl EnfileCode {
    /// 诊断码值。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            EnfileCode::SysCapIgnored => "系统表上限被忽略：全系统 file 对象到顶必须回 ENFILE",
            EnfileCode::PrecedenceFlipped => "判序倒置：open 路径先进程 EMFILE 后系统 ENFILE，别照抄 pipe2",
            EnfileCode::ProcMisjudged => "进程表判错：本进程 fd 用满时即使系统有空也必须 EMFILE",
            EnfileCode::LeakOnFailure => "失败泄漏：ENFILE 不许消费进程 fd 槽，重试占用数不动",
            EnfileCode::CommitBeforeCheck => "先改账后判定：两关都过才允许同时落 fd 与 file 两笔账",
            EnfileCode::GridHole => "矩阵漏格：多进程 × 多调用在系统满时必须逐格 ENFILE",
            EnfileCode::CrossProcLost => "跨进程串账：一个进程耗尽系统表不得清掉别的进程 fd",
            EnfileCode::RecoveryFailed => "恢复失败：释放一个系统槽后原 ENFILE 调用必须立刻成功",
            EnfileCode::CountMismatch => "账实不符：opens − closes == 系统占用 == 各进程占用之和",
            EnfileCode::OrderNoteMissing => "判序注记缺失：open 与 pipe2 两条 hook 必须入册且方向相反",
            EnfileCode::NotReproducible => "双跑不一致：同脚本两次指纹必须相等",
            EnfileCode::SummaryEmpty => "摘要为空：写回必须含对账/注入/判序三组数",
            EnfileCode::SegmentViolation => "段位越界：SEG_BASE 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }
    /// 全码表。
    pub const ALL: [EnfileCode; 13] = [
        EnfileCode::SysCapIgnored, EnfileCode::PrecedenceFlipped, EnfileCode::ProcMisjudged,
        EnfileCode::LeakOnFailure, EnfileCode::CommitBeforeCheck, EnfileCode::GridHole,
        EnfileCode::CrossProcLost, EnfileCode::RecoveryFailed, EnfileCode::CountMismatch,
        EnfileCode::OrderNoteMissing, EnfileCode::NotReproducible, EnfileCode::SummaryEmpty,
        EnfileCode::SegmentViolation,
    ];
}

/// 调用路径：open 家族的三条入口共享同一判序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenKind {
    Open,
    Creat,
    Socket,
}

impl OpenKind {
    /// 路径名。
    pub const fn label(self) -> &'static str {
        match self { OpenKind::Open => "open", OpenKind::Creat => "creat", OpenKind::Socket => "socket" }
    }
    /// 三条入口全表。
    pub const ALL: [OpenKind; 3] = [OpenKind::Open, OpenKind::Creat, OpenKind::Socket];
}

/// 单进程 fd 表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Proc {
    fds: [bool; PROC_CAP],
}

impl Proc {
    pub const fn empty() -> Proc { Proc { fds: [false; PROC_CAP] } }
    /// 占用数。
    pub fn used(&self) -> u32 {
        (0..PROC_CAP).filter_map(|i| self.fds.get(i)).filter(|open| **open).count() as u32
    }
    /// 某 fd 是否占用。
    pub fn has(&self, fd: i32) -> bool {
        if fd < 0 { return false; }
        match self.fds.get(fd as usize) { Some(open) => *open, None => false }
    }
    /// 最低空槽。
    pub fn lowest_free(&self) -> Option<i32> {
        let mut i = 0usize;
        while i < PROC_CAP {
            let taken = match self.fds.get(i) { Some(v) => *v, None => true };
            if !taken { return Some(i as i32); }
            i += 1;
        }
        None
    }
    fn set(&mut self, fd: i32, open: bool) {
        if fd >= 0 {
            if let Some(slot) = self.fds.get_mut(fd as usize) { *slot = open; }
        }
    }
}

/// 系统文件表 + 三张进程表 + 四本账。
#[derive(Clone, Copy, Debug)]
pub struct SysVfs {
    procs: [Proc; PROC_COUNT],
    file_used: u32,
    file_cap: u32,
    opens: u32,
    closes: u32,
    enfile: u32,
    emfile: u32,
}

impl SysVfs {
    pub const fn new() -> SysVfs {
        SysVfs {
            procs: [Proc::empty(); PROC_COUNT],
            file_used: 0,
            file_cap: SYS_CAP,
            opens: 0,
            closes: 0,
            enfile: 0,
            emfile: 0,
        }
    }
    /// 系统占用。
    pub const fn file_used(&self) -> u32 { self.file_used }
    /// 系统容量。
    pub const fn file_cap(&self) -> u32 { self.file_cap }
    /// 某进程视图。
    pub fn proc(&self, pid: usize) -> Option<Proc> { self.procs.get(pid).copied() }
    /// 某进程占用数：坏 pid 回 0。
    pub fn proc_used(&self, pid: usize) -> u32 {
        match self.procs.get(pid) { Some(p) => p.used(), None => 0 }
    }
    /// 全部进程占用之和。
    pub fn procs_used(&self) -> u32 {
        (0..PROC_COUNT).filter_map(|i| self.procs.get(i)).map(|p| p.used()).sum()
    }
    /// 某进程某 fd 是否占用。
    pub fn fd_open(&self, pid: usize, fd: i32) -> bool {
        match self.procs.get(pid) { Some(p) => p.has(fd), None => false }
    }
    /// 累计 ENFILE。
    pub const fn enfile(&self) -> u32 { self.enfile }
    /// 累计 EMFILE。
    pub const fn emfile(&self) -> u32 { self.emfile }
    /// 打开账。
    pub const fn opens(&self) -> u32 { self.opens }
    /// 关闭账。
    pub const fn closes(&self) -> u32 { self.closes }

    /// 打开：pid 校验 → 进程表（EMFILE）→ 系统表（ENFILE）→ 双账同落。
    ///
    /// 与 pipe2 的 ENFILE 先于 EMFILE 相反，这是 open 路径的实测判序。
    pub fn open(&mut self, pid: usize, _kind: OpenKind) -> Result<i32, i32> {
        let used = match self.procs.get(pid) { Some(p) => p.used(), None => return Err(EBADF) };
        if used >= PROC_CAP as u32 {
            self.emfile = self.emfile.saturating_add(1);
            return Err(EMFILE);
        }
        if self.file_used >= self.file_cap {
            self.enfile = self.enfile.saturating_add(1);
            return Err(ENFILE);
        }
        let fd = match self.procs.get(pid) {
            Some(p) => match p.lowest_free() {
                Some(v) => v,
                None => {
                    self.emfile = self.emfile.saturating_add(1);
                    return Err(EMFILE);
                }
            },
            None => return Err(EBADF),
        };
        if let Some(p) = self.procs.get_mut(pid) { p.set(fd, true); }
        self.file_used = self.file_used.saturating_add(1);
        self.opens = self.opens.saturating_add(1);
        Ok(fd)
    }

    /// 关闭：pid/fd 任一坏回 EBADF，成功时两账同退。
    pub fn close(&mut self, pid: usize, fd: i32) -> Result<i32, i32> {
        let has = match self.procs.get(pid) { Some(p) => p.has(fd), None => false };
        if !has { return Err(EBADF); }
        if let Some(p) = self.procs.get_mut(pid) { p.set(fd, false); }
        self.file_used = self.file_used.saturating_sub(1);
        self.closes = self.closes.saturating_add(1);
        Ok(0)
    }

    /// 四本账。
    pub fn totals(&self) -> (u32, u32) { (self.enfile, self.emfile) }
    /// 指纹：三张进程位图 + 系统账 + 计数。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut pid = 0usize;
        while pid < PROC_COUNT {
            if let Some(p) = self.procs.get(pid) {
                let mut fd = 0usize;
                while fd < PROC_CAP {
                    bytes.push(if p.has(fd as i32) { 1 } else { 0 });
                    fd += 1;
                }
            }
            pid += 1;
        }
        bytes.push((self.file_used & 0xFF) as u8);
        bytes.push((self.enfile & 0xFF) as u8);
        bytes.push((self.emfile & 0xFF) as u8);
        fnv1a(&bytes)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        let (enfile, emfile) = self.totals();
        format!("ENFILE：系统 {}/{} / 进程占用 {} / ENFILE {} / EMFILE {} / 判序 进程先于系统",
                self.file_used, self.file_cap, self.procs_used(), enfile, emfile)
    }
}

/// 判序注记：两条路径方向相反，必须都入册。
#[derive(Clone, Copy, Debug)]
pub struct OrderNote {
    pub path: &'static str,
    pub first: &'static str,
    pub second: &'static str,
    pub hook: &'static str,
}

/// 两条注记：open 先 EMFILE 后 ENFILE，pipe2 先 ENFILE 后 EMFILE。
pub const ORDER_NOTES: [OrderNote; 2] = [
    OrderNote { path: "open", first: "EMFILE", second: "ENFILE",
                hook: "do_sys_openat2: get_unused_fd_flags 先于 do_filp_open" },
    OrderNote { path: "pipe2", first: "ENFILE", second: "EMFILE",
                hook: "__do_pipe_flags: create_pipe_files 先于 get_unused_fd_flags" },
];

/// 注记自检：方向相反、hook 非空。
pub fn order_notes_ok() -> bool {
    let open_row = ORDER_NOTES.get(0);
    let pipe_row = ORDER_NOTES.get(1);
    matches!(open_row, Some(n) if n.path == "open" && n.first == "EMFILE"
        && n.second == "ENFILE" && !n.hook.is_empty())
        && matches!(pipe_row, Some(n) if n.path == "pipe2" && n.first == "ENFILE"
            && n.second == "EMFILE" && !n.hook.is_empty())
}

/// 注入类别：系统满 / 双满 / 恢复。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnfileInject {
    SysFull,
    BothFull,
    Recover,
}

impl EnfileInject {
    /// 该类别应回的 errno（恢复类为 0）。
    pub const fn expected(self) -> i32 {
        match self {
            EnfileInject::SysFull => ENFILE,
            EnfileInject::BothFull => EMFILE,
            EnfileInject::Recover => 0,
        }
    }
    /// 三类全表。
    pub const ALL: [EnfileInject; INJECT_CLASSES] = [
        EnfileInject::SysFull, EnfileInject::BothFull, EnfileInject::Recover,
    ];
}

/// 注入账簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnfileLedger {
    pub sys_full: u32,
    pub both_full: u32,
    pub recover: u32,
    pub detects: u32,
    pub misses: u32,
}

impl EnfileLedger {
    pub const fn empty() -> EnfileLedger {
        EnfileLedger { sys_full: 0, both_full: 0, recover: 0, detects: 0, misses: 0 }
    }
    /// 记一笔。
    pub fn account(&mut self, expected: i32, actual: i32, hit: bool) {
        if hit && actual == expected {
            match expected {
                ENFILE => self.sys_full = self.sys_full.saturating_add(1),
                EMFILE => self.both_full = self.both_full.saturating_add(1),
                0 => self.recover = self.recover.saturating_add(1),
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
            && self.sys_full.saturating_add(self.both_full)
                .saturating_add(self.recover) == self.detects
    }
    /// 分类计数。
    pub fn per(&self, errno: i32) -> u32 {
        match errno {
            ENFILE => self.sys_full,
            EMFILE => self.both_full,
            0 => self.recover,
            _ => 0,
        }
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("系统满 {} / 双满 {} / 恢复 {} / 检出 {} / 漏检 {}",
                self.sys_full, self.both_full, self.recover, self.detects, self.misses)
    }
}

/// 把系统表填满：p0 开 8 个、p1 开剩余额度。
pub fn fill_system(vfs: &mut SysVfs) {
    let cap = vfs.file_cap() as usize;
    let mut done = 0usize;
    let mut pid = 0usize;
    while done < cap && pid < PROC_COUNT {
        let mut slot = 0usize;
        while done < cap && slot < PROC_CAP {
            if vfs.open(pid, OpenKind::Open).is_ok() { done += 1; }
            slot += 1;
        }
        pid += 1;
    }
}

/// 单轮注入：每轮换一个目标进程，避免只压一个 pid。
pub fn inject_once(kind: EnfileInject, round: u32) -> (i32, bool) {
    let mut vfs = SysVfs::new();
    match kind {
        EnfileInject::SysFull => {
            fill_system(&mut vfs);
            let pid = 1 + (round as usize % (PROC_COUNT - 1));
            let before = vfs.proc_used(pid);
            match vfs.open(pid, OpenKind::Creat) {
                Err(e) => (e, vfs.proc_used(pid) == before),
                Ok(_) => (0, false),
            }
        }
        EnfileInject::BothFull => {
            fill_system(&mut vfs);
            let target = 0usize;
            let mut i = 0usize;
            while i < PROC_CAP {
                let _ = vfs.open(target, OpenKind::Open);
                i += 1;
            }
            match vfs.open(target, OpenKind::Socket) {
                Err(e) => (e, true),
                Ok(_) => (0, false),
            }
        }
        EnfileInject::Recover => {
            fill_system(&mut vfs);
            let blocked = vfs.open(2, OpenKind::Open) == Err(ENFILE);
            let freed = vfs.close(1, 0).is_ok();
            let worked = vfs.open(2, OpenKind::Open).is_ok();
            if blocked && freed && worked { (0, true) } else { (0, false) }
        }
    }
}

/// 三类 × 10 轮矩阵。
pub fn run_enfile_matrix() -> (EnfileLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = EnfileLedger::empty();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < INJECT_CLASSES {
            let kind = match EnfileInject::ALL.get(i) { Some(k) => *k, None => EnfileInject::SysFull };
            let (actual, hit) = inject_once(kind, round);
            ledger.account(kind.expected(), actual, hit);
            i += 1;
        }
        round += 1;
    }
    let per = [ledger.sys_full, ledger.both_full, ledger.recover];
    (ledger, per)
}

/// 判定次序探针：双满 → EMFILE；仅系统满 → ENFILE；腾槽 → 0。
pub fn judge_order_probe() -> (i32, i32, i32) {
    let mut vfs = SysVfs::new();
    fill_system(&mut vfs);
    let mut i = 0usize;
    while i < PROC_CAP {
        let _ = vfs.open(0, OpenKind::Open);
        i += 1;
    }
    let first = match vfs.open(0, OpenKind::Open) { Err(e) => e, Ok(_) => 0 };
    let second = match vfs.open(2, OpenKind::Open) { Err(e) => e, Ok(_) => 0 };
    let _ = vfs.close(1, 0);
    let third = match vfs.open(2, OpenKind::Open) { Err(e) => e, Ok(_) => 0 };
    (first, second, third)
}

/// 对账行：场景 / 期望 errno / 符号 / Windows 码。
#[derive(Clone, Copy, Debug)]
pub struct ReconRow {
    pub scene: &'static str,
    pub errno: i32,
    pub name: &'static str,
    pub win: u16,
}

/// 对账表八行：正常、两表各自满、双满、两处边界、恢复与坏 pid。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { scene: "open(p2) fresh", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "open(p0) proc-full", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { scene: "open(p2) sys-full", errno: ENFILE, name: "ENFILE", win: 4 },
    ReconRow { scene: "open(p0) both-full", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { scene: "open(p2) sys-last", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "open(p1) proc-last", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "open(p2) after release", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "open(pid=9)", errno: EBADF, name: "EBADF", win: 6 },
];

/// 按行重放。
pub fn replay(case: usize) -> i32 {
    let mut vfs = SysVfs::new();
    match case {
        0 => match vfs.open(2, OpenKind::Open) { Err(e) => e, Ok(_) => 0 },
        1 => {
            let mut i = 0usize;
            while i < PROC_CAP {
                let _ = vfs.open(0, OpenKind::Open);
                i += 1;
            }
            match vfs.open(0, OpenKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        2 => {
            fill_system(&mut vfs);
            match vfs.open(2, OpenKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        3 => {
            fill_system(&mut vfs);
            let mut i = 0usize;
            while i < PROC_CAP {
                let _ = vfs.open(0, OpenKind::Open);
                i += 1;
            }
            match vfs.open(0, OpenKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        4 => {
            let mut left = SYS_CAP - 1;
            let mut pid = 0usize;
            while left > 0 && pid < PROC_COUNT {
                let mut slot = 0usize;
                while left > 0 && slot < PROC_CAP {
                    if vfs.open(pid, OpenKind::Open).is_ok() { left -= 1; }
                    slot += 1;
                }
                pid += 1;
            }
            match vfs.open(2, OpenKind::Socket) { Err(e) => e, Ok(_) => 0 }
        }
        5 => {
            let mut i = 0usize;
            while i < PROC_CAP - 1 {
                let _ = vfs.open(1, OpenKind::Open);
                i += 1;
            }
            match vfs.open(1, OpenKind::Creat) { Err(e) => e, Ok(_) => 0 }
        }
        6 => {
            fill_system(&mut vfs);
            let _ = vfs.open(2, OpenKind::Open);
            let _ = vfs.close(1, 0);
            match vfs.open(2, OpenKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        7 => match vfs.open(PROC_COUNT + 6, OpenKind::Open) { Err(e) => e, Ok(_) => 0 },
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
                text.push_str(&format!("第{}行 {} 表={} 实={}; ", i + 1, row.scene, row.name, got));
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

/// 巡逻脚本：填满 → ENFILE → 释放 → 成功，指纹定值。
pub fn patrol_fingerprint() -> u32 {
    let mut vfs = SysVfs::new();
    fill_system(&mut vfs);
    let _ = vfs.open(2, OpenKind::Open);
    let _ = vfs.close(1, 0);
    let _ = vfs.open(2, OpenKind::Creat);
    vfs.fingerprint()
}

/// 写回摘要。
pub fn enfile_summary() -> String {
    let (ok, total) = reconcile();
    let (ledger, _) = run_enfile_matrix();
    format!("F9225 ENFILE 矩阵：对账 {}/{} / 注入 检{} 漏{} / 系统表 {} / 判序注记 {} 条",
            ok, total, ledger.detects, ledger.misses, SYS_CAP, ORDER_NOTES.len())
}

/// 本卡判据入口。
pub fn run_unx9225_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9225");

    s.add("段位：0x7A5C0 0x40 对齐且落在 0x7A000..=0x7D1C0 内",
          SEG_BASE == 0x7A5C0 && SEG_BASE % 0x40 == 0
              && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0,
          "段位独占，越界即整册返工");

    let mut basic = SysVfs::new();
    let a0 = basic.open(0, OpenKind::Open);
    let a1 = basic.open(0, OpenKind::Creat);
    let a2 = basic.open(1, OpenKind::Open);
    s.add("正常流：两进程各拿自己最低空 fd，系统账按实际打开数计",
          a0 == Ok(0) && a1 == Ok(1) && a2 == Ok(0)
              && basic.proc_used(0) == 2 && basic.proc_used(1) == 1
              && basic.file_used() == 3 && basic.opens() == 3,
          "进程各管各的编号，系统表统一记 3 笔");

    let mut sys_full = SysVfs::new();
    fill_system(&mut sys_full);
    let before = sys_full.proc_used(2);
    let blocked = sys_full.open(2, OpenKind::Open);
    s.add("系统表上限：填满 12 个 file 对象 → 新调用 ENFILE 且不动进程槽",
          sys_full.file_used() == SYS_CAP && blocked == Err(ENFILE)
              && sys_full.proc_used(2) == before && sys_full.enfile() == 1,
          "ENFILE 是系统级失败，不许让目标进程白占一个 fd 槽");

    let mut proc_full = SysVfs::new();
    let mut i = 0usize;
    while i < PROC_CAP {
        let _ = proc_full.open(1, OpenKind::Open);
        i += 1;
    }
    s.add("进程表上限：本进程 8 槽用满 → EMFILE，此时系统表仍有空位",
          proc_full.proc_used(1) == PROC_CAP as u32
              && proc_full.open(1, OpenKind::Open) == Err(EMFILE)
              && proc_full.file_used() < SYS_CAP && proc_full.emfile() == 1,
          "进程闸门先关，系统再有空也不放行本进程");

    let mut both = SysVfs::new();
    fill_system(&mut both);
    s.add("判序：双表皆满 → EMFILE（先进程后系统）",
          both.open(0, OpenKind::Open) == Err(EMFILE)
              && both.emfile() == 1 && both.enfile() == 0,
          "open 路径先要 fd 再开文件，进程关永远排在系统关之前");

    let (o1, o2, o3) = judge_order_probe();
    s.add("判序三探针：双满 EMFILE / 仅系统满 ENFILE / 腾槽后成功",
          o1 == EMFILE && o2 == ENFILE && o3 == 0,
          "三板斧逐格验：先堵双满、再单满、最后释放恢复");

    let mut grid = SysVfs::new();
    fill_system(&mut grid);
    let mut holes = 0u32;
    let mut pid = 1usize;
    while pid < PROC_COUNT {
        let mut kind_i = 0usize;
        while kind_i < OpenKind::ALL.len() {
            if let Some(kind) = OpenKind::ALL.get(kind_i) {
                if grid.open(pid, *kind) == Err(ENFILE) { holes += 1; }
            }
            kind_i += 1;
        }
        pid += 1;
    }
    s.add("全 ENFILE 格：两个有余量进程 × 三条入口共 6 格全部 ENFILE",
          holes == 6 && grid.enfile() == 6 && grid.procs_used() == SYS_CAP,
          "系统满时多进程多入口逐格回 ENFILE，一格不漏");

    let mut recover = SysVfs::new();
    fill_system(&mut recover);
    let blocked = recover.open(2, OpenKind::Open);
    let freed = recover.close(1, 0);
    let after = recover.open(2, OpenKind::Open);
    s.add("恢复：释放一个系统槽后原 ENFILE 调用立刻成功并拿 0 号",
          blocked == Err(ENFILE) && freed == Ok(0) && after == Ok(0)
              && recover.file_used() == SYS_CAP,
          "释放一个 file 对象就应恢复一次分配，恢复后系统表重新满格");

    let mut cross = SysVfs::new();
    let mut i2 = 0usize;
    while i2 < PROC_CAP {
        let _ = cross.open(0, OpenKind::Open);
        i2 += 1;
    }
    let mut j2 = 0usize;
    while j2 < (SYS_CAP as usize - PROC_CAP) {
        let _ = cross.open(1, OpenKind::Open);
        j2 += 1;
    }
    s.add("跨进程：A 耗尽系统表后 B/C 均 ENFILE，A 的 8 个 fd 一个不少",
          cross.file_used() == SYS_CAP
              && cross.open(2, OpenKind::Open) == Err(ENFILE)
              && cross.proc_used(0) == PROC_CAP as u32 && cross.fd_open(0, 7),
          "系统表是共享资源，耗尽不得回清任何进程的既有句柄");

    let mut edge = SysVfs::new();
    let mut e1 = 0usize;
    while e1 < PROC_CAP - 1 {
        let _ = edge.open(2, OpenKind::Open);
        e1 += 1;
    }
    s.add("边界：进程最后一槽 fd=7 可开，下一格立刻 EMFILE",
          edge.open(2, OpenKind::Creat) == Ok(7)
              && edge.open(2, OpenKind::Open) == Err(EMFILE) && edge.proc_used(2) == 8,
          "第 8 槽是最后可成格，第 9 次必须整调用失败");

    let mut sys_edge = SysVfs::new();
    let mut left = SYS_CAP as usize - 1;
    let mut pid2 = 0usize;
    while left > 0 && pid2 < PROC_COUNT {
        let mut slot = 0usize;
        while left > 0 && slot < PROC_CAP {
            if sys_edge.open(pid2, OpenKind::Open).is_ok() { left -= 1; }
            slot += 1;
        }
        pid2 += 1;
    }
    s.add("边界：系统表剩最后 1 槽可开，随后转 ENFILE",
          sys_edge.file_used() == SYS_CAP - 1
              && sys_edge.open(2, OpenKind::Socket) == Ok(0)
              && sys_edge.open(2, OpenKind::Open) == Err(ENFILE),
          "系统表第 12 格是最后可成格，第 13 个必须 ENFILE");

    let mut rel = SysVfs::new();
    let _ = rel.open(0, OpenKind::Open);
    let closed = rel.close(0, 0);
    s.add("关闭双退：close 同时退进程槽与系统 file 账，二次 close 回 EBADF",
          closed == Ok(0) && rel.proc_used(0) == 0 && rel.file_used() == 0
              && rel.close(0, 0) == Err(EBADF) && rel.close(PROC_COUNT, 0) == Err(EBADF),
          "两本账必须同步退，坏 pid/坏 fd 一律 EBADF");

    let (ledger, per) = run_enfile_matrix();
    s.add("注入必抓：系统满/双满/恢复三类各 10/10",
          matches!(per.get(0), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(1), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(2), Some(v) if *v == INJECT_ROUNDS),
          "三类各 10 轮全检出，系统满那类还要验进程槽未被消费");

    s.add("守恒：注入 30 次 = 检出 + 漏检，漏检为 0",
          ledger.detects == INJECT_ROUNDS * INJECT_CLASSES as u32
              && ledger.misses == 0 && ledger.conserved(),
          "检出与漏检之和恒等于注入轮数");

    let mut cons = SysVfs::new();
    let _ = cons.open(0, OpenKind::Open);
    let _ = cons.open(1, OpenKind::Open);
    let _ = cons.open(1, OpenKind::Open);
    let _ = cons.close(1, 0);
    s.add("守恒：opens − closes == 系统占用 == 各进程占用之和",
          cons.opens() == 3 && cons.closes() == 1
              && cons.file_used() == 2 && cons.procs_used() == 2,
          "三账必须逐笔相等：开 3 关 1 余 2");

    s.add("判序注记：open 为 EMFILE→ENFILE，pipe2 为 ENFILE→EMFILE，两条 hook 齐全",
          order_notes_ok() && ORDER_NOTES.len() == 2,
          "两条路径方向相反是内核事实，注记缺一条就分不清谁先谁后");

    let f1 = patrol_fingerprint();
    let f2 = patrol_fingerprint();
    s.add("双跑一致：同脚本两次指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同输入必须逐字段同指纹");

    let (rok, rtotal) = reconcile();
    s.add("对账：8 行 ENFILE/EMFILE 场景回码与对照表一致",
          rok == rtotal && rtotal == 8,
          "ENFILE/EMFILE/EBADF 与 Windows 4/6 对表，逐行可复算");

    let mut text_closed = true;
    let mut d = 0usize;
    while let Some(code) = EnfileCode::ALL.get(d) {
        if code.detail().is_empty() { text_closed = false; }
        if code.val() < SEG_BASE || code.val() > SEG_BASE + 0x3F { text_closed = false; }
        d += 1;
    }
    s.add("文案闭集：13 个诊断码均有非空中文建议且落本段", text_closed,
          "只报码不给路等于没报，诊断码必须自带处置建议");

    let summary = enfile_summary();
    s.add("收口：摘要非空且含对账/注入/判序三组数",
          !summary.is_empty() && summary.contains("对账")
              && summary.contains("注入") && summary.contains("判序"),
          "写回摘要必须一眼看出对账、注入与判序结论");

    let mut dyn_vfs = SysVfs::new();
    fill_system(&mut dyn_vfs);
    let dyn_block = dyn_vfs.open(2, OpenKind::Open);
    let dyn_used = dyn_vfs.proc_used(2);
    s.add_dyn("ENFILE 明细", dyn_block == Err(ENFILE) && dyn_used == 0,
              &format!("系统 12/12 → ENFILE，p2 占用 {}", dyn_used));
    let mut dyn_both = SysVfs::new();
    fill_system(&mut dyn_both);
    let mut b = 0usize;
    while b < PROC_CAP {
        let _ = dyn_both.open(0, OpenKind::Open);
        b += 1;
    }
    let dyn_order = dyn_both.open(0, OpenKind::Open);
    s.add_dyn("EMFILE 明细", dyn_order == Err(EMFILE),
              &format!("p0 8/8 且系统满 → {:?}", dyn_order));
    let recon_text = recon_mismatch_text();
    s.add_dyn("对账失配明细", recon_text.starts_with("8 行"), &recon_text);
    s.add_dyn("注入账簿", ledger.conserved() && ledger.misses == 0, &ledger.summary());

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_open_lowest() {
        let mut vfs = SysVfs::new();
        assert_eq!(vfs.open(0, OpenKind::Open), Ok(0));
        assert_eq!(vfs.open(0, OpenKind::Creat), Ok(1));
        assert_eq!(vfs.open(1, OpenKind::Open), Ok(0));
        assert_eq!(vfs.proc_used(0), 2);
        assert_eq!(vfs.proc_used(1), 1);
        assert_eq!(vfs.file_used(), 3);
        assert_eq!(vfs.procs_used(), 3);
    }

    #[test]
    fn proc_cap_emfile() {
        let mut vfs = SysVfs::new();
        let mut i = 0usize;
        while i < PROC_CAP {
            assert!(vfs.open(1, OpenKind::Open).is_ok());
            i += 1;
        }
        assert_eq!(vfs.open(1, OpenKind::Open), Err(EMFILE));
        assert_eq!(vfs.proc_used(1), PROC_CAP as u32);
        assert_eq!(vfs.file_used(), PROC_CAP as u32);
        assert_eq!(vfs.emfile(), 1);
    }

    #[test]
    fn sys_cap_enfile() {
        let mut vfs = SysVfs::new();
        fill_system(&mut vfs);
        assert_eq!(vfs.file_used(), SYS_CAP);
        let before = vfs.proc_used(2);
        assert_eq!(vfs.open(2, OpenKind::Open), Err(ENFILE));
        assert_eq!(vfs.proc_used(2), before);
        assert_eq!(vfs.enfile(), 1);
    }

    #[test]
    fn precedence_probe() {
        assert_eq!(judge_order_probe(), (EMFILE, ENFILE, 0));
        assert!(order_notes_ok());
        assert_eq!(ORDER_NOTES.len(), 2);
    }

    #[test]
    fn enfile_grid() {
        let mut vfs = SysVfs::new();
        fill_system(&mut vfs);
        let mut holes = 0u32;
        let mut pid = 1usize;
        while pid < PROC_COUNT {
            let mut k = 0usize;
            while let Some(kind) = OpenKind::ALL.get(k) {
                if vfs.open(pid, *kind) == Err(ENFILE) { holes += 1; }
                k += 1;
            }
            pid += 1;
        }
        assert_eq!(holes, 6);
        assert_eq!(vfs.procs_used(), SYS_CAP);
    }

    #[test]
    fn release_recovery() {
        let mut vfs = SysVfs::new();
        fill_system(&mut vfs);
        assert_eq!(vfs.open(2, OpenKind::Open), Err(ENFILE));
        assert_eq!(vfs.close(1, 0), Ok(0));
        assert_eq!(vfs.open(2, OpenKind::Open), Ok(0));
        assert_eq!(vfs.file_used(), SYS_CAP);
    }

    #[test]
    fn cross_process_isolation() {
        let mut vfs = SysVfs::new();
        let mut i = 0usize;
        while i < PROC_CAP {
            assert!(vfs.open(0, OpenKind::Open).is_ok());
            i += 1;
        }
        let mut j = 0usize;
        while j < (SYS_CAP as usize - PROC_CAP) {
            assert!(vfs.open(1, OpenKind::Open).is_ok());
            j += 1;
        }
        assert_eq!(vfs.file_used(), SYS_CAP);
        assert_eq!(vfs.open(2, OpenKind::Open), Err(ENFILE));
        assert_eq!(vfs.proc_used(0), PROC_CAP as u32);
        assert!(vfs.fd_open(0, 7));
        assert_eq!(vfs.close(0, 7), Ok(0));
        assert_eq!(vfs.open(2, OpenKind::Open), Ok(0));
    }

    #[test]
    fn injection_and_conservation() {
        let (ledger, per) = run_enfile_matrix();
        assert_eq!(ledger.detects, INJECT_ROUNDS * INJECT_CLASSES as u32);
        assert_eq!(ledger.misses, 0);
        assert!(ledger.conserved());
        assert_eq!(per, [INJECT_ROUNDS, INJECT_ROUNDS, INJECT_ROUNDS]);
        assert_eq!(ledger.per(ENFILE), INJECT_ROUNDS);
        assert_eq!(ledger.per(EMFILE), INJECT_ROUNDS);
        assert_eq!(ledger.per(0), INJECT_ROUNDS);
    }

    #[test]
    fn determinism_recon_text() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(patrol_fingerprint(), 0);
        let (ok, total) = reconcile();
        assert_eq!(ok, total);
        assert_eq!(total, 8);
        assert!(recon_mismatch_text().starts_with("8 行"));
        assert!(enfile_summary().contains("判序"));
        assert_eq!(EnfileCode::ALL.len(), 13);
        let mut i = 0usize;
        while let Some(code) = EnfileCode::ALL.get(i) {
            assert!(!code.detail().is_empty());
            assert!(code.val() >= SEG_BASE && code.val() <= SEG_BASE + 0x3F);
            i += 1;
        }
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }
}
