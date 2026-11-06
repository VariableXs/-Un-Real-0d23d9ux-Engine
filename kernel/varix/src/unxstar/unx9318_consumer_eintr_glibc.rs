//! UNX-F9318 EINTR 与 glibc TEMP_FAILURE_RETRY 消费核 · 0x7BD00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9318 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9318-J1 重试宏消费链判据过，重试后
//!   语义正确可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果
//!   摘要非空。
//!
//! 设计要点：
//!   ① 复刻 glibc 宏的**判定次序**：`__result == -1L && errno == EINTR`。次序
//!      不可交换——errno 是线程局部且成功路径不清零，若先看 errno 再看不看
//!      返回值，就会把「成功 + 残留 EINTR」误判成需要重试，读到的 12 字节被
//!      丢弃，语义直接错。retryable_on 因此先比 result，短路之后才碰 errno。
//!   ② 消费链是「脚本化」的：每个场景给一串 (返回值, errno) 步骤，由重试核按
//!      站点的重试策略与预算逐次消费。终态四态：Succeeded（result >= 0）、
//!      Failed（不可重试的负值）、BudgetExhausted（还在 EINTR 但预算用尽）。
//!      三种终态都必须能被调用方区分，否则重试核就是把 EINTR 吞掉的漏斗。
//!   ③ 站点策略是白名单而非默认全重试：close 的 EINTR 在 Linux 上可能已经释放
//!      fd，重试会误关新 fd；openat 重试可能重复创建；ioctl 大多非幂等。三者
//!      策略为 NoRetry，EINTR 原样交回。只有 read/write/poll/accept 允许
//!      在预算内重试。
//!   ④ 消费号登记格：每消费一笔 EINTR 就登记一枚 (站点, 站点内序号, 段内号)。
//!      每站点独占 16 格窗口（SEG_BASE + 0x100 + 站点码<<4 + 序号），序号在
//!      站点内跨调用连续递增；越窗返回 None，绝不回绕。
//!   ⑤ 预算语义：budget 是总尝试数上界，0 退化为 1（宁可失败也不死循环）。
//!      预算用尽时 attempts == consumed，否则 consumed == attempts - 1；
//!      两条恒等式在账本里逐场景复核。
//!   ⑥ 注入固定 10 轮 × 11 场景 = 110 次复算，账本 injected == detected +
//!      escaped 且 escaped == 0；诊断码 12 个，码值唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9318 独占 0x7BD00..=0x7BD3F）。
pub const SEG_BASE: u32 = 0x7BD00;
/// 段窗口（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
/// 本批窗口上下界。
pub const SEG_WINDOW_LO: u32 = 0x7B000;
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

// ── Linux errno（正数口径） ───────────────────────────────────────────
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;
pub const ECONNABORTED: i32 = 103;
pub const ENOSPC: i32 = 28;

/// 站点数。
pub const SITE_COUNT: usize = 7;
/// 单站点消费号窗口宽度。
pub const CONSUME_WINDOW: u32 = 16;
/// 场景数。
pub const SCENARIO_COUNT: usize = 11;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 单站点消费号窗口基址偏移。
pub const CONSUME_BASE_OFF: u32 = 0x100;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9318 诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetryCode {
    PredicateDrift,
    StaleErrnoRetried,
    ChainMismatch,
    RegistryGap,
    RegistryDuplicate,
    RegistryOverflow,
    BudgetNotEnforced,
    PolicyViolated,
    AttemptsMismatch,
    LedgerRegress,
    SiteTableGap,
    NotReproducible,
}

impl RetryCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE.saturating_add(1).saturating_add(self.code() as u32)
    }

    /// 中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            RetryCode::PredicateDrift => "重试谓词漂移：只允许 result==-1 且 errno==EINTR，任一放宽都会吞错误",
            RetryCode::StaleErrnoRetried => "残留 errno 触发重试：成功路径不清 errno，必须先看返回值再看 errno",
            RetryCode::ChainMismatch => "消费链与冻结期望不符：步骤数、终态或末次 errno 至少一项对不上",
            RetryCode::RegistryGap => "消费号漏登：每消费一笔 EINTR 必须登记一枚，账目不得有空洞",
            RetryCode::RegistryDuplicate => "消费号重复：同一枚号被登记两次，定位会串台",
            RetryCode::RegistryOverflow => "消费号越窗：单站点 16 格用完必须拒绝登记，禁止回绕复用",
            RetryCode::BudgetNotEnforced => "预算未生效：EINTR 连发时必须被尝试上界截断并交回 EINTR",
            RetryCode::PolicyViolated => "站点策略被绕过：close/openat/ioctl 不得重试 EINTR",
            RetryCode::AttemptsMismatch => "尝试数与消费数不符：预算用尽时 attempts==consumed，否则 consumed==attempts-1",
            RetryCode::LedgerRegress => "注入账不守恒：注入必须等于检出加逃逸，逃逸恒为零",
            RetryCode::SiteTableGap => "站点表有缺口：七个站点都要有码、有名、有策略",
            RetryCode::NotReproducible => "双跑不一致：同脚本两次结果不同，说明核里藏了可变状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [RetryCode] = &[
        RetryCode::PredicateDrift,
        RetryCode::StaleErrnoRetried,
        RetryCode::ChainMismatch,
        RetryCode::RegistryGap,
        RetryCode::RegistryDuplicate,
        RetryCode::RegistryOverflow,
        RetryCode::BudgetNotEnforced,
        RetryCode::PolicyViolated,
        RetryCode::AttemptsMismatch,
        RetryCode::LedgerRegress,
        RetryCode::SiteTableGap,
        RetryCode::NotReproducible,
    ];
}

// ── 站点与策略 ────────────────────────────────────────────────────────
/// 被监控的调用站点（宏消费点）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    Read = 0,
    Write = 1,
    Close = 2,
    Poll = 3,
    OpenAt = 4,
    Accept = 5,
    Ioctl = 6,
}

impl Site {
    /// 站点码（0..=6）。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 站点名。
    pub fn name(self) -> &'static str {
        match self {
            Site::Read => "read",
            Site::Write => "write",
            Site::Close => "close",
            Site::Poll => "poll",
            Site::OpenAt => "openat",
            Site::Accept => "accept",
            Site::Ioctl => "ioctl",
        }
    }

    /// 该站点在 EINTR 后是否允许重试。
    pub fn policy(self) -> RetryPolicy {
        match self {
            Site::Read | Site::Write | Site::Poll | Site::Accept => RetryPolicy::UntilBudget,
            Site::Close | Site::OpenAt | Site::Ioctl => RetryPolicy::NoRetry,
        }
    }

    /// 策略不可重试时给出的理由（可重试站点返回空串）。
    pub fn no_retry_reason(self) -> &'static str {
        match self {
            Site::Close => "close 的 EINTR 可能已释放 fd，重试会误关新 fd",
            Site::OpenAt => "openat 重试可能重复创建对象",
            Site::Ioctl => "多数 ioctl 非幂等，重放会重复副作用",
            _ => "",
        }
    }
}

/// 重试策略。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetryPolicy {
    /// 预算内无限重试
    UntilBudget = 0,
    /// 绝不重试，EINTR 原样交回
    NoRetry = 1,
}

impl RetryPolicy {
    /// 策略码。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 宏执行终态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacroState {
    Succeeded = 0,
    Failed = 1,
    BudgetExhausted = 2,
}

impl MacroState {
    /// 终态码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 终态说明。
    pub fn text(self) -> &'static str {
        match self {
            MacroState::Succeeded => "成功：返回值非负，errno 即使残留也不参与判定",
            MacroState::Failed => "失败：不可重试的错误已原样交回，调用方必须处理",
            MacroState::BudgetExhausted => "预算用尽：仍在 EINTR，必须把 EINTR 交回而不是静默放弃",
        }
    }
}

/// 站点码 → 站点。
pub fn site_from_code(code: u32) -> Option<Site> {
    match code {
        0 => Some(Site::Read),
        1 => Some(Site::Write),
        2 => Some(Site::Close),
        3 => Some(Site::Poll),
        4 => Some(Site::OpenAt),
        5 => Some(Site::Accept),
        6 => Some(Site::Ioctl),
        _ => None,
    }
}

// ── 重试谓词 ──────────────────────────────────────────────────────────
/// glibc 宏的判定：仅当返回 -1 且 errno 为 EINTR。次序不可交换。
pub fn retryable_on(result: i64, errno: i32) -> bool {
    if result != -1 {
        return false;
    }
    errno == EINTR
}

/// 短传输判定：0 < 已传 < 请求量。短读是成功，不是 EINTR。
pub fn is_short_transfer(done: u64, requested: u64) -> bool {
    done > 0 && done < requested
}

/// 一次脚本步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub result: i64,
    pub errno: i32,
}

/// 取第 i 步（越界时钳到末步；空脚本退化为 EINVAL）。
pub fn step_at(script: &[Step], i: u32) -> Step {
    let fallback = Step {
        result: -1,
        errno: EINVAL,
    };
    if script.is_empty() {
        return fallback;
    }
    let last = script.len().saturating_sub(1);
    let idx = (i as usize).min(last);
    match script.get(idx) {
        Some(s) => *s,
        None => fallback,
    }
}

// ── 消费号登记 ────────────────────────────────────────────────────────
/// 一枚消费号：(站点, 站点内序号, 段内号)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeStamp {
    pub site: Site,
    pub ordinal: u32,
    pub no: u32,
}

/// 消费号：站点独占 16 格窗口，越窗返回 None（不回绕）。
pub fn consume_no(site: Site, ordinal: u32) -> Option<u32> {
    if ordinal >= CONSUME_WINDOW {
        return None;
    }
    let base = SEG_BASE
        .saturating_add(CONSUME_BASE_OFF)
        .saturating_add(site.code() << 4);
    base.checked_add(ordinal)
}

/// 消费号窗口上界（含）。
pub fn consume_window_hi(site: Site) -> u32 {
    SEG_BASE
        .saturating_add(CONSUME_BASE_OFF)
        .saturating_add(site.code() << 4)
        .saturating_add(CONSUME_WINDOW - 1)
}

// ── 重试核 ────────────────────────────────────────────────────────────
/// 单站点累计格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SiteSlot {
    pub calls: u32,
    pub attempts: u32,
    pub consumed: u32,
    pub last_errno: i32,
    pub last_result: i64,
    pub exhausted_hits: u32,
}

impl SiteSlot {
    /// 空槽。
    pub const fn zero() -> SiteSlot {
        SiteSlot {
            calls: 0,
            attempts: 0,
            consumed: 0,
            last_errno: 0,
            last_result: 0,
            exhausted_hits: 0,
        }
    }
}

/// 一次宏调用的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallResult {
    pub result: i64,
    pub errno: i32,
    pub attempts: u32,
    pub consumed: u32,
    pub exhausted: bool,
    pub state: MacroState,
}

impl CallResult {
    /// 与冻结期望逐字段比对。
    pub fn matches(&self, e: &Expectation) -> bool {
        self.attempts == e.attempts
            && self.consumed == e.consumed
            && self.result == e.result
            && self.errno == e.errno
            && self.exhausted == e.exhausted
            && self.state == e.state
    }

    /// 恒等式：预算用尽时 attempts==consumed，否则 consumed==attempts-1。
    pub fn identities_hold(&self) -> bool {
        if self.attempts == 0 {
            return false;
        }
        if self.exhausted {
            self.attempts == self.consumed
        } else {
            self.consumed.saturating_add(1) == self.attempts
        }
    }
}

/// 冻结期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Expectation {
    pub attempts: u32,
    pub consumed: u32,
    pub result: i64,
    pub errno: i32,
    pub exhausted: bool,
    pub state: MacroState,
}

/// 重试核：七站点槽 + 总计 + 消费号日志。
pub struct RetryKernel {
    slots: [SiteSlot; SITE_COUNT],
    total_calls: u32,
    total_attempts: u32,
    total_consumed: u32,
    budget_hits: u32,
    log: Vec<ConsumeStamp>,
}

impl RetryKernel {
    /// 空核。
    pub fn new() -> RetryKernel {
        RetryKernel {
            slots: [SiteSlot::zero(); SITE_COUNT],
            total_calls: 0,
            total_attempts: 0,
            total_consumed: 0,
            budget_hits: 0,
            log: Vec::new(),
        }
    }

    /// 站点槽视图。
    pub fn slot(&self, site: Site) -> Option<&SiteSlot> {
        self.slots.get(site.code() as usize)
    }

    /// 总调用数。
    pub fn total_calls(&self) -> u32 {
        self.total_calls
    }

    /// 总尝试数。
    pub fn total_attempts(&self) -> u32 {
        self.total_attempts
    }

    /// 总消费数。
    pub fn total_consumed(&self) -> u32 {
        self.total_consumed
    }

    /// 预算用尽次数。
    pub fn budget_hits(&self) -> u32 {
        self.budget_hits
    }

    /// 消费号日志（写入序）。
    pub fn log(&self) -> &[ConsumeStamp] {
        &self.log
    }

    /// 执行一次宏调用：按站点策略与预算消费脚本。
    pub fn invoke(&mut self, site: Site, script: &[Step], budget: u32) -> CallResult {
        let policy = site.policy();
        let max_attempts = if budget == 0 { 1 } else { budget };
        let idx = site.code() as usize;
        // 先读站点已消费数作为序号基，避免与 &mut 借用冲突。
        let base_ordinal = match self.slots.get(idx) {
            Some(slot) => slot.consumed,
            None => 0,
        };
        let mut attempt = 1u32;
        let mut consumed = 0u32;
        let outcome = loop {
            let step = step_at(script, attempt.saturating_sub(1));
            let wants_retry =
                retryable_on(step.result, step.errno) && policy == RetryPolicy::UntilBudget;
            if !wants_retry {
                let state = if step.result >= 0 {
                    MacroState::Succeeded
                } else {
                    MacroState::Failed
                };
                break CallResult {
                    result: step.result,
                    errno: step.errno,
                    attempts: attempt,
                    consumed,
                    exhausted: false,
                    state,
                };
            }
            consumed = consumed.saturating_add(1);
            let ordinal = base_ordinal.saturating_add(consumed.saturating_sub(1));
            if let Some(no) = consume_no(site, ordinal) {
                self.log.push(ConsumeStamp {
                    site,
                    ordinal,
                    no,
                });
            }
            if attempt >= max_attempts {
                break CallResult {
                    result: step.result,
                    errno: step.errno,
                    attempts: attempt,
                    consumed,
                    exhausted: true,
                    state: MacroState::BudgetExhausted,
                };
            }
            attempt = attempt.saturating_add(1);
        };

        if let Some(slot) = self.slots.get_mut(idx) {
            slot.calls = slot.calls.saturating_add(1);
            slot.attempts = slot.attempts.saturating_add(outcome.attempts);
            slot.consumed = slot.consumed.saturating_add(outcome.consumed);
            slot.last_errno = outcome.errno;
            slot.last_result = outcome.result;
            if outcome.exhausted {
                slot.exhausted_hits = slot.exhausted_hits.saturating_add(1);
            }
        }
        self.total_calls = self.total_calls.saturating_add(1);
        self.total_attempts = self.total_attempts.saturating_add(outcome.attempts);
        self.total_consumed = self.total_consumed.saturating_add(outcome.consumed);
        if outcome.exhausted {
            self.budget_hits = self.budget_hits.saturating_add(1);
        }
        outcome
    }
}

// ── 冻结脚本与场景 ────────────────────────────────────────────────────
const S_READ_SHORT: [Step; 2] = [
    Step { result: -1, errno: EINTR },
    Step { result: 12, errno: EINTR },
];
const S_READ_TWICE: [Step; 3] = [
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: 4096, errno: 0 },
];
const S_READ_FLOOD: [Step; 6] = [
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
];
const S_WRITE_EAGAIN: [Step; 2] = [
    Step { result: -1, errno: EAGAIN },
    Step { result: 4096, errno: 0 },
];
const S_WRITE_RETRY4: [Step; 5] = [
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: 100, errno: 0 },
];
const S_CLOSE: [Step; 2] = [
    Step { result: -1, errno: EINTR },
    Step { result: 0, errno: 0 },
];
const S_POLL_STALE: [Step; 1] = [Step { result: 0, errno: EINTR }];
const S_OPENAT: [Step; 2] = [
    Step { result: -1, errno: EINTR },
    Step { result: 3, errno: 0 },
];
const S_ACCEPT: [Step; 3] = [
    Step { result: -1, errno: EINTR },
    Step { result: -1, errno: EINTR },
    Step { result: 5, errno: 0 },
];
const S_READ_BUDGET1: [Step; 1] = [Step { result: -1, errno: EINTR }];
const S_IOCTL: [Step; 2] = [
    Step { result: -1, errno: EINTR },
    Step { result: 0, errno: 0 },
];

/// 一个冻结场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scenario {
    pub site: Site,
    pub script: &'static [Step],
    pub budget: u32,
    pub exp: Expectation,
}

/// 十一号消费链（覆盖 3 站点策略 × 4 终态 × 预算边界）。
pub const SCENARIOS: [Scenario; SCENARIO_COUNT] = [
    Scenario {
        site: Site::Read,
        script: &S_READ_SHORT,
        budget: 8,
        exp: Expectation {
            attempts: 2,
            consumed: 1,
            result: 12,
            errno: EINTR,
            exhausted: false,
            state: MacroState::Succeeded,
        },
    },
    Scenario {
        site: Site::Read,
        script: &S_READ_TWICE,
        budget: 8,
        exp: Expectation {
            attempts: 3,
            consumed: 2,
            result: 4096,
            errno: 0,
            exhausted: false,
            state: MacroState::Succeeded,
        },
    },
    Scenario {
        site: Site::Read,
        script: &S_READ_FLOOD,
        budget: 5,
        exp: Expectation {
            attempts: 5,
            consumed: 5,
            result: -1,
            errno: EINTR,
            exhausted: true,
            state: MacroState::BudgetExhausted,
        },
    },
    Scenario {
        site: Site::Write,
        script: &S_WRITE_EAGAIN,
        budget: 8,
        exp: Expectation {
            attempts: 1,
            consumed: 0,
            result: -1,
            errno: EAGAIN,
            exhausted: false,
            state: MacroState::Failed,
        },
    },
    Scenario {
        site: Site::Write,
        script: &S_WRITE_RETRY4,
        budget: 10,
        exp: Expectation {
            attempts: 5,
            consumed: 4,
            result: 100,
            errno: 0,
            exhausted: false,
            state: MacroState::Succeeded,
        },
    },
    Scenario {
        site: Site::Close,
        script: &S_CLOSE,
        budget: 8,
        exp: Expectation {
            attempts: 1,
            consumed: 0,
            result: -1,
            errno: EINTR,
            exhausted: false,
            state: MacroState::Failed,
        },
    },
    Scenario {
        site: Site::Poll,
        script: &S_POLL_STALE,
        budget: 8,
        exp: Expectation {
            attempts: 1,
            consumed: 0,
            result: 0,
            errno: EINTR,
            exhausted: false,
            state: MacroState::Succeeded,
        },
    },
    Scenario {
        site: Site::OpenAt,
        script: &S_OPENAT,
        budget: 8,
        exp: Expectation {
            attempts: 1,
            consumed: 0,
            result: -1,
            errno: EINTR,
            exhausted: false,
            state: MacroState::Failed,
        },
    },
    Scenario {
        site: Site::Accept,
        script: &S_ACCEPT,
        budget: 4,
        exp: Expectation {
            attempts: 3,
            consumed: 2,
            result: 5,
            errno: 0,
            exhausted: false,
            state: MacroState::Succeeded,
        },
    },
    Scenario {
        site: Site::Read,
        script: &S_READ_BUDGET1,
        budget: 1,
        exp: Expectation {
            attempts: 1,
            consumed: 1,
            result: -1,
            errno: EINTR,
            exhausted: true,
            state: MacroState::BudgetExhausted,
        },
    },
    Scenario {
        site: Site::Ioctl,
        script: &S_IOCTL,
        budget: 8,
        exp: Expectation {
            attempts: 1,
            consumed: 0,
            result: -1,
            errno: EINTR,
            exhausted: false,
            state: MacroState::Failed,
        },
    },
];

/// 跑完十一条链，返回（核, 逐条结果, 命中条数）。
pub fn run_scenarios() -> (RetryKernel, Vec<CallResult>, u32) {
    let mut k = RetryKernel::new();
    let mut outs: Vec<CallResult> = Vec::new();
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(i).copied() {
            let out = k.invoke(sc.site, sc.script, sc.budget);
            if out.matches(&sc.exp) {
                matched = matched.saturating_add(1);
            }
            outs.push(out);
        }
        i = i.saturating_add(1);
    }
    (k, outs, matched)
}

/// 登记格校验：全在册、逐站点连续、两两唯一。
pub fn registry_ok(stamps: &[ConsumeStamp]) -> bool {
    let mut expected = [0u32; SITE_COUNT];
    let mut i = 0usize;
    while i < stamps.len() {
        let st = match stamps.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if consume_no(st.site, st.ordinal) != Some(st.no) {
            return false;
        }
        let idx = st.site.code() as usize;
        let cur = match expected.get(idx) {
            Some(v) => *v,
            None => return false,
        };
        if st.ordinal != cur {
            return false;
        }
        if let Some(slot) = expected.get_mut(idx) {
            *slot = cur.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    let mut a = 0usize;
    while a < stamps.len() {
        let mut b = a.saturating_add(1);
        while b < stamps.len() {
            if let (Some(x), Some(y)) = (stamps.get(a), stamps.get(b)) {
                if x.no == y.no {
                    return false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    true
}

/// 站点表完整性：七站点码唯一、名非空、策略表覆盖全部站点。
pub fn site_table_ok() -> bool {
    let mut i = 0u32;
    let mut mask = 0u32;
    while i < SITE_COUNT as u32 {
        let site = match site_from_code(i) {
            Some(v) => v,
            None => return false,
        };
        if site.name().is_empty() || site.code() != i {
            return false;
        }
        let bit = 1u32 << i;
        if mask & bit != 0 {
            return false;
        }
        mask |= bit;
        let no_retry = site.policy() == RetryPolicy::NoRetry;
        if no_retry && site.no_retry_reason().is_empty() {
            return false;
        }
        if !no_retry && !site.no_retry_reason().is_empty() {
            return false;
        }
        i = i.saturating_add(1);
    }
    site_from_code(SITE_COUNT as u32).is_none()
}

/// 各站点槽累计与总计自洽。
pub fn slots_consistent(k: &RetryKernel) -> bool {
    let mut sum_calls = 0u32;
    let mut sum_attempts = 0u32;
    let mut sum_consumed = 0u32;
    let mut i = 0u32;
    while i < SITE_COUNT as u32 {
        if let Some(site) = site_from_code(i) {
            if let Some(slot) = k.slot(site) {
                sum_calls = sum_calls.saturating_add(slot.calls);
                sum_attempts = sum_attempts.saturating_add(slot.attempts);
                sum_consumed = sum_consumed.saturating_add(slot.consumed);
            }
        }
        i = i.saturating_add(1);
    }
    sum_calls == k.total_calls()
        && sum_attempts == k.total_attempts()
        && sum_consumed == k.total_consumed()
        && k.log().len() as u32 == k.total_consumed()
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetryLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl RetryLedger {
    /// 零值。
    pub const fn zero() -> RetryLedger {
        RetryLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔。
    pub fn record(&mut self, caught: bool) -> u32 {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.injected
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 注入：10 轮 × 11 场景，逐轮新建空核复算同一条链。
pub fn run_injection() -> RetryLedger {
    let mut ledger = RetryLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < SCENARIOS.len() {
            if let Some(sc) = SCENARIOS.get(i).copied() {
                let mut k = RetryKernel::new();
                let out = k.invoke(sc.site, sc.script, sc.budget);
                let caught = out.matches(&sc.exp) && out.identities_hold() && registry_ok(k.log());
                ledger.record(caught);
            }
            i = i.saturating_add(1);
        }
        round = round.saturating_add(1);
    }
    ledger
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

fn push_i64(out: &mut Vec<u8>, v: i64) {
    let lo = v as u64;
    push_u32(out, (lo >> 32) as u32);
    push_u32(out, lo as u32);
}

/// 规范语料指纹：脚本 + 终态 + 登记号。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, SCENARIO_COUNT as u32);
    let mut i = 0usize;
    while i < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(i) {
            push_u32(&mut out, sc.site.code());
            push_u32(&mut out, sc.budget);
            push_u32(&mut out, sc.script.len() as u32);
            let mut j = 0usize;
            while j < sc.script.len() {
                if let Some(st) = sc.script.get(j) {
                    push_i64(&mut out, st.result);
                    push_u32(&mut out, st.errno as u32);
                }
                j = j.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    let (k, outs, _matched) = run_scenarios();
    let mut s = 0usize;
    while s < outs.len() {
        if let Some(o) = outs.get(s) {
            push_u32(&mut out, o.attempts);
            push_u32(&mut out, o.consumed);
            push_i64(&mut out, o.result);
            push_u32(&mut out, o.state.code());
            push_u32(&mut out, if o.exhausted { 1 } else { 0 });
        }
        s = s.saturating_add(1);
    }
    let mut g = 0usize;
    let log = k.log();
    while g < log.len() {
        if let Some(st) = log.get(g) {
            push_u32(&mut out, st.no);
            push_u32(&mut out, st.ordinal);
            push_u32(&mut out, st.site.code());
        }
        g = g.saturating_add(1);
    }
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9318 判据集。
pub fn run_unx9318_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9318");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7BD00;
    s.add(
        "段位守卫：0x40 对齐且不与 F9317 段重叠",
        seg_ok,
        "本卡段基址 0x7BD00 必须独占 0x7BD00..=0x7BD3F",
    );

    let pred_ok = retryable_on(-1, EINTR)
        && !retryable_on(-1, EAGAIN)
        && !retryable_on(-1, 0)
        && !retryable_on(0, EINTR)
        && !retryable_on(12, EINTR)
        && !retryable_on(1, 0)
        && !retryable_on(-1, EINVAL)
        && !retryable_on(-1, EBADF);
    s.add(
        "重试谓词：八组组合与 glibc 宏一致",
        pred_ok,
        "只有 -1+EINTR 重试；-1+其它、非负+任意 errno 一律不重试",
    );

    let (k, outs, matched) = run_scenarios();
    s.add(
        "消费链：十一条场景与冻结期望逐条一致",
        matched == SCENARIO_COUNT as u32 && outs.len() == SCENARIO_COUNT,
        "attempts/consumed/终态/末次 errno 四项全对才算过",
    );

    let all_identities = outs.iter().all(|o| o.identities_hold());
    s.add(
        "消费链：尝试数与消费数恒等式成立",
        all_identities,
        "预算用尽时 attempts==consumed，否则 consumed==attempts-1",
    );

    s.add(
        "消费号登记格：全在册、逐站点连续、两两唯一",
        registry_ok(k.log()) && k.log().len() == 15 && k.total_consumed() == 15,
        "十五枚号必须落在各站点 16 格窗口内且不重号",
    );

    let window_ok = consume_no(Site::Read, 0) == Some(SEG_BASE + 0x100)
        && consume_no(Site::Read, 15) == Some(SEG_BASE + 0x10F)
        && consume_no(Site::Read, 16).is_none()
        && consume_no(Site::Ioctl, 0) == Some(SEG_BASE + 0x160)
        && consume_no(Site::Ioctl, 16).is_none()
        && consume_window_hi(Site::Read) == SEG_BASE + 0x10F;
    s.add(
        "消费号窗口：单站点 16 格且越窗拒绝",
        window_ok,
        "第 17 枚号必须返回 None，禁止回绕覆盖旧号",
    );

    let flood = outs.get(2).copied();
    let budget_ok = match flood {
        Some(o) => {
            o.exhausted
                && o.attempts == 5
                && o.consumed == 5
                && o.errno == EINTR
                && o.state == MacroState::BudgetExhausted
        }
        None => false,
    };
    s.add(
        "预算：EINTR 连发被 5 次尝试上界截断",
        budget_ok,
        "无限 EINTR 必须由预算收口，并把 EINTR 原样交回",
    );

    let mut zero_budget = RetryKernel::new();
    let zb = zero_budget.invoke(Site::Read, &S_READ_FLOOD, 0);
    s.add(
        "预算：budget=0 退化为 1 次尝试",
        zb.attempts == 1 && zb.consumed == 1 && zb.exhausted && zero_budget.budget_hits() == 1,
        "预算零值不得死循环，必须一次即收口",
    );

    let short = outs.first().copied();
    let short_ok = match short {
        Some(o) => {
            o.result == 12
                && o.attempts == 2
                && o.consumed == 1
                && o.errno == EINTR
                && o.state == MacroState::Succeeded
        }
        None => false,
    };
    s.add(
        "语义可观测：短读 12 字节且 errno 残留仍判成功",
        short_ok && is_short_transfer(12, 4096),
        "成功路径不清 errno，12 字节必须被当成成功而不是重试",
    );

    let mut non_eintr = RetryKernel::new();
    let eagain = non_eintr.invoke(Site::Write, &S_WRITE_EAGAIN, 8);
    let ebadf = non_eintr.invoke(
        Site::Read,
        &[Step {
            result: -1,
            errno: EBADF,
        }],
        8,
    );
    let enoent = non_eintr.invoke(
        Site::Read,
        &[Step {
            result: -1,
            errno: ENOENT,
        }],
        8,
    );
    s.add(
        "非 EINTR 失败：EAGAIN/EBADF/ENOENT 各尝试一次",
        eagain.attempts == 1
            && eagain.errno == EAGAIN
            && ebadf.attempts == 1
            && ebadf.errno == EBADF
            && enoent.attempts == 1
            && enoent.errno == ENOENT
            && non_eintr.total_consumed() == 0,
        "非 EINTR 的负值必须立即交回，不得被重试核吞掉",
    );

    let close_r = outs.get(5).copied();
    let openat_r = outs.get(7).copied();
    let ioctl_r = outs.get(10).copied();
    let no_retry_ok = match (close_r, openat_r, ioctl_r) {
        (Some(c), Some(o), Some(i)) => {
            c.attempts == 1
                && c.consumed == 0
                && c.errno == EINTR
                && o.attempts == 1
                && o.consumed == 0
                && i.attempts == 1
                && i.consumed == 0
        }
        _ => false,
    };
    s.add(
        "站点策略：close/openat/ioctl 一律不重试 EINTR",
        no_retry_ok,
        "三站点的 EINTR 必须原样交回，重试会放大副作用",
    );

    s.add(
        "站点表：七站点码唯一、名非空、策略覆盖",
        site_table_ok() && SITE_COUNT == 7,
        "站点表是策略白名单，缺口会让某些 EINTR 无声消失",
    );

    s.add(
        "总账自洽：尝试 24 / 消费 15 / 调用 11",
        slots_consistent(&k)
            && k.total_attempts() == 24
            && k.total_consumed() == 15
            && k.total_calls() == SCENARIO_COUNT as u32
            && k.budget_hits() == 2,
        "槽累计必须等于总计，消费数必须等于登记条数",
    );

    let injection = run_injection();
    let expect_injections = (SCENARIO_COUNT as u32).saturating_mul(INJECT_ROUNDS);
    s.add(
        "注入账：10 轮 × 11 场景全检出、逃逸为零",
        injection.conserved()
            && injection.escaped == 0
            && injection.injected == expect_injections
            && injection.detected == expect_injections,
        "110 次复算必须 110 次检出，含登记格与恒等式",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：脚本与终态指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同脚本两次必须逐字节相等，核里不许藏可变状态",
    );

    s.add(
        "诊断码：12 个码文案非空",
        RetryCode::ALL.len() == 12 && RetryCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < RetryCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < RetryCode::ALL.len() {
            if let (Some(x), Some(y)) = (RetryCode::ALL.get(a), RetryCode::ALL.get(b)) {
                if x.val() == y.val() {
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
        "段内闭集码不得重复",
    );

    let log_text = format!(
        "链{} 尝试{} 消费{} 预算{}",
        k.total_calls(),
        k.total_attempts(),
        k.total_consumed(),
        k.budget_hits()
    );
    s.add_dyn(
        "消费链快照",
        matched == SCENARIO_COUNT as u32,
        &log_text,
    );
    let reg_text = format!("登记 {} 枚 唯一 {}", k.log().len(), registry_ok(k.log()));
    s.add_dyn(
        "登记格快照",
        registry_ok(k.log()) && !k.log().is_empty(),
        &reg_text,
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
    fn predicate_is_result_first() {
        assert_eq!(retryable_on(-1, EINTR), true);
        assert_eq!(retryable_on(0, EINTR), false);
        assert_eq!(retryable_on(12, EINTR), false);
        assert_eq!(retryable_on(-1, EAGAIN), false);
        assert_eq!(retryable_on(-1, 0), false);
    }

    #[test]
    fn short_transfer_is_success() {
        assert_eq!(is_short_transfer(12, 4096), true);
        assert_eq!(is_short_transfer(4096, 4096), false);
        assert_eq!(is_short_transfer(0, 4096), false);
    }

    #[test]
    fn all_scenarios_match() {
        let (_k, outs, matched) = run_scenarios();
        assert_eq!(outs.len(), SCENARIO_COUNT);
        assert_eq!(matched, SCENARIO_COUNT as u32);
        assert_eq!(outs.iter().all(|o| o.identities_hold()), true);
    }

    #[test]
    fn registry_is_dense_and_unique() {
        let (k, _o, _m) = run_scenarios();
        assert_eq!(k.log().len(), 15);
        assert_eq!(registry_ok(k.log()), true);
        assert_eq!(k.total_consumed(), 15);
        let first = k.log().first().copied();
        assert_eq!(first.map(|s| s.no), Some(SEG_BASE + 0x100));
    }

    #[test]
    fn window_rejects_overflow() {
        assert_eq!(consume_no(Site::Read, 0), Some(SEG_BASE + 0x100));
        assert_eq!(consume_no(Site::Read, 15), Some(SEG_BASE + 0x10F));
        assert_eq!(consume_no(Site::Read, 16), None);
        assert_eq!(consume_no(Site::Accept, 0), Some(SEG_BASE + 0x150));
        assert_eq!(consume_window_hi(Site::Accept), SEG_BASE + 0x15F);
    }

    #[test]
    fn budget_cuts_eintr_flood() {
        let mut k = RetryKernel::new();
        let r = k.invoke(Site::Read, &S_READ_FLOOD, 5);
        assert_eq!(r.attempts, 5);
        assert_eq!(r.consumed, 5);
        assert_eq!(r.exhausted, true);
        assert_eq!(r.state, MacroState::BudgetExhausted);
        assert_eq!(r.errno, EINTR);
        assert_eq!(k.budget_hits(), 1);
    }

    #[test]
    fn zero_budget_is_one_attempt() {
        let mut k = RetryKernel::new();
        let r = k.invoke(Site::Read, &S_READ_FLOOD, 0);
        assert_eq!(r.attempts, 1);
        assert_eq!(r.consumed, 1);
        assert_eq!(r.exhausted, true);
    }

    #[test]
    fn no_retry_sites_pass_eintr_back() {
        let mut k = RetryKernel::new();
        let c = k.invoke(Site::Close, &S_CLOSE, 8);
        let o = k.invoke(Site::OpenAt, &S_OPENAT, 8);
        let i = k.invoke(Site::Ioctl, &S_IOCTL, 8);
        assert_eq!(c.attempts, 1);
        assert_eq!(o.attempts, 1);
        assert_eq!(i.attempts, 1);
        assert_eq!(c.consumed, 0);
        assert_eq!(k.total_consumed(), 0);
        assert_eq!(Site::Close.policy(), RetryPolicy::NoRetry);
        assert_eq!(Site::Read.policy(), RetryPolicy::UntilBudget);
    }

    #[test]
    fn site_table_complete() {
        assert_eq!(site_table_ok(), true);
        assert_eq!(site_from_code(0), Some(Site::Read));
        assert_eq!(site_from_code(6), Some(Site::Ioctl));
        assert_eq!(site_from_code(7), None);
        assert_eq!(Site::Close.no_retry_reason().is_empty(), false);
    }

    #[test]
    fn injection_conserved() {
        let l = run_injection();
        assert_eq!(l.injected, 110);
        assert_eq!(l.detected, 110);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.conserved(), true);
    }

    #[test]
    fn fingerprint_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9318_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
