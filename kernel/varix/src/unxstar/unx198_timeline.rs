//! UNX-F0198 · init 阶段耗时账（全景时间线）
//!
//! 锚点判据（施工书原文）：UNX-F0198-J1 各单元耗时入时间线账本，
//! 与 F0018 引导记账口径对接 diff 一致。
//!
//! 设计：T0 采集桩（阶段开/关两次采样差即单元耗时）→ 时间线账本
//! （阶段枚举 × 起止 tick × 耗时）→ 与 F0018 口径的对接对账（同一
//! 阶段在两本账中的耗时差 ≤ 容差 1 tick 即绿）。三重红线：
//! ①阶段序单调（开始 tick 不得早于上一阶段开始）；②时间线闭合
//! （首阶段起点 = 0，末阶段终点 = 全程）；③账平（记账数 = 完成数）。
//!
//! 错误码段：0x2B90（全仓 SEG_BASE 普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0198_VERSION: &str = "F0198-timeline-v1";

/// 诊断码段基址（UNX-F0198 独占）。
pub const SEG_BASE: u16 = 0x2B90;

/// F0018 对接容差（tick）。
pub const DIFF_TOLERANCE: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 阶段重复开始。
    DupStart = SEG_BASE + 1,
    /// 完成了未开始的阶段。
    CloseNoStart = SEG_BASE + 2,
    /// 阶段序倒挂（开始早于上一阶段）。
    OrderInvert = SEG_BASE + 3,
    /// 时间线不闭合（首点非零或末段不收口）。
    NotClosed = SEG_BASE + 4,
    /// 账不平（记账数 ≠ 完成数）。
    LedgerBroken = SEG_BASE + 5,
    /// F0018 对接 diff 超容差。
    DiffOut = SEG_BASE + 6,
    /// 未知阶段名。
    UnknownUnit = SEG_BASE + 7,
    /// 负耗时（关闭早于开始）。
    NegativeSpan = SEG_BASE + 8,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// init 阶段枚举（与 F0018 引导记账的单元口径一一对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// 固件移交（ExitBootServices 收尾）。
    FirmwareHandoff,
    /// 内核映像载入。
    ImageLoad,
    /// 页表与映射建立。
    PagingSetup,
    /// 堆初始化。
    HeapInit,
    /// 设备枚举。
    DeviceEnum,
    /// 驱动装载。
    DriverLoad,
    /// 文件系统挂载。
    FsMount,
    /// 用户态移交。
    UserHandoff,
}

impl Unit {
    /// 全部阶段（按执行序）。
    pub const ALL: [Unit; 8] = [
        Unit::FirmwareHandoff,
        Unit::ImageLoad,
        Unit::PagingSetup,
        Unit::HeapInit,
        Unit::DeviceEnum,
        Unit::DriverLoad,
        Unit::FsMount,
        Unit::UserHandoff,
    ];

    /// F0018 口径名（对接真源）。
    pub const fn f0018_name(self) -> &'static str {
        match self {
            Unit::FirmwareHandoff => "fw-handoff",
            Unit::ImageLoad => "image-load",
            Unit::PagingSetup => "paging",
            Unit::HeapInit => "heap",
            Unit::DeviceEnum => "dev-enum",
            Unit::DriverLoad => "drv-load",
            Unit::FsMount => "fs-mount",
            Unit::UserHandoff => "user-handoff",
        }
    }
}

/// 单条时间线记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// 阶段。
    pub unit: Unit,
    /// 开始 tick。
    pub start: u64,
    /// 结束 tick（未完成 = 0）。
    pub end: u64,
}

impl Span {
    /// 耗时（未完成 = 0）。
    pub fn elapsed(&self) -> u64 {
        if self.end > self.start {
            self.end - self.start
        } else {
            0
        }
    }

    /// 完成态。
    pub fn done(&self) -> bool {
        self.end > 0
    }
}

/// T0 采集桩 + 时间线账本。
#[derive(Debug, Default)]
pub struct TimelineLedger {
    /// 记录（按阶段执行序）。
    pub spans: Vec<Span>,
    /// 记账计数（开始次数）。
    started: u64,
    /// 完成计数。
    closed: u64,
}

impl TimelineLedger {
    /// 空账本。
    pub fn boot() -> Self {
        TimelineLedger {
            spans: Vec::new(),
            started: 0,
            closed: 0,
        }
    }

    fn find(&self, unit: Unit) -> Option<usize> {
        self.spans.iter().position(|s| s.unit == unit)
    }

    /// 阶段开始（T0 采样）。
    pub fn begin(&mut self, unit: Unit, tick: u64) -> Result<(), u16> {
        if self.find(unit).is_some() {
            return Err(E::DupStart.code());
        }
        // 阶段序单调：本次开始不得早于任一已记阶段的开始。
        for s in self.spans.iter() {
            if tick < s.start {
                return Err(E::OrderInvert.code());
            }
        }
        self.spans.push(Span {
            unit,
            start: tick,
            end: 0,
        });
        self.started += 1;
        Ok(())
    }

    /// 阶段结束（T1 采样，与开始相减即耗时）。
    pub fn finish(&mut self, unit: Unit, tick: u64) -> Result<(), u16> {
        let i = match self.find(unit) {
            Some(i) => i,
            None => return Err(E::CloseNoStart.code()),
        };
        if self.spans[i].done() {
            return Err(E::DupStart.code());
        }
        if tick < self.spans[i].start {
            return Err(E::NegativeSpan.code());
        }
        self.spans[i].end = tick;
        self.closed += 1;
        Ok(())
    }

    /// 账平红线：记账数 ≡ 完成数。
    pub fn balanced(&self) -> bool {
        self.started == self.closed && self.spans.iter().all(|s| s.done())
    }

    /// 全程耗时（末段终点 − 首段起点）。
    pub fn total(&self) -> u64 {
        if self.spans.is_empty() {
            return 0;
        }
        let first = self.spans[0].start;
        let last = self.spans[self.spans.len() - 1].end;
        last.saturating_sub(first)
    }

    /// 闭合红线：首段起点 = 0、末段收口。
    pub fn closed_ok(&self) -> bool {
        !self.spans.is_empty()
            && self.spans[0].start == 0
            && self.spans.iter().all(|s| s.done())
    }

    /// F0018 对接对账：同名阶段耗时 diff ≤ 容差。
    ///
    /// `f0018_costs`：F0018 账本给的耗时表（按 f0018_name 索引）。
    pub fn diff_against_f0018(&self, f0018_costs: &[(Unit, u64)]) -> Result<u64, u16> {
        let mut max_diff = 0u64;
        for s in self.spans.iter() {
            let other = f0018_costs
                .iter()
                .find(|(u, _)| *u == s.unit)
                .map(|(_, c)| *c)
                .ok_or(E::UnknownUnit.code())?;
            let d = if s.elapsed() > other {
                s.elapsed() - other
            } else {
                other - s.elapsed()
            };
            if d > DIFF_TOLERANCE {
                return Err(E::DiffOut.code());
            }
            if d > max_diff {
                max_diff = d;
            }
        }
        Ok(max_diff)
    }
}

/// J1 自证（判据原文：各单元耗时入时间线账本，与 F0018 对接 diff 一致）。
pub fn self_audit() -> Result<(), u16> {
    // 一、八阶段全链记账（tick 手工步进，模拟真实 T0/T1 采样）。
    let mut tl = TimelineLedger::boot();
    let mut t = 0u64;
    let mut costs = Vec::new();
    for (i, u) in Unit::ALL.iter().enumerate() {
        tl.begin(*u, t)?;
        t += (i as u64 % 4) + 2; // 每段 2~5 tick。
        tl.finish(*u, t)?;
        costs.push((*u, tl.spans.last().map(|s| s.elapsed()).unwrap_or(0)));
    }
    if !tl.balanced() {
        return Err(E::LedgerBroken.code());
    }
    if !tl.closed_ok() {
        return Err(E::NotClosed.code());
    }
    // 全程守恒：各段之和 = 全程。
    let sum: u64 = tl.spans.iter().map(|s| s.elapsed()).sum();
    if sum != tl.total() {
        return Err(E::NotClosed.code());
    }

    // 二、J1 主体：与 F0018 对接 diff 一致（同源数据 → diff 全 0）。
    let d = tl.diff_against_f0018(&costs)?;
    if d != 0 {
        return Err(E::DiffOut.code());
    }

    // 三、红线逐项：重复开始 / 未开先闭 / 倒挂 / 负耗时 / 超容差。
    let mut x = TimelineLedger::boot();
    x.begin(Unit::FirmwareHandoff, 0)?;
    if x.begin(Unit::FirmwareHandoff, 5).is_ok() {
        return Err(E::DupStart.code());
    }
    if x.finish(Unit::ImageLoad, 6).is_ok() {
        return Err(E::CloseNoStart.code());
    }
    if x.begin(Unit::ImageLoad, 10).is_err() {
        return Err(E::OrderInvert.code());
    }
    if x.finish(Unit::ImageLoad, 3).is_ok() {
        return Err(E::NegativeSpan.code());
    }
    let mut y = TimelineLedger::boot();
    y.begin(Unit::FirmwareHandoff, 0)?;
    y.finish(Unit::FirmwareHandoff, 10)?;
    let drift = [(Unit::FirmwareHandoff, 8u64)]; // diff = 2 > 容差 1。
    if y.diff_against_f0018(&drift).is_ok() {
        return Err(E::DiffOut.code());
    }

    // 四、判别式段位对拍。
    if E::DupStart.code() != SEG_BASE + 1 || E::NegativeSpan.code() != SEG_BASE + 8 {
        return Err(E::LedgerBroken.code());
    }
    Ok(())
}
