//! UNX-F9332 sched_setscheduler 非法策略 EINVAL 矩阵 · 0x7C080段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9332 逐字）：
//!   UNX-F9332-J1 非法策略注入判据过，全 EINVAL 格过，非法 param 全 EINVAL
//!   格过，A1 消费声明格过。
//!
//! 设计要点：
//!   ① 策略字只承认低 4 位与 SCHED_RESET_ON_FORK 一位：`base_of` 先用
//!      `policy & !(RESET_ON_FORK | 0xF)` 摘出未知位，再在 6 行策略表里查
//!      base。4 与 7 是 Linux 的策略空洞，高位垃圾（0x2000_0000/0x8000_0000）
//!      同属 PolicyGarbage，绝不做「就近取整」。
//!   ② 参数门按策略三分：实时族（FIFO/RR）只认 1..=99；普通族
//!      （OTHER/BATCH/IDLE）只认 0；DEADLINE 要求 priority==0、runtime>0、
//!      deadline>=runtime、period 为 0 或 >=deadline，且全部不超过 2^40 ns；
//!      sched_flags 必须为 0。任何越界都是 EINVAL。
//!   ③ 参数门先于任务查找与权限判定：坏策略/坏 param 不得先去解引用 pid，
//!      否则返回码会随任务表内容漂移（顺序判据的可观测落点）。
//!   ④ 权限面：非 root 只能改自己的任务，且只能落非实时/非 DEADLINE 策略，
//!      其余 EPERM；DEADLINE 另设 4 槽准入容量，第 5 个任务 EAGAIN。
//!   ⑤ 判定次序冻结：pid<0 → 策略 → 空 param → param 值域 → 查找 → 权限 →
//!      准入。失败路径不得写任务策略，也不得消耗 DEADLINE 槽位。
//!   ⑥ 注入账：非法策略 10×10、非法 param 10×10、空 param 6×10，全部满足
//!      injected == detected + escaped 且 escaped == 0、账目单调不回退。
//!   ⑦ A1 消费声明：A1 只读消费本模块 6 行策略表，rows 必须等于策略表长度，
//!      write_back 恒 false；声明一旦漂移即红。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9332 独占 0x7C080..=0x7C0BF）。
pub const SEG_BASE: u32 = 0x7C080;

pub const SCHED_OTHER: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_IDLE: u32 = 5;
pub const SCHED_DEADLINE: u32 = 6;
/// reset-on-fork 标志位（可与任何基策略按位或）。
pub const RESET_ON_FORK: u32 = 0x4000_0000;
/// 基策略可见位。
pub const POLICY_LOW_MASK: u32 = 0x0000_000F;

pub const RTPRIO_MIN: i32 = 1;
pub const RTPRIO_MAX: i32 = 99;
/// DEADLINE 三段时长上限（2^40 ns，远小于 u64 溢出点）。
pub const DL_MAX: u64 = 1 << 40;
/// 任务表容量。
pub const TASK_SLOTS: usize = 12;
/// DEADLINE 准入槽容量。
pub const DL_SLOTS: u32 = 4;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 非法策略取值格数。
pub const ILLEGAL_POLICIES: usize = 10;
/// 非法 param 行数。
pub const ILLEGAL_PARAMS: usize = 10;
/// 策略表行数。
pub const POLICY_ROWS: usize = 6;

pub const EPERM: i32 = 1;
pub const ESRCH: i32 = 3;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

/// 本模块诊断码（段内 +0x01..=+0x0E 闭集，共 14 码）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedCode {
    PolicyGarbage,
    ParamRange,
    ParamNull,
    PidNegative,
    TaskMissing,
    NotOwner,
    AdmissionFull,
    OrderDrift,
    MutationLeak,
    LedgerLeak,
    NotReproducible,
    SegmentViolation,
    ConsumeDeclGap,
    TableDrift,
}

impl SchedCode {
    /// 段内序号（0..=13）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值：SEG_BASE + 1 + 序号（两两唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SchedCode::PolicyGarbage => "策略字非法：只承认 0/1/2/3/5/6 与 RESET_ON_FORK，4/7 及高位垃圾一律 EINVAL",
            SchedCode::ParamRange => "param 越界：实时族 1..=99、普通族 0、DEADLINE 三段时长须满足 runtime<=deadline<=period 或 period==0",
            SchedCode::ParamNull => "param 为空：FIFO/RR/DEADLINE 需要可读的 sched_param，空指针回 EFAULT 而非 EINVAL",
            SchedCode::PidNegative => "pid 为负：sched_setscheduler 不接受负 pid，必须在策略校验之前回 EINVAL",
            SchedCode::TaskMissing => "目标不存在：任务表查无此 pid，回 ESRCH；不得误报 EINVAL 掩盖查找失败",
            SchedCode::NotOwner => "权限不足：非 root 只能改自己且不得设实时/DEADLINE，越权回 EPERM",
            SchedCode::AdmissionFull => "DEADLINE 准入已满：4 个槽位全部占用，第 5 个任务回 EAGAIN",
            SchedCode::OrderDrift => "判定次序漂移：策略/param 必须早于查找与权限，否则返回码随表内容变化",
            SchedCode::MutationLeak => "失败调用污染状态：被拒请求不得改写任务策略、不得消耗准入槽",
            SchedCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
            SchedCode::NotReproducible => "双跑不一致：同一语料两次指纹必须相等，隐藏状态即失败",
            SchedCode::SegmentViolation => "段位越界：0x7C080 必须 0x40 对齐且落在本批 0x7C000..=0x7D1C0 窗口",
            SchedCode::ConsumeDeclGap => "A1 消费声明缺失：rows 必须等于策略表长度、write_back 必须为 false",
            SchedCode::TableDrift => "策略表漂移：needs_param/rt/deadline/优先级带必须与冻结账逐行一致",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: &'static [SchedCode] = &[
        SchedCode::PolicyGarbage, SchedCode::ParamRange, SchedCode::ParamNull,
        SchedCode::PidNegative, SchedCode::TaskMissing, SchedCode::NotOwner,
        SchedCode::AdmissionFull, SchedCode::OrderDrift, SchedCode::MutationLeak,
        SchedCode::LedgerLeak, SchedCode::NotReproducible, SchedCode::SegmentViolation,
        SchedCode::ConsumeDeclGap, SchedCode::TableDrift,
    ];
}
/// 单行策略规格：判定所需的全部字段都在这里，不另立影子开关。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PolicySpec {
    pub base: u32,
    pub name: &'static str,
    pub needs_param: bool,
    pub rt: bool,
    pub deadline: bool,
    pub prio_lo: i32,
    pub prio_hi: i32,
}

impl PolicySpec {
    pub const fn new(
        base: u32, name: &'static str, needs_param: bool, rt: bool,
        deadline: bool, prio_lo: i32, prio_hi: i32,
    ) -> PolicySpec {
        PolicySpec { base, name, needs_param, rt, deadline, prio_lo, prio_hi }
    }
}
/// 六行策略表（唯一数据源）。
pub const POLICIES: [PolicySpec; POLICY_ROWS] = [
    PolicySpec::new(SCHED_OTHER, "SCHED_OTHER", false, false, false, 0, 0),
    PolicySpec::new(SCHED_FIFO, "SCHED_FIFO", true, true, false, RTPRIO_MIN, RTPRIO_MAX),
    PolicySpec::new(SCHED_RR, "SCHED_RR", true, true, false, RTPRIO_MIN, RTPRIO_MAX),
    PolicySpec::new(SCHED_BATCH, "SCHED_BATCH", false, false, false, 0, 0),
    PolicySpec::new(SCHED_IDLE, "SCHED_IDLE", false, false, false, 0, 0),
    PolicySpec::new(SCHED_DEADLINE, "SCHED_DEADLINE", true, false, true, 0, 0),
];

/// 冻结账：(base, needs_param, rt, deadline, prio_lo, prio_hi)。
pub const FROZEN_POLICY_ROWS: [(u32, bool, bool, bool, i32, i32); POLICY_ROWS] = [
    (SCHED_OTHER, false, false, false, 0, 0),
    (SCHED_FIFO, true, true, false, RTPRIO_MIN, RTPRIO_MAX),
    (SCHED_RR, true, true, false, RTPRIO_MIN, RTPRIO_MAX),
    (SCHED_BATCH, false, false, false, 0, 0),
    (SCHED_IDLE, false, false, false, 0, 0),
    (SCHED_DEADLINE, true, false, true, 0, 0),
];

/// 从策略字取基值；未知位或 4/7 空洞返回 None。
pub fn base_of(policy: u32) -> Option<u32> {
    if policy & !(RESET_ON_FORK | POLICY_LOW_MASK) != 0 {
        return None;
    }
    let base = policy & POLICY_LOW_MASK;
    if base > SCHED_DEADLINE || base == 4 {
        None
    } else {
        Some(base)
    }
}
/// 按基值查表。
pub fn spec_of(base: u32) -> Option<&'static PolicySpec> {
    let mut i = 0usize;
    while i < POLICIES.len() {
        if let Some(sp) = POLICIES.get(i) {
            if sp.base == base {
                return Some(sp);
            }
        }
        i += 1;
    }
    None
}
/// 策略表漂移格数（六列逐行比对冻结账）。
pub fn table_drift() -> u32 {
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < POLICIES.len() {
        if let (Some(sp), Some(f)) = (POLICIES.get(i), FROZEN_POLICY_ROWS.get(i)) {
            if sp.base != f.0 || sp.needs_param != f.1 || sp.rt != f.2
                || sp.deadline != f.3 || sp.prio_lo != f.4 || sp.prio_hi != f.5
            {
                drift = drift.saturating_add(1);
            }
        }
        i += 1;
    }
    drift
}
/// 策略行的优先级带（闭集外返回 None）。
pub fn priority_band(base: u32) -> Option<(i32, i32)> {
    match spec_of(base) {
        Some(sp) => Some((sp.prio_lo, sp.prio_hi)),
        None => None,
    }
}
/// sched_param 的判定投影（sched_setscheduler 只读这五个字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchedParam {
    pub priority: i32,
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub flags: u32,
}

impl SchedParam {
    pub const fn new(priority: i32, runtime: u64, deadline: u64, period: u64, flags: u32) -> SchedParam {
        SchedParam { priority, runtime, deadline, period, flags }
    }

    /// 普通族最常用的零参。
    pub const fn plain(priority: i32) -> SchedParam {
        SchedParam { priority, runtime: 0, deadline: 0, period: 0, flags: 0 }
    }
}
/// 参数值域判定：返回 Some(EINVAL) 表示该策略不接受这个 param。
pub fn param_err(spec: &PolicySpec, p: &SchedParam) -> Option<i32> {
    if p.flags != 0 {
        return Some(EINVAL);
    }
    if spec.deadline {
        if p.priority != 0 || p.runtime == 0 || p.runtime > DL_MAX {
            return Some(EINVAL);
        }
        if p.deadline < p.runtime || p.deadline > DL_MAX {
            return Some(EINVAL);
        }
        if p.period != 0 && (p.period < p.deadline || p.period > DL_MAX) {
            return Some(EINVAL);
        }
        None
    } else if spec.rt {
        if p.priority < RTPRIO_MIN || p.priority > RTPRIO_MAX {
            Some(EINVAL)
        } else {
            None
        }
    } else if p.priority != 0 {
        Some(EINVAL)
    } else {
        None
    }
}
/// 按基值直接判参数（表外基值视为 EINVAL，测试与检查共用）。
pub fn param_err_base(base: u32, p: &SchedParam) -> Option<i32> {
    match spec_of(base) {
        Some(sp) => param_err(sp, p),
        None => Some(EINVAL),
    }
}
/// 一个可调度任务的最小状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Task {
    pub pid: i32,
    pub uid: u32,
    pub policy: u32,
    pub deadline: bool,
}

impl Task {
    pub const fn new(pid: i32, uid: u32, policy: u32, deadline: bool) -> Task {
        Task { pid, uid, policy, deadline }
    }

    pub const fn empty() -> Task {
        Task { pid: 0, uid: 0, policy: 0, deadline: false }
    }
}


/// 调度域：定容任务表 + 单调计数。
#[derive(Clone, Copy, Debug)]
pub struct SchedState {
    tasks: [Task; TASK_SLOTS],
    count: usize,
    dl_used: u32,
    accepts: u32,
    rejects: u32,
    last_errno: i32,
}

impl SchedState {
    pub const fn new() -> SchedState {
        SchedState {
            tasks: [Task::empty(); TASK_SLOTS], count: 0,
            dl_used: 0, accepts: 0, rejects: 0, last_errno: 0,
        }
    }

    /// 六个固定任务：root 两个、用户 1000 三个、用户 1001 一个。
    pub fn seed() -> SchedState {
        let mut st = SchedState::new();
        let rows: [Task; 6] = [
            Task::new(1, 0, SCHED_OTHER, false),
            Task::new(100, 1000, SCHED_OTHER, false),
            Task::new(101, 1000, SCHED_OTHER, false),
            Task::new(102, 1001, SCHED_OTHER, false),
            Task::new(103, 0, SCHED_OTHER, false),
            Task::new(104, 1000, SCHED_OTHER, false),
        ];
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(t) = rows.get(i) {
                if let Some(dst) = st.tasks.get_mut(i) {
                    *dst = *t;
                }
            }
            i += 1;
        }
        st.count = rows.len();
        st
    }

    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    /// 拒绝计数（只增）。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn dl_used(&self) -> u32 {
        self.dl_used
    }
    pub fn last_errno(&self) -> i32 {
        self.last_errno
    }

    /// 按 pid 找槽。
    pub fn task_index(&self, pid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(t) = self.tasks.get(i) {
                if t.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn policy_of(&self, pid: i32) -> Option<u32> {
        match self.task_index(pid) {
            Some(i) => match self.tasks.get(i) {
                Some(t) => Some(t.policy),
                None => None,
            },
            None => None,
        }
    }

    /// 记录一次拒绝（单调计数，不触碰任务表）。
    pub fn reject(&mut self, e: i32) -> Result<usize, i32> {
        self.rejects = self.rejects.saturating_add(1);
        self.last_errno = e;
        Err(e)
    }

    /// sched_setscheduler 判定模型。成功返回槽下标。
    pub fn sched_setscheduler(
        &mut self, caller_uid: u32, pid: i32, policy: u32, param: Option<&SchedParam>,
    ) -> Result<usize, i32> {
        // ① pid 符号
        if pid < 0 {
            return self.reject(EINVAL);
        }
        // ② 策略字
        let base = match base_of(policy) {
            Some(b) => b,
            None => return self.reject(EINVAL),
        };
        let spec = match spec_of(base) {
            Some(sp) => sp,
            None => return self.reject(EINVAL),
        };
        // ③ 空 param（仅需要 param 的策略）
        if spec.needs_param && param.is_none() {
            return self.reject(EFAULT);
        }
        // ④ param 值域
        if let Some(p) = param {
            if let Some(code) = param_err(spec, p) {
                return self.reject(code);
            }
        }
        // ⑤ 查找
        let idx = match self.task_index(pid) {
            Some(i) => i,
            None => return self.reject(ESRCH),
        };
        // 先取快照再改，避免 &mut self 与字段借用冲突
        let (target_uid, was_dl) = match self.tasks.get(idx) {
            Some(t) => (t.uid, t.deadline),
            None => (0u32, false),
        };
        // ⑥ 权限
        if caller_uid != 0 && (target_uid != caller_uid || spec.rt || spec.deadline) {
            return self.reject(EPERM);
        }
        // ⑦ 准入容量
        let add_dl = spec.deadline && !was_dl;
        if add_dl && self.dl_used >= DL_SLOTS {
            return self.reject(EAGAIN);
        }
        let new_deadline = spec.deadline;
        if let Some(dst) = self.tasks.get_mut(idx) {
            dst.policy = base;
            dst.deadline = new_deadline;
        }
        if add_dl {
            self.dl_used = self.dl_used.saturating_add(1);
        }
        self.accepts = self.accepts.saturating_add(1);
        self.last_errno = 0;
        Ok(idx)
    }

    /// 无副作用的失败分类（判定次序与 sched_setscheduler 完全一致）。
    pub fn classify_request(
        &self, caller_uid: u32, pid: i32, policy: u32, param: Option<&SchedParam>,
    ) -> Option<SchedCode> {
        if pid < 0 {
            return Some(SchedCode::PidNegative);
        }
        let base = match base_of(policy) {
            Some(b) => b,
            None => return Some(SchedCode::PolicyGarbage),
        };
        let spec = match spec_of(base) {
            Some(sp) => sp,
            None => return Some(SchedCode::TableDrift),
        };
        if spec.needs_param && param.is_none() {
            return Some(SchedCode::ParamNull);
        }
        if let Some(p) = param {
            if param_err(spec, p).is_some() {
                return Some(SchedCode::ParamRange);
            }
        }
        let idx = match self.task_index(pid) {
            Some(i) => i,
            None => return Some(SchedCode::TaskMissing),
        };
        let (target_uid, was_dl) = match self.tasks.get(idx) {
            Some(t) => (t.uid, t.deadline),
            None => (0u32, false),
        };
        if caller_uid != 0 && (target_uid != caller_uid || spec.rt || spec.deadline) {
            return Some(SchedCode::NotOwner);
        }
        if spec.deadline && !was_dl && self.dl_used >= DL_SLOTS {
            return Some(SchedCode::AdmissionFull);
        }
        None
    }
}
/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
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

    /// 记一笔；返回账目是否仍闭合。
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
/// 十格非法策略字（4/7 空洞、未知低半字节、高位垃圾）。
pub const ILLEGAL_POLICY_VALUES: [u32; ILLEGAL_POLICIES] = [
    4, 7, 8, 9, 0x10, 0x1F, 0x2000_0000, 0x8000_0000, 0xFFFF_FFFF, RESET_ON_FORK | 7,
];

/// 非法 param 网格行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamRow {
    pub policy: u32,
    pub priority: i32,
    pub runtime: u64,
    pub deadline: u64,
    pub period: u64,
    pub flags: u32,
}

impl ParamRow {
    pub const fn new(
        policy: u32, priority: i32, runtime: u64, deadline: u64, period: u64, flags: u32,
    ) -> ParamRow {
        ParamRow { policy, priority, runtime, deadline, period, flags }
    }

    pub fn param(&self) -> SchedParam {
        SchedParam::new(self.priority, self.runtime, self.deadline, self.period, self.flags)
    }
}
/// 十行必须 EINVAL 的 param 网格。
pub const ILLEGAL_PARAM_GRID: [ParamRow; ILLEGAL_PARAMS] = [
    ParamRow::new(SCHED_FIFO, 0, 0, 0, 0, 0),
    ParamRow::new(SCHED_FIFO, 100, 0, 0, 0, 0),
    ParamRow::new(SCHED_FIFO, -1, 0, 0, 0, 0),
    ParamRow::new(SCHED_RR, 0, 0, 0, 0, 0),
    ParamRow::new(SCHED_OTHER, 1, 0, 0, 0, 0),
    ParamRow::new(SCHED_BATCH, 5, 0, 0, 0, 0),
    ParamRow::new(SCHED_IDLE, 1, 0, 0, 0, 0),
    ParamRow::new(SCHED_DEADLINE, 0, 0, 2000, 0, 0),
    ParamRow::new(SCHED_DEADLINE, 0, 1000, 500, 0, 0),
    ParamRow::new(SCHED_OTHER, 0, 0, 0, 0, 1),
];

/// 非法策略注入：10 值 × 10 轮，全部必须 EINVAL。
pub fn run_policy_injection() -> (u32, InjectLedger, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut drift = 0u32;
    let probe = SchedParam::plain(0);
    let mut i = 0usize;
    while i < ILLEGAL_POLICY_VALUES.len() {
        if let Some(p) = ILLEGAL_POLICY_VALUES.get(i).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut st = SchedState::seed();
                let caught = st.sched_setscheduler(0, 100, p, Some(&probe)) == Err(EINVAL);
                if caught {
                    hits = hits.saturating_add(1);
                }
                if !ledger.record(caught) {
                    drift = drift.saturating_add(1);
                }
                round += 1;
            }
        }
        i += 1;
    }
    (hits, ledger, drift)
}
/// 非法 param 注入：10 行 × 10 轮，全部必须 EINVAL。
pub fn run_param_injection() -> (u32, InjectLedger, u32) {
    let mut ledger = InjectLedger::zero();
    let mut hits = 0u32;
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < ILLEGAL_PARAM_GRID.len() {
        if let Some(row) = ILLEGAL_PARAM_GRID.get(i) {
            let param = row.param();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut st = SchedState::seed();
                let caught = st.sched_setscheduler(0, 100, row.policy, Some(&param)) == Err(EINVAL);
                if caught {
                    hits = hits.saturating_add(1);
                }
                if !ledger.record(caught) {
                    drift = drift.saturating_add(1);
                }
                round += 1;
            }
        }
        i += 1;
    }
    (hits, ledger, drift)
}
/// 空 param 矩阵：6 策略 × 10 轮；需要 param 的回 EFAULT，其余必须成功。
pub fn run_null_param_matrix() -> (u32, u32, InjectLedger, u32) {
    let mut ledger = InjectLedger::zero();
    let mut faults = 0u32;
    let mut oks = 0u32;
    let mut drift = 0u32;
    let mut i = 0usize;
    while i < POLICIES.len() {
        if let Some(spec) = POLICIES.get(i) {
            let expect_fault = spec.needs_param;
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let mut st = SchedState::seed();
                let got = st.sched_setscheduler(0, 100, spec.base, None);
                let caught = if expect_fault { got == Err(EFAULT) } else { got.is_ok() };
                if caught {
                    if expect_fault {
                        faults = faults.saturating_add(1);
                    } else {
                        oks = oks.saturating_add(1);
                    }
                }
                if !ledger.record(caught) {
                    drift = drift.saturating_add(1);
                }
                round += 1;
            }
        }
        i += 1;
    }
    (faults, oks, ledger, drift)
}
/// 消费声明：A1 只读消费本模块策略表，禁止回写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeDecl {
    pub card: &'static str,
    pub producer: &'static str,
    pub rows: u32,
    pub write_back: bool,
    pub epoch: u32,
}
/// A1 对本模块的只读消费声明。
pub const A1_CONSUME: ConsumeDecl = ConsumeDecl {
    card: "A1", producer: "unx9332", rows: POLICY_ROWS as u32, write_back: false, epoch: 1,
};

/// 声明自洽：消费方/生产方/行数/写回/世代五项同时成立。
pub fn consume_decl_ok() -> bool {
    A1_CONSUME.card == "A1"
        && A1_CONSUME.producer == "unx9332"
        && A1_CONSUME.rows == POLICIES.len() as u32
        && !A1_CONSUME.write_back
        && A1_CONSUME.epoch >= 1
        && !A1_CONSUME.producer.is_empty()
}
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；权限不足"),
    ("ESRCH", -3, 1168, "Windows ERROR_NOT_FOUND；目标不存在"),
    ("EAGAIN", -11, 1237, "Windows ERROR_RETRY；准入失败可重试"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；param 指针不可读"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；策略或 param 非法"),
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

pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}
/// FNV-1a 32 位。
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
/// 编码一次调度判决。
fn encode(out: &mut Vec<u8>, r: Result<usize, i32>) {
    match r {
        Ok(v) => {
            out.push(1);
            out.push((v & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}
/// 规范语料双跑指纹：正常、越权、坏策略、坏 param、空 param、准入满。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut st = SchedState::seed();
    encode(&mut bytes, st.sched_setscheduler(0, 100, SCHED_OTHER, None));
    encode(&mut bytes, st.sched_setscheduler(0, 100, SCHED_FIFO, Some(&SchedParam::plain(50))));
    encode(&mut bytes, st.sched_setscheduler(1000, 101, SCHED_FIFO, Some(&SchedParam::plain(50))));
    encode(&mut bytes, st.sched_setscheduler(0, 101, SCHED_RR, Some(&SchedParam::plain(99))));
    encode(&mut bytes, st.sched_setscheduler(0, 102, SCHED_DEADLINE, Some(&SchedParam::new(0, 1000, 2000, 0, 0))));
    encode(&mut bytes, st.sched_setscheduler(0, 102, SCHED_DEADLINE, Some(&SchedParam::new(0, 1000, 500, 0, 0))));
    encode(&mut bytes, st.sched_setscheduler(0, 103, SCHED_BATCH, None));
    encode(&mut bytes, st.sched_setscheduler(0, 104, 4, None));
    encode(&mut bytes, st.sched_setscheduler(0, 9999, SCHED_OTHER, None));
    encode(&mut bytes, st.sched_setscheduler(0, -1, SCHED_OTHER, None));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9332 判据集。
pub fn run_unx9332_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9332");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7C080 独占且 0x40 对齐", seg_ok,
          "段基址必须落在本批窗口且与邻近卡不重叠");
    let mut st = SchedState::seed();
    let normal = st.sched_setscheduler(0, 100, SCHED_FIFO, Some(&SchedParam::plain(50)));
    s.add("正常流：root 给用户任务设 FIFO 成功",
          normal.is_ok() && st.policy_of(100) == Some(SCHED_FIFO) && st.accepts() == 1,
          "合法策略 + 合法 param 必须落到目标槽并改写策略");
    let (p_hits, p_ledger, p_drift) = run_policy_injection();
    s.add("非法策略：10 值 × 10 轮全 EINVAL",
          p_hits == ILLEGAL_POLICIES as u32 * INJECT_ROUNDS && p_drift == 0,
          "4/7 空洞与高位垃圾都必须在策略门短路");

    let mut pol_class = true;
    let mut i = 0usize;
    while i < ILLEGAL_POLICY_VALUES.len() {
        if let Some(p) = ILLEGAL_POLICY_VALUES.get(i) {
            if SchedState::seed().classify_request(0, 100, *p, None) != Some(SchedCode::PolicyGarbage) {
                pol_class = false;
            }
        }
        i += 1;
    }
    s.add("非法策略分类：十格全部归 PolicyGarbage", pol_class,
          "分类器必须与返回码一致，不能落到 ParamRange 或 TaskMissing");

    let (q_hits, q_ledger, q_drift) = run_param_injection();
    s.add("非法 param：10 行 × 10 轮全 EINVAL",
          q_hits == ILLEGAL_PARAMS as u32 * INJECT_ROUNDS && q_drift == 0,
          "优先级带、DEADLINE 三段顺序、flags 非零都必须 EINVAL");

    let mut q_class = true;
    let mut j = 0usize;
    while j < ILLEGAL_PARAM_GRID.len() {
        if let Some(row) = ILLEGAL_PARAM_GRID.get(j) {
            let param = row.param();
            if SchedState::seed().classify_request(0, 100, row.policy, Some(&param)) != Some(SchedCode::ParamRange) {
                q_class = false;
            }
        }
        j += 1;
    }
    s.add("非法 param 分类：十行全部归 ParamRange", q_class,
          "参数门必须早于权限与准入，分类不得漂到 NotOwner");

    let (nf, no, n_ledger, n_drift) = run_null_param_matrix();
    s.add("空 param：需参策略全 EFAULT、普通族全成功",
          nf == 3 * INJECT_ROUNDS && no == 3 * INJECT_ROUNDS && n_drift == 0,
          "FIFO/RR/DEADLINE 空指针 EFAULT；OTHER/BATCH/IDLE 忽略 param");

    s.add("空 param 账：60 次注入全检出",
          n_ledger.conserved() && n_ledger.escaped == 0
              && n_ledger.injected == POLICY_ROWS as u32 * INJECT_ROUNDS,
          "6 策略 × 10 轮必须一笔不漏，逃逸恒 0");

    let neg_ok = SchedState::seed().sched_setscheduler(0, -1, SCHED_OTHER, None) == Err(EINVAL)
        && SchedState::seed().classify_request(0, -9, SCHED_OTHER, None) == Some(SchedCode::PidNegative);
    s.add("负 pid：先于策略判定回 EINVAL", neg_ok,
          "pid<0 是参数面硬错，不得进入任务查找");

    let missing_ok = SchedState::seed().sched_setscheduler(0, 9999, SCHED_OTHER, None) == Err(ESRCH)
        && SchedState::seed().classify_request(0, 9999, SCHED_OTHER, None) == Some(SchedCode::TaskMissing);
    s.add("不存在 pid：查表失败回 ESRCH", missing_ok,
          "不得把查找失败折叠成 EINVAL");

    let mut perm = SchedState::seed();
    let cross = perm.sched_setscheduler(1000, 102, SCHED_OTHER, None) == Err(EPERM);
    let rt = perm.sched_setscheduler(1000, 100, SCHED_FIFO, Some(&SchedParam::plain(10))) == Err(EPERM);
    s.add("权限面：跨 uid 与普通用户设实时均 EPERM", cross && rt && perm.rejects() == 2,
          "非 root 只能在自身任务上落普通策略");

    let mut adm = SchedState::seed();
    let p = SchedParam::new(0, 1000, 2000, 0, 0);
    let pids = [100u32, 101, 102, 103];
    let mut ok_count = 0u32;
    let mut k = 0usize;
    while k < pids.len() {
        if let Some(pid) = pids.get(k).copied() {
            if adm.sched_setscheduler(0, pid as i32, SCHED_DEADLINE, Some(&p)).is_ok() {
                ok_count = ok_count.saturating_add(1);
            }
        }
        k += 1;
    }
    let fifth = adm.sched_setscheduler(0, 104, SCHED_DEADLINE, Some(&p));
    s.add("准入容量：4 槽填满后第 5 个 EAGAIN",
          ok_count == DL_SLOTS && fifth == Err(EAGAIN) && adm.dl_used() == DL_SLOTS,
          "DEADLINE 槽位定容，满额必须显性回 EAGAIN");

    let mut leak = SchedState::seed();
    let denied = leak.sched_setscheduler(1000, 100, SCHED_FIFO, Some(&SchedParam::plain(20)));
    let leak_ok = denied == Err(EPERM) && leak.policy_of(100) == Some(SCHED_OTHER)
        && leak.accepts() == 0 && leak.rejects() == 1 && leak.dl_used() == 0;
    s.add("状态机：失败调用不改策略、不占槽", leak_ok,
          "被拒请求只允许把 rejects 加一，其余状态必须原样");

    let order_ok = SchedState::seed().sched_setscheduler(0, 9999, 0xFFFF_FFFF, None) == Err(EINVAL)
        && SchedState::seed().sched_setscheduler(0, 9999, SCHED_FIFO, None) == Err(EFAULT)
        && SchedState::seed().sched_setscheduler(0, 9999, SCHED_OTHER, None) == Err(ESRCH);
    s.add("判定次序：策略 → param → 查找", order_ok,
          "同一缺失 pid 随前置门变化返回 EINVAL/EFAULT/ESRCH，证明顺序冻结");

    s.add("表冻结：六行策略规格零漂移",
          table_drift() == 0 && priority_band(SCHED_FIFO) == Some((RTPRIO_MIN, RTPRIO_MAX)),
          "needs_param/rt/deadline/优先级带任一行改动都必须在此暴露");

    s.add("A1 消费声明：只读、行数对齐策略表", consume_decl_ok(),
          "消费方 A1 只能读 6 行策略表，禁止回写与缺行");

    let codes_ok = agrees_with_linux(EINVAL) && agrees_with_linux(EFAULT)
        && agrees_with_linux(EPERM) && agrees_with_linux(ESRCH)
        && agrees_with_linux(EAGAIN) && linux_neg(EINVAL) == -22 && !agrees_with_linux(0);
    s.add("Linux 逐码：五码在表内可复算", codes_ok,
          "返回码必须是 errno 表内正值，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：调度语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：14 个诊断码均有中文建议",
          SchedCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut a = 0usize;
    while a < SchedCode::ALL.len() {
        let mut b = a + 1;
        while b < SchedCode::ALL.len() {
            if let (Some(x), Some(y)) = (SchedCode::ALL.get(a), SchedCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add("诊断码：14 个码值两两唯一", uniq && SchedCode::ALL.len() == 14,
          "段内闭集码不得重复，否则判据无法定位");

    let policy_text = format!(
        "策略注入 {}/{} 检出，账 {}/{}/{}", p_hits,
        ILLEGAL_POLICIES as u32 * INJECT_ROUNDS,
        p_ledger.injected, p_ledger.detected, p_ledger.escaped
    );
    s.add_dyn("策略注入账", p_ledger.conserved(), &policy_text);

    let param_text = format!(
        "param 注入 {}/{}，空参 EFAULT {}/OK {}", q_hits,
        ILLEGAL_PARAMS as u32 * INJECT_ROUNDS, nf, no
    );
    s.add_dyn("param 注入账", q_ledger.conserved() && n_ledger.conserved(), &param_text);

    let fp_text = format!("调度语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_bits_are_closed() {
        assert_eq!(base_of(SCHED_OTHER), Some(0));
        assert_eq!(base_of(SCHED_FIFO), Some(1));
        assert_eq!(base_of(SCHED_DEADLINE | RESET_ON_FORK), Some(6));
        assert_eq!(base_of(4), None);
        assert_eq!(base_of(7), None);
        assert_eq!(base_of(0x8000_0000), None);
        assert_eq!(base_of(0x10), None);
    }
    #[test]
    fn param_priority_windows() {
        assert_eq!(param_err_base(SCHED_FIFO, &SchedParam::plain(0)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_FIFO, &SchedParam::plain(1)), None);
        assert_eq!(param_err_base(SCHED_FIFO, &SchedParam::plain(99)), None);
        assert_eq!(param_err_base(SCHED_FIFO, &SchedParam::plain(100)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_OTHER, &SchedParam::plain(1)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_IDLE, &SchedParam::plain(0)), None);
    }
    #[test]
    fn deadline_field_ordering() {
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(0, 1000, 2000, 0, 0)), None);
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(0, 0, 2000, 0, 0)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(0, 1000, 500, 0, 0)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(0, 1000, 2000, 500, 0)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(1, 1000, 2000, 0, 0)), Some(EINVAL));
        assert_eq!(param_err_base(SCHED_DEADLINE, &SchedParam::new(0, 1000, 2000, 3000, 0)), None);
    }
    #[test]
    fn null_param_split_by_policy() {
        let mut st = SchedState::seed();
        assert_eq!(st.sched_setscheduler(0, 100, SCHED_FIFO, None), Err(EFAULT));
        assert_eq!(st.sched_setscheduler(0, 100, SCHED_OTHER, None).is_ok(), true);
        assert_eq!(st.sched_setscheduler(0, 100, SCHED_DEADLINE, None), Err(EFAULT));
    }
    #[test]
    fn negative_pid_and_lookup() {
        let mut st = SchedState::seed();
        assert_eq!(st.sched_setscheduler(0, -1, SCHED_OTHER, None), Err(EINVAL));
        assert_eq!(st.sched_setscheduler(0, 9999, SCHED_OTHER, None), Err(ESRCH));
        assert_eq!(st.last_errno(), ESRCH);
    }
    #[test]
    fn permissions_and_mutation() {
        let mut st = SchedState::seed();
        assert_eq!(st.sched_setscheduler(1000, 102, SCHED_OTHER, None), Err(EPERM));
        assert_eq!(st.sched_setscheduler(1000, 100, SCHED_FIFO, Some(&SchedParam::plain(10))), Err(EPERM));
        assert_eq!(st.policy_of(100), Some(SCHED_OTHER));
        assert_eq!(st.accepts(), 0);
        assert_eq!(st.sched_setscheduler(0, 100, SCHED_FIFO, Some(&SchedParam::plain(10))).is_ok(), true);
        assert_eq!(st.policy_of(100), Some(SCHED_FIFO));
        assert_eq!(st.accepts(), 1);
    }
    #[test]
    fn deadline_admission_capacity() {
        let mut st = SchedState::seed();
        let p = SchedParam::new(0, 1000, 2000, 0, 0);
        assert_eq!(st.sched_setscheduler(0, 100, SCHED_DEADLINE, Some(&p)).is_ok(), true);
        assert_eq!(st.sched_setscheduler(0, 101, SCHED_DEADLINE, Some(&p)).is_ok(), true);
        assert_eq!(st.sched_setscheduler(0, 102, SCHED_DEADLINE, Some(&p)).is_ok(), true);
        assert_eq!(st.sched_setscheduler(0, 103, SCHED_DEADLINE, Some(&p)).is_ok(), true);
        assert_eq!(st.sched_setscheduler(0, 104, SCHED_DEADLINE, Some(&p)), Err(EAGAIN));
        assert_eq!(st.dl_used(), DL_SLOTS);
    }
    #[test]
    fn injection_ledgers_close() {
        let (hits, led, drift) = run_policy_injection();
        assert_eq!(hits, 100);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        assert_eq!(drift, 0);
        let (qhits, qled, qdrift) = run_param_injection();
        assert_eq!(qhits, 100);
        assert_eq!(qled.conserved(), true);
        assert_eq!(qdrift, 0);
    }
    #[test]
    fn ordering_is_frozen() {
        assert_eq!(SchedState::seed().sched_setscheduler(0, 9999, 0xFFFF_FFFF, None), Err(EINVAL));
        assert_eq!(SchedState::seed().sched_setscheduler(0, 9999, SCHED_FIFO, None), Err(EFAULT));
        assert_eq!(SchedState::seed().sched_setscheduler(0, 9999, SCHED_OTHER, None), Err(ESRCH));
    }
    #[test]
    fn consumer_decl_and_fingerprint() {
        assert_eq!(consume_decl_ok(), true);
        assert_eq!(table_drift(), 0);
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9332_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
