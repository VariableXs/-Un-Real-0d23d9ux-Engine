//! UNX-F9294 进程族 E 型参数探针总成 · 0x7B700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9294 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9294-J1 nargs 逐位提取判据过，
//!   坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 单源描述字：每个系统调用只存一个 64 位 shape，每槽一个字节（8 槽），
//!      字节值即实参种类（无/标量/读指针/写指针/可空读/可空写/字符串/向量）。
//!      实参必须是稠密前缀：第 0..n-1 槽非空、之后全空。稠密性由「计数」与
//!      「最高非空槽」两条独立路径同时验证，任一路不等即 DenseHole。
//!   ② nargs 三路提取：逐槽计数、最高槽+1、右移八位剥字节。三路必须相等，
//!      且逐行等于冻结列——这就是「nargs 逐位提取」的可复算落点。
//!   ③ 拷贝计划从同一份 shape 派生：每个指针槽一个 CopyOp（方向、容量由种类
//!      唯一决定）。计划长度必须等于冻结指针列，所以「参数拷贝探针」与
//!      「nargs 提取」在结构上无法漂移。
//!   ④ 门次序：提取（稠密/容量）→ 值域（flags/option/sig/向量计数）→ 指针。
//!      值域门先于指针门：坏 flags 与坏指针同时出现时必须先回 EINVAL/E2BIG，
//!      不能让用户借 EFAULT 探测地址空间。
//!   ⑤ 坏指针矩阵：21 个指针槽 × 4 类（空/未映射/内核/只读）× 10 轮，另加
//!      字符串无终止符 2 格与向量计数越界 3 格。期望值由独立的格判函数给出，
//!      与 probe 的计划执行路径互为交叉验证。
//!   ⑥ 阶段掩码只有 0x1/0x3/0x7 三个值，必须是 Extract→Value→Ptr 的前缀。
//!   ⑦ 诊断码段内闭集 12 个（+1..=+12），码值 = SEG_BASE + 序号，两两唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9294 独占 0x7B700..=0x7B73F）。
pub const SEG_BASE: u32 = 0x7B700;
/// 段窗口宽度（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
/// 本批段窗口下界（含）。
pub const SEG_WINDOW_LO: u32 = 0x7B000;
/// 本批段窗口上界（含）。
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

/// 用户指针不可访问。
pub const EFAULT: i32 = 14;
/// 参数坏值。
pub const EINVAL: i32 = 22;
/// 实参表过长（execve 向量上限）。
pub const E2BIG: i32 = 7;
/// 资源暂不可用（保留语义，本模块不产生）。
pub const EAGAIN: i32 = 11;
/// 无此进程（保留语义，本模块不产生）。
pub const ESRCH: i32 = 3;

// ── 实参种类 ──────────────────────────────────────────────────────────
/// 槽位为空（实参不存在）。
pub const AK_NONE: u8 = 0;
/// 标量（值传递，不拷贝）。
pub const AK_SCALAR: u8 = 1;
/// 只读指针。
pub const AK_READ: u8 = 2;
/// 写指针（内核回写用户）。
pub const AK_WRITE: u8 = 3;
/// 可空只读指针。
pub const AK_READ_NULL: u8 = 4;
/// 可空写指针。
pub const AK_WRITE_NULL: u8 = 5;
/// NUL 结尾字符串（需终止符）。
pub const AK_STR: u8 = 6;
/// 向量数组（需计数上限内）。
pub const AK_VEC: u8 = 7;

/// 槽位容量。
pub const MAX_SLOTS: usize = 8;
/// 进程族行数。
pub const FAMILY_ROWS: usize = 20;
/// 指针槽总数（冻结账）。
pub const PTR_TOTAL: usize = 21;
/// 写向槽总数（冻结账）。
pub const WRITE_TOTAL: usize = 9;
/// 可空槽总数（冻结账）。
pub const NULLABLE_TOTAL: usize = 8;
/// nargs 列合计（冻结账）。
pub const NARGS_TOTAL: u32 = 55;
/// 每格复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 坏指针类别数。
pub const PTR_CLASSES: u32 = 4;
/// 字符串终止符越界格数。
pub const STR_EXTRA_CELLS: u32 = 2;
/// 向量计数越界格数。
pub const VEC_EXTRA_CELLS: u32 = 3;
/// 注入总格数（21 槽 × 4 类 × 10 轮 + 5 个额外格 × 10 轮）。
pub const TOTAL_CELLS: u32 = 890;

/// 字符串拷贝上限（PATH_MAX）。
pub const CAP_STR: u32 = 4096;
/// 向量拷贝上限（IOV_MAX/ARG_MAX 口径）。
pub const CAP_VEC: u32 = 1024;
/// 标量拷贝宽度。
pub const CAP_SCALAR: u32 = 8;

// ── 阶段位 ────────────────────────────────────────────────────────────
/// 只经过提取门。
pub const ST_EXTRACT: u8 = 0x1;
/// 经过提取门与值域门。
pub const ST_VALUE: u8 = 0x3;
/// 经过提取、值域与指针门（判决完成）。
pub const ST_PTR: u8 = 0x7;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9294 诊断码（段内 +1..=+12 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// nargs 三路提取不一致
    NargsDrift,
    /// 稠密前缀被破坏
    DenseHole,
    /// 指针列漂移
    SlotDrift,
    /// 可空槽误报
    NullableDrift,
    /// 写向槽漏判
    WriteLeak,
    /// 坏指针逃逸
    FaultEscaped,
    /// 字符串缺终止符被放行
    StrLeak,
    /// 向量计数越界用了错码
    VecErrno,
    /// 值域门后置
    ValueLate,
    /// 拷贝计划与描述字不同源
    PlanDrift,
    /// 注入账不闭合
    LedgerLeak,
    /// 段位越界
    SegmentViolation,
}

impl ProbeCode {
    /// 段内序号（契约口径的 u16 码）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值 = SEG_BASE + 1 + 序号，两两唯一。
    pub fn val(self) -> u32 {
        SEG_BASE
            .saturating_add(1)
            .saturating_add(self.code() as u32)
    }

    /// 中文一句话建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NargsDrift => "nargs 三路提取不一致：逐槽计数、最高槽加一、右移剥字节必须同值",
            ProbeCode::DenseHole => "稠密前缀被破坏：实参必须占 0..n-1 槽，中间留空槽会让后续槽错位",
            ProbeCode::SlotDrift => "指针列漂移：shape 中指针种类数必须等于冻结列，改表必须同步改冻结账",
            ProbeCode::NullableDrift => "可空槽误报：clone 的 ptid/ctid、wait4 的 rusage、waitid 的 infop、times 的 buf 传 NULL 合法",
            ProbeCode::WriteLeak => "写向槽漏判：内核回写只读页必须 EFAULT，写向不能按只读入参放行",
            ProbeCode::FaultEscaped => "坏指针逃逸：空/未映射/内核地址必须在指针门短路回 EFAULT",
            ProbeCode::StrLeak => "字符串缺终止符被放行：页内找不到 NUL 必须回 EFAULT，不得按可读放行",
            ProbeCode::VecErrno => "向量计数越界用错码：execve 族是 E2BIG，iovec 计数非法是 EINVAL",
            ProbeCode::ValueLate => "值域门后置：坏 flags/option/sig 必须先回错误，不能先解引用用户地址",
            ProbeCode::PlanDrift => "拷贝计划漂移：计划必须逐槽由 shape 派生，长度与方向不得手写",
            ProbeCode::LedgerLeak => "注入账不闭合：注入必须等于检出加逃逸，且逃逸恒为零",
            ProbeCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7B000..=0x7BFFF",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [ProbeCode] = &[
        ProbeCode::NargsDrift,
        ProbeCode::DenseHole,
        ProbeCode::SlotDrift,
        ProbeCode::NullableDrift,
        ProbeCode::WriteLeak,
        ProbeCode::FaultEscaped,
        ProbeCode::StrLeak,
        ProbeCode::VecErrno,
        ProbeCode::ValueLate,
        ProbeCode::PlanDrift,
        ProbeCode::LedgerLeak,
        ProbeCode::SegmentViolation,
    ];
}

// ── 描述字 ────────────────────────────────────────────────────────────
/// 单槽种类提取（越界槽视为空）。
pub fn slot_kind(shape: u64, slot: usize) -> u8 {
    if slot >= MAX_SLOTS {
        return AK_NONE;
    }
    ((shape >> (slot.saturating_mul(8))) & 0xFF) as u8
}

/// 第一路：逐槽计数。
pub fn nargs_count(shape: u64) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < MAX_SLOTS {
        if slot_kind(shape, i) != AK_NONE {
            n = n.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    n
}

/// 第二路：最高非空槽 + 1（自高向低扫描）。
pub fn nargs_top(shape: u64) -> u8 {
    let mut i = MAX_SLOTS;
    while i > 0 {
        i = i.saturating_sub(1);
        if slot_kind(shape, i) != AK_NONE {
            return (i as u8).saturating_add(1);
        }
    }
    0
}

/// 第三路：右移八位逐字节剥（遇空槽即停，故只在稠密时等于计数）。
pub fn nargs_shift(shape: u64) -> u8 {
    let mut b = shape;
    let mut n = 0u8;
    while n < MAX_SLOTS as u8 {
        if (b & 0xFF) == 0 {
            return n;
        }
        n = n.saturating_add(1);
        b >>= 8;
    }
    n
}

/// 稠密前缀：三路同值。
pub fn shape_is_dense(shape: u64) -> bool {
    let a = nargs_count(shape);
    a == nargs_top(shape) && a == nargs_shift(shape)
}

/// 种类是指针。
pub fn kind_is_ptr(kind: u8) -> bool {
    matches!(
        kind,
        AK_READ | AK_WRITE | AK_READ_NULL | AK_WRITE_NULL | AK_STR | AK_VEC
    )
}

/// 种类是写向。
pub fn kind_is_write(kind: u8) -> bool {
    matches!(kind, AK_WRITE | AK_WRITE_NULL)
}

/// 种类可空。
pub fn kind_is_nullable(kind: u8) -> bool {
    matches!(kind, AK_READ_NULL | AK_WRITE_NULL)
}

/// 种类是字符串。
pub fn kind_is_str(kind: u8) -> bool {
    kind == AK_STR
}

/// 种类是向量。
pub fn kind_is_vec(kind: u8) -> bool {
    kind == AK_VEC
}

/// 值域政策。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValuePolicy {
    /// 没有值域检查。
    None = 0,
    /// execve 族：向量计数超限回 E2BIG。
    ExecVec = 1,
    /// clone：flags 位非法回 EINVAL。
    CloneFlags = 2,
    /// prctl：option 未知回 EINVAL。
    PrctlOpt = 3,
    /// 信号号越界回 EINVAL。
    SignalNo = 4,
}

impl ValuePolicy {
    /// 政策编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 政策短名。
    pub fn name(self) -> &'static str {
        match self {
            ValuePolicy::None => "none",
            ValuePolicy::ExecVec => "exec-vec",
            ValuePolicy::CloneFlags => "clone-flags",
            ValuePolicy::PrctlOpt => "prctl-opt",
            ValuePolicy::SignalNo => "signal-no",
        }
    }
}

/// 一次系统调用的描述（shape 是唯一数据源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysDesc {
    pub nr: u32,
    pub name: &'static str,
    pub shape: u64,
    pub policy: ValuePolicy,
}

impl SysDesc {
    /// 常量构造。
    pub const fn new(nr: u32, name: &'static str, shape: u64, policy: ValuePolicy) -> SysDesc {
        SysDesc {
            nr,
            name,
            shape,
            policy,
        }
    }

    /// nargs（逐槽计数路径）。
    pub fn nargs(&self) -> u8 {
        nargs_count(self.shape)
    }

    /// 指针槽数。
    pub fn ptr_count(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if kind_is_ptr(slot_kind(self.shape, i)) {
                n = n.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 写向槽数。
    pub fn write_count(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if kind_is_write(slot_kind(self.shape, i)) {
                n = n.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 可空槽数。
    pub fn nullable_count(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if kind_is_nullable(slot_kind(self.shape, i)) {
                n = n.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 稠密性。
    pub fn dense(&self) -> bool {
        shape_is_dense(self.shape)
    }

    /// 单槽种类。
    pub fn kind(&self, slot: usize) -> u8 {
        slot_kind(self.shape, slot)
    }
}

// ── shape 构造函数（常量上下文使用） ─────────────────────────────────
const fn s1(a: u8) -> u64 {
    a as u64
}

const fn s2(a: u8, b: u8) -> u64 {
    s1(a) | ((b as u64) << 8)
}

const fn s3(a: u8, b: u8, c: u8) -> u64 {
    s2(a, b) | ((c as u64) << 16)
}

const fn s4(a: u8, b: u8, c: u8, d: u8) -> u64 {
    s3(a, b, c) | ((d as u64) << 24)
}

const fn s5(a: u8, b: u8, c: u8, d: u8, e: u8) -> u64 {
    s4(a, b, c, d) | ((e as u64) << 32)
}

const fn s6(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8) -> u64 {
    s5(a, b, c, d, e) | ((f as u64) << 40)
}

/// 二十行进程族描述表（shape 单源）。
pub const FAMILY: &[SysDesc] = &[
    SysDesc::new(57, "fork", 0, ValuePolicy::None),
    SysDesc::new(58, "vfork", 0, ValuePolicy::None),
    SysDesc::new(
        56,
        "clone",
        s5(AK_SCALAR, AK_SCALAR, AK_WRITE_NULL, AK_WRITE_NULL, AK_SCALAR),
        ValuePolicy::CloneFlags,
    ),
    SysDesc::new(
        59,
        "execve",
        s3(AK_STR, AK_VEC, AK_VEC),
        ValuePolicy::ExecVec,
    ),
    SysDesc::new(
        322,
        "execveat",
        s5(AK_SCALAR, AK_STR, AK_VEC, AK_VEC, AK_SCALAR),
        ValuePolicy::ExecVec,
    ),
    SysDesc::new(
        61,
        "wait4",
        s4(AK_SCALAR, AK_WRITE, AK_SCALAR, AK_WRITE_NULL),
        ValuePolicy::None,
    ),
    SysDesc::new(
        247,
        "waitid",
        s5(AK_SCALAR, AK_SCALAR, AK_WRITE_NULL, AK_SCALAR, AK_WRITE_NULL),
        ValuePolicy::None,
    ),
    SysDesc::new(62, "kill", s2(AK_SCALAR, AK_SCALAR), ValuePolicy::None),
    SysDesc::new(234, "tgkill", s3(AK_SCALAR, AK_SCALAR, AK_SCALAR), ValuePolicy::SignalNo),
    SysDesc::new(109, "setpgid", s2(AK_SCALAR, AK_SCALAR), ValuePolicy::None),
    SysDesc::new(121, "getpgid", s1(AK_SCALAR), ValuePolicy::None),
    SysDesc::new(112, "setsid", 0, ValuePolicy::None),
    SysDesc::new(
        144,
        "sched_setscheduler",
        s3(AK_SCALAR, AK_SCALAR, AK_READ),
        ValuePolicy::None,
    ),
    SysDesc::new(
        143,
        "sched_getparam",
        s2(AK_SCALAR, AK_WRITE),
        ValuePolicy::None,
    ),
    SysDesc::new(
        157,
        "prctl",
        s5(AK_SCALAR, AK_SCALAR, AK_SCALAR, AK_SCALAR, AK_SCALAR),
        ValuePolicy::PrctlOpt,
    ),
    SysDesc::new(
        101,
        "ptrace",
        s4(AK_SCALAR, AK_SCALAR, AK_READ_NULL, AK_READ_NULL),
        ValuePolicy::None,
    ),
    SysDesc::new(98, "getrusage", s2(AK_SCALAR, AK_WRITE), ValuePolicy::None),
    SysDesc::new(100, "times", s1(AK_WRITE_NULL), ValuePolicy::None),
    SysDesc::new(160, "setrlimit", s2(AK_SCALAR, AK_READ), ValuePolicy::None),
    SysDesc::new(
        310,
        "process_vm_readv",
        s6(AK_SCALAR, AK_VEC, AK_SCALAR, AK_VEC, AK_SCALAR, AK_SCALAR),
        ValuePolicy::None,
    ),
];

/// 冻结 nargs 列。
pub const FROZEN_NARGS: [u8; FAMILY_ROWS] = [
    0, 0, 5, 3, 5, 4, 5, 2, 3, 2, 1, 0, 3, 2, 5, 4, 2, 1, 2, 6,
];
/// 冻结指针列。
pub const FROZEN_PTRS: [u8; FAMILY_ROWS] = [
    0, 0, 2, 3, 3, 2, 2, 0, 0, 0, 0, 0, 1, 1, 0, 2, 1, 1, 1, 2,
];
/// 冻结写向列。
pub const FROZEN_WRITES: [u8; FAMILY_ROWS] = [
    0, 0, 2, 0, 0, 2, 2, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0,
];
/// 冻结可空列。
pub const FROZEN_NULLABLE: [u8; FAMILY_ROWS] = [
    0, 0, 2, 0, 0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0,
];

/// 按名取描述。
pub fn desc_of(name: &str) -> Option<SysDesc> {
    FAMILY.iter().find(|d| d.name == name).copied()
}

/// 四列合计：(nargs, 指针, 写向, 可空)。
pub fn column_sums() -> (u32, u32, u32, u32) {
    let mut nargs = 0u32;
    let mut ptrs = 0u32;
    let mut writes = 0u32;
    let mut nullable = 0u32;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            nargs = nargs.saturating_add(nargs_count(d.shape) as u32);
            ptrs = ptrs.saturating_add(d.ptr_count() as u32);
            writes = writes.saturating_add(d.write_count() as u32);
            nullable = nullable.saturating_add(d.nullable_count() as u32);
        }
        i = i.saturating_add(1);
    }
    (nargs, ptrs, writes, nullable)
}

// ── 拷贝计划（由 shape 唯一派生） ─────────────────────────────────────
/// 一次用户拷贝：槽位、方向、容量全部来自种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyOp {
    pub slot: u8,
    pub kind: u8,
    pub write: bool,
    pub nullable: bool,
    pub cap: u32,
}

/// 单槽拷贝容量。
pub fn cap_for(kind: u8) -> u32 {
    if kind_is_str(kind) {
        CAP_STR
    } else if kind_is_vec(kind) {
        CAP_VEC
    } else {
        CAP_SCALAR
    }
}

/// 从描述字派生拷贝计划（槽位升序）。
pub fn plan_for(desc: &SysDesc) -> ([CopyOp; MAX_SLOTS], usize) {
    let mut ops = [CopyOp {
        slot: 0,
        kind: AK_NONE,
        write: false,
        nullable: false,
        cap: 0,
    }; MAX_SLOTS];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MAX_SLOTS {
        let kind = slot_kind(desc.shape, i);
        if kind_is_ptr(kind) {
            if let Some(op) = ops.get_mut(n) {
                *op = CopyOp {
                    slot: i as u8,
                    kind,
                    write: kind_is_write(kind),
                    nullable: kind_is_nullable(kind),
                    cap: cap_for(kind),
                };
                n = n.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    (ops, n)
}

// ── 用户内存模型 ──────────────────────────────────────────────────────
/// 用户指针类别。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 可读可写。
    Valid = 0,
    /// 空指针。
    Null = 1,
    /// 未映射页。
    Unmapped = 2,
    /// 内核地址。
    Kernel = 3,
    /// 只读映射。
    ReadOnly = 4,
}

impl PtrClass {
    /// 类别编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 用户态可读。
    pub fn readable(self) -> bool {
        matches!(self, PtrClass::Valid | PtrClass::ReadOnly)
    }

    /// 用户态可写。
    pub fn writable(self) -> bool {
        matches!(self, PtrClass::Valid)
    }

    /// 类别短名。
    pub fn name(self) -> &'static str {
        match self {
            PtrClass::Valid => "valid",
            PtrClass::Null => "null",
            PtrClass::Unmapped => "unmapped",
            PtrClass::Kernel => "kernel",
            PtrClass::ReadOnly => "readonly",
        }
    }
}

/// 单槽用户内存状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotState {
    pub class: PtrClass,
    /// 字符串是否页内有 NUL 终止符。
    pub nul_ok: bool,
    /// 向量计数是否在限内。
    pub vec_ok: bool,
}

/// 全合法槽初值（`[INIT; N]` 需要常量）。
pub const SLOT_OK: SlotState = SlotState {
    class: PtrClass::Valid,
    nul_ok: true,
    vec_ok: true,
};

/// 探针环境。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeEnv {
    pub slots: [SlotState; MAX_SLOTS],
    /// 标量参数是否合法（flags/option/sig）。
    pub scalar_ok: bool,
}

impl ProbeEnv {
    /// 全合法环境。
    pub const fn clean() -> ProbeEnv {
        ProbeEnv {
            slots: [SLOT_OK; MAX_SLOTS],
            scalar_ok: true,
        }
    }

    /// 单槽改类别。
    pub fn with_class(&self, slot: usize, class: PtrClass) -> ProbeEnv {
        let mut next = *self;
        if let Some(s) = next.slots.get_mut(slot) {
            s.class = class;
        }
        next
    }

    /// 单槽改终止符状态。
    pub fn with_nul(&self, slot: usize, nul_ok: bool) -> ProbeEnv {
        let mut next = *self;
        if let Some(s) = next.slots.get_mut(slot) {
            s.nul_ok = nul_ok;
        }
        next
    }

    /// 单槽改向量计数状态。
    pub fn with_vec(&self, slot: usize, vec_ok: bool) -> ProbeEnv {
        let mut next = *self;
        if let Some(s) = next.slots.get_mut(slot) {
            s.vec_ok = vec_ok;
        }
        next
    }

    /// 标量置坏。
    pub fn with_bad_scalar(&self) -> ProbeEnv {
        let mut next = *self;
        next.scalar_ok = false;
        next
    }
}

// ── 值域门 ────────────────────────────────────────────────────────────
/// 值域门：只读参数值（flags/option/sig）与向量计数，不解引用地址。
pub fn value_gate(desc: &SysDesc, env: &ProbeEnv) -> i32 {
    if desc.policy != ValuePolicy::None && !env.scalar_ok {
        return EINVAL;
    }
    let (ops, n) = plan_for(desc);
    let mut i = 0usize;
    while i < n {
        if let Some(op) = ops.get(i) {
            if kind_is_vec(op.kind) {
                let ok = match env.slots.get(op.slot as usize) {
                    Some(s) => s.vec_ok,
                    None => false,
                };
                if !ok {
                    return match desc.policy {
                        ValuePolicy::ExecVec => E2BIG,
                        _ => EINVAL,
                    };
                }
            }
        }
        i = i.saturating_add(1);
    }
    0
}

// ── 探针 ──────────────────────────────────────────────────────────────
/// 参数探针：返回（判决, 阶段掩码）。
///
/// 阶段掩码语义：0x1 只到提取门、0x3 到值域门、0x7 走完指针门。
pub fn probe(desc: &SysDesc, env: &ProbeEnv) -> (Result<(), i32>, u8) {
    let mut stages = ST_EXTRACT;
    if !desc.dense() || desc.nargs() as usize > MAX_SLOTS {
        return (Err(EINVAL), stages);
    }
    stages = ST_VALUE;
    let v = value_gate(desc, env);
    if v != 0 {
        return (Err(v), stages);
    }
    stages = ST_PTR;
    let (ops, n) = plan_for(desc);
    let mut i = 0usize;
    while i < n {
        if let Some(op) = ops.get(i) {
            let st = match env.slots.get(op.slot as usize) {
                Some(s) => *s,
                None => SlotState {
                    class: PtrClass::Null,
                    nul_ok: false,
                    vec_ok: false,
                },
            };
            if st.class == PtrClass::Null {
                if !op.nullable {
                    return (Err(EFAULT), stages);
                }
            } else if op.write {
                if !st.class.writable() {
                    return (Err(EFAULT), stages);
                }
            } else {
                if !st.class.readable() {
                    return (Err(EFAULT), stages);
                }
                if kind_is_str(op.kind) && !st.nul_ok {
                    return (Err(EFAULT), stages);
                }
            }
        }
        i = i.saturating_add(1);
    }
    (Ok(()), stages)
}

/// 阶段掩码是否为合法前缀。
pub fn stage_is_prefix(stages: u8) -> bool {
    matches!(stages, ST_EXTRACT | ST_VALUE | ST_PTR)
}

/// 独立格判：只按种类与类别给期望（不经过拷贝计划），与 probe 交叉验证。
pub fn cell_expectation(desc: &SysDesc, slot: usize, class: PtrClass, nul_ok: bool) -> i32 {
    let kind = desc.kind(slot);
    if !kind_is_ptr(kind) {
        return 0;
    }
    if class == PtrClass::Null {
        return if kind_is_nullable(kind) { 0 } else { EFAULT };
    }
    if kind_is_write(kind) {
        if class.writable() {
            0
        } else {
            EFAULT
        }
    } else if !class.readable() {
        EFAULT
    } else if kind_is_str(kind) && !nul_ok {
        EFAULT
    } else {
        0
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账。
    pub const fn zero() -> InjectLedger {
        InjectLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔。
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

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 坏指针矩阵 ────────────────────────────────────────────────────────
/// 坏指针矩阵：21 指针槽 × 4 类 × 10 轮 + 5 个字符串/向量额外格 × 10 轮。
///
/// 返回（账本, 每行命中数, 向量错码命中数, 交叉验证不一致数）。
pub fn run_bad_ptr_matrix() -> (InjectLedger, [u32; FAMILY_ROWS], u32, u32) {
    let classes = [
        PtrClass::Null,
        PtrClass::Unmapped,
        PtrClass::Kernel,
        PtrClass::ReadOnly,
    ];
    let mut ledger = InjectLedger::zero();
    let mut per_row = [0u32; FAMILY_ROWS];
    let mut ebig_hits = 0u32;
    let mut mismatch = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(desc) = FAMILY.get(ri) {
            let (ops, n) = plan_for(desc);
            let mut oi = 0usize;
            while oi < n {
                if let Some(op) = ops.get(oi) {
                    let mut ci = 0usize;
                    while ci < classes.len() {
                        if let Some(class) = classes.get(ci).copied() {
                            let env = ProbeEnv::clean().with_class(op.slot as usize, class);
                            let want = cell_expectation(desc, op.slot as usize, class, true);
                            let mut round = 0u32;
                            while round < INJECT_ROUNDS {
                                let (got, stages) = probe(desc, &env);
                                let errno = match got {
                                    Ok(()) => 0,
                                    Err(e) => e,
                                };
                                let caught = errno == want && stage_is_prefix(stages);
                                if errno != want {
                                    mismatch = mismatch.saturating_add(1);
                                }
                                let rec = ledger.record(caught);
                                if rec.is_ok() && caught {
                                    if let Some(slot) = per_row.get_mut(ri) {
                                        *slot = slot.saturating_add(1);
                                    }
                                }
                                round = round.saturating_add(1);
                            }
                        }
                        ci = ci.saturating_add(1);
                    }
                }
                oi = oi.saturating_add(1);
            }
        }
        ri = ri.saturating_add(1);
    }

    // 额外格：字符串无终止符两处（execve 的 path、execveat 的 path）。
    let str_cases: [(&str, usize); STR_EXTRA_CELLS as usize] =
        [("execve", 0), ("execveat", 1)];
    let mut si = 0usize;
    while si < str_cases.len() {
        if let Some((name, slot)) = str_cases.get(si).copied() {
            if let Some(desc) = desc_of(name) {
                let env = ProbeEnv::clean().with_nul(slot, false);
                let want = cell_expectation(&desc, slot, PtrClass::Valid, false);
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let (got, stages) = probe(&desc, &env);
                    let errno = match got {
                        Ok(()) => 0,
                        Err(e) => e,
                    };
                    let caught = errno == want && errno == EFAULT && stage_is_prefix(stages);
                    if errno != want {
                        mismatch = mismatch.saturating_add(1);
                    }
                    let rec = ledger.record(caught);
                    if rec.is_err() {
                        mismatch = mismatch.saturating_add(1);
                    }
                    round = round.saturating_add(1);
                }
            }
        }
        si = si.saturating_add(1);
    }

    // 额外格：向量计数越界三处（execve argv、execveat argv、process_vm_readv 第一组 iovec）。
    let vec_cases: [(&str, usize, i32); VEC_EXTRA_CELLS as usize] = [
        ("execve", 1, E2BIG),
        ("execveat", 2, E2BIG),
        ("process_vm_readv", 1, EINVAL),
    ];
    let mut vi = 0usize;
    while vi < vec_cases.len() {
        if let Some((name, slot, want)) = vec_cases.get(vi).copied() {
            if let Some(desc) = desc_of(name) {
                let env = ProbeEnv::clean().with_vec(slot, false);
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let (got, stages) = probe(&desc, &env);
                    let errno = match got {
                        Ok(()) => 0,
                        Err(e) => e,
                    };
                    let caught = errno == want && stage_is_prefix(stages);
                    if caught {
                        ebig_hits = ebig_hits.saturating_add(1);
                    } else {
                        mismatch = mismatch.saturating_add(1);
                    }
                    let rec = ledger.record(caught);
                    if rec.is_err() {
                        mismatch = mismatch.saturating_add(1);
                    }
                    round = round.saturating_add(1);
                }
            }
        }
        vi = vi.saturating_add(1);
    }

    (ledger, per_row, ebig_hits, mismatch)
}

// ── 计划交叉验证 ──────────────────────────────────────────────────────
/// 全表 × 全类别：计划路径（probe）与格判路径（cell_expectation）逐格一致。
pub fn run_plan_crosscheck() -> (u32, u32) {
    let classes = [
        PtrClass::Valid,
        PtrClass::Null,
        PtrClass::Unmapped,
        PtrClass::Kernel,
        PtrClass::ReadOnly,
    ];
    let mut agree = 0u32;
    let mut total = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(desc) = FAMILY.get(ri) {
            let (ops, n) = plan_for(desc);
            let mut oi = 0usize;
            while oi < n {
                if let Some(op) = ops.get(oi) {
                    let mut ci = 0usize;
                    while ci < classes.len() {
                        if let Some(class) = classes.get(ci).copied() {
                            let env = ProbeEnv::clean().with_class(op.slot as usize, class);
                            let want = cell_expectation(desc, op.slot as usize, class, true);
                            let (got, _stages) = probe(desc, &env);
                            let errno = match got {
                                Ok(()) => 0,
                                Err(e) => e,
                            };
                            total = total.saturating_add(1);
                            if errno == want {
                                agree = agree.saturating_add(1);
                            }
                        }
                        ci = ci.saturating_add(1);
                    }
                }
                oi = oi.saturating_add(1);
            }
        }
        ri = ri.saturating_add(1);
    }
    (agree, total)
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
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

/// 规范语料指纹：20 行 shape 四列 + 每行全合法探针 + 计划快照。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, FAMILY_ROWS as u32);

    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(desc) = FAMILY.get(i) {
            push_u32(&mut out, desc.nr);
            push_u32(&mut out, desc.policy.code());
            out.push(nargs_count(desc.shape));
            out.push(nargs_top(desc.shape));
            out.push(nargs_shift(desc.shape));
            out.push(desc.ptr_count());
            out.push(desc.write_count());
            out.push(desc.nullable_count());
            let (ops, n) = plan_for(desc);
            push_u32(&mut out, n as u32);
            let mut oi = 0usize;
            while oi < n {
                if let Some(op) = ops.get(oi) {
                    out.push(op.slot);
                    out.push(op.kind);
                    out.push(op.write as u8);
                    out.push(op.nullable as u8);
                    push_u32(&mut out, op.cap);
                }
                oi = oi.saturating_add(1);
            }
            let (got, stages) = probe(desc, &ProbeEnv::clean());
            out.push(stages);
            out.push(match got {
                Ok(()) => 0,
                Err(e) => (e & 0x7F) as u8,
            });
        }
        i = i.saturating_add(1);
    }

    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9294 判据集。
pub fn run_unx9294_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9294");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7B700;
    s.add(
        "段位守卫：0x40 对齐且落在 B11 窗口",
        seg_ok,
        "段基址唯一、0x40 对齐，且与相邻卡段互不覆盖",
    );

    let mut table_ok = FAMILY.len() == FAMILY_ROWS;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if let (Some(d), Some(want)) = (FAMILY.get(i), FROZEN_NARGS.get(i)) {
            let three_way = nargs_count(d.shape) == nargs_top(d.shape)
                && nargs_top(d.shape) == nargs_shift(d.shape);
            if !d.dense() || !three_way || d.nargs() != *want {
                table_ok = false;
            }
        }
        i = i.saturating_add(1);
    }
    s.add(
        "nargs 逐位提取：三路同值且逐行等于冻结列",
        table_ok && FAMILY.iter().all(|d| d.dense()),
        "逐槽计数、最高槽加一、右移剥字节三条路径必须互相印证",
    );

    let (nargs_sum, ptr_sum, write_sum, nullable_sum) = column_sums();
    let mut cols_ok = true;
    let mut k = 0usize;
    while k < FAMILY.len() {
        if let (Some(d), Some(p), Some(w), Some(nl)) = (
            FAMILY.get(k),
            FROZEN_PTRS.get(k),
            FROZEN_WRITES.get(k),
            FROZEN_NULLABLE.get(k),
        ) {
            if d.ptr_count() != *p || d.write_count() != *w || d.nullable_count() != *nl {
                cols_ok = false;
            }
        }
        k = k.saturating_add(1);
    }
    s.add(
        "冻结四列：55/21/9/8 与逐行账一致",
        cols_ok
            && nargs_sum == NARGS_TOTAL
            && ptr_sum == PTR_TOTAL as u32
            && write_sum == WRITE_TOTAL as u32
            && nullable_sum == NULLABLE_TOTAL as u32,
        "四列合计是表的指纹，任何一格改动都必须同步改冻结账",
    );

    let mut plan_ok = true;
    let mut p = 0usize;
    while p < FAMILY.len() {
        if let Some(d) = FAMILY.get(p) {
            let (ops, n) = plan_for(d);
            if n != d.ptr_count() as usize {
                plan_ok = false;
            }
            let mut oi = 0usize;
            let mut writes = 0u8;
            while oi < n {
                if let Some(op) = ops.get(oi) {
                    if !kind_is_ptr(op.kind) || op.cap == 0 {
                        plan_ok = false;
                    }
                    if op.write {
                        writes = writes.saturating_add(1);
                    }
                }
                oi = oi.saturating_add(1);
            }
            if writes != d.write_count() {
                plan_ok = false;
            }
        }
        p = p.saturating_add(1);
    }
    s.add(
        "单源：拷贝计划长度与方向逐行等于 shape 派生列",
        plan_ok,
        "计划与 nargs 同源，手写长度或方向会立即让本判据变红",
    );

    let clone = match desc_of("clone") {
        Some(d) => d,
        None => SysDesc::new(0, "clone", 0, ValuePolicy::None),
    };
    let clone_ok = probe(&clone, &ProbeEnv::clean()).0 == Ok(());
    let bad_flags = probe(&clone, &ProbeEnv::clean().with_bad_scalar()).0;
    let bad_ptid = probe(
        &clone,
        &ProbeEnv::clean()
            .with_bad_scalar()
            .with_class(2, PtrClass::Kernel),
    )
    .0;
    let null_ptid = probe(
        &clone,
        &ProbeEnv::clean()
            .with_class(2, PtrClass::Null)
            .with_class(3, PtrClass::Null),
    )
    .0;
    s.add(
        "clone：全合法通过、坏 flags 先 EINVAL、可空写槽 NULL 合法",
        clone_ok
            && bad_flags == Err(EINVAL)
            && bad_ptid == Err(EINVAL)
            && null_ptid == Ok(()),
        "值域门先于指针门，且 WRITE_NULL 传 NULL 是合法用法",
    );

    let wait4 = match desc_of("wait4") {
        Some(d) => d,
        None => SysDesc::new(0, "wait4", 0, ValuePolicy::None),
    };
    let waitid = match desc_of("waitid") {
        Some(d) => d,
        None => SysDesc::new(0, "waitid", 0, ValuePolicy::None),
    };
    let times = match desc_of("times") {
        Some(d) => d,
        None => SysDesc::new(0, "times", 0, ValuePolicy::None),
    };
    let nullable_ok = probe(&wait4, &ProbeEnv::clean().with_class(3, PtrClass::Null)).0 == Ok(())
        && probe(&waitid, &ProbeEnv::clean().with_class(2, PtrClass::Null)).0 == Ok(())
        && probe(&times, &ProbeEnv::clean().with_class(0, PtrClass::Null)).0 == Ok(())
        && probe(&wait4, &ProbeEnv::clean().with_class(1, PtrClass::Null)).0 == Err(EFAULT);
    s.add(
        "可空槽：wait4/waitid/times 的 NULL 放行，非可空 NULL 仍 EFAULT",
        nullable_ok,
        "可空是逐槽属性，不是整表放宽",
    );

    let rusage_ro = probe(
        &wait4,
        &ProbeEnv::clean().with_class(3, PtrClass::ReadOnly),
    )
    .0;
    let status_ro = probe(
        &wait4,
        &ProbeEnv::clean().with_class(1, PtrClass::ReadOnly),
    )
    .0;
    let ptrace_ro = probe(
        &match desc_of("ptrace") {
            Some(d) => d,
            None => SysDesc::new(0, "ptrace", 0, ValuePolicy::None),
        },
        &ProbeEnv::clean().with_class(2, PtrClass::ReadOnly),
    )
    .0;
    s.add(
        "方向不对称：写向只读页 EFAULT、读向只读页放行",
        rusage_ro == Err(EFAULT) && status_ro == Err(EFAULT) && ptrace_ro == Ok(()),
        "把输入向与输出向混判会误杀只读入参或放行内核回写只读页",
    );

    let execve = match desc_of("execve") {
        Some(d) => d,
        None => SysDesc::new(0, "execve", 0, ValuePolicy::None),
    };
    let path_nonul = probe(&execve, &ProbeEnv::clean().with_nul(0, false)).0;
    let argv_short = probe(&execve, &ProbeEnv::clean().with_vec(1, false)).0;
    let pvm = match desc_of("process_vm_readv") {
        Some(d) => d,
        None => SysDesc::new(0, "process_vm_readv", 0, ValuePolicy::None),
    };
    let iov_bad = probe(&pvm, &ProbeEnv::clean().with_vec(1, false)).0;
    s.add(
        "字符串与向量：缺 NUL 回 EFAULT、exec 族越界 E2BIG、iovec 越界 EINVAL",
        path_nonul == Err(EFAULT) && argv_short == Err(E2BIG) && iov_bad == Err(EINVAL),
        "字符串要终止符，向量计数错按调用族分码",
    );

    let hole = SysDesc::new(
        999,
        "hole",
        s3(AK_SCALAR, AK_NONE, AK_SCALAR),
        ValuePolicy::None,
    );
    let hole_probe = probe(&hole, &ProbeEnv::clean());
    s.add(
        "稠密前缀：中间留空槽的 shape 在提取门回 EINVAL",
        !hole.dense()
            && hole_probe.0 == Err(EINVAL)
            && hole_probe.1 == ST_EXTRACT
            && nargs_count(hole.shape) != nargs_top(hole.shape),
        "非稠密描述字会让后续槽错位，必须在提取门拒绝",
    );

    let (cross_agree, cross_total) = run_plan_crosscheck();
    s.add(
        "单源交叉：计划路径与格判路径全格一致",
        cross_agree == cross_total && cross_total == PTR_TOTAL as u32 * 5,
        "两条实现路径独立成文，逐格一致性证明描述字无第二数据源",
    );

    let (ledger, per_row, ebig_hits, mismatch) = run_bad_ptr_matrix();
    let per_row_ok = {
        let mut ok = true;
        let mut j = 0usize;
        while j < per_row.len() {
            if let (Some(n), Some(d)) = (per_row.get(j), FAMILY.get(j)) {
                let want = d.ptr_count() as u32 * PTR_CLASSES * INJECT_ROUNDS;
                if *n != want {
                    ok = false;
                }
            }
            j = j.saturating_add(1);
        }
        ok
    };
    s.add(
        "坏指针矩阵：21 槽 × 4 类逐格 EFAULT/放行正确",
        per_row_ok && mismatch == 0 && ledger.injected == TOTAL_CELLS,
        "空/未映射/内核三类必须 EFAULT，可空 NULL 与只读入参必须放行",
    );
    s.add(
        "注入账：890 次全检出、逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.detected == TOTAL_CELLS
            && ebig_hits == VEC_EXTRA_CELLS * INJECT_ROUNDS,
        "注入必须等于检出，向量错码格各自独立计数",
    );

    let mut prefix_ok = true;
    let mut q = 0usize;
    while q < FAMILY.len() {
        if let Some(d) = FAMILY.get(q) {
            let (_r, st) = probe(d, &ProbeEnv::clean());
            if !stage_is_prefix(st) {
                prefix_ok = false;
            }
            let (_r2, st2) = probe(d, &ProbeEnv::clean().with_bad_scalar());
            if !stage_is_prefix(st2) {
                prefix_ok = false;
            }
        }
        q = q.saturating_add(1);
    }
    s.add(
        "阶段掩码：全部探针为 Extract→Value→Ptr 前缀",
        prefix_ok && stage_is_prefix(ST_EXTRACT) && stage_is_prefix(ST_PTR),
        "跳级掩码说明门被绕过，值域失败必须停在 0x3",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：描述字与计划指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    s.add(
        "诊断码：12 个码文案非空",
        ProbeCode::ALL.len() == 12 && ProbeCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议，禁止空串",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < ProbeCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < ProbeCode::ALL.len() {
            if let (Some(x), Some(y)) = (ProbeCode::ALL.get(a), ProbeCode::ALL.get(b)) {
                if x.val() == y.val() || x.code() == y.code() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复，否则判据无法定位",
    );

    let cell_text = format!(
        "格 {} 检出 {} 逃逸 {} 交叉不一致 {}",
        ledger.injected, ledger.detected, ledger.escaped, mismatch
    );
    s.add_dyn(
        "坏指针账快照",
        ledger.conserved() && mismatch == 0,
        &cell_text,
    );

    let col_text = format!(
        "列 {} 指针 {} 写 {} 可空 {}",
        nargs_sum, ptr_sum, write_sum, nullable_sum
    );
    s.add_dyn(
        "四列快照",
        cols_ok && nargs_sum == NARGS_TOTAL,
        &col_text,
    );

    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nargs_three_paths_agree() {
        let clone = match desc_of("clone") {
            Some(d) => d,
            None => SysDesc::new(0, "clone", 0, ValuePolicy::None),
        };
        assert_eq!(clone.nargs(), 5);
        assert_eq!(nargs_count(clone.shape), 5);
        assert_eq!(nargs_top(clone.shape), 5);
        assert_eq!(nargs_shift(clone.shape), 5);
        assert_eq!(clone.dense(), true);
    }

    #[test]
    fn dense_hole_is_rejected() {
        let hole = SysDesc::new(
            999,
            "hole",
            s3(AK_SCALAR, AK_NONE, AK_SCALAR),
            ValuePolicy::None,
        );
        assert_eq!(hole.dense(), false);
        assert_eq!(nargs_count(hole.shape), 2);
        assert_eq!(nargs_top(hole.shape), 3);
        assert_eq!(nargs_shift(hole.shape), 1);
        let (r, st) = probe(&hole, &ProbeEnv::clean());
        assert_eq!(r, Err(EINVAL));
        assert_eq!(st, ST_EXTRACT);
    }

    #[test]
    fn frozen_columns_match_table() {
        let (n, p, w, nl) = column_sums();
        assert_eq!(n, NARGS_TOTAL);
        assert_eq!(p, PTR_TOTAL as u32);
        assert_eq!(w, WRITE_TOTAL as u32);
        assert_eq!(nl, NULLABLE_TOTAL as u32);
        assert_eq!(FAMILY.len(), FAMILY_ROWS);
    }

    #[test]
    fn plan_is_derived_from_shape() {
        let execve = match desc_of("execve") {
            Some(d) => d,
            None => SysDesc::new(0, "execve", 0, ValuePolicy::None),
        };
        let (ops, n) = plan_for(&execve);
        assert_eq!(n, 3);
        assert_eq!(ops.first().map(|o| o.kind), Some(AK_STR));
        assert_eq!(ops.first().map(|o| o.cap), Some(CAP_STR));
        assert_eq!(ops.get(1).map(|o| o.write), Some(false));
    }

    #[test]
    fn nullable_write_accepts_null() {
        let times = match desc_of("times") {
            Some(d) => d,
            None => SysDesc::new(0, "times", 0, ValuePolicy::None),
        };
        assert_eq!(probe(&times, &ProbeEnv::clean()).0, Ok(()));
        assert_eq!(
            probe(&times, &ProbeEnv::clean().with_class(0, PtrClass::Null)).0,
            Ok(())
        );
        let wait4 = match desc_of("wait4") {
            Some(d) => d,
            None => SysDesc::new(0, "wait4", 0, ValuePolicy::None),
        };
        assert_eq!(
            probe(&wait4, &ProbeEnv::clean().with_class(1, PtrClass::Null)).0,
            Err(EFAULT)
        );
    }

    #[test]
    fn value_gate_precedes_pointer_gate() {
        let clone = match desc_of("clone") {
            Some(d) => d,
            None => SysDesc::new(0, "clone", 0, ValuePolicy::None),
        };
        let env = ProbeEnv::clean()
            .with_bad_scalar()
            .with_class(2, PtrClass::Kernel);
        let (r, st) = probe(&clone, &env);
        assert_eq!(r, Err(EINVAL));
        assert_eq!(st, ST_VALUE);
    }

    #[test]
    fn string_and_vector_errnos() {
        let execve = match desc_of("execve") {
            Some(d) => d,
            None => SysDesc::new(0, "execve", 0, ValuePolicy::None),
        };
        assert_eq!(
            probe(&execve, &ProbeEnv::clean().with_nul(0, false)).0,
            Err(EFAULT)
        );
        assert_eq!(
            probe(&execve, &ProbeEnv::clean().with_vec(1, false)).0,
            Err(E2BIG)
        );
        let pvm = match desc_of("process_vm_readv") {
            Some(d) => d,
            None => SysDesc::new(0, "process_vm_readv", 0, ValuePolicy::None),
        };
        assert_eq!(
            probe(&pvm, &ProbeEnv::clean().with_vec(1, false)).0,
            Err(EINVAL)
        );
    }

    #[test]
    fn crosscheck_paths_agree() {
        let (agree, total) = run_plan_crosscheck();
        assert_eq!(total, 105);
        assert_eq!(agree, 105);
    }

    #[test]
    fn bad_pointer_matrix_is_conserved() {
        let (led, per, ebig, mismatch) = run_bad_ptr_matrix();
        assert_eq!(led.injected, TOTAL_CELLS);
        assert_eq!(led.detected, TOTAL_CELLS);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        assert_eq!(ebig, 30);
        assert_eq!(mismatch, 0);
        assert_eq!(per.get(3).copied(), Some(120));
    }

    #[test]
    fn fingerprint_is_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9294_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
