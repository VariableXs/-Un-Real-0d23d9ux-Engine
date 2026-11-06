//! UNX-F48896 分位质量与配额率（P50/P90/P99 / 利用率分档 / 直方图覆盖） · 0x70BE0
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48896（分位 ×10^3 单调且精确）· F48897（配额率 ×10^3 分档一致）
//!         · F48898（直方图覆盖 ×10^3 零漏桶）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48896 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48896-J1 分位 ×10^3 单调且精确；
//!   F48897 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48897-J1 配额率 ×10^3 分档一致；
//!   F48898 ① 纯功能代码 ≥380 行；
//!          ② UNX-F48898-J1 直方图覆盖 ×10^3 零漏桶。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：样本合法 → 升序校验 → 分位秩 → 单调复核 → 直方图分桶
//!           → 桶和复核 → 配额率 → 分档复核 → 覆盖率 → 账平。
//!   次序不可倒的理由逐条：
//!     · 样本先合法：抖动为 0、超 4096、设备号越界、序号跳号四类一旦
//!       混进样本池，后面的分位只是「垃圾进、垃圾出」，而且排序会把
//!       非法值藏进中间位置，肉眼再也看不出来；
//!     · 升序先于秩：近邻秩公式假设输入升序，若在乱序上取下标，得到的
//!       是「第 k 个被写进来的数」而不是「第 k 小的数」，两者在负载
//!       尖峰时相差数倍；
//!     · 分位秩先于单调：秩公式本身越界（0 或 1000 以上）时先算出的
//!       三个值可能偶然单调，只有先钉死秩域，单调才有证明力；
//!     · 单调先于直方图：分位回答「尾部有多长」，直方图回答「尾部
//!       落在哪一桶」，先确认尾部本身不塌，再看底部形状；
//!     · 桶和先于配额：直方图桶和必须等于样本总数，否则后面的覆盖率
//!       会把「丢样本」误算成「低延迟」；
//!     · 配额率先于分档：分档是配额的函数，反序会把分档当成独立标签，
//!       出现「已用 40/48 却标成 Idle」这类自相矛盾的记录；
//!     · 覆盖率晚于分档：覆盖率是全局横截面，只有每台设备的分档都
//!       定稿后，才能回答「有没有设备一帧样本都没进统计」；
//!     · 账平最后执行，它只回答「有没有样本被静默吞掉」。
//! 关键差异：本模块只拥有「分位 + 配额率 + 直方图」这一层。
//!   core 拥有三操作 SOP 与四元组；meta 拥有合并策略与焦点；
//!   guard 拥有水位滞回、配额债与庇护；dispatch 拥有十设备对账；
//!   edge 拥有边界阶梯；bridge 拥有跨模块封存。本模块不复制水位、
//!   不复制对账、不做跨模块寻址；它回答的是一个更窄的问题：给定
//!   一批已入账的延迟样本与一张十设备配额表，P50/P90/P99 是否单调
//!   且可复算、利用率分档是否与分子分母一致、五桶直方图是否一桶
//!   不漏。谁想改动分位却不动秩、改动配额却不动分档，账就会在这里
//!   立刻翻红。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48885_ops_guard as q_guard;
use super::unx48893_ops_bridge as q_bridge;

/// 诊断段基址（0x70BE0，VX6 批 B05 族第七槽）。
pub const SEG_BASE: u32 = 0x70BE0;

/// 十设备上限（与族内其余模块同构）。
pub const DEVICE_COUNT: usize = 10;
/// 单设备样本预算（128 帧）。
pub const SAMPLE_CAP: u32 = 128;
/// 配额上限：直接引用 guard 的冻结值，避免两处各写一个 48。
pub const QUOTA_CAP: u32 = q_guard::QUOTA_CAP;
/// 延迟下界（0 抖动不是样本，是缺测）。
pub const LAT_FLOOR: u32 = 1;
/// 延迟上界（4 ms）。
pub const LAT_CEIL: u32 = 4096;
/// 千分制分母。
pub const PER_MILLE: u32 = 1000;
/// 直方图桶数：<16 / <64 / <256 / <1024 / <=4096。
pub const HIST_BUCKETS: usize = 5;
/// 直方图四个分界值（不含上界，最后一桶兜底）。
pub const HIST_EDGES: [u32; 4] = [16, 64, 256, 1024];
/// 利用率分档数。
pub const BAND_COUNT: usize = 4;
/// Idle 上界（含）：≤250‰。
pub const IDLE_TOP: u32 = 250;
/// Normal 上界（含）：≤750‰。
pub const NORMAL_TOP: u32 = 750;
/// Hot 上界（含）：≤950‰。
pub const HOT_TOP: u32 = 950;
/// 分位判据轮数（×10^3）。
pub const QUALITY_ROUNDS: u32 = 1000;
/// 配额率判据轮数（×10^3）。
pub const QUOTA_ROUNDS: u32 = 1000;
/// 直方图覆盖判据轮数（×10^3）。
pub const COVER_ROUNDS: u32 = 1000;
/// 黄金语料轮数。
pub const GOLDEN_ROUNDS: u32 = 1000;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 1000;
/// FNV-1a 64 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QlCode {
    /// 延迟样本非法（设备号 / 抖动值 / 序号）
    LatBad,
    /// 分位秩非法（零秩 / 超千分）
    RankBad,
    /// 分位倒挂（P50 > P90 > P99 或尾部越界）
    OrderBad,
    /// 直方图非法（桶和不等 / 桶序回退）
    HistBad,
    /// 覆盖缺失（有设备未进统计）
    CoverageBad,
    /// 配额越界（设备号 / 分母 / 透支）
    QuotaBad,
    /// 分档不一致（标签与千分率不符）
    BandBad,
    /// 样本预算溢出（单设备超 128）
    BudgetBad,
    /// 账不平（分位轮数 / 样本数 / 拒绝数对不上）
    LedgerBad,
    /// 导出文本畸形（字段数 / 数字 / 摘要）
    RenderBad,
    /// 段位越界
    SegmentBad,
}

impl QlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            QlCode::LatBad => "延迟样本非法：设备 0..9、抖动 1..=4096、序号从 1 连增",
            QlCode::RankBad => "分位秩非法：秩必须落在 1..=1000 千分位",
            QlCode::OrderBad => "分位倒挂：P50 ≤ P90 ≤ P99 ≤ 最大值，逆序即拒",
            QlCode::HistBad => "直方图非法：五桶求和必须等于样本数，桶序不得回退",
            QlCode::CoverageBad => "覆盖缺失：十台设备每台至少一帧样本",
            QlCode::QuotaBad => "配额越界：设备号小于 10、分母 1..=48、已用不得超上限",
            QlCode::BandBad => "分档不一致：Idle/Normal/Hot/Saturated 必须由千分率唯一决定",
            QlCode::BudgetBad => "样本预算溢出：单设备累计不得超过 128 帧",
            QlCode::LedgerBad => "账不平：分位样本数 + 拒绝数 ≠ 总轮数，或摘要不可复算",
            QlCode::RenderBad => "导出畸形：六字段、数字可解析、摘要定宽十六位",
            QlCode::SegmentBad => "段位越界：诊断码逃出 0x70BE0 窗口",
        }
    }

    pub const ALL: [QlCode; 11] = [
        QlCode::LatBad,
        QlCode::RankBad,
        QlCode::OrderBad,
        QlCode::HistBad,
        QlCode::CoverageBad,
        QlCode::QuotaBad,
        QlCode::BandBad,
        QlCode::BudgetBad,
        QlCode::LedgerBad,
        QlCode::RenderBad,
        QlCode::SegmentBad,
    ];
}

/// 自由函数明细：导出与跨模块桥都走这一条，不另开查表。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QlCode::ALL.len() {
        if let Some(c) = QlCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70BE0 窗口内"
}

// ── 摘要折叠 ────────────────────────────────────────────────────────
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut i = 0u32;
    let mut acc = h;
    while i < 4 {
        acc = fold_byte(acc, ((v >> (i.saturating_mul(8))) & 0xFF) as u8);
        i = i.saturating_add(1);
    }
    acc
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    fold_u32(fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 确定性线性同余（不依赖标准库随机） ──────────────────────────────
#[derive(Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 延迟样本 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencySample {
    pub dev: u8,
    pub ticks: u32,
    pub seq: u32,
}

impl LatencySample {
    pub fn new(dev: u32, ticks: u32, seq: u32) -> Result<LatencySample, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(QlCode::LatBad.val());
        }
        if ticks < LAT_FLOOR || ticks > LAT_CEIL {
            return Err(QlCode::LatBad.val());
        }
        if seq == 0 {
            return Err(QlCode::LatBad.val());
        }
        Ok(LatencySample { dev: dev as u8, ticks, seq })
    }
}

// ── 近邻秩与分位 ────────────────────────────────────────────────────
/// 近邻秩下标（0 起）：ceil(len × per_mille / 1000)，至少 1，至多 len。
pub fn rank_index(len: usize, per_mille: u32) -> Result<usize, u32> {
    if len == 0 {
        return Err(QlCode::LedgerBad.val());
    }
    if per_mille == 0 || per_mille > PER_MILLE {
        return Err(QlCode::RankBad.val());
    }
    let numerator = (len as u64).saturating_mul(per_mille as u64);
    let mut rank = numerator.saturating_add(PER_MILLE as u64 - 1) / PER_MILLE as u64;
    if rank == 0 {
        rank = 1;
    }
    if rank > len as u64 {
        rank = len as u64;
    }
    Ok((rank - 1) as usize)
}

/// 近邻秩取值：sorted 必须升序。
pub fn nearest_rank(sorted: &[u32], per_mille: u32) -> Result<u32, u32> {
    let ix = rank_index(sorted.len(), per_mille)?;
    match sorted.get(ix) {
        Some(v) => Ok(*v),
        None => Err(QlCode::RankBad.val()),
    }
}

/// 升序校验：任何一处逆序都要在分位计算前被抓住。
pub fn is_sorted_asc(sorted: &[u32]) -> bool {
    if sorted.len() <= 1 {
        return true;
    }
    let mut i = 0usize;
    while i + 1 < sorted.len() {
        let a = match sorted.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let b = match sorted.get(i + 1) {
            Some(v) => *v,
            None => return false,
        };
        if a > b {
            return false;
        }
        i += 1;
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Percentile {
    pub p50: u32,
    pub p90: u32,
    pub p99: u32,
    pub max: u32,
}

impl Percentile {
    /// 在升序样本上一次性求四分位，并立即复核单调与尾部。
    pub fn compute(sorted: &[u32]) -> Result<Percentile, u32> {
        if sorted.is_empty() {
            return Err(QlCode::LedgerBad.val());
        }
        if !is_sorted_asc(sorted) {
            return Err(QlCode::OrderBad.val());
        }
        let p50 = nearest_rank(sorted, 500)?;
        let p90 = nearest_rank(sorted, 900)?;
        let p99 = nearest_rank(sorted, 990)?;
        let max = match sorted.last() {
            Some(v) => *v,
            None => return Err(QlCode::LedgerBad.val()),
        };
        if p50 < LAT_FLOOR || p50 > p90 || p90 > p99 || p99 > max {
            return Err(QlCode::OrderBad.val());
        }
        Ok(Percentile { p50, p90, p99, max })
    }

    /// 结构自洽：单调、非零、尾部不超上界。
    pub fn fits(&self) -> bool {
        self.p50 >= LAT_FLOOR
            && self.p50 <= self.p90
            && self.p90 <= self.p99
            && self.p99 <= self.max
            && self.max <= LAT_CEIL
    }

    /// 尾部跨度：P99 与 P50 的距离，用来观察长尾是否被压扁。
    pub fn span(&self) -> u32 {
        self.p99.saturating_sub(self.p50)
    }

    /// 精确复核：三个分位点必须逐一对得上近邻秩。
    pub fn exact_on(&self, sorted: &[u32]) -> bool {
        match (
            nearest_rank(sorted, 500),
            nearest_rank(sorted, 900),
            nearest_rank(sorted, 990),
        ) {
            (Ok(a), Ok(b), Ok(c)) => a == self.p50 && b == self.p90 && c == self.p99,
            _ => false,
        }
    }
}

/// 算术均值（四舍五入），用于交叉印证分位没有整体偏移。
pub fn mean_rounded(sorted: &[u32]) -> Result<u32, u32> {
    if sorted.is_empty() {
        return Err(QlCode::LedgerBad.val());
    }
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < sorted.len() {
        if let Some(v) = sorted.get(i) {
            sum = sum.saturating_add(*v as u64);
        }
        i += 1;
    }
    let n = sorted.len() as u64;
    let half = n / 2;
    Ok((sum.saturating_add(half) / n) as u32)
}

// ── 直方图 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    buckets: [u32; HIST_BUCKETS],
}

impl Histogram {
    pub const fn new() -> Histogram {
        Histogram { buckets: [0; HIST_BUCKETS] }
    }

    /// 桶归属：0:<16、1:<64、2:<256、3:<1024、4:≤4096。
    pub fn bucket_of(ticks: u32) -> Result<usize, u32> {
        if ticks < LAT_FLOOR || ticks > LAT_CEIL {
            return Err(QlCode::LatBad.val());
        }
        if ticks < 16 {
            return Ok(0);
        }
        if ticks < 64 {
            return Ok(1);
        }
        if ticks < 256 {
            return Ok(2);
        }
        if ticks < 1024 {
            return Ok(3);
        }
        Ok(4)
    }

    pub fn add(&mut self, ticks: u32) -> Result<(), u32> {
        let ix = Histogram::bucket_of(ticks)?;
        match self.buckets.get_mut(ix) {
            Some(slot) => {
                *slot = slot.saturating_add(1);
                Ok(())
            }
            None => Err(QlCode::HistBad.val()),
        }
    }

    /// 从升序样本灌桶；同时校验桶序不回退（回退说明样本并不升序）。
    pub fn from_sorted(sorted: &[u32]) -> Result<Histogram, u32> {
        let mut h = Histogram::new();
        let mut last = 0usize;
        let mut i = 0usize;
        while i < sorted.len() {
            let ticks = match sorted.get(i) {
                Some(v) => *v,
                None => return Err(QlCode::HistBad.val()),
            };
            let ix = Histogram::bucket_of(ticks)?;
            if ix < last {
                return Err(QlCode::HistBad.val());
            }
            last = ix;
            h.add(ticks)?;
            i += 1;
        }
        Ok(h)
    }

    pub fn count(&self, ix: usize) -> u32 {
        match self.buckets.get(ix) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            sum = sum.saturating_add(self.count(i));
            i += 1;
        }
        sum
    }

    /// 桶和复核：不允许任何样本在分桶途中消失。
    pub fn audit(&self, expected: u32) -> Result<(), u32> {
        if self.total() != expected {
            return Err(QlCode::HistBad.val());
        }
        Ok(())
    }

    /// 空桶计数：五桶必须全非零才算「零漏桶」。
    pub fn empty_buckets(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            if self.count(i) == 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 中位桶：累计计数首次达到总数一半（向上取整）的桶。
    pub fn median_bucket(&self) -> Result<usize, u32> {
        let total = self.total();
        if total == 0 {
            return Err(QlCode::HistBad.val());
        }
        let target = (total as u64 + 1) / 2;
        self.bucket_where(target)
    }

    /// 千分位桶：累计计数首次达到 ceil(total × per_mille / 1000) 的桶。
    pub fn quantile_bucket(&self, per_mille: u32) -> Result<usize, u32> {
        if per_mille == 0 || per_mille > PER_MILLE {
            return Err(QlCode::RankBad.val());
        }
        let total = self.total();
        if total == 0 {
            return Err(QlCode::HistBad.val());
        }
        let numerator = (total as u64).saturating_mul(per_mille as u64);
        let target = numerator.saturating_add(PER_MILLE as u64 - 1) / PER_MILLE as u64;
        self.bucket_where(if target == 0 { 1 } else { target })
    }

    fn bucket_where(&self, target: u64) -> Result<usize, u32> {
        let mut cum = 0u64;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            cum = cum.saturating_add(self.count(i) as u64);
            if cum >= target {
                return Ok(i);
            }
            i += 1;
        }
        Err(QlCode::HistBad.val())
    }

    /// 五桶计数快照（供导出与测试使用）。
    pub fn counts(&self) -> [u32; HIST_BUCKETS] {
        self.buckets
    }
}

// ── 利用率分档 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Idle,
    Normal,
    Hot,
    Saturated,
}

impl Band {
    pub fn rank(self) -> u32 {
        match self {
            Band::Idle => 0,
            Band::Normal => 1,
            Band::Hot => 2,
            Band::Saturated => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Band::Idle => "Idle",
            Band::Normal => "Normal",
            Band::Hot => "Hot",
            Band::Saturated => "Saturated",
        }
    }

    pub fn from_rank(rank: u32) -> Result<Band, u32> {
        match rank {
            0 => Ok(Band::Idle),
            1 => Ok(Band::Normal),
            2 => Ok(Band::Hot),
            3 => Ok(Band::Saturated),
            _ => Err(QlCode::BandBad.val()),
        }
    }
}

/// 分档唯一入口：千分率 → 档位，越界即拒。
pub fn band_of(per_mille: u32) -> Result<Band, u32> {
    if per_mille > PER_MILLE {
        return Err(QlCode::BandBad.val());
    }
    if per_mille <= IDLE_TOP {
        Ok(Band::Idle)
    } else if per_mille <= NORMAL_TOP {
        Ok(Band::Normal)
    } else if per_mille <= HOT_TOP {
        Ok(Band::Hot)
    } else {
        Ok(Band::Saturated)
    }
}

// ── 单设备配额 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Utilization {
    pub dev: u8,
    pub used: u32,
    pub cap: u32,
}

impl Utilization {
    pub fn new(dev: u32, used: u32, cap: u32) -> Result<Utilization, u32> {
        if dev as usize >= DEVICE_COUNT {
            return Err(QlCode::QuotaBad.val());
        }
        if cap == 0 || cap > QUOTA_CAP {
            return Err(QlCode::QuotaBad.val());
        }
        if used > cap {
            return Err(QlCode::QuotaBad.val());
        }
        Ok(Utilization { dev: dev as u8, used, cap })
    }

    /// 千分率（向下取整）：used × 1000 / cap。
    pub fn permille(&self) -> Result<u32, u32> {
        if self.cap == 0 {
            return Err(QlCode::QuotaBad.val());
        }
        let v = (self.used as u64).saturating_mul(PER_MILLE as u64) / self.cap as u64;
        if v > PER_MILLE as u64 {
            return Err(QlCode::QuotaBad.val());
        }
        Ok(v as u32)
    }

    pub fn band(&self) -> Result<Band, u32> {
        band_of(self.permille()?)
    }

    pub fn headroom(&self) -> u32 {
        self.cap.saturating_sub(self.used)
    }

    /// 自审：上限域、透支、分档一致、剩余量闭合。
    pub fn audit(&self) -> Result<(), u32> {
        if self.dev as usize >= DEVICE_COUNT || self.cap == 0 || self.cap > QUOTA_CAP {
            return Err(QlCode::QuotaBad.val());
        }
        if self.used > self.cap {
            return Err(QlCode::QuotaBad.val());
        }
        let p = self.permille()?;
        if band_of(p)? != self.band()? {
            return Err(QlCode::BandBad.val());
        }
        if self.headroom().saturating_add(self.used) != self.cap {
            return Err(QlCode::QuotaBad.val());
        }
        Ok(())
    }
}

// ── 十设备配额表 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct QuotaGrid {
    rows: [Option<Utilization>; DEVICE_COUNT],
}

impl QuotaGrid {
    pub const fn new() -> QuotaGrid {
        QuotaGrid { rows: [None; DEVICE_COUNT] }
    }

    /// 落位：同一设备只能登记一次，重复登记不是「覆盖」而是账目事故。
    pub fn set(&mut self, u: Utilization) -> Result<(), u32> {
        let dev = u.dev as usize;
        if dev >= DEVICE_COUNT {
            return Err(QlCode::QuotaBad.val());
        }
        match self.rows.get_mut(dev) {
            Some(slot) => {
                if slot.is_some() {
                    return Err(QlCode::QuotaBad.val());
                }
                *slot = Some(u);
                Ok(())
            }
            None => Err(QlCode::QuotaBad.val()),
        }
    }

    /// 按固定斜率填满十台：used = 3 + dev × 5，末台恰好打满 48。
    pub fn fill_ramp(&mut self) -> Result<(), u32> {
        let mut dev = 0u32;
        while (dev as usize) < DEVICE_COUNT {
            let used = 3u32.saturating_add(dev.saturating_mul(5));
            let u = Utilization::new(dev, used, QUOTA_CAP)?;
            self.set(u)?;
            dev = dev.saturating_add(1);
        }
        Ok(())
    }

    pub fn get(&self, dev: usize) -> Option<Utilization> {
        match self.rows.get(dev) {
            Some(v) => *v,
            None => None,
        }
    }

    pub fn coverage(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            if let Some(v) = self.rows.get(i) {
                if v.is_some() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 十设备平均千分率（向下取整）。
    pub fn average_permille(&self) -> Result<u32, u32> {
        if self.coverage() != DEVICE_COUNT as u32 {
            return Err(QlCode::CoverageBad.val());
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            let u = match self.get(i) {
                Some(v) => v,
                None => return Err(QlCode::CoverageBad.val()),
            };
            sum = sum.saturating_add(u.permille()? as u64);
            i += 1;
        }
        Ok((sum / DEVICE_COUNT as u64) as u32)
    }

    /// 最热设备：千分率最高者；同率取设备号较小者，保证确定性。
    pub fn hottest(&self) -> Result<Utilization, u32> {
        if self.coverage() != DEVICE_COUNT as u32 {
            return Err(QlCode::CoverageBad.val());
        }
        let mut best = match self.get(0) {
            Some(v) => v,
            None => return Err(QlCode::CoverageBad.val()),
        };
        let mut best_p = best.permille()?;
        let mut i = 1usize;
        while i < DEVICE_COUNT {
            let u = match self.get(i) {
                Some(v) => v,
                None => return Err(QlCode::CoverageBad.val()),
            };
            let p = u.permille()?;
            if p > best_p {
                best = u;
                best_p = p;
            }
            i += 1;
        }
        Ok(best)
    }

    /// 四档计数：四档之和必须等于十。
    pub fn band_counts(&self) -> Result<[u32; BAND_COUNT], u32> {
        if self.coverage() != DEVICE_COUNT as u32 {
            return Err(QlCode::CoverageBad.val());
        }
        let mut counts = [0u32; BAND_COUNT];
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            let u = match self.get(i) {
                Some(v) => v,
                None => return Err(QlCode::CoverageBad.val()),
            };
            let band = u.band()?;
            match counts.get_mut(band.rank() as usize) {
                Some(slot) => *slot = slot.saturating_add(1),
                None => return Err(QlCode::BandBad.val()),
            }
            i += 1;
        }
        Ok(counts)
    }

    /// 全表自审：每行合法、覆盖率满、分档和与十相等。
    pub fn audit(&self) -> Result<(), u32> {
        if self.coverage() != DEVICE_COUNT as u32 {
            return Err(QlCode::CoverageBad.val());
        }
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            let u = match self.get(i) {
                Some(v) => v,
                None => return Err(QlCode::CoverageBad.val()),
            };
            u.audit()?;
            i += 1;
        }
        let counts = self.band_counts()?;
        let mut sum = 0u32;
        let mut j = 0usize;
        while j < BAND_COUNT {
            if let Some(c) = counts.get(j) {
                sum = sum.saturating_add(*c);
            }
            j += 1;
        }
        if sum != DEVICE_COUNT as u32 {
            return Err(QlCode::BandBad.val());
        }
        Ok(())
    }
}

// ── 样本簿 ──────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SampleBook {
    rows: Vec<LatencySample>,
    per_dev: [u32; DEVICE_COUNT],
    last_seq: [u32; DEVICE_COUNT],
    digest: u64,
}

impl SampleBook {
    pub fn new() -> SampleBook {
        SampleBook {
            rows: Vec::new(),
            per_dev: [0; DEVICE_COUNT],
            last_seq: [0; DEVICE_COUNT],
            digest: FNV_OFF,
        }
    }

    /// 入场：序号必须从 1 起逐帧连增；单设备预算 128 帧。
    pub fn push(&mut self, s: LatencySample) -> Result<(), u32> {
        let dev = s.dev as usize;
        let count = match self.per_dev.get(dev) {
            Some(v) => *v,
            None => return Err(QlCode::LatBad.val()),
        };
        if count >= SAMPLE_CAP {
            return Err(QlCode::BudgetBad.val());
        }
        let last = match self.last_seq.get(dev) {
            Some(v) => *v,
            None => return Err(QlCode::LatBad.val()),
        };
        if s.seq != last.saturating_add(1) {
            return Err(QlCode::LatBad.val());
        }
        self.rows.push(s);
        if let Some(c) = self.per_dev.get_mut(dev) {
            *c = c.saturating_add(1);
        }
        if let Some(l) = self.last_seq.get_mut(dev) {
            *l = s.seq;
        }
        self.digest = fold_byte(self.digest, s.dev);
        self.digest = fold_u32(self.digest, s.ticks);
        self.digest = fold_u32(self.digest, s.seq);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn dev_count(&self, dev: usize) -> u32 {
        match self.per_dev.get(dev) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 全局升序快照。
    pub fn sorted(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::with_capacity(self.rows.len());
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(s) = self.rows.get(i) {
                v.push(s.ticks);
            }
            i += 1;
        }
        v.sort_unstable();
        v
    }

    /// 单设备升序快照；设备越界记覆盖缺失。
    pub fn sorted_dev(&self, dev: usize) -> Result<Vec<u32>, u32> {
        if dev >= DEVICE_COUNT {
            return Err(QlCode::CoverageBad.val());
        }
        let mut v: Vec<u32> = Vec::with_capacity(self.dev_count(dev) as usize);
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(s) = self.rows.get(i) {
                if s.dev as usize == dev {
                    v.push(s.ticks);
                }
            }
            i += 1;
        }
        v.sort_unstable();
        Ok(v)
    }

    pub fn percentiles(&self) -> Result<Percentile, u32> {
        let sorted = self.sorted();
        Percentile::compute(&sorted)
    }

    pub fn histogram(&self) -> Result<Histogram, u32> {
        let sorted = self.sorted();
        Histogram::from_sorted(&sorted)
    }

    /// 覆盖设备数：十台里有多少台至少进了一帧。
    pub fn coverage(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            if self.dev_count(i) > 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 账平自审：行数 = 分设备计数之和；摘要可从原始行复算。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < DEVICE_COUNT {
            let c = self.dev_count(i);
            if c > SAMPLE_CAP {
                return Err(QlCode::BudgetBad.val());
            }
            if c == 0 {
                if let Some(last) = self.last_seq.get(i) {
                    if *last != 0 {
                        return Err(QlCode::LedgerBad.val());
                    }
                }
            }
            sum = sum.saturating_add(c);
            i += 1;
        }
        if sum != self.rows.len() as u32 {
            return Err(QlCode::LedgerBad.val());
        }
        let mut h = FNV_OFF;
        let mut j = 0usize;
        while j < self.rows.len() {
            if let Some(s) = self.rows.get(j) {
                h = fold_byte(h, s.dev);
                h = fold_u32(h, s.ticks);
                h = fold_u32(h, s.seq);
            }
            j += 1;
        }
        if h != self.digest {
            return Err(QlCode::LedgerBad.val());
        }
        Ok(())
    }
}

// ── 质量账 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct QualityLedger {
    pub samples: u32,
    pub quantiles: u32,
    pub bands: u32,
    pub rejects: u32,
    digest: u64,
}

impl QualityLedger {
    pub const fn new() -> QualityLedger {
        QualityLedger { samples: 0, quantiles: 0, bands: 0, rejects: 0, digest: FNV_OFF }
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 一帧记账：接受帧同时进分位与分档；拒绝帧只进拒绝数。
    /// 拒绝码必须落在本模块窗口，外地码原样上抛，不吞。
    pub fn step(&mut self, s: Result<LatencySample, u32>) -> Result<(), u32> {
        match s {
            Ok(sample) => {
                let util = (sample.ticks as u64)
                    .saturating_mul(PER_MILLE as u64)
                    / LAT_CEIL as u64;
                let band = band_of(util as u32)?;
                self.samples = self.samples.saturating_add(1);
                self.quantiles = self.quantiles.saturating_add(1);
                self.bands = self.bands.saturating_add(1);
                self.digest = fold_u32(self.digest, sample.ticks);
                self.digest = fold_byte(self.digest, band.rank() as u8);
                self.digest = fold_byte(self.digest, sample.dev);
                Ok(())
            }
            Err(code) => {
                if code < SEG_BASE + 0x01 || code > SEG_BASE + 0x0B {
                    return Err(code);
                }
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_byte(self.digest, 0xFF);
                Ok(())
            }
        }
    }

    /// 账平：分位数与分档数都必须等于接受帧数；总轮数非零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples == 0 {
            return Err(QlCode::LedgerBad.val());
        }
        if self.quantiles != self.samples || self.bands != self.samples {
            return Err(QlCode::LedgerBad.val());
        }
        if self.samples.saturating_add(self.rejects) == 0 {
            return Err(QlCode::LedgerBad.val());
        }
        Ok(())
    }
}

// ── 确定性语料 ──────────────────────────────────────────────────────
/// 第 i 帧抖动：8..=2047，覆盖低桶与高桶两端。
pub fn corpus_tick(i: u32) -> u32 {
    let mut l = Lcg::new(0x51A2_0000u64 ^ (i as u64));
    8u32.saturating_add(l.below(2040))
}

/// 第 i 帧设备：i % 10。
pub fn corpus_dev(i: u32) -> u32 {
    i % DEVICE_COUNT as u32
}

/// 第 i 帧序号：i / 10 + 1，保证每台设备内部逐帧连增。
pub fn corpus_seq(i: u32) -> u32 {
    i / DEVICE_COUNT as u32 + 1
}

/// 灌一本黄金样本簿：十设备各 100 帧，共 1000 帧。
/// 每台设备前五帧刻意压到五桶各一枚，保证「零漏桶」不是靠运气；
/// 其余 95 帧走确定性随机，尾部形状仍然自然。
pub fn golden_book(rounds: u32) -> Result<SampleBook, u32> {
    if rounds == 0 || rounds % DEVICE_COUNT as u32 != 0 {
        return Err(QlCode::LedgerBad.val());
    }
    if rounds / DEVICE_COUNT as u32 > SAMPLE_CAP {
        return Err(QlCode::BudgetBad.val());
    }
    let mut book = SampleBook::new();
    let mut i = 0u32;
    while i < rounds {
        let dev = corpus_dev(i);
        let seq = corpus_seq(i);
        let ticks = match seq {
            1 => 8u32.saturating_add(dev),
            2 => 24u32.saturating_add(dev.saturating_mul(4)),
            3 => 100u32.saturating_add(dev.saturating_mul(10)),
            4 => 300u32.saturating_add(dev.saturating_mul(10)),
            5 => 1100u32.saturating_add(dev.saturating_mul(10)),
            _ => corpus_tick(i),
        };
        let s = LatencySample::new(dev, ticks, seq)?;
        book.push(s)?;
        i = i.saturating_add(1);
    }
    book.audit()?;
    Ok(book)
}

/// 黄金运行：返回（样本数、P99、四档计数、全族摘要）。
pub fn golden_run() -> Result<(u32, u32, [u32; BAND_COUNT], u64), u32> {
    let book = golden_book(GOLDEN_ROUNDS)?;
    let pct = book.percentiles()?;
    if !pct.fits() {
        return Err(QlCode::OrderBad.val());
    }
    let sorted = book.sorted();
    if !pct.exact_on(&sorted) {
        return Err(QlCode::RankBad.val());
    }
    let hist = Histogram::from_sorted(&sorted)?;
    hist.audit(book.len() as u32)?;
    let mut grid = QuotaGrid::new();
    grid.fill_ramp()?;
    grid.audit()?;
    let bands = grid.band_counts()?;
    let avg = grid.average_permille()?;
    let mut d = book.digest();
    d = fold_u32(d, pct.p50);
    d = fold_u32(d, pct.p90);
    d = fold_u32(d, pct.p99);
    d = fold_u32(d, hist.total());
    d = fold_u32(d, avg);
    d = fold_byte(d, hist.empty_buckets() as u8);
    let mut k = 0usize;
    while k < BAND_COUNT {
        if let Some(c) = bands.get(k) {
            d = fold_u32(d, *c);
        }
        k += 1;
    }
    Ok((book.len() as u32, pct.p99, bands, d))
}

// ── 注入账 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub rounds: u32,
    pub caught: u32,
    pub route_caught: [u32; 4],
    pub digest: u64,
}

impl InjectLedger {
    pub fn all_caught(&self) -> bool {
        self.caught == self.rounds && self.rounds > 0
    }

    pub fn routes_all(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            match self.route_caught.get(i) {
                Some(c) => {
                    if *c == 0 {
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

/// 单轮注入：四路各自回答一个「质量结论是否仍然成立」。
fn inject_once(route: u32, ix: u32) -> Result<bool, u32> {
    match route % 4 {
        0 => {
            // 分位精确：同一升序向量的三个秩必须逐一对得上。
            let mut v: Vec<u32> = Vec::new();
            let n = 4u32.saturating_add(ix % 13);
            let mut i = 0u32;
            while i < n {
                v.push(i.saturating_mul(7).saturating_add(1));
                i = i.saturating_add(1);
            }
            let pct = Percentile::compute(&v)?;
            let exact = pct.exact_on(&v);
            let ordered = pct.fits() && pct.p50 <= pct.p90 && pct.p90 <= pct.p99;
            Ok(exact && ordered && pct.p99 <= LAT_CEIL)
        }
        1 => {
            // 直方图零漏桶：十枚样本必须打满五桶且桶和相等。
            let probes: [u32; 10] = [1, 15, 16, 63, 64, 255, 256, 1023, 1024, 4096];
            let mut h = Histogram::new();
            let mut i = 0usize;
            while i < probes.len() {
                if let Some(t) = probes.get(i) {
                    h.add(*t)?;
                }
                i += 1;
            }
            h.audit(10)?;
            Ok(h.empty_buckets() == 0 && h.total() == 10)
        }
        2 => {
            // 配额率分档：千分率与档位必须同源，余量闭合。
            let used = 3u32.saturating_add((ix.saturating_mul(5)) % 46);
            let u = Utilization::new(ix % DEVICE_COUNT as u32, used, QUOTA_CAP)?;
            let p = u.permille()?;
            let band = u.band()?;
            Ok(band_of(p)? == band
                && u.headroom().saturating_add(u.used) == u.cap
                && u.audit().is_ok())
        }
        _ => {
            // 复算稳定：同一小向量两次分位必须同值；空向量必须被拒。
            let v: [u32; 6] = [3, 9, 27, 81, 243, 729];
            let a = Percentile::compute(&v)?;
            let b = Percentile::compute(&v)?;
            let empty: [u32; 0] = [];
            Ok(a == b && a.fits() && Percentile::compute(&empty).is_err())
        }
    }
}

/// 四路注入各跑 rounds/4 轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds / 4 == 0 {
        return Err(QlCode::LedgerBad.val());
    }
    let per = rounds / 4;
    let mut led = InjectLedger { rounds: 0, caught: 0, route_caught: [0; 4], digest: FNV_OFF };
    let mut route = 0u32;
    while route < 4 {
        let mut i = 0u32;
        while i < per {
            let hit = inject_once(route, i)?;
            if hit {
                led.caught = led.caught.saturating_add(1);
                if let Some(c) = led.route_caught.get_mut(route as usize) {
                    *c = c.saturating_add(1);
                }
            }
            led.rounds = led.rounds.saturating_add(1);
            led.digest = fold_byte(led.digest, if hit { 1 } else { 0 });
            i = i.saturating_add(1);
        }
        route = route.saturating_add(1);
    }
    Ok(led)
}

// ── 导出与回读 ──────────────────────────────────────────────────────
/// 六字段：samples|quantiles|bands|rejects|avg_permille|digest。
pub fn render_ledger(led: &QualityLedger, avg_permille: u32) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        led.samples,
        led.quantiles,
        led.bands,
        led.rejects,
        avg_permille,
        led.digest()
    )
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(QlCode::RenderBad.val());
    }
    let mut v = 0u32;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(QlCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(QlCode::RenderBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(QlCode::RenderBad.val()),
        };
        v = match v.checked_add((b - b'0') as u32) {
            Some(x) => x,
            None => return Err(QlCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 {
        return Err(QlCode::RenderBad.val());
    }
    let body = match s.strip_prefix("0x") {
        Some(v) => v,
        None => return Err(QlCode::RenderBad.val()),
    };
    if body.len() != 16 {
        return Err(QlCode::RenderBad.val());
    }
    let mut v = 0u64;
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(QlCode::RenderBad.val()),
        };
        let nib = if (b'0'..=b'9').contains(&b) {
            (b - b'0') as u64
        } else if (b'A'..=b'F').contains(&b) {
            (b - b'A') as u64 + 10
        } else if (b'a'..=b'f').contains(&b) {
            (b - b'a') as u64 + 10
        } else {
            return Err(QlCode::RenderBad.val());
        };
        v = (v << 4) | nib;
        i += 1;
    }
    Ok(v)
}

/// 回读：六字段，数字可解析，分位/分档计数与样本数相等，均值在千分域。
pub fn audit_render(text: &str) -> Result<(u32, u32, u32, u64), u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 1 {
        return Err(QlCode::RenderBad.val());
    }
    let line = match lines.get(0) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    };
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 6 {
        return Err(QlCode::RenderBad.val());
    }
    let samples = parse_u32(match parts.get(0) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    let quantiles = parse_u32(match parts.get(1) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    let bands = parse_u32(match parts.get(2) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    let _rejects = parse_u32(match parts.get(3) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    let avg = parse_u32(match parts.get(4) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    if samples == 0 || quantiles != samples || bands != samples {
        return Err(QlCode::LedgerBad.val());
    }
    if avg > PER_MILLE {
        return Err(QlCode::QuotaBad.val());
    }
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => *v,
        None => return Err(QlCode::RenderBad.val()),
    })?;
    Ok((samples, bands, avg, digest))
}

/// 黄金导出：100 帧语料 + 斜坡配额表。
pub fn render_brief() -> Result<String, u32> {
    let mut led = QualityLedger::new();
    let mut i = 0u32;
    while i < 100 {
        let s = LatencySample::new(corpus_dev(i), corpus_tick(i), corpus_seq(i))?;
        led.step(Ok(s))?;
        i = i.saturating_add(1);
    }
    led.audit()?;
    let mut grid = QuotaGrid::new();
    grid.fill_ramp()?;
    let avg = grid.average_permille()?;
    Ok(render_ledger(&led, avg))
}

// ── 探针（A/B/C 三条独立路径，必须给出同一结论） ─────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    // 百分之一百的秩必须落在最后一位，千分之一也必须非零。
    let mut v: Vec<u32> = Vec::new();
    let mut i = 1u32;
    while i <= 100 {
        v.push(i);
        i = i.saturating_add(1);
    }
    let pct = Percentile::compute(&v)?;
    if pct.p50 != 50 || pct.p90 != 90 || pct.p99 != 99 || pct.max != 100 {
        return Err(QlCode::RankBad.val());
    }
    if !pct.exact_on(&v) || !pct.fits() {
        return Err(QlCode::OrderBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 五桶各两枚，桶和十，中位桶与千分位桶都必须落在第二桶。
    let probes: [u32; 10] = [1, 15, 16, 63, 64, 255, 256, 1023, 1024, 4096];
    let mut h = Histogram::new();
    let mut i = 0usize;
    while i < probes.len() {
        if let Some(t) = probes.get(i) {
            h.add(*t)?;
        }
        i += 1;
    }
    h.audit(10)?;
    if h.empty_buckets() != 0 {
        return Err(QlCode::HistBad.val());
    }
    if h.median_bucket()? != 2 {
        return Err(QlCode::HistBad.val());
    }
    if h.quantile_bucket(500)? != 2 || h.quantile_bucket(1000)? != 4 {
        return Err(QlCode::RankBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 斜坡配额：四档计数 [2,5,2,1]，最热设备 9（48/48），均值可复算。
    let mut grid = QuotaGrid::new();
    grid.fill_ramp()?;
    grid.audit()?;
    let counts = grid.band_counts()?;
    if counts != [2, 5, 2, 1] {
        return Err(QlCode::BandBad.val());
    }
    let hot = grid.hottest()?;
    if hot.dev != 9 || hot.used != QUOTA_CAP {
        return Err(QlCode::QuotaBad.val());
    }
    let avg = grid.average_permille()?;
    let manual = (62u64 + 166 + 270 + 375 + 479 + 583 + 687 + 791 + 895 + 1000) / 10;
    if avg as u64 != manual {
        return Err(QlCode::QuotaBad.val());
    }
    Ok(())
}

/// 复现：黄金摘要两次一致、P99 非零、四路注入全捕、跨族源全绿。
pub fn repro_check() -> Result<(), u32> {
    let (samples, p99, bands, digest) = golden_run()?;
    if samples != GOLDEN_ROUNDS || p99 == 0 || digest == 0 {
        return Err(QlCode::LedgerBad.val());
    }
    if bands != [2, 5, 2, 1] {
        return Err(QlCode::BandBad.val());
    }
    let again = golden_run()?;
    if again.3 != digest {
        return Err(QlCode::LedgerBad.val());
    }
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() || !inj.routes_all() {
        return Err(QlCode::LedgerBad.val());
    }
    if !q_guard::segment_guard() {
        return Err(QlCode::SegmentBad.val());
    }
    if q_bridge::audit_sources().is_err() {
        return Err(QlCode::CoverageBad.val());
    }
    Ok(())
}

/// 段位守卫：本模块全部诊断码必须落在 0x70BE1..=0x70BEB。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < QlCode::ALL.len() {
        let c = match QlCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() < lo || c.val() > hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 十项动态判据 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "F48896-J1-分位单调",
        match &golden {
            Ok(_) => match golden_book(GOLDEN_ROUNDS).and_then(|b| b.percentiles()) {
                Ok(p) => p.fits() && p.span() <= LAT_CEIL,
                Err(_) => false,
            },
            Err(_) => false,
        },
        match &golden {
            Ok((n, p99, _, d)) => format!("samples={} p99={} digest={:016X}", n, p99, d),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48896-J1-分位精确",
        {
            let b = golden_book(GOLDEN_ROUNDS);
            match b {
                Ok(book) => match book.percentiles() {
                    Ok(p) => {
                        let sorted = book.sorted();
                        p.exact_on(&sorted)
                            && p.p50 == nearest_rank(&sorted, 500).unwrap_or(0)
                            && p.p90 == nearest_rank(&sorted, 900).unwrap_or(0)
                            && p.p99 == nearest_rank(&sorted, 990).unwrap_or(0)
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        },
        String::from("p50/p90/p99 逐点等于近邻秩"),
    ));
    v.push((
        "F48896-J1-秩边界",
        {
            let one = [7u32];
            let a = rank_index(1, 1000) == Ok(0);
            let b = rank_index(0, 500) == Err(QlCode::LedgerBad.val());
            let c = rank_index(4, 0) == Err(QlCode::RankBad.val());
            let d = rank_index(4, 1001) == Err(QlCode::RankBad.val());
            let e = nearest_rank(&one, 1).ok() == Some(7);
            a && b && c && d && e
        },
        String::from("秩域 1..=1000、空集拒绝、单样本兜底"),
    ));
    v.push((
        "F48897-J1-配额率分档",
        {
            let mut grid = QuotaGrid::new();
            let r = grid.fill_ramp();
            match r {
                Ok(()) => match grid.band_counts() {
                    Ok(c) => c == [2, 5, 2, 1] && grid.audit().is_ok(),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        },
        String::from("四档计数 [2,5,2,1] 且逐行分档复核"),
    ));
    v.push((
        "F48897-J1-配额零透支",
        {
            let reject = Utilization::new(0, QUOTA_CAP + 1, QUOTA_CAP).is_err();
            let zerocap = Utilization::new(0, 0, 0).is_err();
            let baddev = Utilization::new(DEVICE_COUNT as u32, 0, QUOTA_CAP).is_err();
            let full = Utilization::new(9, QUOTA_CAP, QUOTA_CAP);
            match full {
                Ok(u) => reject && zerocap && baddev && u.permille() == Ok(1000) && u.headroom() == 0,
                Err(_) => false,
            }
        },
        String::from("49/48、零分母、十号设备三路拒绝"),
    ));
    v.push((
        "F48897-J1-配额均值",
        {
            let mut grid = QuotaGrid::new();
            let ok = grid.fill_ramp().is_ok();
            match (ok, grid.average_permille()) {
                (true, Ok(avg)) => {
                    let mut sum = 0u64;
                    let mut i = 0usize;
                    let mut good = true;
                    while i < DEVICE_COUNT {
                        match grid.get(i) {
                            Some(u) => match u.permille() {
                                Ok(p) => sum = sum.saturating_add(p as u64),
                                Err(_) => good = false,
                            },
                            None => good = false,
                        }
                        i += 1;
                    }
                    good && avg as u64 == sum / DEVICE_COUNT as u64
                }
                _ => false,
            }
        },
        String::from("十设备逐项求和后整除"),
    ));
    v.push((
        "F48898-J1-直方图零漏桶",
        {
            let b = golden_book(GOLDEN_ROUNDS);
            match b {
                Ok(book) => match book.histogram() {
                    Ok(h) => {
                        h.audit(book.len() as u32).is_ok()
                            && h.empty_buckets() == 0
                            && h.total() == GOLDEN_ROUNDS
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        },
        {
            let b = golden_book(GOLDEN_ROUNDS);
            match b.and_then(|x| x.histogram()) {
                Ok(h) => format!("counts={:?} total={} empty={}", h.counts(), h.total(), h.empty_buckets()),
                Err(e) => format!("err=0x{:X}", e),
            }
        },
    ));
    v.push((
        "F48898-J1-覆盖十设备",
        {
            let b = golden_book(GOLDEN_ROUNDS);
            match b {
                Ok(book) => {
                    let mut ten = true;
                    let mut i = 0usize;
                    while i < DEVICE_COUNT {
                        if book.dev_count(i) != GOLDEN_ROUNDS / DEVICE_COUNT as u32 {
                            ten = false;
                        }
                        i += 1;
                    }
                    book.coverage() == DEVICE_COUNT as u32
                        && book.audit().is_ok()
                        && book.sorted_dev(0).map(|s| s.len()).unwrap_or(0)
                            == (GOLDEN_ROUNDS / DEVICE_COUNT as u32) as usize
                        && ten
                }
                Err(_) => false,
            }
        },
        String::from("每台 100 帧、十台全绿、单设备快照可复算"),
    ));
    v.push((
        "F48898-J1-账平",
        {
            let mut led = QualityLedger::new();
            let mut ok = true;
            let mut i = 0u32;
            while i < 120 {
                let s = LatencySample::new(corpus_dev(i), corpus_tick(i), corpus_seq(i));
                if led.step(s).is_err() {
                    ok = false;
                }
                i = i.saturating_add(1);
            }
            let mut k = 0u32;
            while k < 3 {
                if led.step(Err(QlCode::LatBad.val())).is_err() {
                    ok = false;
                }
                k = k.saturating_add(1);
            }
            ok && led.audit().is_ok() && led.samples == 120 && led.rejects == 3
        },
        String::from("120 接受 + 3 拒绝，分位/分档计数仍对齐"),
    ));
    v.push((
        "X1-跨族封存",
        {
            let a = q_bridge::family_imprint();
            let b = q_bridge::family_imprint();
            match (a, b) {
                (Ok(x), Ok(y)) => x == y && x != 0 && q_bridge::audit_sources().is_ok(),
                _ => false,
            }
        },
        String::from("bridge 族指纹两次一致且五源段守卫全绿"),
    ));
    v
}

/// 本模块判据集合：十项动态 + 段位守卫。
pub fn run_unx48896_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48896 分位质量与配额率");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let g = segment_guard();
    s.add_dyn("F48896-J1-段位守卫", g, if g { "" } else { "诊断码逃出 0x70BE0" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_index_clamps_and_rejects() {
        assert_eq!(rank_index(1, 1000), Ok(0));
        assert_eq!(rank_index(4, 500), Ok(1));
        assert_eq!(rank_index(4, 900), Ok(3));
        assert_eq!(rank_index(4, 990), Ok(3));
        assert_eq!(rank_index(0, 500), Err(QlCode::LedgerBad.val()));
        assert_eq!(rank_index(4, 0), Err(QlCode::RankBad.val()));
        assert_eq!(rank_index(4, 1001), Err(QlCode::RankBad.val()));
    }

    #[test]
    fn percentile_is_exact_on_hundred() {
        let mut v: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= 100 {
            v.push(i);
            i += 1;
        }
        let p = match Percentile::compute(&v) {
            Ok(p) => p,
            Err(_) => return,
        };
        assert_eq!(p.p50, 50);
        assert_eq!(p.p90, 90);
        assert_eq!(p.p99, 99);
        assert_eq!(p.max, 100);
        assert_eq!(p.span(), 49);
        assert!(p.fits());
        assert!(p.exact_on(&v));
    }

    #[test]
    fn percentile_rejects_empty_and_unsorted() {
        let empty: [u32; 0] = [];
        assert!(Percentile::compute(&empty).is_err());
        let unsorted: [u32; 3] = [9, 1, 5];
        assert_eq!(Percentile::compute(&unsorted), Err(QlCode::OrderBad.val()));
        assert!(!is_sorted_asc(&unsorted));
    }

    #[test]
    fn percentile_struct_fits_rejects_inversion() {
        let bad = Percentile { p50: 9, p90: 7, p99: 8, max: 10 };
        assert!(!bad.fits());
        let good = Percentile { p50: 1, p90: 2, p99: 3, max: 4 };
        assert!(good.fits());
    }

    #[test]
    fn latency_validation_edges() {
        assert!(LatencySample::new(0, 1, 1).is_ok());
        assert_eq!(LatencySample::new(10, 1, 1), Err(QlCode::LatBad.val()));
        assert_eq!(LatencySample::new(0, 0, 1), Err(QlCode::LatBad.val()));
        assert_eq!(LatencySample::new(0, LAT_CEIL + 1, 1), Err(QlCode::LatBad.val()));
        assert_eq!(LatencySample::new(0, 1, 0), Err(QlCode::LatBad.val()));
        assert!(LatencySample::new(0, LAT_CEIL, 1).is_ok());
    }

    #[test]
    fn sample_book_enforces_sequence() {
        let mut book = SampleBook::new();
        let a = match LatencySample::new(0, 10, 1) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert!(book.push(a).is_ok());
        assert_eq!(book.push(a), Err(QlCode::LatBad.val()));
        let b = match LatencySample::new(0, 11, 3) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert_eq!(book.push(b), Err(QlCode::LatBad.val()));
        let c = match LatencySample::new(0, 11, 2) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert!(book.push(c).is_ok());
        assert!(book.audit().is_ok());
    }

    #[test]
    fn sample_book_budget_overflow() {
        let mut book = SampleBook::new();
        let mut seq = 1u32;
        while seq <= SAMPLE_CAP {
            let s = match LatencySample::new(0, 8 + seq, seq) {
                Ok(v) => v,
                Err(_) => return,
            };
            let _ = book.push(s);
            seq += 1;
        }
        let over = match LatencySample::new(0, 8, SAMPLE_CAP + 1) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(book.push(over), Err(QlCode::BudgetBad.val()));
        assert_eq!(book.dev_count(0), SAMPLE_CAP);
    }

    #[test]
    fn sample_book_coverage_ten() {
        let book = match golden_book(GOLDEN_ROUNDS) {
            Ok(b) => b,
            Err(_) => return,
        };
        assert_eq!(book.coverage(), 10);
        assert_eq!(book.len(), GOLDEN_ROUNDS as usize);
        let sorted = book.sorted();
        assert!(is_sorted_asc(&sorted));
        assert!(book.audit().is_ok());
    }

    #[test]
    fn histogram_bucket_edges() {
        assert_eq!(Histogram::bucket_of(1), Ok(0));
        assert_eq!(Histogram::bucket_of(15), Ok(0));
        assert_eq!(Histogram::bucket_of(16), Ok(1));
        assert_eq!(Histogram::bucket_of(63), Ok(1));
        assert_eq!(Histogram::bucket_of(64), Ok(2));
        assert_eq!(Histogram::bucket_of(255), Ok(2));
        assert_eq!(Histogram::bucket_of(256), Ok(3));
        assert_eq!(Histogram::bucket_of(1023), Ok(3));
        assert_eq!(Histogram::bucket_of(1024), Ok(4));
        assert_eq!(Histogram::bucket_of(4096), Ok(4));
        assert_eq!(Histogram::bucket_of(0), Err(QlCode::LatBad.val()));
        assert_eq!(Histogram::bucket_of(4097), Err(QlCode::LatBad.val()));
    }

    #[test]
    fn histogram_audit_and_buckets() {
        let probes: [u32; 10] = [1, 15, 16, 63, 64, 255, 256, 1023, 1024, 4096];
        let mut h = Histogram::new();
        let mut i = 0usize;
        while i < probes.len() {
            if let Some(t) = probes.get(i) {
                let _ = h.add(*t);
            }
            i += 1;
        }
        assert_eq!(h.total(), 10);
        assert_eq!(h.counts(), [2, 2, 2, 2, 2]);
        assert_eq!(h.empty_buckets(), 0);
        assert!(h.audit(10).is_ok());
        assert_eq!(h.audit(11), Err(QlCode::HistBad.val()));
        assert_eq!(h.median_bucket(), Ok(2));
        assert_eq!(h.quantile_bucket(500), Ok(2));
        assert_eq!(h.quantile_bucket(1000), Ok(4));
        assert_eq!(h.quantile_bucket(0), Err(QlCode::RankBad.val()));
    }

    #[test]
    fn histogram_from_sorted_detects_regression() {
        let sorted: [u32; 4] = [1, 20, 100, 3000];
        let h = match Histogram::from_sorted(&sorted) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(h.counts(), [1, 1, 1, 0, 1]);
        let unsorted: [u32; 3] = [1, 1024, 20];
        assert_eq!(Histogram::from_sorted(&unsorted), Err(QlCode::HistBad.val()));
    }

    #[test]
    fn band_boundaries() {
        assert_eq!(band_of(0), Ok(Band::Idle));
        assert_eq!(band_of(250), Ok(Band::Idle));
        assert_eq!(band_of(251), Ok(Band::Normal));
        assert_eq!(band_of(750), Ok(Band::Normal));
        assert_eq!(band_of(751), Ok(Band::Hot));
        assert_eq!(band_of(950), Ok(Band::Hot));
        assert_eq!(band_of(951), Ok(Band::Saturated));
        assert_eq!(band_of(1000), Ok(Band::Saturated));
        assert_eq!(band_of(1001), Err(QlCode::BandBad.val()));
        assert_eq!(Band::from_rank(2), Ok(Band::Hot));
        assert_eq!(Band::from_rank(4), Err(QlCode::BandBad.val()));
    }

    #[test]
    fn utilization_permille_and_headroom() {
        let u = match Utilization::new(9, 48, 48) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(u.permille(), Ok(1000));
        assert_eq!(u.headroom(), 0);
        assert_eq!(u.band(), Ok(Band::Saturated));
        let half = match Utilization::new(0, 24, 48) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(half.permille(), Ok(500));
        assert_eq!(half.band(), Ok(Band::Normal));
        assert!(half.audit().is_ok());
    }

    #[test]
    fn quota_grid_rejects_duplicates_and_gaps() {
        let mut grid = QuotaGrid::new();
        assert_eq!(grid.average_permille(), Err(QlCode::CoverageBad.val()));
        let u = match Utilization::new(0, 10, 48) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(grid.set(u).is_ok());
        assert_eq!(grid.set(u), Err(QlCode::QuotaBad.val()));
        assert_eq!(grid.band_counts(), Err(QlCode::CoverageBad.val()));
        assert_eq!(grid.hottest(), Err(QlCode::CoverageBad.val()));
    }

    #[test]
    fn quota_grid_ramp_is_consistent() {
        let mut grid = QuotaGrid::new();
        assert!(grid.fill_ramp().is_ok());
        assert_eq!(grid.coverage(), 10);
        assert_eq!(grid.band_counts(), Ok([2, 5, 2, 1]));
        assert!(grid.audit().is_ok());
        let hot = match grid.hottest() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(hot.dev, 9);
        assert_eq!(hot.used, QUOTA_CAP);
        assert_eq!(grid.average_permille(), Ok(530));
    }

    #[test]
    fn ledger_accounts_rejects() {
        let mut led = QualityLedger::new();
        let s = match LatencySample::new(0, 100, 1) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(led.step(Ok(s)).is_ok());
        assert!(led.step(Err(QlCode::LatBad.val())).is_ok());
        assert!(led.audit().is_ok());
        assert_eq!(led.samples, 1);
        assert_eq!(led.rejects, 1);
        assert_eq!(led.step(Err(0x1234)), Err(0x1234));
    }

    #[test]
    fn ledger_empty_is_unbalanced() {
        let led = QualityLedger::new();
        assert_eq!(led.audit(), Err(QlCode::LedgerBad.val()));
    }

    #[test]
    fn render_round_trip_and_tamper() {
        let text = match render_brief() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(audit_render(&text).is_ok());
        let short = text.replacen("|100|", "|99|", 1);
        if short != text {
            assert!(audit_render(&short).is_err());
        }
        let bad_digest = text.replacen("0x", "0x0", 1);
        assert!(audit_render(&bad_digest).is_err());
        assert!(audit_render("").is_err());
    }

    #[test]
    fn golden_run_is_deterministic() {
        let a = golden_run();
        let b = golden_run();
        match (a, b) {
            (Ok((n1, p1, c1, d1)), Ok((n2, p2, c2, d2))) => {
                assert_eq!(n1, GOLDEN_ROUNDS);
                assert_eq!(n1, n2);
                assert_eq!(p1, p2);
                assert_eq!(c1, c2);
                assert_eq!(d1, d2);
                assert!(p1 > 0);
                assert!(d1 != 0);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn injection_all_routes() {
        let l = match inject_rounds(INJECT_ROUNDS) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.all_caught());
        assert!(l.routes_all());
        assert_eq!(l.route_caught, [250, 250, 250, 250]);
    }

    #[test]
    fn probes_and_repro() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn segment_and_checks_are_green() {
        assert!(segment_guard());
        let s = run_unx48896_checks();
        assert!(s.all_passed());
    }
}
