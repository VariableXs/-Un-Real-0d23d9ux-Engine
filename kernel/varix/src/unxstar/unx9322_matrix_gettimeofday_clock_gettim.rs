//! UNX-F9322 gettimeofday/clock_gettime 坏指针矩阵（EFAULT） · 0x7BE00段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9322 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9322-J1 坏指针注入判据过，全 EFAULT
//!   格过，10/10 次检出格过，与 B06 时间族对账格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 两个 syscall 共用一条「用户写回探针」：gettimeofday(96) 写 timeval
//!      （16 字节）、clock_gettime(228) 写 timespec（16 字节）。探针只做纯整数
//!      区间运算：空指针 → 非规范地址 → 内核地址 → 用户上界溢出 → 区域查找
//!      → 跨区/只读 → 放行；全程 checked_* 与位掩码，不做除法取模。
//!   ② 判定次序与 Linux 对齐：gettimeofday 无 clockid 参数，先探针后取值；
//!      clock_gettime 先解析 clockid（EINVAL）再拷贝（EFAULT）——「非法
//!      clockid + 坏指针」必须回 EINVAL 而不是 EFAULT，这本身是一条判据。
//!   ③ 写回原子：任何故障格 wrote 恒为 0（不得半写）；成功格 wrote 恒为 16。
//!   ④ 十格指针矩阵 × 10 轮：7 个故障格每轮必须是 EFAULT（不得漂移成
//!      EINVAL/ENOSYS），3 个正常格每轮写满 16 字节；账本
//!      injected == detected + escaped，escaped 恒为 0。
//!   ⑤ 与 B06 时间族对账：同 tick 内 gettimeofday 的 usec 必须等于
//!      clock_gettime(CLOCK_REALTIME) 的 nsec/1000；COARSE 等于 fine 按 4ms
//!      取整；MONOTONIC 严格单调，REALTIME 允许且只允许一次回拨。
//!   ⑥ 遗留 tz 指针自 2.6 起被忽略，任何非空 tz 都不得解引用（tz_touched 恒 false）。
//!   ⑦ 段基址 0x7BE00 超出 u16，故诊断码判别值用 u32（码值 = SEG_BASE + 序号）。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9322 独占 0x7BE00..=0x7BE3F）。
pub const SEG_BASE: u32 = 0x7BE00;
/// 用户地址空间上界（47 位规范用户区最后字节）。
pub const USER_TOP: u64 = 0x0000_7FFF_FFFF_FFFF;
/// 非规范空洞起点。
pub const NONCANON_BASE: u64 = 0x0000_8000_0000_0000;
/// 内核半区起点。
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
/// 只读代码段、可写堆段、guard 红区、用户栈、内核直映射。
pub const TEXT_BASE: u64 = 0x0000_0000_0040_0000;
pub const TEXT_LEN: u64 = 0x0000_0000_0010_0000;
pub const HEAP_BASE: u64 = 0x0000_0000_0100_0000;
pub const HEAP_LEN: u64 = 0x0000_0000_0100_0000;
pub const GUARD_BASE: u64 = 0x0000_7FFF_FEF0_0000;
pub const GUARD_LEN: u64 = 0x0000_0000_0010_0000;
pub const STACK_BASE: u64 = 0x0000_7FFF_FF00_0000;
pub const STACK_LEN: u64 = 0x0000_0000_0100_0000;
pub const KERNEL_LEN: u64 = 0x0000_0000_1000_0000;
/// 堆段末尾（不含）、栈段末尾（恰为规范区上界）。
pub const HEAP_END: u64 = HEAP_BASE + HEAP_LEN;
pub const STACK_END: u64 = STACK_BASE + STACK_LEN;
/// 页大小与结构体字节数（x86-64 上 timeval/timespec 均 16 字节）。
pub const PAGE: u64 = 0x1000;
pub const TIMEVAL_SIZE: u64 = 16;
pub const TIMESPEC_SIZE: u64 = 16;
/// 系统调用号。
pub const SYS_GETTIMEOFDAY: u32 = 96;
pub const SYS_CLOCK_GETTIME: u32 = 228;
/// clockid 取值（Linux uapi）。
pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
/// BOOTTIME 与 MONOTONIC 的建模偏移（秒）。
pub const BOOTTIME_OFFSET_SEC: i64 = 1000;
/// 单位换算与 COARSE 分辨率（4ms，CONFIG_HZ=250 量级）。
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
pub const NSEC_PER_USEC: u64 = 1_000;
pub const COARSE_RES_NS: u32 = 4_000_000;
/// Linux errno（正数口径）。
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;
/// 矩阵规模：十格 × 10 轮。
pub const INJECT_ROUNDS: u32 = 10;
pub const CELL_COUNT: usize = 10;
/// B06 对账：六样本、步长 250ms、末样本前回拨 1s。
pub const SAMPLE_COUNT: usize = 6;
pub const SAMPLE_TICK_USEC: u32 = 250_000;
pub const STEP_SAMPLE_INDEX: usize = 5;
pub const STEP_DELTA_SEC: i64 = -1;
/// 开机初值（整数常量，绝无墙钟）。
pub const BOOT_REAL_SEC: i64 = 1_700_000_000;
pub const BOOT_MONO_SEC: i64 = 12;
pub const BOOT_MONO_NSEC: u32 = 345_000_000;
/// 规范语料条数。
pub const CORPUS_COUNT: usize = 12;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（码值 = SEG_BASE + 1 + 变体序号，两两唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum TimeCode {
    SegmentDrift = SEG_BASE + 1,
    NullPointer = SEG_BASE + 2,
    GuardPage = SEG_BASE + 3,
    KernelPointer = SEG_BASE + 4,
    NonCanonical = SEG_BASE + 5,
    ReadOnlyTarget = SEG_BASE + 6,
    StraddleWrite = SEG_BASE + 7,
    WrapAround = SEG_BASE + 8,
    TzDereferenced = SEG_BASE + 9,
    PartialWrite = SEG_BASE + 10,
    ClockPrecedence = SEG_BASE + 11,
    ReconDrift = SEG_BASE + 12,
}

impl TimeCode {
    /// 段内绝对码值。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TimeCode::SegmentDrift => "段位漂移：诊断码必须落在 0x7BE00..=0x7BE3F 且按 0x40 对齐；请核对施工卡段基址",
            TimeCode::NullPointer => "空指针写回：tp 为 NULL 时内核不得解引用；用户态应先判空再调用",
            TimeCode::GuardPage => "命中 guard 页：栈下红区无映射；请检查栈指针是否越过 RLIMIT_STACK",
            TimeCode::KernelPointer => "内核地址写回：用户半区之外一律 EFAULT；请勿把内核符号地址当用户缓冲",
            TimeCode::NonCanonical => "非规范地址：bit47..bit63 必须全等；请用正规用户态分配器取址",
            TimeCode::ReadOnlyTarget => "只读段写回：代码段不可写；请把 timeval 放在栈或堆上",
            TimeCode::StraddleWrite => "跨区写回：16 字节区间跨出所属映射段；请保证落在同一段内",
            TimeCode::WrapAround => "地址上界溢出：tp+16 越过 USER_TOP；请换低地址缓冲",
            TimeCode::TzDereferenced => "遗留 tz 被解引用：gettimeofday 的 tz 自 2.6 起被忽略，不得触碰",
            TimeCode::PartialWrite => "半写：故障路径 wrote 必须为 0；请先整段探针再取时钟",
            TimeCode::ClockPrecedence => "次序倒置：clock_gettime 必须先判 clockid 再拷贝，非法 id 回 EINVAL",
            TimeCode::ReconDrift => "对账漂移：gettimeofday 与 clock_gettime(REALTIME) 不得跨 tick 分叉",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: &'static [TimeCode] = &[
        TimeCode::SegmentDrift, TimeCode::NullPointer, TimeCode::GuardPage,
        TimeCode::KernelPointer, TimeCode::NonCanonical, TimeCode::ReadOnlyTarget,
        TimeCode::StraddleWrite, TimeCode::WrapAround, TimeCode::TzDereferenced,
        TimeCode::PartialWrite, TimeCode::ClockPrecedence, TimeCode::ReconDrift,
    ];
}

// ── 探针判定 ──────────────────────────────────────────────────────────
/// 写回探针结果（码值 0 = 放行，非 0 = 故障种类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeOutcome {
    Ok,
    Null,
    Unmapped,
    KernelOnly,
    NonCanonical,
    ReadOnly,
    Straddle,
    Overflow,
}

impl ProbeOutcome {
    /// 闭集编码（矩阵表与指纹共用）。
    pub fn code(self) -> u32 {
        match self {
            ProbeOutcome::Ok => 0,
            ProbeOutcome::Null => 1,
            ProbeOutcome::Unmapped => 2,
            ProbeOutcome::KernelOnly => 3,
            ProbeOutcome::NonCanonical => 4,
            ProbeOutcome::ReadOnly => 5,
            ProbeOutcome::Straddle => 6,
            ProbeOutcome::Overflow => 7,
        }
    }
    /// 是否属于故障（一律映射 EFAULT）。
    pub fn is_fault(self) -> bool {
        self.code() != 0
    }
    /// 对应 errno：故障恒 EFAULT，放行恒 0。
    pub fn errno(self) -> i32 {
        if self.is_fault() { EFAULT } else { 0 }
    }
}

/// 地址空间区域种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    UserRo,
    UserRw,
    Guard,
    Kernel,
}

/// 一个地址区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region { pub base: u64, pub len: u64, pub kind: RegionKind }

impl Region {
    /// 区间末尾（不含）。
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.len)
    }
    /// 是否包含该地址。
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }
}

/// 冻结地址空间表（按 base 升序、两两不重叠）。
pub const REGIONS: &'static [Region] = &[
    Region { base: TEXT_BASE, len: TEXT_LEN, kind: RegionKind::UserRo },
    Region { base: HEAP_BASE, len: HEAP_LEN, kind: RegionKind::UserRw },
    Region { base: GUARD_BASE, len: GUARD_LEN, kind: RegionKind::Guard },
    Region { base: STACK_BASE, len: STACK_LEN, kind: RegionKind::UserRw },
    Region { base: KERNEL_BASE, len: KERNEL_LEN, kind: RegionKind::Kernel },
];

/// 按地址查区域（顺序扫描，表恒为 5 项）。
pub fn lookup(addr: u64) -> Option<Region> {
    let mut i = 0usize;
    while i < REGIONS.len() {
        if let Some(r) = REGIONS.get(i).copied() {
            if r.contains(addr) {
                return Some(r);
            }
        }
        i += 1;
    }
    None
}

/// 写回探针：`addr..addr+size` 是否可作为用户写回目标。
///
/// 次序即语义：空指针 → 非规范 → 内核 → 上界溢出 → 未映射 → 跨区 → 只读。
/// 任何一步都不产生副作用，也不做除法取模。
pub fn probe_write(addr: u64, size: u64) -> ProbeOutcome {
    if addr == 0 {
        return ProbeOutcome::Null;
    }
    if addr >= NONCANON_BASE && addr < KERNEL_BASE {
        return ProbeOutcome::NonCanonical;
    }
    if addr >= KERNEL_BASE {
        return ProbeOutcome::KernelOnly;
    }
    // 用 size-1 求末字节，避免 addr+size 回绕漏判。
    let last = match addr.checked_add(size.saturating_sub(1)) {
        Some(v) => v,
        None => return ProbeOutcome::Overflow,
    };
    if last > USER_TOP {
        return ProbeOutcome::Overflow;
    }
    let region = match lookup(addr) {
        Some(r) => r,
        None => return ProbeOutcome::Unmapped,
    };
    if last >= region.end() {
        // 首字节在某段、末字节越界：内核按页锁，第二页未命中即整体失败。
        return ProbeOutcome::Straddle;
    }
    match region.kind {
        RegionKind::UserRw => ProbeOutcome::Ok,
        RegionKind::UserRo => ProbeOutcome::ReadOnly,
        RegionKind::Guard => ProbeOutcome::Unmapped,
        RegionKind::Kernel => ProbeOutcome::KernelOnly,
    }
}

// ── 系统调用模型 ──────────────────────────────────────────────────────
/// 一次时间系统调用（两个 syscall 共用同一形状）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    pub nr: u32,
    pub clockid: i32,
    pub ptr: u64,
    pub tz: u64,
}

/// 调用结果：errno + 落盘字节 + 故障种类 + tz 是否被触碰。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallResult {
    pub errno: i32,
    pub wrote: u32,
    pub fault: Option<ProbeOutcome>,
    pub tz_touched: bool,
}

impl CallResult {
    pub const fn copied(wrote: u32) -> CallResult {
        CallResult { errno: 0, wrote, fault: None, tz_touched: false }
    }
    pub const fn faulted(kind: ProbeOutcome) -> CallResult {
        CallResult { errno: EFAULT, wrote: 0, fault: Some(kind), tz_touched: false }
    }
    pub const fn rejected(errno: i32) -> CallResult {
        CallResult { errno, wrote: 0, fault: None, tz_touched: false }
    }
}

/// clockid → 归类；表外返回 None（调用方回 EINVAL）。
pub fn clock_supported(id: i32) -> bool {
    id == CLOCK_REALTIME
        || id == CLOCK_MONOTONIC
        || id == CLOCK_MONOTONIC_RAW
        || id == CLOCK_REALTIME_COARSE
        || id == CLOCK_MONOTONIC_COARSE
        || id == CLOCK_BOOTTIME
}

/// 派发一次调用（纯判定，不触碰任何时钟状态）。
pub fn dispatch(call: &Call) -> CallResult {
    if call.nr == SYS_GETTIMEOFDAY {
        // gettimeofday 没有 clockid 参数：先整段探针，再取 REALTIME。
        let p = probe_write(call.ptr, TIMEVAL_SIZE);
        if p.is_fault() {
            return CallResult::faulted(p);
        }
        return CallResult::copied(TIMEVAL_SIZE as u32);
    }
    if call.nr == SYS_CLOCK_GETTIME {
        // 次序：clockid 解析（EINVAL）先于拷贝（EFAULT），与 Linux 一致。
        if !clock_supported(call.clockid) {
            return CallResult::rejected(EINVAL);
        }
        let p = probe_write(call.ptr, TIMESPEC_SIZE);
        if p.is_fault() {
            return CallResult::faulted(p);
        }
        return CallResult::copied(TIMESPEC_SIZE as u32);
    }
    // 未实现的系统调用号：显性 ENOSYS，绝不假装成功。
    CallResult::rejected(ENOSYS)
}

/// 判定一次结果是否符合冻结期望。
pub fn verdict(r: CallResult, expect: ProbeOutcome) -> bool {
    if expect.is_fault() {
        r.errno == EFAULT && r.wrote == 0 && r.fault.map(|f| f.code()) == Some(expect.code())
    } else {
        r.errno == 0 && r.wrote == TIMESPEC_SIZE as u32 && r.fault.is_none() && !r.tz_touched
    }
}

// ── 时钟状态（B06 时间族对账用） ────────────────────────────────────
/// 规范化时间值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timespec { pub sec: i64, pub nsec: u32 }

impl Timespec {
    /// 纳秒总量（单调性比较；sec 恒非负）。
    pub fn total_ns(&self) -> Option<u64> {
        (self.sec as u64)
            .checked_mul(NSEC_PER_SEC)
            .and_then(|v| v.checked_add(self.nsec as u64))
    }
    /// 微秒部分（对账量）。
    pub fn usec(&self) -> u32 {
        self.nsec / NSEC_PER_USEC as u32
    }
}

/// 双钟状态：REALTIME 可回拨，MONOTONIC 只增。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockState {
    pub realtime: Timespec,
    pub monotonic: Timespec,
}

impl ClockState {
    pub const fn boot() -> ClockState {
        ClockState {
            realtime: Timespec { sec: BOOT_REAL_SEC, nsec: 0 },
            monotonic: Timespec { sec: BOOT_MONO_SEC, nsec: BOOT_MONO_NSEC },
        }
    }

    /// 两钟同时前进 `usec` 微秒；usec 越界回 EINVAL。
    pub fn tick_us(&mut self, usec: u32) -> Result<(), i32> {
        if usec >= 1_000_000 {
            return Err(EINVAL);
        }
        let add_ns = (usec as u64).saturating_mul(NSEC_PER_USEC);
        advance(&mut self.realtime, add_ns)?;
        advance(&mut self.monotonic, add_ns)?;
        Ok(())
    }

    /// REALTIME 回拨/前拨 `delta_sec` 秒；越过纪元回 EINVAL。
    pub fn step_realtime(&mut self, delta_sec: i64) -> Result<(), i32> {
        let sec = self.realtime.sec.checked_add(delta_sec).ok_or(EINVAL)?;
        if sec < 0 {
            return Err(EINVAL);
        }
        self.realtime.sec = sec;
        Ok(())
    }

    /// 按 clockid 读回；未知 clockid 返回 None。
    pub fn read(&self, clockid: i32) -> Option<Timespec> {
        if clockid == CLOCK_REALTIME {
            Some(self.realtime)
        } else if clockid == CLOCK_MONOTONIC || clockid == CLOCK_MONOTONIC_RAW {
            Some(self.monotonic)
        } else if clockid == CLOCK_BOOTTIME {
            Some(Timespec {
                sec: self.monotonic.sec.saturating_add(BOOTTIME_OFFSET_SEC),
                nsec: self.monotonic.nsec,
            })
        } else if clockid == CLOCK_REALTIME_COARSE {
            Some(Timespec { sec: self.realtime.sec, nsec: truncate_coarse(self.realtime.nsec) })
        } else if clockid == CLOCK_MONOTONIC_COARSE {
            Some(Timespec { sec: self.monotonic.sec, nsec: truncate_coarse(self.monotonic.nsec) })
        } else {
            None
        }
    }
}

/// 纳秒累加 + 进位（单次 tick 增量 < 1e9，进位至多 1）。
fn advance(ts: &mut Timespec, add_ns: u64) -> Result<(), i32> {
    let total = (ts.nsec as u64).checked_add(add_ns).ok_or(EINVAL)?;
    let carry = total / NSEC_PER_SEC;
    let nsec = total % NSEC_PER_SEC;
    let sec = ts.sec.checked_add(carry as i64).ok_or(EINVAL)?;
    ts.sec = sec;
    ts.nsec = nsec as u32;
    Ok(())
}

/// COARSE 粒度取整（除数是常量，允许；非常量除法一律禁用）。
pub fn truncate_coarse(nsec: u32) -> u32 {
    (nsec / COARSE_RES_NS) * COARSE_RES_NS
}

// ── 十格指针矩阵 ──────────────────────────────────────────────────────
/// 冻结矩阵行：地址 + 期望判定 + 标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell { pub addr: u64, pub expect: ProbeOutcome, pub label: &'static str }

/// 冻结十格（正常/空/guard/内核/非规范/只读/跨区/溢出/边界接受）。
pub const CELLS: &'static [Cell] = &[
    Cell { addr: 0, expect: ProbeOutcome::Null, label: "空指针" },
    Cell { addr: GUARD_BASE + 0x40, expect: ProbeOutcome::Unmapped, label: "guard 红区" },
    Cell { addr: KERNEL_BASE + 0x1000, expect: ProbeOutcome::KernelOnly, label: "内核直映射" },
    Cell { addr: NONCANON_BASE + 0x8, expect: ProbeOutcome::NonCanonical, label: "非规范空洞" },
    Cell { addr: TEXT_BASE + 0x100, expect: ProbeOutcome::ReadOnly, label: "只读代码段" },
    Cell { addr: USER_TOP - 7, expect: ProbeOutcome::Overflow, label: "用户上界回绕" },
    Cell { addr: HEAP_BASE, expect: ProbeOutcome::Ok, label: "堆首字节" },
    Cell { addr: STACK_BASE + 0x8000, expect: ProbeOutcome::Ok, label: "栈中部" },
    Cell { addr: HEAP_END - 8, expect: ProbeOutcome::Straddle, label: "堆末尾跨界" },
    Cell { addr: HEAP_END - 16, expect: ProbeOutcome::Ok, label: "堆末尾贴合" },
];

/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一笔；守恒被破坏则回 EINVAL（调用方必须处理）。
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

/// 矩阵运行账目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixOutcome {
    pub per_cell: [u32; CELL_COUNT],
    /// 每轮都以 EFAULT + wrote=0 收场的故障格数。
    pub efaul_cells: u32,
    /// 每轮都写满 16 字节的正常格数。
    pub ok_cells: u32,
    pub ledger: InjectLedger, pub ledger_ok: bool,
}

/// 十格 × 10 轮：逐格检出数、EFAULT 格数与账本。
pub fn run_pointer_matrix() -> MatrixOutcome {
    let mut out = MatrixOutcome {
        per_cell: [0u32; CELL_COUNT], efaul_cells: 0, ok_cells: 0,
        ledger: InjectLedger::zero(), ledger_ok: true,
    };
    let mut ci = 0usize;
    while ci < CELL_COUNT {
        let cell = match CELLS.get(ci).copied() {
            Some(c) => c,
            None => break,
        };
        let mut round = 0u32;
        let mut all_efault = true;
        let mut all_write = true;
        while round < INJECT_ROUNDS {
            // 同一格分别走两个 syscall：gettimeofday 与 clock_gettime。
            let g = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: cell.addr, tz: 0 });
            let c = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_MONOTONIC, ptr: cell.addr, tz: 0 });
            let caught = verdict(g, cell.expect) && verdict(c, cell.expect);
            if caught {
                if let Some(slot) = out.per_cell.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
                if cell.expect.is_fault() {
                    if g.errno != EFAULT || c.errno != EFAULT || g.wrote != 0 || c.wrote != 0 {
                        all_efault = false;
                    }
                } else if g.wrote != TIMEVAL_SIZE as u32 || c.wrote != TIMESPEC_SIZE as u32 {
                    all_write = false;
                }
            }
            if out.ledger.record(caught).is_err() {
                out.ledger_ok = false;
            }
            round += 1;
        }
        if cell.expect.is_fault() && all_efault {
            out.efaul_cells = out.efaul_cells.saturating_add(1);
        }
        if !cell.expect.is_fault() && all_write {
            out.ok_cells = out.ok_cells.saturating_add(1);
        }
        ci += 1;
    }
    out
}

// ── B06 时间族对账 ────────────────────────────────────────────────────
/// 冻结样本：REALTIME 秒、gettimeofday 的 usec、期望 COARSE 纳秒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample { pub sec: i64, pub usec: u32, pub coarse_nsec: u32 }

/// 六个样本（末样本前有一次 -1s 回拨）。
pub const SAMPLES: &'static [Sample] = &[
    Sample { sec: 1_700_000_000, usec: 0, coarse_nsec: 0 },
    Sample { sec: 1_700_000_000, usec: 250_000, coarse_nsec: 248_000_000 },
    Sample { sec: 1_700_000_000, usec: 500_000, coarse_nsec: 500_000_000 },
    Sample { sec: 1_700_000_000, usec: 750_000, coarse_nsec: 748_000_000 },
    Sample { sec: 1_700_000_001, usec: 0, coarse_nsec: 0 },
    Sample { sec: 1_700_000_000, usec: 250_000, coarse_nsec: 248_000_000 },
];

/// 对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Recon {
    pub samples: u32, pub matched: u32, pub nsec_aligned: u32,
    pub coarse_ok: u32, pub mono_ok: u32, pub regressions: u32,
}

impl Recon {
    pub const fn zero() -> Recon {
        Recon { samples: 0, matched: 0, nsec_aligned: 0, coarse_ok: 0, mono_ok: 0, regressions: 0 }
    }
}

/// 六样本对账：gettimeofday vs clock_gettime(REALTIME) vs COARSE vs MONOTONIC。
pub fn run_b06_reconciliation() -> Recon {
    let mut clk = ClockState::boot();
    let mut recon = Recon::zero();
    let mut prev_mono: Option<u64> = None;
    let mut prev_real: Option<u64> = None;
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        let sample = match SAMPLES.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        if i > 0 {
            if i == STEP_SAMPLE_INDEX && clk.step_realtime(STEP_DELTA_SEC).is_err() {
                return recon;
            }
            if clk.tick_us(SAMPLE_TICK_USEC).is_err() {
                return recon;
            }
        }
        let fine = match clk.read(CLOCK_REALTIME) {
            Some(v) => v,
            None => return recon,
        };
        let coarse = match clk.read(CLOCK_REALTIME_COARSE) {
            Some(v) => v,
            None => return recon,
        };
        let mono = match clk.read(CLOCK_MONOTONIC) {
            Some(v) => v,
            None => return recon,
        };
        recon.samples = recon.samples.saturating_add(1);
        // ① gettimeofday(usec) 与 clock_gettime(REALTIME)(nsec) 同 tick 一致。
        if fine.sec == sample.sec && fine.usec() == sample.usec {
            recon.matched = recon.matched.saturating_add(1);
        }
        // ② nsec 必须是 1000 的整数倍（否则 usec 换算丢精度）。
        if fine.nsec % (NSEC_PER_USEC as u32) == 0 {
            recon.nsec_aligned = recon.nsec_aligned.saturating_add(1);
        }
        // ③ COARSE = fine 按 4ms 取整，且秒不变。
        if coarse.sec == fine.sec && coarse.nsec == sample.coarse_nsec {
            recon.coarse_ok = recon.coarse_ok.saturating_add(1);
        }
        // ④ MONOTONIC 严格单调。
        if let Some(total) = mono.total_ns() {
            let ok = match prev_mono {
                Some(p) => total > p,
                None => true,
            };
            if ok {
                recon.mono_ok = recon.mono_ok.saturating_add(1);
            }
            prev_mono = Some(total);
        }
        // ⑤ REALTIME 只允许一次回拨（step 样本）。
        if let Some(total) = fine.total_ns() {
            if let Some(p) = prev_real {
                if total < p {
                    recon.regressions = recon.regressions.saturating_add(1);
                }
            }
            prev_real = Some(total);
        }
        i += 1;
    }
    recon
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

/// 规范语料：混合成功、坏指针、非法 clockid、未知系统调用。
pub const CORPUS: &'static [Call] = &[
    Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_BASE, tz: 0 },
    Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: 0, tz: 0 },
    Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: GUARD_BASE + 0x40, tz: 0 },
    Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: TEXT_BASE + 0x10, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_MONOTONIC, ptr: HEAP_END - 8, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_MONOTONIC, ptr: HEAP_END - 16, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_BOOTTIME, ptr: STACK_BASE + 0x1000, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_REALTIME_COARSE, ptr: USER_TOP - 7, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: 99, ptr: HEAP_BASE, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: -1, ptr: HEAP_BASE, tz: 0 },
    Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_MONOTONIC, ptr: KERNEL_BASE, tz: 0 },
    Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: NONCANON_BASE + 0x8, tz: 0 },
];

/// 编码一次调用结果（errno、落盘字节、故障码+1、tz 标记）。
fn encode_call(out: &mut Vec<u8>, r: CallResult) {
    out.push((r.errno & 0x7F) as u8);
    out.push((r.wrote & 0xFF) as u8);
    match r.fault {
        Some(f) => out.push(((f.code() as u8) & 0x0F) + 1),
        None => out.push(0),
    }
    out.push(if r.tz_touched { 1 } else { 0 });
}

/// 规范语料指纹（双跑必须相等）。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CORPUS_COUNT {
        if let Some(call) = CORPUS.get(i) {
            encode_call(&mut bytes, dispatch(call));
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9322 判据集（静态 15 条 + 动态 3 条）。
pub fn run_unx9322_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9322");

    let seg_ok = SEG_BASE == 0x7BE00 && SEG_BASE % 0x40 == 0 && (0x7BE00..=0x7BE3F).contains(&SEG_BASE);
    s.add("段位守卫：0x7BE00 起 0x40 对齐且独占窗口", seg_ok, "段基址必须与施工卡一致，诊断码不得越窗");

    // 区域表：升序、不重叠、按页对齐。
    let mut table_ok = REGIONS.len() == 5;
    let mut i = 0usize;
    while i < REGIONS.len() {
        match REGIONS.get(i) {
            Some(r) => {
                if r.len == 0 || r.len % PAGE != 0 || r.base % PAGE != 0 {
                    table_ok = false;
                }
                if let Some(next) = REGIONS.get(i + 1) {
                    if r.end() > next.base {
                        table_ok = false;
                    }
                }
            }
            None => table_ok = false,
        }
        i += 1;
    }
    s.add("地址空间表：五段升序不重叠且页对齐", table_ok, "冻结表必须单调，探针才能一遍顺序扫描定论");

    // 正常流：gettimeofday 写满 timeval。
    let g = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_BASE, tz: 0 });
    s.add(
        "正常流：gettimeofday 写满 16 字节 timeval",
        g.errno == 0 && g.wrote == TIMEVAL_SIZE as u32 && g.fault.is_none() && !g.tz_touched,
        "落盘字节必须恰为 timeval 大小，且不触碰 tz",
    );

    // 正常流：六种 clockid 全绿。
    let clocks: [i32; 6] = [
        CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_RAW, CLOCK_REALTIME_COARSE,
        CLOCK_MONOTONIC_COARSE, CLOCK_BOOTTIME,
    ];
    let mut all_clocks = true;
    let mut k = 0usize;
    while k < clocks.len() {
        if let Some(id) = clocks.get(k).copied() {
            let r = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: id, ptr: STACK_BASE + 0x8000, tz: 0 });
            if r.errno != 0 || r.wrote != TIMESPEC_SIZE as u32 {
                all_clocks = false;
            }
        }
        k += 1;
    }
    s.add("正常流：六种 clockid 均写满 16 字节 timespec", all_clocks, "合法 clockid 全集必须放行，缺一即白名单漏项");

    // 十格 × 10 轮。
    let run = run_pointer_matrix();
    let mut per_cell_ok = true;
    let mut ci = 0usize;
    while ci < CELL_COUNT {
        match run.per_cell.get(ci) {
            Some(n) => {
                if *n != INJECT_ROUNDS {
                    per_cell_ok = false;
                }
            }
            None => per_cell_ok = false,
        }
        ci += 1;
    }
    s.add("注入：十格 × 10 轮逐格 10/10 检出", per_cell_ok, "任一格掉一轮即说明分类表与探针分叉");

    s.add(
        "全 EFAULT 格：七个故障格逐轮 EFAULT 且零落盘",
        run.efaul_cells == 7 && run.ok_cells == 3,
        "故障格不得漂移成 EINVAL/ENOSYS，也不得半写",
    );

    let ledger_ok = run.ledger_ok
        && run.ledger.conserved()
        && run.ledger.escaped == 0
        && run.ledger.injected == CELL_COUNT as u32 * INJECT_ROUNDS;
    s.add("注入账：100 次注入全检出、逃逸为 0", ledger_ok, "injected == detected + escaped，且 escaped 必须为 0");

    let tail_a = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_END - 16, tz: 0 });
    let tail_b = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_END - 8, tz: 0 });
    s.add("边界格：末 16 字节放行、末 8 字节跨界 EFAULT", tail_a.errno == 0 && tail_b.errno == EFAULT, "跨区判定必须看末字节，而不是只看首字节");

    let tz = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_BASE, tz: 0xDEAD_BEEF_0000_0000 });
    s.add(
        "遗留 tz：非空坏 tz 不解引用、不改变返回值",
        tz.errno == 0 && tz.wrote == TIMEVAL_SIZE as u32 && !tz.tz_touched,
        "gettimeofday 的 tz 自 2.6 起被忽略，解引用即回归 bug",
    );

    let prec = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: -1, ptr: 0, tz: 0 });
    let prec2 = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: 99, ptr: KERNEL_BASE, tz: 0 });
    s.add(
        "次序：非法 clockid 回 EINVAL 且先于 EFAULT",
        prec.errno == EINVAL && prec.wrote == 0 && prec2.errno == EINVAL,
        "clock_gettime 必须先解析 clockid，否则坏指针会掩盖参数错",
    );

    let nosys = dispatch(&Call { nr: 999, clockid: 0, ptr: HEAP_BASE, tz: 0 });
    s.add("未知系统调用：显性 ENOSYS 且零落盘", nosys.errno == ENOSYS && nosys.wrote == 0 && nosys.fault.is_none(), "不得把未知号当成功，也不得回 EFAULT 掩盖");

    let recon = run_b06_reconciliation();
    let recon_ok = recon.samples == SAMPLE_COUNT as u32
        && recon.matched == SAMPLE_COUNT as u32
        && recon.nsec_aligned == SAMPLE_COUNT as u32
        && recon.coarse_ok == SAMPLE_COUNT as u32
        && recon.mono_ok == SAMPLE_COUNT as u32
        && recon.regressions == 1;
    s.add("B06 对账：六样本 usec/nsec 一致、COARSE 取整、单次回拨", recon_ok, "同 tick 内两个 syscall 不得分叉，MONOTONIC 不得回退");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b && fp_a != 0, "同输入两次必须逐字节相等，隐藏状态即失败");

    let mut text_ok = true;
    let mut tj = 0usize;
    while tj < TimeCode::ALL.len() {
        match TimeCode::ALL.get(tj) {
            Some(c) => {
                if c.detail().is_empty() {
                    text_ok = false;
                }
            }
            None => text_ok = false,
        }
        tj += 1;
    }
    s.add("文案闭集：12 个诊断码均有中文建议", text_ok, "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut a = 0usize;
    while a < TimeCode::ALL.len() {
        let mut b = a + 1;
        while b < TimeCode::ALL.len() {
            if let (Some(x), Some(y)) = (TimeCode::ALL.get(a), TimeCode::ALL.get(b)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq, "段内闭集码不得重复，否则判据无法定位");

    let cell_text = format!(
        "十格注入 {} / 检出 {} / 逃逸 {}；EFAULT 格 {} / 正常格 {}",
        run.ledger.injected, run.ledger.detected, run.ledger.escaped, run.efaul_cells, run.ok_cells
    );
    s.add_dyn("指针矩阵快照", per_cell_ok && ledger_ok, &cell_text);

    let recon_text = format!(
        "B06 对账 样本 {} 命中 {} / nsec 对齐 {} / coarse {} / mono {} / 回拨 {}",
        recon.samples, recon.matched, recon.nsec_aligned, recon.coarse_ok, recon.mono_ok, recon.regressions
    );
    s.add_dyn("对账快照", recon_ok, &recon_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_sorted_aligned_disjoint() {
        let mut i = 0usize;
        while i < REGIONS.len() {
            let r = match REGIONS.get(i) {
                Some(v) => *v,
                None => break,
            };
            assert_eq!(r.len % PAGE, 0);
            assert_eq!(r.base % PAGE, 0);
            if let Some(next) = REGIONS.get(i + 1) {
                assert_eq!(r.end() <= next.base, true);
            }
            i += 1;
        }
        assert_eq!(STACK_END, NONCANON_BASE);
        assert_eq!(HEAP_END, 0x0000_0000_0200_0000);
    }

    #[test]
    fn probe_boundary_classification() {
        assert_eq!(probe_write(0, 16).code(), 1);
        assert_eq!(probe_write(GUARD_BASE, 16).code(), 2);
        assert_eq!(probe_write(KERNEL_BASE, 16).code(), 3);
        assert_eq!(probe_write(NONCANON_BASE, 16).code(), 4);
        assert_eq!(probe_write(TEXT_BASE + 0x100, 16).code(), 5);
        assert_eq!(probe_write(HEAP_END - 8, 16).code(), 6);
        assert_eq!(probe_write(USER_TOP - 7, 16).code(), 7);
        assert_eq!(probe_write(HEAP_BASE, 16).code(), 0);
    }

    #[test]
    fn probe_last_byte_math() {
        assert_eq!(probe_write(HEAP_END - 16, 16).code(), 0);
        assert_eq!(probe_write(HEAP_END - 15, 16).code(), 6);
        assert_eq!(probe_write(0x0000_7FFF_FFFF_FFF0, 16).code(), 0);
        assert_eq!(probe_write(0x0000_7FFF_FFFF_FFF1, 16).code(), 7);
    }

    #[test]
    fn dispatch_precedence_and_unknown() {
        let bad_clock = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: -1, ptr: 0, tz: 0 });
        assert_eq!(bad_clock.errno, EINVAL);
        assert_eq!(bad_clock.fault, None);
        let nosys = dispatch(&Call { nr: 999, clockid: 0, ptr: HEAP_BASE, tz: 0 });
        assert_eq!(nosys.errno, ENOSYS);
    }

    #[test]
    fn dispatch_normal_and_tz() {
        let g = dispatch(&Call { nr: SYS_GETTIMEOFDAY, clockid: CLOCK_REALTIME, ptr: HEAP_BASE, tz: 0xDEAD_BEEF });
        assert_eq!(g.errno, 0);
        assert_eq!(g.wrote, 16);
        assert_eq!(g.tz_touched, false);
        let c = dispatch(&Call { nr: SYS_CLOCK_GETTIME, clockid: CLOCK_MONOTONIC, ptr: STACK_BASE + 0x8000, tz: 0 });
        assert_eq!(c.errno, 0);
        assert_eq!(c.wrote, 16);
    }

    #[test]
    fn matrix_all_cells_ten_of_ten() {
        let run = run_pointer_matrix();
        let mut i = 0usize;
        while i < CELL_COUNT {
            match run.per_cell.get(i) {
                Some(n) => assert_eq!(*n, INJECT_ROUNDS),
                None => assert_eq!(1, 0),
            }
            i += 1;
        }
        assert_eq!(run.efaul_cells, 7);
        assert_eq!(run.ok_cells, 3);
        assert_eq!(run.ledger.injected, 100);
        assert_eq!(run.ledger.escaped, 0);
        assert_eq!(run.ledger_ok, true);
    }

    #[test]
    fn reconciliation_matches_frozen_table() {
        let r = run_b06_reconciliation();
        assert_eq!(r.samples, 6);
        assert_eq!(r.matched, 6);
        assert_eq!(r.nsec_aligned, 6);
        assert_eq!(r.coarse_ok, 6);
        assert_eq!(r.mono_ok, 6);
        assert_eq!(r.regressions, 1);
    }

    #[test]
    fn coarse_truncation_is_4ms() {
        assert_eq!(truncate_coarse(0), 0);
        assert_eq!(truncate_coarse(3_999_999), 0);
        assert_eq!(truncate_coarse(4_000_000), 4_000_000);
        assert_eq!(truncate_coarse(250_000_000), 248_000_000);
        assert_eq!(truncate_coarse(999_999_999), 996_000_000);
    }

    #[test]
    fn clock_state_is_checked() {
        let mut clk = ClockState::boot();
        assert_eq!(clk.tick_us(1_000_000), Err(EINVAL));
        assert_eq!(clk.tick_us(250_000), Ok(()));
        assert_eq!(clk.realtime.usec(), 250_000);
        assert_eq!(clk.step_realtime(-2_000_000_000), Err(EINVAL));
        assert_eq!(clk.step_realtime(-1), Ok(()));
        assert_eq!(clk.realtime.sec, BOOT_REAL_SEC - 1);
    }

    #[test]
    fn fingerprint_and_codes() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint() != 0, true);
        let mut a = 0usize;
        while a < TimeCode::ALL.len() {
            let x = match TimeCode::ALL.get(a) {
                Some(v) => *v,
                None => break,
            };
            assert_eq!(x.detail().is_empty(), false);
            assert_eq!(x.code() >= SEG_BASE + 1 && x.code() <= SEG_BASE + 12, true);
            let mut b = a + 1;
            while b < TimeCode::ALL.len() {
                if let Some(y) = TimeCode::ALL.get(b) {
                    assert_eq!(x.code() != y.code(), true);
                }
                b += 1;
            }
            a += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9322_checks();
        assert_eq!(s.all_passed(), true);
    }
}
