//! UNX-F9304 nanosleep EINTR remaining 回写矩阵（联 B20） · 0x7B980段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9304 逐字）：
//!   UNX-F9304-J1 中断注入 remaining 回写判据过，剩余量逐 ns 可观测格过，与 B20 休眠族对账一致格过。
//!
//! 设计要点：
//!   ① 时间基元：TimeSpec(秒, 纳秒) 先过闸再折成 u64 纳秒；sec<0、nsec<0、nsec>=1e9 一律 EINVAL。
//!   ② 时间线：一次原始请求视为只前进的游标，注入点即段终点；每段"已耗 = 段前剩余"。
//!   ③ 剩量恒等：任何一步满足 各段已耗之和 + 剩余 == 原始总量，剩余量逐 ns 可规格化回代。
//!   ④ 回写纪律：仅 nanosleep + 被中断 + rem 指针有效 才写 *rem；clock_nanosleep 全档位不写。
//!   ⑤ 次序：参数面 EINVAL 先于号面 ENOSYS 先于回写面 EFAULT；EFAULT 只落在中断回写点。
//!   ⑥ B20 对账：8 行休眠族核对 (写 rem, 直接返回, 绝对时钟) 三面掩码，逐行冻结。
//!   ⑦ 注入账本：20 例 × 10 轮，injected == detected + escaped 且 escaped 恒 0。
//!   ⑧ 指纹：FNV-1a 32 位喂入每例 (总量, 已耗, 剩余, errno, 写旗标) 五字节，双跑相等。
//! ─────────────────────────────────────────────────────────────
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9304 独占 0x7B980..=0x7B9BF）。
pub const SEG_BASE: u32 = 0x7B980;
/// 一秒的纳秒数（唯一换算常数，只做常数除模）。
pub const NS_PER_SEC: u64 = 1_000_000_000;

/// Linux errno（正数口径）。
pub const EINTR: i32 = 4;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

/// clock_nanosleep 的绝对时刻标志。
pub const TIMER_ABSTIME: u32 = 1;
/// 每例复算轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 休眠族行数（B20 对账面）。
pub const FAMILY_ROWS: usize = 8;
/// 单次请求最多注入点数（对应调用方最多重试次数）。
pub const MAX_INJECT: usize = 3;
/// 单条时间线最多记录的段数（中断段 + 收尾段）。
pub const MAX_STEPS: usize = MAX_INJECT + 1;
/// 矩阵总例数（8 行 × 2 形态 + 4 特殊例）。
pub const CASE_ROWS: usize = 20;
/// 基准休眠量（1 秒），行总量用 2 的幂放大。
pub const BASE_NS: u64 = 1_000_000_000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepCode {
    SecNegative,
    NsecRange,
    RemNotNormalized,
    RemOverTotal,
    RemDrift,
    RemWrittenOnClock,
    RemWrittenOnAbs,
    RemWrittenOnSuccess,
    FaultTooEarly,
    OrderViolation,
    LedgerEscaped,
    SegmentViolation,
    NotReproducible,
}

impl SleepCode {
    /// 段内序号（repr(u16) 保证与判别值一致）。
    pub fn code(self) -> u16 { self as u16 }

    /// 段内唯一地址 = 段基址 + 1 + 序号（0x40 窗口内）。
    pub fn addr(self) -> u32 { SEG_BASE + 1 + self.code() as u32 }

    /// 中文诊断与可操作建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SleepCode::SecNegative => "秒字段为负：tv_sec<0 必须 EINVAL，休眠时长不存在负值",
            SleepCode::NsecRange => "纳秒越界：tv_nsec 必须落在 0..=999999999，1e9 不是合法纳秒",
            SleepCode::RemNotNormalized => "剩余量未规格化：回写 rem 前必须折成 sec/nsec 且 nsec<1e9",
            SleepCode::RemOverTotal => "剩余量超过总量：剩余 = 总量 - 已耗，单调不增是硬不变量",
            SleepCode::RemDrift => "剩余量漂移：逐 ns 比对失败，注入点与游标存在隐藏偏移",
            SleepCode::RemWrittenOnClock => "clock_nanosleep 回写了 rem：内核该接口从不写剩余量，任何档位都不许",
            SleepCode::RemWrittenOnAbs => "TIMER_ABSTIME 回写了 rem：绝对时刻请求的剩余量无意义，禁止回写",
            SleepCode::RemWrittenOnSuccess => "成功返回仍写 rem：只有被中断那一刻才写，睡满返回不得触碰用户缓冲区",
            SleepCode::FaultTooEarly => "EFAULT 出现过早：剩余量指针只在中断回写点解引用，参数校验阶段不得触碰",
            SleepCode::OrderViolation => "判定次序违规：EINVAL(参数面) 必须先于 ENOSYS(号面) 与 EFAULT(回写面)",
            SleepCode::LedgerEscaped => "注入账逃逸：injected 必须等于 detected 加 escaped 且 escaped 为 0",
            SleepCode::SegmentViolation => "段位越界：段基址必须为 0x7B980 且 0x40 对齐，不得侵占相邻卡段",
            SleepCode::NotReproducible => "双跑不一致：同一语料两次指纹不同，存在隐藏状态或非确定遍历",
        }
    }

    /// 闭集全量（文案完整性与码值唯一性检查用）。
    pub const ALL: &'static [SleepCode] = &[
        SleepCode::SecNegative, SleepCode::NsecRange,
        SleepCode::RemNotNormalized, SleepCode::RemOverTotal,
        SleepCode::RemDrift, SleepCode::RemWrittenOnClock,
        SleepCode::RemWrittenOnAbs, SleepCode::RemWrittenOnSuccess,
        SleepCode::FaultTooEarly, SleepCode::OrderViolation,
        SleepCode::LedgerEscaped, SleepCode::SegmentViolation,
        SleepCode::NotReproducible,
    ];

    /// 地址是否落在本卡 0x40 窗口内。
    pub fn in_window(self) -> bool {
        let v = self.addr();
        v > SEG_BASE && v <= SEG_BASE + 0x3F
    }
}

// ── 时间基元 ──────────────────────────────────────────────────────────
/// 用户态 timespec 的受控表示：只允许非负且 nsec 规范。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeSpec {
    pub sec: i64,
    pub nsec: i64,
}

impl TimeSpec {
    /// 零时长（合法请求，立即返回成功）。
    pub const ZERO: TimeSpec = TimeSpec { sec: 0, nsec: 0 };

    /// 构造（不做校验，校验在 validate）。
    pub const fn new(sec: i64, nsec: i64) -> TimeSpec { TimeSpec { sec, nsec } }

    /// 参数闸：负秒、负纳秒、纳秒越界一律 EINVAL。
    pub fn validate(self) -> Result<(), i32> {
        if self.sec < 0 {
            return Err(EINVAL);
        }
        if self.nsec < 0 || self.nsec >= NS_PER_SEC as i64 {
            return Err(EINVAL);
        }
        Ok(())
    }

    /// 折成纳秒；不合法或溢出返回 None（不 panic，不饱和掩盖错误）。
    pub fn to_ns(self) -> Option<u64> {
        if self.validate().is_err() {
            return None;
        }
        let base = match (self.sec as u64).checked_mul(NS_PER_SEC) {
            Some(v) => v,
            None => return None,
        };
        base.checked_add(self.nsec as u64)
    }

    /// 由纳秒折回规范 timespec（nsec 恒 < 1e9）。
    pub fn from_ns(ns: u64) -> TimeSpec {
        TimeSpec { sec: (ns / NS_PER_SEC) as i64, nsec: (ns % NS_PER_SEC) as i64 }
    }
}

/// 时钟号是否被内核承认（0..=4）。
pub fn clock_known(id: u8) -> bool { id <= 4 }

/// 时钟名（越界给问号，禁止裸索引）。
pub const CLOCK_NAMES: [&'static str; 5] = [
    "CLOCK_REALTIME", "CLOCK_MONOTONIC", "CLOCK_BOOTTIME", "CLOCK_TAI", "CLOCK_MONOTONIC_RAW",
];

/// 取时钟名（越界返回 "?"）。
pub fn clock_name(id: u8) -> &'static str {
    match CLOCK_NAMES.get(id as usize) {
        Some(v) => v,
        None => "?",
    }
}

// ── 休眠族（B20 对账面） ──────────────────────────────────────────────
/// 接口形态：nanosleep 走 -1/errno，clock_nanosleep 直接返回 errno。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepApi { Nano, Clock }

impl SleepApi {
    /// 是否直接返回正 errno（clock_nanosleep 族）。
    pub fn direct(self) -> bool { matches!(self, SleepApi::Clock) }
}

/// 一行休眠族：名字、时钟、接口、是否绝对、是否写剩余量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SleepRow {
    pub name: &'static str,
    pub clock: u8,
    pub api: SleepApi,
    pub abs: bool,
    pub writes_rem: bool,
}

/// 八行冻结族（nanosleep + 五种相对 clock + 两种绝对 clock）。
// name / clock / api / abs / writes_rem
pub const SLEEP_FAMILY: [SleepRow; FAMILY_ROWS] = [
    SleepRow { name: "nanosleep", clock: 1, api: SleepApi::Nano, abs: false, writes_rem: true },
    SleepRow { name: "clock_nanosleep(REALTIME)", clock: 0, api: SleepApi::Clock, abs: false, writes_rem: false },
    SleepRow { name: "clock_nanosleep(MONOTONIC)", clock: 1, api: SleepApi::Clock, abs: false, writes_rem: false },
    SleepRow { name: "clock_nanosleep(BOOTTIME)", clock: 2, api: SleepApi::Clock, abs: false, writes_rem: false },
    SleepRow { name: "clock_nanosleep(TAI)", clock: 3, api: SleepApi::Clock, abs: false, writes_rem: false },
    SleepRow { name: "clock_nanosleep(MONOTONIC_RAW)", clock: 4, api: SleepApi::Clock, abs: false, writes_rem: false },
    SleepRow { name: "clock_nanosleep(MONOTONIC,ABSTIME)", clock: 1, api: SleepApi::Clock, abs: true, writes_rem: false },
    SleepRow { name: "clock_nanosleep(REALTIME,ABSTIME)", clock: 0, api: SleepApi::Clock, abs: true, writes_rem: false },
];

/// B20 冻结掩码：bit0 写 rem、bit1 直接返回 errno、bit2 绝对时钟。
pub const B20_FROZEN: [u8; FAMILY_ROWS] = [1, 2, 2, 2, 2, 2, 6, 6];

impl SleepRow {
    /// 三面掩码（与 B20 冻结表逐行比对的对象）。
    pub fn b20_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.writes_rem { m |= 0x1; }
        if self.api.direct() { m |= 0x2; }
        if self.abs { m |= 0x4; }
        m
    }

    /// 行是否自洽：写 rem 只允许出现在 nanosleep 行。
    pub fn self_consistent(&self) -> bool {
        let nano = matches!(self.api, SleepApi::Nano);
        self.writes_rem == nano && clock_known(self.clock) && (!self.abs || self.api.direct())
    }
}

/// 取一行（越界 None）。
pub fn family_row(id: usize) -> Option<SleepRow> {
    SLEEP_FAMILY.get(id).copied()
}

/// 行总量：1s 起按 2 的幂放大（只用移位，不做除法）。
pub fn row_total(id: usize) -> u64 { BASE_NS << (id & 3) }

// ── 注入计划 ──────────────────────────────────────────────────────────
/// 一次请求的注入点集合（严格递增，单位 ns）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plan {
    pub pts: [u64; MAX_INJECT],
    pub n: u8,
}

/// 两种形态：单点（3/4 处）与三点（1/16、1/2、7/8 处），全靠移位。
pub fn plan_for(pattern: usize, total: u64) -> Plan {
    if pattern == 0 {
        Plan { pts: [total - (total >> 2), 0, 0], n: 1 }
    } else {
        Plan { pts: [total >> 4, total >> 1, total - (total >> 3)], n: 3 }
    }
}

// ── 时间线 ────────────────────────────────────────────────────────────
/// 只前进的休眠游标：剩余量由总量与游标差直接给出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timeline {
    pub total_ns: u64,
    pub cursor_ns: u64,
}

impl Timeline {
    /// 从 0 开始计时。
    pub const fn new(total_ns: u64) -> Timeline { Timeline { total_ns, cursor_ns: 0 } }

    /// 当前剩余量（游标超过总量视为 0，绝不回绕）。
    pub fn remaining(&self) -> u64 { self.total_ns.saturating_sub(self.cursor_ns) }

    /// 前进到 at（夹到总量），返回本段耗掉的纳秒数。
    pub fn advance_to(&mut self, at: u64) -> u64 {
        let dst = if at < self.total_ns { at } else { self.total_ns };
        let step = dst.saturating_sub(self.cursor_ns);
        self.cursor_ns = dst;
        step
    }

    /// 一路睡满。
    pub fn complete(&mut self) -> u64 { self.advance_to(self.total_ns) }
}

// ── 单段记录与运行结果 ────────────────────────────────────────────────
/// 段类型：中断段、回写故障段、收尾完成段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepKind { Elapsed, Fault, Completed }

/// 一段：本段耗时、段末剩余量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub kind: StepKind,
    pub consumed: u64,
    pub rem_ns: u64,
}

/// 数组初始化占位（永不作为真实结果对外）。
pub const STEP_INIT: Step = Step { kind: StepKind::Elapsed, consumed: 0, rem_ns: 0 };

/// 一次调用（含调用方重试）的最终结果面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepOutcome { Completed, Interrupted, Faulted, Rejected }

/// 一次请求的完整轨迹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Run {
    pub row: usize,
    pub total_ns: u64,
    pub steps: [Step; MAX_STEPS],
    pub n: u8,
    pub outcome: SleepOutcome,
    pub errno: i32,
    pub ret: i64,
    pub wrote_rem: bool,
    pub direct: bool,
    pub rem_capable: bool,
    pub rem_ptr: bool,
}

/// 追加一段（容量满则丢弃尾部，不越界）。
fn push_step(run: &mut Run, kind: StepKind, consumed: u64, rem_ns: u64) {
    let idx = run.n as usize;
    if let Some(slot) = run.steps.get_mut(idx) {
        *slot = Step { kind, consumed, rem_ns };
        run.n = run.n.saturating_add(1);
    }
}

/// 返回码映射：nanosleep 用 -errno，clock 族直接给 errno 正数。
pub fn map_ret(api: SleepApi, errno: i32) -> i64 {
    if errno == 0 {
        0
    } else if api.direct() {
        errno as i64
    } else {
        -(errno as i64)
    }
}

/// 空壳结果（校验失败时直接返回它，保证不记录任何注入段）。
fn rejected(row_id: usize, row: &SleepRow, errno: i32, rem_ptr: bool) -> Run {
    Run {
        row: row_id,
        total_ns: 0,
        steps: [STEP_INIT; MAX_STEPS],
        n: 0,
        outcome: SleepOutcome::Rejected,
        errno,
        ret: map_ret(row.api, errno),
        wrote_rem: false,
        direct: row.api.direct(),
        rem_capable: row.writes_rem,
        rem_ptr,
    }
}

/// 核心仿真：参数闸 → 号闸 → 逐注入点推进游标 → 回写判定。
pub fn simulate(row: &SleepRow, row_id: usize, spec: TimeSpec, plan: &Plan, rem_ptr: bool) -> Run {
    let total_ns = match spec.to_ns() {
        Some(v) => v,
        None => return rejected(row_id, row, EINVAL, rem_ptr),
    };
    if !clock_known(row.clock) {
        return rejected(row_id, row, ENOSYS, rem_ptr);
    }
    let mut run = Run {
        row: row_id,
        total_ns,
        steps: [STEP_INIT; MAX_STEPS],
        n: 0,
        outcome: SleepOutcome::Completed,
        errno: 0,
        ret: 0,
        wrote_rem: false,
        direct: row.api.direct(),
        rem_capable: row.writes_rem,
        rem_ptr,
    };
    // 零时长请求：内核立即成功返回，绝不写 rem，也不产生注入点。
    if total_ns == 0 {
        return run;
    }
    let mut tl = Timeline::new(total_ns);
    let mut i = 0usize;
    while i < plan.n as usize {
        if run.outcome == SleepOutcome::Faulted {
            break;
        }
        let at = match plan.pts.get(i) {
            Some(v) => *v,
            None => break,
        };
        // 本段应耗时长 = advance_to 返回的游标增量（dst - cursor）；
        // 只有首段它才等于段前剩余量，后续段必须用增量守恒。
        let seg = tl.advance_to(at);
        let rem = tl.remaining();
        if rem == 0 {
            push_step(&mut run, StepKind::Completed, seg, 0);
            run.outcome = SleepOutcome::Completed;
            run.errno = 0;
            run.ret = 0;
            break;
        }
        if row.writes_rem && !rem_ptr {
            // 中断命中但剩余量指针不可写：EFAULT 只在这里发生。
            push_step(&mut run, StepKind::Fault, seg, rem);
            run.outcome = SleepOutcome::Faulted;
            run.errno = EFAULT;
            run.ret = map_ret(row.api, EFAULT);
        } else {
            if row.writes_rem {
                run.wrote_rem = true;
            }
            push_step(&mut run, StepKind::Elapsed, seg, rem);
            run.outcome = SleepOutcome::Interrupted;
            run.errno = EINTR;
            run.ret = map_ret(row.api, EINTR);
        }
        i += 1;
    }
    if run.n == 0 {
        // 没有注入点：调用方一次睡满。
        let seg = tl.complete();
        push_step(&mut run, StepKind::Completed, seg, 0);
        run.outcome = SleepOutcome::Completed;
        run.errno = 0;
        run.ret = 0;
    }
    run
}

// ── 判定面 ────────────────────────────────────────────────────────────
impl Run {
    /// 取第 i 段（越界 None）。
    pub fn step(&self, i: usize) -> Option<Step> { self.steps.get(i).copied() }

    /// 最后一段（空轨迹 None）。
    pub fn last(&self) -> Option<Step> {
        if self.n == 0 {
            return None;
        }
        self.steps.get(self.n as usize - 1).copied()
    }

    /// 各段已耗之和（段长累加，不是游标）。
    pub fn consumed(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.n as usize {
            if let Some(s) = self.steps.get(i) {
                sum = sum.saturating_add(s.consumed);
            }
            i += 1;
        }
        sum
    }

    /// 段末剩余量（最后一段；空轨迹为 0）。
    pub fn final_rem(&self) -> u64 { self.last().map_or(0, |s| s.rem_ns) }

    /// 总量守恒：各段已耗 + 段末剩余 == 原始总量；被拒例必须零段。
    pub fn conserved(&self) -> bool {
        if self.outcome == SleepOutcome::Rejected {
            return self.n == 0;
        }
        self.consumed().checked_add(self.final_rem()) == Some(self.total_ns)
    }

    /// 剩余量逐 ns 可观测：每段末 剩余 == 总量 - 累计已耗，且可规格化回代。
    pub fn rem_exact(&self) -> bool {
        let mut cum = 0u64;
        let mut i = 0usize;
        while i < self.n as usize {
            let s = match self.steps.get(i) {
                Some(v) => *v,
                None => return false,
            };
            cum = cum.saturating_add(s.consumed);
            if s.rem_ns.checked_add(cum) != Some(self.total_ns) {
                return false;
            }
            if TimeSpec::from_ns(s.rem_ns).to_ns() != Some(s.rem_ns) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 剩余量沿重试不增（单调不增是 EINTR 语义的硬约束）。
    pub fn rem_monotone(&self) -> bool {
        let mut prev = self.total_ns;
        let mut i = 0usize;
        while i < self.n as usize {
            match self.steps.get(i) {
                Some(s) => {
                    if s.rem_ns > prev {
                        return false;
                    }
                    prev = s.rem_ns;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 回写纪律：写 rem ⇔ 中断 且 行具备写能力 且 指针有效。
    pub fn writeback_ok(&self) -> bool {
        let expect = self.outcome == SleepOutcome::Interrupted && self.rem_capable && self.rem_ptr;
        self.wrote_rem == expect
    }

    /// 返回风格：成功为 0，失败按接口形态取正 errno 或 -errno。
    pub fn ret_style_ok(&self) -> bool {
        if self.errno == 0 {
            self.ret == 0
        } else if self.direct {
            self.ret == self.errno as i64
        } else {
            self.ret == -(self.errno as i64)
        }
    }

    /// 全部不变量一次过。
    pub fn full_ok(&self) -> bool {
        self.conserved() && self.rem_exact() && self.rem_monotone()
            && self.writeback_ok() && self.ret_style_ok()
    }
}

// ── 矩阵构造 ──────────────────────────────────────────────────────────
/// 构造 20 例：8 行 × 2 注入形态（rem 指针均有效）+ 4 个特殊例。
pub fn matrix() -> Vec<Run> {
    let mut runs = Vec::new();
    let mut r = 0usize;
    while r < FAMILY_ROWS {
        if let Some(row) = family_row(r) {
            let total = row_total(r);
            let spec = TimeSpec::from_ns(total);
            let mut p = 0usize;
            while p < 2 {
                let plan = plan_for(p, total);
                runs.push(simulate(&row, r, spec, &plan, true));
                p += 1;
            }
        }
        r += 1;
    }
    // 特殊例 1：nanosleep 指针不可写 → 中断点 EFAULT（不写、不越界）。
    if let Some(row) = family_row(0) {
        let total = row_total(0);
        let plan = plan_for(0, total);
        runs.push(simulate(&row, 0, TimeSpec::from_ns(total), &plan, false));
    }
    // 特殊例 2：零时长立即成功，不写 rem。
    if let Some(row) = family_row(0) {
        let plan = Plan { pts: [0, 0, 0], n: 0 };
        runs.push(simulate(&row, 0, TimeSpec::ZERO, &plan, true));
    }
    // 特殊例 3：nsec==1e9 非法 → 参数闸先拒，零注入段。
    if let Some(row) = family_row(3) {
        let plan = plan_for(0, row_total(3));
        runs.push(simulate(&row, 3, TimeSpec::new(0, NS_PER_SEC as i64), &plan, true));
    }
    // 特殊例 4：绝对时钟 + 指针不可写 → 仍只回 EINTR，绝不 EFAULT、绝不回写。
    if let Some(row) = family_row(7) {
        let total = row_total(7);
        let plan = plan_for(0, total);
        runs.push(simulate(&row, 7, TimeSpec::from_ns(total), &plan, false));
    }
    runs
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 三计数账本：注入 / 检出 / 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: u32,
}

impl Ledger {
    /// 归零。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0, rounds: 0 } }

    /// 记一次注入结果（不变量成立记检出，否则记逃逸）。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 账本平衡且无逃逸。
    pub fn balanced(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped) && self.escaped == 0
    }
}

/// 审计结果：首轮矩阵 + 十轮账本。
#[derive(Clone, Debug)]
pub struct Audit {
    pub runs: Vec<Run>,
    pub ledger: Ledger,
}

/// 跑满 INJECT_ROUNDS 轮：每轮对全部例复算不变量。
pub fn audit() -> Audit {
    let runs = matrix();
    let mut ledger = Ledger::new();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < runs.len() {
            if let Some(r) = runs.get(i) {
                ledger.record(r.full_ok());
            }
            i += 1;
        }
        ledger.rounds = round.saturating_add(1);
        round = round.saturating_add(1);
    }
    Audit { runs, ledger }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位（自写，不使用标准库哈希）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
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

/// 规范语料：每例 5 字节（总量低字节、已耗低字节、剩余低字节、errno、写旗标）。
pub fn corpus(runs: &[Run]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < runs.len() {
        if let Some(r) = runs.get(i) {
            out.push((r.total_ns & 0xFF) as u8);
            out.push((r.consumed() & 0xFF) as u8);
            out.push((r.final_rem() & 0xFF) as u8);
            out.push((r.errno & 0xFF) as u8);
            out.push(if r.wrote_rem { 1 } else { 0 });
        }
        i += 1;
    }
    out
}

/// 全矩阵指纹（两次独立构造必须相等）。
pub fn corpus_fingerprint() -> u32 { fnv1a(&corpus(&matrix())) }

// ── Linux errno 对照 ──────────────────────────────────────────────────
/// 冻结 errno 表：(名字, 正数值)。
pub const ERRNO_TABLE: [(&'static str, i32); 4] = [
    ("EINTR", EINTR), ("EFAULT", EFAULT), ("EINVAL", EINVAL), ("ENOSYS", ENOSYS),
];

/// 表内码是否与 Linux 数值一致。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some((_, v)) = ERRNO_TABLE.get(i) {
            if *v == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 内核返回口径：失败给负值。
pub fn linux_neg(code: i32) -> i64 { -(code as i64) }

// ── 判据集 ────────────────────────────────────────────────────────────
/// 运行 F9304 全部判据。
pub fn run_unx9304_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9304");

    let seg_ok = SEG_BASE == 0x7B980 && (SEG_BASE & 0x3F) == 0;
    s.add("段位：SEG_BASE=0x7B980 且 0x40 对齐", seg_ok,
          "段基址唯一占用 0x7B980..=0x7B9BF，不得越界");

    let text_ok = SleepCode::ALL.iter().all(|c| !c.detail().is_empty() && c.in_window());
    s.add("文案闭集：13 个诊断码均有中文建议且入窗", text_ok,
          "每个码必须可读，且地址落在本卡 0x40 窗口");

    let uniq_ok = SleepCode::ALL
        .iter()
        .enumerate()
        .all(|(i, a)| SleepCode::ALL.iter().skip(i + 1).all(|b| a.code() != b.code()));
    s.add("诊断码：13 个码值两两唯一", uniq_ok,
          "码值为 repr(u16) 判别值，闭集内不得重复");

    let clock_ok = (0u8..=4).all(|c| clock_known(c) && !clock_name(c).is_empty())
        && !clock_known(5) && clock_name(9) == "?";
    s.add("时钟表：0..=4 有名，越界不承认", clock_ok,
          "clockid 越界一律 ENOSYS，名字表查询不得越界");

    let round_ok = [0u64, 1, 999_999_999, 1_000_000_000, 1_500_000_001, 9_999_999_999]
        .iter()
        .all(|v| TimeSpec::from_ns(*v).to_ns() == Some(*v));
    s.add("规格化回代：纳秒 ↔ (sec,nsec) 逐值恒等", round_ok,
          "剩余量必须先折成规范 timespec，nsec 恒小于 1e9");

    let gate_ok = TimeSpec::new(-1, 0).validate().is_err()
        && TimeSpec::new(0, -1).validate().is_err()
        && TimeSpec::new(0, NS_PER_SEC as i64).validate().is_err()
        && TimeSpec::new(0, NS_PER_SEC as i64 - 1).validate().is_ok()
        && TimeSpec::new(3, 0).validate().is_ok();
    s.add("参数闸：负秒/负纳秒/纳秒越界一律 EINVAL", gate_ok,
          "tv_sec<0 与 tv_nsec∈[0,1e9) 是唯一合法域");

    let audit = audit();
    let runs = &audit.runs;

    let size_ok = runs.len() == CASE_ROWS;
    s.add_dyn("矩阵规模：8 行 × 2 形态 + 4 特殊例 = 20 例", size_ok,
              &format!("runs={} 期望={}", runs.len(), CASE_ROWS));

    let conservation_ok = runs.iter().all(|r| r.conserved());
    s.add("总量守恒：各段已耗 + 段末剩余 == 原始总量", conservation_ok,
          "任何注入点都不得凭空增删纳秒，被拒例必须零段");

    let exact_ok = runs.iter().all(|r| r.rem_exact());
    s.add("剩余量逐 ns 可观测：每段末剩余 == 总量 − 累计已耗", exact_ok,
          "逐 ns 比对而非近似，规格化回代也必须恒等");

    let mono_ok = runs.iter().all(|r| r.rem_monotone());
    s.add("剩余量单调不增：重试链上 rem 只减不增", mono_ok,
          "EINTR 后调用方以 rem 重试，剩余量不允许回升");

    let wb_ok = runs.iter().all(|r| r.writeback_ok());
    s.add("回写纪律：写 rem ⇔ nanosleep 且中断且指针有效", wb_ok,
          "成功返回、绝对时钟、clock 族一律不得触碰用户缓冲区");

    let clock_nowrite = runs.iter().filter(|r| !r.rem_capable).all(|r| !r.wrote_rem);
    s.add("clock 族全档位不写 rem", clock_nowrite,
          "clock_nanosleep 无论相对/绝对、成败，都不回写剩余量");

    let abs_nowrite = runs.iter().filter(|r| r.outcome != SleepOutcome::Rejected)
        .filter(|r| family_row(r.row).map_or(false, |x| x.abs))
        .all(|r| !r.wrote_rem);
    s.add("TIMER_ABSTIME 不写 rem：绝对请求剩余量无定义", abs_nowrite,
          "绝对时刻的剩余量由调用方自行换算，内核只回 errno");

    let faults: Vec<&Run> = runs.iter().filter(|r| r.outcome == SleepOutcome::Faulted).collect();
    let fault_ok = faults.len() == 1
        && faults.iter().all(|r| {
            r.rem_capable && !r.rem_ptr && r.errno == EFAULT
                && r.last().map_or(false, |st| st.kind == StepKind::Fault && st.rem_ns > 0)
        });
    s.add("EFAULT 单点：仅在中断回写点、且剩余量>0", fault_ok,
          "指针无效时不产生部分结果，已耗纳秒仍计入游标");

    let rejected_ok = runs.iter().filter(|r| r.outcome == SleepOutcome::Rejected).all(|r| {
        r.n == 0 && (r.errno == EINVAL || r.errno == ENOSYS) && r.conserved()
    });
    s.add("次序：参数闸先拒且零注入段（EINVAL 先于 EFAULT）", rejected_ok,
          "非法 timespec 在触碰指针之前就被驳回");

    let style_ok = runs.iter().all(|r| r.ret_style_ok());
    s.add("返回风格：nanosleep 给 −errno，clock 族给正 errno", style_ok,
          "两种接口的失败口径不得混用");

    let zero_ok = runs.iter().any(|r| {
        r.total_ns == 0 && r.outcome == SleepOutcome::Completed && !r.wrote_rem && r.n == 0
    });
    s.add("零时长：立即成功、零段、不写 rem", zero_ok,
          "tv_sec=tv_nsec=0 是合法请求，内核直接返回 0");

    let b20_ok = (0usize..FAMILY_ROWS).all(|i| match family_row(i) {
        Some(row) => {
            let frozen = B20_FROZEN.get(i).copied().unwrap_or(0xFF);
            row.self_consistent() && row.b20_mask() == frozen
        }
        None => false,
    });
    s.add("B20 对账：8 行三面掩码与冻结表逐行一致", b20_ok,
          "写 rem / 直接返回 / 绝对时钟三面只要错一面即对账失败");

    let cover_ok = SLEEP_FAMILY.iter().filter(|r| r.writes_rem).count() == 1
        && SLEEP_FAMILY.iter().filter(|r| r.api.direct()).count() == 7
        && SLEEP_FAMILY.iter().filter(|r| r.abs).count() == 2;
    s.add("B20 覆盖：写 rem 1 行、直接返回 7 行、绝对 2 行", cover_ok,
          "休眠族形态分布冻结，缺面说明族表被改坏");

    let rounds_ok = audit.ledger.rounds == INJECT_ROUNDS
        && audit.ledger.injected == CASE_ROWS as u32 * INJECT_ROUNDS
        && audit.ledger.detected == audit.ledger.injected;
    s.add("注入账本：20 例 × 10 轮全检出", rounds_ok,
          "injected == detected + escaped 且 escaped 为 0");

    let ledger_ok = audit.ledger.balanced();
    s.add("账本平衡：detected 加 escaped 等于 injected", ledger_ok,
          "任何逃逸都意味着不变量被绕过");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    let corpus_len = corpus(&matrix()).len();
    s.add("双跑一致：语料指纹逐位相等且语料非空", fp_a == fp_b && fp_a != 0 && corpus_len == 100,
          "同输入两次指纹必须相等，语料长度 20×5 字节");

    let linux_ok = agrees_with_linux(EINTR) && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL) && agrees_with_linux(ENOSYS)
        && linux_neg(EINTR) == -4 && linux_neg(EFAULT) == -14
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINTR=4/EFAULT=14/EINVAL=22/ENOSYS=38", linux_ok,
          "错误码只允许表内正值，负值口径由 linux_neg 统一给出");

    let total_ok = (0usize..FAMILY_ROWS).all(|i| row_total(i) == BASE_NS << (i & 3));
    s.add("行总量形态：1s 起按 2 的幂放大，只用移位", total_ok,
          "总量全部为整纳秒，不存在小数换算");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn need_row(i: usize) -> SleepRow {
        match family_row(i) {
            Some(r) => r,
            None => SleepRow { name: "?", clock: 9, api: SleepApi::Nano, abs: false, writes_rem: false },
        }
    }
    fn need_step(r: &Run, i: usize) -> Step {
        match r.step(i) { Some(v) => v, None => STEP_INIT }
    }
    #[test]
    fn segment_and_codes() {
        assert_eq!(SEG_BASE, 0x7B980);
        assert_eq!(SEG_BASE & 0x3F, 0);
        assert_eq!(SleepCode::ALL.len(), 13);
        assert_eq!(SleepCode::ALL.iter().all(|c| c.in_window() && !c.detail().is_empty()), true);
        assert_eq!(SleepCode::SecNegative.addr(), SEG_BASE + 1);
        assert_eq!(SleepCode::NotReproducible.addr(), SEG_BASE + 13);
    }
    #[test]
    fn timespec_roundtrip() {
        assert_eq!(TimeSpec::from_ns(0), TimeSpec::ZERO);
        assert_eq!(TimeSpec::from_ns(1_500_000_001), TimeSpec::new(1, 500_000_001));
        assert_eq!(TimeSpec::new(1, 500_000_001).to_ns(), Some(1_500_000_001));
        assert_eq!(TimeSpec::from_ns(NS_PER_SEC), TimeSpec::new(1, 0));
        assert_eq!(TimeSpec::from_ns(NS_PER_SEC - 1), TimeSpec::new(0, 999_999_999));
    }
    #[test]
    fn timespec_gate() {
        assert_eq!(TimeSpec::new(-1, 0).validate(), Err(EINVAL));
        assert_eq!(TimeSpec::new(0, -1).validate(), Err(EINVAL));
        assert_eq!(TimeSpec::new(0, 1_000_000_000).validate(), Err(EINVAL));
        assert_eq!(TimeSpec::new(0, 999_999_999).validate(), Ok(()));
        assert_eq!(TimeSpec::new(-1, 0).to_ns(), None);
    }
    #[test]
    fn zero_sleep_completes() {
        let row = need_row(0);
        let plan = Plan { pts: [0, 0, 0], n: 0 };
        let run = simulate(&row, 0, TimeSpec::ZERO, &plan, true);
        assert_eq!(run.outcome, SleepOutcome::Completed);
        assert_eq!(run.n, 0);
        assert_eq!(!run.wrote_rem, true);
        assert_eq!(run.full_ok(), true);
    }
    #[test]
    fn single_injection_nanosleep() {
        let row = need_row(0);
        let total = row_total(0);
        let plan = plan_for(0, total);
        let run = simulate(&row, 0, TimeSpec::from_ns(total), &plan, true);
        assert_eq!(run.outcome, SleepOutcome::Interrupted);
        assert_eq!(run.errno, EINTR);
        assert_eq!(run.ret, -4);
        assert_eq!(run.wrote_rem, true);
        assert_eq!(run.consumed(), 750_000_000);
        assert_eq!(run.final_rem(), 250_000_000);
        assert_eq!(run.full_ok(), true);
    }
    #[test]
    fn triple_injection_nanosleep() {
        let row = need_row(0);
        let total = row_total(0);
        let plan = plan_for(1, total);
        let run = simulate(&row, 0, TimeSpec::from_ns(total), &plan, true);
        assert_eq!(run.n, 3);
        let s0 = need_step(&run, 0);
        let s1 = need_step(&run, 1);
        let s2 = need_step(&run, 2);
        assert_eq!(s0.rem_ns, 937_500_000);
        assert_eq!(s1.rem_ns, 500_000_000);
        assert_eq!(s2.rem_ns, 125_000_000);
        assert_eq!(run.consumed(), 875_000_000);
        assert_eq!(run.final_rem(), 125_000_000);
        assert_eq!(run.rem_monotone() && run.conserved() && run.rem_exact(), true);
    }
    #[test]
    fn clock_never_writes_rem() {
        let row = need_row(2);
        let total = row_total(2);
        let plan = plan_for(1, total);
        let run = simulate(&row, 2, TimeSpec::from_ns(total), &plan, true);
        assert_eq!(run.outcome, SleepOutcome::Interrupted);
        assert_eq!(run.ret, 4);
        assert_eq!(!run.wrote_rem, true);
        assert_eq!(run.n, 3);
        assert_eq!(run.full_ok(), true);
    }
    #[test]
    fn absolute_ignores_bad_pointer() {
        let row = need_row(7);
        let total = row_total(7);
        let plan = plan_for(0, total);
        let run = simulate(&row, 7, TimeSpec::from_ns(total), &plan, false);
        assert_eq!(run.outcome, SleepOutcome::Interrupted);
        assert_eq!(run.errno, EINTR);
        assert_eq!(run.ret, 4);
        assert_eq!(!run.wrote_rem, true);
    }
    #[test]
    fn fault_only_at_writeback() {
        let row = need_row(0);
        let total = row_total(0);
        let plan = plan_for(0, total);
        let run = simulate(&row, 0, TimeSpec::from_ns(total), &plan, false);
        assert_eq!(run.outcome, SleepOutcome::Faulted);
        assert_eq!(run.errno, EFAULT);
        assert_eq!(run.ret, -14);
        assert_eq!(run.n, 1);
        assert_eq!(run.final_rem(), 250_000_000);
        assert_eq!(run.conserved(), true);
    }
    #[test]
    fn reject_before_any_injection() {
        let row = need_row(3);
        let plan = plan_for(0, row_total(3));
        let run = simulate(&row, 3, TimeSpec::new(0, 1_000_000_000), &plan, true);
        assert_eq!(run.outcome, SleepOutcome::Rejected);
        assert_eq!(run.n, 0);
        assert_eq!(run.errno, EINVAL);
        assert_eq!(run.ret, 22);
        assert_eq!(run.conserved(), true);
        let fake = SleepRow { name: "fake", clock: 9, api: SleepApi::Clock, abs: false, writes_rem: false };
        let bad = simulate(&fake, 9, TimeSpec::new(1, 0), &plan, true);
        assert_eq!(bad.errno, ENOSYS);
        assert_eq!(bad.ret, 38);
        assert_eq!(bad.n, 0);
    }
    #[test]
    fn b20_reconciles() {
        assert_eq!(SLEEP_FAMILY.len(), FAMILY_ROWS);
        let mut i = 0usize;
        while i < FAMILY_ROWS {
            let row = need_row(i);
            assert_eq!(row.self_consistent(), true);
            assert_eq!(row.b20_mask(), B20_FROZEN.get(i).copied().unwrap_or(0xFF));
            i += 1;
        }
        assert_eq!(family_row(0).map_or(0, |r| r.b20_mask()), 1);
        assert_eq!(family_row(6).map_or(0, |r| r.b20_mask()), 6);
    }
    #[test]
    fn ledger_and_fingerprint() {
        let a = audit();
        assert_eq!(a.runs.len(), CASE_ROWS);
        assert_eq!(a.runs.iter().all(|r| r.full_ok()), true);
        assert_eq!(a.ledger.injected, 200);
        assert_eq!(a.ledger.detected, 200);
        assert_eq!(a.ledger.escaped, 0);
        assert_eq!(a.ledger.balanced(), true);
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus(&matrix()).len(), 100);
        assert_eq!(corpus_fingerprint(), fnv1a(&corpus(&matrix())));
    }
    #[test]
    fn matrix_composition() {
        let runs = matrix();
        let nano = runs.iter().filter(|r| r.rem_capable).count();
        let clock_runs = runs.iter().filter(|r| !r.rem_capable).count();
        let rejected = runs.iter().filter(|r| r.outcome == SleepOutcome::Rejected).count();
        let faulted = runs.iter().filter(|r| r.outcome == SleepOutcome::Faulted).count();
        let completed = runs.iter().filter(|r| r.outcome == SleepOutcome::Completed).count();
        assert_eq!(nano, 4);
        assert_eq!(clock_runs, 16);
        assert_eq!(rejected, 1);
        assert_eq!(faulted, 1);
        assert_eq!(completed, 1);
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9304_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
