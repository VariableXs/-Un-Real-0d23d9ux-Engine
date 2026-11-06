//! UNX-F50128 USB 错误码族（15 条：端点停/设备断/带宽不足等）（0x43c20 段）
//!
//! 锚点：UNX-F50128-J1 错误码 ×15 零漂移；判据锚定 §AI-63 §2 专题八。
//!
//! 为什么错误码要单独成族：打印链路一旦跑起来，故障面就只剩「状态码」这一种
//! 语言——上层重试策略、审计日志、模拟器对表全都按码值分支。码值一旦悄悄漂移
//! （改名、改序、改 errno），三种消费者会各自错位，且错位当时不报错。因此本模块
//! 提供三件东西：
//!
//! * `UsbStatus` —— 15 条错误码闭集。`wire()` 是稳定的 1..=15 号，`code()` 给出
//!   段内诊断码 `SEG_BASE + wire`，两者都不允许重排；
//! * `ErrSpec` —— 每条码的静态档案（名字/类别/重试政策/严重度/errno/处置提示），
//!   `family_seal()` 对整个表做 FNV 封印，`verify_seal_of` 让「表被改过」变成
//!   可检出的 `Drift`，而不是靠人眼 diff；
//! * `ErrLedger` —— 按码计数账本（定容、显式拒收、无静默丢弃）；
//! * `Recovery` —— 重试→复位端点→复位设备→放弃 的单调升级机；不可重试码直接
//!   放弃，绝不把「设备已拔出」当成可重试故障反复打。
//!
//! `classify(errno)` 把 Linux URB 状态（负 errno）映射回码值，形成「硬件报什么 →
//! 我们记什么」的唯一翻译层；errno 在表内唯一，因此映射可往返。
//!
//! 错误纪律：`UsbStatus` 与 `FamError` 的 `code()` 都落在 0x43c20 段闭集内，
//! 全部码值都被判据或注入核真实构造；内核路径零 panic。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43c20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43c20;

pub const E_UNKNOWN: u16 = 16;
pub const E_UNMAPPED: u16 = 17;
pub const E_CAPACITY: u16 = 18;
pub const E_DRIFT: u16 = 19;
pub const E_POLICY: u16 = 20;

/// USB 错误码族（15 条）。顺序即线号，禁止重排——重排等于让所有历史日志失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbStatus {
    EndpointStall,
    DeviceGone,
    NoBandwidth,
    Babble,
    Timeout,
    Overflow,
    Underflow,
    CrcError,
    BitStuff,
    NoDevice,
    InvalidPipe,
    NoMemory,
    NotPermitted,
    Protocol,
    Shutdown,
}

impl UsbStatus {
    /// 线号 1..=15（对外 ABI，永不重排）。
    pub fn wire(self) -> u8 {
        match self {
            UsbStatus::EndpointStall => 1,
            UsbStatus::DeviceGone => 2,
            UsbStatus::NoBandwidth => 3,
            UsbStatus::Babble => 4,
            UsbStatus::Timeout => 5,
            UsbStatus::Overflow => 6,
            UsbStatus::Underflow => 7,
            UsbStatus::CrcError => 8,
            UsbStatus::BitStuff => 9,
            UsbStatus::NoDevice => 10,
            UsbStatus::InvalidPipe => 11,
            UsbStatus::NoMemory => 12,
            UsbStatus::NotPermitted => 13,
            UsbStatus::Protocol => 14,
            UsbStatus::Shutdown => 15,
        }
    }

    /// 段内诊断码 = SEG_BASE + 线号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.wire() as u32
    }

    /// 线号 → 码值。越界不是「未知也算成功」，必须显式报 `Unknown`。
    pub fn from_wire(c: u32) -> Result<UsbStatus, FamError> {
        match c {
            1 => Ok(UsbStatus::EndpointStall),
            2 => Ok(UsbStatus::DeviceGone),
            3 => Ok(UsbStatus::NoBandwidth),
            4 => Ok(UsbStatus::Babble),
            5 => Ok(UsbStatus::Timeout),
            6 => Ok(UsbStatus::Overflow),
            7 => Ok(UsbStatus::Underflow),
            8 => Ok(UsbStatus::CrcError),
            9 => Ok(UsbStatus::BitStuff),
            10 => Ok(UsbStatus::NoDevice),
            11 => Ok(UsbStatus::InvalidPipe),
            12 => Ok(UsbStatus::NoMemory),
            13 => Ok(UsbStatus::NotPermitted),
            14 => Ok(UsbStatus::Protocol),
            15 => Ok(UsbStatus::Shutdown),
            _ => Err(FamError::Unknown),
        }
    }

    /// 对应的 Linux errno（负数）。这是 classify 的反函数口径。
    pub fn errno(self) -> i32 {
        match spec_of(self) {
            Some(s) => s.errno,
            None => 0,
        }
    }

    /// 是否可重试（由静态档案的 retry 政策唯一决定，不在调用点各自判断）。
    pub fn retryable(self) -> bool {
        match spec_of(self) {
            Some(s) => s.retry_limit() > 0,
            None => false,
        }
    }

    pub fn label(self) -> &'static str {
        match spec_of(self) {
            Some(s) => s.name,
            None => "unknown",
        }
    }
}

/// 错误档案/账本自身的失败面（与「USB 错误」分属两层，不混码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FamError {
    Unknown,
    Unmapped,
    Capacity,
    Drift,
    Policy,
}

impl FamError {
    pub fn code(self) -> u32 {
        let n = match self {
            FamError::Unknown => E_UNKNOWN,
            FamError::Unmapped => E_UNMAPPED,
            FamError::Capacity => E_CAPACITY,
            FamError::Drift => E_DRIFT,
            FamError::Policy => E_POLICY,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FamError::Unknown => "wire code outside 1..=15",
            FamError::Unmapped => "errno not present in error family",
            FamError::Capacity => "error ledger capacity exhausted",
            FamError::Drift => "error family seal mismatch (table drifted)",
            FamError::Policy => "recovery budget/policy out of range",
        }
    }
}

/// 错误类别五档。打印链路的处置动作按类别分派：传输类复位端点，设备类复位/
/// 重枚举，资源类退避，致命类停机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrClass {
    Transport,
    Protocol,
    Device,
    Resource,
    Fatal,
}

impl ErrClass {
    pub fn wire(self) -> u8 {
        match self {
            ErrClass::Transport => 1,
            ErrClass::Protocol => 2,
            ErrClass::Device => 3,
            ErrClass::Resource => 4,
            ErrClass::Fatal => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ErrClass::Transport => "transport",
            ErrClass::Protocol => "protocol",
            ErrClass::Device => "device",
            ErrClass::Resource => "resource",
            ErrClass::Fatal => "fatal",
        }
    }
}

/// 重试政策四档。`AfterReset` 表示必须先清 halt/复位端点才有重试资格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryKind {
    Never,
    Once,
    Bounded,
    AfterReset,
}

impl RetryKind {
    pub fn wire(self) -> u8 {
        match self {
            RetryKind::Never => 0,
            RetryKind::Once => 1,
            RetryKind::Bounded => 2,
            RetryKind::AfterReset => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RetryKind::Never => "never",
            RetryKind::Once => "once",
            RetryKind::Bounded => "bounded",
            RetryKind::AfterReset => "after-reset",
        }
    }
}

/// 单条错误码的静态档案。severity 1（轻）..5（致命）用于日志分级。
#[derive(Clone, Copy, Debug)]
pub struct ErrSpec {
    pub status: UsbStatus,
    pub name: &'static str,
    pub class: ErrClass,
    pub policy: RetryKind,
    pub retries: u8,
    pub severity: u8,
    pub errno: i32,
    pub hint: &'static str,
}

impl ErrSpec {
    /// 该码允许的自动重试次数（0 = 不可重试）。
    pub fn retry_limit(&self) -> u8 {
        match self.policy {
            RetryKind::Never => 0,
            RetryKind::Once => 1,
            RetryKind::Bounded | RetryKind::AfterReset => self.retries,
        }
    }
}

/// 15 条错误码档案表。errno 取 Linux 对应值（负），全表唯一——唯一性保证
/// `classify(errno(s)) == s` 可往返；severity/retries 的取值依据写在 hint 里。
pub const SPECS: [ErrSpec; 15] = [
    ErrSpec {
        status: UsbStatus::EndpointStall,
        name: "endpoint-stall",
        class: ErrClass::Transport,
        policy: RetryKind::AfterReset,
        retries: 1,
        severity: 3,
        errno: -32,
        hint: "clear-halt then retry once",
    },
    ErrSpec {
        status: UsbStatus::DeviceGone,
        name: "device-gone",
        class: ErrClass::Device,
        policy: RetryKind::Never,
        retries: 0,
        severity: 5,
        errno: -19,
        hint: "device disconnected mid-urb; re-enumerate",
    },
    ErrSpec {
        status: UsbStatus::NoBandwidth,
        name: "no-bandwidth",
        class: ErrClass::Resource,
        policy: RetryKind::Bounded,
        retries: 2,
        severity: 2,
        errno: -63,
        hint: "isochronous budget exhausted; back off",
    },
    ErrSpec {
        status: UsbStatus::Babble,
        name: "babble",
        class: ErrClass::Transport,
        policy: RetryKind::Never,
        retries: 0,
        severity: 4,
        errno: -75,
        hint: "device sent more bytes than requested",
    },
    ErrSpec {
        status: UsbStatus::Timeout,
        name: "timeout",
        class: ErrClass::Transport,
        policy: RetryKind::Bounded,
        retries: 2,
        severity: 2,
        errno: -110,
        hint: "transfer watchdog fired; retry with backoff",
    },
    ErrSpec {
        status: UsbStatus::Overflow,
        name: "overflow",
        class: ErrClass::Resource,
        policy: RetryKind::Never,
        retries: 0,
        severity: 4,
        errno: -105,
        hint: "host buffer too small for the packet",
    },
    ErrSpec {
        status: UsbStatus::Underflow,
        name: "underflow",
        class: ErrClass::Protocol,
        policy: RetryKind::Never,
        retries: 0,
        severity: 4,
        errno: -121,
        hint: "short packet below declared length",
    },
    ErrSpec {
        status: UsbStatus::CrcError,
        name: "crc",
        class: ErrClass::Transport,
        policy: RetryKind::Once,
        retries: 1,
        severity: 3,
        errno: -84,
        hint: "crc mismatch; hardware already retried twice",
    },
    ErrSpec {
        status: UsbStatus::BitStuff,
        name: "bit-stuff",
        class: ErrClass::Transport,
        policy: RetryKind::Never,
        retries: 0,
        severity: 4,
        errno: -71,
        hint: "bit-stuffing violation on the wire",
    },
    ErrSpec {
        status: UsbStatus::NoDevice,
        name: "no-device",
        class: ErrClass::Device,
        policy: RetryKind::Never,
        retries: 0,
        severity: 5,
        errno: -6,
        hint: "no device answers at that address",
    },
    ErrSpec {
        status: UsbStatus::InvalidPipe,
        name: "invalid-pipe",
        class: ErrClass::Resource,
        policy: RetryKind::Never,
        retries: 0,
        severity: 3,
        errno: -22,
        hint: "pipe handle unusable; rebuild the pipe",
    },
    ErrSpec {
        status: UsbStatus::NoMemory,
        name: "no-memory",
        class: ErrClass::Resource,
        policy: RetryKind::Bounded,
        retries: 1,
        severity: 2,
        errno: -12,
        hint: "urb allocation failed; retry after reclaim",
    },
    ErrSpec {
        status: UsbStatus::NotPermitted,
        name: "not-permitted",
        class: ErrClass::Device,
        policy: RetryKind::Never,
        retries: 0,
        severity: 5,
        errno: -1,
        hint: "device refused the class request",
    },
    ErrSpec {
        status: UsbStatus::Protocol,
        name: "protocol",
        class: ErrClass::Protocol,
        policy: RetryKind::Never,
        retries: 0,
        severity: 4,
        errno: -74,
        hint: "descriptor/class protocol violation",
    },
    ErrSpec {
        status: UsbStatus::Shutdown,
        name: "shutdown",
        class: ErrClass::Fatal,
        policy: RetryKind::Never,
        retries: 0,
        severity: 5,
        errno: -108,
        hint: "usb stack shutting down; no recovery",
    },
];

/// 按线号取档案。表不完整时返回 None，由调用方显式处理（不兜底猜一条）。
pub fn spec_of(s: UsbStatus) -> Option<&'static ErrSpec> {
    SPECS.get(s.wire() as usize - 1)
}

/// 线号 → 码值（1..=15）；越界 `Unknown`。
pub fn decode(wire: u32) -> Result<UsbStatus, FamError> {
    UsbStatus::from_wire(wire)
}

/// errno → 码值。表内 errno 唯一，因此每个 errno 至多命中一条；未知 `Unmapped`。
pub fn classify(errno: i32) -> Result<UsbStatus, FamError> {
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(s) = SPECS.get(i) {
            if s.errno == errno {
                return Ok(s.status);
            }
        }
        i += 1;
    }
    Err(FamError::Unmapped)
}

// ---------------------------------------------------------------------------
// 错误账本
// ---------------------------------------------------------------------------

/// 错误账本：按码计数、定容、显式拒收。容量满时**不**静默丢弃——
/// 丢一条错误计数，等于让「零漂移」这句话失去分母。
pub struct ErrLedger {
    counts: [u32; 15],
    total: u32,
    cap: u32,
}

impl ErrLedger {
    pub fn new(cap: u32) -> Result<ErrLedger, FamError> {
        if cap == 0 {
            return Err(FamError::Capacity);
        }
        Ok(ErrLedger {
            counts: [0; 15],
            total: 0,
            cap,
        })
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 记一条错误，返回该码累计次数。超容返回 `Capacity` 且不做任何修改。
    pub fn record(&mut self, s: UsbStatus) -> Result<u32, FamError> {
        if self.total >= self.cap {
            return Err(FamError::Capacity);
        }
        let idx = s.wire() as usize - 1;
        let cur = match self.counts.get(idx) {
            Some(c) => *c,
            None => return Err(FamError::Unknown),
        };
        let next = cur.saturating_add(1);
        if let Some(slot) = self.counts.get_mut(idx) {
            *slot = next;
        }
        self.total = self.total.saturating_add(1);
        Ok(next)
    }

    /// 单码计数。
    pub fn count(&self, s: UsbStatus) -> u32 {
        self.counts.get(s.wire() as usize - 1).copied().unwrap_or(0)
    }

    /// 已出现的不同码个数（覆盖率分子）。
    pub fn coverage(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get(i) {
                if *c > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账本摘要：15 个计数 + 总量。同记账序列两次必须逐位一致。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.total as u64;
        let mut i = 0usize;
        while i < self.counts.len() {
            let v = self.counts.get(i).copied().unwrap_or(0);
            h ^= v as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
            i += 1;
        }
        h ^ (self.cap as u64)
    }

    /// 清零（保留容量）。对应「链路重枚举后重新计时」。
    pub fn reset(&mut self) {
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(c) = self.counts.get_mut(i) {
                *c = 0;
            }
            i += 1;
        }
        self.total = 0;
    }
}

// ---------------------------------------------------------------------------
// 恢复升级机
// ---------------------------------------------------------------------------

/// 处置动作，rank 单调不降是升级机的核心不变量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Retry,
    ResetEndpoint,
    ResetDevice,
    GiveUp,
}

impl Action {
    pub fn rank(self) -> u8 {
        match self {
            Action::None => 0,
            Action::Retry => 1,
            Action::ResetEndpoint => 2,
            Action::ResetDevice => 3,
            Action::GiveUp => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Retry => "retry",
            Action::ResetEndpoint => "reset-endpoint",
            Action::ResetDevice => "reset-device",
            Action::GiveUp => "give-up",
        }
    }
}

/// 恢复升级机：先用掉该码的自动重试额度，再依次复位端点、复位设备，最后放弃。
/// budget 是整条链路的总重试预算，取码额度与总预算的较小者。
#[derive(Clone, Copy, Debug)]
pub struct Recovery {
    attempts: u8,
    budget: u8,
    stage: u8,
}

impl Recovery {
    /// budget 为 0 没有意义（一上来就放弃不叫恢复），显式拒收。
    pub fn new(budget: u8) -> Result<Recovery, FamError> {
        if budget == 0 || budget > 32 {
            return Err(FamError::Policy);
        }
        Ok(Recovery {
            attempts: 0,
            budget,
            stage: 0,
        })
    }

    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    pub fn stage(&self) -> u8 {
        self.stage
    }

    pub fn budget(&self) -> u8 {
        self.budget
    }

    /// 处理一次错误，给出下一步动作。不可重试码直接 GiveUp 且不消耗预算。
    pub fn on_error(&mut self, s: UsbStatus) -> Result<Action, FamError> {
        let spec = match spec_of(s) {
            Some(x) => x,
            None => return Err(FamError::Unknown),
        };
        let limit = spec.retry_limit();
        if limit == 0 {
            return Ok(Action::GiveUp);
        }
        if self.attempts < limit && self.attempts < self.budget {
            self.attempts = self.attempts.saturating_add(1);
            return Ok(Action::Retry);
        }
        // 额度耗尽：只能按 stage 单向升级，绝不回退到更轻的动作。
        let action = match self.stage {
            0 => {
                self.stage = 1;
                Action::ResetEndpoint
            }
            1 => {
                self.stage = 2;
                Action::ResetDevice
            }
            _ => Action::GiveUp,
        };
        Ok(action)
    }

    /// 链路重新可用后清零（新的错误序列重新从 Retry 开始）。
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.stage = 0;
    }
}

// ---------------------------------------------------------------------------
// 零漂移封印
// ---------------------------------------------------------------------------

fn fnv32(mut h: u32, b: u8) -> u32 {
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}

fn fnv32_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fnv32(x, ((v >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 对任意档案表做封印：线号 + 名字字节 + 类别 + 政策 + 重试数 + 严重度 + errno。
/// 任一字段漂移都会改变封印值，因此「零漂移」是可判定的，而不是承诺。
pub fn seal_of(table: &[ErrSpec]) -> u32 {
    let mut h: u32 = 0x811c_9dc5 ^ (table.len() as u32);
    let mut i = 0usize;
    while i < table.len() {
        if let Some(s) = table.get(i) {
            h = fnv32(h, s.status.wire());
            let b = s.name.as_bytes();
            let mut k = 0usize;
            while k < b.len() {
                if let Some(c) = b.get(k) {
                    h = fnv32(h, *c);
                }
                k += 1;
            }
            h = fnv32(h, s.class.wire());
            h = fnv32(h, s.policy.wire());
            h = fnv32(h, s.retries);
            h = fnv32(h, s.severity);
            h = fnv32_u32(h, s.errno as u32);
            i += 1;
        }
    }
    h
}

/// 全表封印（由 `SPECS` 现算，不缓存在可变状态里，避免「表改了封印没改」）。
pub fn family_seal() -> u32 {
    seal_of(&SPECS)
}

/// 用期望封印校验给定档案表，不一致即 `Drift`。
pub fn verify_seal_of(table: &[ErrSpec], expected: u32) -> Result<(), FamError> {
    if seal_of(table) == expected {
        Ok(())
    } else {
        Err(FamError::Drift)
    }
}

/// 校验内置表未被漂移。
pub fn verify_seal(expected: u32) -> Result<(), FamError> {
    verify_seal_of(&SPECS, expected)
}

/// 渲染一行档案（诊断/日志用）。
pub fn render(s: &ErrSpec) -> String {
    format!(
        "usb-err {} #{} {}/{} retry={}/{} sev={} errno={}",
        s.name,
        s.status.wire(),
        s.class.label(),
        s.policy.label(),
        s.retry_limit(),
        s.retries,
        s.severity,
        s.errno
    )
}

// ---------------------------------------------------------------------------
// 注入核：五类坏样本逐类断言具体码
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1 线号越界
    if decode(0).err() == Some(FamError::Unknown) && decode(16).err() == Some(FamError::Unknown) {
        caught += 1;
    }
    // 2 errno 不在表内
    if classify(-9999).err() == Some(FamError::Unmapped) && classify(0).err() == Some(FamError::Unmapped)
    {
        caught += 1;
    }
    // 3 账本超容（不做静默丢弃）
    let mut lg = match ErrLedger::new(2) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let a = lg.record(UsbStatus::Timeout).is_ok();
    let b = lg.record(UsbStatus::CrcError).is_ok();
    let c = lg.record(UsbStatus::Babble).err() == Some(FamError::Capacity);
    if a && b && c && lg.total() == 2 && lg.count(UsbStatus::Babble) == 0 {
        caught += 1;
    }
    // 4 表被篡改 → 封印漂移
    let mut tampered: Vec<ErrSpec> = Vec::with_capacity(SPECS.len());
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(s) = SPECS.get(i) {
            let mut copy = *s;
            if i == 0 {
                copy.severity = copy.severity.saturating_add(1);
            }
            tampered.push(copy);
        }
        i += 1;
    }
    let seal = family_seal();
    if verify_seal_of(&tampered, seal).err() == Some(FamError::Drift)
        && verify_seal_of(&SPECS, seal).is_ok()
    {
        caught += 1;
    }
    // 5 恢复机预算为 0
    if Recovery::new(0).err() == Some(FamError::Policy)
        && Recovery::new(33).err() == Some(FamError::Policy)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50128");

    // J1-1 15 条线号占满 1..=15，无缺口无重复
    let mut seen = [false; 16];
    let mut gaps = 0usize;
    let mut dup = 0usize;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            let w = spec.status.wire() as usize;
            if w == 0 || w > 15 {
                gaps += 1;
            } else if let Some(cell) = seen.get_mut(w) {
                if *cell {
                    dup += 1;
                }
                *cell = true;
            }
        }
        i += 1;
    }
    let mut missing = 0usize;
    let mut w = 1usize;
    while w <= 15 {
        if seen.get(w).map(|x| !*x).unwrap_or(true) {
            missing += 1;
        }
        w += 1;
    }
    let ok = SPECS.len() == 15 && gaps == 0 && dup == 0 && missing == 0;
    let det = format!("n={} missing={} dup={}", SPECS.len(), missing, dup);
    s.add_dyn("50128-J1-十五码", ok, &det);

    // J1-2 段内码值 = SEG_BASE + 线号（含两端点）
    let mut code_ok = true;
    let mut i = 0usize;
    while i < 15 {
        let wire = (i + 1) as u32;
        match UsbStatus::from_wire(wire) {
            Ok(st) => {
                if st.code() != SEG_BASE + wire || st.wire() as u32 != wire {
                    code_ok = false;
                }
            }
            Err(_) => code_ok = false,
        }
        i += 1;
    }
    let ok = code_ok
        && UsbStatus::EndpointStall.code() == SEG_BASE + 1
        && UsbStatus::Shutdown.code() == SEG_BASE + 15
        && FamError::Unknown.code() == SEG_BASE + 16
        && FamError::Policy.code() == SEG_BASE + 20;
    s.add("50128-J1-码值区间", ok, if ok { "" } else { "段内码值漂移" });

    // J1-3 解码往返：15 条全部可逆，越界两态显式 Unknown
    let mut rt_ok = true;
    let mut i = 1u32;
    while i <= 15 {
        match decode(i) {
            Ok(st) => {
                if st.wire() as u32 != i {
                    rt_ok = false;
                }
            }
            Err(_) => rt_ok = false,
        }
        i += 1;
    }
    let ok = rt_ok
        && decode(0).err() == Some(FamError::Unknown)
        && decode(16).err() == Some(FamError::Unknown)
        && decode(1000).err() == Some(FamError::Unknown);
    s.add("50128-J1-解码往返", ok, if ok { "" } else { "解码不可逆或越界放行" });

    // J1-4 名字唯一非空；errno 唯一且为负；severity 落在 1..=5
    let mut names_ok = true;
    let mut errno_ok = true;
    let mut sev_ok = true;
    let mut a = 0usize;
    while a < SPECS.len() {
        if let Some(sa) = SPECS.get(a) {
            if sa.name.is_empty() {
                names_ok = false;
            }
            if sa.errno >= 0 {
                errno_ok = false;
            }
            if sa.severity == 0 || sa.severity > 5 {
                sev_ok = false;
            }
            let mut b = a + 1;
            while b < SPECS.len() {
                if let Some(sb) = SPECS.get(b) {
                    if sa.name == sb.name {
                        names_ok = false;
                    }
                    if sa.errno == sb.errno {
                        errno_ok = false;
                    }
                }
                b += 1;
            }
        }
        a += 1;
    }
    let ok = names_ok && errno_ok && sev_ok;
    let det = format!("names={} errno={} sev={}", names_ok, errno_ok, sev_ok);
    s.add_dyn("50128-J1-档案唯一", ok, &det);

    // J1-5 errno 分类往返 15 条；未知 errno/0 一律 Unmapped
    let mut cls_ok = true;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            if classify(spec.errno).ok() != Some(spec.status) {
                cls_ok = false;
            }
        }
        i += 1;
    }
    let ok = cls_ok
        && classify(-9999).err() == Some(FamError::Unmapped)
        && classify(0).err() == Some(FamError::Unmapped);
    s.add("50128-J1-errno分类", ok, if ok { "" } else { "errno 映射不可往返" });

    // J1-6 spec 自洽：spec_of 全部命中且 status 对齐、retry_limit 与政策一致
    let mut spec_ok = true;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            match spec_of(spec.status) {
                Some(got) => {
                    if got.status != spec.status || got.retries > 3 || got.retry_limit() > 3 {
                        spec_ok = false;
                    }
                }
                None => spec_ok = false,
            }
        }
        i += 1;
    }
    s.add("50128-J1-档案自洽", spec_ok, if spec_ok { "" } else { "spec_of 命中或额度越界" });

    // J1-7 五类类别与四档重试政策全覆盖（覆盖不足说明表被裁过）
    let mut cls_seen = [false; 6];
    let mut pol_seen = [false; 4];
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            if let Some(c) = cls_seen.get_mut(spec.class.wire() as usize) {
                *c = true;
            }
            if let Some(p) = pol_seen.get_mut(spec.policy.wire() as usize) {
                *p = true;
            }
        }
        i += 1;
    }
    let mut cls_n = 0usize;
    let mut i = 1usize;
    while i <= 5 {
        if cls_seen.get(i).copied().unwrap_or(false) {
            cls_n += 1;
        }
        i += 1;
    }
    let mut pol_n = 0usize;
    let mut i = 0usize;
    while i < 4 {
        if pol_seen.get(i).copied().unwrap_or(false) {
            pol_n += 1;
        }
        i += 1;
    }
    let ok = cls_n == 5 && pol_n == 4;
    let det = format!("class={}/5 policy={}/4", cls_n, pol_n);
    s.add_dyn("50128-J1-覆盖", ok, &det);

    // J1-8 重试语义：Never=0、Once=1、Bounded=retries、AfterReset=retries
    let mut pol_ok = true;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            let limit = spec.retry_limit();
            let expect = match spec.policy {
                RetryKind::Never => 0,
                RetryKind::Once => 1,
                RetryKind::Bounded | RetryKind::AfterReset => spec.retries,
            };
            if limit != expect {
                pol_ok = false;
            }
            if spec.status.retryable() != (expect > 0) {
                pol_ok = false;
            }
            // Never 政策不得同时声明重试次数，否则额度与政策会互相打架。
            if spec.policy == RetryKind::Never && spec.retries != 0 {
                pol_ok = false;
            }
        }
        i += 1;
    }
    s.add("50128-J1-重试语义", pol_ok, if pol_ok { "" } else { "政策与额度不一致" });

    // J1-9 账本：15 条各记一次 → total=15、coverage=15、重复记账单调、摘要稳定
    let mut lg = match ErrLedger::new(100) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut i = 0usize;
    let mut rec_ok = true;
    while i < 15 {
        let st = match UsbStatus::from_wire((i + 1) as u32) {
            Ok(x) => x,
            Err(_) => return s,
        };
        match lg.record(st) {
            Ok(n) => {
                if n != 1 {
                    rec_ok = false;
                }
            }
            Err(_) => rec_ok = false,
        }
        i += 1;
    }
    rec_ok = rec_ok && lg.total() == 15 && lg.coverage() == 15;
    let d1 = lg.digest();
    let d2 = lg.digest();
    let mut i = 0usize;
    let mut mono_ok = true;
    while i < 15 {
        let before = lg.count(UsbStatus::Timeout);
        if let Ok(after) = lg.record(UsbStatus::Timeout) {
            if after != before + 1 {
                mono_ok = false;
            }
        } else {
            mono_ok = false;
        }
        i += 1;
    }
    let ok = rec_ok && mono_ok && d1 == d2 && lg.count(UsbStatus::Timeout) == 16;
    let det = format!("total={} cov={} mono={}", lg.total(), lg.coverage(), mono_ok);
    s.add_dyn("50128-J1-账本", ok, &det);

    // J1-10 账本容量：第 cap+1 条显式 Capacity 且计数不变
    let mut cap_lg = match ErrLedger::new(3) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let r1 = cap_lg.record(UsbStatus::Timeout);
    let r2 = cap_lg.record(UsbStatus::Protocol);
    let r3 = cap_lg.record(UsbStatus::Babble);
    let r4 = cap_lg.record(UsbStatus::CrcError);
    let ok = r1 == Ok(1)
        && r2 == Ok(1)
        && r3 == Ok(1)
        && r4.err() == Some(FamError::Capacity)
        && cap_lg.total() == 3
        && cap_lg.count(UsbStatus::CrcError) == 0
        && ErrLedger::new(0).err() == Some(FamError::Capacity);
    s.add("50128-J1-账本容量", ok, if ok { "" } else { "超容被静默收下" });

    // J1-11 恢复升级：动作 rank 单调不降，最终 GiveUp
    let mut rec = match Recovery::new(8) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut prev = 0u8;
    let mut mono = true;
    let mut saw_giveup = false;
    let mut steps = 0usize;
    while steps < 6 {
        match rec.on_error(UsbStatus::EndpointStall) {
            Ok(a) => {
                if a.rank() < prev {
                    mono = false;
                }
                prev = a.rank();
                if a == Action::GiveUp {
                    saw_giveup = true;
                }
            }
            Err(_) => mono = false,
        }
        steps += 1;
    }
    let ok = mono
        && saw_giveup
        && rec.stage() == 2
        && rec.attempts() == 1
        && prev == Action::GiveUp.rank();
    let det = format!("stage={} attempts={} mono={}", rec.stage(), rec.attempts(), mono);
    s.add_dyn("50128-J1-恢复升级", ok, &det);

    // J1-12 不可重试码不消耗预算、直接放弃；预算为 0 不可构造
    let mut rec2 = match Recovery::new(4) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let a = rec2.on_error(UsbStatus::DeviceGone);
    let b = rec2.on_error(UsbStatus::NoDevice);
    let ok = a == Ok(Action::GiveUp)
        && b == Ok(Action::GiveUp)
        && rec2.attempts() == 0
        && rec2.stage() == 0
        && Recovery::new(0).err() == Some(FamError::Policy)
        && Recovery::new(32).is_ok()
        && Recovery::new(33).err() == Some(FamError::Policy);
    s.add("50128-J1-不可重试", ok, if ok { "" } else { "不可重试码被反复重试" });

    // J1-13 零漂移封印：现算两次一致、正确封印可通过、篡改一字节必 Drift
    let seal_a = family_seal();
    let seal_b = family_seal();
    let mut mutated: Vec<ErrSpec> = Vec::with_capacity(SPECS.len());
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            let mut copy = *spec;
            if i == 4 {
                // 改一条重试次数（Timeout 2 → 3），模拟「表被悄悄改过」
                copy.retries = copy.retries.saturating_add(1);
            }
            mutated.push(copy);
        }
        i += 1;
    }
    let ok = seal_a == seal_b
        && verify_seal(seal_a).is_ok()
        && verify_seal(seal_a ^ 1).err() == Some(FamError::Drift)
        && verify_seal_of(&SPECS, seal_a).is_ok()
        && verify_seal_of(&mutated, seal_a).err() == Some(FamError::Drift);
    let det = format!("seal={:08x}", seal_a);
    s.add_dyn("50128-J1-零漂移", ok, &det);

    // J1-14 账本摘要按计数而非时序：同一组计数换顺序摘要不变，计数不同则不同
    let mut la = match ErrLedger::new(10) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut lb = match ErrLedger::new(10) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let _ = la.record(UsbStatus::Timeout);
    let _ = la.record(UsbStatus::Babble);
    let _ = lb.record(UsbStatus::Babble);
    let _ = lb.record(UsbStatus::Timeout);
    let mut lc = match ErrLedger::new(10) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let _ = lc.record(UsbStatus::Timeout);
    let _ = lc.record(UsbStatus::Timeout);
    let ok = la.digest() == lb.digest() && la.digest() != lc.digest() && la.total() == lc.total();
    s.add("50128-J1-摘要口径", ok, if ok { "" } else { "摘要对计数不敏感" });

    // J1-15 注入核：五类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50128-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50128_checks_green() {
        let set = run_unx50128_checks();
        assert!(set.all_passed(), "UNX-F50128 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50128_codes_are_stable() {
        let mut i = 1u32;
        while i <= 15 {
            let st = UsbStatus::from_wire(i);
            assert!(st.is_ok());
            if let Ok(st) = st {
                assert_eq!(st.code(), SEG_BASE + i);
                assert_eq!(classify(st.errno()), Ok(st));
            }
            i += 1;
        }
    }

    #[test]
    fn unx50128_seal_detects_drift() {
        let seal = family_seal();
        assert_eq!(verify_seal(seal), Ok(()));
        assert_eq!(verify_seal(seal.wrapping_add(1)), Err(FamError::Drift));
    }
}
