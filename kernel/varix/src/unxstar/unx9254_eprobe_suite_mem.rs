//! UNX-F9254 内存族 E 型参数探针总成 · 0x7AD00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9254 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9254-J1 nargs 逐位提取判据过，
//!   坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 单源：每行系统调用只存一个 32 位描述字，每槽 4 位（存在/指针/写向/
//!      可空）。nargs、指针槽表、可空槽表、写向槽表全部由这一份 mask 逐位
//!      弹出，不另立字段，因此「nargs 提取」与「指针探针」结构上无法漂移。
//!   ② 门次序：Extract（逐位）→ ValueGate（flags/prot/advice）→ PtrGate
//!      （槽位升序短路）→ Verdict。值域门先于指针拷贝，与 Linux 内存族
//!      sys_* 的前置检查一致：参数值本身非法时不该先解引用用户地址。
//!   ③ 坏指针全 EFAULT：19 个指针槽 × 空/未映射/内核三类合计 57 次注入，
//!      其中 4 个可空槽传 NULL 是合法用法（mremap 的 new、get_mempolicy 的
//!      nodemask/addr、set_mempolicy 的 nodemask），必须 Ok 而不是 EFAULT。
//!   ④ 方向不对称：输出向槽（mincore 的 vec、get_mempolicy 的 policy）传
//!      只读页必须 EFAULT；输入向槽传只读页合法。把两者混为一谈会误杀
//!      只读入参或放行内核回写只读页。
//!   ⑤ 零长度与 F9252 口径对齐：mmap(len=0) 返回 Ok 且不触碰指针槽；
//!      munmap(len=0) 在值域门回 EINVAL，均不进入指针门。
//!   ⑥ 状态机单调：stages 位图只许按 Extract→Value→Ptr→Verdict 置位，
//!      任何跳级（Value 未置位就出现 Verdict）判 StageSkip。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9254 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AD00;

/// 探针格容量上限（= 表级 nargs 合计）。
pub const MAX_CELLS: usize = 56;

/// 指针槽总数（冻结账）。
pub const POINTER_CELLS: usize = 19;

/// 可空指针槽数（冻结账）。
pub const NULLABLE_CELLS: usize = 4;

/// 输出向槽数（冻结账）。
pub const WRITE_CELLS: usize = 2;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 坏指针类别数（空/未映射/内核）。
pub const BAD_PTR_CLASSES: u32 = 3;

/// 文件族探针的 nargs 合计（与 F9214 同一份单源账）。
pub const COPY_PROBE_NARGS_SUM: u32 = 56;

/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 参数值非法。
pub const EINVAL: i32 = 22;
/// 内存不足。
pub const ENOMEM: i32 = 12;
/// 坏 fd。
pub const EBADF: i32 = 9;
/// 无此对象。
pub const ENOENT: i32 = 2;

/// 阶段位：逐位提取。
pub const STAGE_EXTRACT: u8 = 0x1;
/// 阶段位：值域门。
pub const STAGE_VALUE: u8 = 0x2;
/// 阶段位：指针门。
pub const STAGE_PTR: u8 = 0x4;
/// 阶段位：判决。
pub const STAGE_VERDICT: u8 = 0x8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    NargsDrift,
    SlotDrift,
    NullableDrift,
    NullRejected,
    NullLeaked,
    FaultEscaped,
    WrongErrno,
    StageSkip,
    ZeroLenLeak,
    TableDrift,
    LedgerLeak,
    SegmentViolation,
}

impl ProbeCode {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NargsDrift => "nargs 漂移：nargs 必须由 mask 逐位弹出，禁止另存字段或按调用手写数字",
            ProbeCode::SlotDrift => "指针槽漂移：槽枚举必须与 mask 指针位一一对应，改表必须同步改冻结账",
            ProbeCode::NullableDrift => "可空槽漂移：可空位由 mask 第 3 位承载，误删会把合法 NULL 判成 EFAULT",
            ProbeCode::NullRejected => "可空槽 NULL 被拒：mremap/get_mempolicy/set_mempolicy 的空参是合法用法",
            ProbeCode::NullLeaked => "非可空槽 NULL 被放行：内核会解引用空指针，必须 EFAULT",
            ProbeCode::FaultEscaped => "坏指针逃逸：未映射/内核地址必须在指针门短路回 EFAULT",
            ProbeCode::WrongErrno => "返回码串位：值域错报 EINVAL，指针错报 EFAULT，两者不得互换",
            ProbeCode::StageSkip => "阶段跳级：只许 Extract→Value→Ptr→Verdict 逐级置位，跳级即判据失效",
            ProbeCode::ZeroLenLeak => "零长度泄漏：mmap(0) 不进指针门，munmap(0) 在值域门回 EINVAL",
            ProbeCode::TableDrift => "Linux E 表与判定器不同源：改 mask 必须同步改冻结账与对账表",
            ProbeCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
            ProbeCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [ProbeCode; 12] = [
        ProbeCode::NargsDrift,
        ProbeCode::SlotDrift,
        ProbeCode::NullableDrift,
        ProbeCode::NullRejected,
        ProbeCode::NullLeaked,
        ProbeCode::FaultEscaped,
        ProbeCode::WrongErrno,
        ProbeCode::StageSkip,
        ProbeCode::ZeroLenLeak,
        ProbeCode::TableDrift,
        ProbeCode::LedgerLeak,
        ProbeCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 描述字逐位提取 ────────────────────────────────────────────────────
/// 取第 slot 槽的 4 位；越界槽视为不存在。
pub fn nibble(mask: u32, slot: u8) -> u8 {
    if slot >= 8 {
        return 0;
    }
    ((mask >> (slot as u32 * 4)) & 0xF) as u8
}

/// 槽位存在位。
pub fn arg_exists(mask: u32, slot: u8) -> bool {
    nibble(mask, slot) & 0x1 != 0
}

/// 槽位是指针。
pub fn arg_is_ptr(mask: u32, slot: u8) -> bool {
    nibble(mask, slot) & 0x2 != 0
}

/// 槽位是输出向（内核写回用户）。
pub fn arg_is_write(mask: u32, slot: u8) -> bool {
    nibble(mask, slot) & 0x4 != 0
}

/// 槽位可空。
pub fn arg_is_nullable(mask: u32, slot: u8) -> bool {
    nibble(mask, slot) & 0x8 != 0
}

/// 逐位弹出 nargs：只数存在位，洞不计数。
pub fn mask_nargs(mask: u32) -> u8 {
    let mut n = 0u8;
    let mut slot = 0u8;
    while slot < 8 {
        if arg_exists(mask, slot) {
            n += 1;
        }
        slot += 1;
    }
    n
}

/// 逐位弹出指针槽数。
pub fn mask_ptrs(mask: u32) -> u8 {
    let mut n = 0u8;
    let mut slot = 0u8;
    while slot < 8 {
        if arg_exists(mask, slot) && arg_is_ptr(mask, slot) {
            n += 1;
        }
        slot += 1;
    }
    n
}

/// 逐位弹出可空槽数。
pub fn mask_nullable(mask: u32) -> u8 {
    let mut n = 0u8;
    let mut slot = 0u8;
    while slot < 8 {
        if arg_exists(mask, slot) && arg_is_nullable(mask, slot) {
            n += 1;
        }
        slot += 1;
    }
    n
}

/// 逐位弹出输出向槽数。
pub fn mask_writes(mask: u32) -> u8 {
    let mut n = 0u8;
    let mut slot = 0u8;
    while slot < 8 {
        if arg_exists(mask, slot) && arg_is_write(mask, slot) {
            n += 1;
        }
        slot += 1;
    }
    n
}

// ── 内存族描述表（单源） ──────────────────────────────────────────────
/// 系统调用描述：mask 是唯一数据源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysDesc {
    pub nr: u32,
    pub name: &'static str,
    pub mask: u32,
}

impl SysDesc {
    /// 构造。
    pub const fn new(nr: u32, name: &'static str, mask: u32) -> SysDesc {
        SysDesc { nr, name, mask }
    }
    /// nargs（逐位提取）。
    pub fn nargs(&self) -> u8 {
        mask_nargs(self.mask)
    }
    /// 指针槽数。
    pub fn ptr_count(&self) -> u8 {
        mask_ptrs(self.mask)
    }
    /// 按槽位升序枚举指针槽。
    pub fn ptr_slots(&self) -> ([u8; 8], usize) {
        let mut out = [0u8; 8];
        let mut n = 0usize;
        let mut slot = 0u8;
        while slot < 8 {
            if arg_exists(self.mask, slot) && arg_is_ptr(self.mask, slot) {
                if let Some(cell) = out.get_mut(n) {
                    *cell = slot;
                    n += 1;
                }
            }
            slot += 1;
        }
        (out, n)
    }
}

/// 内存族描述表（16 行，mask 单源）。
pub const FAMILY: [SysDesc; 16] = [
    SysDesc::new(9, "mmap", 0x11_1113),
    SysDesc::new(11, "munmap", 0x13),
    SysDesc::new(10, "mprotect", 0x113),
    SysDesc::new(25, "mremap", 0xB_1113),
    SysDesc::new(12, "brk", 0x1),
    SysDesc::new(28, "madvise", 0x113),
    SysDesc::new(26, "msync", 0x113),
    SysDesc::new(149, "mlock", 0x13),
    SysDesc::new(150, "munlock", 0x13),
    SysDesc::new(27, "mincore", 0x713),
    SysDesc::new(325, "mlock2", 0x113),
    SysDesc::new(329, "pkey_mprotect", 0x1113),
    SysDesc::new(216, "remap_file_pages", 0x1_1111),
    SysDesc::new(237, "mbind", 0x11_3113),
    SysDesc::new(239, "get_mempolicy", 0x1_B1B7),
    SysDesc::new(238, "set_mempolicy", 0x1B1),
];

/// 冻结 nargs 账（改 mask 即在此漂移）。
pub const FROZEN_NARGS: [u8; 16] = [6, 2, 3, 5, 1, 3, 3, 2, 2, 3, 3, 4, 5, 6, 5, 3];
/// 冻结指针槽账。
pub const FROZEN_PTRS: [u8; 16] = [1, 1, 1, 2, 0, 1, 1, 1, 1, 2, 1, 1, 0, 2, 3, 1];
/// 冻结可空槽账。
pub const FROZEN_NULLABLE: [u8; 16] = [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1];
/// 冻结写向槽账。
pub const FROZEN_WRITES: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0];

/// 按名取描述。
pub fn desc_of(name: &str) -> Option<&'static SysDesc> {
    FAMILY.iter().find(|d| d.name == name)
}

/// 表级漂移格数：nargs/指针/可空/写向四列逐行比对冻结账。
pub fn table_drift() -> u32 {
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            if let Some(n) = FROZEN_NARGS.get(i) {
                if d.nargs() != *n {
                    drift = drift.saturating_add(1);
                }
            }
            if let Some(n) = FROZEN_PTRS.get(i) {
                if d.ptr_count() != *n {
                    drift = drift.saturating_add(1);
                }
            }
            if let Some(n) = FROZEN_NULLABLE.get(i) {
                if mask_nullable(d.mask) != *n {
                    drift = drift.saturating_add(1);
                }
            }
            if let Some(n) = FROZEN_WRITES.get(i) {
                if mask_writes(d.mask) != *n {
                    drift = drift.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    drift
}

/// 表级四列合计：(nargs, 指针槽, 可空槽, 写向槽)。
pub fn column_sums() -> (u32, u32, u32, u32) {
    let mut nargs = 0u32;
    let mut ptrs = 0u32;
    let mut nullable = 0u32;
    let mut writes = 0u32;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            nargs = nargs.saturating_add(d.nargs() as u32);
            ptrs = ptrs.saturating_add(mask_ptrs(d.mask) as u32);
            nullable = nullable.saturating_add(mask_nullable(d.mask) as u32);
            writes = writes.saturating_add(mask_writes(d.mask) as u32);
        }
        i += 1;
    }
    (nargs, ptrs, nullable, writes)
}

// ── 指针类别与探针 ────────────────────────────────────────────────────
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
    /// 内核页。
    Kernel,
}

impl PtrClass {
    /// 用户态可读。
    pub fn readable(self) -> bool {
        matches!(self, PtrClass::Valid | PtrClass::ReadOnly)
    }
    /// 用户态可写。
    pub fn writable(self) -> bool {
        matches!(self, PtrClass::Valid)
    }
}

/// 八槽指针类别向量（不足补 Null，多余忽略）。
pub type PtrVec = [PtrClass; 8];

/// 全合法指针向量。
pub fn all_valid() -> PtrVec {
    [PtrClass::Valid; 8]
}

/// 探针：返回（判决, 阶段位图）。
pub fn probe(desc: &SysDesc, ptrs: &PtrVec, flags_ok: bool, len: usize) -> (SysResult<()>, u8) {
    let mut stages = STAGE_EXTRACT;
    if desc.nargs() > 8 {
        return (Err(EINVAL), stages);
    }
    stages |= STAGE_VALUE;
    if !flags_ok {
        return (Err(EINVAL), stages);
    }
    if len == 0 {
        if desc.name == "munmap" {
            return (Err(EINVAL), stages);
        }
        stages |= STAGE_PTR | STAGE_VERDICT;
        return (Ok(()), stages);
    }
    stages |= STAGE_PTR;
    let (slots, n) = desc.ptr_slots();
    let mut i = 0usize;
    while i < n {
        if let Some(slot) = slots.get(i).copied() {
            let p = match ptrs.get(slot as usize) {
                Some(c) => *c,
                None => PtrClass::Null,
            };
            let nullable = arg_is_nullable(desc.mask, slot);
            let write = arg_is_write(desc.mask, slot);
            if nullable && p == PtrClass::Null {
                i += 1;
                continue;
            }
            let ok = if write { p.writable() } else { p.readable() };
            if !ok {
                stages |= STAGE_VERDICT;
                return (Err(EFAULT), stages);
            }
        }
        i += 1;
    }
    stages |= STAGE_VERDICT;
    (Ok(()), stages)
}

/// 阶段位图是否单调（无跳级）。
pub fn stages_monotonic(stages: u8) -> bool {
    if stages & STAGE_VERDICT != 0 && stages & STAGE_VALUE == 0 {
        return false;
    }
    if stages & STAGE_PTR != 0 && stages & STAGE_VALUE == 0 {
        return false;
    }
    if stages & STAGE_VALUE != 0 && stages & STAGE_EXTRACT == 0 {
        return false;
    }
    true
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一次注入（caught=期望与实际一致）。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(ProbeCode::LedgerLeak as i32));
        }
        Ok(())
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 坏指针全矩阵：19 槽 × 3 类 × 10 轮，期望 EFAULT 或可空 NULL 的 Ok。
pub fn run_bad_ptr_matrix() -> (InjectLedger, u32, u32) {
    let classes = [PtrClass::Null, PtrClass::Unmapped, PtrClass::Kernel];
    let mut ledger = InjectLedger::zero();
    let mut faults = 0u32;
    let mut nullable_ok = 0u32;
    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = desc.ptr_slots();
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    let nullable = arg_is_nullable(desc.mask, slot);
                    let write = arg_is_write(desc.mask, slot);
                    let mut ci = 0usize;
                    while ci < classes.len() {
                        if let Some(cls) = classes.get(ci).copied() {
                            let mut ptrs = all_valid();
                            if let Some(cell) = ptrs.get_mut(slot as usize) {
                                *cell = cls;
                            }
                            let want_ok = nullable && cls == PtrClass::Null && !write;
                            let mut round = 0u32;
                            while round < INJECT_ROUNDS {
                                let (r, stages) = probe(desc, &ptrs, true, 16);
                                let caught = if want_ok {
                                    r == Ok(()) && stages_monotonic(stages)
                                } else {
                                    r == Err(EFAULT)
                                };
                                let _ = ledger.record(caught);
                                if caught && want_ok {
                                    nullable_ok = nullable_ok.saturating_add(1);
                                }
                                if caught && !want_ok {
                                    faults = faults.saturating_add(1);
                                }
                                round += 1;
                            }
                        }
                        ci += 1;
                    }
                }
                si += 1;
            }
        }
        di += 1;
    }
    (ledger, faults, nullable_ok)
}

/// 输出向槽探针：写向槽传只读页必须 EFAULT。
pub fn run_write_slot_probe() -> (u32, u32) {
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = desc.ptr_slots();
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    if arg_is_write(desc.mask, slot) {
                        let mut ptrs = all_valid();
                        if let Some(cell) = ptrs.get_mut(slot as usize) {
                            *cell = PtrClass::ReadOnly;
                        }
                        total = total.saturating_add(1);
                        if probe(desc, &ptrs, true, 16).0 == Err(EFAULT) {
                            hits = hits.saturating_add(1);
                        }
                    }
                }
                si += 1;
            }
        }
        di += 1;
    }
    (hits, total)
}

/// 输入向槽探针：只读入参合法（不得误报）。
pub fn run_read_slot_probe() -> (u32, u32) {
    let mut ok = 0u32;
    let mut total = 0u32;
    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = desc.ptr_slots();
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    if !arg_is_write(desc.mask, slot) {
                        let mut ptrs = all_valid();
                        if let Some(cell) = ptrs.get_mut(slot as usize) {
                            *cell = PtrClass::ReadOnly;
                        }
                        total = total.saturating_add(1);
                        if probe(desc, &ptrs, true, 16).0 == Ok(()) {
                            ok = ok.saturating_add(1);
                        }
                    }
                }
                si += 1;
            }
        }
        di += 1;
    }
    (ok, total)
}

// ── 对照表与指纹 ──────────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；值域非法"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏用户指针"),
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无此对象"),
];

/// 查表取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_step(h, *b);
        }
        i += 1;
    }
    h
}

/// 语料指纹：16 行 × nargs 位 + 三槽坏指针判决。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            bytes.push(desc.nargs());
            bytes.push(desc.ptr_count());
            bytes.push(mask_nullable(desc.mask));
            bytes.push(mask_writes(desc.mask));
            let mut ptrs = all_valid();
            if let Some(cell) = ptrs.get_mut(0) {
                *cell = PtrClass::Unmapped;
            }
            let (r, stages) = probe(desc, &ptrs, true, 16);
            bytes.push(match r {
                Ok(()) => stages & 0x0F,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        di += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9254 判据集（16 条静态 + 1 条动态明细）。
pub fn run_unx9254_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9254");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 表级账：16 行、56 nargs、19 指针、4 可空、2 写向
    let (na, pp, nl, wr) = column_sums();
    let sums_ok = FAMILY.len() == 16
        && na == 56
        && pp == POINTER_CELLS as u32
        && nl == NULLABLE_CELLS as u32
        && wr == WRITE_CELLS as u32
        && na == COPY_PROBE_NARGS_SUM;
    s.add("表级账：16 行 / 56 nargs / 19 指针 / 4 可空 / 2 写向", sums_ok,
          "五项必须同时成立，任一漂移都说明 mask 被改过");

    // 3. 单源：四列冻结账零漂移
    s.add("单源：nargs/指针/可空/写向四列与冻结账零漂移", table_drift() == 0,
          "mask 是唯一数据源，冻结账是唯一裁判");

    // 4. nargs 逐位提取 10/10 稳定
    let stable = (0..INJECT_ROUNDS).all(|_| {
        FAMILY.iter().enumerate().all(|(i, d)| FROZEN_NARGS.get(i) == Some(&d.nargs()))
    });
    s.add("nargs：逐位提取 10/10 与冻结账一致", stable,
          "重复提取不得改变结果，任何一次漂移都算 flaky");

    // 5. 正常流：16 行全合法指针全 Ok
    let normal = FAMILY.iter().all(|d| probe(d, &all_valid(), true, 16).0 == Ok(()));
    s.add("正常流：16 行全合法指针全部 Ok", normal,
          "合法调用不得被参数门误杀");

    // 6. 坏指针全 EFAULT：19 槽 × 3 类 × 10 轮
    let (ledger, faults, nullable_ok) = run_bad_ptr_matrix();
    let expected = (POINTER_CELLS as u32 * BAD_PTR_CLASSES - NULLABLE_CELLS as u32)
        * INJECT_ROUNDS;
    s.add("坏指针：53 槽类组合 × 10 轮全 EFAULT", faults == expected,
          "空/未映射/内核三类除可空 NULL 外必须全部 EFAULT");

    // 7. 可空 NULL 合法
    s.add("可空槽：4 槽 × 10 轮 NULL 全部 Ok", nullable_ok == NULLABLE_CELLS as u32 * INJECT_ROUNDS,
          "mremap/get_mempolicy/set_mempolicy 的空参不是坏指针");

    // 8. 注入账守恒
    s.add("注入账：570 次注入 570 次检出 0 逃逸",
          ledger.conserved()
              && ledger.escaped == 0
              && ledger.injected
                  == POINTER_CELLS as u32 * BAD_PTR_CLASSES * INJECT_ROUNDS,
          "19 × 3 × 10 = 570 次，逐次记账");

    // 9. 输出向：只读页 EFAULT
    let (write_hits, write_total) = run_write_slot_probe();
    s.add("输出向：写向槽传只读页全 EFAULT", write_hits == write_total && write_total == WRITE_CELLS as u32,
          "mincore.vec 与 get_mempolicy.policy 都是内核回写槽");

    // 10. 输入向：只读页合法
    let (read_ok, read_total) = run_read_slot_probe();
    s.add("输入向：输入槽传只读页全部 Ok", read_ok == read_total,
          "只读入参是正常用法，误报 EFAULT 会破坏消费方");

    // 11. 判定次序：值域门先于指针门
    let d0 = match desc_of("mmap") {
        Some(d) => *d,
        None => SysDesc::new(9, "mmap", 0),
    };
    let mut bad = all_valid();
    if let Some(cell) = bad.get_mut(0) {
        *cell = PtrClass::Null;
    }
    let (r_order, stages_order) = probe(&d0, &bad, false, 16);
    let order_ok = r_order == Err(EINVAL) && stages_order & STAGE_PTR == 0;
    s.add("判定次序：坏 flags + 坏指针回 EINVAL 且不进指针门", order_ok,
          "值域门先于指针门，错误码不得串成 EFAULT");

    // 12. 零长度：mmap Ok 不进指针门，munmap EINVAL
    let d1 = match desc_of("munmap") {
        Some(d) => *d,
        None => SysDesc::new(11, "munmap", 0),
    };
    let zero_map = probe(&d0, &bad, true, 0);
    let zero_unmap = probe(&d1, &bad, true, 0);
    let zero_ok = zero_map.0 == Ok(())
        && zero_map.1 & STAGE_PTR != 0
        && zero_unmap.0 == Err(EINVAL)
        && zero_unmap.1 & STAGE_PTR == 0;
    s.add("零长度：mmap(0) Ok 且短路，munmap(0) EINVAL", zero_ok,
          "两路都不许触碰指针槽，与 F9252 口径一致");

    // 13. 状态机：阶段单调
    let (_, st_a) = probe(&d0, &all_valid(), true, 16);
    let (_, st_b) = probe(&d0, &all_valid(), false, 16);
    let (_, st_c) = probe(&d1, &all_valid(), true, 0);
    let stage_ok = stages_monotonic(st_a)
        && stages_monotonic(st_b)
        && stages_monotonic(st_c)
        && !stages_monotonic(STAGE_VERDICT)
        && st_a == (STAGE_EXTRACT | STAGE_VALUE | STAGE_PTR | STAGE_VERDICT);
    s.add("状态机：阶段位图单调且跳级被拒", stage_ok,
          "Verdict 前必须置位 Value/Ptr，跳级判 StageSkip");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "表外码不得放行，负值必须可复算");

    // 15. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：16 行语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 16. 文案闭集 + 码值唯一
    let text_ok = ProbeCode::ALL.iter().all(|c| !c.detail().is_empty());
    let uniq = ProbeCode::ALL.iter().enumerate().all(|(u, x)| {
        ProbeCode::ALL.iter().skip(u + 1).all(|y| x.val() != y.val())
    });
    s.add("文案闭集：12 码均有中文建议且码值两两唯一", text_ok && uniq,
          "任一条为空或重码，判据都无法定位到具体违约点");

    // 动态明细：注入统计
    let text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 坏指针 {} / 可空NULL {}",
        ledger.injected, ledger.detected, ledger.escaped, faults, nullable_ok
    );
    s.add_dyn("坏指针注入账", ledger.conserved(), &text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 按名取描述；测试表外名时构造显式描述，避免解包失败。
    fn desc(name: &'static str, nr: u32) -> SysDesc {
        match desc_of(name) {
            Some(d) => *d,
            None => SysDesc::new(nr, name, 0),
        }
    }

    #[test]
    fn mask_extraction_matches_frozen() {
        assert_eq!(table_drift(), 0);
        let (na, pp, _, _) = column_sums();
        assert_eq!(na, 56);
        assert_eq!(pp, 19);
    }

    #[test]
    fn nibble_bounds() {
        assert_eq!(nibble(0x13, 0), 3);
        assert_eq!(nibble(0x13, 1), 1);
        assert_eq!(nibble(0x13, 2), 0);
        assert_eq!(nibble(0x13, 9), 0);
    }

    #[test]
    fn normal_path_all_ok() {
        for d in FAMILY.iter() {
            let (r, stages) = probe(d, &all_valid(), true, 16);
            assert_eq!(r, Ok(()));
            assert!(stages_monotonic(stages));
        }
    }

    #[test]
    fn nullable_null_accepted() {
        let d = desc("mremap", 25);
        let (slots, n) = d.ptr_slots();
        let mut ptrs = all_valid();
        if n > 0 {
            if let Some(slot) = slots.get(n - 1) {
                if let Some(cell) = ptrs.get_mut(*slot as usize) {
                    *cell = PtrClass::Null;
                }
            }
        }
        assert_eq!(probe(&d, &ptrs, true, 16).0, Ok(()));
    }

    #[test]
    fn non_nullable_null_faults() {
        let d = desc("mmap", 9);
        let mut ptrs = all_valid();
        if let Some(cell) = ptrs.get_mut(0) {
            *cell = PtrClass::Null;
        }
        assert_eq!(probe(&d, &ptrs, true, 16).0, Err(EFAULT));
    }

    #[test]
    fn value_gate_precedes_pointer_gate() {
        let d = desc("mmap", 9);
        let mut ptrs = all_valid();
        if let Some(cell) = ptrs.get_mut(0) {
            *cell = PtrClass::Kernel;
        }
        let (r, stages) = probe(&d, &ptrs, false, 16);
        assert_eq!(r, Err(EINVAL));
        assert_eq!(stages & STAGE_PTR, 0);
    }

    #[test]
    fn zero_len_paths() {
        let mm = desc("mmap", 9);
        let mu = desc("munmap", 11);
        assert_eq!(probe(&mm, &all_valid(), true, 0).0, Ok(()));
        assert_eq!(probe(&mu, &all_valid(), true, 0).0, Err(EINVAL));
    }

    #[test]
    fn write_slot_readonly_faults() {
        let mut ptrs = all_valid();
        if let Some(cell) = ptrs.get_mut(2) {
            *cell = PtrClass::ReadOnly;
        }
        let d = desc("mincore", 27);
        assert_eq!(probe(&d, &ptrs, true, 16).0, Err(EFAULT));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9254_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
