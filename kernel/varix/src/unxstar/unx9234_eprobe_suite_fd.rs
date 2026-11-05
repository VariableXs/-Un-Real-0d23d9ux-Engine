//! UNX-F9234 fd 族 E 型参数探针总成 · 0x7A800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9234 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9234-J1 nargs 逐位提取判据过，
//!   坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 参数描述字单源：每行只存一个 32 位 mask，每槽 4 位（存在/指针/写向/
//!      可空）。nargs 不落字段、只由 mask 逐位弹出，于是「nargs 提取判据」与
//!      「坏指针探针」共用同一份数据，结构上无法漂移，这就是单源声明。
//!   ② fd 族 E 型错误面三层：fd 门（EBADF）→ 值域门（EINVAL）→ 指针门
//!      （EFAULT）。次序是语义：先 fd 可避免对已关闭对象做参数拷贝，先值域
//!      与 Linux sys_* 前置检查一致，指针门按槽位升序短路。
//!   ③ 输出向槽用只读页探针（可读不可写，仍须 EFAULT），输入向槽用未映射
//!      页；方向不对称必须在判定器里显性表达，不能把只读页当合法输出。
//!   ④ 可空槽（sendfile.offset、splice.off_in/off_out）传 NULL 必须成功；
//!      非可空槽传 NULL 必须 EFAULT。空指针不是通用坏指针，一刀切会误杀
//!      正常调用或放行空解引用。
//!   ⑤ 「单源」用冻结账复算：nargs 账 45、指针账 11、可空账 3、写向账 8。
//!      任一 mask 被改动，漂移格与总账格同时报红。
//!   ⑥ 注入必抓按类定轮：每类 10 轮，检出 10/10，且 injected = detected +
//!      escaped，escaped 恒为 0；双跑指纹一致才算稳态。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9234 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A800;
/// 探针格容量上限。
pub const MAX_CELLS: usize = 48;
/// 指针槽总数（冻结账）。
pub const POINTER_CELLS: usize = 11;
/// 可空指针槽数（冻结账）。
pub const NULLABLE_CELLS: usize = 3;
/// 写向指针槽数（冻结账）。
pub const WRITE_PTRS: usize = 8;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 表级 nargs 合计（冻结账）。
pub const TABLE_NARGS_SUM: u32 = 45;
/// 坏文件描述符。
pub const EBADF: u32 = 9;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 非法寻址（本组仅用于管道偏移类注记）。
pub const ESPIPE: u32 = 29;
/// 打开文件数超限。
pub const EMFILE: u32 = 24;
/// 描述符上界（含）。
pub const MAX_FD: i32 = 63;
/// O_CLOEXEC。
pub const O_CLOEXEC: u32 = 0x80000;
/// pipe2 合法 flags（O_CLOEXEC | O_NONBLOCK）。
pub const PIPE2_ALLOWED: u32 = 0x80800;
/// dup3 合法 flags（仅 O_CLOEXEC）。
pub const DUP3_ALLOWED: u32 = O_CLOEXEC;
// ── mask 半字节编码 ──────────────────────────────────────────────────
const NIB_EXISTS: u32 = 0x1;
const NIB_PTR: u32 = 0x2;
const NIB_WRITE: u32 = 0x4;
const NIB_NULLABLE: u32 = 0x8;
/// 取第 idx 个参数的 4 位描述。
pub fn nibble(mask: u32, idx: u32) -> u32 {
    if idx >= 8 {
        return 0;
    }
    (mask >> (idx * 4)) & 0xF
}
/// 该槽是否存在（决定 nargs）。
pub fn arg_exists(mask: u32, idx: u32) -> bool {
    nibble(mask, idx) & NIB_EXISTS != 0
}
/// 该槽是否为指针。
pub fn arg_is_ptr(mask: u32, idx: u32) -> bool {
    nibble(mask, idx) & NIB_PTR != 0
}
/// 该槽是否为写向（内核写回用户）。
pub fn arg_write(mask: u32, idx: u32) -> bool {
    nibble(mask, idx) & NIB_WRITE != 0
}
/// 该槽是否允许 NULL。
pub fn arg_nullable(mask: u32, idx: u32) -> bool {
    nibble(mask, idx) & NIB_NULLABLE != 0
}
/// nargs 逐位提取：从最低槽起数连续「存在」位。
pub fn nargs(mask: u32) -> u32 {
    let mut n = 0u32;
    while n < 8 && arg_exists(mask, n) {
        n += 1;
    }
    n
}
/// 指针槽计数。
pub fn ptr_count(mask: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < nargs(mask) {
        if arg_is_ptr(mask, i) {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 可空槽计数。
pub fn nullable_count(mask: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < nargs(mask) {
        if arg_is_ptr(mask, i) && arg_nullable(mask, i) {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 写向指针槽计数。
pub fn write_ptr_count(mask: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < nargs(mask) {
        if arg_is_ptr(mask, i) && arg_write(mask, i) {
            n += 1;
        }
        i += 1;
    }
    n
}
/// fd 槽下标：0xFF 表示该调用没有 fd 参数。
pub const FD_NONE: u8 = 0xFF;
/// 一条系统调用描述：名字、号、mask、fd 槽。
#[derive(Clone, Copy)]
pub struct SysDesc {
    pub name: &'static str,
    pub nr: u32,
    pub mask: u32,
    pub fd_arg: u8,
}
// 编号沿用 x86_64 前段，便于与 B08/B15 册互认。
const NR_READ: u32 = 0;
const NR_WRITE: u32 = 1;
const NR_PREAD64: u32 = 17;
const NR_PWRITE64: u32 = 18;
const NR_CLOSE: u32 = 3;
const NR_DUP: u32 = 32;
const NR_DUP2: u32 = 33;
const NR_DUP3: u32 = 292;
const NR_FCNTL: u32 = 72;
const NR_IOCTL: u32 = 16;
const NR_FSTAT: u32 = 5;
const NR_LSEEK: u32 = 8;
const NR_PIPE: u32 = 22;
const NR_PIPE2: u32 = 293;
const NR_SENDFILE: u32 = 40;
const NR_SPLICE: u32 = 275;
/// 16 条 fd 族调用的描述表（单一真源）。
pub const FAMILY: [SysDesc; 16] = [
    SysDesc { name: "read", nr: NR_READ, mask: 0x171, fd_arg: 0 },
    SysDesc { name: "write", nr: NR_WRITE, mask: 0x131, fd_arg: 0 },
    SysDesc { name: "pread64", nr: NR_PREAD64, mask: 0x1171, fd_arg: 0 },
    SysDesc { name: "pwrite64", nr: NR_PWRITE64, mask: 0x1131, fd_arg: 0 },
    SysDesc { name: "close", nr: NR_CLOSE, mask: 0x1, fd_arg: 0 },
    SysDesc { name: "dup", nr: NR_DUP, mask: 0x1, fd_arg: 0 },
    SysDesc { name: "dup2", nr: NR_DUP2, mask: 0x11, fd_arg: 0 },
    SysDesc { name: "dup3", nr: NR_DUP3, mask: 0x111, fd_arg: 0 },
    SysDesc { name: "fcntl", nr: NR_FCNTL, mask: 0x111, fd_arg: 0 },
    SysDesc { name: "ioctl", nr: NR_IOCTL, mask: 0x711, fd_arg: 0 },
    SysDesc { name: "fstat", nr: NR_FSTAT, mask: 0x71, fd_arg: 0 },
    SysDesc { name: "lseek", nr: NR_LSEEK, mask: 0x111, fd_arg: 0 },
    SysDesc { name: "pipe", nr: NR_PIPE, mask: 0x7, fd_arg: FD_NONE },
    SysDesc { name: "pipe2", nr: NR_PIPE2, mask: 0x17, fd_arg: FD_NONE },
    SysDesc { name: "sendfile", nr: NR_SENDFILE, mask: 0x1F11, fd_arg: 0 },
    SysDesc { name: "splice", nr: NR_SPLICE, mask: 0x11F1B1, fd_arg: 0 },
];
/// 逐行冻结 nargs（与 mask 互为校验）。
pub const FROZEN_NARGS: [u32; 16] = [3, 3, 4, 4, 1, 1, 2, 3, 3, 3, 2, 3, 1, 2, 4, 6];
/// 逐行冻结指针槽数。
pub const FROZEN_PTRS: [u32; 16] = [1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 2];
/// 逐行冻结可空槽数。
pub const FROZEN_NULLABLE: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2];
/// 冻结结构签名（nargs、指针、可空、写向）。
pub fn frozen_sig() -> (u32, u32, u32, u32) {
    (TABLE_NARGS_SUM, POINTER_CELLS as u32, NULLABLE_CELLS as u32, WRITE_PTRS as u32)
}
/// 由一组 mask 复算结构签名。
pub fn table_sig(masks: &[u32; 16]) -> (u32, u32, u32, u32) {
    let mut nargs_sum = 0u32;
    let mut ptrs = 0u32;
    let mut nulls = 0u32;
    let mut writes = 0u32;
    for m in masks.iter() {
        nargs_sum += nargs(*m);
        ptrs += ptr_count(*m);
        nulls += nullable_count(*m);
        writes += write_ptr_count(*m);
    }
    (nargs_sum, ptrs, nulls, writes)
}
/// 从 FAMILY 抽取当前 mask 表。
pub fn mask_table() -> [u32; 16] {
    let mut out = [0u32; 16];
    for (i, d) in FAMILY.iter().enumerate() {
        out[i] = d.mask;
    }
    out
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProbeCode {
    NargsMismatch,
    MaskBitsSpill,
    PtrVerdictDiverge,
    ReadOnlyOutputAccepted,
    NullRequiredAccepted,
    NullNullableRejected,
    GateOrderInverted,
    FrozenTableDrift,
    CellCapacityBlown,
    LedgerNotConserved,
    DoubleRunDrift,
    SegmentOutOfRange,
}
impl ProbeCode {
    /// 诊断码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议（闭集文案）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NargsMismatch => "nargs 与 mask 存在位不符：先修 mask，再回归逐位提取",
            ProbeCode::MaskBitsSpill => "mask 槽位越界：检查参数个数是否超过 8 槽编码上限",
            ProbeCode::PtrVerdictDiverge => "坏指针未判 EFAULT：核对方向位与可空位",
            ProbeCode::ReadOnlyOutputAccepted => "只读页被当作合法输出：写向槽必须拒绝",
            ProbeCode::NullRequiredAccepted => "非可空槽放行 NULL：补 EFAULT 分支",
            ProbeCode::NullNullableRejected => "可空槽误拒 NULL：不要一刀切判坏指针",
            ProbeCode::GateOrderInverted => "门序倒置：fd 门先于值域门，值域门先于指针门",
            ProbeCode::FrozenTableDrift => "冻结账漂移：nargs/指针/可空/写向合计需回到 45/11/3/8",
            ProbeCode::CellCapacityBlown => "探针格超容：格数须不超过 48，超容即截断误判",
            ProbeCode::LedgerNotConserved => "注入账不守恒：injected 必须等于 detected 加 escaped",
            ProbeCode::DoubleRunDrift => "双跑指纹漂移：注入路径混入了非确定状态",
            ProbeCode::SegmentOutOfRange => "诊断码越出 0x7A800 窗口：段位守卫拒绝登记",
        }
    }
    /// 全量闭集。
    pub const ALL: [ProbeCode; 12] = [
        ProbeCode::NargsMismatch,
        ProbeCode::MaskBitsSpill,
        ProbeCode::PtrVerdictDiverge,
        ProbeCode::ReadOnlyOutputAccepted,
        ProbeCode::NullRequiredAccepted,
        ProbeCode::NullNullableRejected,
        ProbeCode::GateOrderInverted,
        ProbeCode::FrozenTableDrift,
        ProbeCode::CellCapacityBlown,
        ProbeCode::LedgerNotConserved,
        ProbeCode::DoubleRunDrift,
        ProbeCode::SegmentOutOfRange,
    ];
}
/// 指针类别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PtrClass {
    Valid,
    Null,
    Unmapped,
    ReadOnly,
    KernelRange,
    Guard,
}
impl PtrClass {
    /// 稳定标签（指纹用）。
    pub fn tag(self) -> u32 {
        match self {
            PtrClass::Valid => 0,
            PtrClass::Null => 1,
            PtrClass::Unmapped => 2,
            PtrClass::ReadOnly => 3,
            PtrClass::KernelRange => 4,
            PtrClass::Guard => 5,
        }
    }
}
/// fd 状态（含代次失效语义）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FdState {
    Std,
    DynOpen,
    DynClosed,
    Free,
    TooHigh,
}
impl FdState {
    /// 是否可用于系统调用。
    pub fn usable(self) -> bool {
        matches!(self, FdState::Std | FdState::DynOpen)
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            FdState::Std => 0,
            FdState::DynOpen => 1,
            FdState::DynClosed => 2,
            FdState::Free => 3,
            FdState::TooHigh => 4,
        }
    }
}
/// 值域门：仅对带 flags/cmd 的调用生效。
pub fn value_gate(nr: u32, flags: u32) -> u32 {
    match nr {
        NR_DUP3 => {
            if flags & !DUP3_ALLOWED != 0 {
                EINVAL
            } else {
                0
            }
        }
        NR_PIPE2 => {
            if flags & !PIPE2_ALLOWED != 0 {
                EINVAL
            } else {
                0
            }
        }
        NR_FCNTL => {
            if flags == 0 {
                EINVAL
            } else {
                0
            }
        }
        _ => 0,
    }
}
/// 核心判定：fd 门 → 值域门 → 指针门。
pub fn judge_arg(d: &SysDesc, fd: FdState, arg: u8, ptr: PtrClass, val_bad: bool) -> u32 {
    if d.fd_arg != FD_NONE && !fd.usable() {
        return EBADF;
    }
    if val_bad {
        return EINVAL;
    }
    if ptr == PtrClass::Valid {
        return 0;
    }
    if !arg_is_ptr(d.mask, arg as u32) {
        return EINVAL;
    }
    let nullable = arg_nullable(d.mask, arg as u32);
    match ptr {
        PtrClass::Null => {
            if nullable {
                0
            } else {
                EFAULT
            }
        }
        PtrClass::ReadOnly => {
            if arg_write(d.mask, arg as u32) {
                EFAULT
            } else {
                0
            }
        }
        _ => EFAULT,
    }
}
/// 单格探针。
#[derive(Clone, Copy)]
pub struct ProbeCell {
    pub sys: u8,
    pub arg: u8,
    pub class: PtrClass,
    pub expect: u32,
}
/// 生成全部探针格：未映射 11 + 空指针 11 + 只读输出 8 + 内核区 4。
pub fn probe_cells() -> Vec<ProbeCell> {
    let mut v = Vec::new();
    for (si, d) in FAMILY.iter().enumerate() {
        let n = nargs(d.mask);
        let mut a = 0u32;
        while a < n {
            if arg_is_ptr(d.mask, a) {
                v.push(ProbeCell { sys: si as u8, arg: a as u8, class: PtrClass::Unmapped, expect: EFAULT });
            }
            a += 1;
        }
    }
    for (si, d) in FAMILY.iter().enumerate() {
        let n = nargs(d.mask);
        let mut a = 0u32;
        while a < n {
            if arg_is_ptr(d.mask, a) {
                let expect = if arg_nullable(d.mask, a) { 0 } else { EFAULT };
                v.push(ProbeCell { sys: si as u8, arg: a as u8, class: PtrClass::Null, expect });
            }
            a += 1;
        }
    }
    for (si, d) in FAMILY.iter().enumerate() {
        let n = nargs(d.mask);
        let mut a = 0u32;
        while a < n {
            if arg_is_ptr(d.mask, a) && arg_write(d.mask, a) {
                v.push(ProbeCell { sys: si as u8, arg: a as u8, class: PtrClass::ReadOnly, expect: EFAULT });
            }
            a += 1;
        }
    }
    let mut taken = 0u32;
    for (si, d) in FAMILY.iter().enumerate() {
        let n = nargs(d.mask);
        let mut a = 0u32;
        while a < n && taken < 4 {
            if arg_is_ptr(d.mask, a) {
                v.push(ProbeCell { sys: si as u8, arg: a as u8, class: PtrClass::KernelRange, expect: EFAULT });
                taken += 1;
            }
            a += 1;
        }
    }
    v
}
/// 单格判定。
pub fn cell_verdict(c: &ProbeCell, fd: FdState) -> u32 {
    let d = FAMILY[c.sys as usize];
    judge_arg(&d, fd, c.arg, c.class, false)
}
/// 次序探针。
#[derive(Clone, Copy)]
pub struct OrderCell {
    pub sys: u8,
    pub fd: FdState,
    pub arg: u8,
    pub ptr: PtrClass,
    pub val_bad: bool,
    pub expect: u32,
}
/// 四格门序：fd 先于值域、值域先于指针。
pub fn order_cells() -> [OrderCell; 4] {
    [
        OrderCell { sys: 0, fd: FdState::DynClosed, arg: 1, ptr: PtrClass::Unmapped, val_bad: false, expect: EBADF },
        OrderCell { sys: 7, fd: FdState::DynClosed, arg: 0, ptr: PtrClass::Valid, val_bad: true, expect: EBADF },
        OrderCell { sys: 0, fd: FdState::DynOpen, arg: 1, ptr: PtrClass::Unmapped, val_bad: true, expect: EINVAL },
        OrderCell { sys: 0, fd: FdState::DynOpen, arg: 1, ptr: PtrClass::Unmapped, val_bad: false, expect: EFAULT },
    ]
}
/// 次序判定。
pub fn order_verdict(c: &OrderCell) -> u32 {
    let d = FAMILY[c.sys as usize];
    judge_arg(&d, c.fd, c.arg, c.ptr, c.val_bad)
}
/// 总成状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SuiteStage {
    Draft,
    Enumerated,
    Swept,
    Injected,
    Sealed,
}
impl SuiteStage {
    /// 仅允许单步前进。
    pub fn can_go(self, next: SuiteStage) -> bool {
        matches!(
            (self, next),
            (SuiteStage::Draft, SuiteStage::Enumerated)
                | (SuiteStage::Enumerated, SuiteStage::Swept)
                | (SuiteStage::Swept, SuiteStage::Injected)
                | (SuiteStage::Injected, SuiteStage::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            SuiteStage::Draft => 0,
            SuiteStage::Enumerated => 1,
            SuiteStage::Swept => 2,
            SuiteStage::Injected => 3,
            SuiteStage::Sealed => 4,
        }
    }
}
/// 注入变体。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mutant {
    DropPtrBit,
    FlipWriteBit,
    NullableLeak,
}
impl Mutant {
    /// 三类变体。
    pub fn all() -> [Mutant; 3] {
        [Mutant::DropPtrBit, Mutant::FlipWriteBit, Mutant::NullableLeak]
    }
    /// 稳定标签（0..2）。
    pub fn tag(self) -> u32 {
        match self {
            Mutant::DropPtrBit => 0,
            Mutant::FlipWriteBit => 1,
            Mutant::NullableLeak => 2,
        }
    }
    /// 对单行 mask 施加变体。
    pub fn apply(self, mask: u32) -> u32 {
        match self {
            Mutant::DropPtrBit => mask & !(NIB_PTR << 4),
            Mutant::FlipWriteBit => mask | (NIB_WRITE << 4),
            Mutant::NullableLeak => mask & !(NIB_NULLABLE << 8),
        }
    }
}
/// 变体是否必被冻结账抓住。
pub fn mutant_detected(m: Mutant) -> bool {
    let mut masks = mask_table();
    let mut i = 0usize;
    while i < masks.len() {
        masks[i] = m.apply(masks[i]);
        i += 1;
    }
    table_sig(&masks) != frozen_sig()
}
/// 注入账。
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}
impl Ledger {
    /// 空账。
    pub fn new() -> Self {
        Ledger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }
    /// 记一轮。
    pub fn record(&mut self, tag: u8, detected: bool) {
        self.injected += 1;
        if detected {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
        self.rounds.push((tag, if detected { 1 } else { 0 }));
    }
    /// 守恒：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}
/// 3 类 × 10 轮注入。
pub fn run_injections() -> Ledger {
    let mut l = Ledger::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        for m in Mutant::all().iter() {
            let hit = mutant_detected(*m);
            l.record(m.tag() as u8, hit);
        }
        r += 1;
    }
    l
}
/// 每类检出次数。
pub fn per_class_hits(l: &Ledger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        let idx = (tag as usize) % 3;
        out[idx] += hit;
    }
    out
}
/// 探针格容量守卫。
pub fn capacity_ok(n: usize) -> bool {
    n <= MAX_CELLS
}
/// 文案闭集自检：12 条、值唯一、建议非空。
pub fn text_ok() -> bool {
    if ProbeCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        let a = ProbeCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ProbeCode::ALL.len() {
            let b = ProbeCode::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 探针格指纹。
pub fn cell_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9234/cells");
    for c in probe_cells().iter() {
        h = fnv1a(h, &[c.sys, c.arg, c.class.tag() as u8]);
        h = fnv1a(h, &c.expect.to_le_bytes());
    }
    h
}
/// 描述表指纹。
pub fn table_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9234/table");
    for d in FAMILY.iter() {
        h = fnv1a(h, d.name.as_bytes());
        h = fnv1a(h, &d.mask.to_le_bytes());
    }
    h
}
/// 判定路径指纹（双跑对比用）。
pub fn run_digest() -> u32 {
    let mut h = fnv1a(cell_digest(), b"run");
    for c in probe_cells().iter() {
        let v = cell_verdict(c, FdState::DynOpen);
        h = fnv1a(h, &v.to_le_bytes());
    }
    h
}
/// 正常流：16 条调用全绿。
pub fn normal_flow_ok() -> bool {
    for d in FAMILY.iter() {
        let legal_flags = match d.nr {
            NR_DUP3 => DUP3_ALLOWED,
            NR_PIPE2 => PIPE2_ALLOWED,
            NR_FCNTL => 1,
            _ => 0,
        };
        if value_gate(d.nr, legal_flags) != 0 {
            return false;
        }
        let arg = first_ptr_arg(d.mask);
        if judge_arg(d, FdState::DynOpen, arg, PtrClass::Valid, false) != 0 {
            return false;
        }
    }
    true
}
/// 取首个指针槽（无指针则 0）。
pub fn first_ptr_arg(mask: u32) -> u8 {
    let n = nargs(mask);
    let mut i = 0u32;
    while i < n {
        if arg_is_ptr(mask, i) {
            return i as u8;
        }
        i += 1;
    }
    0
}
/// 自检总成。
pub fn run_unx9234_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9234");
    s.add("seg_base", SEG_BASE == 0x7A800, "段基址应为 0x7A800");
    s.add("code_window", ProbeCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "诊断码须落在 0x7A800..0x7A840");
    s.add("text_closed_set", text_ok(), "12 条闭集、值唯一、建议非空");
    let m = mask_table();
    let sig = table_sig(&m);
    s.add("nargs_sum", sig.0 == TABLE_NARGS_SUM, "nargs 合计应冻结为 45");
    s.add("ptr_sum", sig.1 == POINTER_CELLS as u32, "指针槽合计应冻结为 11");
    s.add("nullable_sum", sig.2 == NULLABLE_CELLS as u32, "可空槽合计应冻结为 3");
    s.add("write_ptr_sum", sig.3 == WRITE_PTRS as u32, "写向指针槽应冻结为 8");
    let mut nargs_frozen = true;
    let mut ptrs_frozen = true;
    let mut nulls_frozen = true;
    for (i, d) in FAMILY.iter().enumerate() {
        nargs_frozen &= nargs(d.mask) == FROZEN_NARGS[i];
        ptrs_frozen &= ptr_count(d.mask) == FROZEN_PTRS[i];
        nulls_frozen &= nullable_count(d.mask) == FROZEN_NULLABLE[i];
    }
    s.add("nargs_frozen", nargs_frozen, "逐行 nargs 与冻结数组一致");
    s.add("ptrs_frozen", ptrs_frozen, "逐行指针槽与冻结数组一致");
    s.add("nulls_frozen", nulls_frozen, "逐行可空槽与冻结数组一致");
    s.add("value_gate", value_gate(NR_DUP3, DUP3_ALLOWED) == 0
        && value_gate(NR_DUP3, 0x1) == EINVAL
        && value_gate(NR_PIPE2, PIPE2_ALLOWED) == 0
        && value_gate(NR_PIPE2, 0x1) == EINVAL
        && value_gate(NR_FCNTL, 0) == EINVAL
        && value_gate(NR_READ, 0) == 0, "值域门只对 dup3/pipe2/fcntl 生效");
    s.add("normal_flow", normal_flow_ok(), "16 条调用合法参数均返回 0");
    s.add("boundary_std", judge_arg(&FAMILY[0], FdState::Std, 1, PtrClass::Valid, false) == 0, "标准 fd 0/1/2 合法");
    s.add("boundary_closed", judge_arg(&FAMILY[0], FdState::DynClosed, 1, PtrClass::Valid, false) == EBADF, "代次失效 fd 应 EBADF");
    s.add("boundary_high", judge_arg(&FAMILY[0], FdState::TooHigh, 1, PtrClass::Valid, false) == EBADF, "超过 63 的 fd 应 EBADF");
    let cells = probe_cells();
    let unmapped_ok = cells.iter().filter(|c| c.class == PtrClass::Unmapped).all(|c| cell_verdict(c, FdState::DynOpen) == EFAULT);
    s.add("unmapped_efault", unmapped_ok, "11 个未映射指针格全 EFAULT");
    let null_req_ok = cells.iter().filter(|c| c.class == PtrClass::Null && c.expect == EFAULT).all(|c| cell_verdict(c, FdState::DynOpen) == EFAULT);
    s.add("null_required", null_req_ok, "非可空槽 NULL 必须 EFAULT");
    let null_opt: Vec<&ProbeCell> = cells.iter().filter(|c| c.class == PtrClass::Null && c.expect == 0).collect();
    s.add("null_nullable", null_opt.len() == NULLABLE_CELLS && null_opt.iter().all(|c| cell_verdict(c, FdState::DynOpen) == 0), "3 个可空槽 NULL 必须成功");
    let ro_ok = cells.iter().filter(|c| c.class == PtrClass::ReadOnly).all(|c| cell_verdict(c, FdState::DynOpen) == EFAULT);
    s.add("readonly_output", ro_ok, "写向槽落只读页必须 EFAULT");
    let kr_ok = cells.iter().filter(|c| c.class == PtrClass::KernelRange).all(|c| cell_verdict(c, FdState::DynOpen) == EFAULT);
    s.add("kernel_range", kr_ok, "内核区指针必须 EFAULT");
    let oc = order_cells();
    s.add("order_fd_first", order_verdict(&oc[0]) == EBADF && order_verdict(&oc[1]) == EBADF, "坏 fd 压过坏指针与坏值");
    s.add("order_value_prev", order_verdict(&oc[2]) == EINVAL, "坏值压过坏指针");
    s.add("order_ptr_last", order_verdict(&oc[3]) == EFAULT, "指针门在最后");
    s.add("capacity", capacity_ok(cells.len()) && cells.len() >= 30, "探针格数量在 [30, 48]");
    s.add("capacity_overflow", capacity_ok(MAX_CELLS) && !capacity_ok(MAX_CELLS + 1), "超出 48 必须被守卫拒绝");
    let stage_ok = SuiteStage::Draft.can_go(SuiteStage::Enumerated)
        && SuiteStage::Enumerated.can_go(SuiteStage::Swept)
        && SuiteStage::Swept.can_go(SuiteStage::Injected)
        && SuiteStage::Injected.can_go(SuiteStage::Sealed)
        && !SuiteStage::Draft.can_go(SuiteStage::Swept)
        && !SuiteStage::Sealed.can_go(SuiteStage::Draft);
    s.add("state_machine", stage_ok, "阶段只许单步前进");
    let l = run_injections();
    let hits = per_class_hits(&l);
    s.add("inject_ten_of_ten", hits == [INJECT_ROUNDS; 3], "三类变体各检出 10/10");
    s.add("ledger_conserved", l.conserved() && l.escaped == 0 && l.injected == INJECT_ROUNDS * 3, "注入 30 = 检出 30 + 逃逸 0");
    let d1 = run_digest();
    let d2 = run_digest();
    let l2 = run_injections();
    s.add("double_run", d1 == d2 && l2.injected == l.injected && l2.detected == l.detected, "双跑判定与注入账一致");
    s.add("fnv_basis", fnv1a(0x811C_9DC5, b"") == 0x811C_9DC5, "FNV 偏移基正确");
    s.add_dyn("cell_detail", true, &format!("格={} 未映射={} 可空={}", cells.len(), POINTER_CELLS, NULLABLE_CELLS));
    s.add_dyn("inject_detail", hits[0] == 10 && hits[1] == 10 && hits[2] == 10, &format!("丢指针位={} 翻写向={} 漏可空={}", hits[0], hits[1], hits[2]));
    s.add_dyn("digest_detail", table_digest() != 0 && cell_digest() != 0, &format!("表指纹={:08x} 格指纹={:08x} 跑指纹={:08x}", table_digest(), cell_digest(), d1));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_all_green() {
        let s = run_unx9234_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn nargs_extracted_from_mask() {
        for (i, d) in FAMILY.iter().enumerate() {
            assert_eq!(nargs(d.mask), FROZEN_NARGS[i]);
            assert_eq!(ptr_count(d.mask), FROZEN_PTRS[i]);
            assert_eq!(nullable_count(d.mask), FROZEN_NULLABLE[i]);
        }
    }
    #[test]
    fn single_source_totals() {
        let sig = table_sig(&mask_table());
        assert_eq!(sig, frozen_sig());
    }
    #[test]
    fn every_bad_pointer_is_efault() {
        for c in probe_cells().iter() {
            if c.class == PtrClass::Unmapped || c.class == PtrClass::KernelRange {
                assert_eq!(cell_verdict(c, FdState::DynOpen), EFAULT);
            }
        }
    }
    #[test]
    fn nullable_asymmetry() {
        let cells = probe_cells();
        let opt = cells
            .iter()
            .filter(|c| c.class == PtrClass::Null && c.expect == 0)
            .count();
        let req = cells
            .iter()
            .filter(|c| c.class == PtrClass::Null && c.expect == EFAULT)
            .count();
        assert_eq!(opt, NULLABLE_CELLS);
        assert_eq!(req, POINTER_CELLS - NULLABLE_CELLS);
    }
    #[test]
    fn gate_order_is_fd_value_ptr() {
        let oc = order_cells();
        assert_eq!(order_verdict(&oc[0]), EBADF);
        assert_eq!(order_verdict(&oc[1]), EBADF);
        assert_eq!(order_verdict(&oc[2]), EINVAL);
        assert_eq!(order_verdict(&oc[3]), EFAULT);
    }
    #[test]
    fn injection_is_deterministic_ten_of_ten() {
        let a = run_injections();
        let b = run_injections();
        assert_eq!(per_class_hits(&a), [10, 10, 10]);
        assert_eq!(per_class_hits(&b), [10, 10, 10]);
        assert_eq!(a.escaped, 0);
        assert!(a.conserved());
    }
    #[test]
    fn capacity_guard_rejects_overflow() {
        assert!(capacity_ok(MAX_CELLS));
        assert!(!capacity_ok(MAX_CELLS + 1));
        assert!(capacity_ok(probe_cells().len()));
    }
    #[test]
    fn stage_machine_single_step() {
        let mut cur = SuiteStage::Draft;
        let chain = [
            SuiteStage::Enumerated,
            SuiteStage::Swept,
            SuiteStage::Injected,
            SuiteStage::Sealed,
        ];
        for nxt in chain.iter() {
            assert!(cur.can_go(*nxt));
            cur = *nxt;
        }
        assert!(!SuiteStage::Draft.can_go(SuiteStage::Sealed));
    }
    #[test]
    fn digests_are_stable() {
        assert_eq!(run_digest(), run_digest());
        assert_eq!(cell_digest(), cell_digest());
        assert_eq!(table_digest(), table_digest());
        assert_ne!(table_digest(), 0);
    }
    #[test]
    fn diagnostic_text_closed_set() {
        assert_eq!(ProbeCode::ALL.len(), 12);
        assert!(text_ok());
        for c in ProbeCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE);
            assert!(c.val() < SEG_BASE + 0x40);
        }
    }
}
