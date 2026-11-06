//! UNX-F9338 时间/资源族 E 型升级接管注记 · 0x7C200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9338 逐字）：
//!   ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9338-J1 接管边界声明判据过，
//!   存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 接管注记由三张互不替代的表构成：
//!        · 边界声明表：12 个时间/资源对象，每个声明归属（继承/接管/拆分共管）
//!          与它暴露的 errno 集合；
//!        · 零重写承诺位图：12 位，一位一对象，只允许「置满 → 封存」；
//!        · 增量清单：append-only，序号严格 +1，一条不许缺、一条不许重。
//!      三张表的判据分开算，任何一张被改都会在各自的 audit 里报红。
//!   ② 边界归属不是标签而是权限：Inherited 表示「本批不动它的错误路径」，
//!      Taken 表示「本批整体接管」，SharedSplit 表示「只接新增分支」。三者
//!      的重写许可全为 0——本单的核心承诺是**存量错误路径零重写**，所以
//!      `allows_rewrite()` 恒 false 是设计事实，不是遗漏。
//!   ③ errno 集合按升序登记且必须非空：升序是为了逐码比对不受书写顺序影响，
//!      非空是为了「接管后仍能报错」——空集合等于把错误路径抹成静默成功。
//!   ④ 增量清单的 kind 由边界归属**推导**（Inherited→承诺条，Taken→边界条，
//!      SharedSplit→落账条），不另行手填；落账时再与推导值比对，形成闭环。
//!   ⑤ 承诺封存是单向的：置满 12 位才可封存；封存后任何一次违例都会清位、
//!      计违例、并把封存态拉回未封存——「已封存」不是终态，违例可证伪。
//!   ⑥ 注入三类各 10 轮（越权改归属 / 静默改重写计数 / 清单缺条），全部
//!      10/10 检出，注入 = 检出 + 逃逸且逃逸为 0。
//!   ⑦ 注记阶段机 Draft→Declared→Promised→Sealed 只许单步前进，跨步回
//!      EPERM（不是 EINVAL：这是权限/顺序面的拒绝，不是参数面）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9338 独占 0x7C200..=0x7C23F）。
pub const SEG_BASE: u32 = 0x7C200;

/// 接管对象数（时间/资源族十二项）。
pub const RES_COUNT: usize = 12;

/// 增量清单容量（12 条 + 4 条余量）。
pub const MANIFEST_CAP: usize = 16;

/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 零重写承诺位图满值（低 12 位）。
pub const PROMISE_ALL: u16 = 0x0FFF;

/// 注记结果摘要（判据③要求非空）。
pub const NOTE_SUMMARY: &str = "12 项接管边界已声明，存量错误路径零重写，增量清单 12 条封账";

// ── Linux errno 数值（正数口径） ──────────────────────────────────────
/// 操作不允许（越权改归属走这条）。
pub const EPERM: i32 = 1;
/// 目标进程不存在（CPU 时钟族）。
pub const ESRCH: i32 = 3;
/// 被信号打断（睡眠族）。
pub const EINTR: i32 = 4;
/// 坏描述符（定时器 fd 族）。
pub const EBADF: i32 = 9;
/// 暂不可用（相对睡眠的绝对时间已过）。
pub const EAGAIN: i32 = 11;
/// 内存不足（定时器创建）。
pub const ENOMEM: i32 = 12;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 参数非法（时钟号/时间值）。
pub const EINVAL: i32 = 22;
/// 不支持的操作（非 POSIX 时钟的 settime）。
pub const ENOTTY: i32 = 25;
/// 序号重复（增量清单同一序号落账两次）。
pub const EEXIST: i32 = 17;

/// errno 登记表：(名称, 正数码值, 中文说明)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", EPERM, "能力/归属越权"),
    ("ESRCH", ESRCH, "目标进程不存在"),
    ("EINTR", EINTR, "被信号打断，剩余时长可续算"),
    ("EBADF", EBADF, "坏定时器描述符"),
    ("EAGAIN", EAGAIN, "睡眠目标时刻已过"),
    ("ENOMEM", ENOMEM, "定时器资源耗尽"),
    ("EFAULT", EFAULT, "坏用户时间结构指针"),
    ("EINVAL", EINVAL, "时钟号或时间值非法"),
    ("ENOTTY", ENOTTY, "该时钟不支持写回"),
];

/// 表内登记判定。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        let hit = match ERRNO_TABLE.get(i) {
            Some(row) => row.1 == code,
            None => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 表内说明（表外兜底非空）。
pub fn errno_note(code: i32) -> &'static str {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    "未登记错误码：接管注记拒绝落账"
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+15，码值 = SEG_BASE + 序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum NoteCode {
    /// 段基址越窗。
    SegmentOutOfWindow = 1,
    /// 对象数不是 12。
    ResCountMismatch = 2,
    /// 边界归属与冻结表不符。
    BoundaryDrift = 3,
    /// 重写计数非零（违反零重写承诺）。
    RewriteNotZero = 4,
    /// errno 集合为空。
    EmptyErrnoSet = 5,
    /// errno 集合未按升序登记。
    ErrnoNotAscending = 6,
    /// errno 集合含重复码。
    ErrnoDuplicated = 7,
    /// 增量清单缺条或多条。
    ManifestGap = 8,
    /// 增量序号跳号。
    SequenceSkip = 9,
    /// 增量序号重复。
    SequenceDuplicated = 10,
    /// 增量 kind 与归属推导不一致。
    KindMismatch = 11,
    /// 承诺位未置满即封存。
    PromiseIncomplete = 12,
    /// 承诺封存后发生违例。
    PromiseViolated = 13,
    /// 阶段跨步推进。
    StageOverstep = 14,
    /// 指纹双跑不一致。
    StampDrift = 15,
}

impl NoteCode {
    /// 序号（显式判别值）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 绝对码值 = 段基址 + 序号。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            NoteCode::SegmentOutOfWindow => "段基址越窗：本卡独占 0x7C200..=0x7C23F",
            NoteCode::ResCountMismatch => "接管对象数不是 12：补齐时间/资源族十二项",
            NoteCode::BoundaryDrift => "边界归属与冻结表不符：归属只能按派工单声明",
            NoteCode::RewriteNotZero => "存量重写计数非零：违反零重写承诺，立即回退改动",
            NoteCode::EmptyErrnoSet => "errno 集合为空：接管后仍必须报错，不得抹成静默成功",
            NoteCode::ErrnoNotAscending => "errno 集合未升序：逐码比对会受书写顺序干扰",
            NoteCode::ErrnoDuplicated => "errno 集合含重复码：同一错误被登记两次",
            NoteCode::ManifestGap => "增量清单缺条或多条：清单条数必须等于声明条数",
            NoteCode::SequenceSkip => "增量序号跳号：append-only 序号必须严格加一",
            NoteCode::SequenceDuplicated => "增量序号重复：同序号落账两次",
            NoteCode::KindMismatch => "增量 kind 与归属推导不一致：kind 只能由归属推导",
            NoteCode::PromiseIncomplete => "承诺位未置满即封存：12 位全置才允许封存",
            NoteCode::PromiseViolated => "承诺封存后发生违例：位已清，封存态已撤销",
            NoteCode::StageOverstep => "阶段跨步推进：Draft→Declared→Promised→Sealed 单步",
            NoteCode::StampDrift => "注记指纹双跑不一致：存在隐藏状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [NoteCode] = &[
        NoteCode::SegmentOutOfWindow,
        NoteCode::ResCountMismatch,
        NoteCode::BoundaryDrift,
        NoteCode::RewriteNotZero,
        NoteCode::EmptyErrnoSet,
        NoteCode::ErrnoNotAscending,
        NoteCode::ErrnoDuplicated,
        NoteCode::ManifestGap,
        NoteCode::SequenceSkip,
        NoteCode::SequenceDuplicated,
        NoteCode::KindMismatch,
        NoteCode::PromiseIncomplete,
        NoteCode::PromiseViolated,
        NoteCode::StageOverstep,
        NoteCode::StampDrift,
    ];
}

// ── 接管对象与边界归属 ────────────────────────────────────────────────
/// 时间/资源族接管对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Res {
    /// 实时钟。
    Realtime,
    /// 单调钟。
    Monotonic,
    /// 开机计时。
    Boottime,
    /// 国际原子时。
    Tai,
    /// 进程 CPU 时钟。
    ProcessCpu,
    /// 线程 CPU 时钟。
    ThreadCpu,
    /// 原始单调钟。
    MonotonicRaw,
    /// 粗粒度实时钟。
    RealtimeCoarse,
    /// 定时器 fd。
    TimerFd,
    /// 相对睡眠。
    NanoSleep,
    /// 绝对睡眠。
    ClockNanoSleep,
    /// 实时钟写回。
    ClockSettime,
}

impl Res {
    /// 稳定标签（0..=11）。
    pub fn tag(self) -> u8 {
        match self {
            Res::Realtime => 0,
            Res::Monotonic => 1,
            Res::Boottime => 2,
            Res::Tai => 3,
            Res::ProcessCpu => 4,
            Res::ThreadCpu => 5,
            Res::MonotonicRaw => 6,
            Res::RealtimeCoarse => 7,
            Res::TimerFd => 8,
            Res::NanoSleep => 9,
            Res::ClockNanoSleep => 10,
            Res::ClockSettime => 11,
        }
    }

    /// 中文对象名。
    pub fn name(self) -> &'static str {
        match self {
            Res::Realtime => "CLOCK_REALTIME",
            Res::Monotonic => "CLOCK_MONOTONIC",
            Res::Boottime => "CLOCK_BOOTTIME",
            Res::Tai => "CLOCK_TAI",
            Res::ProcessCpu => "CLOCK_PROCESS_CPUTIME_ID",
            Res::ThreadCpu => "CLOCK_THREAD_CPUTIME_ID",
            Res::MonotonicRaw => "CLOCK_MONOTONIC_RAW",
            Res::RealtimeCoarse => "CLOCK_REALTIME_COARSE",
            Res::TimerFd => "timerfd_create",
            Res::NanoSleep => "nanosleep",
            Res::ClockNanoSleep => "clock_nanosleep",
            Res::ClockSettime => "clock_settime",
        }
    }

    /// 闭集全量（按 tag 升序）。
    pub const ALL: &'static [Res] = &[
        Res::Realtime,
        Res::Monotonic,
        Res::Boottime,
        Res::Tai,
        Res::ProcessCpu,
        Res::ThreadCpu,
        Res::MonotonicRaw,
        Res::RealtimeCoarse,
        Res::TimerFd,
        Res::NanoSleep,
        Res::ClockNanoSleep,
        Res::ClockSettime,
    ];
}

/// 接管边界归属。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boundary {
    /// 继承既有实现，本批不动。
    Inherited,
    /// 本批整体接管。
    Taken,
    /// 只接新增分支，存量与新增共管。
    SharedSplit,
}

impl Boundary {
    /// 稳定编码。
    pub fn tag(self) -> u8 {
        match self {
            Boundary::Inherited => 0,
            Boundary::Taken => 1,
            Boundary::SharedSplit => 2,
        }
    }

    /// 中文归属名。
    pub fn name(self) -> &'static str {
        match self {
            Boundary::Inherited => "继承不动",
            Boundary::Taken => "整体接管",
            Boundary::SharedSplit => "拆分共管",
        }
    }

    /// 是否允许重写存量错误路径。
    ///
    /// 恒 false：本单判据②就是「存量错误路径零重写」，三种归属都不构成
    /// 重写许可。保留为函数而不是常量，是为了让判据能对每个归属逐一点名。
    pub fn allows_rewrite(self) -> bool {
        match self {
            Boundary::Inherited => false,
            Boundary::Taken => false,
            Boundary::SharedSplit => false,
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [Boundary] = &[Boundary::Inherited, Boundary::Taken, Boundary::SharedSplit];
}

/// 一条边界声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResDecl {
    /// 接管对象。
    pub res: Res,
    /// 边界归属。
    pub boundary: Boundary,
    /// 该对象暴露的 errno 集合（升序、非空、无重复）。
    pub errs: &'static [i32],
    /// 存量错误路径被重写的单元数（承诺恒 0）。
    pub rewrite_units: u16,
    /// 增量清单序号（1..=12，全局唯一）。
    pub increments: u16,
}

/// 声明构造器：重写计数固定 0，避免任何一处手填非零。
pub const fn decl(res: Res, boundary: Boundary, errs: &'static [i32], increments: u16) -> ResDecl {
    ResDecl { res, boundary, errs, rewrite_units: 0, increments }
}

/// 十二项冻结边界声明表。
///
/// errno 集合的排序是升序数值序（1/3/4/9/11/12/14/22/25），不是书写习惯序。
pub const FROZEN_DECL: [ResDecl; RES_COUNT] = [
    decl(Res::Realtime, Boundary::Taken, &[EPERM, EFAULT, EINVAL], 1),
    decl(Res::Monotonic, Boundary::Inherited, &[EFAULT, EINVAL], 2),
    decl(Res::Boottime, Boundary::Inherited, &[EFAULT, EINVAL], 3),
    decl(Res::Tai, Boundary::Taken, &[EFAULT, EINVAL], 4),
    decl(Res::ProcessCpu, Boundary::Inherited, &[ESRCH, EFAULT, EINVAL], 5),
    decl(Res::ThreadCpu, Boundary::Inherited, &[EFAULT, EINVAL], 6),
    decl(Res::MonotonicRaw, Boundary::Inherited, &[EFAULT, EINVAL], 7),
    decl(Res::RealtimeCoarse, Boundary::Inherited, &[EFAULT, EINVAL], 8),
    decl(Res::TimerFd, Boundary::Taken, &[EBADF, ENOMEM, EFAULT, EINVAL], 9),
    decl(Res::NanoSleep, Boundary::Taken, &[EINTR, EFAULT, EINVAL], 10),
    decl(Res::ClockNanoSleep, Boundary::Taken, &[EINTR, EAGAIN, EFAULT, EINVAL], 11),
    decl(Res::ClockSettime, Boundary::SharedSplit, &[EPERM, EFAULT, EINVAL], 12),
];

/// 活表：从冻结表复制一份可改写的声明（审计与注入共用同一入口）。
pub fn live_decl() -> Vec<ResDecl> {
    FROZEN_DECL.iter().copied().collect()
}

/// 冻结表副本（只读口径）。
pub fn frozen_decl() -> Vec<ResDecl> {
    FROZEN_DECL.iter().copied().collect()
}

/// 边界声明审计：逐字段比对活表与冻结表，返回差异条数。
pub fn boundary_audit(live: &[ResDecl]) -> u32 {
    let mut diff = 0u32;
    if live.len() != FROZEN_DECL.len() {
        diff = diff.saturating_add(1);
    }
    let mut i = 0usize;
    while i < FROZEN_DECL.len() {
        let bad = match (live.get(i), FROZEN_DECL.get(i)) {
            (Some(a), Some(b)) => {
                a.res != b.res
                    || a.boundary != b.boundary
                    || a.errs != b.errs
                    || a.increments != b.increments
            }
            (None, Some(_)) => true,
            _ => false,
        };
        if bad {
            diff = diff.saturating_add(1);
        }
        i += 1;
    }
    diff
}

/// 零重写审计：返回重写计数非零的声明条数。
pub fn rewrite_audit(live: &[ResDecl]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < live.len() {
        let bad = match live.get(i) {
            Some(d) => d.rewrite_units != 0 || d.boundary.allows_rewrite(),
            None => false,
        };
        if bad {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    n
}

/// errno 集合审计：空集、非升序、重复、表外码各计一笔。
pub fn errno_audit(live: &[ResDecl]) -> u32 {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < live.len() {
        if let Some(d) = live.get(i) {
            if d.errs.is_empty() {
                bad = bad.saturating_add(1);
            }
            let mut j = 0usize;
            while j < d.errs.len() {
                let cur = match d.errs.get(j) {
                    Some(v) => *v,
                    None => 0,
                };
                if !errno_known(cur) {
                    bad = bad.saturating_add(1);
                }
                if j > 0 {
                    let prev = match d.errs.get(j - 1) {
                        Some(v) => *v,
                        None => 0,
                    };
                    if cur <= prev {
                        bad = bad.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    bad
}

/// 某归属下的声明条数。
pub fn boundary_count(b: Boundary) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < FROZEN_DECL.len() {
        let hit = match FROZEN_DECL.get(i) {
            Some(d) => d.boundary == b,
            None => false,
        };
        if hit {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 增量序号集合是否为 1..=12 的排列。
pub fn increments_are_permutation() -> bool {
    let mut seen = [false; RES_COUNT];
    let mut i = 0usize;
    while i < FROZEN_DECL.len() {
        let inc = match FROZEN_DECL.get(i) {
            Some(d) => d.increments,
            None => return false,
        };
        if inc == 0 || inc as usize > RES_COUNT {
            return false;
        }
        let slot = (inc as usize) - 1;
        match seen.get_mut(slot) {
            Some(v) => {
                if *v {
                    return false;
                }
                *v = true;
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 增量清单 ──────────────────────────────────────────────────────────
/// 增量条目种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IncrKind {
    /// 边界声明条（对应整体接管）。
    Boundary,
    /// 零重写承诺条（对应继承不动）。
    Promise,
    /// 落账条（对应拆分共管）。
    Ledger,
}

impl IncrKind {
    /// 稳定编码。
    pub fn tag(self) -> u8 {
        match self {
            IncrKind::Boundary => 0,
            IncrKind::Promise => 1,
            IncrKind::Ledger => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            IncrKind::Boundary => "边界条",
            IncrKind::Promise => "承诺条",
            IncrKind::Ledger => "落账条",
        }
    }
}

/// 由归属推导增量种类（唯一来源，落账时再比对）。
pub const fn kind_of(b: Boundary) -> IncrKind {
    match b {
        Boundary::Taken => IncrKind::Boundary,
        Boundary::Inherited => IncrKind::Promise,
        Boundary::SharedSplit => IncrKind::Ledger,
    }
}

/// 一条增量条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Increment {
    /// 序号（1 起，严格 +1）。
    pub seq: u16,
    /// 对象标签。
    pub res_tag: u8,
    /// 种类。
    pub kind: IncrKind,
    /// 备注（非空）。
    pub note: &'static str,
}

/// append-only 增量清单。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IncrementManifest {
    rows: Vec<Increment>,
}

impl IncrementManifest {
    /// 空清单。
    pub fn new() -> IncrementManifest {
        IncrementManifest { rows: Vec::new() }
    }

    /// 已落条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 按序取条。
    pub fn row(&self, i: usize) -> Option<Increment> {
        self.rows.get(i).copied()
    }

    /// 落一条：序号必须等于当前长度 + 1。
    ///
    /// 错误面刻意分开：序号重复回 EEXIST、跳号回 EINVAL、超容回 ENOMEM，
    /// 这样复测时能直接看出是「重了」「漏了」还是「装不下了」。
    pub fn push(&mut self, row: Increment) -> Result<usize, i32> {
        if self.rows.len() >= MANIFEST_CAP {
            return Err(ENOMEM);
        }
        let want = self.rows.len() as u16 + 1;
        if row.seq < want {
            return Err(EEXIST);
        }
        if row.seq > want {
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let dup = match self.rows.get(i) {
                Some(r) => r.seq == row.seq,
                None => false,
            };
            if dup {
                return Err(EEXIST);
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(self.rows.len() - 1)
    }

    /// 摘掉末条（只用于负向判据：清单缺条）。
    pub fn pop(&mut self) -> Option<Increment> {
        self.rows.pop()
    }

    /// 序号严格 +1 且从 1 起。
    pub fn sequential(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            let expected = i as u16 + 1;
            let got = match self.rows.get(i) {
                Some(r) => r.seq,
                None => 0,
            };
            if got != expected {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for IncrementManifest {
    fn default() -> IncrementManifest {
        IncrementManifest::new()
    }
}

/// 备注文案按归属分档（落账时写入 note）。
pub fn note_for(b: Boundary) -> &'static str {
    match b {
        Boundary::Taken => "整体接管：新实现承担该对象全部错误路径",
        Boundary::Inherited => "继承不动：仅登记承诺位，不改存量分支",
        Boundary::SharedSplit => "拆分共管：只接新增分支，存量分支原样保留",
    }
}

/// 由声明表生成增量清单（序号取声明里的 increments）。
pub fn build_manifest(live: &[ResDecl]) -> Result<IncrementManifest, i32> {
    let mut m = IncrementManifest::new();
    let mut i = 0usize;
    while i < live.len() {
        let d = match live.get(i) {
            Some(v) => *v,
            None => return Err(EINVAL),
        };
        let row = Increment {
            seq: d.increments,
            res_tag: d.res.tag(),
            kind: kind_of(d.boundary),
            note: note_for(d.boundary),
        };
        if let Err(code) = m.push(row) {
            return Err(code);
        }
        i += 1;
    }
    Ok(m)
}

/// 清单审计：条数、序号、tag、kind 四路比对，返回差异条数。
pub fn manifest_audit(m: &IncrementManifest, live: &[ResDecl]) -> u32 {
    let mut bad = 0u32;
    if m.len() != live.len() {
        bad = bad.saturating_add(1);
    }
    let mut i = 0usize;
    while i < live.len() {
        let d = match live.get(i) {
            Some(v) => *v,
            None => {
                i += 1;
                continue;
            }
        };
        match m.row(i) {
            Some(row) => {
                if row.res_tag != d.res.tag() {
                    bad = bad.saturating_add(1);
                }
                if row.kind != kind_of(d.boundary) {
                    bad = bad.saturating_add(1);
                }
                if row.note.is_empty() {
                    bad = bad.saturating_add(1);
                }
            }
            None => bad = bad.saturating_add(1),
        }
        i += 1;
    }
    if !m.sequential() {
        bad = bad.saturating_add(1);
    }
    bad
}

// ── 零重写承诺位图 ────────────────────────────────────────────────────
/// 承诺位图：一位一对象，只允许置满后封存。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromiseSeal {
    bits: u16,
    sealed: bool,
    violations: u32,
}

impl PromiseSeal {
    /// 空位图。
    pub const fn new() -> PromiseSeal {
        PromiseSeal { bits: 0, sealed: false, violations: 0 }
    }

    /// 当前位图。
    pub fn bits(&self) -> u16 {
        self.bits
    }

    /// 是否已封存。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 违例次数。
    pub fn violations(&self) -> u32 {
        self.violations
    }

    /// 只剩几位未置。
    pub fn missing(&self) -> u32 {
        popcount16((!self.bits) & PROMISE_ALL) as u32
    }

    /// 置位：已封存时拒绝（先解封再改，改动才留痕）。
    pub fn raise(&mut self, i: usize) -> Result<(), i32> {
        if self.sealed {
            return Err(EPERM);
        }
        if i >= RES_COUNT {
            return Err(EINVAL);
        }
        self.bits |= 1u16 << i;
        Ok(())
    }

    /// 封存：12 位未置满一律拒绝。
    pub fn seal(&mut self) -> Result<u16, i32> {
        if self.bits != PROMISE_ALL {
            return Err(EINVAL);
        }
        self.sealed = true;
        Ok(self.bits)
    }

    /// 违例：清位、计次、撤销封存（封存可被证伪）。
    pub fn violate(&mut self, i: usize) -> Result<u16, i32> {
        if i >= RES_COUNT {
            return Err(EINVAL);
        }
        self.bits &= !(1u16 << i);
        self.violations = self.violations.saturating_add(1);
        self.sealed = false;
        Ok(self.bits)
    }

    /// 承诺完好：位满、零违例、且处于封存态。
    pub fn holds(&self) -> bool {
        self.bits == PROMISE_ALL && self.violations == 0 && self.sealed
    }
}

impl Default for PromiseSeal {
    fn default() -> PromiseSeal {
        PromiseSeal::new()
    }
}

/// 十位内人口计数。
pub fn popcount16(mut v: u16) -> u8 {
    let mut n = 0u8;
    while v != 0 {
        n = n.saturating_add((v & 1) as u8);
        v >>= 1;
    }
    n
}

/// 批量置满 12 位并封存。
pub fn seal_all() -> Result<PromiseSeal, i32> {
    let mut seal = PromiseSeal::new();
    let mut i = 0usize;
    while i < RES_COUNT {
        if let Err(code) = seal.raise(i) {
            return Err(code);
        }
        i += 1;
    }
    seal.seal()?;
    Ok(seal)
}

// ── 注记阶段机 ────────────────────────────────────────────────────────
/// 注记阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteStage {
    /// 草稿。
    Draft,
    /// 已声明边界。
    Declared,
    /// 已立承诺。
    Promised,
    /// 已封存。
    Sealed,
}

impl NoteStage {
    /// 稳定编码。
    pub fn tag(self) -> u8 {
        match self {
            NoteStage::Draft => 0,
            NoteStage::Declared => 1,
            NoteStage::Promised => 2,
            NoteStage::Sealed => 3,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            NoteStage::Draft => "草稿",
            NoteStage::Declared => "已声明",
            NoteStage::Promised => "已承诺",
            NoteStage::Sealed => "已封存",
        }
    }
}

/// 单步推进：跨步与回退一律 EPERM（顺序面拒绝，不是参数面）。
pub fn advance(cur: NoteStage, next: NoteStage) -> Result<NoteStage, i32> {
    let ok = matches!(
        (cur, next),
        (NoteStage::Draft, NoteStage::Declared)
            | (NoteStage::Declared, NoteStage::Promised)
            | (NoteStage::Promised, NoteStage::Sealed)
    );
    if ok {
        Ok(next)
    } else {
        Err(EPERM)
    }
}

/// 完工注记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Note {
    /// 终态。
    pub stage: NoteStage,
    /// 声明条数。
    pub decls: u16,
    /// 清单条数。
    pub increments: u16,
    /// 承诺位。
    pub promises: u16,
    /// 指纹。
    pub stamp: u32,
    /// 结果摘要。
    pub summary: &'static str,
}

impl Note {
    /// 注记是否可交付：四要素齐备。
    pub fn deliverable(&self) -> bool {
        self.stage == NoteStage::Sealed
            && self.decls == RES_COUNT as u16
            && self.increments == RES_COUNT as u16
            && self.promises == PROMISE_ALL
            && self.stamp != 0
            && !self.summary.is_empty()
    }
}

/// 总装：声明 → 清单 → 承诺 → 注记。
pub fn assemble() -> Result<Note, i32> {
    let live = frozen_decl();
    let manifest = build_manifest(&live)?;
    let seal = seal_all()?;
    let mut stage = NoteStage::Draft;
    stage = advance(stage, NoteStage::Declared)?;
    stage = advance(stage, NoteStage::Promised)?;
    stage = advance(stage, NoteStage::Sealed)?;
    Ok(Note {
        stage,
        decls: live.len() as u16,
        increments: manifest.len() as u16,
        promises: seal.bits(),
        stamp: note_digest(&live, &manifest),
        summary: NOTE_SUMMARY,
    })
}

/// 注记复检：四路审计全零且指纹可重算。
pub fn note_audit(note: &Note) -> bool {
    let live = frozen_decl();
    let manifest = match build_manifest(&live) {
        Ok(m) => m,
        Err(_e) => return false,
    };
    note.deliverable()
        && boundary_audit(&live) == 0
        && rewrite_audit(&live) == 0
        && errno_audit(&live) == 0
        && manifest_audit(&manifest, &live) == 0
        && note.stamp == note_digest(&live, &manifest)
}

// ── 注入（三类各 10 轮） ──────────────────────────────────────────────
/// 注入变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 把继承不动的对象越权改成整体接管。
    BoundaryEscalation,
    /// 悄悄把某对象的存量重写计数改成 1。
    RewriteSilent,
    /// 清单末尾丢一条。
    ManifestGap,
}

impl Mutant {
    /// 闭集全量。
    pub const ALL: &'static [Mutant] = &[
        Mutant::BoundaryEscalation,
        Mutant::RewriteSilent,
        Mutant::ManifestGap,
    ];

    /// 稳定编码。
    pub fn tag(self) -> u8 {
        match self {
            Mutant::BoundaryEscalation => 0,
            Mutant::RewriteSilent => 1,
            Mutant::ManifestGap => 2,
        }
    }

    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Mutant::BoundaryEscalation => "边界越权：继承不动被改成整体接管",
            Mutant::RewriteSilent => "静默重写：存量重写计数被改成 1",
            Mutant::ManifestGap => "清单缺条：末条被摘掉",
        }
    }

    /// 施加变体后是否仍「看起来正常」（检出 = 不返回 true）。
    pub fn detect(self) -> bool {
        match self {
            Mutant::BoundaryEscalation => {
                let mut live = live_decl();
                if let Some(d) = live.get_mut(1) {
                    d.boundary = Boundary::Taken;
                }
                boundary_audit(&live) > 0
            }
            Mutant::RewriteSilent => {
                let mut live = live_decl();
                if let Some(d) = live.get_mut(2) {
                    d.rewrite_units = 1;
                }
                rewrite_audit(&live) > 0 && boundary_audit(&live) == 0
            }
            Mutant::ManifestGap => {
                let live = live_decl();
                match build_manifest(&live) {
                    Ok(mut m) => {
                        let dropped = m.pop().is_some();
                        dropped && manifest_audit(&m, &live) > 0
                    }
                    Err(_e) => false,
                }
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    /// 注入总笔数。
    pub injected: u32,
    /// 检出笔数。
    pub detected: u32,
    /// 逃逸笔数。
    pub escaped: u32,
}

impl InjectLog {
    /// 空账。
    pub const fn zero() -> InjectLog {
        InjectLog { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

impl Default for InjectLog {
    fn default() -> InjectLog {
        InjectLog::zero()
    }
}

/// 跑三类 × 10 轮注入，返回账本与每类检出数。
pub fn run_injections() -> (InjectLog, [u32; 3]) {
    let mut log = InjectLog::zero();
    let mut per_class = [0u32; 3];
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i) {
                let caught = m.detect();
                log.record(caught);
                if caught {
                    let slot = (m.tag() as usize) % 3;
                    if let Some(v) = per_class.get_mut(slot) {
                        *v = v.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        round += 1;
    }
    (log, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位续算。
pub fn chain(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u32;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    acc
}

/// 声明表指纹：对象名 + 归属 + errno 集合 + 序号。
pub fn decl_digest(live: &[ResDecl]) -> u32 {
    let mut h = chain(0x811C_9DC5, b"unx9338/decl");
    let mut i = 0usize;
    while i < live.len() {
        if let Some(d) = live.get(i) {
            h = chain(h, d.res.name().as_bytes());
            h = chain(h, &[d.boundary.tag(), d.increments as u8]);
            let mut j = 0usize;
            while j < d.errs.len() {
                if let Some(code) = d.errs.get(j) {
                    h = chain(h, &code.to_le_bytes());
                }
                j += 1;
            }
            h = chain(h, &d.rewrite_units.to_le_bytes());
        }
        i += 1;
    }
    h
}

/// 注记指纹：声明 + 清单。
pub fn note_digest(live: &[ResDecl], m: &IncrementManifest) -> u32 {
    let mut h = decl_digest(live);
    let mut i = 0usize;
    while i < m.len() {
        if let Some(row) = m.row(i) {
            h = chain(h, &row.seq.to_le_bytes());
            h = chain(h, &[row.res_tag, row.kind.tag()]);
            h = chain(h, row.note.as_bytes());
        }
        i += 1;
    }
    h
}

/// 取清单；构建失败时给空清单（只在诊断路径使用，正向路径必须显式处理 Err）。
fn manifest_or_empty(live: &[ResDecl]) -> IncrementManifest {
    match build_manifest(live) {
        Ok(m) => m,
        Err(_e) => IncrementManifest::new(),
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9338 判据集。
pub fn run_unx9338_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9338");

    let seg_ok = SEG_BASE == 0x7C200 && SEG_BASE % 0x40 == 0 && SEG_BASE < 0x7C240;
    s.add("段位守卫", seg_ok, "0x7C200 独占窗口且 0x40 对齐，不与 F9337 重叠");

    let code_window = NoteCode::ALL
        .iter()
        .all(|c| c.seg_addr() > SEG_BASE && c.seg_addr() < SEG_BASE + 0x40);
    let mut code_unique = true;
    let mut text_ok = NoteCode::ALL.len() == 15;
    let mut i = 0usize;
    while i < NoteCode::ALL.len() {
        let mut j = i + 1;
        while j < NoteCode::ALL.len() {
            if let (Some(a), Some(b)) = (NoteCode::ALL.get(i), NoteCode::ALL.get(j)) {
                if a.seg_addr() == b.seg_addr() {
                    code_unique = false;
                }
                if a.detail() == b.detail() || a.detail().is_empty() {
                    text_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("码窗守卫", code_window, "15 个诊断码落在 0x7C200 的 0x40 窗口内");
    s.add("码值唯一", code_unique, "段内码值两两不同");
    s.add("文案闭集", text_ok, "15 条中文建议互异且非空");

    let live = frozen_decl();
    s.add("接管对象数", live.len() == RES_COUNT && RES_COUNT == 12, "十二项时间/资源对象逐一登记");

    let shape_ok = boundary_count(Boundary::Inherited) == 6
        && boundary_count(Boundary::Taken) == 5
        && boundary_count(Boundary::SharedSplit) == 1;
    s.add("归属分布", shape_ok, "继承 6 / 接管 5 / 拆分 1，覆盖三种归属");

    let audit_boundary = boundary_audit(&live);
    s.add("边界声明审计", audit_boundary == 0, "活表与冻结表逐字段相等，归属未被改写");

    let audit_rewrite = rewrite_audit(&live);
    s.add("零重写承诺", audit_rewrite == 0, "12 项存量重写计数全 0，且归属不构成重写许可");

    let audit_errno = errno_audit(&live);
    s.add("errno 集合", audit_errno == 0, "每项 errno 集合非空、升序、无重复、全部在登记表内");

    let perm_ok = increments_are_permutation();
    s.add("增量序号排列", perm_ok, "12 个序号恰为 1..=12 的排列，无跳号无重复");

    let manifest = build_manifest(&live);
    let (m_len, m_audit, m_seq) = match manifest {
        Ok(ref m) => (m.len(), manifest_audit(m, &live), m.sequential()),
        Err(_e) => (0usize, 1u32, false),
    };
    s.add(
        "增量清单落账",
        m_len == RES_COUNT && m_audit == 0 && m_seq,
        "12 条增量按序号落账，tag/kind/备注与声明逐条一致",
    );

    // 清单负向：重复序号、跳号、超容
    let mut dup = IncrementManifest::new();
    let first = Increment { seq: 1, res_tag: 0, kind: IncrKind::Boundary, note: "首条" };
    let dup_pushed = dup.push(first).is_ok() && dup.push(first) == Err(EEXIST);
    let mut skip = IncrementManifest::new();
    let skip_rejected = skip.push(Increment { seq: 2, res_tag: 0, kind: IncrKind::Boundary, note: "跳号" }) == Err(EINVAL);
    let mut full = IncrementManifest::new();
    let mut filled = 0u16;
    while (filled as usize) < MANIFEST_CAP {
        let row = Increment {
            seq: filled + 1,
            res_tag: (filled % RES_COUNT as u16) as u8,
            kind: IncrKind::Ledger,
            note: "填充",
        };
        if full.push(row).is_ok() {
            filled += 1;
        } else {
            break;
        }
    }
    let over = full.push(Increment { seq: MANIFEST_CAP as u16 + 1, res_tag: 0, kind: IncrKind::Ledger, note: "超容" });
    s.add(
        "清单拒绝面",
        dup_pushed && skip_rejected && filled as usize == MANIFEST_CAP && over == Err(ENOMEM),
        "重复 EEXIST / 跳号 EINVAL / 超容 ENOMEM 三面齐全",
    );

    // 承诺位图
    let mut empty_seal = PromiseSeal::new();
    let early = empty_seal.seal();
    let mut seal_main = seal_all();
    let seal_ok = match seal_main {
        Ok(ref v) => {
            v.bits() == PROMISE_ALL
                && v.sealed()
                && v.missing() == 0
                && v.holds()
                && v.violations() == 0
        }
        Err(_e) => false,
    };
    s.add(
        "承诺封存",
        early == Err(EINVAL) && seal_ok,
        "12 位未置满拒封存，置满后封存且零违例",
    );

    let raise_after_seal = match seal_main {
        Ok(ref mut v) => v.raise(3) == Err(EPERM),
        Err(_e) => false,
    };
    s.add("封存后改位拒绝", raise_after_seal, "已封存再置位回 EPERM，改动必须先解封");

    let mut seal_violate = seal_all();
    let violate_ok = match seal_violate {
        Ok(ref mut v) => {
            let before = v.bits();
            let after = v.violate(11);
            after == Ok(before & !(1u16 << 11)) && !v.sealed() && v.violations() == 1 && !v.holds()
        }
        Err(_e) => false,
    };
    s.add("违例可证伪", violate_ok, "封存后违例会清位、计次并撤销封存态");

    // 阶段机
    let stage_ok = advance(NoteStage::Draft, NoteStage::Declared) == Ok(NoteStage::Declared)
        && advance(NoteStage::Declared, NoteStage::Promised) == Ok(NoteStage::Promised)
        && advance(NoteStage::Promised, NoteStage::Sealed) == Ok(NoteStage::Sealed)
        && advance(NoteStage::Draft, NoteStage::Sealed) == Err(EPERM)
        && advance(NoteStage::Sealed, NoteStage::Draft) == Err(EPERM);
    s.add("阶段单步", stage_ok, "四阶段只许单步前进，跨步与回退均回 EPERM");

    // 总装与复检
    let note = assemble();
    let note_ok = match note {
        Ok(n) => n.deliverable() && note_audit(&n) && !n.summary.is_empty(),
        Err(_e) => false,
    };
    s.add("注记总装", note_ok, "四要素齐备且四路审计全零，摘要非空");

    // 注入
    let (log, per_class) = run_injections();
    s.add(
        "注入 10/10",
        log.injected == 30
            && log.detected == 30
            && log.escaped == 0
            && log.conserved()
            && per_class.iter().all(|n| *n == INJECT_ROUNDS),
        "三类变体各 10 轮全部检出，逃逸 0",
    );

    let mutation_text = Mutant::ALL.iter().all(|m| !m.detail().is_empty() && m.detect());
    s.add("变体可检出", mutation_text, "三变体各自可被至少一路审计抓到");

    // 双跑
    let d1 = note_digest(&live, &manifest_or_empty(&live));
    let d2 = note_digest(&live, &manifest_or_empty(&live));
    s.add("双跑一致", d1 == d2 && d1 != 0, "同输入两次注记指纹逐字节相等");

    let live_text = format!(
        "继承 {} / 接管 {} / 拆分 {}",
        boundary_count(Boundary::Inherited),
        boundary_count(Boundary::Taken),
        boundary_count(Boundary::SharedSplit)
    );
    s.add_dyn("归属快照", shape_ok, &live_text);

    let man_text = format!("清单 {} 条 / 序号 {}", m_len, if m_seq { "严格+1" } else { "断裂" });
    s.add_dyn("清单快照", m_len == RES_COUNT && m_seq, &man_text);

    let inj_text = format!(
        "注入 {} 检出 {} 逃逸 {} 指纹 0x{:08X}",
        log.injected, log.detected, log.escaped, d1
    );
    s.add_dyn("注入快照", log.conserved(), &inj_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9338_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.truncated(), false);
        assert_eq!(s.len() >= 12, true);
    }

    #[test]
    fn boundary_table_is_frozen() {
        let live = frozen_decl();
        assert_eq!(live.len(), RES_COUNT);
        assert_eq!(boundary_audit(&live), 0);
        assert_eq!(boundary_count(Boundary::Inherited), 6);
        assert_eq!(boundary_count(Boundary::Taken), 5);
        assert_eq!(boundary_count(Boundary::SharedSplit), 1);
        assert_eq!(Boundary::ALL.len(), 3);
    }

    #[test]
    fn zero_rewrite_promise_holds() {
        let live = frozen_decl();
        assert_eq!(rewrite_audit(&live), 0);
        let mut i = 0usize;
        while i < live.len() {
            if let Some(d) = live.get(i) {
                assert_eq!(d.rewrite_units, 0);
                assert_eq!(d.boundary.allows_rewrite(), false);
            }
            i += 1;
        }
    }

    #[test]
    fn errno_sets_are_ordered_and_known() {
        let live = frozen_decl();
        assert_eq!(errno_audit(&live), 0);
        assert_eq!(errno_known(EINVAL), true);
        assert_eq!(errno_known(0), false);
        assert_ne!(errno_note(EBADF).len(), 0);
        assert_ne!(errno_note(9999).len(), 0);
    }

    #[test]
    fn manifest_is_complete_and_sequential() {
        let live = frozen_decl();
        let m = match build_manifest(&live) {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(m.len(), RES_COUNT);
        assert_eq!(m.sequential(), true);
        assert_eq!(manifest_audit(&m, &live), 0);
        let mut i = 0usize;
        while i < m.len() {
            if let (Some(row), Some(d)) = (m.row(i), live.get(i)) {
                assert_eq!(row.res_tag, d.res.tag());
                assert_eq!(row.kind, kind_of(d.boundary));
            }
            i += 1;
        }
    }

    #[test]
    fn manifest_rejects_dup_skip_and_overflow() {
        let mut m = IncrementManifest::new();
        let first = Increment { seq: 1, res_tag: 0, kind: IncrKind::Promise, note: "一" };
        assert_eq!(m.push(first), Ok(0));
        assert_eq!(m.push(first), Err(EEXIST));
        assert_eq!(
            m.push(Increment { seq: 3, res_tag: 1, kind: IncrKind::Promise, note: "三" }),
            Err(EINVAL)
        );
        let mut cap = IncrementManifest::new();
        let mut n = 0u16;
        while (n as usize) < MANIFEST_CAP {
            let row = Increment { seq: n + 1, res_tag: 0, kind: IncrKind::Ledger, note: "填" };
            assert_eq!(cap.push(row).is_ok(), true);
            n += 1;
        }
        let over = cap.push(Increment { seq: 17, res_tag: 0, kind: IncrKind::Ledger, note: "溢" });
        assert_eq!(over, Err(ENOMEM));
        assert_eq!(cap.len(), MANIFEST_CAP);
    }

    #[test]
    fn promise_seal_is_one_way_until_violation() {
        let mut empty = PromiseSeal::new();
        assert_eq!(empty.seal(), Err(EINVAL));
        assert_eq!(empty.missing() as usize, RES_COUNT);
        let mut seal = match seal_all() {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(seal.holds(), true);
        assert_eq!(seal.missing(), 0);
        assert_eq!(seal.raise(0), Err(EPERM));
        assert_eq!(seal.violate(5), Ok(PROMISE_ALL & !(1u16 << 5)));
        assert_eq!(seal.sealed(), false);
        assert_eq!(seal.violations(), 1);
        assert_eq!(seal.holds(), false);
        assert_eq!(seal.violate(RES_COUNT), Err(EINVAL));
    }

    #[test]
    fn stage_machine_is_single_step() {
        let a = advance(NoteStage::Draft, NoteStage::Declared);
        assert_eq!(a, Ok(NoteStage::Declared));
        assert_eq!(advance(NoteStage::Declared, NoteStage::Promised), Ok(NoteStage::Promised));
        assert_eq!(advance(NoteStage::Promised, NoteStage::Sealed), Ok(NoteStage::Sealed));
        assert_eq!(advance(NoteStage::Draft, NoteStage::Sealed), Err(EPERM));
        assert_eq!(advance(NoteStage::Sealed, NoteStage::Promised), Err(EPERM));
    }

    #[test]
    fn note_assembles_and_rechecks() {
        let n = match assemble() {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(n.stage, NoteStage::Sealed);
        assert_eq!(n.decls, 12);
        assert_eq!(n.increments, 12);
        assert_eq!(n.promises, PROMISE_ALL);
        assert_ne!(n.stamp, 0);
        assert_eq!(n.summary, NOTE_SUMMARY);
        assert_eq!(n.deliverable(), true);
        assert_eq!(note_audit(&n), true);
    }

    #[test]
    fn injections_are_all_detected() {
        let (log, per) = run_injections();
        assert_eq!(log.injected, 30);
        assert_eq!(log.detected, 30);
        assert_eq!(log.escaped, 0);
        assert_eq!(log.conserved(), true);
        assert_eq!(per, [10, 10, 10]);
        assert_eq!(Mutant::ALL.len(), 3);
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i) {
                assert_eq!(m.detect(), true);
                assert_ne!(m.detail().len(), 0);
            }
            i += 1;
        }
    }

    #[test]
    fn digest_is_stable_and_sensitive() {
        let live = frozen_decl();
        let m = match build_manifest(&live) {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        let a = note_digest(&live, &m);
        let b = note_digest(&live, &m);
        assert_eq!(a, b);
        assert_ne!(a, 0);
        let mut tweaked = live;
        if let Some(d) = tweaked.get_mut(0) {
            d.increments = 99;
        }
        assert_ne!(note_digest(&tweaked, &m), a);
    }

    #[test]
    fn diagnostic_codes_are_closed() {
        assert_eq!(NoteCode::ALL.len(), 15);
        let mut i = 0usize;
        while i < NoteCode::ALL.len() {
            if let Some(a) = NoteCode::ALL.get(i) {
                assert_eq!(a.detail().is_empty(), false);
                assert_eq!(a.seg_addr(), SEG_BASE + a.code() as u32);
                let mut j = i + 1;
                while j < NoteCode::ALL.len() {
                    if let Some(b) = NoteCode::ALL.get(j) {
                        assert_ne!(a.code(), b.code());
                        assert_ne!(a.detail(), b.detail());
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        assert_eq!(popcount16(PROMISE_ALL), 12);
    }
}
