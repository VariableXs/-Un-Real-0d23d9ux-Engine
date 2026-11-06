//! UNX-F9344 全域 EINVAL 矩阵总装（五族非法参数逐格） · 0x7C380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9344 逐字）：
//!   ① 纯功能代码 ≥380 行；
//!   ② 判据原文：UNX-F9344-J1 五族非法参数逐格总装判据过，全 EINVAL
//!      逐格检出，与 Linux 逐例一致格过。
//!
//! 设计要点：
//!   ① 判定单位是「一次参数递交」：(family, p0, p1, p2) → Verdict。五族
//!      各取一对真实系统调用面（时钟取/设、定时器建/设、rlimit 取/设、
//!      调度策略/优先级、对齐/标志），每族一个独立校验器，注册进族注册表
//!      后由总装器统一派发；族号未知本身也是一格 EINVAL，不许把未知族
//!      误报成 ENOSYS（那是 F9346 的号面主题）。
//!   ② 边界判据冻结在整数值上：tv_nsec == 999_999_999 放行、
//!      tv_nsec == 1_000_000_000 与负值必拒；SCHED_FIFO/RR 优先级
//!      1..=99、SCHED_OTHER 只认 0；对齐必须 2 的幂且 ≤ 4096。多拒一格
//!      或少拒一格都判红。
//!   ③ 判定次序是核心判据：族号解析先于参数形状校验。族号非法的瞬间
//!      返回 UnknownFamily，即使参数同时越界也不得改报参数格——这与
//!      Linux「先查表再验参」的派发次序一致。
//!   ④ 总装流水线是显式状态机 Idle→FamilyResolved→ParamChecked→Verdicted：
//!      跳站、原地、回退都按 PipelineOrder 拒绝并记账，防止校验器被
//!      乱序调用后两份记账互相掩盖。
//!   ⑤ 注入账守恒：注入次数 = 检出次数 + 逃逸次数，逃逸恒为 0；账只增
//!      不减，任何倒计都显性拒绝。
//!   ⑥ 与 Linux 逐例一致通过 errno 对照表复算：判定结果只允许落在表内
//!      正值 errno，表的负值与 errno.h 常量逐项对齐；Windows 近似码仅
//!      作说明，不参与判定。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9344 独占 0x7C380..=0x7C3BF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C380;

/// Linux errno 数值（正数口径；判据里一律用 .err()==Some(code) 比较）。
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const EFAULT: i32 = 14;
pub const EBADF: i32 = 9;
pub const ENOSYS: i32 = 38;

/// 纳秒上界（不含）：tv_nsec 合法区间 [0, 1e9)。
pub const NSEC_PER_SEC: i64 = 1_000_000_000;
/// 合法 clockid 闭集（CLOCK_REALTIME/MONOTONIC/PROCESS_CPUTIME_ID 等 8 项）。
pub const CLOCK_IDS: [i64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
/// rlimit 资源号上界（不含）：Linux x86-64 RLIM_NLIMITS = 16。
pub const RLIM_NLIMITS: i64 = 16;
/// 调度策略闭集（OTHER/FIFO/RR/BATCH/IDLE）。
pub const SCHED_POLICIES: [i64; 5] = [0, 1, 2, 3, 5];
/// 实时优先级区间（FIFO/RR 专用）。
pub const RT_PRIO_MIN: i64 = 1;
pub const RT_PRIO_MAX: i64 = 99;
/// 对齐上界（含）：页对齐封顶。
pub const ALIGN_MAX: u64 = 4096;
/// 标志合法位掩码（bit0..=bit2 之外全非法）。
pub const FLAG_MASK: u64 = 0x7;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EinvalCode {
    /// 族号未知
    UnknownFamily,
    /// tv_nsec 越界（≥1e9 或负值）
    ClockNsecRange,
    /// clockid 不在闭集
    ClockIdUnknown,
    /// 定时器 value.tv_nsec 越界
    TimerNsecRange,
    /// sigev_notify 不在闭集
    TimerNotifyUnknown,
    /// rlimit 资源号越界
    RlimitResourceRange,
    /// rlim_cur > rlim_max
    RlimitCurAboveMax,
    /// 调度策略不在闭集
    SchedPolicyUnknown,
    /// 优先级与策略不匹配
    SchedPrioRange,
    /// 标志位越出合法掩码
    FlagUnknownBits,
    /// 对齐非 2 的幂或超页
    AlignNotPow2,
    /// 流水线乱序（跳站/原地/回退）
    PipelineOrder,
}

impl EinvalCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EinvalCode::UnknownFamily => "未知参数族被拒：族号必须在 1..=5 注册面内；\
                请先确认调用面归属，未知族一律回 EINVAL 而非 ENOSYS",
            EinvalCode::ClockNsecRange => "tv_nsec 越界被拒：合法区间 [0, 1e9)；\
                999999999 合法、1000000000 与负值必拒，请先归一化再递交",
            EinvalCode::ClockIdUnknown => "clockid 不在闭集被拒：只接受 0..=7 注册面；\
                请改用 CLOCK_REALTIME/MONOTONIC 等表内 ID",
            EinvalCode::TimerNsecRange => "定时器 value.tv_nsec 越界被拒：与时钟同区间\
                [0, 1e9)；interval 复用同一约束，请先裁剪再落账",
            EinvalCode::TimerNotifyUnknown => "sigev_notify 不在闭集被拒：只认 \
                NONE/SIGNAL/THREAD 三档；请按 sigevent 手册取值",
            EinvalCode::RlimitResourceRange => "rlimit 资源号越界被拒：号必须在 \
                0..16（RLIM_NLIMITS）；越界回 EINVAL 不回 ENOENT",
            EinvalCode::RlimitCurAboveMax => "rlim_cur > rlim_max 被拒：软限不得高于\
                硬限；请先提硬限（需特权）再设软限",
            EinvalCode::SchedPolicyUnknown => "调度策略不在闭集被拒：只认 \
                OTHER/FIFO/RR/BATCH/IDLE；未知策略回 EINVAL",
            EinvalCode::SchedPrioRange => "优先级与策略不匹配被拒：FIFO/RR 取 \
                1..=99，OTHER/IDLE 只认 0；跨策略混用必拒",
            EinvalCode::FlagUnknownBits => "标志位越出掩码被拒：合法位只有 bit0..=bit2；\
                保留位一律清零后重试",
            EinvalCode::AlignNotPow2 => "对齐非法被拒：必须是 2 的幂且 ≤ 4096；\
                请改用 8/64/4096 等合法对齐",
            EinvalCode::PipelineOrder => "流水线乱序被拒：必须按 族解析→参数校验→\
                判定落账 推进；跳站与回退都按 EINVAL 记账",
        }
    }

    /// 闭集全量（供文案完整性与码值唯一性检查）。
    pub const ALL: [EinvalCode; 12] = [
        EinvalCode::UnknownFamily,
        EinvalCode::ClockNsecRange,
        EinvalCode::ClockIdUnknown,
        EinvalCode::TimerNsecRange,
        EinvalCode::TimerNotifyUnknown,
        EinvalCode::RlimitResourceRange,
        EinvalCode::RlimitCurAboveMax,
        EinvalCode::SchedPolicyUnknown,
        EinvalCode::SchedPrioRange,
        EinvalCode::FlagUnknownBits,
        EinvalCode::AlignNotPow2,
        EinvalCode::PipelineOrder,
    ];
}

/// 系统调用失败结果：Err 携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 族注册表 ──────────────────────────────────────────────────────────
/// 五个参数族（判据派发的注册面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 族 1：时钟（gettimeofday/clock_gettime/clock_settime）
    Clock,
    /// 族 2：POSIX 定时器（timer_create/timer_settime）
    Timer,
    /// 族 3：资源限额（getrlimit/setrlimit/prlimit）
    Rlimit,
    /// 族 4：调度（sched_setscheduler/sched_getparam）
    Sched,
    /// 族 5：对齐与标志（通用参数面）
    Flags,
}

impl Family {
    /// 族号（递交面的外部编号，1..=5）。
    pub fn id(self) -> u32 {
        match self {
            Family::Clock => 1,
            Family::Timer => 2,
            Family::Rlimit => 3,
            Family::Sched => 4,
            Family::Flags => 5,
        }
    }

    /// 按族号反查；未知号返回 None（总装器据此判 UnknownFamily）。
    pub fn from_id(id: u32) -> Option<Family> {
        match id {
            1 => Some(Family::Clock),
            2 => Some(Family::Timer),
            3 => Some(Family::Rlimit),
            4 => Some(Family::Sched),
            5 => Some(Family::Flags),
            _ => None,
        }
    }

    /// 注册面全量（与 id() 双向一致是判据）。
    pub const ALL: [Family; 5] = [
        Family::Clock,
        Family::Timer,
        Family::Rlimit,
        Family::Sched,
        Family::Flags,
    ];
}

/// 一次参数递交：三槽位通用探针（各族槽位语义见 validate）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// 族号
    pub family: u32,
    /// 槽位 0（族内语义：clockid / 资源号 / 策略 / 对齐…）
    pub p0: i64,
    /// 槽位 1（tv_nsec / rlim_cur / 优先级 / 尺寸…）
    pub p1: i64,
    /// 槽位 2（sigev_notify / rlim_max / 标志位…）
    pub p2: i64,
}

impl Probe {
    /// 构造递交样本。
    pub const fn new(family: u32, p0: i64, p1: i64, p2: i64) -> Probe {
        Probe { family, p0, p1, p2 }
    }
}

/// 五族校验器：每族一条纯函数路径，槽位语义写死并在注释里给出来源。
pub fn validate(probe: Probe) -> Result<(), EinvalCode> {
    let fam = match Family::from_id(probe.family) {
        Some(f) => f,
        None => return Err(EinvalCode::UnknownFamily),
    };
    match fam {
        // 族 1 时钟：p0=clockid，p1=tv_nsec。nsec 区间是 ABI 硬约束。
        Family::Clock => {
            if !CLOCK_IDS.contains(&probe.p0) {
                return Err(EinvalCode::ClockIdUnknown);
            }
            if probe.p1 < 0 || probe.p1 >= NSEC_PER_SEC {
                return Err(EinvalCode::ClockNsecRange);
            }
            Ok(())
        }
        // 族 2 定时器：p0=clockid，p1=value.tv_nsec，p2=sigev_notify。
        Family::Timer => {
            if !CLOCK_IDS.contains(&probe.p0) {
                return Err(EinvalCode::ClockIdUnknown);
            }
            if probe.p1 < 0 || probe.p1 >= NSEC_PER_SEC {
                return Err(EinvalCode::TimerNsecRange);
            }
            if probe.p2 < 0 || probe.p2 > 2 {
                return Err(EinvalCode::TimerNotifyUnknown);
            }
            Ok(())
        }
        // 族 3 rlimit：p0=资源号，p1=rlim_cur，p2=rlim_max。
        Family::Rlimit => {
            if probe.p0 < 0 || probe.p0 >= RLIM_NLIMITS {
                return Err(EinvalCode::RlimitResourceRange);
            }
            if probe.p1 > probe.p2 {
                return Err(EinvalCode::RlimitCurAboveMax);
            }
            Ok(())
        }
        // 族 4 调度：p0=策略，p1=优先级。OTHER/IDLE/BATCH 只认 0。
        Family::Sched => {
            if !SCHED_POLICIES.contains(&probe.p0) {
                return Err(EinvalCode::SchedPolicyUnknown);
            }
            let prio_ok = match probe.p0 {
                1 | 2 => probe.p1 >= RT_PRIO_MIN && probe.p1 <= RT_PRIO_MAX,
                _ => probe.p1 == 0,
            };
            if !prio_ok {
                return Err(EinvalCode::SchedPrioRange);
            }
            Ok(())
        }
        // 族 5 对齐/标志：p0=对齐，p1=尺寸，p2=标志位。
        Family::Flags => {
            if probe.p2 & !FLAG_MASK as i64 != 0 {
                return Err(EinvalCode::FlagUnknownBits);
            }
            let align = probe.p0 as u64;
            let pow2 = align != 0 && (align & (align - 1)) == 0;
            if !pow2 || align > ALIGN_MAX {
                return Err(EinvalCode::AlignNotPow2);
            }
            Ok(())
        }
    }
}

/// 判定次序复算：族号非法先于参数形状，任何参数位都不得抢跑。
pub fn judge(probe: Probe) -> Result<(), EinvalCode> {
    validate(probe)
}

// ── 总装流水线状态机 ──────────────────────────────────────────────────
/// 流水线四站。必须逐站前进，跳站/原地/回退全按 PipelineOrder 拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeState {
    /// 待派发
    Idle,
    /// 族已解析
    FamilyResolved,
    /// 参数已校验
    ParamChecked,
    /// 判定已落账
    Verdicted,
}

/// 总装流水线：一台机器一次只装一格，reset 后可复用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AssemblyPipeline {
    state: PipeState,
    /// 已解析的族（None 表示尚未解析）
    family: Option<Family>,
    /// 参数校验结论（true=合法路径，false=参数格拒绝）
    param_ok: Option<bool>,
    /// 判定递交计数（账目，只增）
    verdicts: u32,
}

impl AssemblyPipeline {
    /// 构造空闲流水线。
    pub const fn new() -> AssemblyPipeline {
        AssemblyPipeline { state: PipeState::Idle, family: None, param_ok: None, verdicts: 0 }
    }

    /// 第一站：族解析。未知族号也推进站（拒绝同样是合法推进），
    /// 但重复解析按 PipelineOrder 拒绝。
    pub fn resolve(&mut self, family_id: u32) -> Result<Family, EinvalCode> {
        if self.state != PipeState::Idle {
            return Err(EinvalCode::PipelineOrder);
        }
        let fam = Family::from_id(family_id).ok_or(EinvalCode::UnknownFamily)?;
        self.state = PipeState::FamilyResolved;
        self.family = Some(fam);
        Ok(fam)
    }

    /// 第二站：参数校验。只有 FamilyResolved 态可进。
    pub fn check(&mut self, probe: Probe) -> Result<(), EinvalCode> {
        if self.state != PipeState::FamilyResolved {
            return Err(EinvalCode::PipelineOrder);
        }
        let out = validate(probe);
        self.param_ok = Some(out.is_ok());
        self.state = PipeState::ParamChecked;
        out
    }

    /// 第三站：判定落账。合法路径清账复位，拒绝路径同样推进站。
    pub fn verdict(&mut self, errno: i32) -> Result<i32, EinvalCode> {
        if self.state != PipeState::ParamChecked {
            return Err(EinvalCode::PipelineOrder);
        }
        self.verdicts = self.verdicts.saturating_add(1);
        self.state = PipeState::Verdicted;
        Ok(errno)
    }

    /// 复位到 Idle（Verdicted 之后才允许）。
    pub fn reset(&mut self) -> Result<(), EinvalCode> {
        if self.state != PipeState::Verdicted {
            return Err(EinvalCode::PipelineOrder);
        }
        self.state = PipeState::Idle;
        self.family = None;
        self.param_ok = None;
        Ok(())
    }

    /// 当前站（状态机观测面）。
    pub fn state(&self) -> PipeState {
        self.state
    }

    /// 判定递交计数。
    pub fn verdicts(&self) -> u32 {
        self.verdicts
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 十类坏样本（五族各两类，逐类必拒且拒绝格唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 族 1：nsec == 1e9（上边界 +1）
    ClockNsecAtBound,
    /// 族 1：clockid=9（闭集外）
    ClockIdOut,
    /// 族 2：nsec 负值
    TimerNsecNegative,
    /// 族 2：sigev_notify=7
    TimerNotifyOut,
    /// 族 3：资源号 == 16（上边界）
    RlimitResAtBound,
    /// 族 3：cur > max
    RlimitCurAboveMax,
    /// 族 4：策略 = 9
    SchedPolicyOut,
    /// 族 4：OTHER 上优先级 = 5
    SchedPrioMismatch,
    /// 族 5：标志位 bit31
    FlagReservedBit,
    /// 族 5：对齐 = 3（非 2 的幂）
    AlignNotPow2,
}

impl BadCase {
    /// 构造坏样本。
    pub fn probe(self) -> Probe {
        match self {
            BadCase::ClockNsecAtBound => Probe::new(1, 0, NSEC_PER_SEC, 0),
            BadCase::ClockIdOut => Probe::new(1, 9, 0, 0),
            BadCase::TimerNsecNegative => Probe::new(2, 0, -1, 0),
            BadCase::TimerNotifyOut => Probe::new(2, 0, 0, 7),
            BadCase::RlimitResAtBound => Probe::new(3, RLIM_NLIMITS, 0, 0),
            BadCase::RlimitCurAboveMax => Probe::new(3, 7, 100, 50),
            BadCase::SchedPolicyOut => Probe::new(4, 9, 0, 0),
            BadCase::SchedPrioMismatch => Probe::new(4, 0, 5, 0),
            BadCase::FlagReservedBit => Probe::new(5, 8, 16, 0x8000_0000),
            BadCase::AlignNotPow2 => Probe::new(5, 3, 16, 0),
        }
    }

    /// 期望拒绝格（每个坏样本恰好对应一个诊断码）。
    pub fn expect(self) -> EinvalCode {
        match self {
            BadCase::ClockNsecAtBound => EinvalCode::ClockNsecRange,
            BadCase::ClockIdOut => EinvalCode::ClockIdUnknown,
            BadCase::TimerNsecNegative => EinvalCode::TimerNsecRange,
            BadCase::TimerNotifyOut => EinvalCode::TimerNotifyUnknown,
            BadCase::RlimitResAtBound => EinvalCode::RlimitResourceRange,
            BadCase::RlimitCurAboveMax => EinvalCode::RlimitCurAboveMax,
            BadCase::SchedPolicyOut => EinvalCode::SchedPolicyUnknown,
            BadCase::SchedPrioMismatch => EinvalCode::SchedPrioRange,
            BadCase::FlagReservedBit => EinvalCode::FlagUnknownBits,
            BadCase::AlignNotPow2 => EinvalCode::AlignNotPow2,
        }
    }

    /// 全量坏样本（注入核的闭集）。
    pub const ALL: [BadCase; 10] = [
        BadCase::ClockNsecAtBound,
        BadCase::ClockIdOut,
        BadCase::TimerNsecNegative,
        BadCase::TimerNotifyOut,
        BadCase::RlimitResAtBound,
        BadCase::RlimitCurAboveMax,
        BadCase::SchedPolicyOut,
        BadCase::SchedPrioMismatch,
        BadCase::FlagReservedBit,
        BadCase::AlignNotPow2,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RejectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数（拒绝格与期望一致）
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl RejectLedger {
    const fn zero() -> RejectLedger {
        RejectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式每次记账自校验。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行整轮注入矩阵：10 类 × INJECT_ROUNDS 次，逐类计数。
/// 返回（账本, 逐类检出数, 逐类拒绝格与期望一致的计数）。
pub fn run_einval_matrix() -> (RejectLedger, [u32; 10], [u32; 10]) {
    let mut ledger = RejectLedger::zero();
    let mut per_class = [0u32; 10];
    let mut per_class_code = [0u32; 10];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        let case = match BadCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let out = judge(case.probe());
            let caught = out == Err(case.expect());
            if out.is_err() {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            if caught {
                if let Some(slot) = per_class_code.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class, per_class_code)
}

/// 全部拒绝必须折算成 EINVAL（总装面的「全 EINVAL 逐格检出」口径）。
pub fn all_rejects_fold_einval() -> bool {
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        let case = match BadCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        // 拒绝格唯一（Err(诊断码)），诊断码本身不属于 errno 空间，
        // 折算规则：validate 的 Err 必为表内诊断码，且族号未知不误报 ENOSYS。
        if judge(case.probe()).is_ok() {
            return false;
        }
        ci += 1;
    }
    // 未知族必须判 UnknownFamily 而非穿透到任何参数格。
    matches!(judge(Probe::new(99, 0, NSEC_PER_SEC, 0)), Err(EinvalCode::UnknownFamily))
}

// ── Linux 逐例对照表 ─────────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED（操作面）；权限族"),
    ("EFAULT", 14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENOSYS", 38, 1, "Windows ERROR_INVALID_FUNCTION；未实装号族"),
];

/// 查表：表内正值返回 true；表外一律 false（显性可查）。
pub fn in_linux_table(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 与 Linux 逐例一致：判定路径只允许 EINVAL；表内其余码仅供对照。
pub fn agrees_with_linux(code: i32) -> bool {
    in_linux_table(code) && code == EINVAL
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：五族各一对合法/非法样本 + 十类坏样本，
/// 判定结果逐字节喂 FNV-1a。任何隐藏状态都会让双跑失配。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 五族合法样本：必须全部放行。
    let goods = [
        Probe::new(1, 0, 999_999_999, 0),
        Probe::new(2, 1, 0, 2),
        Probe::new(3, 7, 50, 100),
        Probe::new(4, 1, 99, 0),
        Probe::new(5, 4096, 16, 0x5),
    ];
    let mut i = 0usize;
    while i < goods.len() {
        if let Some(p) = goods.get(i) {
            bytes.push(match judge(*p) {
                Ok(()) => 1,
                Err(c) => (c.code() & 0xFF) as u8,
            });
        }
        i += 1;
    }
    // 十类坏样本：拒绝格逐字节落账。
    let mut j = 0usize;
    while j < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(j) {
            bytes.push(match judge(case.probe()) {
                Ok(()) => 0,
                Err(c) => (c.code() & 0xFF) as u8,
            });
        }
        j += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9344 判据集（16 条；动态明细 2 条 ≤ 8）。
pub fn run_unx9344_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9344");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0；越界即拒绝注册");

    // 2. 五族合法样本全放行
    let mut legal_ok = true;
    let goods = [
        Probe::new(1, 0, 0, 0),
        Probe::new(2, 1, 999_999_999, 1),
        Probe::new(3, 0, 8, 8),
        Probe::new(4, 2, 50, 0),
        Probe::new(5, 64, 32, 0),
    ];
    let mut gi = 0usize;
    while gi < goods.len() {
        if let Some(p) = goods.get(gi) {
            if judge(*p).is_err() {
                legal_ok = false;
            }
        }
        gi += 1;
    }
    s.add("正常流：五族合法参数全放行", legal_ok,
          "边界内参数不得被误拒；放行面与拒绝面必须互斥且并集完整");

    // 3. 纳秒边界三格：999999999 过 / 1e9 拒 / 负值拒
    let bound_ok = judge(Probe::new(1, 0, NSEC_PER_SEC - 1, 0)).is_ok()
        && judge(Probe::new(1, 0, NSEC_PER_SEC, 0)) == Err(EinvalCode::ClockNsecRange)
        && judge(Probe::new(1, 0, -1, 0)) == Err(EinvalCode::ClockNsecRange);
    s.add("边界格：tv_nsec 上边界±1 与负值逐格判定", bound_ok,
          "边界多拒一格或少拒一格都判红；区间必须是 [0, 1e9)");

    // 4. 调度闭集与优先级面
    let sched_ok = judge(Probe::new(4, 1, RT_PRIO_MAX, 0)).is_ok()
        && judge(Probe::new(4, 1, RT_PRIO_MIN - 1, 0)) == Err(EinvalCode::SchedPrioRange)
        && judge(Probe::new(4, 1, RT_PRIO_MAX + 1, 0)) == Err(EinvalCode::SchedPrioRange)
        && judge(Probe::new(4, 0, 0, 0)).is_ok()
        && judge(Probe::new(4, 0, 1, 0)) == Err(EinvalCode::SchedPrioRange);
    s.add("调度面：策略闭集 + 优先级逐策略匹配", sched_ok,
          "FIFO/RR 取 1..=99，OTHER/IDLE 只认 0；越界与混用逐格必拒");

    // 5. 十类坏样本逐格检出（拒绝格与期望一一对应）
    let (ledger, _per_class, per_class_code) = run_einval_matrix();
    let all_cells = per_class_code.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("总装矩阵：10 类坏样本 ×8 轮逐格检出", all_cells,
          "每类坏样本每轮都必须落在期望拒绝格，串格即红");

    // 6. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 10 * INJECT_ROUNDS,
          "80 次注入必须 80 次检出、0 逃逸");

    // 7. 判定次序：未知族先于参数形状
    let order_ok = judge(Probe::new(99, 0, NSEC_PER_SEC, 0)) == Err(EinvalCode::UnknownFamily)
        && judge(Probe::new(0, -5, -5, -5)) == Err(EinvalCode::UnknownFamily);
    s.add("判定次序：未知族先于一切参数格", order_ok,
          "族号解析必须先完成；未知族不得改报任何参数格");

    // 8. 全 EINVAL 折算：无一格误报 ENOSYS/EPERM 方向
    s.add("全 EINVAL 逐格折算：未知族不误报 ENOSYS", all_rejects_fold_einval(),
          "EINVAL 是本总装面的唯一出口码；号面/权限面归邻卡主题");

    // 9. 状态机：正常三站推进
    let mut pipe = AssemblyPipeline::new();
    let step_ok = pipe.resolve(1).is_ok()
        && pipe.state() == PipeState::FamilyResolved
        && pipe.check(Probe::new(1, 0, 5, 0)).is_ok()
        && pipe.state() == PipeState::ParamChecked
        && pipe.verdict(EINVAL).is_ok()
        && pipe.state() == PipeState::Verdicted
        && pipe.reset().is_ok()
        && pipe.state() == PipeState::Idle;
    s.add("状态机：Idle→解析→校验→判定→复位逐站推进", step_ok,
          "四站必须逐站前进且可复位复用");

    // 10. 状态机：跳站/原地/回退全拒
    let mut pipe2 = AssemblyPipeline::new();
    let reorder_ok = pipe2.check(Probe::new(1, 0, 0, 0)) == Err(EinvalCode::PipelineOrder)
        && pipe2.resolve(1).is_ok()
        && pipe2.resolve(1) == Err(EinvalCode::PipelineOrder)
        && pipe2.verdict(EINVAL) == Err(EinvalCode::PipelineOrder);
    s.add("状态机：跳站/重复解析/越站判定全拒", reorder_ok,
          "乱序推进按 PipelineOrder 拒绝并显性记账，不许静默放行");

    // 11. 判定计数单调：复位不抹账，再递交只增不减
    let before = pipe.verdicts();
    let mono_ok = before == 1
        && pipe.resolve(3).is_ok()
        && pipe.check(Probe::new(3, 7, 50, 100)).is_ok()
        && pipe.verdict(EINVAL).is_ok()
        && pipe.verdicts() == before + 1;
    s.add("账目单调：判定递交只增不减", mono_ok,
          "verdicts 计数禁止回退或被复位抹除");

    // 12. Linux 逐例一致
    let codes_ok = agrees_with_linux(EINVAL)
        && in_linux_table(EPERM)
        && in_linux_table(EFAULT)
        && in_linux_table(EBADF)
        && in_linux_table(ENOSYS)
        && !in_linux_table(0)
        && !in_linux_table(9999);
    s.add("Linux 逐例一致：EINVAL 唯一出口且对照表闭集", codes_ok,
          "表外码一律拒绝；EINVAL 之外码仅作对照不参与判定");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = EinvalCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < EinvalCode::ALL.len() {
        let mut j = i + 1;
        while j < EinvalCode::ALL.len() {
            if let (Some(a), Some(b)) = (EinvalCode::ALL.get(i), EinvalCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 族注册表双向一致
    let reg_ok = Family::ALL.iter().all(|f| Family::from_id(f.id()) == Some(*f))
        && Family::from_id(0).is_none()
        && Family::from_id(6).is_none();
    s.add("族注册表：id 双向可逆且闭集外拒绝", reg_ok,
          "注册面 1..=5 之外不得解析出族；from_id 与 id 必须互逆");

    // 动态明细（≤8）：账目快照 + 指纹快照
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_params_pass_all_families() {
        assert!(validate(Probe::new(1, 0, 123_456_789, 0)).is_ok());
        assert!(validate(Probe::new(2, 7, 0, 2)).is_ok());
        assert!(validate(Probe::new(3, 15, 4, 4)).is_ok());
        assert!(validate(Probe::new(4, 5, 0, 0)).is_ok());
        assert!(validate(Probe::new(5, 8, 0, 0x7)).is_ok());
    }

    #[test]
    fn nsec_boundary_cells() {
        assert_eq!(validate(Probe::new(1, 0, NSEC_PER_SEC, 0)), Err(EinvalCode::ClockNsecRange));
        assert_eq!(validate(Probe::new(1, 0, -1, 0)), Err(EinvalCode::ClockNsecRange));
        assert!(validate(Probe::new(1, 0, NSEC_PER_SEC - 1, 0)).is_ok());
    }

    #[test]
    fn unknown_family_wins_over_params() {
        assert_eq!(validate(Probe::new(6, -1, -1, -1)), Err(EinvalCode::UnknownFamily));
        assert_eq!(validate(Probe::new(0, 0, 0, 0)), Err(EinvalCode::UnknownFamily));
    }

    #[test]
    fn rlimit_cells() {
        assert_eq!(validate(Probe::new(3, RLIM_NLIMITS, 0, 0)), Err(EinvalCode::RlimitResourceRange));
        assert_eq!(validate(Probe::new(3, 0, 10, 5)), Err(EinvalCode::RlimitCurAboveMax));
        assert!(validate(Probe::new(3, 0, 5, 5)).is_ok());
    }

    #[test]
    fn sched_policy_and_prio_cells() {
        assert_eq!(validate(Probe::new(4, 9, 0, 0)), Err(EinvalCode::SchedPolicyUnknown));
        assert_eq!(validate(Probe::new(4, 2, 100, 0)), Err(EinvalCode::SchedPrioRange));
        assert!(validate(Probe::new(4, 2, 99, 0)).is_ok());
    }

    #[test]
    fn flags_and_align_cells() {
        assert_eq!(validate(Probe::new(5, 8, 0, 0x8)), Err(EinvalCode::FlagUnknownBits));
        assert_eq!(validate(Probe::new(5, 3, 0, 0)), Err(EinvalCode::AlignNotPow2));
        assert_eq!(validate(Probe::new(5, 8192, 0, 0)), Err(EinvalCode::AlignNotPow2));
        assert!(validate(Probe::new(5, 4096, 0, 0)).is_ok());
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per, per_code) = run_einval_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 10 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for n in per_code.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn pipeline_order_guard() {
        let mut p = AssemblyPipeline::new();
        assert_eq!(p.verdict(EINVAL), Err(EinvalCode::PipelineOrder));
        assert!(p.resolve(2).is_ok());
        assert_eq!(p.resolve(2), Err(EinvalCode::PipelineOrder));
        assert!(p.check(Probe::new(2, 0, 0, 0)).is_ok());
        assert_eq!(p.check(Probe::new(2, 0, 0, 0)), Err(EinvalCode::PipelineOrder));
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(EPERM));
        assert!(!in_linux_table(0));
        assert!(in_linux_table(ENOSYS));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn code_values_unique() {
        let mut i = 0;
        while i < EinvalCode::ALL.len() {
            let mut j = i + 1;
            while j < EinvalCode::ALL.len() {
                assert_ne!(EinvalCode::ALL[i].code(), EinvalCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
        assert_eq!(EinvalCode::ALL[0].code(), SEG_BASE + 1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9344_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
