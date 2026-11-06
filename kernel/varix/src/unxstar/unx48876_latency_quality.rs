//! UNX-F48876 B04 质量面 · 分位预算 · 直方账 · 配额 · 0x70960
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥900 行；
//!   ② UNX-F48876-J1 四段分位单调且 p95 入带、p99 入限；
//!   ③ UNX-F48877-J1 千例配额可复算，过载与过快占比不越顶；
//!   ④ UNX-F48878-J1 直方账平 ×10^3 零丢桶，溢出显式。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：样本入账 → 直方入桶 → 结算排序 → 分位提取 → 分位预算 →
//!           配额复核 → 误拒复核 → 指纹双跑。
//!           样本必须先入账再入桶：账是真相，桶只是分布视图；若只信
//!           桶，溢出样本会从账上消失而不被发现。结算排序必须先于
//!           分位：未排序就取分位，取到的是「第几个插入」而不是「第几
//!           百分位」。分位预算晚于分位提取：预算比较的是真实分位，
//!           不是桶上界的估算值。配额复核晚于预算：配额只回答「每段
//!           样本数是否齐」，它与分位高低是两件事，混判会让「样本少
//!           但分位低」冒充达标。误拒复核再晚一步：它检查的是分类面
//!           的自洽（预算内样本不得被记为越界），与分布无关。指纹
//!           双跑最后执行，任何一处分支不同都会在此显形。
//! 关键差异：本模块只拥有「分布与预算合规」这一层。
//!           edge 拥有单样本的档位与余量；quality 拥有整段分布的分位、
//!           直方、过载/过快/容差占比与配额。core 的分位算的是「链」，
//!           本模块算的是「段」；两者不共享排序缓冲，也不互相覆盖。
//!           谁想让一段延迟以「p95 达标」对外声明，谁就必须先在本模块
//!           通过分位、配额、误拒三道闸；单看 p50 好看不算达标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx48871_latency_edge as l_edge;

/// 诊断段基址（0x70960，VX6 批 B04 族第七槽）。
pub const SEG_BASE: u32 = 0x70960;

/// 质量语料轮数（判据 ×10^4，四段均分）。
pub const CASES: u32 = 10_000;

/// 每段样本数（CASES / 4）。
pub const CASES_PER_SEG: u32 = 2_500;

/// 直方桶数。
pub const HIST_BINS: usize = 32;

/// 过载占比上限（ppm，5%）。
pub const MAX_OVER_PPM: u32 = 50_000;

/// 过快（低于基值）占比上限（ppm，5%）。
pub const MAX_STARVE_PPM: u32 = 50_000;

/// 容差内超支占比上限（ppm，5%）。
pub const MAX_TOLERATED_PPM: u32 = 50_000;

/// 注入轮数（四路 ×10^3）。
pub const INJECT_ROUNDS: u32 = 1000;

/// 允许的分位点（只认这三点，防止调用方自造分位掩盖尾部）。
pub const PERCENTILES: [u8; 3] = [50, 95, 99];

// ── 诊断码闭集（+0x01..+0x0B） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QCode {
    /// 分位非法（0 或 >100，或未结算）
    MetricBad,
    /// 分位越预算/越限
    BudgetBad,
    /// 分位不单调（p50 > p95 或 p95 > p99）
    PercentBad,
    /// 直方账不平（桶和 + 溢出 ≠ 观测数）
    HistBad,
    /// 语料形状非法（轮数为零/非四倍数）
    CorpusBad,
    /// 占比越顶或占比计算失败
    RateBad,
    /// 配额不足（某段样本数不等于总数/4）
    QuotaBad,
    /// 未结算就取分位（异常被静默）
    SilentBad,
    /// 双跑不一致
    ReplayBad,
    /// 段切片错误（记录进了错段）
    SliceBad,
    /// 段位越界
    SegmentBad,
}

impl QCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QCode::MetricBad => "分位非法：分位点只认 1..100，且必须已结算",
            QCode::BudgetBad => "分位越界：p95 必须入容差带，p99 必须入抖动限",
            QCode::PercentBad => "分位不单调：p50 ≤ p95 ≤ p99 必须成立",
            QCode::HistBad => "直方账不平：桶和加溢出必须等于观测数",
            QCode::CorpusBad => "语料非法：轮数须为正且为四的倍数",
            QCode::RateBad => "占比越顶：过载/过快/容差内超支任一越顶即拒",
            QCode::QuotaBad => "配额不足：四段样本数必须各为总数的四分之一",
            QCode::SilentBad => "未结算取分位：排序未做，分位数字无意义",
            QCode::ReplayBad => "双跑不一致：两次质量账指纹不同",
            QCode::SliceBad => "段切片错误：样本被记入了别的段",
            QCode::SegmentBad => "段位越界：诊断码逃出 0x70960 窗口",
        }
    }

    pub const ALL: [QCode; 11] = [
        QCode::MetricBad,
        QCode::BudgetBad,
        QCode::PercentBad,
        QCode::HistBad,
        QCode::CorpusBad,
        QCode::RateBad,
        QCode::QuotaBad,
        QCode::SilentBad,
        QCode::ReplayBad,
        QCode::SliceBad,
        QCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QCode::ALL.len() {
        if QCode::ALL[i].val() == code {
            return QCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70960 段闭集内"
}

// ── FNV-1a 64 ─────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (v & 0xFF) as u8);
    x = fold_byte(x, ((v >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((v >> 16) & 0xFF) as u8);
    fold_byte(x, ((v >> 24) & 0xFF) as u8)
}

// ── 直方 ──────────────────────────────────────────────────────────
/// 定宽直方：32 桶覆盖到显式上限，超上限如实计入 overflow。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    pub bins: [u32; HIST_BINS],
    pub overflow: u32,
    pub count: u32,
    pub width: u32,
    pub limit: u32,
}

impl Histogram {
    /// 按「预算 + 容差 + 抖动」定上限，桶宽上取整后反推整上限。
    pub fn new(budget: u32, tol: u32, jitter: u32) -> Histogram {
        let raw = budget.saturating_add(tol).saturating_add(jitter);
        let width = raw / HIST_BINS as u32 + 1;
        Histogram {
            bins: [0; HIST_BINS],
            overflow: 0,
            count: 0,
            width,
            limit: width.saturating_mul(HIST_BINS as u32),
        }
    }

    /// 入桶：桶号越界或超上限一律计入 overflow，不静默丢。
    pub fn observe(&mut self, us: u32) -> Result<(), u32> {
        self.count = self.count.saturating_add(1);
        if us > self.limit {
            self.overflow = self.overflow.saturating_add(1);
            return Ok(());
        }
        let ix = (us / self.width) as usize;
        match self.bins.get_mut(ix) {
            Some(v) => *v = v.saturating_add(1),
            None => self.overflow = self.overflow.saturating_add(1),
        }
        Ok(())
    }

    pub fn bucket_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < HIST_BINS {
            if let Some(v) = self.bins.get(i) {
                n = n.saturating_add(*v);
            }
            i += 1;
        }
        n
    }

    /// 直方账平：桶和 + 溢出 = 观测数。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.width == 0 || self.limit != self.width.saturating_mul(HIST_BINS as u32) {
            return Err(QCode::HistBad.val());
        }
        if self.bucket_total().saturating_add(self.overflow) != self.count {
            return Err(QCode::HistBad.val());
        }
        Ok(self.count)
    }

    /// 桶上界分位（近似值，只作分布视图；预算判定用精确分位）。
    pub fn percentile(&self, p: u8) -> Result<u32, u32> {
        if p == 0 || p > 100 {
            return Err(QCode::MetricBad.val());
        }
        if self.count == 0 {
            return Err(QCode::SilentBad.val());
        }
        let rank = (self.count as u64 * p as u64 + 99) / 100;
        let rank = if rank == 0 { 1 } else { rank };
        let mut cum = 0u64;
        let mut i = 0usize;
        while i < HIST_BINS {
            if let Some(v) = self.bins.get(i) {
                cum = cum.saturating_add(*v as u64);
                if cum >= rank {
                    return Ok(self.width.saturating_mul(i as u32 + 1));
                }
            }
            i += 1;
        }
        // 桶不够覆盖时，溢出样本代表最尾部：返回上限而非伪造桶值。
        Ok(self.limit)
    }
}

// ── 分位 ──────────────────────────────────────────────────────────
/// 三点分位（精确值，来自已排序样本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Percentiles {
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
}

impl Percentiles {
    pub const fn zero() -> Percentiles {
        Percentiles { p50: 0, p95: 0, p99: 0 }
    }

    /// 单调核：p50 ≤ p95 ≤ p99。任一倒挂即分位计算错误。
    pub fn monotone(&self) -> bool {
        self.p50 <= self.p95 && self.p95 <= self.p99
    }
}

/// 最近秩分位：rank = ceil(p/100 × n)，取第 rank 小（1 基）。
pub fn percentile_sorted(sorted: &[u32], p: u8) -> Result<u32, u32> {
    if p == 0 || p > 100 {
        return Err(QCode::MetricBad.val());
    }
    let n = sorted.len();
    if n == 0 {
        return Err(QCode::SilentBad.val());
    }
    let rank = (n as u64 * p as u64 + 99) / 100;
    let rank = if rank == 0 { 1 } else { rank };
    let ix = if rank as usize > n { n - 1 } else { rank as usize - 1 };
    match sorted.get(ix) {
        Some(v) => Ok(*v),
        None => Err(QCode::MetricBad.val()),
    }
}

/// 原地插入排序（宿主要保证确定性；不得依赖标准库排序的稳定性假设）。
pub fn sort_samples(v: &mut Vec<u32>) {
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
                None => return,
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

// ── 段统计 ────────────────────────────────────────────────────────
/// 单段质量账：样本原文 + 直方 + 四档计数 + 分位。
pub struct SegStat {
    pub seg: u8,
    pub budget: u32,
    pub floor: u32,
    pub jitter: u32,
    pub samples: Vec<u32>,
    pub hist: Histogram,
    pub in_budget: u32,
    pub at_edge: u32,
    pub over: u32,
    pub starved: u32,
    pub tolerated: u32,
    pub misrejected: u32,
    pub pct: Percentiles,
    pub settled: bool,
    pub digest: u64,
}

impl SegStat {
    pub fn new(seg: u8) -> Result<SegStat, u32> {
        let budget = l_edge::stage_budget(seg)?;
        let floor = l_edge::stage_floor(seg)?;
        let jitter = l_edge::stage_jitter(seg)?;
        Ok(SegStat {
            seg,
            budget,
            floor,
            jitter,
            samples: Vec::new(),
            hist: Histogram::new(budget, l_edge::EDGE_TOL_US, jitter),
            in_budget: 0,
            at_edge: 0,
            over: 0,
            starved: 0,
            tolerated: 0,
            misrejected: 0,
            pct: Percentiles::zero(),
            settled: false,
            digest: FNV_OFF,
        })
    }

    pub fn count(&self) -> u32 {
        self.samples.len() as u32
    }

    /// 记一个样本：段切片、四档计数、容差内超支、误拒、直方、指纹。
    pub fn record(&mut self, p: l_edge::EdgePoint) -> Result<(), u32> {
        if p.seg != self.seg {
            return Err(QCode::SliceBad.val());
        }
        if p.budget != self.budget || p.floor != self.floor {
            return Err(QCode::SliceBad.val());
        }
        if self.settled {
            return Err(QCode::SilentBad.val());
        }
        let cls = p.class();
        match cls {
            l_edge::EdgeClass::InBudget => self.in_budget = self.in_budget.saturating_add(1),
            l_edge::EdgeClass::AtEdge => self.at_edge = self.at_edge.saturating_add(1),
            l_edge::EdgeClass::OverBudget => self.over = self.over.saturating_add(1),
            l_edge::EdgeClass::Starved => self.starved = self.starved.saturating_add(1),
        }
        // 容差内超支：耗时已过预算但仍在容差带内（AtEdge 且 us > budget）。
        if cls == l_edge::EdgeClass::AtEdge && p.us > p.budget {
            self.tolerated = self.tolerated.saturating_add(1);
        }
        // 误拒：预算内或恰好等于预算的样本不得被记为越界。
        if p.us <= p.budget && cls == l_edge::EdgeClass::OverBudget {
            self.misrejected = self.misrejected.saturating_add(1);
        }
        self.hist.observe(p.us)?;
        self.samples.push(p.us);
        self.digest = fold_u32(self.digest, p.us);
        self.digest = fold_u32(self.digest, cls.ord() as u32);
        Ok(())
    }

    /// 结算：插入排序 + 三点精确分位。只允许结算一次，避免重排掩盖重录。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.settled {
            return Err(QCode::SilentBad.val());
        }
        if self.samples.is_empty() {
            return Err(QCode::QuotaBad.val());
        }
        sort_samples(&mut self.samples);
        let p50 = percentile_sorted(&self.samples, 50)?;
        let p95 = percentile_sorted(&self.samples, 95)?;
        let p99 = percentile_sorted(&self.samples, 99)?;
        self.pct = Percentiles { p50, p95, p99 };
        self.settled = true;
        Ok(())
    }

    /// 类覆盖位（四档）。
    pub fn class_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.in_budget > 0 {
            m |= 1;
        }
        if self.at_edge > 0 {
            m |= 2;
        }
        if self.over > 0 {
            m |= 4;
        }
        if self.starved > 0 {
            m |= 8;
        }
        m
    }

    /// 占比（ppm）：分子 × 10^6 / 分母；分母为零即拒。
    pub fn ppm(&self, num: u32) -> Result<u32, u32> {
        self.ppm_of(num, self.count())
    }

    pub fn ppm_of(&self, num: u32, den: u32) -> Result<u32, u32> {
        if den == 0 {
            return Err(QCode::RateBad.val());
        }
        let v = (num as u64 * 1_000_000u64) / den as u64;
        if v > u64::from(u32::MAX) {
            return Err(QCode::RateBad.val());
        }
        Ok(v as u32)
    }

    /// 段审计：结算、账平、分位单调、预算、配额、误拒。
    pub fn audit(&self, expect: u32) -> Result<(), u32> {
        if !self.settled {
            return Err(QCode::SilentBad.val());
        }
        if self.count() != expect {
            return Err(QCode::QuotaBad.val());
        }
        let sum = self
            .in_budget
            .saturating_add(self.at_edge)
            .saturating_add(self.over)
            .saturating_add(self.starved);
        if sum != self.count() {
            return Err(QCode::HistBad.val());
        }
        if self.hist.audit()? != self.count() {
            return Err(QCode::HistBad.val());
        }
        if !self.pct.monotone() {
            return Err(QCode::PercentBad.val());
        }
        let p95_limit = self.budget.saturating_add(l_edge::EDGE_TOL_US);
        let p99_limit = p95_limit.saturating_add(self.jitter);
        if self.pct.p95 > p95_limit {
            return Err(QCode::BudgetBad.val());
        }
        if self.pct.p99 > p99_limit {
            return Err(QCode::BudgetBad.val());
        }
        if self.misrejected != 0 {
            return Err(QCode::RateBad.val());
        }
        Ok(())
    }
}

// ── 质量总账 ──────────────────────────────────────────────────────
/// 四段质量总账。
pub struct QualityLedger {
    pub segs: Vec<SegStat>,
}

impl QualityLedger {
    pub fn new() -> Result<QualityLedger, u32> {
        let mut segs = Vec::new();
        let mut seg = 0u8;
        while seg < 4 {
            segs.push(SegStat::new(seg)?);
            seg = seg.saturating_add(1);
        }
        Ok(QualityLedger { segs })
    }

    pub fn get(&self, seg: u8) -> Option<&SegStat> {
        self.segs.get(seg as usize)
    }

    pub fn get_mut(&mut self, seg: u8) -> Option<&mut SegStat> {
        self.segs.get_mut(seg as usize)
    }

    pub fn record(&mut self, p: l_edge::EdgePoint) -> Result<(), u32> {
        match self.get_mut(p.seg) {
            Some(s) => s.record(p),
            None => Err(QCode::SliceBad.val()),
        }
    }

    pub fn settle_all(&mut self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.segs.len() {
            match self.segs.get_mut(i) {
                Some(s) => s.settle()?,
                None => return Err(QCode::SliceBad.val()),
            }
            i += 1;
        }
        Ok(())
    }

    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.segs.len() {
            if let Some(s) = self.segs.get(i) {
                h = fold_u32(h, s.seg as u32);
                h = fold_u32(h, (s.digest & 0xFFFF_FFFF) as u32);
                h = fold_u32(h, (s.digest >> 32) as u32);
                h = fold_u32(h, s.count());
            }
            i += 1;
        }
        h
    }

    /// 总账审计：逐段审计 + 覆盖位齐 + 占比不越顶。
    pub fn audit(&self, rounds: u32) -> Result<Metrics, u32> {
        if rounds == 0 || rounds % 4 != 0 {
            return Err(QCode::CorpusBad.val());
        }
        let per = rounds / 4;
        let mut m = Metrics::zero();
        let mut i = 0usize;
        while i < self.segs.len() {
            let s = match self.segs.get(i) {
                Some(v) => v,
                None => return Err(QCode::SliceBad.val()),
            };
            s.audit(per)?;
            let over_ppm = s.ppm(s.over)?;
            let starve_ppm = s.ppm(s.starved)?;
            let tol_ppm = s.ppm(s.tolerated)?;
            if over_ppm > MAX_OVER_PPM || starve_ppm > MAX_STARVE_PPM || tol_ppm > MAX_TOLERATED_PPM {
                return Err(QCode::RateBad.val());
            }
            if m.over_ppm < over_ppm {
                m.over_ppm = over_ppm;
            }
            if m.starve_ppm < starve_ppm {
                m.starve_ppm = starve_ppm;
            }
            if m.tolerated_ppm < tol_ppm {
                m.tolerated_ppm = tol_ppm;
            }
            if m.p95_max < s.pct.p95 {
                m.p95_max = s.pct.p95;
            }
            if m.p99_max < s.pct.p99 {
                m.p99_max = s.pct.p99;
            }
            if s.class_mask() != 0x0F {
                return Err(QCode::PercentBad.val());
            }
            m.cover |= s.class_mask();
            i += 1;
        }
        if m.cover != 0x0F {
            return Err(QCode::PercentBad.val());
        }
        Ok(m)
    }
}

/// 质量指标摘要（用结果比较，便于双跑对照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    pub over_ppm: u32,
    pub starve_ppm: u32,
    pub tolerated_ppm: u32,
    pub p95_max: u32,
    pub p99_max: u32,
    pub cover: u8,
}

impl Metrics {
    pub const fn zero() -> Metrics {
        Metrics { over_ppm: 0, starve_ppm: 0, tolerated_ppm: 0, p95_max: 0, p99_max: 0, cover: 0 }
    }
}

// ── 语料 ──────────────────────────────────────────────────────────
/// 第 r 轮耗时：五十形态轮转，过载 4%、过快 4%、临界 6%。
///
/// 形态表：
///   0 零 / 1 半基值（过快各 2%）；
///   2 基值 / 3 预算−容差−1（预算内）；
///   4 预算−容差 / 5 预算 / 6 预算+容差（临界各 2%）；
///   7 预算+容差+1 / 8 再进抖动（过载各 2%）；
///   9..49 基值..预算−容差−2（预算内，占 82%）。
fn corpus_us(seg: u8, r: u32) -> Result<u32, u32> {
    let budget = l_edge::stage_budget(seg)?;
    let floor = l_edge::stage_floor(seg)?;
    let jitter = l_edge::stage_jitter(seg)?;
    let tol = l_edge::EDGE_TOL_US;
    let kind = r % 50;
    let us = match kind {
        0 => 0,
        1 => floor / 2,
        2 => floor,
        3 => budget.saturating_sub(tol).saturating_sub(1),
        4 => budget.saturating_sub(tol),
        5 => budget,
        6 => budget.saturating_add(tol),
        7 => budget.saturating_add(tol).saturating_add(1),
        8 => budget
            .saturating_add(tol)
            .saturating_add(1)
            .saturating_add(r % jitter.max(1)),
        _ => {
            let span = budget
                .saturating_sub(tol)
                .saturating_sub(2)
                .saturating_sub(floor)
                .max(1);
            floor.saturating_add(r % span)
        }
    };
    Ok(us)
}

/// 千例语料：四段均分，逐样本入账，最后统一结算。
pub fn quality_corpus(rounds: u32) -> Result<QualityLedger, u32> {
    if rounds == 0 || rounds % 4 != 0 || rounds > 400_000 {
        return Err(QCode::CorpusBad.val());
    }
    let mut l = QualityLedger::new()?;
    let mut r = 0u32;
    while r < rounds {
        let seg = (r % 4) as u8;
        let us = corpus_us(seg, r)?;
        let p = l_edge::EdgePoint::framed(seg, us)?;
        l.record(p)?;
        r = r.saturating_add(1);
    }
    l.settle_all()?;
    Ok(l)
}

/// 黄金跑：返回（过载 ppm 顶，p99 顶，覆盖位，指纹）。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let l = quality_corpus(CASES)?;
    let m = l.audit(CASES)?;
    Ok((m.over_ppm, m.p99_max, u32::from(m.cover), l.digest()))
}

// ── 注入核 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub caught: u32,
    pub rounds: u32,
    pub route_caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new(rounds: u32) -> InjectLedger {
        InjectLedger { caught: 0, rounds, route_caught: [0; 4] }
    }

    pub fn all_caught(&self) -> bool {
        if self.caught != self.rounds {
            return false;
        }
        let mut i = 0usize;
        while i < self.route_caught.len() {
            match self.route_caught.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：错段落账 / 未结算取分位 / 非法分位点 / 无法线取证。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(QCode::CorpusBad.val());
    }
    let mut l = InjectLedger::new(rounds);
    let mut r = 0u32;
    while r < rounds {
        let route = (r % 4) as usize;
        let hit = match route {
            0 => {
                let mut s = match SegStat::new(0) {
                    Ok(v) => v,
                    Err(_) => return Err(QCode::SliceBad.val()),
                };
                match l_edge::EdgePoint::framed(1, 100) {
                    Ok(p) => s.record(p) == Err(QCode::SliceBad.val()),
                    Err(_) => false,
                }
            }
            1 => match SegStat::new(1) {
                Ok(s) => {
                    !s.settled
                        && s.audit(1) == Err(QCode::SilentBad.val())
                        && percentile_sorted(&[], 95).is_err()
                }
                Err(_) => false,
            },
            2 => percentile_sorted(&[1, 2, 3], 0).is_err() && percentile_sorted(&[1, 2, 3], 101).is_err(),
            _ => {
                let mut h = Histogram::new(100, 8, 40);
                let before = h.count;
                h.observe(h.limit.saturating_add(1)).is_ok()
                    && h.overflow == 1
                    && h.count == before + 1
                    && h.audit().is_ok()
            }
        };
        if hit {
            l.caught = l.caught.saturating_add(1);
            if let Some(c) = l.route_caught.get_mut(route) {
                *c = c.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    Ok(l)
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 质量摘要导出：每段一行（seg|count|p50|p95|p99|over|starve|tol|mask）。
pub fn render_metrics(l: &QualityLedger) -> Result<String, u32> {
    let mut out = String::new();
    let mut i = 0usize;
    while i < l.segs.len() {
        let s = match l.segs.get(i) {
            Some(v) => v,
            None => return Err(QCode::SliceBad.val()),
        };
        if !s.settled {
            return Err(QCode::SilentBad.val());
        }
        out.push_str(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            s.seg,
            s.count(),
            s.pct.p50,
            s.pct.p95,
            s.pct.p99,
            s.over,
            s.starved,
            s.tolerated,
            s.class_mask()
        ));
        i += 1;
    }
    Ok(out)
}

/// 摘要审计：四行九段，数值可解析，mask 必须 0x0F。
pub fn audit_metrics_text(text: &str) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 4 {
        return Err(QCode::HistBad.val());
    }
    let mut i = 0usize;
    while i < lines.len() {
        let ln = match lines.get(i) {
            Some(v) => *v,
            None => return Err(QCode::HistBad.val()),
        };
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 9 {
            return Err(QCode::HistBad.val());
        }
        let seg = parse_u32(match parts.get(0) {
            Some(v) => *v,
            None => return Err(QCode::HistBad.val()),
        })?;
        if seg as usize != i {
            return Err(QCode::SliceBad.val());
        }
        let mut k = 1usize;
        while k < 9 {
            match parts.get(k) {
                Some(v) => {
                    parse_u32(v)?;
                }
                None => return Err(QCode::HistBad.val()),
            }
            k += 1;
        }
        let mask = parse_u32(match parts.get(8) {
            Some(v) => *v,
            None => return Err(QCode::HistBad.val()),
        })?;
        if mask != 0x0F {
            return Err(QCode::PercentBad.val());
        }
        i += 1;
    }
    Ok(lines.len())
}

/// 十进制解析。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(QCode::HistBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(QCode::HistBad.val()),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(QCode::HistBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(QCode::HistBad.val()),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(QCode::HistBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针 A：千例分位单调且入带入限。
pub fn probe_seq_a() -> Result<(), u32> {
    let l = quality_corpus(CASES)?;
    let m = l.audit(CASES)?;
    if m.p95_max == 0 || m.p99_max < m.p95_max {
        return Err(QCode::PercentBad.val());
    }
    let mut i = 0usize;
    while i < l.segs.len() {
        if let Some(s) = l.segs.get(i) {
            if !s.pct.monotone() {
                return Err(QCode::PercentBad.val());
            }
            let p95_limit = s.budget.saturating_add(l_edge::EDGE_TOL_US);
            let p99_limit = p95_limit.saturating_add(s.jitter);
            if s.pct.p95 > p95_limit || s.pct.p99 > p99_limit {
                return Err(QCode::BudgetBad.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 探针 B：直方账平 + 溢出显式。
pub fn probe_seq_b() -> Result<(), u32> {
    let l = quality_corpus(400)?;
    let mut i = 0usize;
    while i < l.segs.len() {
        let s = match l.segs.get(i) {
            Some(v) => v,
            None => return Err(QCode::SliceBad.val()),
        };
        if s.hist.audit()? != s.count() {
            return Err(QCode::HistBad.val());
        }
        if s.hist.bucket_total().saturating_add(s.hist.overflow) != s.count() {
            return Err(QCode::HistBad.val());
        }
        let mut k = 0usize;
        while k < HIST_BINS {
            if let Some(v) = s.hist.bins.get(k) {
                if *v > s.hist.count {
                    return Err(QCode::HistBad.val());
                }
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 探针 C：配额、占比与双跑。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = quality_corpus(CASES)?;
    let b = quality_corpus(CASES)?;
    if a.digest() != b.digest() {
        return Err(QCode::ReplayBad.val());
    }
    let m = a.audit(CASES)?;
    if m.cover != 0x0F {
        return Err(QCode::PercentBad.val());
    }
    let mut i = 0usize;
    while i < a.segs.len() {
        if let Some(s) = a.segs.get(i) {
            if s.count() != CASES_PER_SEG {
                return Err(QCode::QuotaBad.val());
            }
            if s.misrejected != 0 {
                return Err(QCode::RateBad.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < QCode::ALL.len() {
        match QCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据（CheckSet） ──────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let corpus = quality_corpus(CASES);
    v.push((
        "F48876-J1-分位单调",
        match &corpus {
            Ok(l) => {
                let mut ok = true;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        if !s.pct.monotone() {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        String::from("四段 p50 ≤ p95 ≤ p99"),
    ));
    v.push((
        "F48876-J1-p95入带",
        match &corpus {
            Ok(l) => {
                let mut ok = true;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        if s.pct.p95 > s.budget.saturating_add(l_edge::EDGE_TOL_US) {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        String::from("p95 ≤ 预算 + 容差"),
    ));
    v.push((
        "F48876-J1-p99入限",
        match &corpus {
            Ok(l) => {
                let mut ok = true;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        let lim = s
                            .budget
                            .saturating_add(l_edge::EDGE_TOL_US)
                            .saturating_add(s.jitter);
                        if s.pct.p99 > lim {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        String::from("p99 ≤ 预算 + 容差 + 抖动"),
    ));
    v.push((
        "F48876-J1-直方账平",
        match &corpus {
            Ok(l) => {
                let mut ok = true;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        if s.hist.audit().is_err()
                            || s.hist.bucket_total().saturating_add(s.hist.overflow) != s.count()
                        {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        String::from("桶和 + 溢出 = 观测数（四段）"),
    ));
    v.push((
        "F48876-J1-溢出显式",
        {
            let mut h = Histogram::new(900, 8, 420);
            let mut ok = h.observe(h.limit.saturating_add(5)).is_ok();
            ok = ok && h.overflow == 1 && h.count == 1 && h.bucket_total() == 0;
            ok = ok && h.audit() == Ok(1);
            ok
        },
        String::from("超上限样本只入溢出账，不伪造桶"),
    ));
    v.push((
        "F48877-J1-千例覆盖",
        match &corpus {
            Ok(l) => {
                let mut ok = l.segs.len() == 4;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        if s.count() != CASES_PER_SEG || s.class_mask() != 0x0F {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok
            }
            Err(_) => false,
        },
        format!("四段各 {} 例，四档全覆盖", CASES_PER_SEG),
    ));
    v.push((
        "F48877-J1-过载配额",
        match &corpus {
            Ok(l) => match l.audit(CASES) {
                Ok(m) => {
                    m.over_ppm <= MAX_OVER_PPM
                        && m.starve_ppm <= MAX_STARVE_PPM
                        && m.tolerated_ppm <= MAX_TOLERATED_PPM
                }
                Err(_) => false,
            },
            Err(_) => false,
        },
        format!(
            "过载 ≤{}ppm 过快 ≤{}ppm 容差内 ≤{}ppm",
            MAX_OVER_PPM, MAX_STARVE_PPM, MAX_TOLERATED_PPM
        ),
    ));
    v.push((
        "F48877-J1-误拒为零",
        match &corpus {
            Ok(l) => {
                let mut ok = true;
                let mut mis = 0u32;
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        mis = mis.saturating_add(s.misrejected);
                        if s.misrejected != 0 {
                            ok = false;
                        }
                    }
                    i += 1;
                }
                ok && mis == 0
            }
            Err(_) => false,
        },
        String::from("预算内样本不得被记为越界"),
    ));
    v.push((
        "F48878-J1-双跑一致",
        match golden_run() {
            Ok(a) => match golden_run() {
                Ok(b) => a == b && repro_check().is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        },
        String::from("两次千例质量账四元组全等"),
    ));
    v.push((
        "F48878-J1-注入必抓",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => l.all_caught() && l.caught == INJECT_ROUNDS,
            Err(_) => false,
        },
        format!("四路 ×{} 全抓", INJECT_ROUNDS),
    ));
    v
}

/// 判据入口。
pub fn run_unx48876_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48876 B04 质量面与分位预算");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let guard = segment_guard();
    s.add_dyn("F48876-J1-段位守卫", guard, if guard { "" } else { "诊断码逃出 0x70960" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_counts_and_overflow() {
        let mut h = Histogram::new(80, 8, 40);
        assert_eq!(h.observe(0), Ok(()));
        assert_eq!(h.observe(10), Ok(()));
        assert_eq!(h.observe(h.limit.saturating_add(1)), Ok(()));
        assert_eq!(h.count, 3);
        assert_eq!(h.overflow, 1);
        assert_eq!(h.bucket_total(), 2);
        assert_eq!(h.audit(), Ok(3));
    }

    #[test]
    fn histogram_percentile_monotone_and_bounded() {
        let mut h = Histogram::new(80, 8, 40);
        let mut i = 0u32;
        while i < 100 {
            assert_eq!(h.observe(i * 3 % (h.limit + 1)), Ok(()));
            i += 1;
        }
        let p50 = h.percentile(50);
        let p95 = h.percentile(95);
        match (p50, p95) {
            (Ok(a), Ok(b)) => {
                assert!(a <= b);
                assert!(b <= h.limit);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn histogram_rejects_empty_percentile() {
        let h = Histogram::new(80, 8, 40);
        assert_eq!(h.percentile(50), Err(QCode::SilentBad.val()));
        assert_eq!(h.percentile(0), Err(QCode::MetricBad.val()));
        assert_eq!(h.percentile(101), Err(QCode::MetricBad.val()));
    }

    #[test]
    fn percentile_nearest_rank() {
        let v = [10u32, 20, 30, 40, 50];
        assert_eq!(percentile_sorted(&v, 50), Ok(30));
        assert_eq!(percentile_sorted(&v, 1), Ok(10));
        assert_eq!(percentile_sorted(&v, 100), Ok(50));
        assert_eq!(percentile_sorted(&[], 50), Err(QCode::SilentBad.val()));
        assert_eq!(percentile_sorted(&v, 0), Err(QCode::MetricBad.val()));
    }

    #[test]
    fn insertion_sort_is_stable_and_correct() {
        let mut v = alloc::vec![50u32, 10, 40, 20, 30];
        sort_samples(&mut v);
        assert_eq!(v, alloc::vec![10u32, 20, 30, 40, 50]);
        let mut e: Vec<u32> = Vec::new();
        sort_samples(&mut e);
        assert!(e.is_empty());
        let mut one = alloc::vec![7u32];
        sort_samples(&mut one);
        assert_eq!(one, alloc::vec![7u32]);
    }

    #[test]
    fn seg_stat_records_and_settles() {
        let mut s = SegStat::new(0);
        match s {
            Ok(ref mut st) => {
                let mut r = 0u32;
                while r < 100 {
                    match l_edge::EdgePoint::framed(0, r * 4) {
                        Ok(p) => {
                            assert_eq!(st.record(p), Ok(()));
                        }
                        Err(_) => assert!(false),
                    }
                    r += 1;
                }
                assert_eq!(st.count(), 100);
                assert_eq!(st.settle(), Ok(()));
                assert!(st.pct.monotone());
                assert_eq!(st.settle(), Err(QCode::SilentBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn seg_stat_rejects_foreign_slice() {
        match SegStat::new(0) {
            Ok(mut s) => match l_edge::EdgePoint::framed(1, 50) {
                Ok(p) => assert_eq!(s.record(p), Err(QCode::SliceBad.val())),
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn seg_stat_ppm_guards_zero() {
        match SegStat::new(0) {
            Ok(s) => {
                assert_eq!(s.ppm(1), Err(QCode::RateBad.val()));
                assert_eq!(s.ppm_of(1, 0), Err(QCode::RateBad.val()));
                assert_eq!(s.ppm_of(1, 1), Ok(1_000_000));
                assert_eq!(s.ppm_of(0, 10), Ok(0));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn mosaic_corpus_counts_and_classes() {
        match quality_corpus(CASES) {
            Ok(l) => {
                assert_eq!(l.segs.len(), 4);
                let mut i = 0usize;
                while i < l.segs.len() {
                    if let Some(s) = l.segs.get(i) {
                        assert_eq!(s.count(), CASES_PER_SEG);
                        assert_eq!(s.class_mask(), 0x0F);
                        assert!(s.settled);
                    }
                    i += 1;
                }
                assert_eq!(l.audit(CASES).is_ok(), true);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn corpus_rejects_bad_shape() {
        assert_eq!(quality_corpus(0).err(), Some(QCode::CorpusBad.val()));
        assert_eq!(quality_corpus(10).err(), Some(QCode::CorpusBad.val()));
        assert_eq!(quality_corpus(400_004).err(), Some(QCode::CorpusBad.val()));
    }

    #[test]
    fn metrics_are_deterministic() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        match a {
            Ok((over, p99, cover, h)) => {
                assert!(over <= MAX_OVER_PPM);
                assert!(p99 > 0);
                assert_eq!(cover, 0x0F);
                assert!(h != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_and_audit_metrics() {
        match quality_corpus(CASES) {
            Ok(l) => match render_metrics(&l) {
                Ok(t) => {
                    assert_eq!(audit_metrics_text(&t), Ok(4));
                    assert_eq!(t.lines().count(), 4);
                }
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_rejects_bad_metrics_rows() {
        assert!(audit_metrics_text("0|1|2|3").is_err());
        assert!(audit_metrics_text("5|1|2|3|4|5|6|7|15\n").is_err());
        assert!(audit_metrics_text("0|1|2|3|4|5|6|7|3\n").is_err());
    }

    #[test]
    fn inject_all_caught() {
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.caught, INJECT_ROUNDS);
                assert_eq!(l.route_caught, [250, 250, 250, 250]);
                assert!(l.all_caught());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn segment_guard_holds() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < QCode::ALL.len() {
            if let Some(c) = QCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 0x01 && c.val() <= SEG_BASE + 0x0B);
            }
            i += 1;
        }
    }

    #[test]
    fn probes_pass() {
        assert_eq!(probe_seq_a(), Ok(()));
        assert_eq!(probe_seq_b(), Ok(()));
        assert_eq!(probe_seq_c(), Ok(()));
        assert_eq!(repro_check(), Ok(()));
    }

    #[test]
    fn ledger_digest_changes_with_corpus_size() {
        match (quality_corpus(400), quality_corpus(800)) {
            (Ok(a), Ok(b)) => assert!(a.digest() != b.digest()),
            _ => assert!(false),
        }
    }
}
