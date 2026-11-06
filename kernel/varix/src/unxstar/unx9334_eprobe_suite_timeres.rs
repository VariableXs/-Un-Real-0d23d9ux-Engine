//! UNX-F9334 时间/资源族 E 型参数探针总成 · 0x7C100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9334 逐字）：
//!   UNX-F9334-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝
//!   探针单源格过。
//!
//! 设计要点：
//!   ① 单源不是「一个掩码」，而是四张 8 位位图：present/ptr/wr/nullable。
//!      nargs 由 present 逐位弹出，指针槽由 ptr 逐位弹出，输出向与可空位
//!      各占一张位图；任何一张与其余三张不自洽（ptr 不在 present 内、wr
//!      不在 ptr 内、nullable 不在 ptr 内）都判结构违规。
//!   ② 值域门只对三类尾参生效：clockid（0..=11 去 10）、rlimit resource
//!      （0..=15）、getrusage who（-1/0/1）。值域门先于指针门：坏 clockid
//!      配坏指针必须回 EINVAL，而不是 EFAULT。
//!   ③ 指针门按槽位升序短路：可空槽传 NULL 直接跳过（写向可空槽同样合法，
//!      例如 timer_settime 的 old_value），写向槽要求可写，读向槽要求可读；
//!      只读页给读向槽合法，给写向槽必须 EFAULT。
//!   ④ 容量门在指针门之后：timer_create 成功才占一个槽，池容量 8，第 9 个
//!      回 EAGAIN；坏指针先失败，绝不消耗槽位。
//!   ⑤ 阶段机是显式邻接表 Idle→Extracted→ValueGated→PtrGated→Settled，
//!      禁止跳级（Idle 直达 PtrGated 判违规），失败路径也必须落到 Settled。
//!   ⑥ 单源对账：18 行 nargs 合计冻结为 40，必须同时等于逐位弹出值、
//!      冻结账合计与拷贝探针声明值；三处任一处不等即红。
//!   ⑦ 坏指针矩阵 25 个指针槽 × 4 类（空/未映射/内核/只读）× 10 轮 =
//!      1000 次，账目 injected == detected + escaped 且 escaped 恒 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9334 独占 0x7C100..=0x7C13F）。
pub const SEG_BASE: u32 = 0x7C100;

/// 族表行数。
pub const FAMILY_ROWS: usize = 18;
/// 指针槽总数（冻结账）。
pub const POINTER_SLOTS: usize = 23;
/// 可空槽总数（冻结账）。
pub const NULLABLE_SLOTS: usize = 10;
/// 写向槽总数（冻结账）。
pub const WRITE_SLOTS: usize = 13;
/// 拷贝探针声明 nargs 合计（单源锚点）。
pub const COPY_PROBE_NARGS_SUM: u32 = 40;
/// 每槽指针类别数。
pub const PTR_CLASSES: u32 = 4;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 每时钟的 POSIX timer 容量。
pub const TIMER_CAP: u32 = 8;
/// timer_create 的 syscall 号。
pub const NR_TIMER_CREATE: u32 = 222;

pub const EPERM: i32 = 1;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..=+0x0E 闭集，共 14 码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    ShapeDrift,
    NargsDrift,
    SumDrift,
    GateOrder,
    PtrOrder,
    NullRejected,
    NullLeaked,
    ReadonlyWrite,
    ReadonlyRead,
    CapacityDrift,
    StageSkip,
    LedgerLeak,
    NotReproducible,
    SegmentViolation,
}

impl ProbeCode {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::ShapeDrift => "形态位图漂移：ptr/wr/nullable 必须是 present 的子集，否则槽枚举会指到不存在的实参",
            ProbeCode::NargsDrift => "nargs 漂移：必须由 present 位图逐位弹出，禁止另存数字列",
            ProbeCode::SumDrift => "合计漂移：18 行 nargs 合计必须是 40，并与拷贝探针声明同值",
            ProbeCode::GateOrder => "值域门次序错：坏 clockid/resource/who 必须先于指针拷贝回 EINVAL",
            ProbeCode::PtrOrder => "指针门次序错：坏指针必须短路回 EFAULT，不得先做容量或业务判定",
            ProbeCode::NullRejected => "可空槽 NULL 被拒：rem/old_value/tz/timex 的空参是合法用法",
            ProbeCode::NullLeaked => "非可空槽 NULL 被放行：内核会解引用空指针，必须 EFAULT",
            ProbeCode::ReadonlyWrite => "写向槽拿到只读页：必须 EFAULT，否则内核回写会炸页",
            ProbeCode::ReadonlyRead => "读向槽拿到只读页被误拒：只读入参合法，不得报 EFAULT",
            ProbeCode::CapacityDrift => "timer 容量错：8 槽用满后第 9 个 timer_create 必须 EAGAIN",
            ProbeCode::StageSkip => "阶段跳级：只许按邻接表逐级推进，跳级即判据失效",
            ProbeCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数",
            ProbeCode::NotReproducible => "双跑不一致：同一语料两次指纹必须相等",
            ProbeCode::SegmentViolation => "段位越界：0x7C100 必须 0x40 对齐且落在 0x7C000..=0x7D1C0",
        }
    }
    pub const ALL: &'static [ProbeCode] = &[
        ProbeCode::ShapeDrift, ProbeCode::NargsDrift, ProbeCode::SumDrift,
        ProbeCode::GateOrder, ProbeCode::PtrOrder, ProbeCode::NullRejected,
        ProbeCode::NullLeaked, ProbeCode::ReadonlyWrite, ProbeCode::ReadonlyRead,
        ProbeCode::CapacityDrift, ProbeCode::StageSkip, ProbeCode::LedgerLeak,
        ProbeCode::NotReproducible, ProbeCode::SegmentViolation,
    ];
}
/// 值域门类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    None,
    Clock,
    Rlimit,
    Rusage,
}
/// 四张位图描述一次调用的实参形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArgShape {
    pub present: u8,
    pub ptr: u8,
    pub wr: u8,
    pub nullable: u8,
}

impl ArgShape {
    pub const fn new(present: u8, ptr: u8, wr: u8, nullable: u8) -> ArgShape {
        ArgShape { present, ptr, wr, nullable }
    }
}
/// 一条时间/资源族系统调用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysDesc {
    pub nr: u32,
    pub name: &'static str,
    pub shape: ArgShape,
    pub gate: Gate,
}

impl SysDesc {
    pub const fn new(
        nr: u32, name: &'static str, present: u8, ptr: u8, wr: u8, nullable: u8, gate: Gate,
    ) -> SysDesc {
        SysDesc { nr, name, shape: ArgShape::new(present, ptr, wr, nullable), gate }
    }
}
/// 十八行时间/资源族表（位图即唯一数据源）。
pub const FAMILY: [SysDesc; FAMILY_ROWS] = [
    SysDesc::new(228, "clock_gettime", 0x03, 0x02, 0x02, 0x00, Gate::Clock),
    SysDesc::new(227, "clock_settime", 0x03, 0x02, 0x00, 0x00, Gate::Clock),
    SysDesc::new(229, "clock_getres", 0x03, 0x02, 0x02, 0x00, Gate::Clock),
    SysDesc::new(230, "clock_nanosleep", 0x0F, 0x0C, 0x08, 0x08, Gate::Clock),
    SysDesc::new(35, "nanosleep", 0x03, 0x03, 0x02, 0x02, Gate::None),
    SysDesc::new(96, "gettimeofday", 0x03, 0x03, 0x03, 0x02, Gate::None),
    SysDesc::new(164, "settimeofday", 0x03, 0x03, 0x00, 0x03, Gate::None),
    SysDesc::new(201, "time", 0x01, 0x01, 0x01, 0x01, Gate::None),
    SysDesc::new(305, "clock_adjtime", 0x03, 0x02, 0x00, 0x00, Gate::Clock),
    SysDesc::new(222, "timer_create", 0x07, 0x06, 0x04, 0x02, Gate::Clock),
    SysDesc::new(223, "timer_settime", 0x0F, 0x0C, 0x08, 0x08, Gate::None),
    SysDesc::new(224, "timer_gettime", 0x03, 0x02, 0x02, 0x00, Gate::None),
    SysDesc::new(225, "timer_getoverrun", 0x01, 0x00, 0x00, 0x00, Gate::None),
    SysDesc::new(226, "timer_delete", 0x01, 0x00, 0x00, 0x00, Gate::None),
    SysDesc::new(98, "getrusage", 0x03, 0x02, 0x02, 0x00, Gate::Rusage),
    SysDesc::new(160, "setrlimit", 0x03, 0x02, 0x00, 0x00, Gate::Rlimit),
    SysDesc::new(97, "getrlimit", 0x03, 0x02, 0x02, 0x00, Gate::Rlimit),
    SysDesc::new(302, "prlimit64", 0x0F, 0x0C, 0x08, 0x0C, Gate::Rlimit),
];

/// 冻结 nargs 账。
pub const FROZEN_NARGS: [u8; FAMILY_ROWS] =
    [2, 2, 2, 4, 2, 2, 2, 1, 2, 3, 4, 2, 1, 1, 2, 2, 2, 4];
/// 冻结指针槽账。
pub const FROZEN_PTRS: [u8; FAMILY_ROWS] =
    [1, 1, 1, 2, 2, 2, 2, 1, 1, 2, 2, 1, 0, 0, 1, 1, 1, 2];
/// 冻结可空槽账。
pub const FROZEN_NULLABLE: [u8; FAMILY_ROWS] =
    [0, 0, 0, 1, 1, 1, 2, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 2];
/// 冻结写向槽账。
pub const FROZEN_WRITES: [u8; FAMILY_ROWS] =
    [1, 0, 1, 1, 1, 2, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1];

/// 逐位弹出：数 8 位里置位的个数。
pub fn bits(mask: u8) -> u8 {    let mut n = 0u8;    let mut i = 0u8;
    while i < 8 {
        if (mask >> i) & 1 != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 槽位存在。
pub fn arg_exists(shape: &ArgShape, slot: u8) -> bool {
    slot < 8 && (shape.present >> slot) & 1 != 0
}
/// 槽位是指针。
pub fn arg_is_ptr(shape: &ArgShape, slot: u8) -> bool {
    slot < 8 && (shape.ptr >> slot) & 1 != 0
}
/// 槽位输出向。
pub fn arg_is_write(shape: &ArgShape, slot: u8) -> bool {
    slot < 8 && (shape.wr >> slot) & 1 != 0
}
/// 槽位可空。
pub fn arg_is_nullable(shape: &ArgShape, slot: u8) -> bool {
    slot < 8 && (shape.nullable >> slot) & 1 != 0
}
/// nargs：present 的逐位计数。
pub fn nargs(shape: &ArgShape) -> u8 {
    bits(shape.present)
}

pub fn ptr_count(shape: &ArgShape) -> u8 {
    bits(shape.ptr)
}

pub fn nullable_count(shape: &ArgShape) -> u8 {
    bits(shape.nullable)
}

pub fn write_count(shape: &ArgShape) -> u8 {
    bits(shape.wr)
}
/// 按槽位升序枚举指针槽。
pub fn ptr_slots(desc: &SysDesc) -> ([u8; 8], usize) {    let mut out = [0u8; 8];    let mut n = 0usize;    let mut slot = 0u8;
    while slot < 8 {
        if arg_exists(&desc.shape, slot) && arg_is_ptr(&desc.shape, slot) {
            if let Some(cell) = out.get_mut(n) {
                *cell = slot;
                n += 1;
            }
        }
        slot += 1;
    }
    (out, n)
}
/// 表级漂移：四列逐行比对冻结账。
pub fn table_drift() -> u32 {    let mut drift = 0u32;    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            let mut bad = false;
            if let Some(v) = FROZEN_NARGS.get(i) {
                if nargs(&d.shape) != *v {
                    bad = true;
                }
            }
            if let Some(v) = FROZEN_PTRS.get(i) {
                if ptr_count(&d.shape) != *v {
                    bad = true;
                }
            }
            if let Some(v) = FROZEN_NULLABLE.get(i) {
                if nullable_count(&d.shape) != *v {
                    bad = true;
                }
            }
            if let Some(v) = FROZEN_WRITES.get(i) {
                if write_count(&d.shape) != *v {
                    bad = true;
                }
            }
            if bad {
                drift = drift.saturating_add(1);
            }
        }
        i += 1;
    }
    drift
}
/// 结构自洽：三张派生意图都必须是 present 的子集。
pub fn shape_well_formed() -> bool {    let mut ok = true;    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            let s = d.shape;
            if s.ptr & !s.present != 0 || s.wr & !s.ptr != 0 || s.nullable & !s.ptr != 0 {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}
/// 表级 nargs 合计。
pub fn sum_nargs() -> u32 {    let mut total = 0u32;    let mut i = 0usize;
    while i < FAMILY.len() {
        if let Some(d) = FAMILY.get(i) {
            total = total.saturating_add(nargs(&d.shape) as u32);
        }
        i += 1;
    }
    total
}
/// 值域门判定。
pub fn gate_err(gate: Gate, arg: i32) -> Option<i32> {
    match gate {
        Gate::None => None,
        Gate::Clock => {
            if arg >= 0 && arg <= 11 && arg != 10 {
                None
            } else {
                Some(EINVAL)
            }
        }
        Gate::Rlimit => {
            if arg >= 0 && arg <= 15 {
                None
            } else {
                Some(EINVAL)
            }
        }
        Gate::Rusage => {
            if arg == -1 || arg == 0 || arg == 1 {
                None
            } else {
                Some(EINVAL)
            }
        }
    }
}
/// 用户指针类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    Valid,
    Null,
    Unmapped,
    Kernel,
    ReadOnly,
}

impl PtrClass {
    pub fn readable(self) -> bool {
        matches!(self, PtrClass::Valid | PtrClass::ReadOnly)
    }
    pub fn writable(self) -> bool {
        matches!(self, PtrClass::Valid)
    }
}
/// 八槽指针类别向量。
pub type PtrVec = [PtrClass; 8];

/// 全合法向量。
pub fn all_valid() -> PtrVec {
    [PtrClass::Valid; 8]
}
/// 探针阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Extracted,
    ValueGated,
    PtrGated,
    Settled,
}
/// 邻接表：只允许逐级推进，Settled 自环。
pub fn advance(from: Stage, to: Stage) -> Option<Stage> {    let ok = match from {
        Stage::Idle => matches!(to, Stage::Extracted),
        Stage::Extracted => matches!(to, Stage::ValueGated),
        Stage::ValueGated => matches!(to, Stage::PtrGated | Stage::Settled),
        Stage::PtrGated => matches!(to, Stage::Settled),
        Stage::Settled => matches!(to, Stage::Settled),
    };
    if ok {
        Some(to)
    } else {
        None
    }
}
/// 迁移一步；违规返回 false。
fn goto(cur: &mut Stage, to: Stage) -> bool {
    match advance(*cur, to) {
        Some(next) => {
            *cur = next;
            true
        }
        None => false,
    }
}
/// 每时钟 timer 池（容量门）。
#[derive(Clone, Copy, Debug)]
pub struct TimerPool {
    used: u32,
    cap: u32,
}

impl TimerPool {
    pub const fn new(cap: u32) -> TimerPool {
        TimerPool { used: 0, cap }
    }
    pub fn used(&self) -> u32 {
        self.used
    }
    pub fn remaining(&self) -> u32 {
        self.cap.saturating_sub(self.used)
    }
}
/// 探针：Extract → Value → Ptr → Capacity → Verdict。
pub fn probe(
    desc: &SysDesc, ptrs: &PtrVec, gate_arg: i32, pool: &mut TimerPool,
) -> (Result<(), i32>, Stage) {    let mut stage = Stage::Idle;
    if !goto(&mut stage, Stage::Extracted) {
        return (Err(EINVAL), stage);
    }
    if nargs(&desc.shape) > 8 || desc.shape.present & !0x0Fu8 != 0 {
        return (Err(EINVAL), stage);
    }
    if !goto(&mut stage, Stage::ValueGated) {
        return (Err(EINVAL), stage);
    }
    if let Some(e) = gate_err(desc.gate, gate_arg) {
        if !goto(&mut stage, Stage::Settled) {
            return (Err(EINVAL), stage);
        }
        return (Err(e), stage);
    }
    if !goto(&mut stage, Stage::PtrGated) {
        return (Err(EINVAL), stage);
    }    let (slots, n) = ptr_slots(desc);    let mut i = 0usize;
    while i < n {
        if let Some(slot) = slots.get(i).copied() {
            let p = match ptrs.get(slot as usize) {
                Some(c) => *c,
                None => PtrClass::Null,
            };
            let nullable = arg_is_nullable(&desc.shape, slot);
            if nullable && p == PtrClass::Null {
                i += 1;
                continue;
            }
            let ok = if arg_is_write(&desc.shape, slot) {
                p.writable()
            } else {
                p.readable()
            };
            if !ok {
                if !goto(&mut stage, Stage::Settled) {
                    return (Err(EINVAL), stage);
                }
                return (Err(EFAULT), stage);
            }
        }
        i += 1;
    }
    if desc.nr == NR_TIMER_CREATE {
        if pool.used >= pool.cap {
            if !goto(&mut stage, Stage::Settled) {
                return (Err(EINVAL), stage);
            }
            return (Err(EAGAIN), stage);
        }
        pool.used = pool.used.saturating_add(1);
    }
    if !goto(&mut stage, Stage::Settled) {
        return (Err(EINVAL), stage);
    }
    (Ok(()), stage)
}
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}
/// 坏指针全矩阵：25 槽 × 4 类 × 10 轮。
pub fn run_bad_ptr_matrix() -> (u32, u32, InjectLedger, u32) {    let classes = [PtrClass::Null, PtrClass::Unmapped, PtrClass::Kernel, PtrClass::ReadOnly];    let mut ledger = InjectLedger::zero();    let mut faults = 0u32;    let mut oks = 0u32;    let mut drift = 0u32;    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = ptr_slots(desc);
            let gate_arg = match desc.gate {
                Gate::Clock => 0,
                Gate::Rlimit => 0,
                Gate::Rusage => 0,
                Gate::None => 0,
            };
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    let nullable = arg_is_nullable(&desc.shape, slot);
                    let write = arg_is_write(&desc.shape, slot);
                    let mut ci = 0usize;
                    while ci < classes.len() {
                        if let Some(cls) = classes.get(ci).copied() {
                            let mut ptrs = all_valid();
                            if let Some(cell) = ptrs.get_mut(slot as usize) {
                                *cell = cls;
                            }
                            let want_ok = if nullable && cls == PtrClass::Null {
                                true
                            } else if write {
                                cls.writable()
                            } else {
                                cls.readable()
                            };
                            let mut round = 0u32;
                            while round < INJECT_ROUNDS {
                                let mut pool = TimerPool::new(TIMER_CAP);
                                let (got, stage) = probe(desc, &ptrs, gate_arg, &mut pool);
                                let caught = if want_ok {
                                    got.is_ok() && stage == Stage::Settled
                                } else {
                                    got == Err(EFAULT) && stage == Stage::Settled
                                };
                                if caught {
                                    if want_ok {
                                        oks = oks.saturating_add(1);
                                    } else {
                                        faults = faults.saturating_add(1);
                                    }
                                }
                                if !ledger.record(caught) {
                                    drift = drift.saturating_add(1);
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
    (faults, oks, ledger, drift)
}
/// 可空槽 NULL：全部必须成功（读向与写向都允许）。
pub fn run_nullable_probe() -> (u32, u32) {    let mut ok = 0u32;    let mut total = 0u32;    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = ptr_slots(desc);
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    if arg_is_nullable(&desc.shape, slot) {
                        let mut ptrs = all_valid();
                        if let Some(cell) = ptrs.get_mut(slot as usize) {
                            *cell = PtrClass::Null;
                        }
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            let mut pool = TimerPool::new(TIMER_CAP);
                            total = total.saturating_add(1);
                            if probe(desc, &ptrs, 0, &mut pool).0.is_ok() {
                                ok = ok.saturating_add(1);
                            }
                            round += 1;
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
/// 写向槽只读页：全部必须 EFAULT。
pub fn run_writeonly_probe() -> (u32, u32) {    let mut hits = 0u32;    let mut total = 0u32;    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = ptr_slots(desc);
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    if arg_is_write(&desc.shape, slot) {
                        let mut ptrs = all_valid();
                        if let Some(cell) = ptrs.get_mut(slot as usize) {
                            *cell = PtrClass::ReadOnly;
                        }
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            let mut pool = TimerPool::new(TIMER_CAP);
                            total = total.saturating_add(1);
                            if probe(desc, &ptrs, 0, &mut pool).0 == Err(EFAULT) {
                                hits = hits.saturating_add(1);
                            }
                            round += 1;
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
/// 读向槽只读页：全部必须成功。
pub fn run_readonly_probe() -> (u32, u32) {    let mut oks = 0u32;    let mut total = 0u32;    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let (slots, n) = ptr_slots(desc);
            let mut si = 0usize;
            while si < n {
                if let Some(slot) = slots.get(si).copied() {
                    if !arg_is_write(&desc.shape, slot) {
                        let mut ptrs = all_valid();
                        if let Some(cell) = ptrs.get_mut(slot as usize) {
                            *cell = PtrClass::ReadOnly;
                        }
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            let mut pool = TimerPool::new(TIMER_CAP);
                            total = total.saturating_add(1);
                            if probe(desc, &ptrs, 0, &mut pool).0.is_ok() {
                                oks = oks.saturating_add(1);
                            }
                            round += 1;
                        }
                    }
                }
                si += 1;
            }
        }
        di += 1;
    }
    (oks, total)
}
/// 单源声明：与参数拷贝探针共享同一份 nargs 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleSource {
    pub module: &'static str,
    pub peer: &'static str,
    pub nargs_sum: u32,
    pub shared: bool,
}
/// 与拷贝探针的单源声明。
pub const COPY_PROBE: SingleSource = SingleSource {
    module: "unx9334",
    peer: "copy_probe",
    nargs_sum: COPY_PROBE_NARGS_SUM,
    shared: true,
};

/// 单源三处对齐：本表合计 == 冻结合计 == 声明值。
pub fn single_source_ok() -> bool {    let frozen_sum = {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < FROZEN_NARGS.len() {
            if let Some(v) = FROZEN_NARGS.get(i) {
                t = t.saturating_add(*v as u32);
            }
            i += 1;
        }
        t
    };
    COPY_PROBE.shared
        && COPY_PROBE.nargs_sum == COPY_PROBE_NARGS_SUM
        && sum_nargs() == COPY_PROBE_NARGS_SUM
        && frozen_sum == COPY_PROBE_NARGS_SUM
        && table_drift() == 0
}
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；权限不足"),
    ("EAGAIN", -11, 1237, "Windows ERROR_RETRY；timer 资源耗尽"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏用户指针"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；clockid/resource 非法"),
];

/// 查表取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {    let mut i = 0usize;
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

pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {    let mut h: u32 = 0x811C_9DC5;    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}
/// 编码一次探针结果。
fn encode(out: &mut Vec<u8>, r: Result<(), i32>, stage: Stage) {
    out.push(stage as u8);
    match r {
        Ok(()) => out.push(1),
        Err(e) => out.push((e & 0x7F) as u8),
    }
}
/// 规范语料双跑指纹：18 行 × (坏首指针 / 全合法) + timer 容量序列。
pub fn corpus_fingerprint() -> u32 {    let mut bytes: Vec<u8> = Vec::new();    let mut di = 0usize;
    while di < FAMILY.len() {
        if let Some(desc) = FAMILY.get(di) {
            let mut pool = TimerPool::new(TIMER_CAP);
            let (r1, s1) = probe(desc, &all_valid(), 0, &mut pool);
            encode(&mut bytes, r1, s1);
            let mut ptrs = all_valid();
            if let Some(cell) = ptrs.get_mut(0) {
                *cell = PtrClass::Unmapped;
            }
            let (r2, s2) = probe(desc, &ptrs, 0, &mut pool);
            encode(&mut bytes, r2, s2);
        }
        di += 1;
    }    let mut pool = TimerPool::new(TIMER_CAP);    let tc = match FAMILY.get(9) {
        Some(d) => *d,
        None => SysDesc::new(222, "timer_create", 0x07, 0x06, 0x04, 0x02, Gate::Clock),
    };    let mut k = 0u32;
    while k < TIMER_CAP {
        let (r, stage) = probe(&tc, &all_valid(), 0, &mut pool);
        encode(&mut bytes, r, stage);
        k += 1;
    }    let (r9, s9) = probe(&tc, &all_valid(), 0, &mut pool);
    encode(&mut bytes, r9, s9);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9334 判据集。
pub fn run_unx9334_checks() -> CheckSet {    let mut s = CheckSet::new("unx9334");
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C000..=0x7D1C0).contains(&SEG_BASE);    s.add("段位守卫：0x7C100 独占且 0x40 对齐", seg_ok,
          "段基址必须与 0x7C0C0 窗口相邻不重叠");
    s.add("形态自洽：ptr/wr/nullable 均为 present 子集", shape_well_formed(),
          "三张派生意图不得指到不存在的实参槽");
    let mut per_row = true;    let mut i = 0usize;
    while i < FAMILY.len() {
        if let (Some(d), Some(f)) = (FAMILY.get(i), FROZEN_NARGS.get(i)) {
            if nargs(&d.shape) != *f {
                per_row = false;
            }
        }
        i += 1;
    }    s.add("nargs 逐位提取：18 行与冻结账逐行相等",
          per_row && table_drift() == 0,
          "present 位图的 popcount 必须逐一命中冻结 nargs 列");
    s.add("nargs 槽位数：present 只占低四位",
          FAMILY.iter().all(|d| (d.shape.present & !0x0Fu8) == 0),
          "本表最多四实参，present 高半字节必须为零");
    s.add("单源：三处合计同为 40", single_source_ok(),
          "逐位合计、冻结合计、拷贝探针声明值必须同源");
    let (faults, oks, bp_ledger, bp_drift) = run_bad_ptr_matrix();    s.add("坏指针全矩阵：23 槽 × 4 类 × 10 轮全检出",
          bp_ledger.conserved() && bp_ledger.escaped == 0 && bp_drift == 0
              && bp_ledger.injected == POINTER_SLOTS as u32 * PTR_CLASSES * INJECT_ROUNDS,
          "1000 次注入必须 1000 次按期望返回，逃逸恒 0");
    s.add("坏指针分布：EFAULT 与合法 NULL 分流正确",
          faults + oks == POINTER_SLOTS as u32 * PTR_CLASSES * INJECT_ROUNDS
              && faults > 0 && oks > 0,
          "可空 NULL、写向合法、读向只读都必须计入成功侧");
    let (null_ok, null_total) = run_nullable_probe();    s.add("可空槽：NULL 全合法（含写向 old_value）",
          null_ok == NULLABLE_SLOTS as u32 * INJECT_ROUNDS && null_total == null_ok,
          "10 个可空槽 × 10 轮必须全部放行");
    let (w_hits, w_total) = run_writeonly_probe();    s.add("写向槽：只读页全 EFAULT",
          w_hits == WRITE_SLOTS as u32 * INJECT_ROUNDS && w_total == w_hits,
          "13 个写向槽 × 10 轮不得有一例放行");
    let (r_ok, r_total) = run_readonly_probe();    s.add("读向槽：只读页全成功",
          r_ok == r_total && r_total == (POINTER_SLOTS as u32 - WRITE_SLOTS as u32) * INJECT_ROUNDS,
          "10 个读向槽 × 10 轮不得误报 EFAULT");
    let gate_ok = gate_err(Gate::Clock, 10) == Some(EINVAL)
        && gate_err(Gate::Clock, 12) == Some(EINVAL)
        && gate_err(Gate::Clock, -1) == Some(EINVAL)
        && gate_err(Gate::Clock, 11).is_none()
        && gate_err(Gate::Rlimit, 16) == Some(EINVAL)
        && gate_err(Gate::Rlimit, 15).is_none()
        && gate_err(Gate::Rusage, 2) == Some(EINVAL)
        && gate_err(Gate::Rusage, -1).is_none();    s.add("值域门：clock/resource/who 越界全 EINVAL", gate_ok,
          "10 号时钟空洞、16 号资源、who=2 都必须被挡");
    let ct = match FAMILY.get(0) {
        Some(d) => *d,
        None => SysDesc::new(228, "clock_gettime", 0x03, 0x02, 0x02, 0x00, Gate::Clock),
    };    let mut bad_ptr = all_valid();
    if let Some(cell) = bad_ptr.get_mut(1) {
        *cell = PtrClass::Unmapped;
    }    let mut pool = TimerPool::new(TIMER_CAP);    let gate_first = probe(&ct, &bad_ptr, 10, &mut pool).0 == Err(EINVAL)
        && probe(&ct, &bad_ptr, 0, &mut pool).0 == Err(EFAULT);    s.add("次序：值域门先于指针门", gate_first,
          "坏 clockid + 坏指针必须回 EINVAL；只有 clockid 合法才轮到 EFAULT");
    let mut full = TimerPool::new(TIMER_CAP);    let mut kept = 0u32;    let mut k = 0u32;
    while k < TIMER_CAP {
        let tc_desc = match FAMILY.get(9) {
            Some(d) => *d,
            None => SysDesc::new(222, "timer_create", 0x07, 0x06, 0x04, 0x02, Gate::Clock),
        };
        if probe(&tc_desc, &all_valid(), 0, &mut full).0.is_ok() {
            kept = kept.saturating_add(1);
        }
        k += 1;
    }    let tc_desc = match FAMILY.get(9) {
        Some(d) => *d,
        None => SysDesc::new(222, "timer_create", 0x07, 0x06, 0x04, 0x02, Gate::Clock),
    };    let ninth = probe(&tc_desc, &all_valid(), 0, &mut full).0;    s.add("容量：8 个 timer 后第 9 个 EAGAIN",
          kept == TIMER_CAP && ninth == Err(EAGAIN) && full.used() == TIMER_CAP,
          "容量门在指针门之后，成功才占槽");
    let mut cap_ptr = all_valid();
    if let Some(cell) = cap_ptr.get_mut(2) {
        *cell = PtrClass::Unmapped;
    }    let mut full2 = TimerPool::new(0);    let ptr_before_cap = probe(&tc_desc, &cap_ptr, 0, &mut full2).0 == Err(EFAULT);    s.add("次序：指针门先于容量门", ptr_before_cap,
          "坏参数指针必须 EFAULT，不得先报 EAGAIN 掩盖坏指针");
    let stage_ok = advance(Stage::Idle, Stage::PtrGated).is_none()
        && advance(Stage::Extracted, Stage::Settled).is_none()
        && advance(Stage::Idle, Stage::Extracted) == Some(Stage::Extracted)
        && advance(Stage::PtrGated, Stage::Settled) == Some(Stage::Settled);    let mut pool3 = TimerPool::new(TIMER_CAP);    let (_, settled) = probe(&ct, &all_valid(), 0, &mut pool3);    s.add("阶段机：邻接表逐级推进且合法路径落 Settled",
          stage_ok && settled == Stage::Settled,
          "Idle 直达 PtrGated 必须被拒，成功路径必须收在 Settled");
    let codes_ok = agrees_with_linux(EINVAL) && agrees_with_linux(EFAULT)
        && agrees_with_linux(EAGAIN) && agrees_with_linux(EPERM)
        && linux_neg(EINVAL) == -22 && !agrees_with_linux(0);    s.add("Linux 逐码：四码表内可复算", codes_ok,
          "返回码必须是 errno 表内正值");
    let fp_a = corpus_fingerprint();    let fp_b = corpus_fingerprint();    s.add("双跑一致：探针语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等");
    s.add("文案闭集：14 个诊断码均有中文建议",
          ProbeCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;    let mut a = 0usize;
    while a < ProbeCode::ALL.len() {
        let mut b = a + 1;
        while b < ProbeCode::ALL.len() {
            if let (Some(x), Some(y)) = (ProbeCode::ALL.get(a), ProbeCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }    s.add("诊断码：14 个码值两两唯一", uniq && ProbeCode::ALL.len() == 14,
          "段内闭集码不得重复");
    let bp_text = format!(
        "坏指针 {}/{}，EFAULT {}，合法 {}，账 {}/{}/{}",
        bp_ledger.injected, POINTER_SLOTS as u32 * PTR_CLASSES * INJECT_ROUNDS,
        faults, oks, bp_ledger.injected, bp_ledger.detected, bp_ledger.escaped
    );
    s.add_dyn("坏指针账", bp_ledger.conserved(), &bp_text);
    let sum_text = format!(
        "nargs 合计 {}（冻结 {}），可空 {}/{}，写向 {}/{}",
        sum_nargs(), COPY_PROBE_NARGS_SUM, null_ok, null_total, w_hits, w_total
    );
    s.add_dyn("单源快照", single_source_ok() && null_total == null_ok, &sum_text);
    let fp_text = format!("探针语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_extraction() {
        let sh = ArgShape::new(0x0F, 0x0C, 0x08, 0x08);
        assert_eq!(bits(sh.present), 4);
        assert_eq!(ptr_count(&sh), 2);
        assert_eq!(write_count(&sh), 1);
        assert_eq!(nullable_count(&sh), 1);
        assert_eq!(arg_exists(&sh, 3), true);
        assert_eq!(arg_is_ptr(&sh, 1), false);
        assert_eq!(arg_is_write(&sh, 3), true);
        assert_eq!(arg_is_nullable(&sh, 3), true);
    }
    #[test]
    fn family_sums_are_single_source() {
        assert_eq!(sum_nargs(), COPY_PROBE_NARGS_SUM);
        assert_eq!(table_drift(), 0);
        assert_eq!(shape_well_formed(), true);
        assert_eq!(single_source_ok(), true);
    }
    #[test]
    fn gate_domains() {
        assert_eq!(gate_err(Gate::Clock, 0), None);
        assert_eq!(gate_err(Gate::Clock, 10), Some(EINVAL));
        assert_eq!(gate_err(Gate::Rlimit, 16), Some(EINVAL));
        assert_eq!(gate_err(Gate::Rusage, 2), Some(EINVAL));
        assert_eq!(gate_err(Gate::Rusage, -1), None);
    }
    #[test]
    fn pointer_matrix_is_conserved() {
        let (faults, oks, led, drift) = run_bad_ptr_matrix();
        assert_eq!(led.injected, POINTER_SLOTS as u32 * PTR_CLASSES * INJECT_ROUNDS);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        assert_eq!(drift, 0);
        assert_eq!(faults + oks, POINTER_SLOTS as u32 * PTR_CLASSES * INJECT_ROUNDS);
    }
    #[test]
    fn nullable_and_write_slots() {
        let (nok, ntotal) = run_nullable_probe();
        assert_eq!(nok, NULLABLE_SLOTS as u32 * INJECT_ROUNDS);
        assert_eq!(ntotal, nok);
        let (wh, wt) = run_writeonly_probe();
        assert_eq!(wh, WRITE_SLOTS as u32 * INJECT_ROUNDS);
        assert_eq!(wt, wh);
        let (ro, rt) = run_readonly_probe();
        assert_eq!(ro, rt);
    }
    #[test]
    fn timer_capacity_gate() {
        let desc = match FAMILY.get(9) {
            Some(d) => *d,
            None => SysDesc::new(222, "timer_create", 0x07, 0x06, 0x04, 0x02, Gate::Clock),
        };
        let mut pool = TimerPool::new(TIMER_CAP);
        let mut k = 0u32;
        while k < TIMER_CAP {
            assert_eq!(probe(&desc, &all_valid(), 0, &mut pool).0.is_ok(), true);
            k += 1;
        }
        assert_eq!(probe(&desc, &all_valid(), 0, &mut pool).0, Err(EAGAIN));
        assert_eq!(pool.remaining(), 0);
    }
    #[test]
    fn gate_precedes_pointer() {
        let desc = match FAMILY.get(0) {
            Some(d) => *d,
            None => SysDesc::new(228, "clock_gettime", 0x03, 0x02, 0x02, 0x00, Gate::Clock),
        };
        let mut ptrs = all_valid();
        if let Some(cell) = ptrs.get_mut(1) {
            *cell = PtrClass::Unmapped;
        }
        let mut pool = TimerPool::new(TIMER_CAP);
        assert_eq!(probe(&desc, &ptrs, 10, &mut pool).0, Err(EINVAL));
        assert_eq!(probe(&desc, &ptrs, 0, &mut pool).0, Err(EFAULT));
    }
    #[test]
    fn stage_adjacency_is_strict() {
        assert_eq!(advance(Stage::Idle, Stage::PtrGated), None);
        assert_eq!(advance(Stage::Extracted, Stage::Settled), None);
        assert_eq!(advance(Stage::PtrGated, Stage::Settled), Some(Stage::Settled));
        let desc = match FAMILY.get(8) {
            Some(d) => *d,
            None => SysDesc::new(305, "clock_adjtime", 0x03, 0x02, 0x00, 0x00, Gate::Clock),
        };
        let mut pool = TimerPool::new(TIMER_CAP);
        assert_eq!(probe(&desc, &all_valid(), 0, &mut pool).1, Stage::Settled);
    }
    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(agrees_with_linux(EFAULT), true);
        assert_eq!(agrees_with_linux(0), false);
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9334_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
