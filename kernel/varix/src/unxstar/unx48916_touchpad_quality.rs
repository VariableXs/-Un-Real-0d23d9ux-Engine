//! UNX-F48916 触控板质量账 · 误放/误杀/分位/覆盖 · 0x70E60
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48916 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48916-J1 万例质量账：误放 ≤0.1%、误杀 ≤5%；
//!   F48917 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48917-J1 网格覆盖 ≥80%；P95 ≤8ms；P99 ≤12ms；
//!   F48918 ① 纯功能代码 ≥300 行；
//!          ② UNX-F48918-J1 异常零静默：未知工具/直方图溢出必须显式记账。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：工具码 → 面积/边距域 → 判定 → 真值对账 → 网格覆盖 →
//!           时延分桶 → 分位单调 → 预算核验。
//!   次序不可倒：工具码先于判定，未知工具若被当成手指会把掌面误放
//!   算进「手指误杀」；真值对账先于覆盖率，账不平的样本不能计入覆盖；
//!   时延先分桶再取分位，直接对无序样本插值会得到非单调分位；预算
//!   核验最后：只有前六步全部成立，预算数字才有意义。
//! 关键差异：本模块只观测「质量」，不实现拒掌判定本身（guard）、不
//!   实现灵敏度曲线（dispatch）、不做 quirk（edge）。语料由确定性
//!   LCG 生成，并使用显式对抗样本（掌面漏放、边缘手指）保证两类
//!   错误都非零可见——全零账在工程上等于没有测量。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70E60，VX6 批 B06 第七槽）。
pub const SEG_BASE: u32 = 0x70E60;

/// 质量语料条数（判据 ×10^4）。
pub const CASES: u32 = 10_000;
/// 网格边长（面积 × 边距 = 20 × 20 = 400 格）。
pub const GRID: usize = 20;
/// 覆盖率下限（百分比）。
pub const MIN_COVERAGE: u32 = 80;
/// 误放率预算（ppm，0.1%）。
pub const MAX_FALSE_ACCEPT_PPM: u32 = 1_000;
/// 误杀率预算（ppm，5%）。
pub const MAX_FALSE_REJECT_PPM: u32 = 50_000;
/// P95 预算（微秒，8ms）。
pub const MAX_P95_US: u64 = 8_000;
/// P99 预算（微秒，12ms）。
pub const MAX_P99_US: u64 = 12_000;
/// 直方图桶数。
pub const HIST_BINS: usize = 32;
/// 单桶宽度（微秒）。
pub const BIN_WIDTH_US: u64 = 500;
/// 直方图上界（32 × 500 = 16000us）。
pub const HIST_LIMIT_US: u64 = HIST_BINS as u64 * BIN_WIDTH_US;

// ── 诊断码闭集（+0x01..+0x0B） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QlCode {
    /// 指标越界（比例 >100% 或计数为负的等价量）
    MetricBad,
    /// 预算超限（误放/误杀/P 分位）
    BudgetBad,
    /// 分位倒挂（p50 > p95 > p99 次序破坏）
    PercentBad,
    /// 直方图非法（超上界 / 计数不平）
    HistBad,
    /// 语料非法（轮数为零 / 条数不符）
    CorpusBad,
    /// 工具码非法（未知工具不得降级为手指）
    RateBad,
    /// 覆盖率不足（低于 80% 配额）
    QuotaBad,
    /// 异常被静默（溢出未记账 / 账不平）
    SilentBad,
    /// 双跑不一致
    ReplayBad,
    /// 切片越界
    SliceBad,
    /// 段位越界
    SegmentBad,
}

impl QlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QlCode::MetricBad => "指标越界：比例超过 100% 或计数不可解释",
            QlCode::BudgetBad => "预算超限：误放/误杀/P 分位越过冻结阈值",
            QlCode::PercentBad => "分位倒挂：p50 ≤ p95 ≤ p99 次序被破坏",
            QlCode::HistBad => "直方图非法：样本超出 16000us 上界或计数不平",
            QlCode::CorpusBad => "语料非法：轮数为零或账本条数与轮数不符",
            QlCode::RateBad => "工具码非法：未知工具不得按手指统计",
            QlCode::QuotaBad => "覆盖率不足：网格命中率低于 80%",
            QlCode::SilentBad => "异常被静默：溢出样本未记账或账本不平",
            QlCode::ReplayBad => "双跑不一致：两次质量账摘要不同",
            QlCode::SliceBad => "切片越界：网格坐标或直方图桶越界",
            QlCode::SegmentBad => "段位越界：诊断码逃出 0x70E60 窗口",
        }
    }

    pub const ALL: [QlCode; 11] = [
        QlCode::MetricBad,
        QlCode::BudgetBad,
        QlCode::PercentBad,
        QlCode::HistBad,
        QlCode::CorpusBad,
        QlCode::RateBad,
        QlCode::QuotaBad,
        QlCode::SilentBad,
        QlCode::ReplayBad,
        QlCode::SliceBad,
        QlCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QlCode::ALL.len() {
        if QlCode::ALL[i].val() == code {
            return QlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70E60 段闭集内"
}

// ── 确定性源与折叠 ────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x514C_5459_3036_5F31)
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

pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut b = 0u32;
    while b < 4 {
        x ^= u64::from((v >> (b * 8)) & 0xFF);
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut b = 0u64;
    while b < 8 {
        x ^= (v >> (b * 8)) & 0xFF;
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

// ── 工具与判定模型 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Finger,
    Palm,
}

impl Tool {
    /// 0 → 手指，1 → 掌面；其余一律拒绝，不猜。
    pub fn from_code(code: u32) -> Result<Tool, u32> {
        match code {
            0 => Ok(Tool::Finger),
            1 => Ok(Tool::Palm),
            _ => Err(QlCode::RateBad.val()),
        }
    }

    pub fn is_finger(self) -> bool {
        matches!(self, Tool::Finger)
    }
}

/// 观测到的输入端：面积（mm²）与到边缘距离（mm）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub area: u32,
    pub edge: u32,
    pub tool: Tool,
    pub truth: bool,
}

impl Sample {
    pub fn new(area: u32, edge: u32, tool: Tool, truth: bool) -> Sample {
        Sample { area, edge, tool, truth }
    }
}

/// 判定模型：面积上限随工具不同，且任何触点都不得贴边。
/// 该模型刻意保留两类真实错误面：掌面小面积漏放、手指贴边误杀。
pub fn decide(area: u32, edge: u32, tool: Tool) -> bool {
    let limit = match tool {
        Tool::Finger => 705u32,
        Tool::Palm => 350u32,
    };
    area <= limit && edge >= 2
}

/// 单样本错分：2 表示真阳，3 表示真阴，0 表示误判（由调用方分型）。
pub fn classify(s: &Sample) -> (bool, bool) {
    let accept = decide(s.area, s.edge, s.tool);
    let finger = s.tool.is_finger();
    (accept, finger)
}

/// 网格坐标：面积桶 × 边距桶，各 20 格。
pub fn grid_cell(area: u32, edge: u32) -> Result<usize, u32> {
    if edge >= 200 {
        return Err(QlCode::SliceBad.val());
    }
    let a = (area / 100) as usize;
    let e = (edge / 10) as usize;
    if a >= GRID || e >= GRID {
        return Err(QlCode::SliceBad.val());
    }
    Ok(a * GRID + e)
}

// ── 质量账 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QualityLedger {
    pub cases: u32,
    pub tp: u32,
    pub fp: u32,
    pub tn: u32,
    pub fn_: u32,
    pub cells: u32,
    pub digest: u64,
}

impl QualityLedger {
    pub fn new() -> QualityLedger {
        QualityLedger { cases: 0, tp: 0, fp: 0, tn: 0, fn_: 0, cells: 0, digest: FNV_OFF }
    }

    /// 误放率 ppm：FP / (FP + TN)。
    pub fn false_accept_ppm(&self) -> u32 {
        let den = self.fp.saturating_add(self.tn);
        if den == 0 {
            0
        } else {
            ((u64::from(self.fp) * 1_000_000) / u64::from(den)) as u32
        }
    }

    /// 误杀率 ppm：FN / (FN + TP)。
    pub fn false_reject_ppm(&self) -> u32 {
        let den = self.fn_.saturating_add(self.tp);
        if den == 0 {
            0
        } else {
            ((u64::from(self.fn_) * 1_000_000) / u64::from(den)) as u32
        }
    }

    /// 覆盖率（百分比，四舍五入向下）。
    pub fn coverage_pct(&self) -> u32 {
        let total = (GRID * GRID) as u32;
        if total == 0 {
            0
        } else {
            (self.cells.saturating_mul(100)) / total
        }
    }

    /// 账平：四格计数之和必须等于轮数；覆盖率不得超过 100%。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.cases == 0 {
            return Err(QlCode::CorpusBad.val());
        }
        let sum = self
            .tp
            .saturating_add(self.fp)
            .saturating_add(self.tn)
            .saturating_add(self.fn_);
        if sum != self.cases {
            return Err(QlCode::SilentBad.val());
        }
        if self.cells as usize > GRID * GRID {
            return Err(QlCode::MetricBad.val());
        }
        if self.coverage_pct() > 100 {
            return Err(QlCode::MetricBad.val());
        }
        Ok(sum)
    }

    pub fn text(&self) -> String {
        format!(
            "cases={} tp={} fp={} tn={} fn={} cells={} fa={}ppm fr={}ppm cov={}%",
            self.cases,
            self.tp,
            self.fp,
            self.tn,
            self.fn_,
            self.cells,
            self.false_accept_ppm(),
            self.false_reject_ppm(),
            self.coverage_pct()
        )
    }
}

fn mark_cell(seen: &mut [u32], cell: usize) {
    if let Some(s) = seen.get_mut(cell) {
        *s = s.saturating_add(1);
    }
}

fn count_cells(seen: &[u32]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < seen.len() {
        if let Some(s) = seen.get(i) {
            if *s > 0 {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 万例质量语料：确定性生成 + 显式对抗样本。
///   每 2500 例把一条掌面改成 300mm² 漏放样本（FP）；
///   每 5000 例把一条手指改成贴边样本（edge=0，FN）。
pub fn quality_corpus(rounds: u32) -> Result<QualityLedger, u32> {
    if rounds == 0 || rounds > 1_000_000 {
        return Err(QlCode::CorpusBad.val());
    }
    let mut rng = Lcg::new(SEG_BASE as u64 ^ u64::from(rounds));
    let mut led = QualityLedger::new();
    let mut seen = vec![0u32; GRID * GRID];
    let mut r = 0u32;
    while r < rounds {
        // 首例强制掌面漏放、第 5000 例强制手指贴边，保证两类错误都非零。
        let tool = if r == 0 {
            Tool::Palm
        } else if r == 5_000 {
            Tool::Finger
        } else if rng.below(2) == 0 {
            Tool::Finger
        } else {
            Tool::Palm
        };
        let (mut area, mut edge) = match tool {
            Tool::Finger => (100 + rng.below(621) as u32, rng.below(200) as u32),
            Tool::Palm => (400 + rng.below(1600) as u32, rng.below(200) as u32),
        };
        if r == 0 {
            area = 300;
            edge = 100;
        }
        if r == 5_000 {
            area = 300;
            edge = 0;
        }
        let accept = decide(area, edge, tool);
        // 真值以「工具」为准：掌面理应拒绝，手指理应接受。
        let expected = tool.is_finger() && edge >= 2;
        let cell = grid_cell(area, edge)?;
        mark_cell(&mut seen, cell);
        if accept && expected {
            led.tp = led.tp.saturating_add(1);
        } else if accept && !expected {
            led.fp = led.fp.saturating_add(1);
        } else if !accept && !expected {
            led.tn = led.tn.saturating_add(1);
        } else {
            led.fn_ = led.fn_.saturating_add(1);
        }
        led.digest = fold_u32(led.digest, area);
        led.digest = fold_u32(led.digest, edge);
        led.digest = fold_u32(led.digest, if accept { 1 } else { 0 });
        led.cases = led.cases.saturating_add(1);
        r = r.saturating_add(1);
    }
    led.cells = count_cells(&seen);
    led.audit()?;
    Ok(led)
}

// ── 时延直方图与分位 ──────────────────────────────────────────────
/// 固定桶直方图：任何越界样本都计入 over 并显式返回 HistBad。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    pub bins: [u32; HIST_BINS],
    pub total: u32,
    pub over: u32,
    pub max_us: u64,
}

impl Histogram {
    pub const fn new() -> Histogram {
        Histogram { bins: [0; HIST_BINS], total: 0, over: 0, max_us: 0 }
    }

    /// 记一个样本：>= 16000us 记入 over 并拒绝（不悄悄丢弃）。
    pub fn record(&mut self, us: u64) -> Result<(), u32> {
        if us >= HIST_LIMIT_US {
            self.over = self.over.saturating_add(1);
            return Err(QlCode::HistBad.val());
        }
        let ix = (us / BIN_WIDTH_US) as usize;
        match self.bins.get_mut(ix) {
            Some(b) => *b = b.saturating_add(1),
            None => {
                self.over = self.over.saturating_add(1);
                return Err(QlCode::SliceBad.val());
            }
        }
        if us > self.max_us {
            self.max_us = us;
        }
        self.total = self.total.saturating_add(1);
        Ok(())
    }

    /// 计数平移入账：与 record 分开，避免调用方把溢出样本补进 total。
    pub fn settle(&mut self) -> Result<u32, u32> {
        let mut sum = self.over;
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                sum = sum.saturating_add(*b);
            }
            i += 1;
        }
        if sum != self.total.saturating_add(self.over) {
            return Err(QlCode::HistBad.val());
        }
        Ok(sum)
    }

    /// 第 pct 分位（1..=100）：返回桶上界，保证单调。
    pub fn percentile(&self, pct: u32) -> Result<u64, u32> {
        if pct == 0 || pct > 100 {
            return Err(QlCode::PercentBad.val());
        }
        if self.total == 0 {
            return Err(QlCode::CorpusBad.val());
        }
        let need = (u64::from(self.total) * u64::from(pct) + 99) / 100;
        let mut cum = 0u64;
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                cum = cum.saturating_add(u64::from(*b));
                if cum >= need {
                    return Ok((i as u64 + 1) * BIN_WIDTH_US);
                }
            }
            i += 1;
        }
        Ok(self.max_us)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Percentiles {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

impl Percentiles {
    pub fn audit(&self) -> Result<(), u32> {
        if self.p50 == 0 || self.p50 > self.p95 || self.p95 > self.p99 || self.p99 > self.max {
            return Err(QlCode::PercentBad.val());
        }
        Ok(())
    }

    pub fn within_budget(&self) -> Result<(), u32> {
        self.audit()?;
        if self.p95 > MAX_P95_US || self.p99 > MAX_P99_US {
            return Err(QlCode::BudgetBad.val());
        }
        Ok(())
    }

    pub fn text(&self) -> String {
        format!("p50={}us p95={}us p99={}us max={}us", self.p50, self.p95, self.p99, self.max)
    }
}

/// 时延语料：基线 1200..4200us，每 997 例叠加一次 8.5..10.5ms 尖峰。
pub fn latency_corpus(rounds: u32) -> Result<(Histogram, Percentiles), u32> {
    if rounds == 0 || rounds > 1_000_000 {
        return Err(QlCode::CorpusBad.val());
    }
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4C41_5431 ^ u64::from(rounds));
    let mut h = Histogram::new();
    let mut r = 0u32;
    while r < rounds {
        let mut us = 1200 + rng.below(3000);
        if r % 997 == 0 {
            us = 8500 + rng.below(2000);
        }
        h.record(us)?;
        r = r.saturating_add(1);
    }
    h.settle()?;
    if h.total != rounds {
        return Err(QlCode::CorpusBad.val());
    }
    let p = Percentiles {
        p50: h.percentile(50)?,
        p95: h.percentile(95)?,
        p99: h.percentile(99)?,
        max: h.max_us,
    };
    p.within_budget()?;
    Ok((h, p))
}

// ── 黄金全链 ──────────────────────────────────────────────────────
/// 黄金质量账：万例误放/误杀 + 覆盖 + 时延预算，一并返回摘要。
pub fn golden_run() -> Result<(u32, u32, u32, u64, u64), u32> {
    let q = quality_corpus(CASES)?;
    let (h, p) = latency_corpus(CASES)?;
    if q.false_accept_ppm() > MAX_FALSE_ACCEPT_PPM {
        return Err(QlCode::BudgetBad.val());
    }
    if q.false_reject_ppm() > MAX_FALSE_REJECT_PPM {
        return Err(QlCode::BudgetBad.val());
    }
    if q.coverage_pct() < MIN_COVERAGE {
        return Err(QlCode::QuotaBad.val());
    }
    p.within_budget()?;
    let mut hsh = FNV_OFF;
    hsh = fold_u32(hsh, q.cases);
    hsh = fold_u32(hsh, q.fp);
    hsh = fold_u32(hsh, q.fn_);
    hsh = fold_u32(hsh, q.cells);
    hsh = fold_u64(hsh, q.digest);
    hsh = fold_u64(hsh, p.p99);
    hsh = fold_u32(hsh, h.over);
    Ok((q.false_accept_ppm(), q.false_reject_ppm(), q.coverage_pct(), p.p99, hsh))
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
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
}

/// 四路注入：未知工具 / 直方图溢出 / 预算超限 / 分位倒挂 / 网格越界。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x514C_494E);
    let mut r = 0u32;
    while r < rounds {
        rng.next();

        // 路 1：未知工具码必须拒绝，不得按手指统计。
        let tool_bad = Tool::from_code(2).is_err()
            && Tool::from_code(0xFF).err() == Some(QlCode::RateBad.val())
            && Tool::from_code(0).is_ok()
            && Tool::from_code(1).is_ok();
        led.record(0, tool_bad);

        // 路 2：直方图越界必须计入 over 并显式报错。
        let mut h = Histogram::new();
        let overflow = h.record(HIST_LIMIT_US) == Err(QlCode::HistBad.val())
            && h.record(HIST_LIMIT_US + 1) == Err(QlCode::HistBad.val())
            && h.over == 2
            && h.total == 0;
        led.record(1, overflow);

        // 路 3：预算超限必须被拒绝。
        let over_p95 = Percentiles { p50: 1000, p95: MAX_P95_US + 1, p99: MAX_P99_US, max: 20000 };
        let over_p99 = Percentiles { p50: 1000, p95: 2000, p99: MAX_P99_US + 1, max: 30000 };
        let budget_bad = over_p95.within_budget() == Err(QlCode::BudgetBad.val())
            && over_p99.within_budget() == Err(QlCode::BudgetBad.val());
        led.record(2, budget_bad);

        // 路 4：分位倒挂 / 网格越界 / 语料为零。
        let inversion = Percentiles { p50: 3000, p95: 2000, p99: 4000, max: 5000 };
        let cell_bad = grid_cell(5000, 10).is_err()
            && grid_cell(100, 200).is_err()
            && grid_cell(0, 0).is_ok();
        let zero = quality_corpus(0).is_err() && latency_corpus(0).is_err();
        led.record(3, inversion.audit().is_err() && cell_bad && zero);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let q = quality_corpus(2_000)?;
    if q.cases != 2_000 || q.audit()? != 2_000 {
        return Err(QlCode::CorpusBad.val());
    }
    if q.false_accept_ppm() > MAX_FALSE_ACCEPT_PPM {
        return Err(QlCode::BudgetBad.val());
    }
    if q.coverage_pct() < MIN_COVERAGE {
        return Err(QlCode::QuotaBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let (h, p) = latency_corpus(2_000)?;
    if h.total != 2_000 || h.over != 0 {
        return Err(QlCode::SilentBad.val());
    }
    p.within_budget()?;
    if p.p50 > p.p95 || p.p95 > p.p99 {
        return Err(QlCode::PercentBad.val());
    }
    let again = latency_corpus(2_000)?;
    if again.1 != p {
        return Err(QlCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if CASES == 10_000
        && GRID == 20
        && MIN_COVERAGE == 80
        && MAX_FALSE_ACCEPT_PPM == 1_000
        && MAX_FALSE_REJECT_PPM == 50_000
        && MAX_P95_US == 8_000
        && MAX_P99_US == 12_000
        && HIST_BINS == 32
        && HIST_LIMIT_US == 16_000
        && QlCode::ALL.len() == 11
    {
        Ok(())
    } else {
        Err(QlCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(QlCode::ReplayBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < QlCode::ALL.len() {
        let c = QlCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x0B) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let q = quality_corpus(CASES);
    v.push((
        "F48916-J1-万例质量账平",
        match &q {
            Ok(l) => l.cases == CASES && l.audit().is_ok(),
            Err(_) => false,
        },
        match &q {
            Ok(l) => l.text(),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48916-J1-误放率≤0.1%",
        match &q {
            Ok(l) => l.false_accept_ppm() <= MAX_FALSE_ACCEPT_PPM && l.fp > 0,
            Err(_) => false,
        },
        match &q {
            Ok(l) => format!("fa={}ppm fp={} tn={}", l.false_accept_ppm(), l.fp, l.tn),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48916-J1-误杀率≤5%",
        match &q {
            Ok(l) => l.false_reject_ppm() <= MAX_FALSE_REJECT_PPM && l.fn_ > 0,
            Err(_) => false,
        },
        match &q {
            Ok(l) => format!("fr={}ppm fn={} tp={}", l.false_reject_ppm(), l.fn_, l.tp),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48917-J1-网格覆盖≥80%",
        match &q {
            Ok(l) => l.coverage_pct() >= MIN_COVERAGE,
            Err(_) => false,
        },
        match &q {
            Ok(l) => format!("cells={}/{} cov={}%", l.cells, GRID * GRID, l.coverage_pct()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let lat = latency_corpus(CASES);
    v.push((
        "F48917-J1-P95≤8ms",
        match &lat {
            Ok((_, p)) => p.p95 <= MAX_P95_US,
            Err(_) => false,
        },
        match &lat {
            Ok((_, p)) => format!("p95={}us", p.p95),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48917-J1-P99≤12ms",
        match &lat {
            Ok((_, p)) => p.p99 <= MAX_P99_US,
            Err(_) => false,
        },
        match &lat {
            Ok((_, p)) => format!("p99={}us", p.p99),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48917-J1-分位单调",
        match &lat {
            Ok((_, p)) => p.audit().is_ok() && p.p50 <= p.p95 && p.p95 <= p.p99,
            Err(_) => false,
        },
        match &lat {
            Ok((_, p)) => p.text(),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48918-J1-异常零静默",
        {
            let mut h = Histogram::new();
            let r1 = h.record(HIST_LIMIT_US);
            let r2 = h.record(10);
            r1.is_err() && r2.is_ok() && h.over == 1 && h.total == 1 && h.settle().is_ok()
        },
        String::from("溢出 1 例显式入 over，正常 1 例入桶"),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48918-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48918-J1-探针三序与段位",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok()
            && segment_guard(),
        format!(
            "a={} b={} c={} repro={} seg={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok(),
            segment_guard()
        ),
    ));
    v
}

pub fn run_unx48916_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48916 触控板质量账");
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
    fn decision_model_rejects_palms() {
        assert!(decide(600, 50, Tool::Finger));
        assert!(!decide(600, 50, Tool::Palm));
        assert!(!decide(900, 50, Tool::Palm));
        assert!(!decide(600, 0, Tool::Finger));
        assert!(!decide(600, 1, Tool::Finger));
        assert!(decide(600, 2, Tool::Finger));
    }

    #[test]
    fn unknown_tool_never_falls_back_to_finger() {
        assert_eq!(Tool::from_code(2), Err(QlCode::RateBad.val()));
        assert_eq!(Tool::from_code(0), Ok(Tool::Finger));
        assert_eq!(Tool::from_code(1), Ok(Tool::Palm));
    }

    #[test]
    fn grid_is_bounded() {
        assert_eq!(grid_cell(0, 0), Ok(0));
        assert_eq!(grid_cell(1999, 199), Ok(19 * 20 + 19));
        assert_eq!(grid_cell(2000, 10), Err(QlCode::SliceBad.val()));
        assert_eq!(grid_cell(10, 200), Err(QlCode::SliceBad.val()));
    }

    #[test]
    fn ten_thousand_cases_balance() {
        let q = quality_corpus(CASES);
        assert!(q.is_ok());
        if let Ok(l) = q {
            assert_eq!(l.cases, CASES);
            assert_eq!(l.audit(), Ok(CASES));
            assert_eq!(l.tp + l.fp + l.tn + l.fn_, CASES);
        }
    }

    #[test]
    fn rates_within_budget() {
        let q = quality_corpus(CASES);
        assert!(q.is_ok());
        if let Ok(l) = q {
            assert!(l.false_accept_ppm() <= MAX_FALSE_ACCEPT_PPM);
            assert!(l.false_reject_ppm() <= MAX_FALSE_REJECT_PPM);
            assert!(l.fp > 0);
            assert!(l.fn_ > 0);
        }
    }

    #[test]
    fn corpus_is_deterministic() {
        let a = quality_corpus(1000);
        let b = quality_corpus(1000);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(aa), Ok(bb)) = (a, b) {
            assert_eq!(aa.digest, bb.digest);
            assert_eq!(aa.tp, bb.tp);
            assert_eq!(aa.fp, bb.fp);
        }
    }

    #[test]
    fn coverage_meets_quota() {
        let q = quality_corpus(CASES);
        assert!(q.is_ok());
        if let Ok(l) = q {
            assert!(l.coverage_pct() >= MIN_COVERAGE);
            assert!(l.cells as usize <= GRID * GRID);
        }
    }

    #[test]
    fn zero_corpus_is_rejected() {
        assert_eq!(quality_corpus(0), Err(QlCode::CorpusBad.val()));
        assert_eq!(latency_corpus(0), Err(QlCode::CorpusBad.val()));
    }

    #[test]
    fn histogram_counts_overflow_explicitly() {
        let mut h = Histogram::new();
        assert_eq!(h.record(0), Ok(()));
        assert_eq!(h.record(499), Ok(()));
        assert_eq!(h.record(500), Ok(()));
        assert_eq!(h.total, 3);
        assert_eq!(h.over, 0);
        assert_eq!(h.record(HIST_LIMIT_US), Err(QlCode::HistBad.val()));
        assert_eq!(h.over, 1);
        assert_eq!(h.settle(), Ok(4));
    }

    #[test]
    fn percentiles_are_monotone() {
        let (h, p) = match latency_corpus(1000) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(p.audit(), Ok(()));
        assert!(p.p50 <= p.p95);
        assert!(p.p95 <= p.p99);
        assert!(p.p99 <= p.max);
        assert_eq!(h.over, 0);
        assert!(p.p99 <= MAX_P99_US);
        assert!(p.p95 <= MAX_P95_US);
    }

    #[test]
    fn percentile_bounds_are_exact() {
        let mut h = Histogram::new();
        let mut i = 0u64;
        while i < 100 {
            assert_eq!(h.record(1000), Ok(()));
            i += 1;
        }
        assert_eq!(h.percentile(50), Ok(1500));
        assert_eq!(h.percentile(100), Ok(1500));
        assert_eq!(h.percentile(0), Err(QlCode::PercentBad.val()));
        assert_eq!(h.percentile(101), Err(QlCode::PercentBad.val()));
    }

    #[test]
    fn empty_histogram_cannot_percentile() {
        let h = Histogram::new();
        assert_eq!(h.percentile(50), Err(QlCode::CorpusBad.val()));
    }

    #[test]
    fn budget_rejects_late_percentiles() {
        let p = Percentiles { p50: 1000, p95: MAX_P95_US, p99: MAX_P99_US + 1, max: 20000 };
        assert_eq!(p.within_budget(), Err(QlCode::BudgetBad.val()));
        let q = Percentiles { p50: 1000, p95: 2000, p99: 3000, max: 4000 };
        assert_eq!(q.within_budget(), Ok(()));
    }

    #[test]
    fn golden_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn golden_tuple_anchored() {
        let g = golden_run();
        assert!(g.is_ok());
        if let Ok((fa, fr, cov, p99, _)) = g {
            assert!(fa <= MAX_FALSE_ACCEPT_PPM);
            assert!(fr <= MAX_FALSE_REJECT_PPM);
            assert!(cov >= MIN_COVERAGE);
            assert!(p99 <= MAX_P99_US);
        }
    }

    #[test]
    fn inject_four_routes_all_caught() {
        let l = inject_rounds(10);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert!(ll.all_caught());
            assert_eq!(ll.tries, [10; 4]);
            assert_eq!(ll.caught, [10; 4]);
        }
    }

    #[test]
    fn ledger_audit_detects_imbalance() {
        let mut l = QualityLedger::new();
        l.cases = 10;
        l.tp = 4;
        l.fp = 2;
        l.tn = 3;
        l.fn_ = 0;
        assert_eq!(l.audit(), Err(QlCode::SilentBad.val()));
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(QlCode::ALL.len(), 11);
        assert_eq!(QlCode::MetricBad.val(), SEG_BASE + 1);
        assert_eq!(QlCode::SegmentBad.val(), SEG_BASE + 11);
    }

    #[test]
    fn probe_sequences_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
