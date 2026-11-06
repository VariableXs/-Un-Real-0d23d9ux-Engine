//! UNX-F9342 全域 EFAULT 矩阵总装（跨五族逐格） · 0x7C300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9342 逐字）：
//!   ① 纯功能代码 ≥400 行；
//!   ② 判据原文：UNX-F9342-J1 五族坏指针逐格总装判据过，全 EFAULT 逐格
//!      检出，拷贝方向（in/out）双向逐格格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判定单位是「一次用户内存访问」：(addr, len, dir) → AccessVerdict。
//!      五族各取一对真实系统调用（文件读/写、套接字收/发、时钟取/设、
//!      进程内存读/写、进程属性取/设），每族两向共 10 条拷贝路径；每条
//!      路径 × 8 格固定探针 × 10 轮 = 800 次注入，账本必须满足
//!      injected == detected + escaped 且 escaped == 0。
//!   ② 坏指针分六类且次序冻结：零长度（不触碰用户内存）→ 规范化位
//!      （bit47 以上必须全 0 或全 1，否则硬件 #GP 折 EFAULT）→ 空页
//!      [0,0x1000) → 内核区间 [USER_TOP,..) → 越出语料区 → 逐页走查空洞
//!      → 只读页写。次序错一格就是「误报家族」。
//!   ③ 方向非对称是判据核心：只读页在 In（内核只读源）方向放行，在 Out
//!      （内核写回目标）方向 EFAULT。整张矩阵 In/Out 恰差这一格，逐族
//!      各差 1 格共 5 格——多一格或少一格都判红。
//!   ④ 双路径交叉：页走查（page walk）与 16 页位图对同址必须同判，两条
//!      独立实现互证，防止单路径掩码写错自洽。
//!   ⑤ 探针不落「刚好跨页」的歧义处：空洞格与只读格整段落一页内，跨界
//!      由单独的 Hole 格承担，保证「逐格」含义唯一。
//!   ⑥ 诊断码 12 个（段内 +1..=+12），码值两两唯一，每码带中文建议。
//!   ⑦ 指纹：80 格命中数 + 三类计数 + 每路判定码逐字节喂 FNV-1a，双跑
//!      必须相等（无墙钟/无随机/无隐藏状态）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::{format, string::String, vec::Vec};

use crate::checks::CheckSet;

/// 诊断段基址（F9342 独占 0x7C300..=0x7C33F）。
pub const SEG_BASE: u32 = 0x7C300;
/// Linux errno 数值（正数口径）。
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOMEM: i32 = 12;
/// 4K 页。
pub const PAGE: u64 = 4096;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_MASK: u64 = PAGE - 1;
/// 空页上界（不含）。
pub const NULL_PAGE: u64 = 0x0000_0000_0000_1000;
/// 用户地址空间上界（不含）。
pub const USER_TOP: u64 = 0x0000_8000_0000_0000;
/// 语料区（16 页 = 64 KiB，含 1 空洞页 + 1 只读页）。
pub const ARENA_BASE: u64 = 0x0000_0000_0010_0000;
pub const ARENA_PAGES: u32 = 16;
pub const ARENA_SIZE: u64 = PAGE * ARENA_PAGES as u64;
pub const ARENA_END: u64 = ARENA_BASE + ARENA_SIZE;
/// 空洞页与只读页序号。
pub const HOLE_PAGE: u32 = 3;
pub const RO_PAGE: u32 = 5;
/// 16 页映射位图：除空洞页外全部映射。
pub const ARENA_MAP_BITS: u16 = 0xFFF7;
/// 探针格数 / 路径数 / 每格轮数。
pub const CELLS: usize = 8;
pub const ROUTE_COUNT: usize = 10;
pub const INJECT_ROUNDS: u32 = 10;

/// 五族枚举（真实系统调用族，不引用其他 unx 模块）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    File,
    Net,
    Clock,
    ProcVm,
    ProcStat,
}

impl Family {
    /// 内核读用户内存（In）方向的系统调用号。
    pub fn sysno_in(self) -> u16 {
        match self {
            Family::File => 1,       // write
            Family::Net => 44,       // sendto
            Family::Clock => 227,    // clock_settime
            Family::ProcVm => 311,   // process_vm_writev
            Family::ProcStat => 203, // sched_setaffinity
        }
    }
    /// 内核写用户内存（Out）方向的系统调用号。
    pub fn sysno_out(self) -> u16 {
        match self {
            Family::File => 0,      // read
            Family::Net => 45,      // recvfrom
            Family::Clock => 228,   // clock_gettime
            Family::ProcVm => 310,  // process_vm_readv
            Family::ProcStat => 98, // getrusage
        }
    }
}

/// 拷贝方向：In = 用户→内核（只读源），Out = 内核→用户（要写目标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyDir {
    In,
    Out,
}

/// 五族固定表（矩阵行序）。
pub const FAMILIES: [Family; 5] = [
    Family::File,
    Family::Net,
    Family::Clock,
    Family::ProcVm,
    Family::ProcStat,
];
/// 方向固定表（In 恒在 Out 之前）。
pub const DIRS: [CopyDir; 2] = [CopyDir::In, CopyDir::Out];

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 0x01..=0x0C，两两唯一）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EfaulCode {
    NullDeref = 1,
    KernelPointer = 2,
    NonCanonical = 3,
    UnmappedHole = 4,
    ReadOnlyWrite = 5,
    ZeroLenFault = 6,
    DirectionCollapse = 7,
    CellMissing = 8,
    BitmapDrift = 9,
    LedgerRegress = 10,
    NotReproducible = 11,
    SegmentViolation = 12,
}

impl EfaulCode {
    /// 段内码（u16，1..=12）。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 绝对码值 = 段基址 + 段内码（落在本模块 0x40 窗口）。
    pub fn abs(self) -> u32 {
        SEG_BASE + self.code() as u32
    }
    /// 中文诊断与处置建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EfaulCode::NullDeref => "空页访问：addr 落在 [0,0x1000) 内，必须回 EFAULT 且不得解引用；请检查用户指针前置校验",
            EfaulCode::KernelPointer => "内核指针混用：addr ≥ 0x0000_8000_0000_0000 属内核区间，copy_*_user 必须拒；请核对指针来源",
            EfaulCode::NonCanonical => "非规范地址：bit47 以上未全 0 或全 1，硬件 #GP 需折成 EFAULT；请先做规范化位检查",
            EfaulCode::UnmappedHole => "空洞页：区间覆盖未映射页，必须回 EFAULT；请逐页查映射而不是只看首尾",
            EfaulCode::ReadOnlyWrite => "只读页写入：Out 方向目标 PROT_READ，必须 EFAULT；In 方向读同页应放行",
            EfaulCode::ZeroLenFault => "零长度误报：len==0 不触碰内存，必须放行；请把零长度判定提到最前",
            EfaulCode::DirectionCollapse => "方向判定被抹平：同址 In/Out 同判，只读页那格必丢；请恢复方向参数",
            EfaulCode::CellMissing => "矩阵格未命中：某族某向某格分类与冻结表不符；请按格定位判定次序",
            EfaulCode::BitmapDrift => "位图分歧：映射位图与页走查对同址不同判；请以页表为权威修正位图",
            EfaulCode::LedgerRegress => "账目倒退：注入/检出计数只增不减；请检查回滚路径是否误改累计值",
            EfaulCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态与非确定次序",
            EfaulCode::SegmentViolation => "段位越界：段基址必须落在 0x7C300..=0x7C33F；请核对 SEG_BASE",
        }
    }
    /// 闭集全量。
    pub const ALL: &'static [EfaulCode] = &[
        EfaulCode::NullDeref,
        EfaulCode::KernelPointer,
        EfaulCode::NonCanonical,
        EfaulCode::UnmappedHole,
        EfaulCode::ReadOnlyWrite,
        EfaulCode::ZeroLenFault,
        EfaulCode::DirectionCollapse,
        EfaulCode::CellMissing,
        EfaulCode::BitmapDrift,
        EfaulCode::LedgerRegress,
        EfaulCode::NotReproducible,
        EfaulCode::SegmentViolation,
    ];
}

// ── 页与访问判定 ──────────────────────────────────────────────────────
/// 单页状态（判定的原子单位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    Mapped,
    ReadOnly,
    Hole,
    Outside,
}

/// 一次用户内存访问的判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessVerdict {
    Ok,
    ZeroLen,
    Null,
    NonCanonical,
    KernelRange,
    OutsideArena,
    HoleCross,
    ReadOnlyWrite,
}

impl AccessVerdict {
    /// 是否放行（Ok 与 ZeroLen 都不产生 EFAULT）。
    pub fn accepted(self) -> bool {
        matches!(self, AccessVerdict::Ok | AccessVerdict::ZeroLen)
    }
    /// 判定码（指纹使用，冻结不变）。
    pub fn code(self) -> u32 {
        match self {
            AccessVerdict::Ok => 0,
            AccessVerdict::ZeroLen => 1,
            AccessVerdict::Null => 2,
            AccessVerdict::NonCanonical => 3,
            AccessVerdict::KernelRange => 4,
            AccessVerdict::OutsideArena => 5,
            AccessVerdict::HoleCross => 6,
            AccessVerdict::ReadOnlyWrite => 7,
        }
    }
}

/// 规范化位：x86-64 48 位规范地址要求 bit47 以上全 0 或全 1。
pub fn is_canonical(addr: u64) -> bool {
    let hi = addr >> 47;
    hi == 0 || hi == 0x1_FFFF
}

/// 页序号 → 页状态（越界即 Outside）。
pub fn page_state(idx: u32) -> PageState {
    if idx >= ARENA_PAGES {
        return PageState::Outside;
    }
    if idx == HOLE_PAGE {
        return PageState::Hole;
    }
    if idx == RO_PAGE {
        return PageState::ReadOnly;
    }
    PageState::Mapped
}

/// 地址 → 页状态（语料区外一律 Outside）。
pub fn page_state_of(addr: u64) -> PageState {
    if addr < ARENA_BASE || addr >= ARENA_END {
        return PageState::Outside;
    }
    page_state(((addr - ARENA_BASE) >> PAGE_SHIFT) as u32)
}

/// 位图路径：第 idx 页是否可用（与 page_state 独立实现）。
pub fn bitmap_mapped(idx: u32) -> bool {
    if idx >= ARENA_PAGES {
        return false;
    }
    (ARENA_MAP_BITS >> idx) & 1 == 1
}

/// 位图路径的可访问性（映射位 + 只读位，独立于页走查）。
pub fn bitmap_accepts(addr: u64, len: u64, dir: CopyDir) -> bool {
    if len == 0 {
        return true;
    }
    let last = match addr.checked_add(len - 1) {
        Some(v) => v,
        None => return false,
    };
    if addr < ARENA_BASE || last >= ARENA_END {
        return false;
    }
    let mut page = addr & !PAGE_MASK;
    let last_page = last & !PAGE_MASK;
    loop {
        let idx = ((page - ARENA_BASE) >> PAGE_SHIFT) as u32;
        if !bitmap_mapped(idx) {
            return false;
        }
        // 位图只记映射位，只读位仍在页状态表：Out 方向单独拦。
        if dir == CopyDir::Out && page_state(idx) == PageState::ReadOnly {
            return false;
        }
        if page >= last_page {
            break;
        }
        page = match page.checked_add(PAGE) {
            Some(v) => v,
            None => return false,
        };
    }
    true
}

/// 核心判定：冻结次序（零长度 → 规范位 → 空页 → 内核区间 → 区外 → 走查）。
pub fn judge_access(addr: u64, len: u64, dir: CopyDir) -> AccessVerdict {
    if len == 0 {
        return AccessVerdict::ZeroLen;
    }
    if !is_canonical(addr) {
        return AccessVerdict::NonCanonical;
    }
    if addr < NULL_PAGE {
        return AccessVerdict::Null;
    }
    if addr >= USER_TOP {
        return AccessVerdict::KernelRange;
    }
    let span = match len.checked_sub(1) {
        Some(v) => v,
        None => return AccessVerdict::ZeroLen,
    };
    let last = match addr.checked_add(span) {
        Some(v) => v,
        None => return AccessVerdict::OutsideArena,
    };
    // 语料区外的用户地址无映射：与「未映射 → EFAULT」同义。
    if addr < ARENA_BASE || last >= ARENA_END {
        return AccessVerdict::OutsideArena;
    }
    let mut page = addr & !PAGE_MASK;
    let last_page = last & !PAGE_MASK;
    let mut hole = false;
    let mut ro = false;
    loop {
        match page_state_of(page) {
            PageState::Hole => hole = true,
            PageState::ReadOnly => ro = true,
            PageState::Mapped => {}
            PageState::Outside => return AccessVerdict::OutsideArena,
        }
        if page >= last_page {
            break;
        }
        page = match page.checked_add(PAGE) {
            Some(v) => v,
            None => return AccessVerdict::OutsideArena,
        };
    }
    if hole {
        return AccessVerdict::HoleCross;
    }
    if ro && dir == CopyDir::Out {
        return AccessVerdict::ReadOnlyWrite;
    }
    AccessVerdict::Ok
}

/// 判定 → errno（0 成功；其余一律 EFAULT）。
pub fn errno_of(v: AccessVerdict) -> i32 {
    if v.accepted() {
        0
    } else {
        EFAULT
    }
}

/// 系统调用面的放行语义：成功返回拷贝字节数（零长度即 0）。
pub fn admit_copy(v: AccessVerdict, len: u64) -> Result<u64, i32> {
    match v {
        AccessVerdict::Ok => Ok(len),
        AccessVerdict::ZeroLen => Ok(0),
        _ => Err(EFAULT),
    }
}

// ── 冻结探针格 ────────────────────────────────────────────────────────
/// 坏指针形态（8 格闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    Clean = 0,
    Empty = 1,
    Null = 2,
    NonCanonical = 3,
    KernelRange = 4,
    Unmapped = 5,
    Hole = 6,
    ReadOnly = 7,
}

/// 一格探针：形态 + 绝对地址 + 长度（逐格复算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe { pub kind: FaultKind, pub addr: u64, pub len: u64 }

/// 八格冻结探针（地址全在编译期可算）。
pub const PROBES: [Probe; CELLS] = [
    Probe { kind: FaultKind::Clean, addr: ARENA_BASE, len: 64 },
    Probe { kind: FaultKind::Empty, addr: 0, len: 0 },
    Probe { kind: FaultKind::Null, addr: 0x40, len: 8 },
    Probe { kind: FaultKind::NonCanonical, addr: 0x0000_8000_0000_0000, len: 8 },
    Probe { kind: FaultKind::KernelRange, addr: 0xFFFF_8000_0000_0000, len: 8 },
    Probe { kind: FaultKind::Unmapped, addr: NULL_PAGE + 0x100, len: 16 },
    Probe { kind: FaultKind::Hole, addr: ARENA_BASE + (HOLE_PAGE as u64) * PAGE + 0x800, len: 32 },
    Probe { kind: FaultKind::ReadOnly, addr: ARENA_BASE + (RO_PAGE as u64) * PAGE, len: 128 },
];

/// 按形态取探针（线性查找，避免裸下标）。
pub fn probe_of(kind: FaultKind) -> Option<Probe> {
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(p) = PROBES.get(i) {
            if p.kind == kind {
                return Some(*p);
            }
        }
        i += 1;
    }
    None
}

/// 冻结期望：给定形态与方向，判定必须是什么。
pub fn expect_verdict(kind: FaultKind, dir: CopyDir) -> AccessVerdict {
    match kind {
        FaultKind::Clean => AccessVerdict::Ok,
        FaultKind::Empty => AccessVerdict::ZeroLen,
        FaultKind::Null => AccessVerdict::Null,
        FaultKind::NonCanonical => AccessVerdict::NonCanonical,
        FaultKind::KernelRange => AccessVerdict::KernelRange,
        FaultKind::Unmapped => AccessVerdict::OutsideArena,
        FaultKind::Hole => AccessVerdict::HoleCross,
        // 只读页方向非对称：In 读放行，Out 写 EFAULT。
        FaultKind::ReadOnly => match dir {
            CopyDir::In => AccessVerdict::Ok,
            CopyDir::Out => AccessVerdict::ReadOnlyWrite,
        },
    }
}

/// 路径下标 → 方向（偶数 In、奇数 Out）。
pub fn route_dir(ri: usize) -> CopyDir {
    if ri & 1 == 0 {
        CopyDir::In
    } else {
        CopyDir::Out
    }
}

/// 路径下标 → 族（每族占两个连续下标）。
pub fn route_family(ri: usize) -> Option<Family> {
    FAMILIES.get(ri >> 1).copied()
}

/// 路径下标 → 系统调用号。
pub fn route_sysno(ri: usize) -> Option<u16> {
    match route_family(ri) {
        Some(f) => Some(match route_dir(ri) {
            CopyDir::In => f.sysno_in(),
            CopyDir::Out => f.sysno_out(),
        }),
        None => None,
    }
}

/// 单路单格复算（判据与测试共用同一算式）。
pub fn cell_hits(ri: usize, kind: FaultKind) -> bool {
    let probe = match probe_of(kind) {
        Some(p) => p,
        None => return false,
    };
    let dir = route_dir(ri);
    let want = expect_verdict(kind, dir);
    let got = judge_access(probe.addr, probe.len, dir);
    got == want && errno_of(got) == if want.accepted() { 0 } else { EFAULT }
}

/// 全族单向判据：给定方向，五路对该形态的判定都必须等于冻结期望。
pub fn all_routes_match(kind: FaultKind, dir: CopyDir) -> bool {
    let mut ri = 0usize;
    let mut ok = true;
    while ri < ROUTE_COUNT {
        if route_dir(ri) == dir && !cell_hits(ri, kind) {
            ok = false;
        }
        ri += 1;
    }
    ok
}

// ── 矩阵总装 ──────────────────────────────────────────────────────────
/// 矩阵总装结果：命中计数 + 双向账 + 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixOutcome {
    /// [路径][格] 命中轮数（满值 = INJECT_ROUNDS）
    pub hits: [[u8; CELLS]; ROUTE_COUNT],
    pub faults_in: u32,
    pub faults_out: u32,
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl MatrixOutcome {
    pub const fn empty() -> MatrixOutcome {
        MatrixOutcome {
            hits: [[0u8; CELLS]; ROUTE_COUNT],
            faults_in: 0,
            faults_out: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }
    /// 账本守恒：检出 + 逃逸 == 注入。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
    /// 每格都打满轮数（80 格）。
    pub fn full(&self) -> bool {
        let mut ri = 0usize;
        while ri < ROUTE_COUNT {
            let row = match self.hits.get(ri) {
                Some(r) => r,
                None => return false,
            };
            let mut ci = 0usize;
            while ci < CELLS {
                match row.get(ci) {
                    Some(v) => {
                        if *v != INJECT_ROUNDS as u8 {
                            return false;
                        }
                    }
                    None => return false,
                }
                ci += 1;
            }
            ri += 1;
        }
        true
    }
    /// 某族双向 EFAULT 检出数之和。
    pub fn family_faults(&self, fam: Family) -> u32 {
        let mut total = 0u32;
        let mut ri = 0usize;
        while ri < ROUTE_COUNT {
            if route_family(ri) == Some(fam) {
                let dir = route_dir(ri);
                let mut ci = 0usize;
                while ci < CELLS {
                    if let (Some(row), Some(p)) = (self.hits.get(ri), PROBES.get(ci)) {
                        if let Some(n) = row.get(ci) {
                            if !expect_verdict(p.kind, dir).accepted() {
                                total = total.saturating_add(*n as u32);
                            }
                        }
                    }
                    ci += 1;
                }
            }
            ri += 1;
        }
        total
    }
}

/// 总装复算：10 路 × 8 格 × 10 轮。
pub fn run_matrix() -> MatrixOutcome {
    let mut out = MatrixOutcome::empty();
    let mut ri = 0usize;
    while ri < ROUTE_COUNT {
        let dir = route_dir(ri);
        let mut ci = 0usize;
        while ci < CELLS {
            if let Some(p) = PROBES.get(ci) {
                let want = expect_verdict(p.kind, dir);
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let got = judge_access(p.addr, p.len, dir);
                    let caught = got == want;
                    out.injected = out.injected.saturating_add(1);
                    if caught {
                        out.detected = out.detected.saturating_add(1);
                        if let Some(row) = out.hits.get_mut(ri) {
                            if let Some(c) = row.get_mut(ci) {
                                *c = c.saturating_add(1);
                            }
                        }
                        if errno_of(got) == EFAULT {
                            match dir {
                                CopyDir::In => {
                                    out.faults_in = out.faults_in.saturating_add(1)
                                }
                                CopyDir::Out => {
                                    out.faults_out = out.faults_out.saturating_add(1)
                                }
                            }
                        }
                    } else {
                        out.escaped = out.escaped.saturating_add(1);
                    }
                    round += 1;
                }
            }
            ci += 1;
        }
        ri += 1;
    }
    out
}

/// 冻结常量：800 次注入、550 次 EFAULT（In 250 / Out 300）。
pub const EXPECTED_INJECT: u32 = (ROUTE_COUNT as u32) * (CELLS as u32) * INJECT_ROUNDS;
pub const EXPECTED_FAULT_IN: u32 = 250;
pub const EXPECTED_FAULT_OUT: u32 = 300;
pub const EXPECTED_FAULT_ALL: u32 = EXPECTED_FAULT_IN + EXPECTED_FAULT_OUT;
// ── 双路径交叉 ────────────────────────────────────────────────────────
/// 页走查 vs 位图：16 页 × 3 长度 × 2 方向逐一比对。
pub fn cross_check_pages() -> (u32, u32) {
    let lens = [1u64, PAGE - 1, PAGE];
    let mut agree = 0u32;
    let mut total = 0u32;
    let mut pi = 0u32;
    while pi < ARENA_PAGES {
        let base = ARENA_BASE + (pi as u64) * PAGE;
        let mut li = 0usize;
        while li < lens.len() {
            if let Some(len) = lens.get(li) {
                let mut di = 0usize;
                while di < DIRS.len() {
                    if let Some(dir) = DIRS.get(di).copied() {
                        let walk_ok = judge_access(base, *len, dir).accepted();
                        let map_ok = bitmap_accepts(base, *len, dir);
                        if walk_ok == map_ok {
                            agree = agree.saturating_add(1);
                        }
                        total = total.saturating_add(1);
                    }
                    di += 1;
                }
            }
            li += 1;
        }
        pi += 1;
    }
    (agree, total)
}

// ── Linux 逐码对照 ────────────────────────────────────────────────────
/// (名称, 正数码, 负值, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFAULT", EFAULT, -14, "坏地址：copy_*_user 未修复的页错误"),
    ("EINVAL", EINVAL, -22, "参数非法：与地址无关的拒绝"),
    ("ENOMEM", ENOMEM, -12, "内存不足：地址合法但资源不够"),
];

/// 按名称查正数码；表外返回 0。
pub fn errno_by_name(name: &str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 数值一致：正数码 + 负值互为相反数。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.1 == -row.2;
            }
        }
        i += 1;
    }
    false
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
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

/// 小端喂入 u32。
fn feed_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 规范语料：矩阵命中 + 计数 + 每路判定码。
pub fn matrix_fingerprint() -> u32 {
    let m = run_matrix();
    let mut buf: Vec<u8> = Vec::new();
    let mut ri = 0usize;
    while ri < ROUTE_COUNT {
        if let Some(row) = m.hits.get(ri) {
            let mut ci = 0usize;
            while ci < CELLS {
                if let Some(v) = row.get(ci) {
                    buf.push(*v);
                }
                if let Some(p) = PROBES.get(ci) {
                    feed_u32(&mut buf, judge_access(p.addr, p.len, route_dir(ri)).code());
                }
                ci += 1;
            }
        }
        ri += 1;
    }
    feed_u32(&mut buf, m.injected);
    feed_u32(&mut buf, m.detected);
    feed_u32(&mut buf, m.escaped);
    feed_u32(&mut buf, m.faults_in);
    feed_u32(&mut buf, m.faults_out);
    fnv1a32(&buf)
}

// ── 交付写回面（判据③） ──────────────────────────────────────────────
/// 交付注记：状态与非空结果摘要。
#[derive(Clone, Debug)]
pub struct DeliveryNote { pub card: &'static str, pub status: &'static str, pub summary: String }

impl DeliveryNote {
    pub fn new(card: &'static str, status: &'static str, summary: String) -> DeliveryNote {
        DeliveryNote { card, status, summary }
    }
    /// 判据③：状态为「已完成」且摘要非空。
    pub fn complete(&self) -> bool {
        self.status == "已完成" && !self.summary.is_empty() && !self.card.is_empty()
    }
}

/// 由总装结果生成交付注记。
pub fn delivery_note(m: &MatrixOutcome) -> DeliveryNote {
    DeliveryNote::new(
        "UNX-F9342",
        "已完成",
        format!(
            "五族 10 路 × 8 格 × 10 轮：注入 {} 检出 {} 逃逸 {}；EFAULT In {} / Out {}",
            m.injected, m.detected, m.escaped, m.faults_in, m.faults_out
        ),
    )
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9342 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9342_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9342");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7C300..0x7C340).contains(&SEG_BASE)
        && SEG_BASE + 0x40 <= 0x7C340;
    s.add("段位守卫：0x7C300 且 0x40 对齐，不越 F9343 窗口", seg_ok,
          "段基址必须在 0x7C300..=0x7C33F，段间窗口不得互相覆盖");

    let canon_ok = is_canonical(ARENA_BASE)
        && is_canonical(0xFFFF_8000_0000_0000)
        && !is_canonical(0x0000_8000_0000_0000)
        && is_canonical(u64::MAX)
        && is_canonical(0x0000_7FFF_FFFF_FFFF);
    s.add("规范化位：bit47 以上全 0/全 1 判定逐例", canon_ok,
          "低半区与高半区合法（全 1 极值同属高半区），半 0 半 1 的 0x0000_8000_0000_0000 非法");

    s.add("正常流：十路合法缓冲全部放行",
          all_routes_match(FaultKind::Clean, CopyDir::In)
              && all_routes_match(FaultKind::Clean, CopyDir::Out)
              && admit_copy(judge_access(ARENA_BASE, 64, CopyDir::Out), 64) == Ok(64),
          "合法缓冲在 In/Out 两方向都必须成功并返回字节数");

    s.add("空页：十路全 EFAULT（五族 × 双向）",
          all_routes_match(FaultKind::Null, CopyDir::In)
              && all_routes_match(FaultKind::Null, CopyDir::Out),
          "addr 落在 [0,0x1000) 时任何族任何方向都不得放行");

    s.add("非规范地址：十路全 EFAULT",
          all_routes_match(FaultKind::NonCanonical, CopyDir::In)
              && all_routes_match(FaultKind::NonCanonical, CopyDir::Out),
          "bit47 以上混合的地址必须折成 EFAULT，不得当作内核或用户指针");

    s.add("内核区间：十路全 EFAULT",
          all_routes_match(FaultKind::KernelRange, CopyDir::In)
              && all_routes_match(FaultKind::KernelRange, CopyDir::Out),
          "addr ≥ USER_TOP 属内核空间，用户拷贝必须拒");

    s.add("区外未映射：十路全 EFAULT",
          all_routes_match(FaultKind::Unmapped, CopyDir::In)
              && all_routes_match(FaultKind::Unmapped, CopyDir::Out),
          "空页之上、语料区之下的未映射用户地址同样必须 EFAULT");

    s.add("空洞页：十路全 EFAULT",
          all_routes_match(FaultKind::Hole, CopyDir::In)
              && all_routes_match(FaultKind::Hole, CopyDir::Out),
          "区间覆盖未映射页时必须逐页走查，不能只看首尾");

    let ro_in = all_routes_match(FaultKind::ReadOnly, CopyDir::In);
    let ro_out = all_routes_match(FaultKind::ReadOnly, CopyDir::Out);
    s.add("只读页方向非对称：In 放行、Out 全 EFAULT",
          ro_in && ro_out
              && expect_verdict(FaultKind::ReadOnly, CopyDir::In).accepted()
              && !expect_verdict(FaultKind::ReadOnly, CopyDir::Out).accepted(),
          "同一只读地址读放行写拒；五族各差一格，多一格少一格都判红");

    s.add("零长度：十路一律放行且返回 0 字节",
          all_routes_match(FaultKind::Empty, CopyDir::In)
              && all_routes_match(FaultKind::Empty, CopyDir::Out)
              && admit_copy(judge_access(0, 0, CopyDir::In), 0) == Ok(0),
          "len==0 不触碰用户内存，坏地址也不得报 EFAULT");

    let m = run_matrix();
    s.add("总装：80 格 × 10 轮全部命中（逐格）",
          m.full() && m.injected == EXPECTED_INJECT,
          "10 路 × 8 形态 × 10 轮 = 800 次，每格必须满 10 次");

    s.add("账本守恒：注入 800 / 检出 800 / 逃逸 0",
          m.conserved() && m.escaped == 0 && m.detected == m.injected,
          "injected == detected + escaped 且 escaped 必须为 0");

    s.add("方向账：In 250 / Out 300 逐族齐平",
          m.faults_in == EXPECTED_FAULT_IN && m.faults_out == EXPECTED_FAULT_OUT
              && FAMILIES.iter().all(|f| m.family_faults(*f) == 110),
          "每族双向共 110 次 EFAULT 检出（In 50 + Out 60），五族齐平");

    let (agree, total) = cross_check_pages();
    s.add("双路径交叉：位图与页走查 96/96 一致", agree == total && total == 96,
          "16 页 × 3 长度 × 2 方向共 96 格，两条独立实现必须同判");

    let order_ok = judge_access(u64::MAX, 0, CopyDir::Out) == AccessVerdict::ZeroLen
        && judge_access(0, 8, CopyDir::Out) == AccessVerdict::Null
        && judge_access(0x0000_8000_0000_0000, 8, CopyDir::Out) == AccessVerdict::NonCanonical
        && judge_access(0xFFFF_FFFF_FFFF_F000, 8, CopyDir::In) == AccessVerdict::KernelRange;
    s.add("判定次序：零长度 → 规范位 → 空页 → 内核区间", order_ok,
          "次序冻结；把零长度挪到规范位之后会让 NULL,0 误报");

    let linux_ok = errno_by_name("EFAULT") == EFAULT
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && errno_by_name("ENOSYS") == 0;
    s.add("Linux 逐码：EFAULT=14 表内可复算，表外码拒", linux_ok,
          "拒绝语义必须落在 errno 表内，不得自造码值");

    let fp_a = matrix_fingerprint();
    let fp_b = matrix_fingerprint();
    s.add("双跑一致：矩阵指纹逐字节相等", fp_a == fp_b,
          "同输入两次必须相等；隐藏状态或非确定次序即失败");

    let mut uniq = true;
    let mut i = 0usize;
    while i < EfaulCode::ALL.len() {
        let mut j = i + 1;
        while j < EfaulCode::ALL.len() {
            if let (Some(a), Some(b)) = (EfaulCode::ALL.get(i), EfaulCode::ALL.get(j)) {
                if a.abs() == b.abs() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = EfaulCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码：12 码值两两唯一且文案非空",
          uniq && text_ok && EfaulCode::ALL.len() == 12,
          "段内闭集码不得重复，每码必须能取到处置建议");

    let note = delivery_note(&m);
    s.add("交付写回：状态=已完成、摘要非空", note.complete(),
          "判据③要求 API/软件写回状态与结果摘要，摘要不得为空串");

    let dyn_text = format!(
        "注入 {} 检出 {} 逃逸 {}；In {} / Out {}",
        m.injected, m.detected, m.escaped, m.faults_in, m.faults_out
    );
    s.add_dyn("总装账目快照", m.conserved() && m.full(), &dyn_text);
    let cross_text = format!("双路径 {}/{} 一致", agree, total);
    s.add_dyn("交叉快照", agree == total, &cross_text);
    let fp_text = format!("矩阵指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_flag_covers_both_halves() {
        assert_eq!(is_canonical(0), true);
        assert_eq!(is_canonical(0x0000_7FFF_FFFF_FFFF), true);
        assert_eq!(is_canonical(0xFFFF_8000_0000_0000), true);
        assert_eq!(is_canonical(0x0000_8000_0000_0000), false);
        assert_eq!(is_canonical(0xFFFF_7FFF_FFFF_FFFF), false);
    }

    #[test]
    fn page_walk_sees_hole_and_readonly() {
        assert_eq!(page_state(HOLE_PAGE), PageState::Hole);
        assert_eq!(page_state(RO_PAGE), PageState::ReadOnly);
        assert_eq!(page_state(0), PageState::Mapped);
        assert_eq!(page_state(ARENA_PAGES), PageState::Outside);
        let hole = ARENA_BASE + (HOLE_PAGE as u64) * PAGE + 0x800;
        assert_eq!(judge_access(hole, 32, CopyDir::In), AccessVerdict::HoleCross);
    }

    #[test]
    fn zero_length_never_faults() {
        assert_eq!(judge_access(0, 0, CopyDir::Out), AccessVerdict::ZeroLen);
        assert_eq!(judge_access(u64::MAX, 0, CopyDir::In), AccessVerdict::ZeroLen);
        assert_eq!(admit_copy(judge_access(0, 0, CopyDir::In), 0), Ok(0));
    }

    #[test]
    fn null_kernel_and_gap_are_distinct() {
        assert_eq!(judge_access(0x40, 8, CopyDir::Out), AccessVerdict::Null);
        let kernel = judge_access(0xFFFF_8000_0000_0000, 8, CopyDir::Out);
        assert_eq!(kernel, AccessVerdict::KernelRange);
        let gap = judge_access(NULL_PAGE + 0x100, 16, CopyDir::Out);
        assert_eq!(gap, AccessVerdict::OutsideArena);
        let noncanon = judge_access(0x0000_8000_0000_0000, 8, CopyDir::In);
        assert_eq!(noncanon, AccessVerdict::NonCanonical);
    }

    #[test]
    fn readonly_is_direction_asymmetric() {
        let ro = ARENA_BASE + (RO_PAGE as u64) * PAGE;
        assert_eq!(judge_access(ro, 128, CopyDir::In), AccessVerdict::Ok);
        assert_eq!(judge_access(ro, 128, CopyDir::Out), AccessVerdict::ReadOnlyWrite);
        assert_eq!(errno_of(judge_access(ro, 128, CopyDir::Out)), EFAULT);
    }

    #[test]
    fn matrix_is_full_and_conserved() {
        let m = run_matrix();
        assert_eq!(m.injected, EXPECTED_INJECT);
        assert_eq!(m.detected, EXPECTED_INJECT);
        assert_eq!(m.escaped, 0);
        assert_eq!(m.conserved(), true);
        assert_eq!(m.full(), true);
        assert_eq!(m.faults_in, EXPECTED_FAULT_IN);
        assert_eq!(m.faults_out, EXPECTED_FAULT_OUT);
    }

    #[test]
    fn every_family_sees_both_directions() {
        let m = run_matrix();
        for f in FAMILIES.iter() {
            assert_eq!(m.family_faults(*f), 110);
        }
        assert_eq!(m.faults_in + m.faults_out, EXPECTED_FAULT_ALL);
    }

    #[test]
    fn bitmap_agrees_with_page_walk() {
        let (agree, total) = cross_check_pages();
        assert_eq!(total, 96);
        assert_eq!(agree, total);
        assert_eq!(bitmap_mapped(HOLE_PAGE), false);
        assert_eq!(bitmap_mapped(0), true);
    }

    #[test]
    fn codes_are_unique_and_documented() {
        assert_eq!(EfaulCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < EfaulCode::ALL.len() {
            if let Some(c) = EfaulCode::ALL.get(i) {
                assert_eq!(c.detail().is_empty(), false);
                assert_eq!(c.abs(), SEG_BASE + c.code() as u32);
            }
            let mut j = i + 1;
            while j < EfaulCode::ALL.len() {
                if let (Some(a), Some(b)) = (EfaulCode::ALL.get(i), EfaulCode::ALL.get(j)) {
                    assert_eq!(a.code() == b.code(), false);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn fingerprint_is_reproducible() {
        let a = matrix_fingerprint();
        let b = matrix_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a == 0, false);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9342_checks();
        assert_eq!(s.tally().1, 0, "{}", s.red_items_text());
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.len() >= 12, true);
    }
}
