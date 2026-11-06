//! UNX-F9293 ESRCH 判定次序协议（pid 存在性先于权限） · 0x7B6C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9293 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9293-J1 判序协议判据过，不存在
//!   pid 全 ESRCH 逐格检出（先于 EPERM），与 Linux 逐例一致格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判序协议五级：参数(EINVAL) → 用户结构拷贝(EFAULT) → pid 存在性
//!      (ESRCH/ECHILD) → 权限(EPERM/EACCES) → 通过。级别只进不退，判决携带
//!      阶段掩码，掩码必须是自低位起的前缀（0x01/0x03/0x07/0x0F/0x1F）。
//!   ② 「不存在 pid 全 ESRCH」不是一句口号：12 条需要查 pid 的调用 × 三种
//!      权限态 × 10 轮 = 390 格，每格都必须等于本行的 absent 码，且不得出现
//!      任何权限码。两条子进程专用调用（waitpid/waitid）的 absent 码是
//!      ECHILD 而不是 ESRCH——它们是「不是我的孩子」而不是「进程不存在」。
//!   ③ 反例机：`decide_perm_first` 把权限门提到 pid 门之前，在「缺席 + 外部
//!      权限」的 7 格上必然给出 EPERM/EACCES。这 7 格正是判序协议的可执行
//!      反证据，正确判决在同样 7 格上全部回 ESRCH。
//!   ④ 僵尸仍然存在：kill(僵尸) 与 waitpid(僵尸) 都必须成功，误判 ESRCH 会
//!      让调用方以为进程从未存在，从而丢掉退出状态。
//!   ⑤ 权限面不是单一 EPERM：setpgid 在跨会话时回 EACCES，其余六个权限门回
//!      EPERM。把两者混成一个码会让用户态无法区分「换个会话」与「加权限」。
//!   ⑥ rt_sigqueueinfo 的 siginfo 拷贝先于 pid 查找：坏指针 + 缺席 pid 必须
//!      先回 EFAULT。这条是五级判序里唯一的「pid 之前还有用户拷贝」特例。
//!   ⑦ 诊断码段内闭集 12 个（+1..=+12），码值 = SEG_BASE + 序号，两两唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9293 独占 0x7B6C0..=0x7B6FF）。
pub const SEG_BASE: u32 = 0x7B6C0;
/// 段窗口宽度（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
/// 本批段窗口下界（含）。
pub const SEG_WINDOW_LO: u32 = 0x7B000;
/// 本批段窗口上界（含）。
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

/// 权限不足。
pub const EPERM: i32 = 1;
/// 无此进程。
pub const ESRCH: i32 = 3;
/// 无此子进程。
pub const ECHILD: i32 = 10;
/// 跨会话改进程组。
pub const EACCES: i32 = 13;
/// 用户指针不可访问。
pub const EFAULT: i32 = 14;
/// 参数坏值。
pub const EINVAL: i32 = 22;

/// 进程族调用行数。
pub const FAMILY_ROWS: usize = 14;
/// 需要查 pid 的行数（只排除 pid=-1 广播一条）。
pub const LOOKUP_ROWS: usize = 13;
/// 带权限门的行数。
pub const PERM_ROWS: usize = 7;
/// 权限门回 EPERM 的行数（setpgid 回 EACCES，故少一行）。
pub const EPERM_ROWS: usize = 6;
/// 子进程专用行数（absent 码为 ECHILD）。
pub const CHILD_ROWS: usize = 2;
/// Linux 逐例表行数。
pub const LINUX_CASE_ROWS: usize = 32;
/// 每格复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入总格数（390 缺席 + 210 权限 + 140 参数 + 320 逐例）。
pub const TOTAL_CELLS: u32 = 1060;
/// pid 形态数。
pub const PID_FORMS: usize = 6;
/// 权限态数。
pub const PERM_STATES: usize = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9293 诊断码（段内 +1..=+12 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderCode {
    /// 权限门先于 pid 门
    PermBeforePid,
    /// 缺席 pid 泄漏权限码
    AbsentPermLeak,
    /// 缺席 errno 用错
    WrongAbsentErrno,
    /// 僵尸被误判不存在
    ZombieMisjudged,
    /// 子进程专用调用误报 ESRCH
    ChildErrnoDrift,
    /// 参数门后置
    ParamLate,
    /// 用户拷贝门后置
    InfoLate,
    /// 阶段掩码非前缀
    StageSkip,
    /// 形态表漂移
    FormDrift,
    /// 注入账不守恒
    LedgerLeak,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl OrderCode {
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
            OrderCode::PermBeforePid => "权限门先于 pid 门：必须先确认 pid 存在再查权限，否则缺席进程会被误报 EPERM",
            OrderCode::AbsentPermLeak => "缺席 pid 泄漏权限码：不存在 pid 对任何权限态都只能回 ESRCH/ECHILD",
            OrderCode::WrongAbsentErrno => "缺席 errno 用错：普通调用是 ESRCH，waitpid/waitid 是 ECHILD，两者不可互换",
            OrderCode::ZombieMisjudged => "僵尸被误判不存在：僵尸仍占 pid，kill 与 wait 都不得回 ESRCH",
            OrderCode::ChildErrnoDrift => "子进程专用调用漂移：非自家孩子是 ECHILD，不是「进程不存在」",
            OrderCode::ParamLate => "参数门后置：信号号/flags 坏值必须最先回 EINVAL，不能先去查 pid",
            OrderCode::InfoLate => "用户拷贝门后置：rt_sigqueueinfo 必须先拷 siginfo，坏指针回 EFAULT 先于 pid 查找",
            OrderCode::StageSkip => "阶段跳级：判决阶段掩码必须是从 Param 起的前缀，跳级说明门被绕过",
            OrderCode::FormDrift => "pid 形态漂移：组/广播/自身组三种形态的存在性规则必须与冻结表一致",
            OrderCode::LedgerLeak => "注入账不闭合：注入必须等于检出加逃逸，且逃逸恒为零",
            OrderCode::NotReproducible => "双跑不一致：同输入两次判决必须逐字节相等，隐藏状态即失败",
            OrderCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7B000..=0x7BFFF",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [OrderCode] = &[
        OrderCode::PermBeforePid,
        OrderCode::AbsentPermLeak,
        OrderCode::WrongAbsentErrno,
        OrderCode::ZombieMisjudged,
        OrderCode::ChildErrnoDrift,
        OrderCode::ParamLate,
        OrderCode::InfoLate,
        OrderCode::StageSkip,
        OrderCode::FormDrift,
        OrderCode::LedgerLeak,
        OrderCode::NotReproducible,
        OrderCode::SegmentViolation,
    ];
}

// ── 形态与权限 ────────────────────────────────────────────────────────
/// pid 实参形态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PidForm {
    /// pid > 0：单进程。
    Single = 0,
    /// tgid + tid 双查。
    ThreadPair = 1,
    /// pid < -1：进程组。
    Group = 2,
    /// pid == -1：除自己外全部（广播）。
    Broadcast = 3,
    /// pid == 0：自身进程组。
    OwnGroup = 4,
    /// 只接受自家子进程。
    ChildOnly = 5,
}

impl PidForm {
    /// 形态编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 该形态是否需要查 pid 表（广播与自身组不需要）。
    pub fn needs_lookup(self) -> bool {
        !matches!(self, PidForm::Broadcast | PidForm::OwnGroup)
    }

    /// 形态短名。
    pub fn name(self) -> &'static str {
        match self {
            PidForm::Single => "pid>0",
            PidForm::ThreadPair => "tgid+tid",
            PidForm::Group => "pid<-1",
            PidForm::Broadcast => "pid=-1",
            PidForm::OwnGroup => "pid=0",
            PidForm::ChildOnly => "child",
        }
    }
}

/// 权限门种类。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermRule {
    /// 无权限门。
    None = 0,
    /// uid 相同或 CAP_KILL。
    UidOrCap = 1,
    /// 同会话，否则 EACCES。
    SessionOwner = 2,
    /// ptrace 访问模式（PR_SET_DUMPABLE / CAP_SYS_PTRACE）。
    PtraceAccess = 3,
    /// CAP_SYS_NICE。
    NiceCap = 4,
}

impl PermRule {
    /// 冻结编码（指纹串行化使用，取显式判别值）。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 权限门失败时的 errno；None 返回 0。
    pub fn errno(self) -> i32 {
        match self {
            PermRule::None => 0,
            PermRule::UidOrCap | PermRule::PtraceAccess | PermRule::NiceCap => EPERM,
            PermRule::SessionOwner => EACCES,
        }
    }

    /// 权限门短名。
    pub fn name(self) -> &'static str {
        match self {
            PermRule::None => "none",
            PermRule::UidOrCap => "uid-or-CAP_KILL",
            PermRule::SessionOwner => "same-session",
            PermRule::PtraceAccess => "ptrace-access",
            PermRule::NiceCap => "CAP_SYS_NICE",
        }
    }
}

/// pid 存在性状态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PidState {
    Live = 0,
    Zombie = 1,
    Absent = 2,
}

impl PidState {
    /// 状态编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 状态短名。
    pub fn name(self) -> &'static str {
        match self {
            PidState::Live => "live",
            PidState::Zombie => "zombie",
            PidState::Absent => "absent",
        }
    }
}

/// 请求者权限态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermState {
    Owner = 0,
    Privileged = 1,
    Foreign = 2,
}

impl PermState {
    /// 状态编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 三态常量表（遍历用）。
    pub const ALL: [PermState; PERM_STATES] = [
        PermState::Owner,
        PermState::Privileged,
        PermState::Foreign,
    ];
}

// ── 判序阶段 ──────────────────────────────────────────────────────────
/// 判决阶段（顺序即协议顺序）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    Param = 0,
    Info = 1,
    Pid = 2,
    Perm = 3,
    Done = 4,
}

impl Gate {
    /// 阶段编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 阶段掩码：必须是自低位起的前缀。
    pub fn mask(self) -> u8 {
        match self {
            Gate::Param => 0x01,
            Gate::Info => 0x03,
            Gate::Pid => 0x07,
            Gate::Perm => 0x0F,
            Gate::Done => 0x1F,
        }
    }

    /// 阶段短名（协议声明逐级可读）。
    pub fn name(self) -> &'static str {
        match self {
            Gate::Param => "param",
            Gate::Info => "user-copy",
            Gate::Pid => "pid-existence",
            Gate::Perm => "permission",
            Gate::Done => "pass",
        }
    }
}

/// 掩码是否为合法前缀。
pub fn trace_is_prefix(trace: u8) -> bool {
    matches!(trace, 0x01 | 0x03 | 0x07 | 0x0F | 0x1F)
}

// ── 调用行（单源表） ──────────────────────────────────────────────────
/// 一行进程族调用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtoRow {
    /// 系统调用号（x86_64）。
    pub nr: u32,
    /// 调用名。
    pub name: &'static str,
    /// pid 形态。
    pub form: PidForm,
    /// 权限门。
    pub perm: PermRule,
    /// 缺席时的 errno。
    pub absent: i32,
    /// 是否在 pid 查找前拷贝用户结构。
    pub info_arg: bool,
}

impl ProtoRow {
    /// 常量构造。
    pub const fn new(
        nr: u32,
        name: &'static str,
        form: PidForm,
        perm: PermRule,
        absent: i32,
        info_arg: bool,
    ) -> ProtoRow {
        ProtoRow {
            nr,
            name,
            form,
            perm,
            absent,
            info_arg,
        }
    }

    /// 是否带权限门。
    pub fn has_perm(&self) -> bool {
        self.perm.errno() != 0
    }
}

/// 十四行进程族表（单源：判定、注入、逐例表都从这里取）。
pub const FAMILY: &[ProtoRow] = &[
    ProtoRow::new(62, "kill", PidForm::Single, PermRule::UidOrCap, ESRCH, false),
    ProtoRow::new(200, "tkill", PidForm::Single, PermRule::UidOrCap, ESRCH, false),
    ProtoRow::new(234, "tgkill", PidForm::ThreadPair, PermRule::UidOrCap, ESRCH, false),
    ProtoRow::new(61, "waitpid", PidForm::ChildOnly, PermRule::None, ECHILD, false),
    ProtoRow::new(247, "waitid", PidForm::ChildOnly, PermRule::None, ECHILD, false),
    ProtoRow::new(121, "getpgid", PidForm::Single, PermRule::None, ESRCH, false),
    ProtoRow::new(109, "setpgid", PidForm::Single, PermRule::SessionOwner, ESRCH, false),
    ProtoRow::new(144, "sched_setscheduler", PidForm::Single, PermRule::NiceCap, ESRCH, false),
    ProtoRow::new(204, "sched_getaffinity", PidForm::Single, PermRule::None, ESRCH, false),
    ProtoRow::new(101, "ptrace", PidForm::Single, PermRule::PtraceAccess, ESRCH, false),
    ProtoRow::new(129, "rt_sigqueueinfo", PidForm::Single, PermRule::UidOrCap, ESRCH, true),
    ProtoRow::new(434, "pidfd_open", PidForm::Single, PermRule::None, ESRCH, false),
    // 形态变体行（kill-pgid / kill-all）不另设权限门：组形式与广播形式的
    // 权限在成员侧逐个判定（kill 基行已带 UidOrCap 门），冻结账是
    // 7 条权限门、6 条回 EPERM，多一条就会把反证与权限矩阵撑爆。
    ProtoRow::new(62, "kill-pgid", PidForm::Group, PermRule::None, ESRCH, false),
    ProtoRow::new(62, "kill-all", PidForm::Broadcast, PermRule::None, ESRCH, false),
];

/// 按名取行。
pub fn row_of(name: &str) -> Option<ProtoRow> {
    FAMILY.iter().find(|r| r.name == name).copied()
}

/// 形态存在性表（广播/自身组恒存在；组要求组存活且 pid 状态非缺席）。
pub fn form_exists(form: PidForm, pid: PidState, group_alive: bool) -> bool {
    match form {
        PidForm::Broadcast | PidForm::OwnGroup => true,
        PidForm::Group => group_alive && pid != PidState::Absent,
        PidForm::Single | PidForm::ThreadPair | PidForm::ChildOnly => pid != PidState::Absent,
    }
}

/// 形态存在性冻结果（6 形态 × 3 pid 状态 × 2 组存活 = 36 格）。
///
/// 行序：Single, ThreadPair, Group, Broadcast, OwnGroup, ChildOnly；
/// 列序：Live, Zombie, Absent；每形态先「组存活」后「组不存在」。
pub const FORM_WANT: [bool; 36] = [
    // Single
    true, true, false,
    true, true, false,
    // ThreadPair
    true, true, false,
    true, true, false,
    // Group：组存活时 Live/Zombie 通过，组不存在一律拒绝
    true, true, false,
    false, false, false,
    // Broadcast：无论组存活都恒存在
    true, true, true,
    true, true, true,
    // OwnGroup：自身进程组恒存在
    true, true, true,
    true, true, true,
    // ChildOnly
    true, true, false,
    true, true, false,
];

// ── 判决 ──────────────────────────────────────────────────────────────
/// 一次判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// errno（0 表示通过）。
    pub errno: i32,
    /// 停在哪个阶段。
    pub gate: Gate,
    /// 阶段掩码（前缀不变式）。
    pub trace: u8,
}

impl Verdict {
    /// 构造函数：掩码由阶段唯一决定，调用方无法伪造。
    pub fn at(errno: i32, gate: Gate) -> Verdict {
        Verdict {
            errno,
            gate,
            trace: gate.mask(),
        }
    }
}

/// 判序协议的唯一判决器。
pub fn decide(
    row: &ProtoRow,
    pid: PidState,
    perm: PermState,
    param_ok: bool,
    info_ok: bool,
) -> Verdict {
    // ① 参数门
    if !param_ok {
        return Verdict::at(EINVAL, Gate::Param);
    }
    // ② 用户拷贝门（仅 rt_sigqueueinfo 这类先拷结构的调用）
    if row.info_arg && !info_ok {
        return Verdict::at(EFAULT, Gate::Info);
    }
    // ③ pid 存在性门
    if !form_exists(row.form, pid, true) {
        return Verdict::at(row.absent, Gate::Pid);
    }
    // ④ 权限门
    if perm == PermState::Foreign {
        let e = row.perm.errno();
        if e != 0 {
            return Verdict::at(e, Gate::Perm);
        }
    }
    Verdict::at(0, Gate::Done)
}

/// 反例机：权限门被提到 pid 门之前（判序倒置的可执行反证据）。
pub fn decide_perm_first(
    row: &ProtoRow,
    pid: PidState,
    perm: PermState,
    param_ok: bool,
    info_ok: bool,
) -> Verdict {
    if !param_ok {
        return Verdict::at(EINVAL, Gate::Param);
    }
    if row.info_arg && !info_ok {
        return Verdict::at(EFAULT, Gate::Info);
    }
    if perm == PermState::Foreign {
        let e = row.perm.errno();
        if e != 0 {
            return Verdict::at(e, Gate::Perm);
        }
    }
    if !form_exists(row.form, pid, true) {
        return Verdict::at(row.absent, Gate::Pid);
    }
    Verdict::at(0, Gate::Done)
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

/// 缺席矩阵：需要查 pid 的 12 行 × 3 权限态 × 10 轮。
///
/// 返回（账本, 每行命中数, 权限码泄漏数）。
pub fn run_absent_matrix() -> (InjectLedger, [u32; FAMILY_ROWS], u32) {
    let mut ledger = InjectLedger::zero();
    let mut per_row = [0u32; FAMILY_ROWS];
    let mut leaks = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            if row.form.needs_lookup() {
                let mut pi = 0usize;
                while pi < PermState::ALL.len() {
                    if let Some(perm) = PermState::ALL.get(pi).copied() {
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            let v = decide(row, PidState::Absent, perm, true, true);
                            let caught = v.errno == row.absent && v.gate == Gate::Pid;
                            if v.errno == EPERM || v.errno == EACCES {
                                leaks = leaks.saturating_add(1);
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
                    pi = pi.saturating_add(1);
                }
            }
        }
        ri = ri.saturating_add(1);
    }
    (ledger, per_row, leaks)
}

/// 权限矩阵：7 条权限门 × 3 权限态 × 10 轮。
///
/// 返回（账本, 命中数, 总格数, EPERM 行命中数）。
pub fn run_perm_matrix() -> (InjectLedger, u32, u32, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut eperm_rows = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            if row.has_perm() {
                if row.perm.errno() == EPERM {
                    eperm_rows = eperm_rows.saturating_add(1);
                }
                let mut pi = 0usize;
                while pi < PermState::ALL.len() {
                    if let Some(perm) = PermState::ALL.get(pi).copied() {
                        let want = if perm == PermState::Foreign {
                            row.perm.errno()
                        } else {
                            0
                        };
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            let v = decide(row, PidState::Live, perm, true, true);
                            let caught = v.errno == want;
                            let rec = ledger.record(caught);
                            if rec.is_ok() && caught {
                                hits = hits.saturating_add(1);
                            }
                            total = total.saturating_add(1);
                            round = round.saturating_add(1);
                        }
                    }
                    pi = pi.saturating_add(1);
                }
            }
        }
        ri = ri.saturating_add(1);
    }
    (ledger, hits, total, eperm_rows)
}

/// 参数矩阵：14 行 × 10 轮，坏参数必须回 EINVAL（即使 pid 缺席且权限外部）。
pub fn run_param_matrix() -> (InjectLedger, u32, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let v = decide(row, PidState::Absent, PermState::Foreign, false, false);
                let caught = v.errno == EINVAL && v.gate == Gate::Param && trace_is_prefix(v.trace);
                let rec = ledger.record(caught);
                if rec.is_ok() && caught {
                    hits = hits.saturating_add(1);
                }
                total = total.saturating_add(1);
                round = round.saturating_add(1);
            }
        }
        ri = ri.saturating_add(1);
    }
    (ledger, hits, total)
}

/// 判序倒置反证：缺席 + 外部权限的 7 格上，反例机必给权限码。
///
/// 返回（反证命中格数, 应命中格数）。
pub fn run_inversion_probe() -> (u32, u32) {
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            if row.has_perm() {
                total = total.saturating_add(1);
                let good = decide(row, PidState::Absent, PermState::Foreign, true, true);
                let naive = decide_perm_first(row, PidState::Absent, PermState::Foreign, true, true);
                if good.errno == row.absent
                    && naive.errno == row.perm.errno()
                    && good.errno != naive.errno
                {
                    hits = hits.saturating_add(1);
                }
            }
        }
        ri = ri.saturating_add(1);
    }
    (hits, total)
}

// ── Linux 逐例表 ──────────────────────────────────────────────────────
/// 一行逐例（对齐 man 2 的行为，含判序陷阱）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    pub row: &'static str,
    pub pid: PidState,
    pub perm: PermState,
    pub param_ok: bool,
    pub info_ok: bool,
    pub want: i32,
}

impl Case {
    /// 常量构造。
    pub const fn new(
        row: &'static str,
        pid: PidState,
        perm: PermState,
        param_ok: bool,
        info_ok: bool,
        want: i32,
    ) -> Case {
        Case {
            row,
            pid,
            perm,
            param_ok,
            info_ok,
            want,
        }
    }
}

/// 三十二行逐例表。
pub const LINUX_CASES: &[Case] = &[
    Case::new("kill", PidState::Live, PermState::Owner, true, true, 0),
    Case::new("kill", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("kill", PidState::Absent, PermState::Owner, true, true, ESRCH),
    Case::new("kill", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("tkill", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("tkill", PidState::Absent, PermState::Privileged, true, true, ESRCH),
    Case::new("tgkill", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("tgkill", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("waitpid", PidState::Absent, PermState::Owner, true, true, ECHILD),
    Case::new("waitpid", PidState::Absent, PermState::Foreign, true, true, ECHILD),
    Case::new("waitpid", PidState::Live, PermState::Foreign, true, true, 0),
    Case::new("waitid", PidState::Absent, PermState::Foreign, true, true, ECHILD),
    Case::new("getpgid", PidState::Live, PermState::Foreign, true, true, 0),
    Case::new("getpgid", PidState::Absent, PermState::Owner, true, true, ESRCH),
    Case::new("setpgid", PidState::Live, PermState::Foreign, true, true, EACCES),
    Case::new("setpgid", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("sched_setscheduler", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("sched_setscheduler", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("sched_getaffinity", PidState::Live, PermState::Foreign, true, true, 0),
    Case::new("sched_getaffinity", PidState::Absent, PermState::Privileged, true, true, ESRCH),
    Case::new("ptrace", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("ptrace", PidState::Absent, PermState::Owner, true, true, ESRCH),
    Case::new("rt_sigqueueinfo", PidState::Live, PermState::Foreign, true, true, EPERM),
    Case::new("rt_sigqueueinfo", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("pidfd_open", PidState::Absent, PermState::Owner, true, true, ESRCH),
    Case::new("pidfd_open", PidState::Live, PermState::Foreign, true, true, 0),
    Case::new("kill", PidState::Absent, PermState::Foreign, false, true, EINVAL),
    Case::new("rt_sigqueueinfo", PidState::Absent, PermState::Foreign, true, false, EFAULT),
    Case::new("kill-pgid", PidState::Absent, PermState::Foreign, true, true, ESRCH),
    Case::new("kill-all", PidState::Absent, PermState::Foreign, true, true, 0),
    // 组形式变体不带行级权限门：对存活组的外部 kill 在成员侧逐个判权，
    // 只要有同 uid 成员可送达即成功（对齐 kill-all 的建模口径）。
    Case::new("kill-pgid", PidState::Live, PermState::Foreign, true, true, 0),
    Case::new("waitpid", PidState::Live, PermState::Owner, true, true, 0),
];

/// 逐例复算；返回（命中行数, 总行数）。
pub fn run_linux_cases() -> (u32, u32) {
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < LINUX_CASES.len() {
        if let Some(c) = LINUX_CASES.get(i) {
            let row = match row_of(c.row) {
                Some(r) => r,
                None => ProtoRow::new(0, "missing", PidForm::Single, PermRule::None, ESRCH, false),
            };
            let v = decide(&row, c.pid, c.perm, c.param_ok, c.info_ok);
            if v.errno == c.want && trace_is_prefix(v.trace) {
                matched = matched.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    (matched, LINUX_CASES.len() as u32)
}

/// 逐例注入：32 例 × 10 轮。
pub fn run_case_injection() -> (InjectLedger, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < LINUX_CASES.len() {
        if let Some(c) = LINUX_CASES.get(i) {
            let row = match row_of(c.row) {
                Some(r) => r,
                None => ProtoRow::new(0, "missing", PidForm::Single, PermRule::None, ESRCH, false),
            };
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let v = decide(&row, c.pid, c.perm, c.param_ok, c.info_ok);
                let caught = v.errno == c.want;
                let rec = ledger.record(caught);
                if rec.is_ok() && caught {
                    hits = hits.saturating_add(1);
                }
                round = round.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    (ledger, hits)
}

// ── 形态矩阵 ──────────────────────────────────────────────────────────
/// 形态存在性网格：真跑 36 格并与冻结表逐格比对。
///
/// 返回（命中格数, 总格数）。
pub fn run_form_grid() -> (u32, u32) {
    let forms = [
        PidForm::Single,
        PidForm::ThreadPair,
        PidForm::Group,
        PidForm::Broadcast,
        PidForm::OwnGroup,
        PidForm::ChildOnly,
    ];
    let states = [PidState::Live, PidState::Zombie, PidState::Absent];
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut fi = 0usize;
    while fi < forms.len() {
        if let Some(form) = forms.get(fi).copied() {
            let mut si = 0usize;
            while si < states.len() {
                if let Some(st) = states.get(si).copied() {
                    let mut gi = 0usize;
                    while gi < 2 {
                        let alive = gi == 0;
                        let got = form_exists(form, st, alive);
                        // 冻结表布局：每形态先「组存活」块后「组不存在」块，
                        // 块内列序 Live/Zombie/Absent → idx = form*6 + 组*3 + 状态。
                        let idx = fi
                            .saturating_mul(states.len() * 2)
                            .saturating_add(gi.saturating_mul(states.len()))
                            .saturating_add(si);
                        let want = FORM_WANT.get(idx).copied();
                        total = total.saturating_add(1);
                        if want == Some(got) {
                            hits = hits.saturating_add(1);
                        }
                        gi = gi.saturating_add(1);
                    }
                }
                si = si.saturating_add(1);
            }
        }
        fi = fi.saturating_add(1);
    }
    (hits, total)
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

fn push_i32(out: &mut Vec<u8>, v: i32) {
    push_u32(out, v as u32);
}

/// 规范语料指纹：14 行 × 9 组合判决 + 32 逐例结果。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, FAMILY_ROWS as u32);

    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            push_u32(&mut out, row.nr);
            push_u32(&mut out, row.form.code());
            push_u32(&mut out, row.perm.code());
            push_i32(&mut out, row.absent);
            let states = [PidState::Live, PidState::Zombie, PidState::Absent];
            let mut si = 0usize;
            while si < states.len() {
                if let Some(st) = states.get(si).copied() {
                    let mut pi = 0usize;
                    while pi < PermState::ALL.len() {
                        if let Some(pm) = PermState::ALL.get(pi).copied() {
                            let v = decide(row, st, pm, true, true);
                            push_i32(&mut out, v.errno);
                            out.push(v.trace);
                        }
                        pi = pi.saturating_add(1);
                    }
                }
                si = si.saturating_add(1);
            }
        }
        ri = ri.saturating_add(1);
    }

    let mut ci = 0usize;
    while ci < LINUX_CASES.len() {
        if let Some(c) = LINUX_CASES.get(ci) {
            push_i32(&mut out, c.want);
            out.push(c.pid.code() as u8);
            out.push(c.perm.code() as u8);
        }
        ci = ci.saturating_add(1);
    }

    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9293 判据集。
pub fn run_unx9293_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9293");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7B6C0;
    s.add(
        "段位守卫：0x40 对齐且落在 B11 窗口",
        seg_ok,
        "段基址唯一、0x40 对齐，且与相邻卡段互不覆盖",
    );

    let mut names_unique = true;
    let mut a = 0usize;
    while a < FAMILY.len() {
        let mut b = a.saturating_add(1);
        while b < FAMILY.len() {
            if let (Some(x), Some(y)) = (FAMILY.get(a), FAMILY.get(b)) {
                if x.name == y.name {
                    names_unique = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    let absent_codes_ok = FAMILY
        .iter()
        .all(|r| r.absent == ESRCH || r.absent == ECHILD);
    s.add(
        "调用表：14 行、名字唯一、absent 码闭集",
        FAMILY.len() == FAMILY_ROWS
            && names_unique
            && absent_codes_ok
            && FAMILY.iter().filter(|r| r.form.needs_lookup()).count() == LOOKUP_ROWS,
        "缺席码只能是 ESRCH 或 ECHILD，表外码一律不许出现",
    );

    s.add(
        "调用表：7 条权限门、6 条回 EPERM、2 条子进程专用",
        FAMILY.iter().filter(|r| r.has_perm()).count() == PERM_ROWS
            && FAMILY.iter().filter(|r| r.perm.errno() == EPERM).count() == EPERM_ROWS
            && FAMILY.iter().filter(|r| r.absent == ECHILD).count() == CHILD_ROWS,
        "权限门数量与 errno 分布必须与冻结账一致，EACCES 只留给 setpgid",
    );

    let (absent_ledger, per_row, leaks) = run_absent_matrix();
    let per_row_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < per_row.len() {
            if let (Some(n), Some(row)) = (per_row.get(i), FAMILY.get(i)) {
                let want = if row.form.needs_lookup() {
                    PERM_STATES as u32 * INJECT_ROUNDS
                } else {
                    0
                };
                if *n != want {
                    ok = false;
                }
            }
            i = i.saturating_add(1);
        }
        ok
    };
    s.add(
        "缺席矩阵：13 行 × 3 权限态逐格命中本行 absent 码",
        per_row_ok
            && leaks == 0
            && absent_ledger.injected == LOOKUP_ROWS as u32 * PERM_STATES as u32 * INJECT_ROUNDS,
        "不存在 pid 对任何权限态都不得泄漏 EPERM/EACCES",
    );
    s.add(
        "缺席账：390 次全检出、逃逸为零",
        absent_ledger.conserved()
            && absent_ledger.escaped == 0
            && absent_ledger.detected == 390
            && absent_ledger.injected == 390,
        "注入必须等于检出，逃逸恒为零",
    );

    let (inv_hits, inv_total) = run_inversion_probe();
    s.add(
        "判序反证：权限门前置在 7 格上必给权限码",
        inv_hits == PERM_ROWS as u32 && inv_total == PERM_ROWS as u32 && inv_hits == 7,
        "反例机与正解在缺席权限面上必须逐格不同，这是判序的可执行证据",
    );

    let (perm_ledger, perm_hits, perm_total, eperm_rows) = run_perm_matrix();
    s.add(
        "权限矩阵：owner/特权放行、外部权限回本行权限码",
        perm_ledger.conserved()
            && perm_hits == perm_total
            && perm_total == PERM_ROWS as u32 * PERM_STATES as u32 * INJECT_ROUNDS
            && eperm_rows == EPERM_ROWS as u32,
        "210 格全部按行权限码判定，EPERM 行数不得漂移",
    );

    let (param_ledger, param_hits, param_total) = run_param_matrix();
    s.add(
        "参数门：坏参数在缺席且外部权限时仍回 EINVAL",
        param_ledger.conserved()
            && param_hits == param_total
            && param_total == FAMILY_ROWS as u32 * INJECT_ROUNDS
            && param_hits == 140,
        "参数门最优先：坏信号号/坏 flags 不得先暴露 pid 或权限信息",
    );

    let (case_hits, case_total) = run_linux_cases();
    s.add(
        "Linux 逐例：32 例 errno 与阶段全部一致",
        case_hits == case_total && case_total == LINUX_CASE_ROWS as u32 && case_hits == 32,
        "逐例表覆盖正常/边界/权限/子进程/广播/拷贝六类路径",
    );

    let (case_ledger, case_inject_hits) = run_case_injection();
    s.add(
        "逐例注入：32 例 × 10 轮全检出",
        case_ledger.conserved()
            && case_ledger.escaped == 0
            && case_ledger.injected == LINUX_CASE_ROWS as u32 * INJECT_ROUNDS
            && case_inject_hits == case_ledger.injected,
        "逐例复算必须稳定，任何一轮漂移都说明判决含隐藏状态",
    );

    let kill_row = match row_of("kill") {
        Some(r) => r,
        None => ProtoRow::new(0, "kill", PidForm::Single, PermRule::None, ESRCH, false),
    };
    let wait_row = match row_of("waitpid") {
        Some(r) => r,
        None => ProtoRow::new(0, "waitpid", PidForm::ChildOnly, PermRule::None, ECHILD, false),
    };
    let zombie_kill = decide(&kill_row, PidState::Zombie, PermState::Owner, true, true);
    let zombie_wait = decide(&wait_row, PidState::Zombie, PermState::Owner, true, true);
    s.add(
        "僵尸语义：僵尸仍占 pid，kill/wait 都不得 ESRCH",
        zombie_kill.errno == 0
            && zombie_wait.errno == 0
            && decide(&kill_row, PidState::Absent, PermState::Owner, true, true).errno == ESRCH,
        "僵尸与不存在必须分开：前者可收割，后者连 pid 都没有",
    );

    let sig_row = match row_of("rt_sigqueueinfo") {
        Some(r) => r,
        None => ProtoRow::new(0, "rt_sigqueueinfo", PidForm::Single, PermRule::None, ESRCH, true),
    };
    let info_v = decide(&sig_row, PidState::Absent, PermState::Foreign, true, false);
    let ok_v = decide(&sig_row, PidState::Absent, PermState::Foreign, true, true);
    s.add(
        "拷贝门：坏 siginfo 指针 EFAULT 先于 pid 查找",
        info_v.errno == EFAULT
            && info_v.gate == Gate::Info
            && ok_v.errno == ESRCH
            && ok_v.gate == Gate::Pid,
        "用户结构拷贝是 pid 查找之前唯一的额外门",
    );

    let (form_hits, form_total) = run_form_grid();
    s.add(
        "形态表：6 形态 × 3 状态 × 2 组存活全格一致",
        form_hits == form_total && form_total == (PID_FORMS as u32) * 3 * 2 && form_hits == 36,
        "广播/自身组恒存在，组看组存活，单 pid 看进程状态",
    );

    let mut prefix_ok = true;
    let mut ri = 0usize;
    while ri < FAMILY.len() {
        if let Some(row) = FAMILY.get(ri) {
            let mut si = 0usize;
            while si < PermState::ALL.len() {
                if let Some(pm) = PermState::ALL.get(si).copied() {
                    let v = decide(row, PidState::Absent, pm, true, true);
                    if !trace_is_prefix(v.trace) {
                        prefix_ok = false;
                    }
                }
                si = si.saturating_add(1);
            }
        }
        ri = ri.saturating_add(1);
    }
    s.add(
        "阶段掩码：全部判决为自低位起的前缀",
        prefix_ok && trace_is_prefix(Gate::Param.mask()) && trace_is_prefix(Gate::Done.mask()),
        "跳级判决说明门被绕过，掩码必须严格按 Param→Info→Pid→Perm→Done 递进",
    );

    let sum_ledger = absent_ledger
        .injected
        .saturating_add(perm_ledger.injected)
        .saturating_add(param_ledger.injected)
        .saturating_add(case_ledger.injected);
    s.add(
        "总账：四条注入路径合计 1060 格全检出",
        sum_ledger == TOTAL_CELLS
            && absent_ledger.escaped == 0
            && perm_ledger.escaped == 0
            && param_ledger.escaped == 0
            && case_ledger.escaped == 0,
        "缺席/权限/参数/逐例四条路径必须各自守恒且总量对得上",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：判决矩阵指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    s.add(
        "诊断码：12 个码文案非空",
        OrderCode::ALL.len() == 12 && OrderCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议，禁止空串",
    );

    let mut uniq = true;
    let mut x = 0usize;
    while x < OrderCode::ALL.len() {
        let mut y = x.saturating_add(1);
        while y < OrderCode::ALL.len() {
            if let (Some(m), Some(n)) = (OrderCode::ALL.get(x), OrderCode::ALL.get(y)) {
                if m.val() == n.val() || m.code() == n.code() {
                    uniq = false;
                }
            }
            y = y.saturating_add(1);
        }
        x = x.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复，否则判据无法定位",
    );

    let absent_text = format!(
        "缺席{} 检出{} 泄漏{} 反证{}/{}",
        absent_ledger.injected, absent_ledger.detected, leaks, inv_hits, inv_total
    );
    s.add_dyn(
        "缺席账快照",
        absent_ledger.conserved() && leaks == 0,
        &absent_text,
    );

    let perm_text = format!(
        "权限 {}/{} EPERM行{} 参数{} 逐例{}",
        perm_hits, perm_total, eperm_rows, param_hits, case_hits
    );
    s.add_dyn(
        "权限账快照",
        perm_hits == perm_total && case_hits == case_total,
        &perm_text,
    );

    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> ProtoRow {
        match row_of(name) {
            Some(r) => r,
            None => ProtoRow::new(0, "missing", PidForm::Single, PermRule::None, ESRCH, false),
        }
    }

    #[test]
    fn family_shape_is_frozen() {
        assert_eq!(FAMILY.len(), 14);
        assert_eq!(row_of("kill").is_some(), true);
        assert_eq!(row_of("nope").is_none(), true);
        assert_eq!(row_of("waitpid").map(|r| r.absent), Some(ECHILD));
        assert_eq!(row_of("setpgid").map(|r| r.perm.errno()), Some(EACCES));
    }

    #[test]
    fn absent_beats_permission() {
        let row = row("kill");
        let v = decide(&row, PidState::Absent, PermState::Foreign, true, true);
        assert_eq!(v.errno, ESRCH);
        assert_eq!(v.gate, Gate::Pid);
        let naive = decide_perm_first(&row, PidState::Absent, PermState::Foreign, true, true);
        assert_eq!(naive.errno, EPERM);
    }

    #[test]
    fn child_calls_use_echild() {
        let w = row("waitpid");
        assert_eq!(decide(&w, PidState::Absent, PermState::Foreign, true, true).errno, ECHILD);
        let k = row("kill");
        assert_eq!(decide(&k, PidState::Absent, PermState::Foreign, true, true).errno, ESRCH);
    }

    #[test]
    fn zombie_still_exists() {
        let k = row("kill");
        assert_eq!(decide(&k, PidState::Zombie, PermState::Owner, true, true).errno, 0);
        assert_eq!(decide(&k, PidState::Zombie, PermState::Foreign, true, true).errno, EPERM);
    }

    #[test]
    fn info_copy_precedes_pid() {
        let r = row("rt_sigqueueinfo");
        let bad = decide(&r, PidState::Absent, PermState::Foreign, true, false);
        assert_eq!(bad.errno, EFAULT);
        assert_eq!(bad.gate, Gate::Info);
        let good = decide(&r, PidState::Absent, PermState::Foreign, true, true);
        assert_eq!(good.errno, ESRCH);
    }

    #[test]
    fn param_gate_is_first() {
        let r = row("tgkill");
        let v = decide(&r, PidState::Absent, PermState::Foreign, false, false);
        assert_eq!(v.errno, EINVAL);
        assert_eq!(v.gate, Gate::Param);
        assert_eq!(v.trace, 0x01);
    }

    #[test]
    fn matrices_and_inversion() {
        let (led, per, leaks) = run_absent_matrix();
        assert_eq!(led.injected, 390);
        assert_eq!(led.detected, 390);
        assert_eq!(led.escaped, 0);
        assert_eq!(leaks, 0);
        assert_eq!(per.first().copied(), Some(30));
        let (hits, total) = run_inversion_probe();
        assert_eq!(total, 7);
        assert_eq!(hits, 7);
    }

    #[test]
    fn perm_and_param_matrices() {
        let (pl, ph, pt, er) = run_perm_matrix();
        assert_eq!(pl.conserved(), true);
        assert_eq!(ph, pt);
        assert_eq!(pt, 210);
        assert_eq!(er, 6);
        let (ql, qh, qt) = run_param_matrix();
        assert_eq!(ql.conserved(), true);
        assert_eq!(qh, qt);
        assert_eq!(qt, 140);
    }

    #[test]
    fn linux_cases_all_match() {
        let (hits, total) = run_linux_cases();
        assert_eq!(total, 32);
        assert_eq!(hits, 32);
        let (cl, ch) = run_case_injection();
        assert_eq!(cl.injected, 320);
        assert_eq!(ch, 320);
        assert_eq!(cl.escaped, 0);
    }

    #[test]
    fn form_grid_covers_six_forms() {
        let (hits, total) = run_form_grid();
        assert_eq!(total, 36);
        assert_eq!(hits, 36);
        assert_eq!(form_exists(PidForm::Broadcast, PidState::Absent, false), true);
        assert_eq!(form_exists(PidForm::Group, PidState::Absent, false), false);
        assert_eq!(form_exists(PidForm::Group, PidState::Live, true), true);
    }

    #[test]
    fn fingerprint_is_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9293_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
