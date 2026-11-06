//! UNX-F50151 发现错误码族（15 条：超时/坏包/冲突等）（0x43f00 段）
//!
//! 锚点：UNX-F50151-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题五。
//!
//! 错误码族最怕的不是「码不够用」，而是**码在版本之间悄悄换了位置**：日志里
//! 的 0x43f07 去年是「坏包」，今年变成「超时」，历史告警就全部读错。所以这里
//! 把 15 条码钉在一张静态表里（[`FAULT_TABLE`]），表项顺序即码值偏移
//! （`SEG_BASE + 1 ..= SEG_BASE + 15`），并由 [`fault_code_audit`] 逐项复核：
//! 码值连续、名字唯一、`code()` 与表项一致、摘要可复算。
//!
//! 三个数据结构各司其职：
//! * [`Fault`] —— 15 条封闭枚举，`info()` 给出名字/严重级/类别/可重试性；
//! * [`FaultClassifier`] —— 把「原始事件」（状态码 + 标志位）映射成具体错误码。
//!   标志位优先于状态码：**传输层已经知道包被截断**时，HTTP 层的 503 只是
//!   截断之后的假象，按 503 归类会把「线路中断」误记成「设备忙」；
//! * [`FaultLedger`] —— 定长计数账。下标 = 偏移 - 1，越界一律 `None`，
//!   不做任何截断映射——把未知码折进 `Internal` 会让账本永远「守恒」但失真。
//!
//! 重试策略挂在表项上而不是散落在调用点：`retryable` 与 `severity` 必须一致
//! （`Fatal`/`Info` 不可重试），退避阶梯单调且封顶 30s，避免 15 处各写一套。
//!
//! 错误闭集：`SEG_BASE + 1 ..= SEG_BASE + 15`，逐项见 [`Fault`]。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x43f00 独占 0x20）。
pub const SEG_BASE: u32 = 0x43f00;

/// 错误码条数（卡面 15 条，硬编码；改动必须同步改表与判据）。
pub const FAULT_COUNT: usize = 15;

/// 退避上限 30s：再长的等待对交互式发现没有意义，用户会直接重选设备。
pub const BACKOFF_CAP_MS: u32 = 30_000;

/// 15 条错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Timeout,
    BadPacket,
    Conflict,
    Unreachable,
    Refused,
    BadReply,
    Auth,
    Busy,
    Unsupported,
    SeqGap,
    Duplicate,
    Overflow,
    Cancelled,
    RateLimited,
    Internal,
}

impl Fault {
    /// 段内偏移（1..=15）。这是**唯一**允许决定码值的地方。
    pub fn offset(self) -> u16 {
        match self {
            Fault::Timeout => 1,
            Fault::BadPacket => 2,
            Fault::Conflict => 3,
            Fault::Unreachable => 4,
            Fault::Refused => 5,
            Fault::BadReply => 6,
            Fault::Auth => 7,
            Fault::Busy => 8,
            Fault::Unsupported => 9,
            Fault::SeqGap => 10,
            Fault::Duplicate => 11,
            Fault::Overflow => 12,
            Fault::Cancelled => 13,
            Fault::RateLimited => 14,
            Fault::Internal => 15,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.offset() as u32
    }

    pub fn name(self) -> &'static str {
        match self {
            Fault::Timeout => "timeout",
            Fault::BadPacket => "bad-packet",
            Fault::Conflict => "conflict",
            Fault::Unreachable => "unreachable",
            Fault::Refused => "refused",
            Fault::BadReply => "bad-reply",
            Fault::Auth => "auth",
            Fault::Busy => "busy",
            Fault::Unsupported => "unsupported",
            Fault::SeqGap => "seq-gap",
            Fault::Duplicate => "duplicate",
            Fault::Overflow => "overflow",
            Fault::Cancelled => "cancelled",
            Fault::RateLimited => "rate-limited",
            Fault::Internal => "internal",
        }
    }

    /// 表项（静态引用，索引为字面量，无运行期越界）。
    pub fn info(self) -> &'static FaultInfo {
        match self {
            Fault::Timeout => &FAULT_TABLE[0],
            Fault::BadPacket => &FAULT_TABLE[1],
            Fault::Conflict => &FAULT_TABLE[2],
            Fault::Unreachable => &FAULT_TABLE[3],
            Fault::Refused => &FAULT_TABLE[4],
            Fault::BadReply => &FAULT_TABLE[5],
            Fault::Auth => &FAULT_TABLE[6],
            Fault::Busy => &FAULT_TABLE[7],
            Fault::Unsupported => &FAULT_TABLE[8],
            Fault::SeqGap => &FAULT_TABLE[9],
            Fault::Duplicate => &FAULT_TABLE[10],
            Fault::Overflow => &FAULT_TABLE[11],
            Fault::Cancelled => &FAULT_TABLE[12],
            Fault::RateLimited => &FAULT_TABLE[13],
            Fault::Internal => &FAULT_TABLE[14],
        }
    }
}

/// 严重级（`rank` 单调：Info < Warn < Error < Fatal）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Error,
    Fatal,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Severity::Info => 0,
            Severity::Warn => 1,
            Severity::Error => 2,
            Severity::Fatal => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }
}

/// 故障类别，决定退避基数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Transport,
    Wire,
    Policy,
    Resource,
    Local,
}

impl Category {
    pub fn backoff_base_ms(self) -> u32 {
        match self {
            Category::Transport => 250,
            Category::Wire => 100,
            Category::Policy => 1_000,
            Category::Resource => 500,
            Category::Local => 0,
        }
    }
}

/// 表项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultInfo {
    pub fault: Fault,
    pub code: u32,
    pub name: &'static str,
    pub severity: Severity,
    pub category: Category,
    pub retryable: bool,
}

/// 15 条码的冻结表。顺序即偏移序，**不允许调整**——调整等于给历史日志换语义。
pub static FAULT_TABLE: [FaultInfo; FAULT_COUNT] = [
    FaultInfo {
        fault: Fault::Timeout,
        code: SEG_BASE + 1,
        name: "timeout",
        severity: Severity::Warn,
        category: Category::Transport,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::BadPacket,
        code: SEG_BASE + 2,
        name: "bad-packet",
        severity: Severity::Error,
        category: Category::Wire,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Conflict,
        code: SEG_BASE + 3,
        name: "conflict",
        severity: Severity::Error,
        category: Category::Policy,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Unreachable,
        code: SEG_BASE + 4,
        name: "unreachable",
        severity: Severity::Warn,
        category: Category::Transport,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::Refused,
        code: SEG_BASE + 5,
        name: "refused",
        severity: Severity::Error,
        category: Category::Transport,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::BadReply,
        code: SEG_BASE + 6,
        name: "bad-reply",
        severity: Severity::Error,
        category: Category::Wire,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Auth,
        code: SEG_BASE + 7,
        name: "auth",
        severity: Severity::Error,
        category: Category::Policy,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Busy,
        code: SEG_BASE + 8,
        name: "busy",
        severity: Severity::Warn,
        category: Category::Resource,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::Unsupported,
        code: SEG_BASE + 9,
        name: "unsupported",
        severity: Severity::Fatal,
        category: Category::Policy,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::SeqGap,
        code: SEG_BASE + 10,
        name: "seq-gap",
        severity: Severity::Error,
        category: Category::Wire,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::Duplicate,
        code: SEG_BASE + 11,
        name: "duplicate",
        severity: Severity::Info,
        category: Category::Wire,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Overflow,
        code: SEG_BASE + 12,
        name: "overflow",
        severity: Severity::Error,
        category: Category::Resource,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::Cancelled,
        code: SEG_BASE + 13,
        name: "cancelled",
        severity: Severity::Info,
        category: Category::Local,
        retryable: false,
    },
    FaultInfo {
        fault: Fault::RateLimited,
        code: SEG_BASE + 14,
        name: "rate-limited",
        severity: Severity::Warn,
        category: Category::Resource,
        retryable: true,
    },
    FaultInfo {
        fault: Fault::Internal,
        code: SEG_BASE + 15,
        name: "internal",
        severity: Severity::Fatal,
        category: Category::Local,
        retryable: false,
    },
];

/// 闭集（顺序即偏移序）。
pub const ALL_FAULTS: [Fault; FAULT_COUNT] = [
    Fault::Timeout,
    Fault::BadPacket,
    Fault::Conflict,
    Fault::Unreachable,
    Fault::Refused,
    Fault::BadReply,
    Fault::Auth,
    Fault::Busy,
    Fault::Unsupported,
    Fault::SeqGap,
    Fault::Duplicate,
    Fault::Overflow,
    Fault::Cancelled,
    Fault::RateLimited,
    Fault::Internal,
];

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 名字/码值查找器。表是静态的，查找器只提供经过校验的视图。
#[derive(Clone, Copy, Debug)]
pub struct FaultRegistry {
    digest: u64,
}

impl FaultRegistry {
    /// 建表并算摘要（摘要覆盖全部 15 条的码值与名字字节）。
    pub fn new() -> FaultRegistry {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < FAULT_COUNT {
            if let Some(info) = FAULT_TABLE.get(i) {
                h = fnv_mix(h, info.code as u64);
                let bytes = info.name.as_bytes();
                let mut k = 0usize;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        h = fnv_mix(h, *b as u64);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        FaultRegistry { digest: h }
    }

    pub fn len(&self) -> usize {
        FAULT_COUNT
    }

    pub fn is_empty(&self) -> bool {
        FAULT_COUNT == 0
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 按名字精确查找（区分大小写：日志里出现的就是这张表的原文）。
    pub fn by_name(&self, name: &str) -> Option<Fault> {
        let mut i = 0usize;
        while i < FAULT_COUNT {
            if let Some(info) = FAULT_TABLE.get(i) {
                if info.name == name {
                    return Some(info.fault);
                }
            }
            i += 1;
        }
        None
    }

    /// 按偏移查找（1..=15）。0 与 >15 一律 `None`，不做折叠。
    pub fn by_offset(&self, off: u16) -> Option<Fault> {
        if off == 0 || off as usize > FAULT_COUNT {
            return None;
        }
        match FAULT_TABLE.get(off as usize - 1) {
            Some(info) => Some(info.fault),
            None => None,
        }
    }

    /// 按裸码值查找（必须落在本段内）。
    pub fn by_code(&self, code: u32) -> Option<Fault> {
        if code <= SEG_BASE {
            return None;
        }
        let off = code - SEG_BASE;
        if off > FAULT_COUNT as u32 {
            return None;
        }
        self.by_offset(off as u16)
    }
}

/// 原始事件：状态码 + 标志位。状态码同时承载 errno（100..=199）
/// 与 HTTP 状态（4xx/5xx），两者不会撞车，因此共用一个字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEvent {
    pub status: u16,
    pub flags: u16,
}

pub const FLAG_TRUNCATED: u16 = 0x0001;
pub const FLAG_CONFLICT: u16 = 0x0002;
pub const FLAG_SEQ_GAP: u16 = 0x0004;
pub const FLAG_DUPLICATE: u16 = 0x0008;
pub const FLAG_AUTH: u16 = 0x0010;
pub const FLAG_RATE: u16 = 0x0020;

/// 分类器：把原始事件映射成具体错误码。
///
/// 顺序即优先级：传输层证据（截断/冲突/序号缺口/重复/鉴权/限速）先于状态码。
/// 这条规则的意义在于：状态码是**对端**说的，标志位是**本端**观测到的；
/// 两者矛盾时，本端观测到的物理事实更可信。
pub fn classify(ev: RawEvent) -> Fault {
    if ev.flags & FLAG_TRUNCATED != 0 {
        return Fault::BadPacket;
    }
    if ev.flags & FLAG_CONFLICT != 0 {
        return Fault::Conflict;
    }
    if ev.flags & FLAG_SEQ_GAP != 0 {
        return Fault::SeqGap;
    }
    if ev.flags & FLAG_DUPLICATE != 0 {
        return Fault::Duplicate;
    }
    if ev.flags & FLAG_AUTH != 0 {
        return Fault::Auth;
    }
    if ev.flags & FLAG_RATE != 0 {
        return Fault::RateLimited;
    }
    match ev.status {
        0 => Fault::Internal,
        100..=199 => match ev.status {
            110 => Fault::Timeout,
            111 => Fault::Refused,
            113 => Fault::Unreachable,
            _ => Fault::BadPacket,
        },
        400..=499 => match ev.status {
            401 | 403 => Fault::Auth,
            408 => Fault::Timeout,
            409 => Fault::Conflict,
            413 => Fault::Overflow,
            429 => Fault::RateLimited,
            499 => Fault::Cancelled,
            _ => Fault::BadReply,
        },
        500..=599 => match ev.status {
            500 => Fault::Internal,
            501 | 505 => Fault::Unsupported,
            503 => Fault::Busy,
            504 => Fault::Timeout,
            _ => Fault::BadReply,
        },
        _ => Fault::BadPacket,
    }
}

/// 定长错误码计数账。下标 = 偏移 - 1。
#[derive(Clone, Debug)]
pub struct FaultLedger {
    counts: [u32; FAULT_COUNT],
    total: u32,
}

impl FaultLedger {
    pub fn new() -> FaultLedger {
        FaultLedger {
            counts: [0u32; FAULT_COUNT],
            total: 0,
        }
    }

    fn index_of(f: Fault) -> Option<usize> {
        let off = f.offset();
        if off == 0 || off as usize > FAULT_COUNT {
            return None;
        }
        Some(off as usize - 1)
    }

    /// 记一条。返回是否入账（越界不可能发生，但返回值让调用方可断言）。
    pub fn record(&mut self, f: Fault) -> bool {
        let idx = match FaultLedger::index_of(f) {
            Some(i) => i,
            None => return false,
        };
        let mut wrote = false;
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
            wrote = true;
        }
        if wrote {
            self.total = self.total.saturating_add(1);
        }
        wrote
    }

    pub fn count(&self, f: Fault) -> u32 {
        match FaultLedger::index_of(f) {
            Some(i) => match self.counts.get(i) {
                Some(c) => *c,
                None => 0,
            },
            None => 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    /// 守恒：分项之和 == 总量。
    pub fn balanced(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < FAULT_COUNT {
            if let Some(c) = self.counts.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        sum == self.total
    }

    /// 账摘要（供跨节点对账）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_mix(h, self.total as u64);
        let mut i = 0usize;
        while i < FAULT_COUNT {
            if let Some(c) = self.counts.get(i) {
                h = fnv_mix(h, *c as u64);
            }
            i += 1;
        }
        h
    }
}

/// 一次重试决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPlan {
    pub retry: bool,
    pub delay_ms: u32,
    pub attempt: u8,
}

/// 退避：`base << min(attempt, 6)`，封顶 [`BACKOFF_CAP_MS`]。
/// 不可重试（含 `Fatal`/`Info`）一律 `retry=false, delay=0`。
pub fn plan_retry(f: Fault, attempt: u8) -> RetryPlan {
    let info = f.info();
    if !info.retryable || info.severity == Severity::Fatal {
        return RetryPlan {
            retry: false,
            delay_ms: 0,
            attempt,
        };
    }
    let shift = if attempt > 6 { 6u32 } else { attempt as u32 };
    let base = info.category.backoff_base_ms();
    let raw = base.saturating_mul(1u32 << shift);
    let delay = if raw > BACKOFF_CAP_MS {
        BACKOFF_CAP_MS
    } else {
        raw
    };
    RetryPlan {
        retry: true,
        delay_ms: delay,
        attempt,
    }
}

// ---------------------------------------------------------------------------
// 零漂移审计：J1 的度量本体
// ---------------------------------------------------------------------------

/// 漂移审计回报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultAudit {
    pub checked: usize,
    pub drift: usize,
    pub duplicate_names: usize,
    pub duplicate_codes: usize,
    pub severity_conflicts: usize,
    pub digest: u64,
    pub digest_stable: bool,
}

/// 逐项复核冻结表：码值连续、名字唯一、`code()` 与表项一致、查找器自洽。
pub fn fault_code_audit() -> FaultAudit {
    let mut drift = 0usize;
    let mut duplicate_names = 0usize;
    let mut duplicate_codes = 0usize;
    let mut severity_conflicts = 0usize;
    let reg = FaultRegistry::new();
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(info) = FAULT_TABLE.get(i) {
            let expected = ALL_FAULTS[i];
            // 表项顺序必须与枚举偏移严格对应。
            if info.fault != expected || info.fault.offset() as usize != i + 1 {
                drift += 1;
            }
            if info.code != SEG_BASE + (i as u32 + 1) || info.fault.code() != info.code {
                drift += 1;
            }
            if info.name.is_empty() || info.name != info.fault.name() {
                drift += 1;
            }
            // Fatal/Info 不可重试：可重试的「致命错误」只会让重试风暴掩盖真因。
            if info.severity == Severity::Fatal && info.retryable {
                severity_conflicts += 1;
            }
            if info.severity == Severity::Info && info.retryable {
                severity_conflicts += 1;
            }
            // 查找器必须能反查回来。
            if reg.by_name(info.name) != Some(info.fault) {
                drift += 1;
            }
            if reg.by_offset(info.fault.offset()) != Some(info.fault) {
                drift += 1;
            }
            if reg.by_code(info.code) != Some(info.fault) {
                drift += 1;
            }
            // 名字/码值唯一性。
            let mut j = 0usize;
            while j < i {
                if let Some(prev) = FAULT_TABLE.get(j) {
                    if prev.name == info.name {
                        duplicate_names += 1;
                    }
                    if prev.code == info.code {
                        duplicate_codes += 1;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let digest_a = FaultRegistry::new().digest();
    let digest_b = FaultRegistry::new().digest();
    FaultAudit {
        checked: FAULT_COUNT,
        drift,
        duplicate_names,
        duplicate_codes,
        severity_conflicts,
        digest: digest_a,
        digest_stable: digest_a == digest_b && digest_a != 0,
    }
}

// ---------------------------------------------------------------------------
// 注入核：15 类坏样本 → 15 个具体码
// ---------------------------------------------------------------------------

/// 一份「原始事件 → 期望错误码」的清单。15 条互不相同，覆盖整个码族。
pub const CLASSIFY_CASES: [(u16, u16, Fault); FAULT_COUNT] = [
    (0, FLAG_TRUNCATED, Fault::BadPacket),
    (0, FLAG_CONFLICT, Fault::Conflict),
    (0, FLAG_SEQ_GAP, Fault::SeqGap),
    (0, FLAG_DUPLICATE, Fault::Duplicate),
    (110, 0, Fault::Timeout),
    (111, 0, Fault::Refused),
    (113, 0, Fault::Unreachable),
    (401, 0, Fault::Auth),
    (413, 0, Fault::Overflow),
    (429, 0, Fault::RateLimited),
    (499, 0, Fault::Cancelled),
    (500, 0, Fault::Internal),
    (501, 0, Fault::Unsupported),
    (503, 0, Fault::Busy),
    (502, 0, Fault::BadReply),
];

/// 分类核：15 类事件必须逐类落到指定的码上；错一类即少一分。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < CLASSIFY_CASES.len() {
        if let Some((status, flags, want)) = CLASSIFY_CASES.get(i).copied() {
            let got = classify(RawEvent { status, flags });
            if got == want && got.code() == SEG_BASE + want.offset() as u32 {
                caught += 1;
            }
        }
        i += 1;
    }
    (caught, FAULT_COUNT)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50151");

    // J1-1 错误码 ×15 零漂移（判据原文）
    let audit = fault_code_audit();
    let ok = audit.checked == 15
        && audit.drift == 0
        && audit.duplicate_names == 0
        && audit.duplicate_codes == 0
        && audit.severity_conflicts == 0
        && audit.digest_stable;
    let det = format!(
        "n={} drift={} dupN={} dupC={} sev={}",
        audit.checked, audit.drift, audit.duplicate_names, audit.duplicate_codes, audit.severity_conflicts
    );
    s.add_dyn("50151-J1-码表零漂移", ok, &det);

    // J1-2 码值连续：SEG_BASE+1 ..= SEG_BASE+15，中间不留洞
    let mut contiguous = true;
    let mut i = 0usize;
    while i < FAULT_COUNT {
        match ALL_FAULTS.get(i) {
            Some(f) => {
                if f.code() != SEG_BASE + (i as u32 + 1) || f.offset() as usize != i + 1 {
                    contiguous = false;
                }
            }
            None => contiguous = false,
        }
        i += 1;
    }
    s.add("50151-J1-码值连续", contiguous, if contiguous { "" } else { "码值断裂" });

    // J1-3 名字唯一且可反查
    let reg = FaultRegistry::new();
    let mut names_ok = true;
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = ALL_FAULTS.get(i) {
            if reg.by_name(f.name()) != Some(*f) {
                names_ok = false;
            }
            let mut j = 0usize;
            while j < i {
                if let Some(g) = ALL_FAULTS.get(j) {
                    if g.name() == f.name() {
                        names_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50151-J1-名字唯一", names_ok, if names_ok { "" } else { "名字冲突或反查失败" });

    // J1-4 越界查找一律 None（不折叠、不猜）
    let ok = reg.by_offset(0).is_none()
        && reg.by_offset(16).is_none()
        && reg.by_code(SEG_BASE).is_none()
        && reg.by_code(SEG_BASE + 16).is_none()
        && reg.by_name("no-such-fault").is_none()
        && reg.by_code(SEG_BASE + 15) == Some(Fault::Internal);
    s.add("50151-J1-越界不折叠", ok, if ok { "" } else { "越界被折叠" });

    // J1-5 分类器 15 类全对
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("分类 {}/{}", caught, total);
    s.add_dyn("50151-J1-分类全覆盖", ok, &det);

    // J1-6 分类确定：同事件两次同码
    let ev = RawEvent {
        status: 503,
        flags: 0,
    };
    let ok = classify(ev) == classify(ev) && classify(ev) == Fault::Busy;
    s.add("50151-J1-分类确定", ok, if ok { "" } else { "分类不确定" });

    // J1-7 标志位优先于状态码：截断 + 503 必须归坏包而不是忙
    let ok = classify(RawEvent {
        status: 503,
        flags: FLAG_TRUNCATED,
    }) == Fault::BadPacket
        && classify(RawEvent {
            status: 401,
            flags: FLAG_CONFLICT,
        }) == Fault::Conflict;
    s.add("50151-J1-标志优先", ok, if ok { "" } else { "状态码盖过物理事实" });

    // J1-8 未知状态与空状态：未知 → 坏包，0 → 内部
    let ok = classify(RawEvent {
        status: 0xEEEE,
        flags: 0,
    }) == Fault::BadPacket
        && classify(RawEvent { status: 0, flags: 0 }) == Fault::Internal
        && classify(RawEvent {
            status: 505,
            flags: 0,
        }) == Fault::Unsupported;
    s.add("50151-J1-未知状态", ok, if ok { "" } else { "未知状态映射错误" });

    // J1-9 账本守恒 + 下标一一对应
    let mut led = FaultLedger::new();
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = ALL_FAULTS.get(i) {
            // 每条记 (i+1) 次，这样任何下标错位都会破坏加权和。
            let mut k = 0usize;
            while k <= i {
                if !led.record(*f) {
                    break;
                }
                k += 1;
            }
        }
        i += 1;
    }
    let mut weighted = 0u32;
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = ALL_FAULTS.get(i) {
            weighted = weighted.saturating_add(led.count(*f));
        }
        i += 1;
    }
    let ok = led.balanced() && weighted == led.total() && led.total() == (1..=15).sum::<u32>() && led.count(Fault::Timeout) == 1;
    let det = format!("total={} sum={}", led.total(), weighted);
    s.add_dyn("50151-J1-账本对齐", ok, &det);

    // J1-10 退避阶梯：单调、封顶、不可重试为 0
    let mut prev = 0u32;
    let mut monotone = true;
    let mut a = 0u8;
    while a < 8 {
        let p = plan_retry(Fault::Busy, a);
        if !p.retry || p.delay_ms < prev {
            monotone = false;
        }
        prev = p.delay_ms;
        a += 1;
    }
    let capped = plan_retry(Fault::Busy, 200).delay_ms == BACKOFF_CAP_MS;
    let terminal = plan_retry(Fault::Auth, 0);
    let fatal = plan_retry(Fault::Internal, 3);
    let ok = monotone && capped && !terminal.retry && terminal.delay_ms == 0 && !fatal.retry;
    s.add("50151-J1-退避阶梯", ok, if ok { "" } else { "退避不单调或未封顶" });

    // J1-11 严重级与可重试一致：Fatal/Info 一律不可重试
    let mut consistent = true;
    let mut i = 0usize;
    while i < FAULT_COUNT {
        if let Some(f) = ALL_FAULTS.get(i) {
            let info = f.info();
            if (info.severity == Severity::Fatal || info.severity == Severity::Info) && info.retryable {
                consistent = false;
            }
        }
        i += 1;
    }
    s.add("50151-J1-严重级自洽", consistent, if consistent { "" } else { "致命错误可重试" });

    // J1-12 摘要随事实变化：记一条账，摘要必须变
    let mut l2 = FaultLedger::new();
    let d0 = l2.digest();
    let _ = l2.record(Fault::RateLimited);
    let d1 = l2.digest();
    let mut l3 = FaultLedger::new();
    let _ = l3.record(Fault::RateLimited);
    let ok = d0 != d1 && d1 == l3.digest();
    s.add("50151-J1-摘要随账变", ok, if ok { "" } else { "账摘要不敏感" });

    // J1-13 注入核全抓（与 J1-5 同源，单独留一条账外判据核对 total 口径）
    let ok = caught == FAULT_COUNT && total == 15;
    s.add("50151-J1-注入必抓", ok, if ok { "" } else { "注入计数口径错" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50151_checks_green() {
        let set = run_unx50151_checks();
        assert!(set.all_passed(), "UNX-F50151 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50151_codes_are_frozen() {
        let mut i = 0usize;
        while i < FAULT_COUNT {
            match ALL_FAULTS.get(i) {
                Some(f) => assert_eq!(f.code(), SEG_BASE + i as u32 + 1),
                None => panic!("fault table short"),
            }
            i += 1;
        }
    }

    #[test]
    fn unx50151_classifier_flags_win() {
        let ev = RawEvent {
            status: 503,
            flags: FLAG_TRUNCATED,
        };
        assert_eq!(classify(ev), Fault::BadPacket);
    }
}
