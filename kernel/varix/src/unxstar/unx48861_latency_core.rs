//! UNX-F48861 中断到达段打点 · 0x70780
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥440 行；
//!   ② UNX-F48861-J1 第一段 ×10^3 打点全落账；
//!   ③ UNX-F48862-J1 第二段 ×10^3 打点全落账。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：段号合法 → 跨度非负 → 同核 → 频换算 → 结果码 → 分位排序。
//!           次序不可倒：段号未判就把样本塞进四个桶，坏段号会落进
//!           末桶冒充「帧呈现」；跨度未判先排序，负数（回退）会被
//!           当成 0 参与 P50，把一次时钟回退洗成「很快」；同核未判
//!           就做差，跨核未校准的偏移会被当成真实延迟；频换算在
//!           分位之前，是因为分位只认微秒，换算错误会把整条分布
//!           平移而不报错。
//! 关键差异：本模块只拥有「四段数值模型 + 分位/直方图」这一层——
//!           不判单调性闸（guard 的四闸）、不管 TSC 权威源与跨核
//!           校准（meta 的 OffsetBad/AuthBad）、不落环不导出
//!           （dispatch）。本模块给出 SegId/Span/SegmentSample 与
//!           Quartet，向上三层都按这里的定义取值；谁绕过本模块
//!           自己算分位，谁就要自己承担「nearest-rank 与插值混用」
//!           的口径漂移。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70780，VX6 批 B04 族首槽）。
pub const SEG_BASE: u32 = 0x70780;

/// 四段冻结：中断到达 / vxwm 焦点分发 / 应用回调 / 帧呈现。
pub const SEG_COUNT: usize = 4;

/// 判据轮数：每段 ×10^3 打点。
pub const CHAIN_ROUNDS: u32 = 1000;

/// TSC 名义频率（2 GHz 不变 TSC，与 A2 时钟基座同口径）。
pub const TSC_HZ: u64 = 2_000_000_000;

/// 每秒纳秒数（换算常量，不随基座变化）。
pub const NS_PER_SEC: u64 = 1_000_000_000;

/// 每段预算（µs）：越过即进入 warning，只用于诊断文案分级。
pub const BUDGET_US: [u32; SEG_COUNT] = [80, 240, 900, 2600];

/// 每段基值（µs）：黄金链合成用的最小耗时。
pub const FLOOR_US: [u32; SEG_COUNT] = [18, 55, 180, 640];

/// 每段抖动上限（µs）：LCG 在 [0, 上限) 内加抖。
pub const JITTER_US: [u32; SEG_COUNT] = [40, 120, 420, 1200];

/// 分位冻结：P50 / P95 / P99。
pub const PERCENTILES: [u8; 3] = [50, 95, 99];

/// 直方图分桶边界（µs，升序），桶数为边界数 + 1。
pub const HIST_EDGES_US: [u32; 7] = [80, 240, 500, 900, 1600, 2600, 5000];

/// 账本硬容量（core 只做模型校验，环容量在 dispatch）。
pub const CORE_CAP: usize = 4096;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0A） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LCode {
    /// 段号越出 0..=3
    SegBad,
    /// 跨度反序（end < start）
    SpanBad,
    /// 时钟值非法（零频、换算溢出、零跨度要求非零）
    ClockBad,
    /// 样本字段非法（序号回退、容量已满）
    SampleBad,
    /// 分位参数非法（空集或 p 不在 1..=100）
    QuantBad,
    /// 段链不贯通（缺段/乱序/不连续）
    ChainBad,
    /// 频率换算不成立（TSC_HZ 为零或结果溢出 u64）
    FreqBad,
    /// 跨核做差：两端 CPU 不同且未给校准偏移
    CpuBad,
    /// 双跑复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl LCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            LCode::SegBad => "段号非法：四段模型只允许 0..=3",
            LCode::SpanBad => "跨度反序：结束 TSC 小于起始 TSC",
            LCode::ClockBad => "时钟值非法：换算溢出或频点为零",
            LCode::SampleBad => "样本非法：序号回退、字段缺失或账本已满",
            LCode::QuantBad => "分位非法：空样本集或百分位不在 1..=100",
            LCode::ChainBad => "段链不贯通：缺段、乱序或首尾不相接",
            LCode::FreqBad => "频率换算不成立：频点为零或纳秒溢出 u64",
            LCode::CpuBad => "跨核做差未校准：两端 CPU 不同",
            LCode::ReplayBad => "复算不一致：两次黄金全链结果不同",
            LCode::SegmentBad => "段位越界：诊断码逃出 0x70780 窗口",
        }
    }

    pub const ALL: [LCode; 10] = [
        LCode::SegBad,
        LCode::SpanBad,
        LCode::ClockBad,
        LCode::SampleBad,
        LCode::QuantBad,
        LCode::ChainBad,
        LCode::FreqBad,
        LCode::CpuBad,
        LCode::ReplayBad,
        LCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外返回显式文案（不静默）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < LCode::ALL.len() {
        if let Some(c) = LCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70780 段闭集内"
}

// ── 四段模型 ──────────────────────────────────────────────────────
/// 四段编号。ordon 即打点次序，也是黄金链的拼接次序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegId {
    /// 第一段：中断到达 → 事件层入队
    IrqArrival,
    /// 第二段：入队 → vxwm 焦点分发
    FocusDispatch,
    /// 第三段：分发 → 应用回调
    AppCallback,
    /// 第四段：回调 → 帧呈现
    FramePresent,
}

impl SegId {
    pub fn ord(self) -> u8 {
        match self {
            SegId::IrqArrival => 0,
            SegId::FocusDispatch => 1,
            SegId::AppCallback => 2,
            SegId::FramePresent => 3,
        }
    }

    pub fn from_ord(v: u8) -> Result<SegId, u32> {
        match v {
            0 => Ok(SegId::IrqArrival),
            1 => Ok(SegId::FocusDispatch),
            2 => Ok(SegId::AppCallback),
            3 => Ok(SegId::FramePresent),
            _ => Err(LCode::SegBad.val()),
        }
    }

    /// 中文段名（导出与诊断文案的唯一来源）。
    pub fn label(self) -> &'static str {
        match self {
            SegId::IrqArrival => "中断到达",
            SegId::FocusDispatch => "焦点分发",
            SegId::AppCallback => "应用回调",
            SegId::FramePresent => "帧呈现",
        }
    }

    /// 机器可读段名（CSV 列名用，全小写无空格）。
    pub fn stage(self) -> &'static str {
        match self {
            SegId::IrqArrival => "irq-arrival",
            SegId::FocusDispatch => "focus-dispatch",
            SegId::AppCallback => "app-callback",
            SegId::FramePresent => "frame-present",
        }
    }

    /// 下一段；末段无后继（链收口点）。
    pub fn next(self) -> Option<SegId> {
        match self {
            SegId::IrqArrival => Some(SegId::FocusDispatch),
            SegId::FocusDispatch => Some(SegId::AppCallback),
            SegId::AppCallback => Some(SegId::FramePresent),
            SegId::FramePresent => None,
        }
    }

    /// 本段预算（µs）。
    pub fn budget_us(self) -> u32 {
        match BUDGET_US.get(self.ord() as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 本段基值（µs）。
    pub fn floor_us(self) -> u32 {
        match FLOOR_US.get(self.ord() as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 本段抖动上限（µs）。
    pub fn jitter_us(self) -> u32 {
        match JITTER_US.get(self.ord() as usize) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 样本结果码：只有 Delivered 计入「成功段」，其余一律带原因入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegResult {
    Delivered,
    Timeout,
    Dropped,
    Bypassed,
    Rejected,
}

impl SegResult {
    pub fn ord(self) -> u8 {
        match self {
            SegResult::Delivered => 0,
            SegResult::Timeout => 1,
            SegResult::Dropped => 2,
            SegResult::Bypassed => 3,
            SegResult::Rejected => 4,
        }
    }

    pub fn from_ord(v: u8) -> Result<SegResult, u32> {
        match v {
            0 => Ok(SegResult::Delivered),
            1 => Ok(SegResult::Timeout),
            2 => Ok(SegResult::Dropped),
            3 => Ok(SegResult::Bypassed),
            4 => Ok(SegResult::Rejected),
            _ => Err(LCode::SampleBad.val()),
        }
    }

    /// 只有真正送达才算成功；Bypassed 是「无账本旁路」，绝不算成功。
    pub fn is_delivered(self) -> bool {
        self == SegResult::Delivered
    }

    pub fn text(self) -> &'static str {
        match self {
            SegResult::Delivered => "delivered",
            SegResult::Timeout => "timeout",
            SegResult::Dropped => "dropped",
            SegResult::Bypassed => "bypassed",
            SegResult::Rejected => "rejected",
        }
    }
}

/// 单个 TSC 读数：值 + 采样核。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    pub tsc: u64,
    pub cpu: u8,
}

impl Stamp {
    pub fn new(tsc: u64, cpu: u8) -> Stamp {
        Stamp { tsc, cpu }
    }

    /// 本核前进 `delta` tick；溢出拒。
    pub fn advance(self, delta: u64) -> Result<Stamp, u32> {
        if delta == 0 {
            return Err(LCode::ClockBad.val());
        }
        match self.tsc.checked_add(delta) {
            Some(v) => Ok(Stamp { tsc: v, cpu: self.cpu }),
            None => Err(LCode::ClockBad.val()),
        }
    }

    pub fn same_core(self, other: Stamp) -> bool {
        self.cpu == other.cpu
    }
}

/// 一段的起止：跨度是所有分位与阈值判据的原子量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: Stamp,
    pub end: Stamp,
}

impl Span {
    pub fn new(start: Stamp, end: Stamp) -> Span {
        Span { start, end }
    }

    /// 同核跨度（tick）。反序拒；跨核拒（必须走校准版）。
    pub fn elapsed_ticks(self) -> Result<u64, u32> {
        if !self.start.same_core(self.end) {
            return Err(LCode::CpuBad.val());
        }
        if self.end.tsc < self.start.tsc {
            return Err(LCode::SpanBad.val());
        }
        Ok(self.end.tsc - self.start.tsc)
    }

    /// 跨核跨度：两端各给校准偏移（tick，可负），校正后再做差。
    ///
    /// 偏移语义：`corrected = raw + off`。先把两端都搬到参考核时间轴，
    /// 再比较；未校准（偏移为 0 且核不同）由调用方保证——本函数只做
    /// 算术，不做权威判定（权威判定在 meta 的 TimeBase）。
    pub fn elapsed_ticks_cross(self, off_start: i64, off_end: i64) -> Result<u64, u32> {
        let a = apply_offset(self.start.tsc, off_start)?;
        let b = apply_offset(self.end.tsc, off_end)?;
        if b < a {
            return Err(LCode::SpanBad.val());
        }
        Ok(b - a)
    }

    /// 同核跨度（ns）。
    pub fn elapsed_ns(self) -> Result<u64, u32> {
        let t = self.elapsed_ticks()?;
        ticks_to_ns(t)
    }

    /// 同核跨度（µs，向下取整——账本对外一律微秒）。
    pub fn elapsed_us(self) -> Result<u64, u32> {
        let ns = self.elapsed_ns()?;
        Ok(ns / 1000)
    }

    /// 本段是否越过预算。
    pub fn over_budget(self, seg: SegId) -> Result<bool, u32> {
        let us = self.elapsed_us()?;
        Ok(us > seg.budget_us() as u64)
    }

    pub fn text(self) -> String {
        format!(
            "[{}@{} -> {}@{}]",
            self.start.tsc, self.start.cpu, self.end.tsc, self.end.cpu
        )
    }
}

/// 带符号偏移应用：负偏移走 checked_sub，正偏移走 checked_add。
pub fn apply_offset(raw: u64, off: i64) -> Result<u64, u32> {
    if off >= 0 {
        match raw.checked_add(off as u64) {
            Some(v) => Ok(v),
            None => Err(LCode::ClockBad.val()),
        }
    } else {
        // i64::MIN 取绝对值会溢出，先拒绝这一端。
        match off.checked_neg() {
            Some(neg) => match raw.checked_sub(neg as u64) {
                Some(v) => Ok(v),
                None => Err(LCode::ClockBad.val()),
            },
            None => Err(LCode::ClockBad.val()),
        }
    }
}

// ── 频率换算（TSC tick ↔ ns ↔ µs） ────────────────────────────────
/// tick → ns：以 u128 中继，先乘后除；结果溢出 u64 即拒。
pub fn ticks_to_ns(ticks: u64) -> Result<u64, u32> {
    if TSC_HZ == 0 {
        return Err(LCode::FreqBad.val());
    }
    let scaled = (ticks as u128).saturating_mul(NS_PER_SEC as u128);
    let ns = scaled / (TSC_HZ as u128);
    if ns > u64::MAX as u128 {
        return Err(LCode::FreqBad.val());
    }
    Ok(ns as u64)
}

/// tick → µs（ns 再除 1000，两次取整等价于一次除法，但保留下界语义清晰）。
pub fn ticks_to_us(ticks: u64) -> Result<u64, u32> {
    let ns = ticks_to_ns(ticks)?;
    Ok(ns / 1000)
}

/// µs → tick：用于黄金链合成与阈值反算。
pub fn us_to_ticks(us: u64) -> Result<u64, u32> {
    if TSC_HZ == 0 {
        return Err(LCode::FreqBad.val());
    }
    let scaled = (us as u128).saturating_mul(TSC_HZ as u128);
    let t = scaled / 1_000_000u128;
    if t > u64::MAX as u128 {
        return Err(LCode::FreqBad.val());
    }
    Ok(t as u64)
}

/// ppm 误差：|measured - expected| / expected，单位百万分之一。
pub fn ppm_error(expected: u64, measured: u64) -> Result<u32, u32> {
    if expected == 0 {
        return Err(LCode::ClockBad.val());
    }
    let diff = if measured >= expected {
        measured - expected
    } else {
        expected - measured
    };
    let scaled = (diff as u128).saturating_mul(1_000_000u128) / (expected as u128);
    if scaled > u32::MAX as u128 {
        return Err(LCode::FreqBad.val());
    }
    Ok(scaled as u32)
}

// ── 分位引擎（nearest-rank，口径唯一） ────────────────────────────
/// 升序插入排序（就地，不分配）。n 小（≤ 4096），O(n²) 可接受且确定性好。
pub fn sort_ticks(v: &mut Vec<u64>) {
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let key = match v.get(i) {
            Some(x) => *x,
            None => return,
        };
        let mut j = i;
        while j > 0 {
            let prev = match v.get(j - 1) {
                Some(x) => *x,
                None => break,
            };
            if prev <= key {
                break;
            }
            if let Some(slot) = v.get_mut(j) {
                *slot = prev;
            }
            j -= 1;
        }
        if let Some(slot) = v.get_mut(j) {
            *slot = key;
        }
        i += 1;
    }
}

/// nearest-rank 分位：rank = ceil(p·n/100)，取第 rank 个（1 基）。
///
/// 为什么必须钉死一种口径：P95 用插值法比 nearest-rank 小半格，
/// 快照与全量重算只要混用就会「同数据两个 P95」。本族一律用
/// nearest-rank，guard 的快照复算也调这里。
pub fn percentile_nearest_rank(sorted: &[u64], p: u8) -> Result<u64, u32> {
    if p == 0 || p > 100 {
        return Err(LCode::QuantBad.val());
    }
    let n = sorted.len();
    if n == 0 {
        return Err(LCode::QuantBad.val());
    }
    let scaled = (p as u128).saturating_mul(n as u128);
    let mut rank = (scaled.saturating_add(99) / 100) as usize;
    if rank == 0 {
        rank = 1;
    }
    if rank > n {
        rank = n;
    }
    match sorted.get(rank - 1) {
        Some(v) => Ok(*v),
        None => Err(LCode::QuantBad.val()),
    }
}

/// 四分位集：n / p50 / p95 / p99 / min / max。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quartet {
    pub n: u32,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
}

impl Quartet {
    /// 从**已排序**切片求四分位。空集拒；单样本三档同值。
    pub fn of(sorted: &[u64]) -> Result<Quartet, u32> {
        if sorted.is_empty() {
            return Err(LCode::QuantBad.val());
        }
        let p50 = percentile_nearest_rank(sorted, 50)?;
        let p95 = percentile_nearest_rank(sorted, 95)?;
        let p99 = percentile_nearest_rank(sorted, 99)?;
        let min = match sorted.first() {
            Some(v) => *v,
            None => return Err(LCode::QuantBad.val()),
        };
        let max = match sorted.last() {
            Some(v) => *v,
            None => return Err(LCode::QuantBad.val()),
        };
        Ok(Quartet {
            n: sorted.len() as u32,
            p50,
            p95,
            p99,
            min,
            max,
        })
    }

    /// 三档单调：这是分位引擎唯一的结构不变量。
    pub fn monotone(&self) -> bool {
        self.min <= self.p50 && self.p50 <= self.p95 && self.p95 <= self.p99 && self.p99 <= self.max
    }

    /// 尾比：p99 / p50（放大 100 倍取整），用于长尾识别。
    pub fn tail_ratio_x100(&self) -> u64 {
        if self.p50 == 0 {
            return 0;
        }
        self.p99.saturating_mul(100) / self.p50
    }

    pub fn text(&self) -> String {
        format!(
            "n={} p50={} p95={} p99={} min={} max={}",
            self.n, self.p50, self.p95, self.p99, self.min, self.max
        )
    }
}

// ── 直方图（微秒分桶，落桶数为桶边界 + 1） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    pub counts: [u32; 8],
    pub total: u32,
}

impl Histogram {
    pub fn new() -> Histogram {
        Histogram { counts: [0; 8], total: 0 }
    }

    /// 落桶：第一个 edges[i] >= us 的桶；越界进末桶（长尾桶）。
    pub fn bucket_of(us: u64) -> usize {
        let mut i = 0usize;
        while i < HIST_EDGES_US.len() {
            if let Some(edge) = HIST_EDGES_US.get(i) {
                if us <= *edge as u64 {
                    return i;
                }
            }
            i += 1;
        }
        HIST_EDGES_US.len()
    }

    pub fn add(&mut self, us: u64) {
        let idx = Self::bucket_of(us);
        if let Some(slot) = self.counts.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    /// 峰值桶（并列取靠前者）。
    pub fn peak(&self) -> usize {
        let mut best = 0usize;
        let mut best_v = 0u32;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(v) = self.counts.get(i) {
                if *v > best_v {
                    best_v = *v;
                    best = i;
                }
            }
            i += 1;
        }
        best
    }

    /// 桶和必须等于总数——账不平即样本丢。
    pub fn balanced(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.counts.len() {
            if let Some(v) = self.counts.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum == self.total
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.counts.len() {
            if i > 0 {
                out.push(',');
            }
            let c = self.counts.get(i).copied().unwrap_or(0);
            let cell = format!("b{}={}", i, c);
            out.push_str(cell.as_str());
            i += 1;
        }
        out
    }
}

// ── 样本与账本 ────────────────────────────────────────────────────
/// 一条段样本：序号把四段绑成一条链，seg 决定它在链上的位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentSample {
    pub seq: u32,
    pub seg: SegId,
    pub span: Span,
    pub result: SegResult,
}

impl SegmentSample {
    pub fn new(seq: u32, seg: SegId, span: Span, result: SegResult) -> SegmentSample {
        SegmentSample { seq, seg, span, result }
    }

    /// 本样本耗时（µs）；反序/跨核在这里第一次被拒。
    pub fn elapsed_us(self) -> Result<u64, u32> {
        self.span.elapsed_us()
    }

    /// 是否计入「成功分布」：必须 Delivered 且跨度合法。
    pub fn countable(self) -> bool {
        self.result.is_delivered() && self.elapsed_us().is_ok()
    }

    pub fn text(self) -> String {
        format!(
            "#{} {} {} {} {}us",
            self.seq,
            self.seg.stage(),
            self.span.text(),
            self.result.text(),
            self.elapsed_us().unwrap_or(u64::MAX)
        )
    }
}

/// 核心账本：按段归集样本，按需算分位与直方图。
pub struct CoreLedger {
    samples: Vec<SegmentSample>,
    cap: usize,
    rejected: u32,
    last_seq: u32,
}

impl CoreLedger {
    pub fn new(cap: usize) -> CoreLedger {
        CoreLedger {
            samples: Vec::new(),
            cap: if cap == 0 { CORE_CAP } else { cap },
            rejected: 0,
            last_seq: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 落账：段号合法、跨度可算、序号不回退（同序号允许四段连续）。
    pub fn record(&mut self, s: SegmentSample) -> Result<(), u32> {
        if self.samples.len() >= self.cap {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LCode::SampleBad.val());
        }
        if s.seq == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LCode::SampleBad.val());
        }
        if s.seq < self.last_seq {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LCode::SampleBad.val());
        }
        // 跨度反序/跨核在这里被拒，不允许「先入账后修」。
        let _ = s.elapsed_us()?;
        self.samples.push(s);
        self.last_seq = s.seq;
        Ok(())
    }

    /// 某段全部样本（含非成功样本，用于计数对账）。
    pub fn count_of(&self, seg: SegId) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.seg == seg {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 四段计数向量。
    pub fn coverage(&self) -> [u32; SEG_COUNT] {
        let mut out = [0u32; SEG_COUNT];
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                let idx = s.seg.ord() as usize;
                if let Some(cell) = out.get_mut(idx) {
                    *cell = cell.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 结果码分布（按 SegResult 的 ord）。
    pub fn result_profile(&self) -> [u32; 5] {
        let mut out = [0u32; 5];
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                let idx = s.result.ord() as usize;
                if let Some(cell) = out.get_mut(idx) {
                    *cell = cell.saturating_add(1);
                }
            }
            i += 1;
        }
        out
    }

    /// 取某段「成功样本」的耗时（µs），已排序；空集返回 Err。
    pub fn durations_us(&self, seg: SegId) -> Result<Vec<u64>, u32> {
        let mut v: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.seg == seg && s.countable() {
                    let us = s.elapsed_us()?;
                    v.push(us);
                }
            }
            i += 1;
        }
        if v.is_empty() {
            return Err(LCode::QuantBad.val());
        }
        sort_ticks(&mut v);
        Ok(v)
    }

    /// 某段四分位。
    pub fn stats(&self, seg: SegId) -> Result<Quartet, u32> {
        let v = self.durations_us(seg)?;
        Quartet::of(&v)
    }

    /// 某段直方图。
    pub fn histogram(&self, seg: SegId) -> Result<Histogram, u32> {
        let v = self.durations_us(seg)?;
        let mut h = Histogram::new();
        let mut i = 0usize;
        while i < v.len() {
            if let Some(us) = v.get(i) {
                h.add(*us);
            }
            i += 1;
        }
        Ok(h)
    }

    /// 全账本四分位（四段各一份）。
    pub fn all_stats(&self) -> Result<[Quartet; SEG_COUNT], u32> {
        let mut out = [Quartet { n: 0, p50: 0, p95: 0, p99: 0, min: 0, max: 0 }; SEG_COUNT];
        let mut s = 0u8;
        while s < SEG_COUNT as u8 {
            let seg = SegId::from_ord(s)?;
            let q = self.stats(seg)?;
            if let Some(slot) = out.get_mut(s as usize) {
                *slot = q;
            }
            s += 1;
        }
        Ok(out)
    }

    /// 预算越界计数（按段）。
    pub fn over_budget(&self) -> [u32; SEG_COUNT] {
        let mut out = [0u32; SEG_COUNT];
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.countable() {
                    if let Ok(us) = s.elapsed_us() {
                        if us > s.seg.budget_us() as u64 {
                            let idx = s.seg.ord() as usize;
                            if let Some(cell) = out.get_mut(idx) {
                                *cell = cell.saturating_add(1);
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        out
    }
}

// ── 段链审计（四段首尾相接） ──────────────────────────────────────
/// 一条链 = 四段样本，seq 相同、段号 0..3 各一次、时间首尾相接。
pub fn chain_audit(chain: &[SegmentSample]) -> Result<u64, u32> {
    if chain.len() != SEG_COUNT {
        return Err(LCode::ChainBad.val());
    }
    let mut expect = 0u8;
    let mut prev_end: Option<Stamp> = None;
    let mut total: u64 = 0;
    let mut i = 0usize;
    while i < chain.len() {
        let s = match chain.get(i) {
            Some(v) => v,
            None => return Err(LCode::ChainBad.val()),
        };
        if s.seg.ord() != expect {
            return Err(LCode::ChainBad.val());
        }
        if !s.result.is_delivered() {
            return Err(LCode::ChainBad.val());
        }
        let ticks = s.span.elapsed_ticks()?;
        if let Some(prev) = prev_end {
            if prev.tsc != s.span.start.tsc || prev.cpu != s.span.start.cpu {
                return Err(LCode::ChainBad.val());
            }
        }
        total = total.saturating_add(ticks);
        prev_end = Some(s.span.end);
        expect = expect.saturating_add(1);
        i += 1;
    }
    Ok(total)
}

/// 按基础 TSC 与四段 tick 时长构造一条合法链（测试与黄金链共用）。
pub fn build_chain(seq: u32, base: u64, ticks: [u64; SEG_COUNT]) -> Result<Vec<SegmentSample>, u32> {
    let mut out: Vec<SegmentSample> = Vec::new();
    let mut cursor = Stamp::new(base, 0);
    let mut i = 0usize;
    while i < SEG_COUNT {
        let seg = SegId::from_ord(i as u8)?;
        let d = match ticks.get(i) {
            Some(v) => *v,
            None => return Err(LCode::SampleBad.val()),
        };
        if d == 0 {
            return Err(LCode::ClockBad.val());
        }
        let end = cursor.advance(d)?;
        out.push(SegmentSample::new(seq, seg, Span::new(cursor, end), SegResult::Delivered));
        cursor = end;
        i += 1;
    }
    Ok(out)
}

// ── LCG（本域独立盐，避免与相邻模块同流） ─────────────────────────
/// 打点合成器：只用于黄金链与注入轮的**输入生成**，不参与判定。
pub struct StampRng(u64);

impl StampRng {
    pub fn new(seed: u64) -> StampRng {
        StampRng(seed ^ 0x4C41_5445_4E43_5931)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金全链 ──────────────────────────────────────────────────────
/// 黄金结果（可比较，双跑一致靠它）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreGolden {
    pub samples: u32,
    pub chains: u32,
    pub per_seg: [u32; SEG_COUNT],
    pub p50_us: [u64; 3],
    pub p99_us: [u64; 2],
    pub cross_core: u32,
    pub rejected: u32,
}

/// 黄金全链：1000 条四段链，逐段落账，再算分位。
pub fn golden_run() -> Result<CoreGolden, u32> {
    let mut led = CoreLedger::new(CORE_CAP);
    let mut rng = StampRng::new(SEG_BASE as u64);
    let mut seq = 1u32;
    let mut base: u64 = match us_to_ticks(1_000) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let mut chains = 0u32;
    let mut cross_core = 0u32;
    let mut r = 0u32;
    while r < CHAIN_ROUNDS {
        let mut cursor = Stamp::new(base, 0);
        let mut s = 0u8;
        while s < SEG_COUNT as u8 {
            let seg = SegId::from_ord(s)?;
            let jit = rng.below(seg.jitter_us() as u64);
            let us = seg.floor_us() as u64 + jit;
            let ticks = us_to_ticks(us)?;
            let end = cursor.advance(ticks)?;
            let span = Span::new(cursor, end);
            // 每 40 条链用校准路径复算一次跨度：偏移为零时必须同值。
            if r % 40 == 0 {
                let a = span.elapsed_ticks()?;
                let b = span.elapsed_ticks_cross(0, 0)?;
                if a != b {
                    return Err(LCode::ReplayBad.val());
                }
                cross_core = cross_core.saturating_add(1);
            }
            // 每 61 条链的第三段模拟一次超时：仍入账，但不进成功分布。
            let result = if (r % 61) == 0 && seg == SegId::AppCallback {
                SegResult::Timeout
            } else {
                SegResult::Delivered
            };
            let sample = SegmentSample::new(seq, seg, span, result);
            led.record(sample)?;
            cursor = end;
            s += 1;
        }
        // 链尾再前进 10µs，下一条链不与上一条重叠。
        base = cursor.tsc.saturating_add(match us_to_ticks(10) {
            Ok(v) => v,
            Err(e) => return Err(e),
        });
        seq = seq.saturating_add(1);
        chains = chains.saturating_add(1);
        r += 1;
    }
    let per_seg = led.coverage();
    let q0 = led.stats(SegId::IrqArrival)?;
    let q1 = led.stats(SegId::FocusDispatch)?;
    let q2 = led.stats(SegId::AppCallback)?;
    let q3 = led.stats(SegId::FramePresent)?;
    Ok(CoreGolden {
        samples: led.len() as u32,
        chains,
        per_seg,
        p50_us: [q0.p50, q1.p50, q2.p50],
        p99_us: [q0.p99, q3.p99],
        cross_core,
        rejected: led.rejected(),
    })
}

// ── 注入账（四路：反序跨度/跨核未校准/分位非法/链不贯通） ────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if let Some(t) = self.tries.get(i) {
                sum = sum.saturating_add(*t);
            }
            i += 1;
        }
        sum
    }
}

/// 每轮四路注入，全部必须被拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = StampRng::new(SEG_BASE as u64 ^ 0x1C);
    let mut r = 0u32;
    while r < rounds {
        let jitter = rng.below(97);

        // 路 1：结束 TSC 小于起始（时钟回退伪装成负延迟）。
        let a = Stamp::new(5_000 + jitter, 0);
        let b = Stamp::new(4_000 + jitter, 0);
        let span_bad = matches!(Span::new(a, b).elapsed_ticks(), Err(e) if e == LCode::SpanBad.val());
        led.record(0, span_bad);

        // 路 2：跨核未校准（同值两端，核不同）。
        let c = Stamp::new(9_000 + jitter, 0);
        let d = Stamp::new(9_000 + jitter, 1);
        let cpu_bad = matches!(Span::new(c, d).elapsed_ticks(), Err(e) if e == LCode::CpuBad.val());
        led.record(1, cpu_bad);

        // 路 3：分位参数非法（p=0 与空集两处都要拒）。
        let p_bad = percentile_nearest_rank(&[1, 2, 3], 0).is_err()
            && percentile_nearest_rank(&[], 95).is_err()
            && percentile_nearest_rank(&[1, 2, 3], 101).is_err();
        led.record(2, p_bad);

        // 路 4：链不贯通（缺段 / 乱序）。
        let mut short: Vec<SegmentSample> = Vec::new();
        if let Ok(chain) = build_chain(1 + jitter as u32, 1_000, [10, 20, 30, 40]) {
            let mut k = 0usize;
            while k < 3 {
                if let Some(s) = chain.get(k) {
                    short.push(*s);
                }
                k += 1;
            }
        }
        let mut swapped: Vec<SegmentSample> = Vec::new();
        if let Ok(chain) = build_chain(2 + jitter as u32, 2_000, [10, 20, 30, 40]) {
            if let (Some(s0), Some(s1), Some(s2), Some(s3)) =
                (chain.get(0), chain.get(1), chain.get(2), chain.get(3))
            {
                // 首两段互换：段号次序被破坏，链审计必须拒。
                swapped.push(*s1);
                swapped.push(*s0);
                swapped.push(*s2);
                swapped.push(*s3);
            }
        }
        let chain_bad = chain_audit(&short).is_err() && chain_audit(&swapped).is_err();
        led.record(3, chain_bad);

        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 序 A：黄金全链两跑同值，且每段恰好 1000 条落账。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LCode::ReplayBad.val());
    }
    if a.chains != CHAIN_ROUNDS || a.samples != CHAIN_ROUNDS * SEG_COUNT as u32 {
        return Err(LCode::SampleBad.val());
    }
    let mut i = 0usize;
    while i < SEG_COUNT {
        if a.per_seg.get(i).copied().unwrap_or(0) != CHAIN_ROUNDS {
            return Err(LCode::SampleBad.val());
        }
        i += 1;
    }
    Ok(())
}

/// 序 B：四类非法输入逐类拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if SegId::from_ord(4).err() != Some(LCode::SegBad.val()) {
        return Err(LCode::SegBad.val());
    }
    let bad = Span::new(Stamp::new(10, 0), Stamp::new(9, 0));
    if bad.elapsed_ticks().err() != Some(LCode::SpanBad.val()) {
        return Err(LCode::SpanBad.val());
    }
    if Quartet::of(&[]).err() != Some(LCode::QuantBad.val()) {
        return Err(LCode::QuantBad.val());
    }
    if us_to_ticks(0) != Ok(0) {
        return Err(LCode::FreqBad.val());
    }
    Ok(())
}

/// 序 C：冻结常量（四段 / 千轮 / 2GHz / 7 桶边界 / 3 分位）。
pub fn probe_seq_c() -> Result<(), u32> {
    if SEG_COUNT == 4
        && CHAIN_ROUNDS == 1000
        && TSC_HZ == 2_000_000_000
        && HIST_EDGES_US.len() == 7
        && PERCENTILES.len() == 3
        && BUDGET_US.len() == 4
    {
        Ok(())
    } else {
        Err(LCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫：诊断码全部落在 [SEG_BASE, SEG_BASE + 0x20) 且互异。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < LCode::ALL.len() {
        let a = match LCode::ALL.get(i) {
            Some(c) => *c,
            None => return false,
        };
        let va = a.val();
        if va < SEG_BASE || va >= SEG_BASE + 0x20 {
            return false;
        }
        let mut j = i + 1;
        while j < LCode::ALL.len() {
            if let Some(b) = LCode::ALL.get(j) {
                if b.val() == va {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();

    // ① 第一段千轮全落账。
    let seg0 = match &g {
        Ok(x) => x.per_seg.get(0).copied().unwrap_or(0),
        Err(_) => 0,
    };
    v.push((
        "F48861-J1-首段千轮全落账",
        seg0 == CHAIN_ROUNDS,
        match &g {
            Ok(x) => format!("seg0={} chains={} samples={}", seg0, x.chains, x.samples),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    // ② 第二段千轮全落账。
    let seg1 = match &g {
        Ok(x) => x.per_seg.get(1).copied().unwrap_or(0),
        Err(_) => 0,
    };
    v.push((
        "F48862-J1-次段千轮全落账",
        seg1 == CHAIN_ROUNDS,
        match &g {
            Ok(x) => format!("seg1={} p50={}us", seg1, x.p50_us.get(1).copied().unwrap_or(0)),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    // ③ 四段模型冻结：段号稠密、标签互异、后继链完整。
    let mut dense = true;
    let mut i = 0u8;
    while i < SEG_COUNT as u8 {
        match SegId::from_ord(i) {
            Ok(s) => {
                if s.ord() != i || s.label().is_empty() || s.stage().is_empty() {
                    dense = false;
                }
                if i + 1 < SEG_COUNT as u8 && s.next().is_none() {
                    dense = false;
                }
                if i + 1 == SEG_COUNT as u8 && s.next().is_some() {
                    dense = false;
                }
            }
            Err(_) => dense = false,
        }
        i += 1;
    }
    v.push((
        "F48861-J1-四段模型冻结",
        dense,
        String::from("段号 0..=3 稠密，标签/机器名/后继链完整"),
    ));

    // ④ 分位次序与尾比（四段各自单调）。
    let stats = match &g {
        Ok(_) => {
            let mut led = CoreLedger::new(CORE_CAP);
            let mut ok = true;
            let mut s = 0u8;
            while s < SEG_COUNT as u8 {
                if let Ok(seg) = SegId::from_ord(s) {
                    let base = 1_000u64 + s as u64 * 500;
                    // 每段给一条递增时长链（≥100µs，避免 µs 截断成 0）。
                    let d = 200_000u64 + seg.ord() as u64 * 40_000;
                    if let Ok(chain) = build_chain(100 + s as u32, base, [d, d * 2, d * 4, d * 8]) {
                        let mut k = 0usize;
                        while k < chain.len() {
                            if let Some(sample) = chain.get(k) {
                                if led.record(*sample).is_err() {
                                    ok = false;
                                }
                            }
                            k += 1;
                        }
                    }
                }
                s += 1;
            }
            let mut quartets: Vec<Quartet> = Vec::new();
            let mut s2 = 0u8;
            while s2 < SEG_COUNT as u8 {
                if let Ok(seg) = SegId::from_ord(s2) {
                    if let Ok(q) = led.stats(seg) {
                        quartets.push(q);
                    }
                }
                s2 += 1;
            }
            Ok((ok, quartets))
        }
        Err(e) => Err(*e),
    };
    let (stat_ok, stat_detail) = match stats {
        Ok((ok, qs)) => {
            let mut monotone = ok && qs.len() == SEG_COUNT;
            let mut tailless = true;
            let mut k = 0usize;
            while k < qs.len() {
                if let Some(q) = qs.get(k) {
                    if !q.monotone() {
                        monotone = false;
                    }
                    if q.tail_ratio_x100() < 100 {
                        tailless = false;
                    }
                }
                k += 1;
            }
            (
                monotone && tailless,
                format!("quartets={} 三档单调且 p99/p50>=1", qs.len()),
            )
        }
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(e))),
    };
    v.push(("F48862-J1-分位三档单调", stat_ok, stat_detail));

    // ⑤ 频率换算零漂：2GHz 下 2000 tick 恰为 1µs。
    let freq_ok = ticks_to_us(2_000) == Ok(1)
        && ticks_to_ns(2_000) == Ok(1_000)
        && us_to_ticks(1_000) == Ok(2_000_000)
        && ppm_error(1_000, 1_000) == Ok(0)
        && ppm_error(1, u64::MAX).is_err();
    v.push((
        "F48862-J1-频率换算零漂",
        freq_ok,
        format!(
            "2000tick={:?}us 1000us={:?}tick ppm_err={}",
            ticks_to_us(2_000).ok(),
            us_to_ticks(1_000).ok(),
            ppm_error(1, u64::MAX).is_err()
        ),
    ));

    // ⑥ 直方图桶账平衡。
    let hist = match &g {
        Ok(_) => {
            let mut led = CoreLedger::new(CORE_CAP);
            if let Ok(chain) = build_chain(7, 4_000, [200_000, 400_000, 800_000, 1_600_000]) {
                let mut i2 = 0usize;
                while i2 < chain.len() {
                    if let Some(sample) = chain.get(i2) {
                        let _ = led.record(*sample);
                    }
                    i2 += 1;
                }
            }
            led.histogram(SegId::FramePresent)
        }
        Err(e) => Err(*e),
    };
    let (hist_ok, hist_detail) = match &hist {
        Ok(h) => (
            h.balanced() && h.total == 1,
            format!("total={} {} peak=b{}", h.total, h.text(), h.peak()),
        ),
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(*e))),
    };
    v.push(("F48861-J1-直方图桶账平衡", hist_ok, hist_detail));

    // ⑦ 注入四路 10/10 被拒。
    let inj = inject_rounds(10);
    v.push((
        "F48862-J1-注入四路全拒",
        match &inj {
            Ok(l) => l.all_caught() && l.total() == 40,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X} {}", e, detail(*e)),
        },
    ));

    // ⑧ 探针三序 + 双跑 + 段位守卫。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "F48861-J1-探针三序与段位",
        a && b && c && repro_check().is_ok() && segment_guard(),
        format!("a={} b={} c={} repro={} seg={}", a, b, c, repro_check().is_ok(), segment_guard()),
    ));

    v
}

pub fn run_unx48861_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48861 中断到达段打点");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_ids_are_dense_and_labelled() {
        let mut i = 0u8;
        while i < SEG_COUNT as u8 {
            let seg = SegId::from_ord(i).ok().unwrap_or(SegId::IrqArrival);
            assert_eq!(seg.ord(), i);
            assert!(!seg.label().is_empty());
            assert!(!seg.stage().is_empty());
            i += 1;
        }
        assert_eq!(SegId::from_ord(4).err(), Some(LCode::SegBad.val()));
    }

    #[test]
    fn segment_chain_is_contiguous() {
        let chain = build_chain(1, 10_000, [100, 200, 300, 400]).ok().unwrap_or_default();
        assert_eq!(chain.len(), 4);
        assert_eq!(chain_audit(&chain).ok(), Some(1_000));
    }

    #[test]
    fn chain_with_gap_is_rejected() {
        let mut chain = build_chain(2, 10_000, [100, 200, 300, 400]).ok().unwrap_or_default();
        if let Some(s) = chain.get_mut(2) {
            let shifted = Span::new(Stamp::new(s.span.start.tsc + 1, 0), s.span.end);
            *s = SegmentSample::new(s.seq, s.seg, shifted, s.result);
        }
        assert_eq!(chain_audit(&chain).err(), Some(LCode::ChainBad.val()));
    }

    #[test]
    fn chain_with_three_segments_is_rejected() {
        let chain = build_chain(3, 10_000, [100, 200, 300, 400]).ok().unwrap_or_default();
        let short: Vec<SegmentSample> = chain.iter().take(3).copied().collect();
        assert_eq!(chain_audit(&short).err(), Some(LCode::ChainBad.val()));
    }

    #[test]
    fn backwards_span_is_span_bad() {
        let sp = Span::new(Stamp::new(500, 0), Stamp::new(499, 0));
        assert_eq!(sp.elapsed_ticks().err(), Some(LCode::SpanBad.val()));
    }

    #[test]
    fn cross_core_span_needs_calibration() {
        let sp = Span::new(Stamp::new(500, 0), Stamp::new(700, 1));
        assert_eq!(sp.elapsed_ticks().err(), Some(LCode::CpuBad.val()));
        // 校准偏移：核 0 相对核 1 慢 10 tick，校正后跨度 190。
        assert_eq!(sp.elapsed_ticks_cross(10, 0).ok(), Some(190));
    }

    #[test]
    fn offset_application_checks_overflow() {
        assert_eq!(apply_offset(10, -4).ok(), Some(6));
        assert_eq!(apply_offset(10, 4).ok(), Some(14));
        assert_eq!(apply_offset(3, -4).err(), Some(LCode::ClockBad.val()));
        assert_eq!(apply_offset(0, i64::MIN).err(), Some(LCode::ClockBad.val()));
    }

    #[test]
    fn frequency_conversion_is_exact_at_2ghz() {
        assert_eq!(ticks_to_ns(2_000).ok(), Some(1_000));
        assert_eq!(ticks_to_us(2_000).ok(), Some(1));
        assert_eq!(us_to_ticks(1_000).ok(), Some(2_000_000));
        assert_eq!(ppm_error(2_000_000, 2_000_100).ok(), Some(50));
        assert_eq!(ppm_error(0, 1).err(), Some(LCode::ClockBad.val()));
    }

    #[test]
    fn percentile_nearest_rank_is_pinned() {
        let mut v: Vec<u64> = Vec::new();
        let mut i = 1u64;
        while i <= 100 {
            v.push(i);
            i += 1;
        }
        sort_ticks(&mut v);
        assert_eq!(percentile_nearest_rank(&v, 50).ok(), Some(50));
        assert_eq!(percentile_nearest_rank(&v, 95).ok(), Some(95));
        assert_eq!(percentile_nearest_rank(&v, 99).ok(), Some(99));
        assert_eq!(percentile_nearest_rank(&v, 100).ok(), Some(100));
        assert_eq!(percentile_nearest_rank(&v, 0).err(), Some(LCode::QuantBad.val()));
    }

    #[test]
    fn permutation_invariance_of_quantiles() {
        let a = [9u64, 1, 7, 3, 5];
        let mut va: Vec<u64> = a.to_vec();
        sort_ticks(&mut va);
        let mut vb: Vec<u64> = Vec::new();
        let mut i = a.len();
        while i > 0 {
            i -= 1;
            if let Some(x) = a.get(i) {
                vb.push(*x);
            }
        }
        sort_ticks(&mut vb);
        assert_eq!(va, vb);
        assert_eq!(Quartet::of(&va).ok(), Quartet::of(&vb).ok());
    }

    #[test]
    fn quartet_single_sample_collapses() {
        let q = Quartet::of(&[42]).ok().unwrap_or(Quartet { n: 0, p50: 0, p95: 0, p99: 0, min: 0, max: 0 });
        assert_eq!(q.n, 1);
        assert_eq!(q.p50, 42);
        assert_eq!(q.p95, 42);
        assert_eq!(q.p99, 42);
        assert!(q.monotone());
    }

    #[test]
    fn histogram_buckets_are_balanced() {
        let mut h = Histogram::new();
        h.add(0);
        h.add(80);
        h.add(81);
        h.add(10_000);
        assert_eq!(h.total, 4);
        assert!(h.balanced());
        assert_eq!(Histogram::bucket_of(80), 0);
        assert_eq!(Histogram::bucket_of(81), 1);
        assert_eq!(Histogram::bucket_of(10_000), 7);
    }

    #[test]
    fn ledger_rejects_regressing_seq_and_full_cap() {
        let mut led = CoreLedger::new(2);
        let chain = build_chain(5, 1_000, [100, 100, 100, 100]).ok().unwrap_or_default();
        let mut i = 0usize;
        while i < 3 {
            if let Some(s) = chain.get(i) {
                let r = led.record(*s);
                if i < 2 {
                    assert!(r.is_ok());
                } else {
                    assert_eq!(r.err(), Some(LCode::SampleBad.val()));
                }
            }
            i += 1;
        }
        assert_eq!(led.rejected(), 1);
    }

    #[test]
    fn timeout_sample_stays_out_of_success_distribution() {
        let mut led = CoreLedger::new(16);
        let chain = build_chain(9, 1_000, [100, 200, 300, 400]).ok().unwrap_or_default();
        let mut i = 0usize;
        while i < chain.len() {
            if let Some(s) = chain.get(i) {
                let res = if s.seg == SegId::AppCallback {
                    SegResult::Timeout
                } else {
                    SegResult::Delivered
                };
                let _ = led.record(SegmentSample::new(s.seq, s.seg, s.span, res));
            }
            i += 1;
        }
        assert_eq!(led.count_of(SegId::AppCallback), 1);
        assert!(led.stats(SegId::AppCallback).is_err());
        assert_eq!(led.coverage(), [1, 1, 1, 1]);
    }

    #[test]
    fn golden_is_deterministic_and_full() {
        let g = golden_run();
        assert!(g.is_ok());
        match g {
            Ok(x) => {
                assert_eq!(x.chains, 1000);
                assert_eq!(x.samples, 4000);
                assert_eq!(x.per_seg, [1000, 1000, 1000, 1000]);
                assert_eq!(x.rejected, 0);
                assert!(x.p50_us[0] < x.p50_us[1]);
                assert!(x.p99_us[1] > 0);
            }
            Err(_) => assert!(false),
        }
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four_lanes() {
        let led = inject_rounds(10);
        assert!(led.is_ok());
        match led {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.total(), 40);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_segment_guard_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(LCode::ALL.len(), 10);
    }

    #[test]
    fn checks_are_all_green() {
        let v = extra_checks();
        assert_eq!(v.len(), 8);
        let mut i = 0usize;
        while i < v.len() {
            if let Some((n, p, d)) = v.get(i) {
                assert!(n.starts_with("F4886"));
                assert!(*p, "red: {} {}", n, d);
            }
            i += 1;
        }
        assert!(run_unx48861_checks().all_passed());
    }
}
