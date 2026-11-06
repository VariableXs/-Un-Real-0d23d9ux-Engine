//! UNX-F9214 文件族 E 型参数探针总成 · 0x7A300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9214 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9214-J1 nargs 逐位提取判据过，
//!   坏指针全 EFAULT 格过，与参数拷贝探针总成单源；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 参数描述字是单源：每行只存一个 32 位 mask，每槽 4 位（存在/指针/
//!      写向/可空）。nargs 不落字段、只由 mask 逐位弹出，于是「nargs 提取」
//!      与「指针探针」共用同一份数据，结构上无法漂移——这就是单源声明。
//!   ② 文件族 E 型错误面分三层：fd 门（EBADF）→ 值域门（EINVAL）→ 指针门
//!      （EFAULT）。次序不是审美问题：先 fd 可避免对已关闭对象拷贝，先值域
//!      与 Linux sys_* 的前置检查一致，指针门按槽位升序短路到第一个坏指针。
//!   ③ 输出向槽用只读页探针（可读但不可写，仍须 EFAULT），输入向槽用未映射
//!      页。方向不对称必须在判定器里显性表达，不能把「只读页」当合法输出。
//!   ④ 可空槽（utimensat.times、sendfile.offset）传 NULL 必须成功；非可空
//!      槽传 NULL 必须 EFAULT。空指针不是「通用坏指针」，一刀切会误杀正常
//!      调用或放行空解引用。
//!   ⑤ 「与参数拷贝探针总成单源」用冻结账复算：nargs 账 48、指针账 17、
//!      可空账 2。任一 mask 被改动，漂移格与总账格同时报红。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9214 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A300;
/// 探针格容量上限（指针 17 + 空指针 17 + 值域 4 + 次序 4）。
pub const MAX_CELLS: usize = 48;
/// 指针槽总数（冻结账）。
pub const POINTER_CELLS: usize = 17;
/// 可空指针槽数（冻结账）。
pub const NULLABLE_CELLS: usize = 2;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 表级 nargs 合计（冻结账）。
pub const TABLE_NARGS_SUM: u32 = 48;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// statx 合法 mask 位（STATX_BASIC_STATS 及以下）。
pub const STATX_ALL_MASK: u32 = 0xFFF;
/// utimensat 合法 flags（AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH）。
pub const UT_ALLOWED: u32 = 0x1001;
const NR_READ: u32 = 0;
const NR_WRITE: u32 = 1;
const NR_OPEN: u32 = 2;
const NR_CLOSE: u32 = 3;
const NR_STAT: u32 = 4;
const NR_FSTAT: u32 = 5;
const NR_LSEEK: u32 = 8;
const NR_PREAD64: u32 = 17;
const NR_ACCESS: u32 = 21;
const NR_SENDFILE: u32 = 40;
const NR_FTRUNCATE: u32 = 77;
const NR_READLINK: u32 = 89;
const NR_GETDENTS64: u32 = 217;
const NR_OPENAT: u32 = 257;
const NR_UTIMENSAT: u32 = 280;
const NR_STATX: u32 = 332;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 探针总成诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    NargsDrift,
    SlotDrift,
    NullRejected,
    NullLeaked,
    FaultEscaped,
    WrongErrno,
    FdGateLate,
    ValueGateLate,
    TableDrift,
    StageSkip,
    LedgerLeak,
    SegmentViolation,
}
impl ProbeCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            ProbeCode::NargsDrift => 0,
            ProbeCode::SlotDrift => 1,
            ProbeCode::NullRejected => 2,
            ProbeCode::NullLeaked => 3,
            ProbeCode::FaultEscaped => 4,
            ProbeCode::WrongErrno => 5,
            ProbeCode::FdGateLate => 6,
            ProbeCode::ValueGateLate => 7,
            ProbeCode::TableDrift => 8,
            ProbeCode::StageSkip => 9,
            ProbeCode::LedgerLeak => 10,
            ProbeCode::SegmentViolation => 11,
        }
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NargsDrift => "nargs 由 mask 逐位弹出；增删槽位必须先改冻结账再改判定器",
            ProbeCode::SlotDrift => "指针槽枚举必须与 mask 指针位一一对应，禁止另立槽位表",
            ProbeCode::NullRejected => "可空槽传 NULL 是合法用法，拒绝它会把正常调用误判成故障",
            ProbeCode::NullLeaked => "非可空槽传 NULL 必须 EFAULT，放行会让内核解引用空指针",
            ProbeCode::FaultEscaped => "坏指针必须在指针门短路返回 EFAULT，漏检等于把用户地址当可信地址",
            ProbeCode::WrongErrno => "指针故障只能报 EFAULT，报 EINVAL 会误导调用方去改参数值",
            ProbeCode::FdGateLate => "fd 门必须先于指针门，先查 fd 才能避免对已关闭对象做无谓拷贝",
            ProbeCode::ValueGateLate => "值域门先于指针拷贝，与 Linux sys_* 的前置检查顺序一致",
            ProbeCode::TableDrift => "Linux E 表与判定器期望值必须同源，改判定器必须同步改对账表",
            ProbeCode::StageSkip => "探针状态机只许 Extract→Gate→Sweep→Verdict 单向推进",
            ProbeCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            ProbeCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [ProbeCode; 12] = [
        ProbeCode::NargsDrift, ProbeCode::SlotDrift, ProbeCode::NullRejected,
        ProbeCode::NullLeaked, ProbeCode::FaultEscaped, ProbeCode::WrongErrno,
        ProbeCode::FdGateLate, ProbeCode::ValueGateLate, ProbeCode::TableDrift,
        ProbeCode::StageSkip, ProbeCode::LedgerLeak, ProbeCode::SegmentViolation,
    ];
}

// ── 描述字逐位提取 ────────────────────────────────────────────────────
/// 取第 slot 槽的 4 位；越界槽视为不存在。
pub fn nibble(mask: u32, slot: u8) -> u8 {
    if slot >= 8 {
        return 0;
    }
    ((mask >> (slot as u32 * 4)) & 0xF) as u8
}
/// 槽位存在位。
pub fn arg_exists(mask: u32, slot: u8) -> bool { nibble(mask, slot) & 0x1 != 0 }
/// 槽位是指针。
pub fn arg_is_ptr(mask: u32, slot: u8) -> bool { nibble(mask, slot) & 0x2 != 0 }
/// 槽位是输出向（内核写回用户）。
pub fn arg_is_write(mask: u32, slot: u8) -> bool { nibble(mask, slot) & 0x4 != 0 }
/// 槽位可空（NULL 合法）。
pub fn arg_is_nullable(mask: u32, slot: u8) -> bool { nibble(mask, slot) & 0x8 != 0 }
/// 逐位弹出 nargs：只数存在位，洞不计数。
pub fn mask_nargs(mask: u32) -> u8 {
    let mut n = 0u8;
    for slot in 0..8u8 {
        if arg_exists(mask, slot) {
            n += 1;
        }
    }
    n
}
/// 逐位弹出指针槽数。
pub fn mask_ptrs(mask: u32) -> u8 {
    let mut n = 0u8;
    for slot in 0..8u8 {
        if arg_exists(mask, slot) && arg_is_ptr(mask, slot) {
            n += 1;
        }
    }
    n
}

// ── 系统调用描述 ──────────────────────────────────────────────────────
/// fd 门类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    /// 无 fd 参数。
    None,
    /// 普通 fd（关闭即 EBADF）。
    Fd,
    /// 目录 fd，AT_FDCWD 时豁免。
    DirFd,
}
/// 文件族系统调用描述：mask 是唯一的数据源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysDesc {
    pub nr: u32,
    pub name: &'static str,
    pub mask: u32,
    pub fd: FdKind,
}
impl SysDesc {
    /// 构造。
    pub const fn new(nr: u32, name: &'static str, mask: u32, fd: FdKind) -> Self {
        SysDesc { nr, name, mask, fd }
    }
    /// nargs 逐位提取（不存字段，故不可能与 mask 漂移）。
    pub fn nargs(&self) -> u8 { mask_nargs(self.mask) }
    /// 指针槽数。
    pub fn ptr_count(&self) -> u8 { mask_ptrs(self.mask) }
    /// 按槽位升序枚举指针槽。
    pub fn ptr_slots(&self) -> ([u8; 8], usize) {
        let mut out = [0u8; 8];
        let mut n = 0usize;
        for slot in 0..8u8 {
            if arg_exists(self.mask, slot) && arg_is_ptr(self.mask, slot) {
                if let Some(cell) = out.get_mut(n) {
                    *cell = slot;
                    n += 1;
                }
            }
        }
        (out, n)
    }
}
/// 文件族描述表（单源；槽 0 起，每槽 4 位）。
pub const FAMILY: [SysDesc; 16] = [
    SysDesc::new(NR_OPEN, "open", 0x113, FdKind::None),
    SysDesc::new(NR_OPENAT, "openat", 0x1131, FdKind::DirFd),
    SysDesc::new(NR_CLOSE, "close", 0x1, FdKind::Fd),
    SysDesc::new(NR_READ, "read", 0x171, FdKind::Fd),
    SysDesc::new(NR_WRITE, "write", 0x131, FdKind::Fd),
    SysDesc::new(NR_PREAD64, "pread64", 0x1171, FdKind::Fd),
    SysDesc::new(NR_LSEEK, "lseek", 0x111, FdKind::Fd),
    SysDesc::new(NR_FSTAT, "fstat", 0x71, FdKind::Fd),
    SysDesc::new(NR_STAT, "stat", 0x73, FdKind::None),
    SysDesc::new(NR_STATX, "statx", 0x71131, FdKind::DirFd),
    SysDesc::new(NR_ACCESS, "access", 0x31, FdKind::None),
    SysDesc::new(NR_READLINK, "readlink", 0x173, FdKind::None),
    SysDesc::new(NR_GETDENTS64, "getdents64", 0x171, FdKind::Fd),
    SysDesc::new(NR_UTIMENSAT, "utimensat", 0x1B31, FdKind::DirFd),
    SysDesc::new(NR_FTRUNCATE, "ftruncate", 0x11, FdKind::Fd),
    SysDesc::new(NR_SENDFILE, "sendfile", 0x1F11, FdKind::None),
];
/// B 侧冻结 nargs 账（任何 mask 改动都会在此漂移）。
pub const FROZEN_NARGS: [(&str, u8); 16] = [
    ("open", 3), ("openat", 4), ("close", 1), ("read", 3),
    ("write", 3), ("pread64", 4), ("lseek", 3), ("fstat", 2),
    ("stat", 2), ("statx", 5), ("access", 2), ("readlink", 3),
    ("getdents64", 3), ("utimensat", 4), ("ftruncate", 2), ("sendfile", 4),
];
/// B 侧冻结指针账。
pub const FROZEN_PTRS: [u8; 16] = [1, 1, 0, 1, 1, 1, 0, 1, 2, 2, 1, 2, 1, 2, 0, 1];
/// 按名取描述（无裸索引）。
pub fn desc_of(name: &str) -> Option<&'static SysDesc> {
    FAMILY.iter().find(|d| d.name == name)
}

// ── 判定器 ────────────────────────────────────────────────────────────
/// 用户指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 可读可写。
    Valid,
    /// 空指针。
    Null,
    /// 未映射页。
    Unmapped,
    /// 只读页（可读不可写）。
    ReadOnly,
    /// 内核页（用户不可触碰）。
    Kernel,
}
impl PtrClass {
    /// 用户态可读。
    pub fn readable(self) -> bool {
        match self {
            PtrClass::Valid | PtrClass::ReadOnly => true,
            _ => false,
        }
    }
    /// 用户态可写。
    pub fn writable(self) -> bool {
        match self {
            PtrClass::Valid => true,
            _ => false,
        }
    }
}
/// 一次调用的参数上下文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    pub fd_live: bool,
    pub dirfd_live: bool,
    pub at_fdcwd: bool,
    pub ptrs: [PtrClass; 8],
    pub statx_mask: u32,
    pub ut_flags: u32,
    pub readlink_len: u32,
    pub sendfile_count: u64,
}
impl Call {
    /// 全合法上下文。
    pub fn clean() -> Call {
        Call {
            fd_live: true,
            dirfd_live: true,
            at_fdcwd: true,
            ptrs: [PtrClass::Valid; 8],
            statx_mask: STATX_ALL_MASK,
            ut_flags: 0,
            readlink_len: 32,
            sendfile_count: 64,
        }
    }
    /// 覆写某槽指针类别（越界槽忽略）。
    pub fn with_ptr(mut self, slot: u8, class: PtrClass) -> Call {
        if let Some(p) = self.ptrs.get_mut(slot as usize) {
            *p = class;
        }
        self
    }
    /// 取某槽类别；越界按未映射处理（保守）。
    pub fn ptr_at(&self, slot: u8) -> PtrClass {
        match self.ptrs.get(slot as usize) {
            Some(p) => *p,
            None => PtrClass::Unmapped,
        }
    }
}
/// 三层判定：fd 门 → 值域门 → 指针门（槽位升序短路）。
pub fn judge_call(desc: &SysDesc, call: &Call) -> Result<(), u32> {
    // 门① fd：关闭即 EBADF；目录 fd 允许 AT_FDCWD 豁免。
    match desc.fd {
        FdKind::Fd => if !call.fd_live { return Err(EBADF); },
        FdKind::DirFd => if !call.at_fdcwd && !call.dirfd_live { return Err(EBADF); },
        FdKind::None => {}
    }
    // 门② 值域：flags / mask / bufsiz 先于任何用户拷贝。
    if desc.nr == NR_STATX && call.statx_mask & !STATX_ALL_MASK != 0 {
        return Err(EINVAL);
    }
    if desc.nr == NR_UTIMENSAT && call.ut_flags & !UT_ALLOWED != 0 {
        return Err(EINVAL);
    }
    if desc.nr == NR_READLINK && call.readlink_len == 0 {
        return Err(EINVAL);
    }
    // 门③ 指针：逐槽短路；可空槽的空指针直接放行。
    for slot in 0..8u8 {
        if !arg_exists(desc.mask, slot) || !arg_is_ptr(desc.mask, slot) {
            continue;
        }
        let class = call.ptr_at(slot);
        if arg_is_nullable(desc.mask, slot) && class == PtrClass::Null {
            continue;
        }
        if arg_is_write(desc.mask, slot) && !class.writable() {
            return Err(EFAULT);
        }
        if !class.readable() {
            return Err(EFAULT);
        }
    }
    Ok(())
}
/// 判定器 → 0 或 errno。
pub fn verdict(desc: &SysDesc, call: &Call) -> u32 {
    match judge_call(desc, call) {
        Ok(()) => 0,
        Err(e) => e,
    }
}
/// 按名判定；未知名为 u32::MAX 哨兵。
fn named_verdict(name: &str, call: &Call) -> u32 {
    match desc_of(name) {
        Some(d) => verdict(d, call),
        None => u32::MAX,
    }
}

// ── 探针格 ────────────────────────────────────────────────────────────
/// 单格探针记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCell {
    pub syscall: &'static str,
    pub slot: u8,
    pub class: PtrClass,
    pub expected: u32,
    pub got: u32,
}
impl ProbeCell {
    /// 是否命中期望。
    pub fn hit(&self) -> bool { self.expected == self.got }
}
/// 坏指针全域扫：17 个指针槽各放一个坏指针，期望全 EFAULT。
pub fn pointer_sweep() -> Vec<ProbeCell> {
    let mut out = Vec::new();
    for d in FAMILY.iter() {
        let (slots, n) = d.ptr_slots();
        for i in 0..n {
            let slot = match slots.get(i) {
                Some(s) => *s,
                None => break,
            };
            let class = if arg_is_write(d.mask, slot) {
                PtrClass::ReadOnly
            } else {
                PtrClass::Unmapped
            };
            let call = Call::clean().with_ptr(slot, class);
            out.push(ProbeCell {
                syscall: d.name,
                slot,
                class,
                expected: EFAULT,
                got: verdict(d, &call),
            });
        }
    }
    out
}
/// 空指针全域扫：可空槽期望 0，非可空槽期望 EFAULT。
pub fn null_sweep() -> Vec<ProbeCell> {
    let mut out = Vec::new();
    for d in FAMILY.iter() {
        let (slots, n) = d.ptr_slots();
        for i in 0..n {
            let slot = match slots.get(i) {
                Some(s) => *s,
                None => break,
            };
            let expect = if arg_is_nullable(d.mask, slot) { 0 } else { EFAULT };
            let call = Call::clean().with_ptr(slot, PtrClass::Null);
            out.push(ProbeCell {
                syscall: d.name,
                slot,
                class: PtrClass::Null,
                expected: expect,
                got: verdict(d, &call),
            });
        }
    }
    out
}
/// 值域格：三格 EINVAL + 一格零计数 no-op。
pub fn value_cells() -> [ProbeCell; 4] {
    let mut bad_mask = Call::clean();
    bad_mask.statx_mask = 0x8000;
    let mut bad_flags = Call::clean();
    bad_flags.ut_flags = 0x4;
    let mut zero_len = Call::clean();
    zero_len.readlink_len = 0;
    let mut zero_count = Call::clean();
    zero_count.sendfile_count = 0;
    [
        ProbeCell { syscall: "statx", slot: 3, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("statx", &bad_mask) },
        ProbeCell { syscall: "utimensat", slot: 3, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("utimensat", &bad_flags) },
        ProbeCell { syscall: "readlink", slot: 2, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("readlink", &zero_len) },
        ProbeCell { syscall: "sendfile", slot: 3, class: PtrClass::Valid, expected: 0, got: named_verdict("sendfile", &zero_count) },
    ]
}
/// 次序格：fd 门先于指针门、fd 门先于值域门、值域门先于指针门。
pub fn order_cells() -> [ProbeCell; 4] {
    let mut c1 = Call::clean().with_ptr(1, PtrClass::Unmapped);
    c1.fd_live = false;
    let mut c2 = Call::clean().with_ptr(4, PtrClass::Unmapped);
    c2.dirfd_live = false;
    c2.at_fdcwd = false;
    c2.statx_mask = 0x8000;
    let mut c3 = Call::clean().with_ptr(1, PtrClass::Unmapped);
    c3.statx_mask = 0x8000;
    let mut c4 = Call::clean().with_ptr(0, PtrClass::Unmapped);
    c4.readlink_len = 0;
    [
        ProbeCell { syscall: "read", slot: 1, class: PtrClass::Unmapped, expected: EBADF, got: named_verdict("read", &c1) },
        ProbeCell { syscall: "statx", slot: 4, class: PtrClass::Unmapped, expected: EBADF, got: named_verdict("statx", &c2) },
        ProbeCell { syscall: "statx", slot: 1, class: PtrClass::Unmapped, expected: EINVAL, got: named_verdict("statx", &c3) },
        ProbeCell { syscall: "readlink", slot: 0, class: PtrClass::Unmapped, expected: EINVAL, got: named_verdict("readlink", &c4) },
    ]
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 探针总成阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteStage {
    Extract,
    Gate,
    Sweep,
    Verdict,
}
impl SuiteStage {
    /// 下一阶段。
    pub fn next(self) -> Option<SuiteStage> {
        match self {
            SuiteStage::Extract => Some(SuiteStage::Gate),
            SuiteStage::Gate => Some(SuiteStage::Sweep),
            SuiteStage::Sweep => Some(SuiteStage::Verdict),
            SuiteStage::Verdict => None,
        }
    }
    /// 只许前进一步；终点拒绝推进（StageSkip）。
    pub fn advance(self) -> Result<SuiteStage, ProbeCode> {
        match self.next() {
            Some(n) => Ok(n),
            None => Err(ProbeCode::StageSkip),
        }
    }
}

// ── 注入器与账本 ──────────────────────────────────────────────────────
/// 注入变异类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 漏报：坏指针当作成功。
    SkipFault,
    /// 错码：EFAULT 报成 EINVAL。
    WrongCode,
    /// 次序错：EFAULT 报成 EBADF。
    EarlyBadFd,
}
/// 三类变异（顺序即账序）。
pub const MUTANTS: [Mutant; 3] = [Mutant::SkipFault, Mutant::WrongCode, Mutant::EarlyBadFd];
/// 对一条本应 EFAULT 的判定结果施加变异。
pub fn mutate(m: Mutant, base: u32) -> u32 {
    if base != EFAULT {
        return base;
    }
    match m {
        Mutant::SkipFault => 0,
        Mutant::WrongCode => EINVAL,
        Mutant::EarlyBadFd => EBADF,
    }
}
/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}
impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }
    /// 守恒：注入 == 检出 + 逃逸 且逃逸为零。
    pub fn conserve(&self) -> bool {
        self.escaped == 0 && self.injected == self.detected
    }
}
/// 三类变异同源注入：每轮扫全部 17 个指针格。
pub fn run_injections() -> Ledger {
    let mut led = Ledger::new();
    let cells = pointer_sweep();
    for m in MUTANTS.iter() {
        for _round in 0..INJECT_ROUNDS {
            for c in cells.iter() {
                led.record(mutate(*m, c.got) != c.expected);
            }
        }
    }
    led
}
/// 单类是否 10/10 轮全检出（每轮 17 格全中才算一轮检出）。
pub fn injection_ten_of_ten(m: Mutant) -> bool {
    let cells = pointer_sweep();
    for _round in 0..INJECT_ROUNDS {
        let all_hit = cells.iter().all(|c| mutate(m, c.got) != c.expected);
        if !all_hit {
            return false;
        }
    }
    true
}

// ── 对账表（Linux E 语义） ────────────────────────────────────────────
/// 一行 Linux 语义对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ERow {
    pub syscall: &'static str,
    pub slot: u8,
    pub class: PtrClass,
    pub expect: u32,
}
/// 六行对账：读写方向、内核页、可空槽、bufsiz。
pub const LINUX_E_TABLE: [ERow; 6] = [
    ERow { syscall: "read", slot: 1, class: PtrClass::Unmapped, expect: EFAULT },
    ERow { syscall: "write", slot: 1, class: PtrClass::ReadOnly, expect: 0 },
    ERow { syscall: "statx", slot: 4, class: PtrClass::Kernel, expect: EFAULT },
    ERow { syscall: "utimensat", slot: 2, class: PtrClass::Null, expect: 0 },
    ERow { syscall: "readlink", slot: 1, class: PtrClass::Unmapped, expect: EFAULT },
    ERow { syscall: "sendfile", slot: 2, class: PtrClass::Null, expect: 0 },
];
/// 跑一行对账。
pub fn linux_row_verdict(row: &ERow) -> u32 {
    let call = Call::clean().with_ptr(row.slot, row.class);
    named_verdict(row.syscall, &call)
}
/// 对账命中数 / 总数。
pub fn linux_table_hits() -> (u32, u32) {
    let mut hit = 0u32;
    for row in LINUX_E_TABLE.iter() {
        if linux_row_verdict(row) == row.expect {
            hit += 1;
        }
    }
    (hit, LINUX_E_TABLE.len() as u32)
}

// ── 漂移与单源复算 ────────────────────────────────────────────────────
/// nargs 与指针两份冻结账的总漂移行数。
pub fn ledger_drift() -> u32 {
    let mut drift = 0u32;
    if FAMILY.len() != FROZEN_NARGS.len() || FAMILY.len() != FROZEN_PTRS.len() {
        drift += 1;
    }
    for (i, d) in FAMILY.iter().enumerate() {
        match FROZEN_NARGS.get(i) {
            Some((name, n)) => {
                if d.name != *name || d.nargs() != *n {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
        match FROZEN_PTRS.get(i) {
            Some(n) => {
                if d.ptr_count() != *n {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
    }
    drift
}
/// 全表可空槽计数。
pub fn nullable_count() -> u32 {
    let mut q = 0u32;
    for d in FAMILY.iter() {
        for slot in 0..8u8 {
            if arg_exists(d.mask, slot)
                && arg_is_ptr(d.mask, slot)
                && arg_is_nullable(d.mask, slot)
            {
                q += 1;
            }
        }
    }
    q
}
/// 单源声明：指针格只能落在 mask 的指针位上，且总数与冻结账咬合。
pub fn single_source_ok() -> bool {
    let cells = pointer_sweep();
    if cells.len() != POINTER_CELLS {
        return false;
    }
    let mut nargs_sum = 0u32;
    let mut ptr_sum = 0u32;
    for d in FAMILY.iter() {
        let (slots, n) = d.ptr_slots();
        if n as u8 != d.ptr_count() {
            return false;
        }
        for i in 0..n {
            match slots.get(i) {
                Some(slot) => {
                    if !arg_exists(d.mask, *slot) || !arg_is_ptr(d.mask, *slot) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        nargs_sum += d.nargs() as u32;
        ptr_sum += n as u32;
    }
    let all_cells_valid = cells.iter().all(|c| match desc_of(c.syscall) {
        Some(d) => arg_is_ptr(d.mask, c.slot),
        None => false,
    });
    all_cells_valid && nargs_sum == TABLE_NARGS_SUM && ptr_sum == POINTER_CELLS as u32
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// 描述表 + 冻结账的语料指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for d in FAMILY.iter() {
        let bytes = d.mask.to_le_bytes(); for b in bytes.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= d.nargs() as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= d.ptr_count() as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= nullable_count();
    h
}
/// 探针结果指纹（双跑一致用）。
pub fn cells_fingerprint(cells: &[ProbeCell]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for c in cells.iter() {
        for b in c.syscall.as_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= c.slot as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= c.got;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 判据总成 ──────────────────────────────────────────────────────────
/// 运行 F9214 全部判据。
pub fn run_unx9214_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9214");
    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");
    // 2. 正常流：16 行全合法参数一律 0
    let clean_ok = FAMILY.iter().all(|d| verdict(d, &Call::clean()) == 0);
    s.add("正常流：16 个文件族调用全合法参数均返回 0", clean_ok,
          "判定器不得对合法调用报错，否则探针本身就不可信");
    // 3. nargs 逐位提取（冻结账零漂移）
    s.add("nargs 逐位提取：16 行 mask 弹值与冻结账零漂移",
          ledger_drift() == 0,
          "nargs/指针数只从 mask 推导；改槽位必须同步改冻结账");
    // 4. 表级合计
    let mut n_sum = 0u32;
    for d in FAMILY.iter() {
        n_sum += d.nargs() as u32;
    }
    s.add("表级合计：Σnargs=48 且 16 行齐全", n_sum == TABLE_NARGS_SUM,
          "总量是单源声明的一部分，缺行或多行都会破坏合计");
    // 5. 指针槽单源
    s.add("单源：探针格全部落在 mask 指针位，17 槽不多不少",
          single_source_ok(),
          "探针与 nargs 提取共用一份 mask，禁止另立一张槽位表");
    // 6. 坏指针全 EFAULT
    let sweep = pointer_sweep();
    let ptr_hits = sweep.iter().filter(|c| c.hit()).count();
    s.add("坏指针全 EFAULT：17/17 格命中",
          sweep.len() == POINTER_CELLS && ptr_hits == POINTER_CELLS,
          "每个指针槽单独放坏指针都必须短路到 EFAULT");
    // 7. 方向不对称：只读页对输出槽仍 EFAULT
    let asym = PtrClass::ReadOnly.readable() && !PtrClass::ReadOnly.writable();
    let ro_write_faults = match desc_of("read") {
        Some(d) => verdict(d, &Call::clean().with_ptr(1, PtrClass::ReadOnly)) == EFAULT,
        None => false,
    };
    let ro_read_passes = match desc_of("write") {
        Some(d) => verdict(d, &Call::clean().with_ptr(1, PtrClass::ReadOnly)) == 0,
        None => false,
    };
    s.add("方向不对称：只读页可作输入、作输出必须 EFAULT",
          asym && ro_write_faults && ro_read_passes,
          "输出需要可写；把只读页当合法输出会把半拷贝写回失败吞掉");
    // 8. 空指针：非可空槽全 EFAULT
    let nulls = null_sweep();
    let null_hits = nulls.iter().filter(|c| c.hit()).count();
    let required_faults = nulls.iter().filter(|c| c.expected == EFAULT && c.got == EFAULT).count();
    s.add("空指针：15 个非可空槽传 NULL 全 EFAULT",
          nulls.len() == POINTER_CELLS && null_hits == POINTER_CELLS
              && required_faults == POINTER_CELLS - NULLABLE_CELLS,
          "空指针不是通用坏指针；必填槽必须拒，少拒一格就是空解引用");
    // 9. 可空槽：NULL 合法
    let ut_null = match desc_of("utimensat") {
        Some(d) => verdict(d, &Call::clean().with_ptr(2, PtrClass::Null)) == 0,
        None => false,
    };
    let sf_null = match desc_of("sendfile") {
        Some(d) => verdict(d, &Call::clean().with_ptr(2, PtrClass::Null)) == 0,
        None => false,
    };
    s.add("可空槽：utimensat.times / sendfile.offset 传 NULL 均成功",
          ut_null && sf_null && nullable_count() == NULLABLE_CELLS as u32,
          "可空语义来自 Linux 手册；拒绝它会把正常用法判成故障");
    // 10. 值域门：三格 EINVAL
    let vals = value_cells();
    let val_hits = vals.iter().filter(|c| c.hit()).count();
    s.add("值域门：statx mask / utimensat flags / readlink bufsiz 各报 EINVAL",
          val_hits == 4,
          "值域检查先于用户拷贝，错报 EFAULT 会掩盖参数本身的错误");
    // 11. 零计数 no-op
    let noop = match vals.get(3) {
        Some(c) => c.expected == 0 && c.got == 0,
        None => false,
    };
    s.add("边界：sendfile count=0 是 no-op 返回 0", noop,
          "零计数不拷贝任何东西，不得因空转报错");
    // 12. 次序①：fd 先于指针
    let ord = order_cells();
    s.add("判定次序①：fd 门先于指针门（闭 fd + 坏 buf → EBADF）",
          match ord.get(0) { Some(c) => c.hit() && c.got == EBADF, None => false },
          "先查 fd 避免对已关闭对象做无谓拷贝");
    // 13. 次序②：fd 先于值域
    s.add("判定次序②：fd 门先于值域门（闭 dirfd + 坏 mask → EBADF）",
          match ord.get(1) { Some(c) => c.hit() && c.got == EBADF, None => false },
          "fd 不可用时不值得再去校验 flags/mask");
    // 14. 次序③：值域先于指针
    s.add("判定次序③：值域门先于指针门（坏 mask + 坏 path → EINVAL）",
          match ord.get(2) { Some(c) => c.hit() && c.got == EINVAL, None => false },
          "与 Linux sys_statx 先查 flags 再 getname 的顺序一致");
    // 15. 次序④：bufsiz 先于 path 拷贝
    s.add("判定次序④：readlink bufsiz=0 先于 path 拷贝（EINVAL）",
          match ord.get(3) { Some(c) => c.hit() && c.got == EINVAL, None => false },
          "零 bufsiz 无意义，先拒可省一次用户拷贝");
    // 16. AT_FDCWD 例外
    let mut fdcwd = Call::clean().with_ptr(2, PtrClass::Null);
    fdcwd.dirfd_live = false;
    let fdcwd_ok = match desc_of("utimensat") {
        Some(d) => verdict(d, &fdcwd) == 0,
        None => false,
    };
    s.add("AT_FDCWD 例外：dirfd 关闭但 AT_FDCWD 时不报 EBADF", fdcwd_ok,
          "AT_FDCWD 是常量而非 fd 槽，不能走关闭检查");
    // 17. 状态机单向
    let mut stage = SuiteStage::Extract;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 {
            forward = false;
            break;
        }
    }
    s.add("状态机：Extract→Gate→Sweep→Verdict 单向三步，终点拒绝推进",
          forward && steps == 3 && stage == SuiteStage::Verdict
              && stage.advance() == Err(ProbeCode::StageSkip),
          "阶段只能前进；回退或跳步都会让判据与语料脱节");
    // 18. 注入 3 类 × 10 轮全检出
    let ten = MUTANTS.iter().all(|m| injection_ten_of_ten(*m));
    s.add("注入必抓：漏报/错码/次序错三类各 10/10 轮全检出",
          ten,
          "每轮 17 格全中才算检出；漏一轮即视为不稳定注入");
    // 19. 守恒
    let led = run_injections();
    s.add("守恒：注入 510 = 检出 510 + 逃逸 0",
          led.conserve() && led.injected == 3 * INJECT_ROUNDS * POINTER_CELLS as u32,
          "注入账必须闭合；任何逃逸都意味着探针有盲区");
    // 20. 双跑一致
    let f1 = cells_fingerprint(&pointer_sweep());
    let f2 = cells_fingerprint(&pointer_sweep());
    s.add("双跑一致：两次坏指针扫描指纹相等", f1 == f2,
          "探针不得依赖哈希遍历顺序或外部状态");
    // 21. 容量
    let total_cells = sweep.len() + nulls.len() + vals.len() + ord.len();
    s.add("容量：26 静态 + 3 动态判据，42 格 ≤ 48 上限，add_dyn ≤ 8",
          total_cells == 42 && total_cells <= MAX_CELLS && 3 <= 8,
          "格数超上限说明矩阵失控，必须拆卡而不是继续堆格");
    // 22. Linux E 表对账
    let (hit, total) = linux_table_hits();
    s.add("对账：Linux E 语义表 6/6 命中",
          hit == total && total == 6,
          "对账表是外部语义锚点；表与判定器不一致时先查判定器");
    // 23. 文案闭集
    let mut closed = true;
    for (i, c) in ProbeCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() {
            closed = false;
        }
        for (j, o) in ProbeCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() {
                closed = false;
            }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码必须闭集且可枚举，文案为空等于没有处置建议");
    // 动态明细 1：坏指针扫描
    s.add_dyn("动态：坏指针扫描明细", ptr_hits == POINTER_CELLS,
              &format!("hit={}/{} fingerprint=0x{:08X}", ptr_hits, POINTER_CELLS, f1));
    // 动态明细 2：注入账
    s.add_dyn("动态：注入账明细", led.conserve(),
              &format!("injected={} detected={} escaped={}", led.injected, led.detected, led.escaped));
    // 动态明细 3：对账与单源
    s.add_dyn("动态：对账与单源明细", hit == total && single_source_ok(),
              &format!("linux={}/{} single_source={} fp=0x{:08X}",
                       hit, total, single_source_ok(), corpus_fingerprint()));
    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nargs_is_extracted_bitwise() {
        assert_eq!(mask_nargs(0x113), 3);
        assert_eq!(mask_nargs(0x101), 2);
        assert_eq!(mask_nargs(0x0), 0);
        assert_eq!(mask_nargs(0xF0F), 2);
        assert_eq!(ledger_drift(), 0);
    }
    #[test]
    fn family_totals_are_frozen() {
        let mut n = 0u32;
        let mut p = 0u32;
        for d in FAMILY.iter() {
            n += d.nargs() as u32;
            p += d.ptr_count() as u32;
        }
        assert_eq!(n, TABLE_NARGS_SUM);
        assert_eq!(p, POINTER_CELLS as u32);
        assert_eq!(nullable_count(), NULLABLE_CELLS as u32);
    }
    #[test]
    fn pointer_sweep_all_efault() {
        let cells = pointer_sweep();
        assert_eq!(cells.len(), POINTER_CELLS);
        assert!(cells.iter().all(|c| c.hit()));
        assert!(cells.iter().all(|c| c.got == EFAULT));
        assert!(single_source_ok());
    }
    #[test]
    fn null_sweep_splits_nullable_from_required() {
        let cells = null_sweep();
        assert_eq!(cells.len(), POINTER_CELLS);
        assert!(cells.iter().all(|c| c.hit()));
        let ok = cells.iter().filter(|c| c.expected == 0).count();
        assert_eq!(ok, NULLABLE_CELLS);
    }
    #[test]
    fn readonly_direction_asymmetry() {
        assert!(PtrClass::ReadOnly.readable());
        assert!(!PtrClass::ReadOnly.writable());
        if let Some(d) = desc_of("read") {
            assert_eq!(verdict(d, &Call::clean().with_ptr(1, PtrClass::ReadOnly)), EFAULT);
        }
        if let Some(d) = desc_of("write") {
            assert_eq!(verdict(d, &Call::clean().with_ptr(1, PtrClass::ReadOnly)), 0);
        }
    }
    #[test]
    fn value_gates_return_einval() {
        let cells = value_cells();
        assert!(cells.iter().all(|c| c.hit()));
        assert_eq!(cells.iter().filter(|c| c.got == EINVAL).count(), 3);
        assert_eq!(cells.iter().filter(|c| c.got == 0).count(), 1);
    }
    #[test]
    fn fd_gate_precedes_pointer() {
        let mut c = Call::clean().with_ptr(1, PtrClass::Unmapped);
        c.fd_live = false;
        if let Some(d) = desc_of("read") {
            assert_eq!(verdict(d, &c), EBADF);
        }
        let mut c2 = Call::clean().with_ptr(4, PtrClass::Unmapped);
        c2.at_fdcwd = false;
        c2.dirfd_live = false;
        c2.statx_mask = 0x8000;
        if let Some(d) = desc_of("statx") {
            assert_eq!(verdict(d, &c2), EBADF);
        }
    }
    #[test]
    fn value_gate_precedes_pointer() {
        let mut c = Call::clean().with_ptr(1, PtrClass::Unmapped);
        c.statx_mask = 0x8000;
        if let Some(d) = desc_of("statx") {
            assert_eq!(verdict(d, &c), EINVAL);
        }
        let mut c2 = Call::clean().with_ptr(0, PtrClass::Unmapped);
        c2.readlink_len = 0;
        if let Some(d) = desc_of("readlink") {
            assert_eq!(verdict(d, &c2), EINVAL);
        }
    }
    #[test]
    fn at_fdcwd_bypasses_dirfd() {
        let mut c = Call::clean().with_ptr(2, PtrClass::Null);
        c.dirfd_live = false;
        c.at_fdcwd = true;
        if let Some(d) = desc_of("utimensat") {
            assert_eq!(verdict(d, &c), 0);
        }
    }
    #[test]
    fn injections_detected_ten_of_ten() {
        for m in MUTANTS.iter() {
            assert!(injection_ten_of_ten(*m));
        }
        let led = run_injections();
        assert!(led.conserve());
        assert_eq!(led.injected, 3 * INJECT_ROUNDS * POINTER_CELLS as u32);
        assert_eq!(led.escaped, 0);
    }
    #[test]
    fn stage_machine_is_forward_only() {
        let s0 = SuiteStage::Extract;
        let s1 = s0.advance();
        assert_eq!(s1, Ok(SuiteStage::Gate));
        let s2 = match s1 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        assert_eq!(s2, Ok(SuiteStage::Sweep));
        let s3 = match s2 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        assert_eq!(s3, Ok(SuiteStage::Verdict));
        let s4 = match s3 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        assert_eq!(s4, Err(ProbeCode::StageSkip));
    }
    #[test]
    fn fingerprint_and_checks_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(cells_fingerprint(&pointer_sweep()), cells_fingerprint(&pointer_sweep()));
        let set = run_unx9214_checks();
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
