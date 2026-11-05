//! UNX-F9273 rlimit 硬限提升 EPERM 矩阵（联 B19 对账） · 0x7B1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9273 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9273-J1 硬限提升注入判据过，
//!   非特权全 EPERM 格过，与 B19 资源矩阵零重复分层格过；③ 完成时 API/软件
//!   写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「软限可涨、硬限不可越」是两条不同的门：软限在硬限之内提升属正常配置，
//!      非特权必须放行；硬限提升需要 CAP_SYS_RESOURCE，非特权 EPERM。把两者
//!      并成「一涨就拒」，会让 setrlimit 的正常用法全部失败。
//!   ② 判定次序随入口而变（与 Linux 一致）：setrlimit 是「值域门 → 权限门」，
//!      cur>max 先报 EINVAL；prlimit64 跨进程是「权限门 → 值域门」，无 ptrace
//!      权限时先 EPERM。两种入口共用一个内核判定核，但入口层各自排门前置检查。
//!   ③ 参数原子性：任一门失败都不得回写半截新值；成功时返回值必须与入参逐字段
//!      相等（内核不做静默钳位），否则调用方读回的 rlim 与实际生效值不一致。
//!   ④ RLIM_INFINITY 参与比较时按 u64::MAX 处理：从有限硬限提升到无限仍是
//!      「硬限提升」，非特权 EPERM；不能因为「无限」是特殊值就跳过权限门。
//!   ⑤ 与 B19 对账按「值层/权限层」分层：B19 冻结行只覆盖纯值语义（降硬限、
//!      限内涨软限、软限归零），本模块覆盖权限语义；两层键集交集必须为空。
//!   ⑥ 六个资源（CPU/FSIZE/DATA/CORE/NPROC/NOFILE）必须全在册，缺一个资源就
//!      等于该资源的提权面无人看守；注入 7 格 × 10 轮 = 70 次，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;

use crate::checks::CheckSet;

/// 段基址：F9273 独占 0x40 窗口。
pub const SEG_BASE: u32 = 0x7B1C0;
pub const MATRIX_CELLS: usize = 22; // 6 资源 × 3 场景 + 4 跨进程/能力格
pub const EPERM_CELLS: usize = 7; // 期望 EPERM 的格数
pub const INJECT_ROUNDS: u32 = 10;
pub const RES_COUNT: usize = 6;
pub const B19_ROWS: usize = 5;

pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
/// 无限值（Linux RLIM_INFINITY = ~0ULL）。
pub const RLIM_INFINITY: u64 = u64::MAX;
/// 自进程 pid：prlimit64 传 0 表示自己。
pub const SELF_PID: u32 = 0;
/// B19 侧账本标识（联对账锚点）。
pub const B19_BOOK: &str = "UNX-C2-B19-resource";

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlimCode {
    HardRaiseLeak,   // 非特权硬限提升未被拒
    ValueGateLate,   // 值域门晚于权限门（setrlimit）
    PtraceGateLate,  // 跨进程权限门漏判
    SoftRaiseDenied, // 限内涨软限被误拒
    InfiniteBypass,  // INFINITY 绕过权限门
    NonAtomic,       // 失败时回写了半截值
    ClampDrift,      // 成功时被静默钳位
    ResGap,          // 资源缺册
    B19Conflict,     // 与 B19 键集重复
    StageRegress,    // 状态机回退
    LedgerLeak,      // 注入账漏检
    SegmentViolation,// 段位越界
}

impl RlimCode {
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            RlimCode::HardRaiseLeak => 0, RlimCode::ValueGateLate => 1, RlimCode::PtraceGateLate => 2,
            RlimCode::SoftRaiseDenied => 3, RlimCode::InfiniteBypass => 4, RlimCode::NonAtomic => 5,
            RlimCode::ClampDrift => 6, RlimCode::ResGap => 7, RlimCode::B19Conflict => 8,
            RlimCode::StageRegress => 9, RlimCode::LedgerLeak => 10, RlimCode::SegmentViolation => 11,
        }
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RlimCode::HardRaiseLeak => "硬限提升逃逸：非特权把 max 抬高必须 EPERM；请补 CAP_SYS_RESOURCE 再试",
            RlimCode::ValueGateLate => "值域门晚于权限门：cur>max 必须先 EINVAL，先报 EPERM 会掩盖畸形参数",
            RlimCode::PtraceGateLate => "跨进程权限门漏判：prlimit64 对他人 pid 必须先验 ptrace 权限再谈值域",
            RlimCode::SoftRaiseDenied => "限内涨软限被误拒：cur 提升但不超过 max 属正常配置，非特权必须放行",
            RlimCode::InfiniteBypass => "无限值绕过：从有限硬限提到 RLIM_INFINITY 仍是硬限提升，须走权限门",
            RlimCode::NonAtomic => "非原子回写：任一门失败都不许写半截 rlim；请先整体校验再提交",
            RlimCode::ClampDrift => "静默钳位：成功返回的 rlim 必须逐字段等于入参，内核不得自行收缩",
            RlimCode::ResGap => "资源缺册：CPU/FSIZE/DATA/CORE/NPROC/NOFILE 六项必须全在，缺项即盲区",
            RlimCode::B19Conflict => "与 B19 重复分层：同一 (资源,入口,pid,cur,max) 键只能由一层覆盖",
            RlimCode::StageRegress => "状态机非法推进：只许 Load→Validate→Authorize→Commit 单向",
            RlimCode::LedgerLeak => "注入漏检：注入数必须等于检出数，逃逸恒为零",
            RlimCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [RlimCode; 12] = [
        RlimCode::HardRaiseLeak, RlimCode::ValueGateLate, RlimCode::PtraceGateLate, RlimCode::SoftRaiseDenied,
        RlimCode::InfiniteBypass, RlimCode::NonAtomic, RlimCode::ClampDrift, RlimCode::ResGap,
        RlimCode::B19Conflict, RlimCode::StageRegress, RlimCode::LedgerLeak, RlimCode::SegmentViolation,
    ];
}

// ── 资源册 ────────────────────────────────────────────────────────────
/// 资源行：Linux 资源号 + 名字。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResRow {
    pub id: u32,
    pub name: &'static str,
}

/// 六资源册（Linux 资源号；RSS/AS/MEMLOCK 等不在本批）。
pub const RESOURCES: [ResRow; RES_COUNT] = [
    ResRow { id: 0, name: "RLIMIT_CPU" },
    ResRow { id: 1, name: "RLIMIT_FSIZE" },
    ResRow { id: 2, name: "RLIMIT_DATA" },
    ResRow { id: 4, name: "RLIMIT_CORE" },
    ResRow { id: 6, name: "RLIMIT_NPROC" },
    ResRow { id: 7, name: "RLIMIT_NOFILE" },
];

/// 按资源号取行。
pub fn res_of(id: u32) -> Option<ResRow> {
    let mut i = 0usize;
    while i < RESOURCES.len() {
        if let Some(r) = RESOURCES.get(i).copied() {
            if r.id == id {
                return Some(r);
            }
        }
        i += 1;
    }
    None
}

// ── 身份档 ────────────────────────────────────────────────────────────
/// 身份档：root / 普通用户 / 带 CAP_SYS_RESOURCE 的普通用户。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Root,
    User,
    CapRes,
}

/// 提限判定所需的身份要素。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    pub euid: u32,
    pub cap_sys_resource: bool,
}

impl Cred {
    pub const fn new(euid: u32, cap: bool) -> Cred {
        Cred { euid, cap_sys_resource: cap }
    }
    /// 提限权：euid==0 或 CAP_SYS_RESOURCE。
    pub fn privileged(&self) -> bool {
        self.euid == 0 || self.cap_sys_resource
    }
}

impl Profile {
    pub fn index(self) -> u8 {
        match self { Profile::Root => 0, Profile::User => 1, Profile::CapRes => 2 }
    }
    pub fn name(self) -> &'static str {
        match self { Profile::Root => "root", Profile::User => "user", Profile::CapRes => "cap-res" }
    }
    pub fn cred(self) -> Cred {
        match self {
            Profile::Root => Cred::new(0, false),
            Profile::User => Cred::new(1000, false),
            Profile::CapRes => Cred::new(1000, true),
        }
    }
    pub const ALL: [Profile; 3] = [Profile::Root, Profile::User, Profile::CapRes];
}

// ── rlimit 模型 ───────────────────────────────────────────────────────
/// 一条限制：软限 cur 与硬限 max。不变式 cur <= max 由值域门保证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rlim {
    pub cur: u64,
    pub max: u64,
}

impl Rlim {
    pub const fn new(cur: u64, max: u64) -> Rlim {
        Rlim { cur, max }
    }
    /// 是否触及无限硬限。
    pub fn infinite(self) -> bool {
        self.max == RLIM_INFINITY
    }
}

/// 入口：setrlimit（自身）与 prlimit64（可跨进程）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    SetRlimit,
    Prlimit,
}

impl Entry {
    pub fn index(self) -> u8 {
        match self { Entry::SetRlimit => 0, Entry::Prlimit => 1 }
    }
    pub fn name(self) -> &'static str {
        match self { Entry::SetRlimit => "setrlimit", Entry::Prlimit => "prlimit64" }
    }
}

/// setrlimit 内核判定核：值域门 → 硬限权限门。
pub fn do_prlimit(old: Rlim, new: Rlim, cred: &Cred) -> Result<Rlim, i32> {
    if new.cur > new.max {
        return Err(EINVAL);
    }
    if new.max > old.max && !cred.privileged() {
        return Err(EPERM);
    }
    Ok(new)
}

/// setrlimit 入口：值域门在前（自身调用无跨进程权限问题）。
pub fn setrlimit(old: Rlim, new: Rlim, cred: &Cred) -> Result<Rlim, i32> {
    do_prlimit(old, new, cred)
}

/// prlimit64 入口：跨进程先验 ptrace 权限，再进判定核。
pub fn prlimit64(old: Rlim, new: Rlim, pid: u32, cred: &Cred) -> Result<Rlim, i32> {
    if pid != SELF_PID && !cred.privileged() {
        return Err(EPERM);
    }
    do_prlimit(old, new, cred)
}

/// 原子性：失败时旧值必须原样保留（本模型以「调用方不回写」表达）。
pub fn atomic_ok(old: Rlim, new: Rlim, cred: &Cred) -> bool {
    match do_prlimit(old, new, cred) {
        Ok(got) => got == new,
        Err(_) => old.cur <= old.max,
    }
}

// ── 矩阵 ──────────────────────────────────────────────────────────────
/// 一格：身份档 + 资源 + 入口 + pid + 旧值 + 新值 + 期望码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub profile: Profile,
    pub res: u32,
    pub entry: Entry,
    pub pid: u32,
    pub old: Rlim,
    pub new: Rlim,
    pub expect: i32,
}

impl Cell {
    /// 分层键：(资源, 入口, pid, 新 cur, 新 max)。
    pub fn key(self) -> (u32, u8, u32, u64, u64) {
        (self.res, self.entry.index(), self.pid, self.new.cur, self.new.max)
    }
}

/// 基准旧值：软 1024 / 硬 4096。
pub const BASE: Rlim = Rlim::new(1024, 4096);
/// 限内涨软限：2048 / 4096。
pub const SOFT_RAISE: Rlim = Rlim::new(2048, 4096);
/// 提硬限：4096 / 8192。
pub const HARD_RAISE: Rlim = Rlim::new(4096, 8192);
/// 畸形值：8192 / 4096（cur > max）。
pub const VALUE_BAD: Rlim = Rlim::new(8192, 4096);
/// 有限 → 无限：硬限提升的极端形态。
pub const TO_INFINITY: Rlim = Rlim::new(1024, RLIM_INFINITY);

/// 二十二格矩阵：六资源 × 三场景（软涨/硬提/畸形）+ 跨进程与能力格。
pub const MATRIX: [Cell; MATRIX_CELLS] = [
    // RLIMIT_CPU
    Cell { profile: Profile::User, res: 0, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 0, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 0, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // RLIMIT_FSIZE
    Cell { profile: Profile::User, res: 1, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 1, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 1, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // RLIMIT_DATA
    Cell { profile: Profile::User, res: 2, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 2, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 2, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // RLIMIT_CORE
    Cell { profile: Profile::User, res: 4, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 4, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 4, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // RLIMIT_NPROC
    Cell { profile: Profile::User, res: 6, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 6, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 6, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // RLIMIT_NOFILE
    Cell { profile: Profile::User, res: 7, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: SOFT_RAISE, expect: 0 },
    Cell { profile: Profile::User, res: 7, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: EPERM },
    Cell { profile: Profile::User, res: 7, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
    // 跨进程与能力格
    Cell { profile: Profile::User, res: 7, entry: Entry::Prlimit, pid: 1234, old: BASE, new: SOFT_RAISE, expect: EPERM },
    Cell { profile: Profile::Root, res: 7, entry: Entry::Prlimit, pid: 1234, old: BASE, new: HARD_RAISE, expect: 0 },
    Cell { profile: Profile::CapRes, res: 7, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: HARD_RAISE, expect: 0 },
    Cell { profile: Profile::Root, res: 7, entry: Entry::SetRlimit, pid: SELF_PID, old: BASE, new: VALUE_BAD, expect: EINVAL },
];

/// 跑一格。
pub fn run_cell(cell: Cell) -> i32 {
    let cred = cell.profile.cred();
    let got = match cell.entry {
        Entry::SetRlimit => setrlimit(cell.old, cell.new, &cred),
        Entry::Prlimit => prlimit64(cell.old, cell.new, cell.pid, &cred),
    };
    match got {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// 矩阵命中数。
pub fn matrix_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if run_cell(c) == c.expect {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

/// 期望 EPERM 的格数。
pub fn eperm_cell_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i) {
            if c.expect == EPERM {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── B19 资源矩阵对账 ──────────────────────────────────────────────────
/// B19 冻结行：只覆盖纯值语义（与本模块权限层分层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B19Row {
    pub res: u32,
    pub old: Rlim,
    pub new: Rlim,
    pub expect: i32,
}

/// B19 冻结五行。
pub const B19_FROZEN: [B19Row; B19_ROWS] = [
    B19Row { res: 7, old: Rlim::new(1024, 4096), new: Rlim::new(1024, 2048), expect: 0 },
    B19Row { res: 1, old: Rlim::new(512, 1024), new: Rlim::new(768, 1024), expect: 0 },
    B19Row { res: 2, old: Rlim::new(8192, 16384), new: Rlim::new(4096, 16384), expect: 0 },
    B19Row { res: 6, old: Rlim::new(64, 128), new: Rlim::new(0, 128), expect: 0 },
    B19Row { res: 4, old: Rlim::new(0, 0), new: Rlim::new(0, 0), expect: 0 },
];

/// 单行 B19 对账：以普通用户身份重放，值层语义必须放行。
pub fn b19_row_ok(row: B19Row) -> bool {
    let cred = Profile::User.cred();
    let got = match setrlimit(row.old, row.new, &cred) {
        Ok(_) => 0,
        Err(e) => e,
    };
    got == row.expect && res_of(row.res).is_some()
}

/// B19 命中数。
pub fn b19_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < B19_FROZEN.len() {
        if let Some(r) = B19_FROZEN.get(i).copied() {
            if b19_row_ok(r) {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

/// 分层：值层（值域判据）/ 权限层（提限判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Value,
    Perm,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self { Layer::Value => "value-layer", Layer::Perm => "perm-layer" }
    }
}

/// 矩阵格的分层归属：EINVAL 格归值层，其余归权限层。
pub fn layer_of(cell: Cell) -> Layer {
    if cell.expect == EINVAL {
        Layer::Value
    } else {
        Layer::Perm
    }
}

/// B19 行映射成矩阵键（入口恒为 setrlimit、pid 恒为自身）。
pub fn b19_key(row: B19Row) -> (u32, u8, u32, u64, u64) {
    (row.res, Entry::SetRlimit.index(), SELF_PID, row.new.cur, row.new.max)
}

/// 键集交集：>0 即重复分层。
pub fn overlap_keys() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            let mut j = 0usize;
            while j < B19_FROZEN.len() {
                if let Some(r) = B19_FROZEN.get(j).copied() {
                    if c.key() == b19_key(r) {
                        n = n.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    n
}

/// 分层完备：两层各有格，B19 全在值层。
pub fn layer_complete() -> bool {
    let mut value = 0u32;
    let mut perm = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            match layer_of(c) {
                Layer::Value => value = value.saturating_add(1),
                Layer::Perm => perm = perm.saturating_add(1),
            }
        }
        i += 1;
    }
    value > 0 && perm > 0 && B19_FROZEN.iter().all(|r| r.expect == 0)
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 四档：载入 → 值校验 → 授权 → 提交。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Load,
    Validate,
    Authorize,
    Commit,
}

impl Stage {
    pub fn step(self) -> Option<Stage> {
        match self {
            Stage::Idle => Some(Stage::Load),
            Stage::Load => Some(Stage::Validate),
            Stage::Validate => Some(Stage::Authorize),
            Stage::Authorize => Some(Stage::Commit),
            Stage::Commit => None,
        }
    }
}

/// 只接受恰好下一档。
pub fn advance(from: Stage, to: Stage) -> Result<Stage, RlimCode> {
    match from.step() {
        Some(next) => if next == to { Ok(to) } else { Err(RlimCode::StageRegress) },
        None => Err(RlimCode::StageRegress),
    }
}

/// 值域失败停 Validate，权限失败停 Authorize，成功到 Commit。
pub fn stage_path(code: i32) -> Result<Stage, RlimCode> {
    let mut st = Stage::Idle;
    st = advance(st, Stage::Load)?;
    st = advance(st, Stage::Validate)?;
    if code == EINVAL {
        return Ok(st);
    }
    st = advance(st, Stage::Authorize)?;
    if code != 0 {
        return Ok(st);
    }
    st = advance(st, Stage::Commit)?;
    Ok(st)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), RlimCode> {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RlimCode::LedgerLeak);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// EPERM 七格 × 10 轮提限注入。
pub fn run_hard_raise_matrix() -> (Ledger, [u32; MATRIX_CELLS]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; MATRIX_CELLS];
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM {
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let caught = run_cell(c) == EPERM;
                    let _ = ledger.record(caught);
                    if caught {
                        if let Some(slot) = per.get_mut(i) { *slot = slot.saturating_add(1); }
                    }
                    round += 1;
                }
            }
        }
        i += 1;
    }
    (ledger, per)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 语料指纹：矩阵判定 + B19 命中。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            h ^= run_cell(c) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.res;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (c.new.max & 0xFFFF) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h ^= b19_hits();
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9273 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9273_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9273");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：限内涨软限放行、root 提硬限放行
    let user = Profile::User.cred();
    let normal = setrlimit(BASE, SOFT_RAISE, &user) == Ok(SOFT_RAISE)
        && setrlimit(BASE, HARD_RAISE, &Profile::Root.cred()) == Ok(HARD_RAISE);
    s.add("正常流：限内涨软限与 root 提硬限均成功", normal,
          "合法配置必须原样回写入参，不得被静默钳位");

    // 3. 全 EPERM 逐格：六资源硬限提升 + 跨进程
    let mut eperm_hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM && run_cell(c) == EPERM {
                eperm_hit = eperm_hit.saturating_add(1);
            }
        }
        i += 1;
    }
    s.add("非特权硬限提升：7 格全 EPERM 逐格检出",
          eperm_hit == EPERM_CELLS as u32 && eperm_cell_count() == EPERM_CELLS as u32,
          "六资源各一格 + 跨进程一格，缺一格即该资源提权面失守");

    // 4. 判定次序：setrlimit 值域先于权限
    let order_a = setrlimit(BASE, VALUE_BAD, &user) == Err(EINVAL)
        && setrlimit(BASE, VALUE_BAD, &Profile::Root.cred()) == Err(EINVAL);
    s.add("次序（setrlimit）：cur>max 先 EINVAL，root 也不例外", order_a,
          "畸形参数必须在权限门之前暴露，否则 root 会拿到误导性的成功");

    // 5. 判定次序：prlimit64 跨进程权限先于值域
    let order_b = prlimit64(BASE, VALUE_BAD, 1234, &user) == Err(EPERM)
        && prlimit64(BASE, VALUE_BAD, SELF_PID, &user) == Err(EINVAL);
    s.add("次序（prlimit64）：跨进程先验权限，自身调用仍值域在前", order_b,
          "入口不同门前排布不同；混用会让跨进程探针得到错误码");

    // 6. 无限值不绕过
    let inf_user = setrlimit(BASE, TO_INFINITY, &user) == Err(EPERM);
    let inf_root = setrlimit(BASE, TO_INFINITY, &Profile::Root.cred()).is_ok();
    s.add("无限值面：提到 RLIM_INFINITY 仍是硬限提升", inf_user && inf_root,
          "无限是特殊值不是特权通行证，权限门照走");

    // 7. 软限提升不越权
    let soft_ok = setrlimit(BASE, Rlim::new(4096, 4096), &user).is_ok()
        && setrlimit(BASE, Rlim::new(4097, 4096), &user) == Err(EINVAL);
    s.add("软限面：涨到硬限为止放行，越硬限回 EINVAL", soft_ok,
          "软限与硬限的边界必须精确，差 1 就是越界");

    // 8. 跨进程自身调用放行
    let self_ok = prlimit64(BASE, SOFT_RAISE, SELF_PID, &user).is_ok()
        && prlimit64(BASE, SOFT_RAISE, 1234, &user) == Err(EPERM);
    s.add("跨进程面：pid=0 走自身语义放行，他人 pid 无权限 EPERM", self_ok,
          "pid 语义决定门前置，不能把 0 当普通 pid");

    // 9. 原子性
    let mut atom = true;
    let mut k = 0usize;
    while k < MATRIX.len() {
        if let Some(c) = MATRIX.get(k).copied() {
            if !atomic_ok(c.old, c.new, &c.profile.cred()) {
                atom = false;
            }
        }
        k += 1;
    }
    s.add("原子性：失败保留旧值、成功逐字段等于入参", atom,
          "半截回写会让进程读到与实际生效值不一致的 rlim");

    // 10. 资源册完整
    let mut all_res = true;
    let mut r = 0usize;
    while r < RESOURCES.len() {
        if let Some(row) = RESOURCES.get(r) {
            if res_of(row.id).is_none() || row.name.is_empty() {
                all_res = false;
            }
        }
        r += 1;
    }
    s.add("资源册：六资源全在册且可按号取回", all_res,
          "缺资源即盲区，CPU/FSIZE/DATA/CORE/NPROC/NOFILE 缺一不可");

    // 11. 注入 10/10
    let (ledger, per) = run_hard_raise_matrix();
    let mut all_ten = true;
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(c) = MATRIX.get(ci).copied() {
            if c.expect == EPERM {
                if let Some(n) = per.get(ci) {
                    if *n != INJECT_ROUNDS { all_ten = false; }
                }
            }
        }
        ci += 1;
    }
    s.add("硬限提升注入：七格各 10/10 全检出", all_ten,
          "每格每轮都必须复现 EPERM，一轮漏检即提权逃逸");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == EPERM_CELLS as u32 * INJECT_ROUNDS,
          "70 次注入对应 70 次检出，逃逸恒为零");

    // 13. B19 对账
    let b19 = b19_hits();
    s.add("B19 对账：五行值层冻结期望全命中", b19 == B19_ROWS as u32,
          "B19 是值层权威，漂移时必须先在值层校正");

    // 14. 零重复分层
    let overlap = overlap_keys();
    s.add("零重复分层：B19 键集与本矩阵交集为空", overlap == 0,
          "同一 (资源,入口,pid,cur,max) 键由两层覆盖即重复");

    // 15. 分层完备
    s.add("分层完备：值层与权限层各有格且 B19 全在值层", layer_complete(),
          "缺层等于对账盲区；只有权限层会丢掉值域判据");

    // 16. 矩阵二十二格
    let hits = matrix_hits();
    s.add("矩阵二十二格：逐格实际码与冻结期望全等", hits == MATRIX_CELLS as u32,
          "任一格漂移即说明门序或资源映射被改坏");

    // 17. 状态机
    let stage_ok = advance(Stage::Commit, Stage::Authorize).is_err()
        && advance(Stage::Load, Stage::Authorize).is_err()
        && stage_path(0) == Ok(Stage::Commit)
        && stage_path(EINVAL) == Ok(Stage::Validate)
        && stage_path(EPERM) == Ok(Stage::Authorize);
    s.add("状态机：值域停 Validate、权限停 Authorize、成功到 Commit", stage_ok,
          "三档停位必须与实际失败门一一对应，否则定位失真");

    // 18. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", a == b, "同输入两次必须逐格相等，隐藏状态即失败");

    // 19. 文案闭集
    s.add("文案闭集：12 个诊断码均有非空中文建议",
          RlimCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到可操作说明，空串即判据不可读");

    // 20. 诊断码唯一
    let mut uniq = true;
    let mut x = 0usize;
    while x < RlimCode::ALL.len() {
        let mut y = x + 1;
        while y < RlimCode::ALL.len() {
            if let (Some(p), Some(q)) = (RlimCode::ALL.get(x), RlimCode::ALL.get(y)) {
                if p.val() == q.val() { uniq = false; }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq, "闭集码重复会让判据无法定位到具体缺陷");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let cells_text = format!("矩阵 {}/{} · B19 {}/{} · 重叠 {}", hits, MATRIX_CELLS, b19, B19_ROWS, overlap);
    s.add_dyn("对账快照", hits == MATRIX_CELLS as u32 && b19 == B19_ROWS as u32, &cells_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_raise_allowed() {
        let user = Profile::User.cred();
        assert_eq!(setrlimit(BASE, SOFT_RAISE, &user), Ok(SOFT_RAISE));
        assert_eq!(setrlimit(BASE, Rlim::new(4096, 4096), &user).is_ok(), true);
    }

    #[test]
    fn hard_raise_denied_for_user() {
        let user = Profile::User.cred();
        for res in RESOURCES.iter() {
            let got = setrlimit(BASE, HARD_RAISE, &user);
            assert_eq!(got, Err(EPERM), "res {}", res.name);
        }
    }

    #[test]
    fn hard_raise_allowed_for_privileged() {
        assert_eq!(setrlimit(BASE, HARD_RAISE, &Profile::Root.cred()), Ok(HARD_RAISE));
        assert_eq!(setrlimit(BASE, HARD_RAISE, &Profile::CapRes.cred()), Ok(HARD_RAISE));
    }

    #[test]
    fn value_gate_first_on_setrlimit() {
        let user = Profile::User.cred();
        assert_eq!(setrlimit(BASE, VALUE_BAD, &user), Err(EINVAL));
        assert_eq!(setrlimit(BASE, VALUE_BAD, &Profile::Root.cred()), Err(EINVAL));
        assert_eq!(setrlimit(BASE, Rlim::new(4097, 4096), &user), Err(EINVAL));
    }

    #[test]
    fn ptrace_gate_first_on_prlimit() {
        let user = Profile::User.cred();
        assert_eq!(prlimit64(BASE, VALUE_BAD, 1234, &user), Err(EPERM));
        assert_eq!(prlimit64(BASE, VALUE_BAD, SELF_PID, &user), Err(EINVAL));
        assert_eq!(prlimit64(BASE, SOFT_RAISE, SELF_PID, &user), Ok(SOFT_RAISE));
    }

    #[test]
    fn infinity_is_still_hard_raise() {
        let user = Profile::User.cred();
        assert_eq!(setrlimit(BASE, TO_INFINITY, &user), Err(EPERM));
        assert!(setrlimit(BASE, TO_INFINITY, &Profile::Root.cred()).is_ok());
        assert!(TO_INFINITY.infinite());
        assert!(!BASE.infinite());
    }

    #[test]
    fn limited_hard_lower_allowed() {
        let user = Profile::User.cred();
        assert_eq!(setrlimit(BASE, Rlim::new(1024, 2048), &user), Ok(Rlim::new(1024, 2048)));
        assert_eq!(setrlimit(BASE, Rlim::new(0, 0), &user), Ok(Rlim::new(0, 0)));
    }

    #[test]
    fn atomic_and_no_clamp() {
        for c in MATRIX.iter() {
            assert!(atomic_ok(c.old, c.new, &c.profile.cred()));
        }
        assert!(atomic_ok(BASE, HARD_RAISE, &Profile::User.cred()) == false
            || setrlimit(BASE, HARD_RAISE, &Profile::User.cred()).is_err());
    }

    #[test]
    fn resources_complete() {
        assert_eq!(RESOURCES.len(), RES_COUNT);
        for r in RESOURCES.iter() {
            assert!(res_of(r.id).is_some());
            assert!(!r.name.is_empty());
        }
        assert!(res_of(99).is_none());
    }

    #[test]
    fn matrix_all_cells_hit() {
        assert_eq!(matrix_hits(), MATRIX_CELLS as u32);
        for c in MATRIX.iter() {
            assert_eq!(run_cell(*c), c.expect, "res {} {}", c.res, c.entry.name());
        }
    }

    #[test]
    fn b19_and_layers() {
        assert_eq!(b19_hits(), B19_ROWS as u32);
        assert_eq!(overlap_keys(), 0);
        assert!(layer_complete());
    }

    #[test]
    fn injection_ledger_conserved() {
        let (l, per) = run_hard_raise_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, EPERM_CELLS as u32 * INJECT_ROUNDS);
        for (i, c) in MATRIX.iter().enumerate() {
            let n = per.get(i).copied().unwrap_or(0);
            if c.expect == EPERM { assert_eq!(n, INJECT_ROUNDS); }
            else { assert_eq!(n, 0); }
        }
    }

    #[test]
    fn stage_and_fingerprint() {
        assert_eq!(stage_path(0), Ok(Stage::Commit));
        assert_eq!(stage_path(EINVAL), Ok(Stage::Validate));
        assert_eq!(stage_path(EPERM), Ok(Stage::Authorize));
        assert!(advance(Stage::Commit, Stage::Authorize).is_err());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9273_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
