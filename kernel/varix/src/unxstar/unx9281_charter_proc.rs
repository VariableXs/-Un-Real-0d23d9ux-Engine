//! UNX-F9281 进程族 E 型总纲（进程空间错误面） · 0x7B3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9281-J1 逐字）：
//!   UNX-F9281-J1 进程空间错误面判据过，四类错误域（pid 不存在/权限/
//!   资源耗尽/参数坏值）逐格落账，判序协议声明格过。
//!
//! 设计要点：
//!   ① 四域闭集：NoPid→ESRCH(3)、Perm→EPERM(1)、Resource→EAGAIN(11)、
//!      BadValue→EINVAL(22)；域外错误码不得进入宪章账。
//!   ② 十二行宪章表（四域各 3 条），每行带真实系统调用场景与四枚条件位，
//!      `judge` 逐行复算必须命中本行期望域，禁止只声明不复算。
//!   ③ 判序协议显式声明：坏值 → pid → 权限 → 资源，短路返回首个违约域；
//!      fork 特例跳过 pid 检查（父进程必然存在），但其余三序不变。
//!   ④ 域账逐格落账：4 域 × 3 格 = 12 格，满格即完整，溢出格拒绝并计数。
//!   ⑤ 注入四类（pid 误报权限/权限误报 pid/资源误报坏值/判序倒置）各
//!      10 轮，10/10 必抓，注入 40 = 检出 40 + 逃逸 0。
//!   ⑥ 状态机 Declare→Recompute→Inject→Ledger→Sealed 只进不退；语料指纹
//!      双跑逐位相等，整数统计，无随机无墙钟。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9281 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B3C0;

/// 错误域个数（pid/权限/资源/坏值）。
pub const DOMAIN_COUNT: usize = 4;

/// 每域宪章行数。
pub const ROWS_PER_DOMAIN: u32 = 3;

/// 宪章表总行数（4 × 3）。
pub const RULE_COUNT: usize = 12;

/// 宪章表容量上限（容量守卫用）。
pub const ROW_CAP: usize = 16;

/// 单类注入的复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

// ── 四类错误域 ────────────────────────────────────────────────────────
/// 进程空间错误域闭集（顺序即判序声明顺序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrDomain {
    /// pid 不存在：ESRCH(3)
    NoPid,
    /// 权限不足：EPERM(1)
    Perm,
    /// 资源耗尽：EAGAIN(11)
    Resource,
    /// 参数坏值：EINVAL(22)
    BadValue,
}

impl ErrDomain {
    /// Linux errno 值（四域一一对应，域外码一律拒绝）。
    pub fn errno(self) -> i32 {
        match self { ErrDomain::NoPid => 3, ErrDomain::Perm => 1, ErrDomain::Resource => 11, ErrDomain::BadValue => 22, }
    }

    /// 短名（诊断与格账用）。
    pub fn name(self) -> &'static str {
        match self { ErrDomain::NoPid => "pid-absent", ErrDomain::Perm => "perm", ErrDomain::Resource => "resource", ErrDomain::BadValue => "bad-value", }
    }

    /// 域下标（0..=3，格账定位）。
    pub fn index(self) -> usize {
        match self { ErrDomain::NoPid => 0, ErrDomain::Perm => 1, ErrDomain::Resource => 2, ErrDomain::BadValue => 3, }
    }

    /// 闭集全量。
    pub const ALL: [ErrDomain; 4] = [ErrDomain::NoPid, ErrDomain::Perm, ErrDomain::Resource, ErrDomain::BadValue,];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..=+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcCode {
    /// 域外错误码
    OutOfDomain,
    /// pid 不存在判据失配
    UnknownPid,
    /// 权限判据失配
    PermDenied,
    /// 资源耗尽判据失配
    ResourceExhausted,
    /// 坏值判据失配
    BadValue,
    /// 判序与声明不一致
    OrderViolation,
    /// fork 例外声明缺失或失效
    ForkExceptionMissing,
    /// 宪章行不齐
    RowsIncomplete,
    /// 格账与复算不符
    DomainMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 状态机倒退
    StageRegress,
    /// 段位越界
    SegmentViolation,
}

impl ProcCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self { ProcCode::OutOfDomain => "错误码不在四域闭集内：只认 ESRCH/EPERM/\
                EAGAIN/EINVAL 四码；请回查判定器是否夹带域外码",
            ProcCode::UnknownPid => "pid 不存在判据失配：pid 未登记或已回收时应报\
                ESRCH；请先查 pid 表再判权限",
            ProcCode::PermDenied => "权限判据失配：跨 uid 或能力位不足应报 EPERM；\
                请核对 uid/euid/fsuid 与能力位输入",
            ProcCode::ResourceExhausted => "资源耗尽判据失配：任务/内存配额触顶应报\
                EAGAIN；请检查配额账与回收路径",
            ProcCode::BadValue => "参数坏值判据失配：信号号/nice/flags 越界应报\
                EINVAL；坏值必须最先判，先于 pid 与权限",
            ProcCode::OrderViolation => "判序违约：必须按坏值→pid→权限→资源短路；\
                请修判定器分支顺序，禁止后置判据抢先返回",
            ProcCode::ForkExceptionMissing => "fork 例外未声明：fork 跳过 pid 检查\
                （父进程必在），但坏值/权限/资源三序仍须保留",
            ProcCode::RowsIncomplete => "宪章行不齐：四域各 3 行、共 12 行缺一不可；\
                请补齐缺行后重新复算",
            ProcCode::DomainMismatch => "格账与复算不符：格账必须由 12 行逐行复算\
                生成；请勿手工改格账数字",
            ProcCode::NotReproducible => "双跑不一致：同输入两次指纹/格账/注入账\
                必须逐位相等；请排查隐藏状态",
            ProcCode::StageRegress => "状态机倒退：只允许 Declare→Sealed 单向推进；\
                请检查回退路径",
            ProcCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x7A000..=0x7D1C0；越界即拒账",
        }
    }

    /// 闭集全量。
    pub const ALL: [ProcCode; 12] = [ProcCode::OutOfDomain, ProcCode::UnknownPid, ProcCode::PermDenied, ProcCode::ResourceExhausted, ProcCode::BadValue, ProcCode::OrderViolation, ProcCode::ForkExceptionMissing, ProcCode::RowsIncomplete, ProcCode::DomainMismatch, ProcCode::NotReproducible, ProcCode::StageRegress, ProcCode::SegmentViolation,];
}

// ── 进程操作与请求条件 ────────────────────────────────────────────────
/// 进程空间操作闭集（宪章行与注入探针复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcOp {
    /// kill(pid, sig)
    Kill,
    /// wait4(pid, ...)
    Wait,
    /// setpriority(which, who, prio)
    SetPrio,
    /// fork()
    Fork,
    /// clone(flags, ...)
    Clone,
    /// getpgid(pid)
    GetPgrp,
    /// ptrace(PTRACE_ATTACH, pid)
    Ptrace,
    /// setrlimit(resource, rlim)
    SetLimit,
}

impl ProcOp {
    /// 操作短名。
    pub fn name(self) -> &'static str {
        match self { ProcOp::Kill => "kill", ProcOp::Wait => "wait4", ProcOp::SetPrio => "setpriority", ProcOp::Fork => "fork", ProcOp::Clone => "clone", ProcOp::GetPgrp => "getpgid", ProcOp::Ptrace => "ptrace", ProcOp::SetLimit => "setrlimit", }
    }

    /// 八操作枚举（正常流遍历）。
    pub const ALL: [ProcOp; 8] = [ProcOp::Kill, ProcOp::Wait, ProcOp::SetPrio, ProcOp::Fork, ProcOp::Clone, ProcOp::GetPgrp, ProcOp::Ptrace, ProcOp::SetLimit,];
}

/// 一次进程空间请求的四枚条件位（true=该面通过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Req {
    /// 参数合法（信号号/nice/flags 在界内）
    pub value_ok: bool,
    /// pid 存在（未回收、未越 pid_max）
    pub pid_ok: bool,
    /// 权限满足（uid/euid/fsuid/能力位）
    pub perm_ok: bool,
    /// 资源充足（任务数/内存/配额）
    pub res_ok: bool,
}

impl Req {
    /// 构造请求条件。
    pub const fn new(value_ok: bool, pid_ok: bool, perm_ok: bool, res_ok: bool) -> Self {
        Req { value_ok, pid_ok, perm_ok, res_ok }
    }

    /// 全绿请求（正常流）。
    pub const fn good() -> Self {
        Req { value_ok: true, pid_ok: true, perm_ok: true, res_ok: true }
    }

    /// 条件位压成 4 位掩码（指纹用，整数无浮点）。
    pub fn mask(self) -> u32 {
        let mut m = 0u32;
if self.value_ok { m |= 1; }
if self.pid_ok { m |= 2; }
if self.perm_ok { m |= 4; }
if self.res_ok { m |= 8; }
        m
    }
}

// ── 判序协议 ──────────────────────────────────────────────────────────
/// 判定器：按声明判序短路返回首个违约域。
///
/// 判序：坏值 → pid → 权限 → 资源；fork 跳过 pid（父进程必然存在）。
pub fn judge(req: &Req, op: ProcOp) -> Result<(), ErrDomain> {
if !req.value_ok { return Err(ErrDomain::BadValue); }
if op != ProcOp::Fork && !req.pid_ok { return Err(ErrDomain::NoPid); }
if !req.perm_ok { return Err(ErrDomain::Perm); }
if !req.res_ok { return Err(ErrDomain::Resource); }
    Ok(())
}

/// 正常流：全绿请求在八操作上一律 Ok。
pub fn good_flow_ok() -> bool {
    let req = Req::good();
    let mut i = 0usize;
while i < ProcOp::ALL.len() { match ProcOp::ALL.get(i) { Some(op) => { if !judge(&req, *op).is_ok() { return false; } } None => return false, } i += 1; }
    true
}

/// 判序探针：多面同时违约时首报域必须与声明一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderProbe {
    /// 请求条件
    pub req: Req,
    /// 操作
    pub op: ProcOp,
    /// 期望首报域
    pub expect: ErrDomain,
}

/// 四枚判序探针：逐层剥离，验证短路次序。
pub const ORDER_PROBES: [OrderProbe; 4] = [OrderProbe { req: Req::new(false, false, false, false), op: ProcOp::Kill, expect: ErrDomain::BadValue, },
    OrderProbe { req: Req::new(true, false, false, false), op: ProcOp::Kill, expect: ErrDomain::NoPid, },
    OrderProbe { req: Req::new(true, true, false, false), op: ProcOp::Kill, expect: ErrDomain::Perm, },
    OrderProbe { req: Req::new(true, true, true, false), op: ProcOp::Kill, expect: ErrDomain::Resource, },
];

/// fork 例外探针：pid 位为假不报 ESRCH，其余三序照旧。
pub const FORK_PROBES: [OrderProbe; 3] = [OrderProbe { req: Req::new(false, false, true, true), op: ProcOp::Fork, expect: ErrDomain::BadValue, },
    OrderProbe { req: Req::new(true, false, false, true), op: ProcOp::Fork, expect: ErrDomain::Perm, },
    OrderProbe { req: Req::new(true, false, true, false), op: ProcOp::Fork, expect: ErrDomain::Resource, },
];

/// 判序协议声明（可复算、可对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderProtocol {
    /// 通用判序（四域）
    pub order: [ErrDomain; 4],
    /// fork 判序（三域，无 pid）
    pub fork_order: [ErrDomain; 3],
    /// fork 是否跳过 pid
    pub fork_skips_pid: bool,
}

impl OrderProtocol {
    /// 正典声明：坏值→pid→权限→资源；fork 坏值→权限→资源。
    pub const fn declared() -> Self {
        OrderProtocol { order: [ErrDomain::BadValue, ErrDomain::NoPid, ErrDomain::Perm, ErrDomain::Resource,], fork_order: [ErrDomain::BadValue, ErrDomain::Perm, ErrDomain::Resource,], fork_skips_pid: true, }
    }

    /// 通用判序探针全部命中。
    pub fn order_probes_hold(&self) -> bool {
        self.order
            == [ErrDomain::BadValue, ErrDomain::NoPid, ErrDomain::Perm, ErrDomain::Resource,]
            && order_probes_match()
    }

    /// fork 例外声明完整且探针命中。
    pub fn fork_declared(&self) -> bool {
if !self.fork_skips_pid || self.fork_order.len() != 3 { return false; }
        let mut i = 0usize;
while i < self.fork_order.len() { match self.fork_order.get(i) { Some(d) => { if *d == ErrDomain::NoPid { return false; } } None => return false, } i += 1; }
        fork_probes_hold() && fork_pid_ignored()
    }
}

/// 通用判序探针全命中。
pub fn order_probes_match() -> bool {
    let mut i = 0usize;
while i < ORDER_PROBES.len() { match ORDER_PROBES.get(i) { Some(p) => { if judge(&p.req, p.op) != Err(p.expect) { return false; } } None => return false, } i += 1; }
    true
}

/// fork 例外探针全命中。
pub fn fork_probes_hold() -> bool {
    let mut i = 0usize;
while i < FORK_PROBES.len() { match FORK_PROBES.get(i) { Some(p) => { if judge(&p.req, p.op) != Err(p.expect) { return false; } } None => return false, } i += 1; }
    true
}

/// fork 且仅 pid 位为假：必须 Ok（pid 检查被跳过）。
pub fn fork_pid_ignored() -> bool {
    judge(&Req::new(true, false, true, true), ProcOp::Fork).is_ok()
}

// ── 十二行宪章表 ──────────────────────────────────────────────────────
/// 一条宪章行：真实场景 + 期望首报域 + 可复算条件位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcRule {
    /// 行号 0..=11
    pub id: u8,
    /// 操作
    pub op: ProcOp,
    /// 场景描述（pid/凭据/配额真实路径）
    pub scenario: &'static str,
    /// 期望首报域
    pub domain: ErrDomain,
    /// 条件位
    pub req: Req,
}

/// 四域各 3 行的宪章表（全部可由 `judge` 逐行复算）。
pub const RULE_TABLE: [ProcRule; 12] = [ProcRule { id: 0, op: ProcOp::Kill, scenario: "kill 已回收 pid：pid 表无项，先于权限报 ESRCH", domain: ErrDomain::NoPid, req: Req::new(true, false, true, true), },
    ProcRule { id: 1, op: ProcOp::Wait, scenario: "wait4 非子进程 pid：无父子关系，报 ESRCH", domain: ErrDomain::NoPid, req: Req::new(true, false, true, true), },
    ProcRule { id: 2, op: ProcOp::GetPgrp, scenario: "getpgid 空进程组：组表无项，报 ESRCH", domain: ErrDomain::NoPid, req: Req::new(true, false, true, true), },
    ProcRule { id: 3, op: ProcOp::Kill, scenario: "kill 跨 uid 目标：uid/euid 均不匹配，报 EPERM", domain: ErrDomain::Perm, req: Req::new(true, true, false, true), },
    ProcRule { id: 4, op: ProcOp::SetPrio, scenario: "setpriority 他人进程：降 nice 属特权操作，报 EPERM", domain: ErrDomain::Perm, req: Req::new(true, true, false, true), },
    ProcRule { id: 5, op: ProcOp::Ptrace, scenario: "ptrace 附加不可 dump 目标：能力位不足，报 EPERM", domain: ErrDomain::Perm, req: Req::new(true, true, false, true), },
    ProcRule { id: 6, op: ProcOp::Fork, scenario: "fork 触顶任务配额：RLIMIT_NPROC 用尽，报 EAGAIN", domain: ErrDomain::Resource, req: Req::new(true, true, true, false), },
    ProcRule { id: 7, op: ProcOp::Clone, scenario: "clone 内存配额不足：预留失败，报 EAGAIN", domain: ErrDomain::Resource, req: Req::new(true, true, true, false), },
    ProcRule { id: 8, op: ProcOp::SetLimit, scenario: "setrlimit 越过硬限：资源账拒降，报 EAGAIN", domain: ErrDomain::Resource, req: Req::new(true, true, true, false), },
    ProcRule { id: 9, op: ProcOp::Kill, scenario: "kill 信号号越界（0 或 >64）：先判坏值，报 EINVAL", domain: ErrDomain::BadValue, req: Req::new(false, true, true, true), },
    ProcRule { id: 10, op: ProcOp::SetPrio, scenario: "setpriority nice 越界（<-20 或 >19）：报 EINVAL", domain: ErrDomain::BadValue, req: Req::new(false, true, true, true), },
    ProcRule { id: 11, op: ProcOp::Clone, scenario: "clone flags 非法组合：坏值先于配额判定，报 EINVAL", domain: ErrDomain::BadValue, req: Req::new(false, true, true, true), },
];

/// 逐行复算：`judge` 首报域必须等于本行期望域。
pub fn rule_verdict(row: &ProcRule) -> Result<(), ErrDomain> {
    judge(&row.req, row.op)
}

/// 十二行全部可复算。
pub fn rows_recompute() -> bool {
    let mut i = 0usize;
while i < RULE_TABLE.len() { match RULE_TABLE.get(i) { Some(row) => { if rule_verdict(row) != Err(row.domain) { return false; } } None => return false, } i += 1; }
    true
}

/// 四域各恰好 3 行。
pub fn domain_coverage_ok() -> bool {
    let mut counts = [0u32; 4];
    let mut i = 0usize;
while i < RULE_TABLE.len() { match RULE_TABLE.get(i) { Some(row) => match counts.get_mut(row.domain.index()) { Some(c) => *c += 1, None => return false, }, None => return false, } i += 1; }
    let mut d = 0usize;
while d < DOMAIN_COUNT { match counts.get(d) { Some(c) => { if *c != ROWS_PER_DOMAIN { return false; } } None => return false, } d += 1; }
    true
}

// ── 域格账 ────────────────────────────────────────────────────────────
/// 每域期望格数。
pub const DOMAIN_EXPECT: [u32; 4] = [3, 3, 3, 3];

/// 四域格账：每域 3 格，满格即完整，溢出格拒绝并计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainLedger {
    cells: [u32; 4],
    rejected: u32,
}

impl DomainLedger {
    /// 空格账。
    pub const fn new() -> Self {
        DomainLedger { cells: [0; 4], rejected: 0 }
    }

    /// 记一格；该域满格或域外时拒绝并累计 rejected。
    pub fn record(&mut self, domain: ErrDomain) -> bool {
        let idx = domain.index();
        let expect = match DOMAIN_EXPECT.get(idx) { Some(v) => *v, None => { self.rejected += 1; return false; } };
        let slot = match self.cells.get_mut(idx) { Some(s) => s, None => { self.rejected += 1; return false; } };
if *slot >= expect { self.rejected += 1; return false; }
        *slot += 1;
        true
    }

    /// 单域格数。
    pub fn cell(&self, domain: ErrDomain) -> u32 {
        match self.cells.get(domain.index()) { Some(v) => *v, None => 0, }
    }

    /// 总支格数。
    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
while i < DOMAIN_COUNT { if let Some(v) = self.cells.get(i) { t += *v; } i += 1; }
        t
    }

    /// 被拒绝的溢出记录数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 满格（每域恰好 3）。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
while i < DOMAIN_COUNT { let expect = match DOMAIN_EXPECT.get(i) { Some(v) => *v, None => return false, }; let got = match self.cells.get(i) { Some(v) => *v, None => return false, }; if got != expect { return false; } i += 1; }
        true
    }

    /// 守恒：满格且零拒绝且总数 12。
    pub fn conserved(&self) -> bool {
        self.complete() && self.rejected == 0 && self.total() == RULE_COUNT as u32
    }
}

/// 由十二行宪章表逐格落账。
pub fn fill_ledger() -> DomainLedger {
    let mut led = DomainLedger::new();
    let mut i = 0usize;
while i < RULE_TABLE.len() { if let Some(row) = RULE_TABLE.get(i) { led.record(row.domain); } i += 1; }
    led
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 四类注入手法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tamper {
    /// pid 缺失却误报权限
    PidAsPerm,
    /// 权限不足却误报 pid
    PermAsPid,
    /// 资源耗尽却误报坏值
    ResourceAsBadValue,
    /// 判序倒置：资源抢在权限前
    OrderInverted,
}

impl Tamper {
    /// 手法短名。
    pub fn name(self) -> &'static str {
        match self { Tamper::PidAsPerm => "pid-as-perm", Tamper::PermAsPid => "perm-as-pid", Tamper::ResourceAsBadValue => "res-as-value", Tamper::OrderInverted => "order-inverted", }
    }

    /// 闭集全量。
    pub const ALL: [Tamper; 4] = [Tamper::PidAsPerm, Tamper::PermAsPid, Tamper::ResourceAsBadValue, Tamper::OrderInverted,];
}

/// 一次注入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BadProcCase {
    /// 注入手法
    pub tamper: Tamper,
    /// 轮次 0..=9
    pub round: u32,
}

impl BadProcCase {
    /// 构造样本；轮次越界返回 None（边界守卫）。
    pub fn new(tamper: Tamper, round: u32) -> Option<Self> {
if round >= INJECT_ROUNDS { None } else { Some(BadProcCase { tamper, round }) }
    }
}

/// 注入探针：条件位 + 操作 + 被篡改的期望域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TamperProbe {
    /// 请求条件
    pub req: Req,
    /// 操作
    pub op: ProcOp,
    /// 注入者伪造的域（非真实首报域）
    pub tampered: ErrDomain,
}

/// 轮次决定操作选择，保证每轮样本可区分。
fn op_for(tamper: Tamper, round: u32) -> ProcOp {
    let base = match tamper { Tamper::PidAsPerm => 0u32, Tamper::PermAsPid => 1u32, Tamper::ResourceAsBadValue => 2u32, Tamper::OrderInverted => 3u32, };
    let pick = (base + round) % 3;
    match pick { 0 => ProcOp::Kill, 1 => ProcOp::SetPrio, _ => ProcOp::Clone, }
}

/// 由手法与轮次生成注入探针（四类条件位保证真实域已知）。
pub fn probe_of(tamper: Tamper, round: u32) -> TamperProbe {
    let op = op_for(tamper, round);
    match tamper { Tamper::PidAsPerm => TamperProbe { req: Req::new(true, false, true, true), op, tampered: ErrDomain::Perm, }, Tamper::PermAsPid => TamperProbe { req: Req::new(true, true, false, true), op, tampered: ErrDomain::NoPid, }, Tamper::ResourceAsBadValue => TamperProbe { req: Req::new(true, true, true, false), op, tampered: ErrDomain::BadValue, }, Tamper::OrderInverted => TamperProbe { req: Req::new(true, true, false, false), op, tampered: ErrDomain::Resource, }, }
}

/// 检出判定：真实首报域与伪造域不同即抓。
pub fn injection_detected(case: &BadProcCase) -> bool {
    let probe = probe_of(case.tamper, case.round);
    match judge(&probe.req, probe.op) { Ok(()) => false, Err(actual) => actual != probe.tampered, }
}

/// 单类 10 轮检出数。
pub fn class_hits(tamper: Tamper) -> u32 {
    let mut hits = 0u32;
    let mut round = 0u32;
while round < INJECT_ROUNDS { if let Some(case) = BadProcCase::new(tamper, round) { if injection_detected(&case) { hits += 1; } } round += 1; }
    hits
}

/// 注入账（注入 = 检出 + 逃逸）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectTally {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须 0）
    pub escaped: u32,
}

impl InjectTally {
    /// 守恒：注入 = 检出 + 逃逸，且逃逸为零。
    pub fn conserved(&self) -> bool {
        self.escaped == 0 && self.injected == self.detected + self.escaped
    }
}

/// 跑全部四类 × 10 轮注入。
pub fn run_injection() -> InjectTally {
    let mut injected = 0u32;
    let mut detected = 0u32;
    let mut i = 0usize;
while i < Tamper::ALL.len() { if let Some(tamper) = Tamper::ALL.get(i) { injected += INJECT_ROUNDS; detected += class_hits(*tamper); } i += 1; }
    InjectTally { injected, detected, escaped: injected.saturating_sub(detected), }
}

/// 四类各 10/10 全检出。
pub fn injection_ten_of_ten() -> bool {
    let mut i = 0usize;
while i < Tamper::ALL.len() { match Tamper::ALL.get(i) { Some(t) => { if class_hits(*t) != INJECT_ROUNDS { return false; } } None => return false, } i += 1; }
    true
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 总纲状态：声明→复算→注入→格账→封存，只进不退。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ProcStage {
    /// 声明判序协议
    Declare,
    /// 复算十二行
    Recompute,
    /// 注入四类 × 10
    Inject,
    /// 逐格落账
    Ledger,
    /// 封存
    Sealed,
}

impl ProcStage {
    /// 下一阶；封存后无下一阶。
    pub fn next(self) -> Option<ProcStage> {
        match self { ProcStage::Declare => Some(ProcStage::Recompute), ProcStage::Recompute => Some(ProcStage::Inject), ProcStage::Inject => Some(ProcStage::Ledger), ProcStage::Ledger => Some(ProcStage::Sealed), ProcStage::Sealed => None, }
    }

    /// 阶序号。
    pub fn index(self) -> u32 {
        match self { ProcStage::Declare => 0, ProcStage::Recompute => 1, ProcStage::Inject => 2, ProcStage::Ledger => 3, ProcStage::Sealed => 4, }
    }

    /// 阶名。
    pub fn name(self) -> &'static str {
        match self { ProcStage::Declare => "declare", ProcStage::Recompute => "recompute", ProcStage::Inject => "inject", ProcStage::Ledger => "ledger", ProcStage::Sealed => "sealed", }
    }
}

/// 状态机：只能 advance，不能回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageMachine {
    stage: ProcStage,
    steps: u32,
}

impl StageMachine {
    /// 从 Declare 起步。
    pub const fn new() -> Self {
        StageMachine { stage: ProcStage::Declare, steps: 0 }
    }

    /// 当前阶。
    pub fn stage(&self) -> ProcStage {
        self.stage
    }

    /// 推进步数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 前进一步；封存后推进报 StageRegress。
    pub fn advance(&mut self) -> Result<ProcStage, ProcCode> {
        let next = match self.stage.next() { Some(n) => n, None => return Err(ProcCode::StageRegress), };
        self.stage = next;
        self.steps += 1;
        Ok(next)
    }

    /// 回退一律拒绝（状态机禁退）。
    pub fn rewind(&mut self) -> Result<ProcStage, ProcCode> {
        Err(ProcCode::StageRegress)
    }
}

/// 走完四步到 Sealed，返回步数；异常返回 None。
pub fn walk_stages() -> Option<u32> {
    let mut m = StageMachine::new();
    let mut guard = 0u32;
while guard < 8 { match m.advance() { Ok(_) => { if m.stage() == ProcStage::Sealed { return Some(m.steps()); } } Err(_) => return None, } guard += 1; }
    None
}

/// 状态机禁退：封存后推进与任意回退都报 StageRegress。
pub fn stage_regress_blocked() -> bool {
    let mut m = StageMachine::new();
    let mut guard = 0u32;
while guard < 8 { if m.advance().is_err() { return false; } if m.stage() == ProcStage::Sealed { break; } guard += 1; }
if m.stage() != ProcStage::Sealed { return false; }
    m.advance() == Err(ProcCode::StageRegress)
        && m.rewind() == Err(ProcCode::StageRegress)
        && m.stage() == ProcStage::Sealed
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 整数混洗。
pub fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

/// 语料指纹：十二行 + 四类注入 + 状态步数，全整数。
pub fn corpus_fingerprint() -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
while i < RULE_TABLE.len() { if let Some(row) = RULE_TABLE.get(i) { h = mix(h, row.id as u32); h = mix(h, row.domain.index() as u32); h = mix(h, row.op.name().len() as u32); h = mix(h, row.req.mask()); } i += 1; }
    let mut t = 0usize;
while t < Tamper::ALL.len() { if let Some(tamper) = Tamper::ALL.get(t) { h = mix(h, class_hits(*tamper)); } t += 1; }
    let stages = match walk_stages() { Some(n) => n, None => 0, };
    mix(h, stages)
}

/// 双跑一致：指纹、格账、注入账三面逐位相等。
pub fn double_run_equal() -> bool {
    let fa = corpus_fingerprint();
    let fb = corpus_fingerprint();
    let la = fill_ledger();
    let lb = fill_ledger();
    let ta = run_injection();
    let tb = run_injection();
    fa == fb && la == lb && ta == tb
}

// ── 文案与码值守卫 ────────────────────────────────────────────────────
/// 文案闭集：12 码 detail 非空且两两互异。
pub fn text_closed() -> bool {
    let mut i = 0usize;
while i < ProcCode::ALL.len() { let cur = match ProcCode::ALL.get(i) { Some(c) => c, None => return false, }; if cur.detail().is_empty() { return false; } let mut j = i + 1; while j < ProcCode::ALL.len() { match ProcCode::ALL.get(j) { Some(other) => { if cur.detail() == other.detail() { return false; } } None => return false, } j += 1; } i += 1; }
    true
}

/// 码值守卫：段内 +0x00..=+0x0B 连续且唯一。
pub fn codes_within_segment() -> bool {
    let mut i = 0usize;
while i < ProcCode::ALL.len() { let cur = match ProcCode::ALL.get(i) { Some(c) => c, None => return false, }; if cur.val() != SEG_BASE + i as u32 { return false; } let mut j = i + 1; while j < ProcCode::ALL.len() { match ProcCode::ALL.get(j) { Some(other) => { if cur.val() == other.val() { return false; } } None => return false, } j += 1; } i += 1; }
    true
}

/// errno 对照：四域与表内值一一相等。
pub const ERRNO_TABLE: [(ErrDomain, i32); 4] = [(ErrDomain::NoPid, 3), (ErrDomain::Perm, 1), (ErrDomain::Resource, 11), (ErrDomain::BadValue, 22),];

/// 四域 errno 与对照表一致。
pub fn errno_table_ok() -> bool {
    let mut i = 0usize;
while i < ErrDomain::ALL.len() { let domain = match ErrDomain::ALL.get(i) { Some(d) => *d, None => return false, }; let mut hit = false; let mut j = 0usize; while j < ERRNO_TABLE.len() { if let Some((ed, code)) = ERRNO_TABLE.get(j) { if *ed == domain && *code == domain.errno() { hit = true; } } j += 1; } if !hit { return false; } i += 1; }
    true
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9281 判据集（静态 21 条 + 动态 4 条）。
pub fn run_unx9281_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9281");

    // 1. 段位对齐
    s.add("段位守卫：0x7B3C0 对齐", SEG_BASE % 0x40 == 0, "段基址必须 0x40 对齐，越界拒绝落账",);
    // 2. 段位窗口
    let in_window = (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：落在 0x7A000..=0x7D1C0", in_window, "段基址越出窗口即报 SegmentViolation",);
    // 3. 行数
    s.add("正常流：宪章表 12 行齐备", RULE_TABLE.len() == RULE_COUNT && RULE_COUNT == 12, "四域各 3 行，缺一即 RowsIncomplete",);
    // 4. 四域覆盖
    s.add("四域覆盖：每域恰好 3 行", domain_coverage_ok(), "pid/权限/资源/坏值逐域落格，不允许偏域",);
    // 5. 正常流
    s.add("正常流：全绿请求在八操作上均 Ok", good_flow_ok(), "kill/wait/setpriority/fork/clone/getpgid/ptrace/setrlimit",);
    // 6. 逐行复算
    s.add("复算：十二行 judge 首报域等于期望域", rows_recompute(), "禁止只声明不复算，行行可重算",);
    // 7. 判序声明
    let proto = OrderProtocol::declared();
    s.add("判序声明：坏值→pid→权限→资源", proto.order == [ErrDomain::BadValue, ErrDomain::NoPid, ErrDomain::Perm, ErrDomain::Resource,], "通用判序四阶逐字声明，短路返回首报域",);
    // 8. 判序探针
    s.add("判序探针：四探针首报域与声明一致", proto.order_probes_hold(), "多层违约时后置判据不得抢先",);
    // 9. fork 例外
    s.add("fork 例外声明：跳过 pid 且并入三序", proto.fork_declared(), "fork 坏值→权限→资源，pid 位为假不报 ESRCH",);
    // 10. 边界：坏值最先
    let edge_value = judge(&Req::new(false, false, false, false), ProcOp::Kill);
    s.add("边界：坏值与 pid 同时违约首报 EINVAL", edge_value == Err(ErrDomain::BadValue), "坏值先于 pid 判定，不得被 ESRCH 抢占",);
    // 11. 域格账
    let led = fill_ledger();
    s.add("对账：四域格账逐格落账 3/3/3/3", led.complete() && led.cell(ErrDomain::NoPid) == 3, "格账由十二行复算生成，手工改数即失配",);
    // 12. 守恒
    s.add("守恒：格账 12 格且零拒绝", led.conserved() && led.total() == 12 && led.rejected() == 0, "总格数等于行数，溢出格必须拒绝",);
    // 13. 注入 10/10
    s.add("注入必抓：四类各 10/10 检出", injection_ten_of_ten(), "pid/权限、权限/pid、资源/坏值、判序倒置四类全抓",);
    // 14. 注入账
    let tally = run_injection();
    s.add("守恒：注入 40 = 检出 40 + 逃逸 0", tally.conserved() && tally.injected == 40 && tally.detected == 40 && tally.escaped == 0, "四类 × 10 轮，逃逸恒零",);
    // 15. 状态机
    let stages = match walk_stages() { Some(n) => n, None => 0, };
    s.add("状态机：Declare→Sealed 四步走通", stages == 4, "声明/复算/注入/格账/封存五阶四步",);
    // 16. 禁退
    s.add("状态机：回退与封存后推进均拒绝", stage_regress_blocked(), "只进不退，倒退报 StageRegress",);
    // 17. 容量
    s.add("容量：12 行不超过 16 槽", RULE_TABLE.len() <= ROW_CAP, "容量守卫留 4 槽余量，不得借槽扩域",);
    // 18. 双跑
    s.add("双跑一致：指纹/格账/注入账逐位相等", double_run_equal(), "同输入两次必须逐位相等，隐藏状态即失败",);
    // 19. 文案闭集
    s.add("文案闭集：12 码 detail 非空且互异", text_closed(), "诊断必须带可操作中文建议",);
    // 20. 码值
    s.add("码值守卫：段内 +0x00..=+0x0B 唯一", codes_within_segment(), "诊断码只能取本段连续码位",);
    // 21. errno
    s.add("errno 对照：ESRCH/EPERM/EAGAIN/EINVAL", errno_table_ok(), "四域 errno 为 3/1/11/22，逐位一致",);

    // 动态明细（≤8）
    s.add_dyn("行账快照", RULE_TABLE.len() == 12, &format!("rows={} doms={}", RULE_TABLE.len(), DOMAIN_COUNT),);
    s.add_dyn("注入快照", tally.conserved(), &format!("inj={} det={} esc={}", tally.injected, tally.detected, tally.escaped),);
    s.add_dyn("指纹快照", double_run_equal(), &format!("fp=0x{:08X}", corpus_fingerprint()),);
    s.add_dyn("状态快照", stages == 4, &format!("stage={} steps={}", ProcStage::Sealed.name(), stages),);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_base_guarded() {
        assert_eq!(SEG_BASE, 0x7B3C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn charter_rows_complete() {
        assert_eq!(RULE_TABLE.len(), 12);
        assert!(domain_coverage_ok());
        assert!(rows_recompute());
    }

    #[test]
    fn good_flow_passes() {
        assert!(good_flow_ok());
        assert_eq!(judge(&Req::good(), ProcOp::Fork), Ok(()));
        assert_eq!(judge(&Req::good(), ProcOp::Kill), Ok(()));
    }

    #[test]
    fn judgment_order_value_first() {
        assert_eq!(judge(&Req::new(false, false, false, false), ProcOp::Kill), Err(ErrDomain::BadValue));
        assert_eq!(judge(&Req::new(true, false, false, false), ProcOp::Kill), Err(ErrDomain::NoPid));
        assert_eq!(judge(&Req::new(true, true, false, false), ProcOp::Kill), Err(ErrDomain::Perm));
        assert_eq!(judge(&Req::new(true, true, true, false), ProcOp::Kill), Err(ErrDomain::Resource));
    }

    #[test]
    fn fork_skips_pid() {
        assert!(fork_pid_ignored());
        assert!(fork_probes_hold());
        assert_eq!(judge(&Req::new(true, false, false, true), ProcOp::Fork), Err(ErrDomain::Perm));
    }

    #[test]
    fn order_protocol_declared() {
        let proto = OrderProtocol::declared();
        assert!(proto.order_probes_hold());
        assert!(proto.fork_declared());
        assert!(proto.fork_skips_pid);
        assert_eq!(proto.fork_order.len(), 3);
    }

    #[test]
    fn ledger_fills_and_overflows() {
        let led = fill_ledger();
        assert!(led.conserved());
        assert_eq!(led.total(), 12);
        assert_eq!(led.rejected(), 0);
for d in ErrDomain::ALL.iter() { assert_eq!(led.cell(*d), 3); }
        let mut over = led;
        assert!(!over.record(ErrDomain::NoPid));
        assert_eq!(over.rejected(), 1);
        assert!(!over.conserved());
    }

    #[test]
    fn injection_matrix_ten_of_ten() {
        assert!(injection_ten_of_ten());
        let tally = run_injection();
        assert_eq!(tally.injected, 40);
        assert_eq!(tally.detected, 40);
        assert_eq!(tally.escaped, 0);
        assert!(tally.conserved());
    }

    #[test]
    fn per_class_hits_exact() {
for t in Tamper::ALL.iter() { assert_eq!(class_hits(*t), INJECT_ROUNDS); }
        assert!(BadProcCase::new(Tamper::PidAsPerm, 9).is_some());
        assert!(BadProcCase::new(Tamper::PidAsPerm, 10).is_none());
    }

    #[test]
    fn stage_machine_forward_only() {
        assert_eq!(walk_stages(), Some(4));
        assert!(stage_regress_blocked());
        let mut m = StageMachine::new();
        assert_eq!(m.advance(), Ok(ProcStage::Recompute));
        assert_eq!(m.advance(), Ok(ProcStage::Inject));
        assert_eq!(m.advance(), Ok(ProcStage::Ledger));
        assert_eq!(m.advance(), Ok(ProcStage::Sealed));
        assert_eq!(m.advance(), Err(ProcCode::StageRegress));
        assert_eq!(m.rewind(), Err(ProcCode::StageRegress));
    }

    #[test]
    fn double_run_stable() {
        assert!(double_run_equal());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn text_and_codes_closed() {
        assert_eq!(ProcCode::ALL.len(), 12);
        assert!(text_closed());
        assert!(codes_within_segment());
        assert_eq!(ProcCode::UnknownPid.val(), SEG_BASE + 1);
    }

    #[test]
    fn errno_table_agrees() {
        assert!(errno_table_ok());
        assert_eq!(ErrDomain::NoPid.errno(), 3);
        assert_eq!(ErrDomain::Perm.errno(), 1);
        assert_eq!(ErrDomain::Resource.errno(), 11);
        assert_eq!(ErrDomain::BadValue.errno(), 22);
    }

    #[test]
    fn checks_green_and_deterministic() {
        let set = run_unx9281_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
        assert!(!set.truncated());
        assert!(set.len() >= 12);
    }
}
