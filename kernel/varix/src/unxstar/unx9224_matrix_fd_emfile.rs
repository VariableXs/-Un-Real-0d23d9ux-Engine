//! UNX-F9224 fd 表耗尽 EMFILE 矩阵（RLIMIT_NOFILE 联动） · 0x7A580段
//!
//! 判据原文（逐字摘录）：
//! ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9224-J1 限额触顶注入判据过，
//! 分配全 EMFILE 格过，限额上调后恢复正判据格过，与 B19 资源族联动格过；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. RLIMIT_NOFILE 双限：soft 是分配闸门，hard 是 soft 的天花板；非特权进程
//!    只能把 hard 往下压，往上抬必须走特权路径（EPERM）。
//! 2. 限额判定序：负值/soft>hard → EINVAL，先于 hard 越权 → EPERM，
//!    最后才轮到分配触顶 → EMFILE；三关同在时按此顺序回码。
//! 3. 降限不杀存量：soft 降到当前使用量以下合法，已有 fd 不受影响，
//!    但新分配立即 EMFILE，直到关到 soft 以下才恢复。
//! 4. 三条分配路径（open/dup/socket）共用同一闸门，触顶时逐格 EMFILE，
//!    不允许某一路偷偷放行。
//! 5. 与 B19 资源族按槽位联动：NOFILE/NPROC/MEMLOCK 三行映射可逐项复算。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（派工表给定，0x7A580 独占 0x40 格）。
pub const SEG_BASE: u32 = 0x7A580;
/// fd 位图容量，也就是 hard 的物理上限。
pub const TABLE_CAP: usize = 32;
/// 默认 soft 限。
pub const DEFAULT_SOFT: i32 = 8;
/// 默认 hard 限。
pub const DEFAULT_HARD: i32 = 16;
/// hard 的可设置上限。
pub const HARD_MAX: i32 = TABLE_CAP as i32;
/// 每类违约注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 3;
/// fd 表耗尽。
pub const EMFILE: i32 = 24;
/// 越权改限。
pub const EPERM: i32 = 1;
/// 限额参数非法。
pub const EINVAL: i32 = 22;
/// 坏 fd。
pub const EBADF: i32 = 9;

/// 诊断码闭集：EMFILE 面被写错时的十三种形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmfileCode {
    SoftIgnored,
    HardBreached,
    PermWrong,
    NegativeLimit,
    RecoveryFailed,
    PeakRegress,
    CountMismatch,
    LinkBroken,
    OrderViolation,
    InheritLost,
    NotReproducible,
    SummaryEmpty,
    SegmentViolation,
}

impl EmfileCode {
    /// 诊断码值。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            EmfileCode::SoftIgnored => "软限被忽略：使用量达到 soft 后新分配必须 EMFILE",
            EmfileCode::HardBreached => "硬限被击穿：soft 永远不得超过 hard，越权抬 hard 也不行",
            EmfileCode::PermWrong => "越权判定错：非特权抬 hard 必须 EPERM，特权才放行",
            EmfileCode::NegativeLimit => "负限值被接受：soft/hard 必须非负，且 soft ≤ hard",
            EmfileCode::RecoveryFailed => "恢复失败：抬高 soft 后原本 EMFILE 的分配必须立即成功",
            EmfileCode::PeakRegress => "峰值回退：使用峰值只增不减，回退即账错",
            EmfileCode::CountMismatch => "账实不符：opens − closes 必须等于当前占用数",
            EmfileCode::LinkBroken => "B19 联动断链：资源族三行槽位必须连续且不重复",
            EmfileCode::OrderViolation => "判定次序倒置：EINVAL 先于 EPERM，EPERM 先于 EMFILE",
            EmfileCode::InheritLost => "限额继承丢失：子表复制后 soft/hard 与原表逐字段相等",
            EmfileCode::NotReproducible => "双跑不一致：同脚本两次指纹必须相等",
            EmfileCode::SummaryEmpty => "摘要为空：写回必须含触顶/恢复/联动三组数",
            EmfileCode::SegmentViolation => "段位越界：SEG_BASE 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }
    /// 全码表。
    pub const ALL: [EmfileCode; 13] = [
        EmfileCode::SoftIgnored, EmfileCode::HardBreached, EmfileCode::PermWrong,
        EmfileCode::NegativeLimit, EmfileCode::RecoveryFailed, EmfileCode::PeakRegress,
        EmfileCode::CountMismatch, EmfileCode::LinkBroken, EmfileCode::OrderViolation,
        EmfileCode::InheritLost, EmfileCode::NotReproducible, EmfileCode::SummaryEmpty,
        EmfileCode::SegmentViolation,
    ];
}

/// RLIMIT_NOFILE 双限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limit {
    pub soft: i32,
    pub hard: i32,
}

impl Limit {
    pub const fn new(soft: i32, hard: i32) -> Limit { Limit { soft, hard } }
    /// 参数自洽：非负且 soft 不超过 hard。
    pub const fn coherent(self) -> bool {
        self.soft >= 0 && self.hard >= 0 && self.soft <= self.hard
    }
    /// 默认双限。
    pub const fn default_pair() -> Limit { Limit::new(DEFAULT_SOFT, DEFAULT_HARD) }
}

/// 分配路径：三条路共用同一道 soft 闸门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocKind {
    Open,
    Dup,
    Socket,
}

impl AllocKind {
    /// 路径名。
    pub const fn label(self) -> &'static str {
        match self { AllocKind::Open => "open", AllocKind::Dup => "dup", AllocKind::Socket => "socket" }
    }
    /// 三路全表，矩阵按它逐格跑。
    pub const ALL: [AllocKind; 3] = [AllocKind::Open, AllocKind::Dup, AllocKind::Socket];
}

/// B19 资源族联动：本卡只负责 NOFILE 行，另两行用来验槽位对齐。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResKind {
    Nofile,
    Nproc,
    Memlock,
}

impl ResKind {
    pub const ALL: [ResKind; 3] = [ResKind::Nofile, ResKind::Nproc, ResKind::Memlock];
}

/// 资源族联动行。
#[derive(Clone, Copy, Debug)]
pub struct ResLink {
    pub kind: ResKind,
    pub b19_slot: u8,
    pub field: &'static str,
}

/// B19 资源族三行：槽位 1..3 连续，字段非空。
pub const RES_TABLE: [ResLink; 3] = [
    ResLink { kind: ResKind::Nofile, b19_slot: 1, field: "RLIMIT_NOFILE" },
    ResLink { kind: ResKind::Nproc, b19_slot: 2, field: "RLIMIT_NPROC" },
    ResLink { kind: ResKind::Memlock, b19_slot: 3, field: "RLIMIT_MEMLOCK" },
];

/// 联动表自检：槽位连续、无重复、字段非空、NOFILE 行必须在首位。
pub fn link_ok() -> bool {
    if ResKind::ALL.len() != RES_TABLE.len() { return false; }
    let mut i = 0usize;
    while i < RES_TABLE.len() {
        match RES_TABLE.get(i) {
            Some(row) => {
                if row.field.is_empty() { return false; }
                if row.b19_slot as usize != i + 1 { return false; }
            }
            None => return false,
        }
        i += 1;
    }
    match RES_TABLE.get(0) {
        Some(row) => row.kind == ResKind::Nofile,
        None => false,
    }
}

/// 进程 fd 限额表：位图 + 双限 + 五本账。
#[derive(Clone, Copy, Debug)]
pub struct FdLimitTable {
    limit: Limit,
    fds: [bool; TABLE_CAP],
    privileged: bool,
    opens: u32,
    closes: u32,
    peak: u32,
    emfiles: u32,
    eperms: u32,
    einvals: u32,
}

impl FdLimitTable {
    pub const fn new() -> FdLimitTable {
        FdLimitTable {
            limit: Limit::default_pair(),
            fds: [false; TABLE_CAP],
            privileged: false,
            opens: 0,
            closes: 0,
            peak: 0,
            emfiles: 0,
            eperms: 0,
            einvals: 0,
        }
    }
    /// 特权表：可抬 hard。
    pub fn privileged() -> FdLimitTable {
        let mut table = FdLimitTable::new();
        table.privileged = true;
        table
    }
    /// 当前双限。
    pub const fn limit(&self) -> Limit { self.limit }
    /// soft。
    pub const fn soft(&self) -> i32 { self.limit.soft }
    /// hard。
    pub const fn hard(&self) -> i32 { self.limit.hard }
    /// 是否特权。
    pub const fn is_privileged(&self) -> bool { self.privileged }
    /// 当前占用。
    pub fn open_count(&self) -> u32 {
        (0..TABLE_CAP).filter_map(|i| self.fds.get(i)).filter(|open| **open).count() as u32
    }
    /// 峰值。
    pub const fn peak(&self) -> u32 { self.peak }
    /// 累计 EMFILE。
    pub const fn emfiles(&self) -> u32 { self.emfiles }
    /// 累计 EPERM。
    pub const fn eperms(&self) -> u32 { self.eperms }
    /// 累计 EINVAL。
    pub const fn einvals(&self) -> u32 { self.einvals }
    /// 打开账。
    pub const fn opens(&self) -> u32 { self.opens }
    /// 关闭账。
    pub const fn closes(&self) -> u32 { self.closes }
    /// 某 fd 是否占用。
    pub fn fd_open(&self, fd: i32) -> bool {
        if fd < 0 { return false; }
        match self.fds.get(fd as usize) { Some(open) => *open, None => false }
    }
    /// 预占 n 个最低 fd（模拟进程启动时已继承的句柄）。
    pub fn fill(&mut self, n: usize) {
        let mut i = 0usize;
        while i < n && i < TABLE_CAP {
            let already = match self.fds.get(i) { Some(v) => *v, None => true };
            if !already {
                if let Some(slot) = self.fds.get_mut(i) { *slot = true; }
                self.opens = self.opens.saturating_add(1);
            }
            i += 1;
        }
        let used = self.open_count();
        if used > self.peak { self.peak = used; }
    }
    /// 找最低空槽。
    fn lowest_free(&self) -> Option<i32> {
        let mut i = 0usize;
        while i < TABLE_CAP {
            let taken = match self.fds.get(i) { Some(v) => *v, None => true };
            if !taken { return Some(i as i32); }
            i += 1;
        }
        None
    }
    /// 分配：soft 闸门 → 物理槽位，两关都回 EMFILE。
    pub fn alloc(&mut self, _kind: AllocKind) -> Result<i32, i32> {
        if self.open_count() >= self.limit.soft as u32 {
            self.emfiles = self.emfiles.saturating_add(1);
            return Err(EMFILE);
        }
        let fd = match self.lowest_free() {
            Some(v) => v,
            None => {
                self.emfiles = self.emfiles.saturating_add(1);
                return Err(EMFILE);
            }
        };
        if let Some(slot) = self.fds.get_mut(fd as usize) { *slot = true; }
        self.opens = self.opens.saturating_add(1);
        let used = self.open_count();
        if used > self.peak { self.peak = used; }
        Ok(fd)
    }
    /// 关闭：坏 fd 回 EBADF。
    pub fn close(&mut self, fd: i32) -> Result<i32, i32> {
        if !self.fd_open(fd) { return Err(EBADF); }
        if let Some(slot) = self.fds.get_mut(fd as usize) { *slot = false; }
        self.closes = self.closes.saturating_add(1);
        Ok(0)
    }
    /// 设置双限：EINVAL → EPERM 两关在前，成功才落表。
    pub fn setrlimit(&mut self, next: Limit) -> Result<i32, i32> {
        if !next.coherent() {
            self.einvals = self.einvals.saturating_add(1);
            return Err(EINVAL);
        }
        if next.hard > HARD_MAX || (next.hard > self.limit.hard && !self.privileged) {
            self.eperms = self.eperms.saturating_add(1);
            return Err(EPERM);
        }
        self.limit = next;
        Ok(0)
    }
    /// 子表继承：逐字段复制双限，句柄不复制（fork 语义只在本卡验限额）。
    pub fn inherit(&self) -> FdLimitTable {
        let mut child = FdLimitTable::new();
        child.limit = self.limit;
        child.privileged = self.privileged;
        child
    }
    /// 五本账。
    pub fn totals(&self) -> (u32, u32, u32) { (self.emfiles, self.eperms, self.einvals) }
    /// 指纹：位图 + 双限 + 计数。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < TABLE_CAP {
            if let Some(open) = self.fds.get(i) {
                bytes.push(if *open { 1 } else { 0 });
            }
            i += 1;
        }
        bytes.push((self.limit.soft & 0xFF) as u8);
        bytes.push((self.limit.hard & 0xFF) as u8);
        bytes.push((self.peak & 0xFF) as u8);
        bytes.push((self.emfiles & 0xFF) as u8);
        fnv1a(&bytes)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        let (emfiles, eperms, _) = self.totals();
        format!("NOFILE：soft {}/hard {} / 占 {} / 峰值 {} / EMFILE {} / EPERM {}",
                self.limit.soft, self.limit.hard, self.open_count(),
                self.peak, emfiles, eperms)
    }
}

/// 注入类别：触顶 / 越权 / 恢复。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmfileInject {
    SoftCap,
    HardBreach,
    Recovery,
}

impl EmfileInject {
    /// 该类别应回的 errno（恢复类为 0）。
    pub const fn expected(self) -> i32 {
        match self {
            EmfileInject::SoftCap => EMFILE,
            EmfileInject::HardBreach => EPERM,
            EmfileInject::Recovery => 0,
        }
    }
    /// 三类全表。
    pub const ALL: [EmfileInject; INJECT_CLASSES] = [
        EmfileInject::SoftCap, EmfileInject::HardBreach, EmfileInject::Recovery,
    ];
}

/// 注入账簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EmfileLedger {
    pub soft_cap: u32,
    pub hard_breach: u32,
    pub recovery: u32,
    pub detects: u32,
    pub misses: u32,
}

impl EmfileLedger {
    pub const fn empty() -> EmfileLedger {
        EmfileLedger { soft_cap: 0, hard_breach: 0, recovery: 0, detects: 0, misses: 0 }
    }
    /// 记一笔。
    pub fn account(&mut self, expected: i32, actual: i32, hit: bool) {
        if hit && actual == expected {
            match expected {
                EMFILE => self.soft_cap = self.soft_cap.saturating_add(1),
                EPERM => self.hard_breach = self.hard_breach.saturating_add(1),
                0 => self.recovery = self.recovery.saturating_add(1),
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
            && self.soft_cap.saturating_add(self.hard_breach)
                .saturating_add(self.recovery) == self.detects
    }
    /// 分类计数。
    pub fn per(&self, errno: i32) -> u32 {
        match errno {
            EMFILE => self.soft_cap,
            EPERM => self.hard_breach,
            0 => self.recovery,
            _ => 0,
        }
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("触顶 {} / 越权 {} / 恢复 {} / 检出 {} / 漏检 {}",
                self.soft_cap, self.hard_breach, self.recovery, self.detects, self.misses)
    }
}

/// 单轮注入：每轮微调 soft 值，避免十轮同参。
pub fn inject_once(kind: EmfileInject, round: u32) -> (i32, bool) {
    let mut table = FdLimitTable::new();
    match kind {
        EmfileInject::SoftCap => {
            let soft = DEFAULT_SOFT + (round as i32 % 3);
            let _ = table.setrlimit(Limit::new(soft, DEFAULT_HARD));
            table.fill(soft as usize);
            match table.alloc(AllocKind::Open) {
                Err(e) => (e, true),
                Ok(_) => (0, false),
            }
        }
        EmfileInject::HardBreach => {
            let want = Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 1 + (round as i32 % 3));
            match table.setrlimit(want) {
                Err(e) => (e, true),
                Ok(_) => (0, false),
            }
        }
        EmfileInject::Recovery => {
            table.fill(DEFAULT_SOFT as usize);
            let blocked = table.alloc(AllocKind::Open) == Err(EMFILE);
            let raised = table.setrlimit(Limit::new(DEFAULT_SOFT + 1 + (round as i32 % 3), DEFAULT_HARD)).is_ok();
            let worked = table.alloc(AllocKind::Open).is_ok();
            if blocked && raised && worked { (0, true) } else { (0, false) }
        }
    }
}

/// 三类 × 10 轮矩阵。
pub fn run_emfile_matrix() -> (EmfileLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = EmfileLedger::empty();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < INJECT_CLASSES {
            let kind = match EmfileInject::ALL.get(i) { Some(k) => *k, None => EmfileInject::SoftCap };
            let (actual, hit) = inject_once(kind, round);
            ledger.account(kind.expected(), actual, hit);
            i += 1;
        }
        round += 1;
    }
    let per = [ledger.soft_cap, ledger.hard_breach, ledger.recovery];
    (ledger, per)
}

/// 判定次序探针：EINVAL → EPERM → EMFILE。
pub fn judge_order_probe() -> (i32, i32, i32) {
    let mut table = FdLimitTable::new();
    let first = match table.setrlimit(Limit::new(20, 10)) {
        Err(e) => e,
        Ok(_) => 0,
    };
    let second = match table.setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 4)) {
        Err(e) => e,
        Ok(_) => 0,
    };
    table.fill(DEFAULT_SOFT as usize);
    let third = match table.alloc(AllocKind::Open) {
        Err(e) => e,
        Ok(_) => 0,
    };
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

/// 对账表八行：触顶、恢复、越权、非法限、降限。
pub const RECON_TABLE: [ReconRow; 8] = [
    ReconRow { scene: "open@soft=8", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { scene: "open@soft=9", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "setrlimit(20,10)", errno: EINVAL, name: "EINVAL", win: 87 },
    ReconRow { scene: "setrlimit(-1,16)", errno: EINVAL, name: "EINVAL", win: 87 },
    ReconRow { scene: "setrlimit(8,20) unpriv", errno: EPERM, name: "EPERM", win: 5 },
    ReconRow { scene: "setrlimit(8,24) priv", errno: 0, name: "OK", win: 0 },
    ReconRow { scene: "open@soft=4 used=8", errno: EMFILE, name: "EMFILE", win: 4 },
    ReconRow { scene: "open@soft=9 closed=1", errno: 0, name: "OK", win: 0 },
];

/// 按行重放。
pub fn replay(case: usize) -> i32 {
    let mut table = FdLimitTable::new();
    match case {
        0 => {
            table.fill(DEFAULT_SOFT as usize);
            match table.alloc(AllocKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        1 => {
            table.fill(DEFAULT_SOFT as usize);
            let _ = table.setrlimit(Limit::new(DEFAULT_SOFT + 1, DEFAULT_HARD));
            match table.alloc(AllocKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        2 => match table.setrlimit(Limit::new(20, 10)) { Err(e) => e, Ok(_) => 0 },
        3 => match table.setrlimit(Limit::new(-1, DEFAULT_HARD)) { Err(e) => e, Ok(_) => 0 },
        4 => match table.setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 4)) { Err(e) => e, Ok(_) => 0 },
        5 => {
            let mut priv_table = FdLimitTable::privileged();
            match priv_table.setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 8)) { Err(e) => e, Ok(_) => 0 }
        }
        6 => {
            table.fill(DEFAULT_SOFT as usize);
            let _ = table.setrlimit(Limit::new(4, DEFAULT_HARD));
            match table.alloc(AllocKind::Open) { Err(e) => e, Ok(_) => 0 }
        }
        7 => {
            table.fill(DEFAULT_SOFT as usize);
            let _ = table.setrlimit(Limit::new(DEFAULT_SOFT + 1, DEFAULT_HARD));
            let _ = table.close(0);
            match table.alloc(AllocKind::Open) { Err(e) => e, Ok(_) => 0 }
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

/// 巡逻脚本：触顶 → 抬限 → 分配 → 关闭，指纹定值。
pub fn patrol_fingerprint() -> u32 {
    let mut table = FdLimitTable::new();
    table.fill(DEFAULT_SOFT as usize);
    let _ = table.alloc(AllocKind::Open);
    let _ = table.setrlimit(Limit::new(DEFAULT_SOFT + 2, DEFAULT_HARD));
    let _ = table.alloc(AllocKind::Dup);
    let _ = table.close(0);
    table.fingerprint()
}

/// 写回摘要。
pub fn emfile_summary() -> String {
    let (ok, total) = reconcile();
    let (ledger, _) = run_emfile_matrix();
    format!("F9224 EMFILE 矩阵：对账 {}/{} / 注入 检{} 漏{} / 默认 {}/{} / 联动 {} 行",
            ok, total, ledger.detects, ledger.misses,
            DEFAULT_SOFT, DEFAULT_HARD, RES_TABLE.len())
}

/// 本卡判据入口。
pub fn run_unx9224_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9224");

    s.add("段位：0x7A580 0x40 对齐且落在 0x7A000..=0x7D1C0 内",
          SEG_BASE == 0x7A580 && SEG_BASE % 0x40 == 0
              && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0,
          "段位独占，越界即整册返工");

    let mut basic = FdLimitTable::new();
    let mut i = 0u32;
    while i < DEFAULT_SOFT as u32 {
        let _ = basic.alloc(AllocKind::Open);
        i += 1;
    }
    s.add("正常流：默认 soft=8，开满 8 个后第 9 个 EMFILE",
          basic.open_count() == DEFAULT_SOFT as u32
              && basic.alloc(AllocKind::Open) == Err(EMFILE)
              && basic.emfiles() == 1 && basic.peak() == DEFAULT_SOFT as u32,
          "默认双限 8/16，soft 一到就关闸，峰值停在 8");

    let mut grid = FdLimitTable::new();
    grid.fill(DEFAULT_SOFT as usize);
    let mut grid_all = true;
    let mut k = 0usize;
    while k < AllocKind::ALL.len() {
        if let Some(kind) = AllocKind::ALL.get(k) {
            if grid.alloc(*kind) != Err(EMFILE) { grid_all = false; }
        }
        k += 1;
    }
    s.add("分配全 EMFILE 格：open/dup/socket 三路在 soft 处全关闸",
          grid_all && grid.emfiles() == 3 && grid.open_count() == DEFAULT_SOFT as u32,
          "三条分配路径共用一道 soft 闸门，不许任何一路偷偷放行");

    let mut rec = FdLimitTable::new();
    rec.fill(DEFAULT_SOFT as usize);
    let blocked = rec.alloc(AllocKind::Open);
    let raised = rec.setrlimit(Limit::new(12, DEFAULT_HARD));
    let after = rec.alloc(AllocKind::Open);
    s.add("限额上调后恢复正判据：抬高 soft 后原 EMFILE 立刻可开",
          blocked == Err(EMFILE) && raised == Ok(0)
              && after == Ok(DEFAULT_SOFT as i32) && rec.soft() == 12,
          "恢复路径必须实测：先堵、再抬、再开，三步都要对上");

    let mut priv_table = FdLimitTable::privileged();
    let priv_raise = priv_table.setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 8));
    let plain = FdLimitTable::new();
    s.add("硬限越权：非特权抬 hard → EPERM，特权同参数 → Ok",
          {
              let mut plain_table = plain;
              plain_table.setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 8)) == Err(EPERM)
                  && plain_table.eperms() == 1
          },
          "hard 是 soft 的天花板，非特权只能下压不能上抬");

    s.add("特权抬 hard：soft 不动、hard 抬到 24 → Ok 且双限落表",
          priv_raise == Ok(0) && priv_table.hard() == DEFAULT_HARD + 8
              && priv_table.soft() == DEFAULT_SOFT,
          "特权抬 hard 只动 hard，soft 必须原样保留");

    let mut invalids = FdLimitTable::new();
    s.add("非法限：负 soft 与 soft>hard 均回 EINVAL，且不消费 EPERM 账",
          invalids.setrlimit(Limit::new(-1, DEFAULT_HARD)) == Err(EINVAL)
              && invalids.setrlimit(Limit::new(20, 10)) == Err(EINVAL)
              && invalids.einvals() == 2 && invalids.eperms() == 0,
          "参数自洽关排在越权关之前，非法值不许记成 EPERM");

    let mut lower = FdLimitTable::new();
    lower.fill(DEFAULT_SOFT as usize);
    let lowered = lower.setrlimit(Limit::new(4, DEFAULT_HARD));
    let still_blocked = lower.alloc(AllocKind::Open);
    let mut drained = lower;
    while drained.open_count() > 3 {
        let next = drained.open_count() as i32 - 1;
        let _ = drained.close(next);
    }
    let resumed = drained.alloc(AllocKind::Open);
    s.add("降限不杀存量：soft 降到使用量以下仍成功，关到线下才恢复",
          lowered == Ok(0) && still_blocked == Err(EMFILE)
              && resumed == Ok(3) && drained.open_count() == 4,
          "降限只关新分配不杀老句柄，关到 soft 以下才重新放行");

    let mut zero = FdLimitTable::new();
    let _ = zero.setrlimit(Limit::new(0, DEFAULT_HARD));
    s.add("边界：soft=0 → 一切分配 EMFILE，且物理表一格未动",
          zero.alloc(AllocKind::Open) == Err(EMFILE)
              && zero.alloc(AllocKind::Dup) == Err(EMFILE)
              && zero.alloc(AllocKind::Socket) == Err(EMFILE)
              && zero.open_count() == 0 && zero.emfiles() == 3,
          "soft=0 是合法配置，等同于全面关闸，但不得破坏位图");

    let mut cap = FdLimitTable::privileged();
    let _ = cap.setrlimit(Limit::new(HARD_MAX, HARD_MAX));
    let mut n = 0i32;
    while n < HARD_MAX {
        let _ = cap.alloc(AllocKind::Open);
        n += 1;
    }
    s.add("边界：soft/hard 同时顶到 32 → 开满 32 后第 33 个 EMFILE",
          cap.open_count() == HARD_MAX as u32
              && cap.alloc(AllocKind::Open) == Err(EMFILE)
              && cap.peak() == HARD_MAX as u32,
          "物理表 32 槽是最后上限，soft 再高也开不出第 33 个");

    let (o1, o2, o3) = judge_order_probe();
    s.add("判定次序：EINVAL > EPERM > EMFILE，三关不换位",
          o1 == EINVAL && o2 == EPERM && o3 == EMFILE,
          "非法参数先于越权，越权先于分配触顶，回码次序必须固定");

    let (ledger, per) = run_emfile_matrix();
    s.add("注入必抓：触顶/越权/恢复三类各 10/10",
          matches!(per.get(0), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(1), Some(v) if *v == INJECT_ROUNDS)
              && matches!(per.get(2), Some(v) if *v == INJECT_ROUNDS),
          "三类各 10 轮全检出，恢复类还要验先堵后开");

    s.add("守恒：注入 30 次 = 检出 + 漏检，漏检为 0",
          ledger.detects == INJECT_ROUNDS * INJECT_CLASSES as u32
              && ledger.misses == 0 && ledger.conserved(),
          "检出与漏检之和恒等于注入轮数");

    let mut peak = FdLimitTable::new();
    peak.fill(4);
    let p1 = peak.peak();
    peak.fill(6);
    let p2 = peak.peak();
    let _ = peak.close(0);
    let p3 = peak.peak();
    s.add("峰值：只增不减，关闭不回退",
          p1 == 4 && p2 == 6 && p3 == 6,
          "峰值是资源族账的对照量，回退即判账错");

    s.add("B19 资源族联动：三行槽位连续、无重复、NOFILE 在首位",
          link_ok() && RES_TABLE.len() == 3
              && matches!(RES_TABLE.get(0), Some(row) if row.field == "RLIMIT_NOFILE"),
          "NOFILE/NPROC/MEMLOCK 三行必须与 B19 槽位对齐，断链即返工");

    let mut inherit = FdLimitTable::privileged();
    let _ = inherit.setrlimit(Limit::new(12, 24));
    let child = inherit.inherit();
    s.add("限额继承：子表 soft/hard 与原表逐字段相等",
          child.limit() == inherit.limit() && child.soft() == 12
              && child.hard() == 24 && child.is_privileged(),
          "fork 出的子表必须继承双限与特权位，否则子进程行为漂移");

    let f1 = patrol_fingerprint();
    let f2 = patrol_fingerprint();
    s.add("双跑一致：同脚本两次指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同输入必须逐字段同指纹");

    let (rok, rtotal) = reconcile();
    s.add("对账：8 行限额场景回码与对照表一致",
          rok == rtotal && rtotal == 8,
          "EMFILE/EPERM/EINVAL 与 Windows 4/5/87 对表，逐行可复算");

    let mut cons = FdLimitTable::new();
    cons.fill(3);
    let _ = cons.alloc(AllocKind::Open);
    let _ = cons.close(0);
    let _ = cons.close(1);
    s.add("守恒：opens − closes == 当前占用数",
          cons.opens() == 4 && cons.closes() == 2 && cons.open_count() == 2,
          "开 3 继承 + 1 新开 = 4，关 2 余 2，三账必须对齐");

    let mut text_closed = true;
    let mut d = 0usize;
    while let Some(code) = EmfileCode::ALL.get(d) {
        if code.detail().is_empty() { text_closed = false; }
        if code.val() < SEG_BASE || code.val() > SEG_BASE + 0x3F { text_closed = false; }
        d += 1;
    }
    s.add("文案闭集：13 个诊断码均有非空中文建议且落本段", text_closed,
          "只报码不给路等于没报，诊断码必须自带处置建议");

    let summary = emfile_summary();
    s.add("收口：摘要非空且含对账/注入/默认双限三组数",
          !summary.is_empty() && summary.contains("对账")
              && summary.contains("注入") && summary.contains("默认"),
          "写回摘要必须一眼看出对账、注入与默认限额");

    let mut dyn_table = FdLimitTable::new();
    dyn_table.fill(DEFAULT_SOFT as usize);
    let dyn_block = dyn_table.alloc(AllocKind::Open);
    let dyn_raise = dyn_table.setrlimit(Limit::new(10, DEFAULT_HARD));
    let dyn_ok = dyn_table.alloc(AllocKind::Open);
    s.add_dyn("恢复明细", dyn_block == Err(EMFILE) && dyn_raise == Ok(0) && dyn_ok.is_ok(),
              &format!("{} EMFILE → 抬到 10 → Ok({:?})", AllocKind::Open.label(), dyn_ok));
    let perm_probe = FdLimitTable::new().setrlimit(Limit::new(DEFAULT_SOFT, DEFAULT_HARD + 2));
    s.add_dyn("越权明细", perm_probe == Err(EPERM),
              &format!("非特权抬 hard → {:?}", perm_probe));
    let recon_text = recon_mismatch_text();
    s.add_dyn("对账失配明细", recon_text.starts_with("8 行"), &recon_text);
    s.add_dyn("注入账簿", ledger.conserved() && ledger.misses == 0, &ledger.summary());

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_soft_cap() {
        let mut table = FdLimitTable::new();
        assert_eq!(table.limit(), Limit::new(8, 16));
        let mut i = 0u32;
        while i < 8 {
            assert!(table.alloc(AllocKind::Open).is_ok());
            i += 1;
        }
        assert_eq!(table.alloc(AllocKind::Open), Err(EMFILE));
        assert_eq!(table.open_count(), 8);
        assert_eq!(table.emfiles(), 1);
        assert_eq!(table.peak(), 8);
    }

    #[test]
    fn grid_all_emfile() {
        let mut table = FdLimitTable::new();
        table.fill(8);
        let mut i = 0usize;
        while let Some(kind) = AllocKind::ALL.get(i) {
            assert_eq!(table.alloc(*kind), Err(EMFILE));
            i += 1;
        }
        assert_eq!(table.emfiles(), 3);
        assert_eq!(table.open_count(), 8);
    }

    #[test]
    fn raise_and_recover() {
        let mut table = FdLimitTable::new();
        table.fill(8);
        assert_eq!(table.alloc(AllocKind::Open), Err(EMFILE));
        assert_eq!(table.setrlimit(Limit::new(12, 16)), Ok(0));
        assert_eq!(table.alloc(AllocKind::Open), Ok(8));
        assert_eq!(table.soft(), 12);
        assert_eq!(table.open_count(), 9);
    }

    #[test]
    fn hard_raise_permissions() {
        let mut plain = FdLimitTable::new();
        assert_eq!(plain.setrlimit(Limit::new(8, 20)), Err(EPERM));
        assert_eq!(plain.hard(), 16);
        assert_eq!(plain.eperms(), 1);

        let mut priv_table = FdLimitTable::privileged();
        assert_eq!(priv_table.setrlimit(Limit::new(8, 24)), Ok(0));
        assert_eq!(priv_table.hard(), 24);
        assert_eq!(priv_table.soft(), 8);
    }

    #[test]
    fn invalid_limits_order() {
        let mut table = FdLimitTable::new();
        assert_eq!(table.setrlimit(Limit::new(-1, 16)), Err(EINVAL));
        assert_eq!(table.setrlimit(Limit::new(20, 10)), Err(EINVAL));
        assert_eq!(table.einvals(), 2);
        assert_eq!(table.eperms(), 0);
        assert_eq!(judge_order_probe(), (EINVAL, EPERM, EMFILE));
    }

    #[test]
    fn lower_below_usage() {
        let mut table = FdLimitTable::new();
        table.fill(8);
        assert_eq!(table.setrlimit(Limit::new(4, 16)), Ok(0));
        assert_eq!(table.alloc(AllocKind::Open), Err(EMFILE));
        let _ = table.close(7);
        let _ = table.close(6);
        let _ = table.close(5);
        assert_eq!(table.alloc(AllocKind::Open), Err(EMFILE));
        let _ = table.close(4);
        assert_eq!(table.alloc(AllocKind::Open), Err(EMFILE));
        let _ = table.close(3);
        assert_eq!(table.alloc(AllocKind::Open), Ok(3));
        assert_eq!(table.open_count(), 4);
    }

    #[test]
    fn soft_zero_and_table_cap() {
        let mut zero = FdLimitTable::new();
        assert_eq!(zero.setrlimit(Limit::new(0, 16)), Ok(0));
        assert_eq!(zero.alloc(AllocKind::Open), Err(EMFILE));
        assert_eq!(zero.open_count(), 0);

        let mut cap = FdLimitTable::privileged();
        assert_eq!(cap.setrlimit(Limit::new(HARD_MAX, HARD_MAX)), Ok(0));
        let mut i = 0i32;
        while i < HARD_MAX {
            assert!(cap.alloc(AllocKind::Socket).is_ok());
            i += 1;
        }
        assert_eq!(cap.alloc(AllocKind::Socket), Err(EMFILE));
        assert_eq!(cap.peak(), HARD_MAX as u32);
    }

    #[test]
    fn injection_and_conservation() {
        let (ledger, per) = run_emfile_matrix();
        assert_eq!(ledger.detects, INJECT_ROUNDS * INJECT_CLASSES as u32);
        assert_eq!(ledger.misses, 0);
        assert!(ledger.conserved());
        assert_eq!(per, [INJECT_ROUNDS, INJECT_ROUNDS, INJECT_ROUNDS]);
        assert_eq!(ledger.per(EMFILE), INJECT_ROUNDS);
        assert_eq!(ledger.per(EPERM), INJECT_ROUNDS);
        assert_eq!(ledger.per(0), INJECT_ROUNDS);
    }

    #[test]
    fn determinism_recon_link() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_ne!(patrol_fingerprint(), 0);
        let (ok, total) = reconcile();
        assert_eq!(ok, total);
        assert_eq!(total, 8);
        assert!(recon_mismatch_text().starts_with("8 行"));
        assert!(link_ok());
        assert_eq!(RES_TABLE.len(), 3);
        assert!(emfile_summary().contains("联动 3 行"));
        assert_eq!(EmfileCode::ALL.len(), 13);
        let mut i = 0usize;
        while let Some(code) = EmfileCode::ALL.get(i) {
            assert!(!code.detail().is_empty());
            i += 1;
        }
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }
}
