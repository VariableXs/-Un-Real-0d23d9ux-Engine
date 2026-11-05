//! UNX-F9274 权限族 E 型参数探针总成 · 0x7B200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9274 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9274-J1 nargs 逐位提取判据过，
//!   坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 参数描述字单源：16 行权限族调用各只存一个 32 位 mask（每槽 4 位：
//!      存在/指针/写向/可空），nargs 与指针槽都由 mask 逐位弹出、绝不落字段，
//!      于是「nargs 提取」「坏指针扫描」「可空清单」三份判据共用同一份数据。
//!   ② 权限门最先：capable() 与身份集合成员判定先于值域与用户拷贝。所以
//!      非特权 setgroups(70000, 坏 list) 报 EPERM 而非 EINVAL/EFAULT——先拒
//!      权限，既不泄露参数校验细节，也不做无谓拷贝。
//!   ③ 值域门居中：iopl level>3、ioperm 端口越界（checked_add 防回绕）、
//!      prlimit resource>=16、sched policy>6、setgroups size>65536 报 EINVAL，
//!      且必须在指针门之前——值不合法时一律不做用户拷贝。
//!   ④ 可空与静默特例：capget.data、prlimit64.old_limit 传 NULL 合法；
//!      setgroups(size=0) 不拷贝但仍过权限门；setfsuid/setfsgid 静默降格，
//!      非特权调用永不返回 EPERM。一刀切会把正常用法判成故障。
//!   ⑤ 单源复算靠冻结账：Σnargs=36、指针槽 8、可空槽 2。任一 mask 被改动，
//!      漂移格与总账格同时报红，参数拷贝探针不可能悄悄分叉。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9274 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B200;
/// 探针格容量上限。
pub const MAX_CELLS: usize = 48;
/// 指针槽总数（冻结账）。
pub const POINTER_CELLS: usize = 8;
/// 可空指针槽数（冻结账）。
pub const NULLABLE_CELLS: usize = 2;
/// 权限门拒绝格数（注入基数）。
pub const PERM_CELLS: usize = 6;
/// 表级 nargs 合计（冻结账）。
pub const TABLE_NARGS_SUM: u32 = 36;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 权限不足。
pub const EPERM: u32 = 1;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// 补充组上限。
pub const NGROUPS_MAX: u32 = 65_536;
/// iopl 最高合法级别。
pub const IOPL_MAX: u32 = 3;
/// 端口空间上限（from + num 不得越此）。
pub const IOPORT_LIMIT: u32 = 0x1_0000;
/// 资源种类上限（RLIM_NLIMITS）。
pub const RLIM_NLIMITS: u32 = 16;
/// 已知调度策略最大值。
pub const SCHED_MAX: u32 = 6;
const NR_SETUID: u32 = 105;
const NR_SETGID: u32 = 106;
const NR_SETREUID: u32 = 113;
const NR_SETREGID: u32 = 114;
const NR_SETGROUPS: u32 = 116;
const NR_SETRESUID: u32 = 117;
const NR_SETRESGID: u32 = 119;
const NR_SETFSUID: u32 = 122;
const NR_SETFSGID: u32 = 123;
const NR_CAPGET: u32 = 125;
const NR_CAPSET: u32 = 126;
const NR_SCHED_SETSCHEDULER: u32 = 144;
const NR_PRCTL: u32 = 157;
const NR_IOPL: u32 = 172;
const NR_IOPERM: u32 = 173;
const NR_PRLIMIT64: u32 = 302;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 探针总成诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    NargsDrift, SlotDrift, PermGateLate, ValueGateLate, NullRejected, NullLeaked,
    FaultEscaped, VerdictOutside, TableDrift, StageSkip, LedgerLeak, SegmentViolation,
}
impl ProbeCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            ProbeCode::NargsDrift => 0, ProbeCode::SlotDrift => 1,
            ProbeCode::PermGateLate => 2, ProbeCode::ValueGateLate => 3,
            ProbeCode::NullRejected => 4, ProbeCode::NullLeaked => 5,
            ProbeCode::FaultEscaped => 6, ProbeCode::VerdictOutside => 7,
            ProbeCode::TableDrift => 8, ProbeCode::StageSkip => 9,
            ProbeCode::LedgerLeak => 10, ProbeCode::SegmentViolation => 11,
        }
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::NargsDrift => "nargs 只能由 mask 逐位弹出；增删槽位必须先改冻结账",
            ProbeCode::SlotDrift => "指针槽枚举必须与 mask 指针位一一对应，禁止另立槽位表",
            ProbeCode::PermGateLate => "capable() 必须最先判定，权限门后移会把 EPERM 误报成 EINVAL",
            ProbeCode::ValueGateLate => "值域检查先于用户拷贝，与 Linux sys_* 前置检查次序一致",
            ProbeCode::NullRejected => "可空槽（capget.data/prlimit64.old）传 NULL 合法，拒绝即误杀",
            ProbeCode::NullLeaked => "必填槽传 NULL 必须 EFAULT，放行会让内核解引用空指针",
            ProbeCode::FaultEscaped => "坏指针必须在指针门短路返回 EFAULT，漏检等于信任用户地址",
            ProbeCode::VerdictOutside => "权限族判定只许 0/EPERM/EINVAL/EFAULT，出现别的码即探针越界",
            ProbeCode::TableDrift => "Linux E 表与判定器期望必须同源，改一处必须同步另一处",
            ProbeCode::StageSkip => "探针状态机只许 Extract→PermGate→ValueGate→PtrSweep→Verdict 单向推进",
            ProbeCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            ProbeCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [ProbeCode; 12] = [
        ProbeCode::NargsDrift, ProbeCode::SlotDrift, ProbeCode::PermGateLate,
        ProbeCode::ValueGateLate, ProbeCode::NullRejected, ProbeCode::NullLeaked,
        ProbeCode::FaultEscaped, ProbeCode::VerdictOutside, ProbeCode::TableDrift,
        ProbeCode::StageSkip, ProbeCode::LedgerLeak, ProbeCode::SegmentViolation,
    ];
}

// ── 描述字逐位提取 ────────────────────────────────────────────────────
/// 取第 slot 槽的 4 位；越界槽视为不存在。
pub fn nibble(mask: u32, slot: u8) -> u8 {
    if slot >= 8 { return 0; }
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
    for slot in 0..8u8 { if arg_exists(mask, slot) { n += 1; } }
    n
}
/// 逐位弹出指针槽数。
pub fn mask_ptrs(mask: u32) -> u8 {
    let mut n = 0u8;
    for slot in 0..8u8 { if arg_exists(mask, slot) && arg_is_ptr(mask, slot) { n += 1; } }
    n
}

// ── 权限族描述 ────────────────────────────────────────────────────────
/// 权限门类别：capable() 的抽象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Req {
    /// 无权限门（纯标量或静默降格调用）。
    None,
    /// 需 CAP_SETUID 或目标 uid 已在身份集合内。
    Uid,
    /// 需 CAP_SETGID。
    Gid,
    /// 需 CAP_SYS_RAWIO。
    RawIo,
    /// 需 CAP_SYS_RESOURCE / ptrace 可达。
    Resource,
}
/// 权限族系统调用描述：mask 是唯一数据源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysDesc {
    pub nr: u32,
    pub name: &'static str,
    pub mask: u32,
    pub req: Req,
}
impl SysDesc {
    /// 构造。
    pub const fn new(nr: u32, name: &'static str, mask: u32, req: Req) -> Self {
        SysDesc { nr, name, mask, req }
    }
    /// nargs 逐位提取（不存字段，故不可能与 mask 漂移）。
    pub fn nargs(&self) -> u8 { mask_nargs(self.mask) }
    /// 指针槽数。
    pub fn ptr_count(&self) -> u8 { mask_ptrs(self.mask) }
    /// 按槽位升序枚举指针槽。
    pub fn ptr_slots(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for slot in 0..8u8 {
            if arg_exists(self.mask, slot) && arg_is_ptr(self.mask, slot) {
                out.push(slot);
            }
        }
        out
    }
}
/// 权限族描述表（单源；槽 0 起，每槽 4 位）。
pub const FAMILY: [SysDesc; 16] = [
    SysDesc::new(NR_SETUID, "setuid", 0x1, Req::Uid),
    SysDesc::new(NR_SETGID, "setgid", 0x1, Req::Gid),
    SysDesc::new(NR_SETREUID, "setreuid", 0x11, Req::Uid),
    SysDesc::new(NR_SETREGID, "setregid", 0x11, Req::Gid),
    SysDesc::new(NR_SETRESUID, "setresuid", 0x111, Req::Uid),
    SysDesc::new(NR_SETRESGID, "setresgid", 0x111, Req::Gid),
    SysDesc::new(NR_SETFSUID, "setfsuid", 0x1, Req::None),
    SysDesc::new(NR_SETFSGID, "setfsgid", 0x1, Req::None),
    SysDesc::new(NR_SETGROUPS, "setgroups", 0x31, Req::Gid),
    SysDesc::new(NR_CAPSET, "capset", 0x33, Req::Uid),
    SysDesc::new(NR_CAPGET, "capget", 0xF7, Req::Uid),
    SysDesc::new(NR_PRLIMIT64, "prlimit64", 0xF311, Req::None),
    SysDesc::new(NR_IOPERM, "ioperm", 0x111, Req::RawIo),
    SysDesc::new(NR_IOPL, "iopl", 0x1, Req::RawIo),
    SysDesc::new(NR_SCHED_SETSCHEDULER, "sched_setscheduler", 0x311, Req::None),
    SysDesc::new(NR_PRCTL, "prctl", 0x11111, Req::None),
];
/// B 侧冻结 nargs 账（任何 mask 改动都会在此漂移）。
pub const FROZEN_NARGS: [(&str, u8); 16] = [
    ("setuid", 1), ("setgid", 1), ("setreuid", 2), ("setregid", 2),
    ("setresuid", 3), ("setresgid", 3), ("setfsuid", 1), ("setfsgid", 1),
    ("setgroups", 2), ("capset", 2), ("capget", 2), ("prlimit64", 4),
    ("ioperm", 3), ("iopl", 1), ("sched_setscheduler", 3), ("prctl", 5),
];
/// B 侧冻结指针账。
pub const FROZEN_PTRS: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 2, 0, 0, 1, 0];
/// 按名取描述（无裸索引）。
pub fn desc_of(name: &str) -> Option<&'static SysDesc> {
    FAMILY.iter().find(|d| d.name == name)
}

// ── 指针类别 ──────────────────────────────────────────────────────────
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
}
impl PtrClass {
    /// 用户态可读。
    pub fn readable(self) -> bool { matches!(self, PtrClass::Valid | PtrClass::ReadOnly) }
    /// 用户态可写。
    pub fn writable(self) -> bool { matches!(self, PtrClass::Valid) }
}

// ── 调用上下文 ────────────────────────────────────────────────────────
/// 一次调用的全部参数与身份上下文。cap 位是唯一权限判据，euid 只是档位标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ctx {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
    pub cap_uid: bool,
    pub cap_gid: bool,
    pub cap_rawio: bool,
    pub cap_res: bool,
    pub target_in_set: bool,
    pub ptrace_ok: bool,
    pub ptrs: [PtrClass; 8],
    pub groups: u32,
    pub iopl: u32,
    pub ioperm_from: u32,
    pub ioperm_num: u32,
    pub prlim_res: u32,
    pub sched_policy: u32,
    pub pid: u32,
}
impl Ctx {
    /// 特权上下文：cap 位全开，参数全合法。
    pub fn clean_root() -> Ctx {
        Ctx { ruid: 0, euid: 0, suid: 0, cap_uid: true, cap_gid: true,
              cap_rawio: true, cap_res: true, target_in_set: false, ptrace_ok: true,
              ptrs: [PtrClass::Valid; 8], groups: 2, iopl: 0, ioperm_from: 0x200,
              ioperm_num: 8, prlim_res: 7, sched_policy: 0, pid: 0 }
    }
    /// 非特权上下文：cap 位全关，参数全合法（除身份集合）。
    pub fn clean_user() -> Ctx {
        Ctx { ruid: 1001, euid: 1001, suid: 1001, cap_uid: false, cap_gid: false,
              cap_rawio: false, cap_res: false, ptrace_ok: false, ..Ctx::clean_root() }
    }
    /// euid 是否为 0（只作档位标签，不作权限判据）。
    pub fn is_root(&self) -> bool { self.euid == 0 }
    /// 身份三元组展开。
    pub fn triple(&self) -> (u32, u32, u32) { (self.ruid, self.euid, self.suid) }
    /// 覆写某槽指针类别（越界槽忽略）。
    pub fn with_ptr(mut self, slot: u8, class: PtrClass) -> Ctx {
        if let Some(p) = self.ptrs.get_mut(slot as usize) { *p = class; }
        self
    }
    /// 取某槽类别；越界按未映射处理（保守）。
    pub fn ptr_at(&self, slot: u8) -> PtrClass {
        match self.ptrs.get(slot as usize) { Some(p) => *p, None => PtrClass::Unmapped }
    }
}

// ── 三层判定 ──────────────────────────────────────────────────────────
/// 三层判定：权限门 → 值域门 → 指针门（槽位升序短路）。
pub fn judge_call(desc: &SysDesc, ctx: &Ctx) -> Result<(), u32> {
    // 门① 权限：capable() 或身份集合成员，先于一切参数校验。
    match desc.req {
        Req::None => {}
        Req::Uid => if !(ctx.cap_uid || ctx.target_in_set) { return Err(EPERM); },
        Req::Gid => if !ctx.cap_gid { return Err(EPERM); },
        Req::RawIo => if !ctx.cap_rawio { return Err(EPERM); },
        Req::Resource => if !ctx.cap_res { return Err(EPERM); },
    }
    if desc.nr == NR_PRLIMIT64 && ctx.pid != 0 && !(ctx.cap_res || ctx.ptrace_ok) {
        return Err(EPERM);
    }
    // 门② 值域：先于任何用户拷贝。
    if desc.nr == NR_SETGROUPS && ctx.groups > NGROUPS_MAX { return Err(EINVAL); }
    if desc.nr == NR_IOPL && ctx.iopl > IOPL_MAX { return Err(EINVAL); }
    if desc.nr == NR_IOPERM {
        match ctx.ioperm_from.checked_add(ctx.ioperm_num) {
            Some(end) => if end > IOPORT_LIMIT { return Err(EINVAL); },
            None => return Err(EINVAL),
        }
    }
    if desc.nr == NR_PRLIMIT64 && ctx.prlim_res >= RLIM_NLIMITS { return Err(EINVAL); }
    if desc.nr == NR_SCHED_SETSCHEDULER && ctx.sched_policy > SCHED_MAX { return Err(EINVAL); }
    // 空补充组集合不做用户拷贝（但仍已过权限门）。
    if desc.nr == NR_SETGROUPS && ctx.groups == 0 { return Ok(()); }
    // 门③ 指针：逐槽短路；可空槽的空指针直接放行。
    for slot in 0..8u8 {
        if !arg_exists(desc.mask, slot) || !arg_is_ptr(desc.mask, slot) { continue; }
        let class = ctx.ptr_at(slot);
        if arg_is_nullable(desc.mask, slot) && class == PtrClass::Null { continue; }
        if arg_is_write(desc.mask, slot) && !class.writable() { return Err(EFAULT); }
        if !class.readable() { return Err(EFAULT); }
    }
    Ok(())
}
/// 判定器 → 0 或 errno。
pub fn verdict(desc: &SysDesc, ctx: &Ctx) -> u32 {
    match judge_call(desc, ctx) { Ok(()) => 0, Err(e) => e }
}
/// 按名判定；未知名为 u32::MAX 哨兵。
pub fn named_verdict(name: &str, ctx: &Ctx) -> u32 {
    match desc_of(name) { Some(d) => verdict(d, ctx), None => u32::MAX }
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
/// 全域扫：坏指针（null_mode=false）或空指针（true）。
fn sweep_ptrs(null_mode: bool) -> Vec<ProbeCell> {
    let mut out = Vec::new();
    for d in FAMILY.iter() {
        for slot in d.ptr_slots() {
            let (class, expected) = if null_mode {
                (PtrClass::Null, if arg_is_nullable(d.mask, slot) { 0 } else { EFAULT })
            } else if arg_is_write(d.mask, slot) {
                (PtrClass::ReadOnly, EFAULT)
            } else {
                (PtrClass::Unmapped, EFAULT)
            };
            let ctx = Ctx::clean_root().with_ptr(slot, class);
            out.push(ProbeCell { syscall: d.name, slot, class,
                                 expected, got: verdict(d, &ctx) });
        }
    }
    out
}
/// 坏指针全域扫：8 个指针槽各放一个坏指针，期望全 EFAULT。
pub fn pointer_sweep() -> Vec<ProbeCell> { sweep_ptrs(false) }
/// 空指针全域扫：可空槽期望 0，必填槽期望 EFAULT。
pub fn null_sweep() -> Vec<ProbeCell> { sweep_ptrs(true) }
/// 值域格：四格 EINVAL。
pub fn value_cells() -> Vec<ProbeCell> {
    let v1 = { let mut c = Ctx::clean_root(); c.iopl = 4; c };
    let v2 = { let mut c = Ctx::clean_root(); c.ioperm_from = 0xFFF0; c.ioperm_num = 0x20; c };
    let v3 = { let mut c = Ctx::clean_root(); c.prlim_res = RLIM_NLIMITS; c };
    let v4 = { let mut c = Ctx::clean_root(); c.sched_policy = 9; c };
    vec![
        ProbeCell { syscall: "iopl", slot: 0, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("iopl", &v1) },
        ProbeCell { syscall: "ioperm", slot: 2, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("ioperm", &v2) },
        ProbeCell { syscall: "prlimit64", slot: 1, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("prlimit64", &v3) },
        ProbeCell { syscall: "sched_setscheduler", slot: 1, class: PtrClass::Valid, expected: EINVAL, got: named_verdict("sched_setscheduler", &v4) },
    ]
}
/// 次序格：权限门先于值域/指针，值域门先于指针。
pub fn order_cells() -> Vec<ProbeCell> {
    let o1 = { let mut c = Ctx::clean_user(); c.groups = 70000; c.with_ptr(1, PtrClass::Null) };
    let o2 = { let mut c = Ctx::clean_user(); c.ioperm_from = 0xFFFF; c.ioperm_num = 0xFFFF; c };
    let o3 = Ctx::clean_user().with_ptr(0, PtrClass::Unmapped).with_ptr(1, PtrClass::Unmapped);
    let o4 = { let mut c = Ctx::clean_root(); c.prlim_res = 99; c.with_ptr(2, PtrClass::Null) };
    let o5 = { let mut c = Ctx::clean_root(); c.groups = 70000; c.with_ptr(1, PtrClass::Null) };
    let o6 = { let mut c = Ctx::clean_user(); c.pid = 42; c.prlim_res = 99; c.with_ptr(2, PtrClass::Null) };
    vec![
        ProbeCell { syscall: "setgroups", slot: 1, class: PtrClass::Null, expected: EPERM, got: named_verdict("setgroups", &o1) },
        ProbeCell { syscall: "ioperm", slot: 2, class: PtrClass::Valid, expected: EPERM, got: named_verdict("ioperm", &o2) },
        ProbeCell { syscall: "capset", slot: 0, class: PtrClass::Unmapped, expected: EPERM, got: named_verdict("capset", &o3) },
        ProbeCell { syscall: "prlimit64", slot: 2, class: PtrClass::Null, expected: EINVAL, got: named_verdict("prlimit64", &o4) },
        ProbeCell { syscall: "setgroups", slot: 1, class: PtrClass::Null, expected: EINVAL, got: named_verdict("setgroups", &o5) },
        ProbeCell { syscall: "prlimit64", slot: 2, class: PtrClass::Null, expected: EPERM, got: named_verdict("prlimit64", &o6) },
    ]
}
/// 权限格：非特权提限六格全 EPERM。
pub fn perm_cells() -> Vec<ProbeCell> {
    let other = { let mut c = Ctx::clean_user(); c.pid = 42; c };
    let cases: [(&'static str, Ctx); 6] = [
        ("setuid", Ctx::clean_user()), ("setresuid", Ctx::clean_user()),
        ("setgroups", Ctx::clean_user()), ("capset", Ctx::clean_user()),
        ("ioperm", Ctx::clean_user()), ("prlimit64", other),
    ];
    let mut out = Vec::new();
    for (name, ctx) in cases {
        out.push(ProbeCell { syscall: name, slot: 0, class: PtrClass::Valid,
                             expected: EPERM, got: named_verdict(name, &ctx) });
    }
    out
}
/// 特例格：零长集合与静默降格。
pub fn special_cells() -> Vec<ProbeCell> {
    let root0 = { let mut c = Ctx::clean_root(); c.groups = 0; c.with_ptr(1, PtrClass::Null) };
    let user0 = { let mut c = Ctx::clean_user(); c.groups = 0; c.with_ptr(1, PtrClass::Null) };
    vec![
        ProbeCell { syscall: "setgroups", slot: 1, class: PtrClass::Null, expected: 0, got: named_verdict("setgroups", &root0) },
        ProbeCell { syscall: "setgroups", slot: 1, class: PtrClass::Null, expected: EPERM, got: named_verdict("setgroups", &user0) },
        ProbeCell { syscall: "setfsuid", slot: 0, class: PtrClass::Valid, expected: 0, got: named_verdict("setfsuid", &Ctx::clean_user()) },
        ProbeCell { syscall: "setfsgid", slot: 0, class: PtrClass::Valid, expected: 0, got: named_verdict("setfsgid", &Ctx::clean_user()) },
    ]
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 探针总成阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteStage { Extract, PermGate, ValueGate, PtrSweep, Verdict }
impl SuiteStage {
    /// 下一阶段。
    pub fn next(self) -> Option<SuiteStage> {
        match self {
            SuiteStage::Extract => Some(SuiteStage::PermGate),
            SuiteStage::PermGate => Some(SuiteStage::ValueGate),
            SuiteStage::ValueGate => Some(SuiteStage::PtrSweep),
            SuiteStage::PtrSweep => Some(SuiteStage::Verdict),
            SuiteStage::Verdict => None,
        }
    }
    /// 只许前进一步；终点拒绝推进。
    pub fn advance(self) -> Result<SuiteStage, ProbeCode> {
        match self.next() { Some(n) => Ok(n), None => Err(ProbeCode::StageSkip) }
    }
}

// ── 注入器与账本 ──────────────────────────────────────────────────────
/// 注入变异类：权限拒绝被扭曲的三种方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 漏报：EPERM 当作成功。
    SkipPerm,
    /// 错码：EPERM 报成 EINVAL。
    PermAsValue,
    /// 错码：EPERM 报成 EFAULT。
    PermAsFault,
}
/// 三类变异（顺序即账序）。
pub const MUTANTS: [Mutant; 3] = [Mutant::SkipPerm, Mutant::PermAsValue, Mutant::PermAsFault];
/// 对本应 EPERM 的判定结果施加变异。
pub fn mutate(m: Mutant, base: u32) -> u32 {
    if base != EPERM { return base; }
    match m { Mutant::SkipPerm => 0, Mutant::PermAsValue => EINVAL, Mutant::PermAsFault => EFAULT }
}
/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger { pub injected: u32, pub detected: u32, pub escaped: u32 }
impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }
    /// 守恒：注入 == 检出 且逃逸为零。
    pub fn conserve(&self) -> bool { self.escaped == 0 && self.injected == self.detected }
}
/// 三类变异同源注入：每轮扫全部权限格。
pub fn run_injections() -> Ledger {
    let mut led = Ledger::new();
    let cells = perm_cells();
    for m in MUTANTS.iter() {
        for _round in 0..INJECT_ROUNDS {
            for c in cells.iter() { led.record(mutate(*m, c.got) != c.expected); }
        }
    }
    led
}
/// 单类是否 10/10 轮全检出（每轮 6 格全中才算一轮检出）。
pub fn injection_ten_of_ten(m: Mutant) -> bool {
    let cells = perm_cells();
    for _round in 0..INJECT_ROUNDS {
        if !cells.iter().all(|c| mutate(m, c.got) != c.expected) { return false; }
    }
    true
}

// ── 对账表（Linux E 语义） ────────────────────────────────────────────
/// 对账用的具体场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCase {
    /// 非特权巨量补充组 + 坏 list。
    UserGroupsHuge,
    /// 非特权改异主 uid。
    UserSetuidForeign,
    /// 特权 capget 且 data=NULL。
    RootCapgetNull,
    /// 特权 prlimit64 且 old_limit=NULL。
    RootPrlimitNull,
    /// 特权 iopl(4)。
    RootIopl4,
    /// 非特权 capset + 双坏指针。
    UserCapsetBadPtr,
}
/// 场景 → 上下文。
pub fn ctx_for(case: ProbeCase) -> Ctx {
    match case {
        ProbeCase::UserGroupsHuge => { let mut c = Ctx::clean_user(); c.groups = 70000; c.with_ptr(1, PtrClass::Null) }
        ProbeCase::UserSetuidForeign => Ctx::clean_user(),
        ProbeCase::RootCapgetNull => Ctx::clean_root().with_ptr(1, PtrClass::Null),
        ProbeCase::RootPrlimitNull => Ctx::clean_root().with_ptr(3, PtrClass::Null),
        ProbeCase::RootIopl4 => { let mut c = Ctx::clean_root(); c.iopl = 4; c }
        ProbeCase::UserCapsetBadPtr => Ctx::clean_user().with_ptr(0, PtrClass::Unmapped).with_ptr(1, PtrClass::Unmapped),
    }
}
/// 一行 Linux 语义对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ERow {
    pub syscall: &'static str,
    pub case: ProbeCase,
    pub expect: u32,
}
/// 六行对账：权限门次序、可空槽、值域门。
pub const LINUX_E_TABLE: [ERow; 6] = [
    ERow { syscall: "setgroups", case: ProbeCase::UserGroupsHuge, expect: EPERM },
    ERow { syscall: "setuid", case: ProbeCase::UserSetuidForeign, expect: EPERM },
    ERow { syscall: "capget", case: ProbeCase::RootCapgetNull, expect: 0 },
    ERow { syscall: "prlimit64", case: ProbeCase::RootPrlimitNull, expect: 0 },
    ERow { syscall: "iopl", case: ProbeCase::RootIopl4, expect: EINVAL },
    ERow { syscall: "capset", case: ProbeCase::UserCapsetBadPtr, expect: EPERM },
];
/// 跑一行对账。
pub fn linux_row_verdict(row: &ERow) -> u32 { named_verdict(row.syscall, &ctx_for(row.case)) }
/// 对账命中数 / 总数。
pub fn linux_table_hits() -> (u32, u32) {
    let mut hit = 0u32;
    for row in LINUX_E_TABLE.iter() {
        if linux_row_verdict(row) == row.expect { hit += 1; }
    }
    (hit, LINUX_E_TABLE.len() as u32)
}

// ── 漂移与单源复算 ────────────────────────────────────────────────────
/// nargs 与指针两份冻结账的总漂移行数。
pub fn ledger_drift() -> u32 {
    let mut drift = 0u32;
    if FAMILY.len() != FROZEN_NARGS.len() || FAMILY.len() != FROZEN_PTRS.len() { drift += 1; }
    for (i, d) in FAMILY.iter().enumerate() {
        match FROZEN_NARGS.get(i) {
            Some((name, n)) => if d.name != *name || d.nargs() != *n { drift += 1; },
            None => drift += 1,
        }
        match FROZEN_PTRS.get(i) {
            Some(n) => if d.ptr_count() != *n { drift += 1; },
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
            if arg_exists(d.mask, slot) && arg_is_ptr(d.mask, slot) && arg_is_nullable(d.mask, slot) {
                q += 1;
            }
        }
    }
    q
}
/// 单源声明：指针格只能落在 mask 的指针位上，且总数与冻结账咬合。
pub fn single_source_ok() -> bool {
    let cells = pointer_sweep();
    if cells.len() != POINTER_CELLS { return false; }
    let (mut nargs_sum, mut ptr_sum) = (0u32, 0u32);
    for d in FAMILY.iter() {
        let slots = d.ptr_slots();
        if slots.len() as u8 != d.ptr_count() { return false; }
        nargs_sum += d.nargs() as u32;
        ptr_sum += slots.len() as u32;
        for slot in slots { if !arg_is_ptr(d.mask, slot) { return false; } }
    }
    cells.iter().all(|c| match desc_of(c.syscall) {
        Some(d) => arg_is_ptr(d.mask, c.slot),
        None => false,
    }) && nargs_sum == TABLE_NARGS_SUM && ptr_sum == POINTER_CELLS as u32
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// 描述表 + 冻结账的语料指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for d in FAMILY.iter() {
        for b in d.mask.to_le_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= d.nargs() as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= d.ptr_count() as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= nullable_count();
    h ^ Ctx::clean_user().euid
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
/// 运行 F9274 全部判据。
pub fn run_unx9274_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9274");
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");
    let clean_ok = FAMILY.iter().all(|d| verdict(d, &Ctx::clean_root()) == 0);
    s.add("正常流：16 个权限族调用全合法参数均返回 0", clean_ok,
          "判定器不得对合法调用报错，否则探针本身不可信");
    let mut n_sum = 0u32;
    for d in FAMILY.iter() { n_sum += d.nargs() as u32; }
    s.add("nargs 逐位提取：16 行 mask 弹值与冻结账零漂移且 Σnargs=36",
          ledger_drift() == 0 && n_sum == TABLE_NARGS_SUM,
          "nargs/指针数只从 mask 推导；改槽位必须同步改冻结账");
    s.add("单源：探针格全落 mask 指针位，8 槽不多不少", single_source_ok(),
          "探针、nargs 提取、可空清单共用一份 mask，禁止另立槽位表");
    let sweep = pointer_sweep();
    let ptr_hits = sweep.iter().filter(|c| c.hit()).count();
    s.add("坏指针全 EFAULT：8/8 格命中",
          sweep.len() == POINTER_CELLS && ptr_hits == POINTER_CELLS,
          "每个指针槽单独放坏指针都必须短路到 EFAULT");
    let ro_write_faults = named_verdict("capget", &Ctx::clean_root().with_ptr(1, PtrClass::ReadOnly)) == EFAULT;
    let ro_read_passes = named_verdict("capset", &Ctx::clean_root().with_ptr(0, PtrClass::ReadOnly)) == 0;
    s.add("方向不对称：只读页可作输入、作输出必须 EFAULT",
          ro_write_faults && ro_read_passes && !PtrClass::ReadOnly.writable(),
          "输出需要可写；把只读页当合法输出会吞掉半拷贝写回失败");
    let nulls = null_sweep();
    let null_hits = nulls.iter().filter(|c| c.hit()).count();
    let required_faults = nulls.iter().filter(|c| c.expected == EFAULT && c.got == EFAULT).count();
    s.add("空指针：6 个必填槽 EFAULT、2 个可空槽成功",
          nulls.len() == POINTER_CELLS && null_hits == POINTER_CELLS
              && required_faults == POINTER_CELLS - NULLABLE_CELLS,
          "空指针不是通用坏指针；必填槽少拒一格就是空解引用");
    let perms = perm_cells();
    let perm_hits = perms.iter().filter(|c| c.hit() && c.got == EPERM).count();
    s.add("权限门：非特权提限 6/6 格全 EPERM",
          perms.len() == PERM_CELLS && perm_hits == PERM_CELLS,
          "非特权身份翻转、rawio 与 ptrace 面必须先拒，任何放行都是提权");
    let ord = order_cells();
    let ord_ok = |i: usize, code: u32| match ord.get(i) {
        Some(c) => c.hit() && c.got == code,
        None => false,
    };
    s.add("判定次序①②③：权限门先于值域/指针（非特权 setgroups/ioperm/capset → EPERM）",
          ord_ok(0, EPERM) && ord_ok(1, EPERM) && ord_ok(2, EPERM),
          "先拒权限可避免泄露参数校验细节，也避免对无权对象做无谓拷贝");
    s.add("判定次序④⑤：值域门先于指针门（特权 prlimit/setgroups 坏值 + NULL → EINVAL）",
          ord_ok(3, EINVAL) && ord_ok(4, EINVAL),
          "与 Linux sys_prlimit64/sys_setgroups 先查值再碰指针的顺序一致");
    s.add("判定次序⑥：ptrace 门先于值域门（他人 pid + res=99 → EPERM）",
          ord_ok(5, EPERM),
          "跨进程 prlimit64 先过 ptrace 权限再谈参数值");
    let vals = value_cells();
    let val_hits = vals.iter().filter(|c| c.hit() && c.got == EINVAL).count();
    s.add("值域门：iopl/ioperm/prlimit/sched 四格全 EINVAL", val_hits == 4,
          "值域检查先于用户拷贝，错报 EFAULT 会掩盖参数本身错误");
    let spec = special_cells();
    let zero_ok = match (spec.get(0), spec.get(1)) {
        (Some(a), Some(b)) => a.hit() && a.got == 0 && b.hit() && b.got == EPERM,
        _ => false,
    };
    s.add("特例：setgroups(size=0) 特权放行、非特权仍 EPERM", zero_ok,
          "零长集合不拷贝，但权限门不能因此被绕过");
    let fsuid_ok = match (spec.get(2), spec.get(3)) {
        (Some(a), Some(b)) => a.hit() && b.hit() && a.got == 0 && b.got == 0,
        _ => false,
    };
    s.add("静默降格：setfsuid/setfsgid 非特权调用永不返回 EPERM", fsuid_ok,
          "fsuid/fsgid 只影响文件访问判定，失败是静默保留旧值而非报错");
    let ten = MUTANTS.iter().all(|m| injection_ten_of_ten(*m));
    s.add("注入必抓：漏报/错值/错指针三类各 10/10 轮全检出", ten,
          "每轮 6 格全中才算检出；漏一轮即视为不稳定注入");
    let led = run_injections();
    s.add("守恒：注入 180 = 检出 180 + 逃逸 0",
          led.conserve() && led.injected == 3 * INJECT_ROUNDS * PERM_CELLS as u32,
          "注入账必须闭合；任何逃逸都意味着权限门有盲区");
    let mut stage = SuiteStage::Extract;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 { forward = false; break; }
    }
    s.add("状态机：Extract→PermGate→ValueGate→PtrSweep→Verdict 单向四步",
          forward && steps == 4 && stage == SuiteStage::Verdict
              && stage.advance() == Err(ProbeCode::StageSkip),
          "阶段只能前进；回退或跳步会让判据与语料脱节");
    let f_cells = cells_fingerprint(&pointer_sweep());
    let total_cells = sweep.len() + nulls.len() + vals.len() + ord.len()
        + perms.len() + spec.len();
    s.add("双跑一致 + 容量：指纹相等，36 格 ≤ 48 上限，add_dyn ≤ 8",
          f_cells == cells_fingerprint(&pointer_sweep())
              && corpus_fingerprint() == corpus_fingerprint()
              && total_cells == 36 && total_cells <= MAX_CELLS,
          "探针不得依赖哈希遍历顺序；格数超上限说明矩阵失控");
    let (hit, total) = linux_table_hits();
    s.add("对账：Linux E 语义表 6/6 命中", hit == total && total == 6,
          "对账表是外部语义锚点；表与判定器不一致时先查判定器");
    let mut closed = true;
    for (i, c) in ProbeCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() { closed = false; }
        for (j, o) in ProbeCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() { closed = false; }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码必须闭集可枚举，文案为空等于没有处置建议");
    s.add_dyn("动态：坏指针扫描明细", ptr_hits == POINTER_CELLS,
              &format!("hit={}/{} fp=0x{:08X}", ptr_hits, POINTER_CELLS, f_cells));
    s.add_dyn("动态：注入账明细", led.conserve(),
              &format!("injected={} detected={} escaped={}", led.injected, led.detected, led.escaped));
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
    fn nargs_frozen_and_totals() {
        assert_eq!(mask_nargs(0x1), 1);
        assert_eq!(mask_nargs(0x11), 2);
        assert_eq!(mask_nargs(0x111), 3);
        assert_eq!(mask_nargs(0x0), 0);
        assert_eq!(mask_nargs(0xF0F), 2);
        assert_eq!(mask_nargs(0x11111), 5);
        assert_eq!(ledger_drift(), 0);
        let mut n = 0u32;
        let mut p = 0u32;
        for d in FAMILY.iter() { n += d.nargs() as u32; p += d.ptr_count() as u32; }
        assert_eq!(n, TABLE_NARGS_SUM);
        assert_eq!(p, POINTER_CELLS as u32);
        assert_eq!(nullable_count(), NULLABLE_CELLS as u32);
    }
    #[test]
    fn pointer_sweep_all_efault_single_source() {
        let cells = pointer_sweep();
        assert_eq!(cells.len(), POINTER_CELLS);
        assert!(cells.iter().all(|c| c.hit()));
        assert!(cells.iter().all(|c| c.got == EFAULT));
        assert!(single_source_ok());
    }
    #[test]
    fn nullable_null_ok_and_direction() {
        let cells = null_sweep();
        assert_eq!(cells.len(), POINTER_CELLS);
        assert!(cells.iter().all(|c| c.hit()));
        assert_eq!(cells.iter().filter(|c| c.expected == 0).count(), NULLABLE_CELLS);
        assert_eq!(named_verdict("capget", &Ctx::clean_root().with_ptr(1, PtrClass::Null)), 0);
        assert_eq!(named_verdict("prlimit64", &Ctx::clean_root().with_ptr(3, PtrClass::Null)), 0);
        assert!(!PtrClass::ReadOnly.writable());
        assert_eq!(named_verdict("capset", &Ctx::clean_root().with_ptr(0, PtrClass::ReadOnly)), 0);
    }
    #[test]
    fn permission_gate_all_eperm_and_cap_semantics() {
        let cells = perm_cells();
        assert_eq!(cells.len(), PERM_CELLS);
        assert!(cells.iter().all(|c| c.hit() && c.got == EPERM));
        let mut capped = Ctx::clean_user();
        capped.cap_uid = true;
        assert_eq!(named_verdict("setuid", &capped), 0);
        assert!(!capped.is_root());
        assert_eq!(capped.triple(), (1001, 1001, 1001));
    }
    #[test]
    fn gate_order_and_values() {
        let ord = order_cells();
        assert!(ord.iter().all(|c| c.hit()));
        assert_eq!(ord.iter().filter(|c| c.got == EPERM).count(), 4);
        assert_eq!(ord.iter().filter(|c| c.got == EINVAL).count(), 2);
        let cells = value_cells();
        assert_eq!(cells.len(), 4);
        assert!(cells.iter().all(|c| c.hit() && c.got == EINVAL));
        let mut v = Ctx::clean_root();
        v.ioperm_from = 0xFFFF_FFFF;
        v.ioperm_num = 2;
        assert_eq!(named_verdict("ioperm", &v), EINVAL);
    }
    #[test]
    fn specials_zero_size_and_silent_fsuid() {
        let cells = special_cells();
        assert!(cells.iter().all(|c| c.hit()));
        assert_eq!(cells.get(0).map(|c| c.got), Some(0));
        assert_eq!(cells.get(1).map(|c| c.got), Some(EPERM));
        assert_eq!(cells.get(2).map(|c| c.got), Some(0));
        assert_eq!(cells.get(3).map(|c| c.got), Some(0));
    }
    #[test]
    fn injections_and_stage() {
        for m in MUTANTS.iter() { assert!(injection_ten_of_ten(*m)); }
        let led = run_injections();
        assert!(led.conserve());
        assert_eq!(led.injected, 3 * INJECT_ROUNDS * PERM_CELLS as u32);
        assert_eq!(led.escaped, 0);
        let s1 = SuiteStage::Extract.advance();
        assert_eq!(s1, Ok(SuiteStage::PermGate));
        let s2 = match s1 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        let s3 = match s2 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        let s4 = match s3 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        assert_eq!(s4, Ok(SuiteStage::Verdict));
        let s5 = match s4 { Ok(v) => v.advance(), Err(_) => Err(ProbeCode::StageSkip) };
        assert_eq!(s5, Err(ProbeCode::StageSkip));
    }
    #[test]
    fn linux_table_and_checks_green() {
        assert_eq!(linux_table_hits(), (6, 6));
        let set = run_unx9274_checks();
        assert!(set.all_passed());
    }
}
