//! UNX-F49061 延迟账本 P50/P95/P99 落账运营（四段各自分布日常落账）· 0x73280
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第一槽）：
//!   UNX-F49061 延迟账本 P50/P95/P99 落账运营（四段各自分布日常落账）:
//!          ① 纯功能代码 ≥400 行；
//!          ② 判据原文：UNX-F49062-J1 校准流程 ×4 段登记零虚报；
//!             判据锚定 §AI-62 §5 J-1 阈值口径。
//!   UNX-F49062 段阈值试产校准流程（冻结前标校准位——校准流程四步成文）:
//!          ① 纯功能代码 ≥380 行；
//!          ② 判据原文：UNX-F49063-J1 对照列格式联签锚定行 + 格式一致 ×10；
//!             判据锚定 §AI-62 §8 联签对 AI-71。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（分位账）：样本 → 排序 → 分位 → 日账 → 复跑。
//! 样本先于排序：非法的段号/日号/计时值必须在入账口就被挡住，否则排序会把
//!   垃圾数据摊平进分位，P99 被一条 0 值或一条 2^32-1 拉偏而无人察觉；
//! 排序先于分位：最近秩分位的定义域是**有序**样本，未排序时取第 k 个元素
//!   得到的是"第 k 个到达的样本"而不是"第 k 分位"，两者毫无关系；
//! 分位先于日账：日账折的是当日分位与样本数，分位本身错则日账稳定地错；
//! 日账先于复跑：十天逐日全落账之后，复跑摘要一致才证明落账过程无隐藏状态。
//! 闸门次序（校准）：采样 → 估计 → 拟阈 → 冻结。
//! 采样先于估计：样本不足 64 条的段，P99 的最近秩只落在第 1 个样本上，
//!   此时"估计"只是把一条样本抄进报告，属于虚报；
//! 估计先于拟阈：阈值必须由**实测**分位推得，先拟阈值再倒推分位是自证；
//! 拟阈先于冻结：冻结位一旦落章，阈值便不可再改，先冻结后拟阈等于无校准；
//! 冻结先于报告：报告是冻结位的产物，没有冻结位的报告只是草稿。
//! 关键差异：F49061 回答"四段日常分布是否逐日落账、分位是否单调可复跑"；
//! F49062 回答"冻结前的四次校准是否按序完成、阈值是否零虚报"。
//! 本模块**不**做 O1 对照列（F49063 的职责），也不做导出与告警（F49067/69），
//! 边界止于"账本内部分位与冻结位"。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73280，B14 族第一槽）。
pub const SEG_BASE: u32 = 0x73280;

/// 延迟链四段：中断入口 / 事件分发 / 合成 / 呈现。
pub const SEGMENTS: usize = 4;
/// 日常落账天数（判据口径 ×10 日）。
pub const DAYS: usize = 10;
/// 每段每日最少样本数（低于此值不构成"分布"）。
pub const MIN_DAY_SAMPLES: u32 = 32;
/// 校准最少样本数（低于此值 P99 无统计意义）。
pub const MIN_CAL_SAMPLES: u32 = 64;
/// 单本账样本上限。
pub const BOOK_CAP: usize = 1 << 16;
/// 分位分母（千分制）。
pub const PERMILLE: u32 = 1_000;
/// P50 千分位。
pub const P50: u32 = 500;
/// P95 千分位。
pub const P95: u32 = 950;
/// P99 千分位。
pub const P99: u32 = 990;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 直方图桶数（按 log2 分桶，0..=31）。
pub const HIST_BUCKETS: usize = 32;
/// 校准阈值相对 P99 的余量（十分之一，即 +10%）。
pub const THRESHOLD_MARGIN_DIV: u32 = 10;
/// 四段 P99 预算（纳秒）：中断 2µs / 分发 3µs / 合成 8µs / 呈现 12µs。
pub const SEG_BUDGET_NS: [u32; SEGMENTS] = [2_000, 3_000, 8_000, 12_000];
/// 账行字段数。
pub const RENDER_FIELDS: usize = 6;
/// 分位账标签。
pub const LEDGER_TAG: &str = "LEDG";
/// 校准账标签。
pub const CAL_TAG: &str = "CALB";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 延迟账本诊断码";
/// 四段名称（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 校准四步名称（下标即步号）。
pub const STEP_NAMES: [&str; 4] = ["sample", "estimate", "draft", "freeze"];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LatCode {
    /// 段号越出 0..3
    SegBad,
    /// 样本值非法（0 或超过 1 秒）
    SampleBad,
    /// 空账或无有效样本
    EmptyLedger,
    /// 分位越界或单调破坏
    PercentBad,
    /// 日号越界或断日
    DayBad,
    /// 日账合计不守恒
    RollupBad,
    /// 校准步骤次序错乱
    StepOrderBad,
    /// 校准前置条件不满足
    PreconditionBad,
    /// 阈值未满足 p99 < t ≤ 2×p99
    ThresholdBad,
    /// 冻结位被改写
    FreezeBad,
    /// 报告与实测不符（虚报）
    ReportBad,
    /// 直方图定位不覆盖实测分位
    HistBad,
    /// 账行不可解析或往返不等
    RenderBad,
    /// 计数溢出
    Overflow,
}

impl LatCode {
    /// 诊断码值：段基址 + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码中文说明（不得共用同一句）。
    pub fn detail(self) -> &'static str {
        match self {
            LatCode::SegBad => "段号不在 0..3，无法归属延迟链",
            LatCode::SampleBad => "样本计时为 0 或超过 1 秒上限",
            LatCode::EmptyLedger => "账本无样本，分位无定义域",
            LatCode::PercentBad => "分位越界或 P50≤P95≤P99 单调被破坏",
            LatCode::DayBad => "日号越出 0..9 或十天未逐日落账",
            LatCode::RollupBad => "日账各段合计与样本总数不守恒",
            LatCode::StepOrderBad => "校准四步未按 采样→估计→拟阈→冻结 次序执行",
            LatCode::PreconditionBad => "校准前置样本数不足或段缺失",
            LatCode::ThresholdBad => "阈值须满足 p99 < t ≤ 2×p99",
            LatCode::FreezeBad => "冻结位已落章却再次改写阈值",
            LatCode::ReportBad => "校准报告字段与实测分位/样本数不一致",
            LatCode::HistBad => "直方图桶区间未覆盖实测分位",
            LatCode::RenderBad => "账行字段缺失、数字非法或往返不等",
            LatCode::Overflow => "样本或计数累计溢出",
        }
    }
}

/// 自由函数版诊断说明，越界返回 [`UNKNOWN`]。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x0000 => UNKNOWN,
        _ => {
            let idx = code.wrapping_sub(SEG_BASE + 1);
            match LatCode::ALL.get(idx as usize) {
                Some(c) => c.detail(),
                None => UNKNOWN,
            }
        }
    }
}

impl LatCode {
    /// 14 个诊断码全集（顺序即变体顺序）。
    pub const ALL: [LatCode; 14] = [
        LatCode::SegBad,
        LatCode::SampleBad,
        LatCode::EmptyLedger,
        LatCode::PercentBad,
        LatCode::DayBad,
        LatCode::RollupBad,
        LatCode::StepOrderBad,
        LatCode::PreconditionBad,
        LatCode::ThresholdBad,
        LatCode::FreezeBad,
        LatCode::ReportBad,
        LatCode::HistBad,
        LatCode::RenderBad,
        LatCode::Overflow,
    ];
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < LatCode::ALL.len() {
        let v = match LatCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < LatCode::ALL.len() {
            if let Some(other) = LatCode::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 延迟链段 ─────────────────────────────────────────────────────
/// 延迟链四段。判定权属于上游（中断/合成器），本域只落账。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Segment {
    Interrupt,
    Dispatch,
    Compose,
    Present,
}

impl Segment {
    /// 段序号（0..3）。
    pub fn index(self) -> u8 {
        match self {
            Segment::Interrupt => 0,
            Segment::Dispatch => 1,
            Segment::Compose => 2,
            Segment::Present => 3,
        }
    }

    /// 段名。
    pub fn name(self) -> &'static str {
        match SEG_NAMES.get(self.index() as usize) {
            Some(s) => s,
            None => "unknown",
        }
    }

    /// 由序号还原段；越界给出 SegBad。
    pub fn from_index(i: u8) -> Result<Segment, u32> {
        match i {
            0 => Ok(Segment::Interrupt),
            1 => Ok(Segment::Dispatch),
            2 => Ok(Segment::Compose),
            3 => Ok(Segment::Present),
            _ => Err(LatCode::SegBad.val()),
        }
    }

    /// 该段 P99 预算（纳秒）。
    pub fn budget_ns(self) -> u32 {
        SEG_BUDGET_NS
            .get(self.index() as usize)
            .copied()
            .unwrap_or(0)
    }
}

// ── 样本与账本 ───────────────────────────────────────────────────
/// 一条延迟样本：段、日、纳秒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencySample {
    pub seg: u8,
    pub day: u8,
    pub ns: u32,
}

impl LatencySample {
    /// 校验并构造样本。
    pub fn new(seg: u8, day: u8, ns: u32) -> Result<LatencySample, u32> {
        if seg as usize >= SEGMENTS {
            return Err(LatCode::SegBad.val());
        }
        if day as usize >= DAYS {
            return Err(LatCode::DayBad.val());
        }
        if ns == 0 || ns > NS_LIMIT {
            return Err(LatCode::SampleBad.val());
        }
        Ok(LatencySample { seg, day, ns })
    }
}

/// 单段样本容器。排序结果缓存于 `sorted`，失效时置空。
#[derive(Clone, Debug)]
pub struct SegmentSamples {
    seg: Segment,
    raw: Vec<u32>,
    sorted: Vec<u32>,
    dropped: u32,
    digest: u64,
}

impl SegmentSamples {
    /// 新建空段容器。
    pub fn new(seg: Segment) -> SegmentSamples {
        SegmentSamples {
            seg,
            raw: Vec::new(),
            sorted: Vec::new(),
            dropped: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 段号。
    pub fn segment(&self) -> Segment {
        self.seg
    }

    /// 已接收样本数。
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// 容量外被丢弃的样本数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 滚动摘要（按接收序折叠值）。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 追加一条样本；满容时计入 dropped 并返回 Overflow。
    pub fn push(&mut self, ns: u32) -> Result<usize, u32> {
        if ns == 0 || ns > NS_LIMIT {
            return Err(LatCode::SampleBad.val());
        }
        if self.raw.len() >= BOOK_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(LatCode::Overflow.val());
        }
        self.raw.push(ns);
        self.sorted.clear();
        self.digest = p_core::fold_u32(self.digest, ns);
        Ok(self.raw.len())
    }

    /// 排序后的只读视图（惰性、幂等）。
    pub fn sorted(&mut self) -> &[u32] {
        if self.sorted.len() != self.raw.len() {
            self.sorted.clear();
            self.sorted.extend_from_slice(&self.raw);
            self.sorted.sort_unstable();
        }
        &self.sorted
    }

    /// 各值之和（u64，防溢出）。
    pub fn sum(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0usize;
        while i < self.raw.len() {
            if let Some(v) = self.raw.get(i) {
                acc = acc.saturating_add(*v as u64);
            }
            i += 1;
        }
        acc
    }
}

/// 最近秩分位：`permille` ∈ (0, 1000]，返回有序样本中的第 ⌈n·p⌉ 个。
pub fn nearest_rank(sorted: &[u32], permille: u32) -> Option<u32> {
    if sorted.is_empty() || permille == 0 || permille > PERMILLE {
        return None;
    }
    let n = sorted.len() as u64;
    let rank = (n * permille as u64).div_ceil(PERMILLE as u64);
    let idx = rank.saturating_sub(1) as usize;
    sorted.get(idx).copied()
}

/// 分布摘要（同一段一次排序得出全部指标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Distribution {
    pub count: u32,
    pub min: u32,
    pub max: u32,
    pub mean: u32,
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
}

impl Distribution {
    /// 单调性：min ≤ p50 ≤ p95 ≤ p99 ≤ max。
    pub fn monotone(&self) -> bool {
        self.min <= self.p50
            && self.p50 <= self.p95
            && self.p95 <= self.p99
            && self.p99 <= self.max
    }

    /// 尾比（P99/P50，基点）。P50 为 0 时返回 0。
    pub fn tail_ratio_bp(&self) -> u32 {
        if self.p50 == 0 {
            return 0;
        }
        let num = self.p99 as u64 * 10_000u64;
        (num / self.p50 as u64).min(u32::MAX as u64) as u32
    }

    /// 是否落在该段预算内（P99 ≤ 预算）。
    pub fn within_budget(&self, seg: Segment) -> bool {
        self.p99 <= seg.budget_ns()
    }
}

/// 由有序样本求分布摘要；空样本返回 EmptyLedger。
pub fn distribution(sorted: &[u32]) -> Result<Distribution, u32> {
    if sorted.is_empty() {
        return Err(LatCode::EmptyLedger.val());
    }
    let mut min = u32::MAX;
    let mut max = 0u32;
    let mut acc: u64 = 0;
    let mut i = 0usize;
    while i < sorted.len() {
        let v = match sorted.get(i) {
            Some(x) => *x,
            None => return Err(LatCode::PercentBad.val()),
        };
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
        acc = acc.saturating_add(v as u64);
        i += 1;
    }
    let n = sorted.len() as u64;
    let mean = (acc / n).min(u32::MAX as u64) as u32;
    let p50 = match nearest_rank(sorted, P50) {
        Some(v) => v,
        None => return Err(LatCode::PercentBad.val()),
    };
    let p95 = match nearest_rank(sorted, P95) {
        Some(v) => v,
        None => return Err(LatCode::PercentBad.val()),
    };
    let p99 = match nearest_rank(sorted, P99) {
        Some(v) => v,
        None => return Err(LatCode::PercentBad.val()),
    };
    let d = Distribution {
        count: sorted.len().min(u32::MAX as usize) as u32,
        min,
        max,
        mean,
        p50,
        p95,
        p99,
    };
    if !d.monotone() {
        return Err(LatCode::PercentBad.val());
    }
    Ok(d)
}

// ── 直方图（log2 分桶，校准的 O(1) 旁路） ────────────────────────
/// 32 桶 log2 直方图：桶 k 覆盖 [2^k, 2^(k+1))，桶 0 覆盖 [0,2)。
#[derive(Clone, Copy, Debug)]
pub struct Histogram {
    counts: [u32; HIST_BUCKETS],
    total: u32,
}

impl Histogram {
    /// 空直方图。
    pub fn new() -> Histogram {
        Histogram {
            counts: [0u32; HIST_BUCKETS],
            total: 0,
        }
    }

    /// 由排序样本构建；样本数须为 u32 可容纳。
    pub fn build(sorted: &[u32]) -> Result<Histogram, u32> {
        let mut h = Histogram::new();
        let mut i = 0usize;
        while i < sorted.len() {
            let v = match sorted.get(i) {
                Some(x) => *x,
                None => return Err(LatCode::HistBad.val()),
            };
            h.add(v)?;
            i += 1;
        }
        Ok(h)
    }

    /// 单值入桶。
    pub fn add(&mut self, ns: u32) -> Result<(), u32> {
        let b = log2_bucket(ns);
        let slot = match self.counts.get_mut(b as usize) {
            Some(s) => s,
            None => return Err(LatCode::HistBad.val()),
        };
        *slot = slot.saturating_add(1);
        self.total = self.total.saturating_add(1);
        Ok(())
    }

    /// 总计数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 桶计数。
    pub fn bucket(&self, b: usize) -> u32 {
        self.counts.get(b).copied().unwrap_or(0)
    }

    /// 各桶计数之和（对账用）。
    pub fn recount(&self) -> u32 {
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            acc = acc.saturating_add(self.bucket(i));
            i += 1;
        }
        acc
    }

    /// 累计到第 `permille` 分位所在的桶号。
    pub fn locate(&self, permille: u32) -> Option<u8> {
        if self.total == 0 || permille == 0 || permille > PERMILLE {
            return None;
        }
        let target = (self.total as u64 * permille as u64)
            .div_ceil(PERMILLE as u64)
            .min(u32::MAX as u64) as u32;
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            acc = acc.saturating_add(self.bucket(i));
            if acc >= target {
                return Some(i as u8);
            }
            i += 1;
        }
        Some((HIST_BUCKETS - 1) as u8)
    }

    /// 桶区间 [lo, hi]；顶桶 hi 取 NS_LIMIT。
    pub fn bucket_range(b: u8) -> (u32, u32) {
        if b >= (HIST_BUCKETS - 1) as u8 {
            let lo = 1u32 << 31;
            return (lo, NS_LIMIT.max(lo));
        }
        let lo = 1u32 << b;
        let hi = (1u32 << (b as u32 + 1)).saturating_sub(1);
        (lo, hi)
    }

    /// 实测分位是否落在直方图定位桶的区间内。
    pub fn covers(&self, permille: u32, exact: u32) -> bool {
        let b = match self.locate(permille) {
            Some(v) => v,
            None => return false,
        };
        if b == 0 {
            // 桶 0 覆盖 [0,2)：0 与 1 都在内。
            return exact <= 1;
        }
        let (lo, hi) = Histogram::bucket_range(b);
        exact >= lo && exact <= hi
    }
}

/// log2 分桶：0 → 0，其余取最高位序号。
pub fn log2_bucket(ns: u32) -> u8 {
    if ns == 0 {
        return 0;
    }
    let lz = ns.leading_zeros().min(31) as u8;
    31 - lz
}

// ── 四段账本 ─────────────────────────────────────────────────────
/// 四段账本：每段独立容器 + 全局接收计数。
#[derive(Clone, Debug)]
pub struct Ledger {
    books: [SegmentSamples; SEGMENTS],
    accepted: u32,
    rejected: u32,
    digest: u64,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger {
            books: [
                SegmentSamples::new(Segment::Interrupt),
                SegmentSamples::new(Segment::Dispatch),
                SegmentSamples::new(Segment::Compose),
                SegmentSamples::new(Segment::Present),
            ],
            accepted: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 入账：校验后归入对应段；非法样本计入 rejected 并返回诊断码。
    pub fn record(&mut self, s: LatencySample) -> Result<(), u32> {
        let checked = match LatencySample::new(s.seg, s.day, s.ns) {
            Ok(v) => v,
            Err(code) => {
                // 校验口拒绝同样要入拒账：静默返回 Err 会让"异常零静默"判据失真。
                self.rejected = self.rejected.saturating_add(1);
                return Err(code);
            }
        };
        let idx = checked.seg as usize;
        let book = match self.books.get_mut(idx) {
            Some(b) => b,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(LatCode::SegBad.val());
            }
        };
        match book.push(checked.ns) {
            Ok(_) => {
                self.accepted = self.accepted.saturating_add(1);
                self.digest = p_core::fold_u32(self.digest, checked.ns);
                self.digest = p_core::fold_byte(self.digest, checked.day);
                Ok(())
            }
            Err(code) => {
                self.rejected = self.rejected.saturating_add(1);
                Err(code)
            }
        }
    }

    /// 段容器只读引用。
    pub fn book(&self, seg: Segment) -> &SegmentSamples {
        let i = seg.index() as usize;
        match self.books.get(i) {
            Some(b) => b,
            None => &self.books[0],
        }
    }

    /// 段容器可变引用。
    pub fn book_mut(&mut self, seg: Segment) -> Option<&mut SegmentSamples> {
        self.books.get_mut(seg.index() as usize)
    }

    /// 接收样本总数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝样本总数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 账本滚动摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 全部段样本数之和（对账 accepted）。
    pub fn recount(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(b) = self.books.get(i) {
                acc = acc.saturating_add(b.len() as u64);
            }
            i += 1;
        }
        acc
    }

    /// 某段分布摘要。
    pub fn distribution_of(&mut self, seg: Segment) -> Result<Distribution, u32> {
        let i = seg.index() as usize;
        let book = match self.books.get_mut(i) {
            Some(b) => b,
            None => return Err(LatCode::SegBad.val()),
        };
        let sorted = book.sorted();
        distribution(sorted)
    }

    /// 四段分布一次性求值（段序固定）。
    pub fn distributions(&mut self) -> Result<[Distribution; SEGMENTS], u32> {
        let mut out = [Distribution {
            count: 0,
            min: 0,
            max: 0,
            mean: 0,
            p50: 0,
            p95: 0,
            p99: 0,
        }; SEGMENTS];
        let mut i = 0usize;
        while i < SEGMENTS {
            let seg = Segment::from_index(i as u8)?;
            out[i] = self.distribution_of(seg)?;
            i += 1;
        }
        Ok(out)
    }
}

// ── 日常落账 ─────────────────────────────────────────────────────
/// 单日单段计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayCell {
    pub day: u8,
    pub seg: u8,
    pub count: u32,
}

/// 单日汇总：四段计数、总计、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayRollup {
    pub day: u8,
    pub per_seg: [u32; SEGMENTS],
    pub total: u32,
    pub digest: u64,
}

impl DayRollup {
    /// 该日是否四段齐备且每段不少于下限。
    pub fn complete(&self) -> bool {
        let mut i = 0usize;
        while i < SEGMENTS {
            let n = self.per_seg.get(i).copied().unwrap_or(0);
            if n < MIN_DAY_SAMPLES {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 四段合计对账。
    pub fn self_consistent(&self) -> bool {
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < SEGMENTS {
            acc = acc.saturating_add(self.per_seg.get(i).copied().unwrap_or(0));
            i += 1;
        }
        acc == self.total
    }
}

/// 由逐样本记录求十日汇总（调用方持有原始三元组，账本只存段内值）。
///
/// 账本为了排序性能只按段存值，日维度留在入账流水里：这里接受
/// `(day, seg)` 计数矩阵，避免在账本内为每样本再存一份日号。
pub fn daily_rollup(counts: &[[u32; SEGMENTS]; DAYS]) -> Vec<DayRollup> {
    let mut out = Vec::with_capacity(DAYS);
    let mut d = 0usize;
    while d < DAYS {
        let row = match counts.get(d) {
            Some(r) => r,
            None => break,
        };
        let mut per_seg = [0u32; SEGMENTS];
        let mut total: u32 = 0;
        let mut digest = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < SEGMENTS {
            let n = row.get(i).copied().unwrap_or(0);
            per_seg[i] = n;
            total = total.saturating_add(n);
            digest = p_core::fold_u32(digest, n);
            digest = p_core::fold_byte(digest, i as u8);
            i += 1;
        }
        digest = p_core::fold_byte(digest, d as u8);
        out.push(DayRollup {
            day: d as u8,
            per_seg,
            total,
            digest,
        });
        d += 1;
    }
    out
}

/// 日账审计：十天逐日、日号连续、合计守恒、每段齐备。
pub fn audit_daily(rolls: &[DayRollup]) -> Result<u32, u32> {
    if rolls.len() != DAYS {
        return Err(LatCode::DayBad.val());
    }
    let mut total: u32 = 0;
    let mut i = 0usize;
    while i < rolls.len() {
        let r = match rolls.get(i) {
            Some(v) => v,
            None => return Err(LatCode::DayBad.val()),
        };
        if r.day as usize != i {
            return Err(LatCode::DayBad.val());
        }
        if !r.self_consistent() {
            return Err(LatCode::RollupBad.val());
        }
        if !r.complete() {
            return Err(LatCode::PreconditionBad.val());
        }
        total = total.saturating_add(r.total);
        i += 1;
    }
    Ok(total)
}

/// 日账复跑：两遍摘要必须逐日一致。
pub fn daily_replay(counts: &[[u32; SEGMENTS]; DAYS]) -> Result<bool, u32> {
    let a = daily_rollup(counts);
    let b = daily_rollup(counts);
    if a.len() != b.len() {
        return Ok(false);
    }
    let mut i = 0usize;
    while i < a.len() {
        let (x, y) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (x, y),
            _ => return Ok(false),
        };
        if x.digest != y.digest || x.total != y.total || x.day != y.day {
            return Ok(false);
        }
        i += 1;
    }
    Ok(true)
}

// ── 校准四步 ─────────────────────────────────────────────────────
/// 校准四步：采样 → 估计 → 拟阈 → 冻结。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CalibrationStep {
    Sample,
    Estimate,
    Draft,
    Freeze,
}

impl CalibrationStep {
    /// 步号 0..3。
    pub fn index(self) -> u8 {
        match self {
            CalibrationStep::Sample => 0,
            CalibrationStep::Estimate => 1,
            CalibrationStep::Draft => 2,
            CalibrationStep::Freeze => 3,
        }
    }

    /// 步名。
    pub fn name(self) -> &'static str {
        match STEP_NAMES.get(self.index() as usize) {
            Some(s) => s,
            None => "unknown",
        }
    }

    /// 由步号还原；越界给 StepOrderBad。
    pub fn from_index(i: u8) -> Result<CalibrationStep, u32> {
        match i {
            0 => Ok(CalibrationStep::Sample),
            1 => Ok(CalibrationStep::Estimate),
            2 => Ok(CalibrationStep::Draft),
            3 => Ok(CalibrationStep::Freeze),
            _ => Err(LatCode::StepOrderBad.val()),
        }
    }
}

/// 单步登记：步号、样本数、该步产出的阈值（采样/估计步为 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepRecord {
    pub step: CalibrationStep,
    pub seq: u8,
    pub samples: u32,
    pub threshold: u32,
}

/// 校准报告：四步记录 + 四段阈值 + 冻结位 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalibrationReport {
    pub steps: [StepRecord; 4],
    pub thresholds: [u32; SEGMENTS],
    pub frozen: bool,
    pub digest: u64,
}

impl CalibrationReport {
    /// 四步是否按 0,1,2,3 次序登记。
    pub fn ordered(&self) -> bool {
        let mut i = 0usize;
        while i < self.steps.len() {
            let s = match self.steps.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if s.step.index() as usize != i || s.seq != i as u8 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 冻结步是否已落章。
    pub fn freeze_step_ok(&self) -> bool {
        match self.steps.get(3) {
            Some(s) => s.step == CalibrationStep::Freeze && s.seq == 3,
            None => false,
        }
    }
}

/// 由实测 P99 推阈值：`p99 + p99/10`，并校验 `p99 < t ≤ 2×p99`。
pub fn draft_threshold(p99: u32) -> Result<u32, u32> {
    if p99 == 0 {
        return Err(LatCode::ThresholdBad.val());
    }
    let t = p99
        .saturating_add(p99 / THRESHOLD_MARGIN_DIV)
        .max(p99.saturating_add(1));
    if t <= p99 || t as u64 > p99 as u64 * 2 {
        return Err(LatCode::ThresholdBad.val());
    }
    Ok(t)
}

/// 执行四步校准。样本不足、断段、阈值非法均给出对应诊断码。
pub fn calibrate(ledger: &mut Ledger) -> Result<CalibrationReport, u32> {
    // 第一步：采样——四段各自达到最少样本数。
    let mut samples = [0u32; SEGMENTS];
    let mut i = 0usize;
    while i < SEGMENTS {
        let seg = Segment::from_index(i as u8)?;
        let n = ledger.book(seg).len();
        if n < MIN_CAL_SAMPLES as usize {
            return Err(LatCode::PreconditionBad.val());
        }
        samples[i] = n.min(u32::MAX as usize) as u32;
        i += 1;
    }

    // 第二步：估计——由实测样本求分位（不允许用直方图旁路替代）。
    let dists = ledger.distributions()?;

    // 第三步：拟阈——四段阈值全部由 P99 推出。
    let mut thresholds = [0u32; SEGMENTS];
    let mut i = 0usize;
    while i < SEGMENTS {
        let d = match dists.get(i) {
            Some(v) => v,
            None => return Err(LatCode::SegBad.val()),
        };
        if !d.monotone() {
            return Err(LatCode::PercentBad.val());
        }
        thresholds[i] = draft_threshold(d.p99)?;
        i += 1;
    }

    // 第四步：冻结——四步全部登记后摘要落章。
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < SEGMENTS {
        digest = p_core::fold_u32(digest, thresholds[i]);
        digest = p_core::fold_u32(digest, samples[i]);
        i += 1;
    }

    let mut steps = [StepRecord {
        step: CalibrationStep::Sample,
        seq: 0,
        samples: 0,
        threshold: 0,
    }; 4];
    let total_samples = samples
        .iter()
        .fold(0u32, |a, b| a.saturating_add(*b));
    steps[0] = StepRecord {
        step: CalibrationStep::Sample,
        seq: 0,
        samples: total_samples,
        threshold: 0,
    };
    steps[1] = StepRecord {
        step: CalibrationStep::Estimate,
        seq: 1,
        samples: total_samples,
        threshold: 0,
    };
    let mut drafted = 0u32;
    let mut i = 0usize;
    while i < SEGMENTS {
        drafted = drafted.saturating_add(thresholds[i]);
        i += 1;
    }
    steps[2] = StepRecord {
        step: CalibrationStep::Draft,
        seq: 2,
        samples: total_samples,
        threshold: drafted,
    };
    steps[3] = StepRecord {
        step: CalibrationStep::Freeze,
        seq: 3,
        samples: total_samples,
        threshold: drafted,
    };

    Ok(CalibrationReport {
        steps,
        thresholds,
        frozen: true,
        digest,
    })
}

/// 校验报告零虚报：样本数、分位推导的阈值、四步次序、冻结位逐项复算。
pub fn verify_report(ledger: &mut Ledger, report: &CalibrationReport) -> Result<(), u32> {
    if !report.ordered() || !report.freeze_step_ok() || !report.frozen {
        return Err(LatCode::FreezeBad.val());
    }
    let dists = ledger.distributions()?;
    let total = ledger.recount().min(u32::MAX as u64) as u32;
    let mut i = 0usize;
    while i < SEGMENTS {
        let d = match dists.get(i) {
            Some(v) => v,
            None => return Err(LatCode::SegBad.val()),
        };
        let expect = draft_threshold(d.p99)?;
        let got = report.thresholds.get(i).copied().unwrap_or(0);
        if expect != got {
            return Err(LatCode::ReportBad.val());
        }
        i += 1;
    }
    let rec = match report.steps.get(0) {
        Some(s) => *s,
        None => return Err(LatCode::ReportBad.val()),
    };
    if rec.samples != total {
        return Err(LatCode::ReportBad.val());
    }
    Ok(())
}

/// 冻结位复落：内容一致幂等，任一处改动即拒。
pub fn refreeze(
    ledger: &mut Ledger,
    report: &CalibrationReport,
) -> Result<CalibrationReport, u32> {
    if !report.frozen {
        return Err(LatCode::FreezeBad.val());
    }
    verify_report(ledger, report)?;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < SEGMENTS {
        let seg = match Segment::from_index(i as u8) {
            Ok(s) => s,
            Err(code) => return Err(code),
        };
        let n = ledger.book(seg).len().min(u32::MAX as usize) as u32;
        digest = p_core::fold_u32(digest, report.thresholds.get(i).copied().unwrap_or(0));
        digest = p_core::fold_u32(digest, n);
        i += 1;
    }
    if digest != report.digest {
        return Err(LatCode::FreezeBad.val());
    }
    let mut copy = *report;
    copy.digest = digest;
    Ok(copy)
}

/// 虚报注入：返回 (拒绝数, 生成数)。每次只改一个字段，必须全部被拒。
pub fn report_inject(ledger: &mut Ledger, report: &CalibrationReport) -> (u32, u32) {
    let mut rejected = 0u32;
    let mut made = 0u32;
    let mut k = 0usize;
    while k < 10 {
        let mut bad = *report;
        match k {
            0 => {
                bad.thresholds[0] = bad.thresholds[0].saturating_add(1);
            }
            1 => {
                bad.thresholds[1] = bad.thresholds[1].saturating_sub(1);
            }
            2 => {
                bad.thresholds[2] = 0;
            }
            3 => {
                bad.thresholds[3] = bad.thresholds[3].saturating_mul(2);
            }
            4 => {
                bad.steps[0].samples = bad.steps[0].samples.saturating_add(1);
            }
            5 => {
                bad.steps[1].seq = 0;
            }
            6 => {
                bad.steps[2].step = CalibrationStep::Estimate;
            }
            7 => {
                bad.steps[3].seq = 2;
            }
            8 => {
                bad.frozen = false;
            }
            _ => {
                bad.digest = bad.digest ^ 0xFFFF_FFFF;
            }
        }
        made = made.saturating_add(1);
        if verify_report(ledger, &bad).is_err() || refreeze(ledger, &bad).is_err() {
            rejected = rejected.saturating_add(1);
        }
        k += 1;
    }
    (rejected, made)
}

// ── 账行渲染与解析 ───────────────────────────────────────────────
/// 六字段账行：`tag|samples=a|p50=b|p95=c|p99=d|digest=XXXXXXXXXXXXXXXX`。
pub fn render_row(tag: &str, samples: u32, p50: u32, p95: u32, p99: u32, digest: u64) -> String {
    format!(
        "{}|samples={}|p50={}|p95={}|p99={}|digest={:016X}",
        tag, samples, p50, p95, p99, digest
    )
}

/// 十进制 u32 解析（拒绝空串/前导非数字/溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// 16 位大写/小写十六进制解析。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 解析六字段账行：(tag, [samples,p50,p95,p99], digest)。
pub fn row_fields<'a>(row: &'a str) -> Option<(&'a str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

/// 渲染-解析往返恒等。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => {
            render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
        }
        None => false,
    }
}

/// 分位账渲染（样本数 + 三分位 + 摘要）。
pub fn ledger_row(tag: &str, d: &Distribution, digest: u64) -> String {
    render_row(tag, d.count, d.p50, d.p95, d.p99, digest)
}

/// 由段分布与段号折出账本摘要。
pub fn ledger_digest(dists: &[Distribution; SEGMENTS]) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < SEGMENTS {
        if let Some(d) = dists.get(i) {
            h = p_core::fold_u32(h, d.count);
            h = p_core::fold_u32(h, d.p50);
            h = p_core::fold_u32(h, d.p95);
            h = p_core::fold_u32(h, d.p99);
            h = p_core::fold_byte(h, i as u8);
        }
        i += 1;
    }
    h
}

// ── 合成账本（确定性样本，供校准与复跑） ─────────────────────────
/// 单段基准延迟（纳秒）。
pub const SEG_BASE_NS: [u32; SEGMENTS] = [700, 1_100, 2_600, 4_400];
/// 每段每日样本数（≥ MIN_DAY_SAMPLES，留出余量）。
pub const DAY_SAMPLES: u32 = 96;
/// 尾部样本概率分母（1/16 的样本进入长尾）。
pub const TAIL_ONE_IN: u32 = 16;
/// 尾部放大倍数。
pub const TAIL_FACTOR: u32 = 6;

/// 以 LCG 高 61 位生成一段一天的第 k 条样本。
pub fn synth_sample(rng: &mut p_core::Lcg, seg: u8, day: u8, k: u32) -> LatencySample {
    let base = SEG_BASE_NS.get(seg as usize).copied().unwrap_or(1_000);
    // 低比特同余：同一 LCG 连续取值低位相关，四维必须各取高位。
    let r1 = (rng.next() >> 61) as u32;
    let r2 = (rng.next() >> 33) as u32;
    let jitter = r2 % (base / 2 + 1);
    let mut ns = base.saturating_add(jitter);
    if r1 % TAIL_ONE_IN == 0 {
        let mag = 1 + (r2 % TAIL_FACTOR);
        ns = ns.saturating_mul(mag.max(1));
    }
    let ns = ns.clamp(1, NS_LIMIT);
    LatencySample {
        seg,
        day,
        ns: ns.wrapping_add(k % 3),
    }
}

/// 生成完整十日四段账本，并返回逐日计数矩阵。
pub fn synth_ledger(seed: u64) -> Result<(Ledger, [[u32; SEGMENTS]; DAYS]), u32> {
    let mut rng = p_core::Lcg::new(seed);
    let mut ledger = Ledger::new();
    let mut counts = [[0u32; SEGMENTS]; DAYS];
    let mut d = 0u32;
    while d < DAYS as u32 {
        let mut s = 0u32;
        while s < SEGMENTS as u32 {
            let mut k = 0u32;
            while k < DAY_SAMPLES {
                let sample = synth_sample(&mut rng, s as u8, d as u8, k);
                ledger.record(sample)?;
                if let Some(row) = counts.get_mut(d as usize) {
                    if let Some(cell) = row.get_mut(s as usize) {
                        *cell = cell.saturating_add(1);
                    }
                }
                k += 1;
            }
            s += 1;
        }
        d += 1;
    }
    Ok((ledger, counts))
}

// ── 八条检查 ─────────────────────────────────────────────────────
/// 八条检查（两卡各四条）：四段齐备 / 分位单调最近秩 / 十日每日落账 / 账行回读 /
/// 校准四步次序 / 校准前置与拒收 / 冻结幂等与零虚报 / 段位与渲染异常码。
fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::with_capacity(8);

    let synth = synth_ledger(0x0B14_0061);
    let (mut ledger, counts) = match synth {
        Ok(v) => v,
        Err(code) => {
            out.push(("F49061/四段齐备", false, format!("合成账本失败 code={}", code)));
            out.push(("F49061/分位单调", false, String::from("前置失败")));
            out.push(("F49061/十日落账", false, String::from("前置失败")));
            out.push(("F49061/账行回读", false, String::from("前置失败")));
            out.push(("F49062/校准四步", false, String::from("前置失败")));
            out.push(("F49062/前置拒收", false, String::from("前置失败")));
            out.push(("F49062/冻结零虚报", false, String::from("前置失败")));
            out.push(("F49062/段位渲染", false, String::from("前置失败")));
            return out;
        }
    };

    // 1) 四段齐备：样本仅落在四段，拒绝数为 0，recount 与 accepted 相等。
    let ok = ledger.accepted() > 0
        && ledger.recount() == ledger.accepted() as u64
        && ledger.rejected() == 0
        && (0..SEGMENTS).all(|i| {
            Segment::from_index(i as u8)
                .map(|s| ledger.book(s).len() >= MIN_CAL_SAMPLES as usize)
                .unwrap_or(false)
        });
    out.push((
        "F49061/四段齐备",
        ok,
        format!(
            "accepted={} recount={} rejected={}",
            ledger.accepted(),
            ledger.recount(),
            ledger.rejected()
        ),
    ));

    // 2) 分位单调 + 最近秩定义：P50/P95/P99 单调，且 P99 等于有序样本第 ⌈0.99n⌉ 个。
    let dists = ledger.distributions();
    let (mut ok, mut info) = (false, String::from("分布求解失败"));
    if let Ok(ds) = dists {
        let mut all_mono = true;
        let mut rank_ok = true;
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(d) = ds.get(i) {
                if !d.monotone() {
                    all_mono = false;
                }
                if let Ok(seg) = Segment::from_index(i as u8) {
                    let book = ledger.book(seg);
                    let n = book.len() as u64;
                    let rank = (n * P99 as u64).div_ceil(PERMILLE as u64);
                    let expect_idx = rank.saturating_sub(1) as usize;
                    let mut copy: Vec<u32> = book.sorted_copy();
                    copy.sort_unstable();
                    if copy.get(expect_idx).copied().unwrap_or(u32::MAX) != d.p99 {
                        rank_ok = false;
                    }
                }
            } else {
                all_mono = false;
                rank_ok = false;
            }
            i += 1;
        }
        ok = all_mono && rank_ok;
        info = format!("monotone={} rank={}", all_mono, rank_ok);
    }
    out.push(("F49061/分位单调", ok, info));

    // 3) 十日逐日落账：十天齐、日号连续、每段不少于下限、合计守恒、复跑一致。
    let rolls = daily_rollup(&counts);
    let audit = audit_daily(&rolls);
    let replay = daily_replay(&counts).unwrap_or(false);
    let expected_total = DAYS as u32 * SEGMENTS as u32 * DAY_SAMPLES;
    let (ok, info) = match audit {
        Ok(total) => (
            total == expected_total && replay && ledger.accepted() == total,
            format!(
                "total={} expect={} replay={} accepted={}",
                total,
                expected_total,
                replay,
                ledger.accepted()
            ),
        ),
        Err(code) => (false, format!("audit code={}", code)),
    };
    out.push(("F49061/十日落账", ok, info));

    // 4) 账行渲染-解析往返：四段各一行，audit_render 全真；改一位即假。
    let mut ok = true;
    let mut bad = String::new();
    let mut digest_ok = true;
    if let Ok(ds) = ledger.distributions() {
        let mut i = 0usize;
        while i < SEGMENTS {
            if let Some(d) = ds.get(i) {
                let row = ledger_row(LEDGER_TAG, d, ledger_digest(&ds));
                if !audit_render(&row) {
                    ok = false;
                    if bad.is_empty() {
                        bad = format!("row{} not round-trip", i);
                    }
                }
                let broken = format!("{}|samples={}|p50=", LEDGER_TAG, d.count);
                if audit_render(&broken) {
                    ok = false;
                    if bad.is_empty() {
                        bad = String::from("truncated row accepted");
                    }
                }
            } else {
                ok = false;
                digest_ok = false;
            }
            i += 1;
        }
        let d0 = ledger_digest(&ds);
        let d1 = ledger_digest(&ds);
        digest_ok = digest_ok && d0 == d1 && d0 != 0;
        if !digest_ok {
            ok = false;
            if bad.is_empty() {
                bad = String::from("digest unstable");
            }
        }
    } else {
        ok = false;
        bad = String::from("distributions failed");
    }
    out.push((
        "F49061/账行回读",
        ok,
        if ok { String::from("4 rows round-trip") } else { bad },
    ));

    // 5) 校准四步次序：四步 seq 0..3，步名齐，冻结步在末位。
    let cal = calibrate(&mut ledger);
    let (mut ok, mut info) = (false, String::from("校准失败"));
    if let Ok(r) = cal {
        let ordered = r.ordered();
        let freeze_ok = r.freeze_step_ok() && r.frozen;
        let names_ok = (0..4).all(|i| {
            CalibrationStep::from_index(i as u8)
                .map(|s| s.name() == STEP_NAMES[i])
                .unwrap_or(false)
        });
        ok = ordered && freeze_ok && names_ok;
        info = format!("ordered={} freeze={} names={}", ordered, freeze_ok, names_ok);
    }
    out.push(("F49062/校准四步", ok, info));

    // 6) 校准前置与拒收：样本不足的段必须 PreconditionBad；非法样本必须入拒。
    let mut small = Ledger::new();
    let mut precond_ok = false;
    let mut i = 0u32;
    while i < 8 {
        let s = LatencySample::new(0, 0, 900 + i).unwrap_or(LatencySample {
            seg: 0,
            day: 0,
            ns: 900,
        });
        let _ = small.record(s);
        i += 1;
    }
    if let Err(code) = calibrate(&mut small) {
        precond_ok = code == LatCode::PreconditionBad.val();
    }
    let mut declined = 0u32;
    let mut attempts = 0u32;
    let bads = [
        LatencySample { seg: 9, day: 0, ns: 900 },
        LatencySample { seg: 0, day: 19, ns: 900 },
        LatencySample { seg: 0, day: 0, ns: 0 },
        LatencySample { seg: 0, day: 0, ns: NS_LIMIT + 1 },
        LatencySample { seg: 3, day: 9, ns: NS_LIMIT + 5 },
    ];
    let mut k = 0usize;
    while k < bads.len() {
        attempts = attempts.saturating_add(1);
        if let Some(s) = bads.get(k) {
            if small.record(*s).is_err() {
                declined = declined.saturating_add(1);
            }
        }
        k += 1;
    }
    let ok = precond_ok && declined == attempts && small.rejected() == attempts;
    out.push((
        "F49062/前置拒收",
        ok,
        format!(
            "precond={} declined={}/{} rejected={}",
            precond_ok,
            declined,
            attempts,
            small.rejected()
        ),
    ));

    // 7) 冻结幂等 + 零虚报：原报告复落成功；10 次单字段虚报全部被拒。
    let (mut ok, mut info) = (false, String::from("校准失败"));
    if let Ok(report) = calibrate(&mut ledger) {
        let again = refreeze(&mut ledger, &report);
        let idem = matches!(&again, Ok(r) if r.digest == report.digest);
        let verified = verify_report(&mut ledger, &report).is_ok();
        let (rejected, made) = report_inject(&mut ledger, &report);
        ok = idem && verified && rejected == made && made == 10;
        info = format!(
            "idem={} verified={} rejected={}/{}",
            idem, verified, rejected, made
        );
    }
    out.push(("F49062/冻结零虚报", ok, info));

    // 8) 段位与渲染异常码：14 码守卫、越界说明、坏账行全拒、阈值口径。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    let mut bad_rows = 0u32;
    let mut accepted_bad = 0u32;
    let cases = [
        "",
        "LEDG|samples=1|p50=2|p95=3|p99=4",
        "LEDG|samples=x|p50=2|p95=3|p99=4|digest=0000000000000000",
        "LEDG|samples=1|p50=2|p95=3|p99=4|digest=00000000000000zz",
        "LEDG|samples=1|p50=2|p95=3|p99=4|digest=0000000000000000|x=1",
        "|samples=1|p50=2|p95=3|p99=4|digest=0000000000000000",
    ];
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(row) = cases.get(k) {
            bad_rows = bad_rows.saturating_add(1);
            if audit_render(row) {
                accepted_bad = accepted_bad.saturating_add(1);
            }
        }
        k += 1;
    }
    if bad_rows != cases.len() as u32 || accepted_bad != 0 {
        ok = false;
        bits.push_str("bad row accepted; ");
    }
    let t_ok = draft_threshold(1_000).map(|t| t == 1_100).unwrap_or(false)
        && draft_threshold(0).is_err()
        && draft_threshold(u32::MAX).is_err();
    if !t_ok {
        ok = false;
        bits.push_str("threshold drift; ");
    }
    if ok && bits.is_empty() {
        bits = String::from("guard/tail/threshold ok");
    }
    out.push(("F49062/段位渲染", ok, bits));

    out
}

/// 模块入口：八条检查折叠为一个 CheckSet（域标签 `unxstar`）。
pub fn run_unx49061_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    for (name, passed, detail_text) in extra_checks() {
        set.add_dyn(name, passed, &detail_text);
    }
    set
}

// 便于检查内取排序副本：不改变段容器的惰性缓存。
impl SegmentSamples {
    /// 复制一份排序样本（不触碰内部缓存）。
    pub fn sorted_copy(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::with_capacity(self.raw.len());
        v.extend_from_slice(&self.raw);
        v.sort_unstable();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn build(seed: u64) -> (Ledger, [[u32; SEGMENTS]; DAYS]) {
        match synth_ledger(seed) {
            Ok(v) => v,
            Err(_) => (Ledger::new(), [[0u32; SEGMENTS]; DAYS]),
        }
    }

    #[test]
    fn seg_index_roundtrip() {
        for i in 0..SEGMENTS {
            let s = Segment::from_index(i as u8).expect("seg");
            assert_eq!(s.index(), i as u8);
            assert_eq!(s.name(), SEG_NAMES[i]);
        }
        assert!(Segment::from_index(4).is_err());
        assert!(Segment::from_index(255).is_err());
    }

    #[test]
    fn seg_budget_matches_table() {
        for i in 0..SEGMENTS {
            let s = Segment::from_index(i as u8).expect("seg");
            assert_eq!(s.budget_ns(), SEG_BUDGET_NS[i]);
        }
    }

    #[test]
    fn code_values_are_dense_window() {
        let mut seen = [false; 14];
        for c in LatCode::ALL {
            let off = c.val() - SEG_BASE;
            assert!(off >= 1 && off <= 14);
            let idx = (off - 1) as usize;
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn code_detail_nonempty_and_distinct() {
        let mut texts: Vec<&'static str> = Vec::new();
        for c in LatCode::ALL {
            let t = c.detail();
            assert!(!t.is_empty());
            assert!(!texts.contains(&t), "duplicate detail {}", t);
            texts.push(t);
        }
        assert_eq!(texts.len(), 14);
    }

    #[test]
    fn free_detail_bounds() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 1), LatCode::SegBad.detail());
        assert_eq!(detail(SEG_BASE + 14), LatCode::Overflow.detail());
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
    }

    #[test]
    fn sample_new_rejects_bad() {
        assert!(LatencySample::new(0, 0, 1).is_ok());
        assert!(LatencySample::new(3, 9, NS_LIMIT).is_ok());
        assert!(LatencySample::new(4, 0, 1).is_err());
        assert!(LatencySample::new(0, 10, 1).is_err());
        assert!(LatencySample::new(0, 0, 0).is_err());
        assert!(LatencySample::new(0, 0, NS_LIMIT + 1).is_err());
    }

    #[test]
    fn nearest_rank_definition() {
        let s = [10u32, 20, 30, 40, 50];
        assert_eq!(nearest_rank(&s, 500), Some(30));
        assert_eq!(nearest_rank(&s, 950), Some(50));
        assert_eq!(nearest_rank(&s, 990), Some(50));
        assert_eq!(nearest_rank(&s, 1000), Some(50));
        assert_eq!(nearest_rank(&s, 0), None);
        assert_eq!(nearest_rank(&s, 1001), None);
        assert_eq!(nearest_rank(&[], 500), None);
    }

    #[test]
    fn nearest_rank_ceil_boundary() {
        let s = [1u32, 2, 3, 4];
        // ⌈4×0.5⌉=2 → 第 2 个 = 2
        assert_eq!(nearest_rank(&s, 500), Some(2));
        // ⌈4×0.99⌉=4 → 第 4 个 = 4
        assert_eq!(nearest_rank(&s, 990), Some(4));
        // ⌈3×0.5⌉=2 → 第 2 个 = 2
        assert_eq!(nearest_rank(&s[..3], 500), Some(2));
    }

    #[test]
    fn distribution_basic() {
        let s = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let d = distribution(&s).expect("dist");
        assert_eq!(d.count, 10);
        assert_eq!(d.min, 1);
        assert_eq!(d.max, 10);
        assert_eq!(d.mean, 5);
        assert!(d.monotone());
        assert_eq!(d.p50, 5);
        assert_eq!(d.p95, 10);
        assert_eq!(d.p99, 10);
    }

    #[test]
    fn distribution_empty_is_error() {
        assert_eq!(distribution(&[]), Err(LatCode::EmptyLedger.val()));
    }

    #[test]
    fn tail_ratio_bp() {
        let d = Distribution {
            count: 4,
            min: 10,
            max: 100,
            mean: 50,
            p50: 20,
            p95: 80,
            p99: 100,
        };
        assert_eq!(d.tail_ratio_bp(), 50_000);
        let z = Distribution { p50: 0, ..d };
        assert_eq!(z.tail_ratio_bp(), 0);
    }

    #[test]
    fn within_budget() {
        let d = Distribution {
            count: 4,
            min: 10,
            max: 4_000,
            mean: 50,
            p50: 20,
            p95: 80,
            p99: 2_500,
        };
        assert!(!d.within_budget(Segment::Interrupt));
        assert!(d.within_budget(Segment::Dispatch));
    }

    #[test]
    fn segment_samples_push_and_sorted() {
        let mut s = SegmentSamples::new(Segment::Interrupt);
        assert!(s.is_empty());
        assert_eq!(s.push(30), Ok(1));
        assert_eq!(s.push(10), Ok(2));
        assert_eq!(s.push(20), Ok(3));
        assert_eq!(s.sorted(), &[10, 20, 30]);
        assert_eq!(s.sum(), 60);
        assert_eq!(s.dropped(), 0);
    }

    #[test]
    fn segment_samples_rejects_bad_values() {
        let mut s = SegmentSamples::new(Segment::Interrupt);
        assert_eq!(s.push(0), Err(LatCode::SampleBad.val()));
        assert_eq!(s.push(NS_LIMIT + 1), Err(LatCode::SampleBad.val()));
        assert!(s.is_empty());
    }

    #[test]
    fn segment_samples_digest_changes_with_order() {
        let mut a = SegmentSamples::new(Segment::Interrupt);
        let mut b = SegmentSamples::new(Segment::Interrupt);
        a.push(10).expect("a");
        a.push(20).expect("a");
        b.push(20).expect("b");
        b.push(10).expect("b");
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn segment_samples_digest_stable_for_same_order() {
        let mut a = SegmentSamples::new(Segment::Dispatch);
        let mut b = SegmentSamples::new(Segment::Dispatch);
        for v in [5u32, 7, 11, 13] {
            a.push(v).expect("a");
            b.push(v).expect("b");
        }
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn segment_samples_sorted_cache_invalidated() {
        let mut s = SegmentSamples::new(Segment::Compose);
        s.push(30).expect("push");
        s.push(10).expect("push");
        assert_eq!(s.sorted(), &[10, 30]);
        s.push(20).expect("push");
        assert_eq!(s.sorted(), &[10, 20, 30]);
    }

    #[test]
    fn sorted_copy_independent() {
        let mut s = SegmentSamples::new(Segment::Present);
        s.push(3).expect("push");
        s.push(1).expect("push");
        let c = s.sorted_copy();
        assert_eq!(c, vec![1, 3]);
        assert_eq!(s.sorted(), &[1, 3]);
    }

    #[test]
    fn log2_bucket_edges() {
        assert_eq!(log2_bucket(0), 0);
        assert_eq!(log2_bucket(1), 0);
        assert_eq!(log2_bucket(2), 1);
        assert_eq!(log2_bucket(3), 1);
        assert_eq!(log2_bucket(4), 2);
        assert_eq!(log2_bucket(1023), 9);
        assert_eq!(log2_bucket(1024), 10);
        assert_eq!(log2_bucket(u32::MAX), 31);
    }

    #[test]
    fn bucket_range_contains_value() {
        for b in 0u8..31 {
            let (lo, hi) = Histogram::bucket_range(b);
            assert!(lo <= hi);
            assert_eq!(log2_bucket(lo), b);
            assert_eq!(log2_bucket(hi), b);
        }
        let (lo, hi) = Histogram::bucket_range(31);
        assert!(lo <= hi);
        assert_eq!(log2_bucket(lo), 31);
    }

    #[test]
    fn histogram_counts_and_total() {
        let sorted = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let h = Histogram::build(&sorted).expect("hist");
        assert_eq!(h.total(), 8);
        assert_eq!(h.recount(), 8);
        assert_eq!(h.bucket(0), 1);
        assert_eq!(h.bucket(1), 2);
        assert_eq!(h.bucket(2), 4);
        assert_eq!(h.bucket(3), 1);
    }

    #[test]
    fn histogram_locate_p50() {
        let sorted = [1u32, 2, 3, 4];
        let h = Histogram::build(&sorted).expect("hist");
        let b = h.locate(500).expect("bucket");
        assert_eq!(b, 1);
    }

    #[test]
    fn histogram_covers_exact_percentile() {
        let mut sorted: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 100 {
            sorted.push(i);
            i += 1;
        }
        let h = Histogram::build(&sorted).expect("hist");
        let d = distribution(&sorted).expect("dist");
        assert!(h.covers(P50, d.p50));
        assert!(h.covers(P95, d.p95));
        assert!(h.covers(P99, d.p99));
    }

    #[test]
    fn histogram_locate_bounds() {
        let h = Histogram::new();
        assert_eq!(h.locate(500), None);
        let h2 = Histogram::build(&[1u32]).expect("hist");
        assert_eq!(h2.locate(0), None);
        assert_eq!(h2.locate(1001), None);
    }

    #[test]
    fn ledger_record_and_recount() {
        let mut l = Ledger::new();
        assert_eq!(l.record(LatencySample::new(0, 0, 100).expect("s")), Ok(()));
        assert_eq!(l.record(LatencySample::new(3, 9, 200).expect("s")), Ok(()));
        assert_eq!(l.accepted(), 2);
        assert_eq!(l.recount(), 2);
        assert_eq!(l.rejected(), 0);
        assert_eq!(l.book(Segment::Interrupt).len(), 1);
        assert_eq!(l.book(Segment::Present).len(), 1);
    }

    #[test]
    fn ledger_record_rejects_and_counts() {
        let mut l = Ledger::new();
        assert!(l.record(LatencySample { seg: 4, day: 0, ns: 10 }).is_err());
        assert!(l.record(LatencySample { seg: 0, day: 10, ns: 10 }).is_err());
        assert!(l.record(LatencySample { seg: 0, day: 0, ns: 0 }).is_err());
        assert_eq!(l.accepted(), 0);
        assert_eq!(l.rejected(), 3);
        assert_eq!(l.recount(), 0);
    }

    #[test]
    fn ledger_digest_depends_on_values() {
        let mut a = Ledger::new();
        let mut b = Ledger::new();
        a.record(LatencySample::new(0, 0, 10).expect("s")).expect("rec");
        b.record(LatencySample::new(0, 0, 11).expect("s")).expect("rec");
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn ledger_distribution_of_each_segment() {
        let mut l = Ledger::new();
        for v in 1u32..=100 {
            l.record(LatencySample::new(1, (v % DAYS as u32) as u8, v + 100).expect("s"))
                .expect("rec");
        }
        let d = l.distribution_of(Segment::Dispatch).expect("dist");
        assert_eq!(d.count, 100);
        assert!(d.monotone());
        assert_eq!(d.min, 101);
        assert_eq!(d.max, 200);
    }

    #[test]
    fn ledger_distributions_require_all_segments() {
        let mut l = Ledger::new();
        l.record(LatencySample::new(0, 0, 10).expect("s")).expect("rec");
        assert!(l.distributions().is_err());
    }

    #[test]
    fn daily_rollup_shape() {
        let mut counts = [[0u32; SEGMENTS]; DAYS];
        for d in 0..DAYS {
            for s in 0..SEGMENTS {
                counts[d][s] = (d as u32 + 1) * (s as u32 + 1);
            }
        }
        let rolls = daily_rollup(&counts);
        assert_eq!(rolls.len(), DAYS);
        for (i, r) in rolls.iter().enumerate() {
            assert_eq!(r.day as usize, i);
            assert!(r.self_consistent());
            assert!(r.complete() || r.per_seg.iter().any(|x| *x < MIN_DAY_SAMPLES));
        }
    }

    #[test]
    fn daily_rollup_digest_order_sensitive() {
        let mut a = [[10u32; SEGMENTS]; DAYS];
        let mut b = [[10u32; SEGMENTS]; DAYS];
        a[0][0] = 11;
        b[0][1] = 11;
        let ra = daily_rollup(&a);
        let rb = daily_rollup(&b);
        assert_ne!(ra[0].digest, rb[0].digest);
    }

    #[test]
    fn audit_daily_accepts_complete() {
        let counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        let rolls = daily_rollup(&counts);
        let total = audit_daily(&rolls).expect("audit");
        assert_eq!(total, DAYS as u32 * SEGMENTS as u32 * DAY_SAMPLES);
    }

    #[test]
    fn audit_daily_rejects_short_list() {
        let counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        let mut rolls = daily_rollup(&counts);
        rolls.pop();
        assert_eq!(audit_daily(&rolls), Err(LatCode::DayBad.val()));
    }

    #[test]
    fn audit_daily_rejects_gap() {
        let counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        let mut rolls = daily_rollup(&counts);
        rolls[3].day = 7;
        assert_eq!(audit_daily(&rolls), Err(LatCode::DayBad.val()));
    }

    #[test]
    fn audit_daily_rejects_inconsistent_total() {
        let counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        let mut rolls = daily_rollup(&counts);
        rolls[2].total = rolls[2].total.saturating_add(1);
        assert_eq!(audit_daily(&rolls), Err(LatCode::RollupBad.val()));
    }

    #[test]
    fn audit_daily_rejects_thin_day() {
        let mut counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        counts[4][2] = MIN_DAY_SAMPLES - 1;
        let rolls = daily_rollup(&counts);
        assert_eq!(audit_daily(&rolls), Err(LatCode::PreconditionBad.val()));
    }

    #[test]
    fn daily_replay_same_input() {
        let counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        assert_eq!(daily_replay(&counts), Ok(true));
    }

    #[test]
    fn daily_replay_changes_with_counts() {
        let mut counts = [[DAY_SAMPLES; SEGMENTS]; DAYS];
        assert_eq!(daily_replay(&counts), Ok(true));
        counts[9][3] = counts[9][3].saturating_add(1);
        assert_eq!(daily_replay(&counts), Ok(true));
        let a = daily_rollup(&counts);
        counts[9][3] = counts[9][3].saturating_sub(2);
        let b = daily_rollup(&counts);
        assert_ne!(a[9].digest, b[9].digest);
    }

    #[test]
    fn calibration_step_roundtrip() {
        for i in 0..4 {
            let s = CalibrationStep::from_index(i as u8).expect("step");
            assert_eq!(s.index(), i as u8);
            assert_eq!(s.name(), STEP_NAMES[i]);
        }
        assert!(CalibrationStep::from_index(4).is_err());
    }

    #[test]
    fn draft_threshold_margin() {
        assert_eq!(draft_threshold(1_000), Ok(1_100));
        assert_eq!(draft_threshold(1), Ok(2));
        assert_eq!(draft_threshold(0), Err(LatCode::ThresholdBad.val()));
        assert!(draft_threshold(u32::MAX).is_err());
    }

    #[test]
    fn draft_threshold_upper_bound() {
        for p in [1u32, 9, 10, 99, 1_000, 100_000] {
            let t = draft_threshold(p).expect("threshold");
            assert!(t > p);
            assert!(t as u64 <= p as u64 * 2);
        }
    }

    #[test]
    fn calibrate_full_ledger() {
        let (mut l, _) = build(0xABCD_0061);
        let r = calibrate(&mut l).expect("cal");
        assert!(r.frozen);
        assert!(r.ordered());
        assert!(r.freeze_step_ok());
        for i in 0..SEGMENTS {
            let p99 = l
                .distribution_of(Segment::from_index(i as u8).expect("seg"))
                .expect("dist")
                .p99;
            assert_eq!(r.thresholds[i], draft_threshold(p99).expect("t"));
        }
    }

    #[test]
    fn calibrate_rejects_small_ledger() {
        let mut l = Ledger::new();
        for v in 0..MIN_CAL_SAMPLES {
            l.record(LatencySample::new(0, 0, 500 + v).expect("s")).expect("rec");
        }
        assert_eq!(calibrate(&mut l), Err(LatCode::PreconditionBad.val()));
    }

    #[test]
    fn verify_report_exact() {
        let (mut l, _) = build(0x1111_0061);
        let r = calibrate(&mut l).expect("cal");
        assert_eq!(verify_report(&mut l, &r), Ok(()));
    }

    #[test]
    fn verify_report_rejects_inflation() {
        let (mut l, _) = build(0x2222_0061);
        let r = calibrate(&mut l).expect("cal");
        let mut bad = r;
        bad.steps[0].samples = bad.steps[0].samples.saturating_add(1);
        assert_eq!(verify_report(&mut l, &bad), Err(LatCode::ReportBad.val()));
    }

    #[test]
    fn verify_report_rejects_threshold_drift() {
        let (mut l, _) = build(0x3333_0061);
        let r = calibrate(&mut l).expect("cal");
        let mut bad = r;
        bad.thresholds[2] = bad.thresholds[2].saturating_add(1);
        assert_eq!(verify_report(&mut l, &bad), Err(LatCode::ReportBad.val()));
    }

    #[test]
    fn verify_report_rejects_unfrozen() {
        let (mut l, _) = build(0x4444_0061);
        let r = calibrate(&mut l).expect("cal");
        let mut bad = r;
        bad.frozen = false;
        assert_eq!(verify_report(&mut l, &bad), Err(LatCode::FreezeBad.val()));
    }

    #[test]
    fn refreeze_idempotent() {
        let (mut l, _) = build(0x5555_0061);
        let r = calibrate(&mut l).expect("cal");
        let again = refreeze(&mut l, &r).expect("again");
        assert_eq!(again, r);
    }

    #[test]
    fn refreeze_rejects_tamper() {
        let (mut l, _) = build(0x6666_0061);
        let r = calibrate(&mut l).expect("cal");
        let mut bad = r;
        bad.thresholds[0] = bad.thresholds[0].saturating_add(77);
        assert!(refreeze(&mut l, &bad).is_err());
    }

    #[test]
    fn report_inject_all_rejected() {
        let (mut l, _) = build(0x7777_0061);
        let r = calibrate(&mut l).expect("cal");
        let (rejected, made) = report_inject(&mut l, &r);
        assert_eq!(made, 10);
        assert_eq!(rejected, 10);
    }

    #[test]
    fn ledger_row_roundtrip() {
        let d = Distribution {
            count: 3840,
            min: 100,
            max: 9_000,
            mean: 1_500,
            p50: 1_200,
            p95: 4_000,
            p99: 6_500,
        };
        let row = ledger_row(LEDGER_TAG, &d, 0xDEAD_BEEF_1234_5678);
        assert!(audit_render(&row));
        let (tag, vals, digest) = row_fields(&row).expect("fields");
        assert_eq!(tag, LEDGER_TAG);
        assert_eq!(vals, [3840, 1_200, 4_000, 6_500]);
        assert_eq!(digest, 0xDEAD_BEEF_1234_5678);
    }

    #[test]
    fn render_row_zero_padding() {
        let row = render_row("T", 0, 0, 0, 0, 1);
        assert!(row.ends_with("digest=0000000000000001"));
        assert!(audit_render(&row));
    }

    #[test]
    fn parse_u32_guards() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("+1"), None);
    }

    #[test]
    fn parse_hex16_guards() {
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("00000000000000ff"), Some(255));
        assert_eq!(parse_hex16("000000000000000"), None);
        assert_eq!(parse_hex16("00000000000000FG"), None);
        assert_eq!(parse_hex16(""), None);
    }

    #[test]
    fn row_fields_rejects_shapes() {
        assert!(row_fields("").is_none());
        assert!(row_fields("A|1|2|3|4|5").is_none());
        assert!(row_fields("A|a=1|b=2|c=3|d=4").is_none());
        assert!(row_fields("|a=1|b=2|c=3|d=4|digest=0000000000000000").is_none());
        assert!(row_fields("A|a=1|b=2|c=3|d=4|digest=0000000000000000").is_some());
        assert!(row_fields("A|a=1|b=2|c=3|d=4|digest=0000000000000000|x=1").is_none());
        assert!(row_fields("A|a=1|x|b=2|c=3|d=4|digest=0000000000000000").is_none());
    }

    #[test]
    fn audit_render_rejects_corruption() {
        let good = render_row(LEDGER_TAG, 10, 20, 30, 40, 0x1234);
        assert!(audit_render(&good));
        let bad = good.replace("p95=30", "p95=030");
        assert!(!audit_render(&bad));
        let bad2 = good.replace("LEDG", "");
        assert!(!audit_render(&bad2));
    }

    #[test]
    fn ledger_digest_stable_and_seg_sensitive() {
        let base = Distribution {
            count: 10,
            min: 1,
            max: 100,
            mean: 50,
            p50: 10,
            p95: 90,
            p99: 100,
        };
        let mut a = [base; SEGMENTS];
        let d0 = ledger_digest(&a);
        assert_eq!(d0, ledger_digest(&a));
        a[3].p99 = 101;
        assert_ne!(d0, ledger_digest(&a));
    }

    #[test]
    fn synth_ledger_counts_match() {
        let (l, counts) = build(0x8888_0061);
        let total: u32 = counts.iter().flat_map(|r| r.iter()).sum();
        assert_eq!(total, DAYS as u32 * SEGMENTS as u32 * DAY_SAMPLES);
        assert_eq!(l.accepted(), total);
        assert_eq!(l.rejected(), 0);
    }

    #[test]
    fn synth_ledger_deterministic() {
        let (mut a, _) = build(0x9999_0061);
        let (mut b, _) = build(0x9999_0061);
        assert_eq!(a.digest(), b.digest());
        let da = a.distributions().expect("a");
        let db = b.distributions().expect("b");
        assert_eq!(da, db);
    }

    #[test]
    fn synth_ledger_differs_by_seed() {
        let (a, _) = build(0x0001);
        let (b, _) = build(0x0002);
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn synth_segments_monotone_means() {
        let (mut l, _) = build(0xAAAA_0061);
        let ds = l.distributions().expect("dists");
        // 四段基准延迟递增：均值必须递增（合成样本的构造性不变量）。
        assert!(ds[0].mean < ds[1].mean);
        assert!(ds[1].mean < ds[2].mean);
        assert!(ds[2].mean < ds[3].mean);
    }

    #[test]
    fn synth_samples_within_limit() {
        let mut rng = p_core::Lcg::new(7);
        let mut i = 0u32;
        while i < 2_000 {
            let s = synth_sample(&mut rng, (i % 4) as u8, (i % 10) as u8, i);
            assert!(s.ns >= 1 && s.ns <= NS_LIMIT);
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49061_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_names_unique() {
        let set = run_unx49061_checks();
        let mut names: Vec<&str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_deterministic() {
        let a = run_unx49061_checks();
        let b = run_unx49061_checks();
        assert_eq!(a.passed_count(), b.passed_count());
        assert_eq!(a.failed_count(), b.failed_count());
    }

    #[test]
    fn budget_order_is_increasing() {
        for i in 1..SEGMENTS {
            assert!(SEG_BUDGET_NS[i] > SEG_BUDGET_NS[i - 1]);
        }
    }

    #[test]
    fn day_cells_are_addressed() {
        for d in 0..DAYS {
            for s in 0..SEGMENTS {
                let c = DayCell {
                    day: d as u8,
                    seg: s as u8,
                    count: 1,
                };
                assert_eq!(c.day as usize, d);
                assert_eq!(c.seg as usize, s);
            }
        }
    }

    #[test]
    fn invalid_percentile_never_returns_value() {
        let s = [1u32, 2, 3];
        assert_eq!(nearest_rank(&s, 0), None);
        assert_eq!(nearest_rank(&s, 1001), None);
        assert_eq!(nearest_rank(&s, u32::MAX), None);
    }

    #[test]
    fn dist_monotone_violation_detected() {
        let bad = Distribution {
            count: 4,
            min: 10,
            max: 10,
            mean: 10,
            p50: 20,
            p95: 15,
            p99: 30,
        };
        assert!(!bad.monotone());
    }

    #[test]
    fn ledger_book_mut_pushes() {
        let mut l = Ledger::new();
        if let Some(b) = l.book_mut(Segment::Compose) {
            assert_eq!(b.push(1_000), Ok(1));
        }
        assert_eq!(l.book(Segment::Compose).len(), 1);
        assert!(l.book_mut(Segment::Compose).is_some());
    }

    #[test]
    fn ledger_recount_tracks_pushes() {
        let mut l = Ledger::new();
        let mut i = 0u32;
        while i < 40 {
            let seg = (i % SEGMENTS as u32) as u8;
            l.record(LatencySample::new(seg, (i % DAYS as u32) as u8, 100 + i).expect("s"))
                .expect("rec");
            i += 1;
        }
        assert_eq!(l.recount(), 40);
        assert_eq!(l.accepted(), 40);
    }

    #[test]
    fn distribution_counts_saturate_safely() {
        let mut sorted: Vec<u32> = Vec::new();
        let mut i = 0u32;
        while i < 1_000 {
            sorted.push(1 + (i % 999));
            i += 1;
        }
        let d = distribution(&sorted).expect("dist");
        assert_eq!(d.count, 1_000);
        assert!(d.monotone());
    }

    #[test]
    fn ledger_rows_all_segments() {
        let (mut l, _) = build(0xBBBB_0061);
        let ds = l.distributions().expect("dists");
        let digest = ledger_digest(&ds);
        for d in ds.iter() {
            let row = ledger_row(LEDGER_TAG, d, digest);
            assert!(audit_render(&row));
            assert!(row.contains("p99="));
        }
    }

    #[test]
    fn calibration_step_order_guard() {
        let rec = StepRecord {
            step: CalibrationStep::Estimate,
            seq: 0,
            samples: 1,
            threshold: 0,
        };
        let report = CalibrationReport {
            steps: [
                rec,
                StepRecord {
                    step: CalibrationStep::Estimate,
                    seq: 1,
                    samples: 1,
                    threshold: 0,
                },
                StepRecord {
                    step: CalibrationStep::Draft,
                    seq: 2,
                    samples: 1,
                    threshold: 1,
                },
                StepRecord {
                    step: CalibrationStep::Freeze,
                    seq: 3,
                    samples: 1,
                    threshold: 1,
                },
            ],
            thresholds: [1; SEGMENTS],
            frozen: true,
            digest: 0,
        };
        assert!(!report.ordered());
        assert!(report.freeze_step_ok());
    }

    #[test]
    fn unknown_code_message() {
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 100), UNKNOWN);
        assert_eq!(LatCode::Overflow.detail(), detail(SEG_BASE + 14));
    }
}
