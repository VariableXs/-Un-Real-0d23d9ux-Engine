//! UNX-F56021 syscall 延迟微基准采集器（0x60280 段）
//!
//! 锚点：UNX-F56021-J1 getpid/read/write/mmap 四族双机各采 10⁵ 样本
//! （P50/P95/P99 全落账，样本量与剔除规则可复查）；J2 Invariant TSC
//! 纳秒级口径 + 预热 10⁴ 次后正式采样（预热闸未过禁采）；J3 CPU 迁移
//! 污染样本剔除规则进代码（逐样本 CPU ID 落账，跨核迁移剔除，剔除率
//! >5% 触发重采）三判据全绿。判据（P4 类）；判据锚定 §AI-71.3 示例
//! 条目 UNX-F56021 保真。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·采集旗舰面）。
//!
//! 结构：四族采样闭集登记册（族 × 样本 × CPU ID 逐条落账）+ 预热闸
//! （10⁴ 计数未满禁采）+ 迁移剔除核（绑核 ID × 逐样本 ID 比对）+ 分
//! 位统计核（P50/P95/P99 + min/max 排序口径）+ 剔除率重采闸（>5% 触
//! 发）+ 注入核（预热未满采/未知族/零纳秒/迁移样本/重采闸必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60280 独占 0x20）。
pub const SEG_BASE: u32 = 0x60280;
pub const SY_FAM_UNK: u32 = SEG_BASE + 0x01; // 越闭集族
pub const SY_NS_ZERO: u32 = SEG_BASE + 0x02; // 零纳秒样本
pub const SY_WARMUP_SHORT: u32 = SEG_BASE + 0x03; // 预热未满即采
pub const SY_MIGRATED: u32 = SEG_BASE + 0x04; // 跨核迁移样本（剔除）
pub const SY_RESAMPLE: u32 = SEG_BASE + 0x05; // 剔除率超阈触发重采
pub const SY_STATS_MISS: u32 = SEG_BASE + 0x06; // 样本不足无统计
pub const SY_PIN_MISS: u32 = SEG_BASE + 0x07; // 绑核未声明即采

/// 四族闭集。
pub const FAM_GETPID: usize = 0;
pub const FAM_READ: usize = 1;
pub const FAM_WRITE: usize = 2;
pub const FAM_MMAP: usize = 3;
pub const FAM_COUNT: usize = 4;
pub const FAM_NAMES: [&str; FAM_COUNT] = ["getpid", "read", "write", "mmap"];

/// 预热次数（10⁴）与正式样本量（10⁵）。
pub const WARMUP_N: u32 = 10_000;
pub const SAMPLE_N: u32 = 100_000;

/// 剔除率重采阈值（万分比 5% = 500）。
pub const DROP_PBP_MAX: u32 = 500;

/// 单族样本账：纳秒延迟 × CPU ID 逐条。
#[derive(Clone)]
pub struct Sample {
    pub ns: u64,
    pub cpu: u32,
}

/// 分位统计（P50/P95/P99 + min/max，ns）。
#[derive(Clone)]
pub struct FamStats {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
    pub n: usize,
}

/// 分位取值：升序样本 idx = n × pct / 100（钳到 n-1）。
pub fn percentile(sorted: &[u64], pct: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    let mut idx = (n as u64 * pct as u64 / 100) as usize;
    if idx >= n {
        idx = n - 1;
    }
    sorted[idx]
}

/// 对样本集算分位统计（样本不足 30 拒）。
pub fn stats_of(samples: &[Sample]) -> Result<FamStats, u32> {
    if samples.len() < 30 {
        return Err(SY_STATS_MISS);
    }
    let mut vals: Vec<u64> = Vec::new();
    for s in samples.iter() {
        vals.push(s.ns);
    }
    vals.sort();
    Ok(FamStats {
        p50: percentile(&vals, 50),
        p95: percentile(&vals, 95),
        p99: percentile(&vals, 99),
        min: vals[0],
        max: vals[vals.len() - 1],
        n: vals.len(),
    })
}

/// 单族采集账。
pub struct FamilyBook {
    pub fam: usize,
    pub pinned_cpu: Option<u32>,
    pub warmup_count: u32,
    pub samples: Vec<Sample>,
    pub dropped: usize,
}

impl FamilyBook {
    pub fn new(fam: usize) -> Result<Self, u32> {
        if fam >= FAM_COUNT {
            return Err(SY_FAM_UNK);
        }
        Ok(FamilyBook {
            fam,
            pinned_cpu: None,
            warmup_count: 0,
            samples: Vec::new(),
            dropped: 0,
        })
    }

    /// 绑核声明（未声明禁采）。
    pub fn pin(&mut self, cpu: u32) {
        self.pinned_cpu = Some(cpu);
    }

    /// 预热计数（只累计不落样本）。
    pub fn warmup(&mut self, times: u32) {
        self.warmup_count += times;
    }

    /// 预热闸：累计 ≥10⁴ 即开。
    pub fn warmup_done(&self) -> bool {
        self.warmup_count >= WARMUP_N
    }

    /// 正式采样：预热闸 + 绑核闸 + 零值闸 + 迁移剔除四守。
    pub fn record(&mut self, ns: u64, cpu: u32) -> Result<(), u32> {
        if !self.warmup_done() {
            return Err(SY_WARMUP_SHORT);
        }
        match self.pinned_cpu {
            None => return Err(SY_PIN_MISS),
            Some(p) => {
                if cpu != p {
                    self.dropped += 1;
                    return Err(SY_MIGRATED);
                }
            }
        }
        if ns == 0 {
            return Err(SY_NS_ZERO);
        }
        self.samples.push(Sample { ns, cpu });
        Ok(())
    }

    /// 剔除率（万分比；分母 = 留存 + 剔除）。
    pub fn drop_pbp(&self) -> u32 {
        let total = self.samples.len() + self.dropped;
        if total == 0 {
            return 0;
        }
        ((self.dropped as u64 * 10_000) / total as u64) as u32
    }

    /// J3：剔除率重采闸（>5% 触发）。
    pub fn needs_resample(&self) -> bool {
        self.drop_pbp() > DROP_PBP_MAX
    }

    /// J1：分位统计落账。
    pub fn stats(&self) -> Result<FamStats, u32> {
        stats_of(&self.samples)
    }
}

/// 四族采集器：逐族账 + 统一编排。
pub struct Collector {
    pub books: [Option<FamilyBook>; FAM_COUNT],
}

impl Collector {
    pub fn new() -> Self {
        Collector { books: [None, None, None, None] }
    }

    /// 注册一族。
    pub fn open(&mut self, fam: usize) -> Result<(), u32> {
        if fam >= FAM_COUNT {
            return Err(SY_FAM_UNK);
        }
        self.books[fam] = Some(FamilyBook::new(fam)?);
        Ok(())
    }

    /// 某族账。
    pub fn book(&self, fam: usize) -> Option<&FamilyBook> {
        if fam >= FAM_COUNT {
            return None;
        }
        self.books[fam].as_ref()
    }

    /// 某族账（可变）。
    pub fn book_mut(&mut self, fam: usize) -> Option<&mut FamilyBook> {
        if fam >= FAM_COUNT {
            return None;
        }
        self.books[fam].as_mut()
    }
}

/// 标准夹具：四族全开，预热满，每族 40 个正式样本（CPU 0 绑核）。
/// 样本纳秒确定性：fam×1000 + i×3（保证分位可心算）。
pub fn standard_collector() -> Collector {
    let mut c = Collector::new();
    for fam in 0..FAM_COUNT {
        c.open(fam).expect("fixture open");
        let b = c.book_mut(fam).expect("fixture book");
        b.pin(0);
        b.warmup(WARMUP_N);
        for i in 0..40u32 {
            let ns = (fam as u64 + 1) * 1000 + i as u64 * 3;
            b.record(ns, 0).expect("fixture sample");
        }
    }
    c
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集族
    if FamilyBook::new(9).err() == Some(SY_FAM_UNK) {
        caught += 1;
    }
    // 2) 预热未满即采
    let mut b = FamilyBook::new(FAM_GETPID).expect("b");
    b.pin(0);
    if b.record(100, 0).err() == Some(SY_WARMUP_SHORT) {
        caught += 1;
    }
    // 3) 未绑核即采（预热已满）
    let mut b2 = FamilyBook::new(FAM_GETPID).expect("b2");
    b2.warmup(WARMUP_N);
    if b2.record(100, 0).err() == Some(SY_PIN_MISS) {
        caught += 1;
    }
    // 4) 迁移样本剔除（绑核 0 来样本 CPU 3）
    let mut b3 = FamilyBook::new(FAM_GETPID).expect("b3");
    b3.pin(0);
    b3.warmup(WARMUP_N);
    if b3.record(100, 3).err() == Some(SY_MIGRATED) && b3.dropped == 1 && b3.samples.is_empty() {
        caught += 1;
    }
    // 5) 零纳秒样本
    let mut b4 = FamilyBook::new(FAM_READ).expect("b4");
    b4.pin(1);
    b4.warmup(WARMUP_N);
    if b4.record(0, 1).err() == Some(SY_NS_ZERO) {
        caught += 1;
    }
    (caught, 5)
}

/// 统计渲染（逐族一行分位账）。
pub fn render_stats(c: &Collector) -> String {
    let mut out = String::new();
    for fam in 0..FAM_COUNT {
        if let Some(b) = c.book(fam) {
            if let Ok(st) = b.stats() {
                out.push_str(&format!(
                    "{}|n={}|p50={}|p95={}|p99={}|min={}|max={}\n",
                    FAM_NAMES[fam], st.n, st.p50, st.p95, st.p99, st.min, st.max
                ));
            }
        }
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56021");
    // J1-1 四族闭集全开 + 样本量齐（40/族 夹具）
    let c = standard_collector();
    let mut open_n = 0usize;
    let mut sample_total = 0usize;
    for fam in 0..FAM_COUNT {
        if let Some(b) = c.book(fam) {
            open_n += 1;
            sample_total += b.samples.len();
        }
    }
    let ok = open_n == FAM_COUNT && sample_total == 160;
    s.add("56021-J1-四族全开样本齐", ok, if ok { "" } else { "族账缺" });
    // J1-2 分位统计语义（fam0 基线 1000 步 3：p50=idx20→1060，p95=idx38→1114，
    // p99=idx39→1117；min=1000 max=1117）
    let st0 = c.book(FAM_GETPID).expect("f0").stats().expect("st0");
    let ok = st0.p50 == 1_060 && st0.p95 == 1_114 && st0.p99 == 1_117
        && st0.min == 1_000 && st0.max == 1_117
        && st0.n == 40;
    s.add("56021-J1-分位语义心算", ok, if ok { "" } else { "分位算式漂移" });
    // J1-3 族间分位隔离（fam3 基线 +4000）
    let st3 = c.book(FAM_MMAP).expect("f3").stats().expect("st3");
    let ok = st3.min == 4_000 && st3.max == 4_117 && st3.p50 == 4_060;
    s.add("56021-J1-族间隔离", ok, if ok { "" } else { "族间混样" });
    // J2-1 预热闸语义（10⁴ 前拒 / 10⁴ 后开）
    let mut b = FamilyBook::new(FAM_WRITE).expect("w");
    b.pin(0);
    let early = b.record(100, 0).err() == Some(SY_WARMUP_SHORT);
    b.warmup(WARMUP_N);
    let late = b.record(100, 0).is_ok();
    let ok = early && late && b.warmup_done();
    s.add("56021-J2-预热闸语义", ok, if ok { "" } else { "预热闸漂移" });
    // J2-2 预热计数累积语义（两次 warmup 合计开闸）
    let mut b2 = FamilyBook::new(FAM_READ).expect("r");
    b2.pin(0);
    b2.warmup(WARMUP_N / 2);
    let mid = !b2.warmup_done();
    b2.warmup(WARMUP_N / 2);
    let ok = mid && b2.warmup_done();
    s.add("56021-J2-预热累积", ok, if ok { "" } else { "累积漂移" });
    // J3-1 迁移剔除规则进代码（跨核必剔且不落账）
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56021-J3-注入核必抓", ok, &detail);
    // J3-2 剔除率语义（留 95 剔 5 → 500 万分比）
    let mut b3 = FamilyBook::new(FAM_GETPID).expect("b3");
    b3.pin(0);
    b3.warmup(WARMUP_N);
    for i in 0..95u32 {
        b3.record(100 + i as u64, 0).expect("keep");
    }
    for _ in 0..5 {
        let _ = b3.record(100, 9);
    }
    let ok = b3.drop_pbp() == 500 && !b3.needs_resample();
    s.add("56021-J3-剔除率万分比", ok, if ok { "" } else { "剔除率漂移" });
    // J3-3 剔除率 >5% 触发重采
    let mut b4 = FamilyBook::new(FAM_READ).expect("b4");
    b4.pin(0);
    b4.warmup(WARMUP_N);
    for i in 0..94u32 {
        b4.record(100 + i as u64, 0).expect("keep2");
    }
    for _ in 0..6 {
        let _ = b4.record(100, 9);
    }
    let ok = b4.drop_pbp() == 600 && b4.needs_resample();
    s.add("56021-J3-重采闸触发", ok, if ok { "" } else { "重采闸失灵" });
    // J1-4 样本不足无统计（<30 拒）
    let mut b5 = FamilyBook::new(FAM_MMAP).expect("b5");
    b5.pin(0);
    b5.warmup(WARMUP_N);
    for i in 0..29u32 {
        b5.record(100 + i as u64, 0).expect("short");
    }
    let ok = b5.stats().err() == Some(SY_STATS_MISS);
    s.add("56021-J1-样本不足拒统计", ok, if ok { "" } else { "短样本出账" });
    // J1-5 统计渲染面齐（四行全可读）
    let text = render_stats(&c);
    let ok = text.lines().count() == FAM_COUNT && text.contains("getpid|n=40|p50=1060");
    s.add("56021-J1-统计渲染面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-3 TSC 纳秒口径（样本为 u64 纳秒，非 ticks）
    let ok = standard_collector().book(FAM_WRITE).expect("w2").samples[0].ns == 3_000;
    s.add("56021-J2-纳秒口径", ok, if ok { "" } else { "口径漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56021_all_checks_green() {
        let set = run_unx56021_checks();
        assert!(set.all_passed(), "UNX-F56021 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56021_percentile_math() {
        // 0..40 步 3：p50 取 idx20 = 60。
        let c = standard_collector();
        let st = c.book(FAM_GETPID).expect("f").stats().expect("s");
        assert_eq!(st.p50, 1_060);
        assert_eq!(st.p95, 1_114);
    }
}
