//! ves356_s19perf — VE-F4116 · T06 地区性能
//! 地区格式引擎缓存命中基准（命中率阈值+冷启动耗时）+ 合规检查器批量性能（千规则×百地区档）+ 路由器查询 P95 + 命中率劣化缓存调优 + 批量超时分片并行 + 越阈 T07 仲裁重议 + 基准用合成数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC940;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    Ok = 0,
    HitRateLow = SEG_BASE + 0x01,
    ColdSlow = SEG_BASE + 0x02,
    BatchOver = SEG_BASE + 0x03,
    P95Over = SEG_BASE + 0x04,
}

/// 缓存命中率阈值（万分比）。
pub const HIT_RATE_FLOOR: u32 = 9_000; // 90%
/// 冷启动阈值 ms。
pub const COLD_BUDGET_MS: u32 = 300;
/// 批量档：千规则×百地区。
pub const BATCH_RULES: usize = 1_000;
pub const BATCH_REGIONS: usize = 100;
/// 批量预算 ms。
pub const BATCH_BUDGET_MS: u32 = 5_000;
/// 路由 P95 阈值 ms。
pub const ROUTE_P95_MS: u32 = 5;

/// 缓存基准。
pub struct CacheBench { pub hits: u32, pub misses: u32, pub cold_ms: u32 }

impl CacheBench {
    pub fn new() -> Self { CacheBench { hits: 0, misses: 0, cold_ms: 0 } }
    pub fn record(&mut self, hit: bool) {
        if hit { self.hits += 1; } else { self.misses += 1; }
    }
    /// 命中率（万分比；无查询记 0）。
    pub fn hit_rate(&self) -> u32 {
        let t = self.hits + self.misses;
        if t == 0 { 0 } else { self.hits * 10_000 / t }
    }
    pub fn gate(&self) -> Result<(), PfCode> {
        if self.hit_rate() < HIT_RATE_FLOOR { return Err(PfCode::HitRateLow); }
        if self.cold_ms > COLD_BUDGET_MS { return Err(PfCode::ColdSlow); }
        Ok(())
    }
}

/// 批量基准：千规则×百地区 → 耗时；超时分片并行（片数= ceil(耗时/预算)）。
pub struct BatchBench { pub elapsed_ms: u32 }

impl BatchBench {
    pub fn new(elapsed_ms: u32) -> Self { BatchBench { elapsed_ms } }

    pub fn workload(&self) -> usize { BATCH_RULES * BATCH_REGIONS }

    pub fn gate(&self) -> Result<usize, PfCode> {
        if self.elapsed_ms <= BATCH_BUDGET_MS { return Ok(1); }
        // 超时 → 分片并行。
        Ok(((self.elapsed_ms + BATCH_BUDGET_MS - 1) / BATCH_BUDGET_MS) as usize)
    }
}

/// 路由 P95：采样排序取 95 分位。
pub fn p95(samples: &[u32]) -> u32 {
    if samples.is_empty() { return 0; }
    let mut s = samples.to_vec();
    s.sort_unstable();
    let idx = (s.len() * 95 + 99) / 100; // ceil(0.95n)
    s[(idx - 1).min(s.len() - 1)]
}

pub fn route_gate(samples: &[u32]) -> Result<u32, PfCode> {
    let v = p95(samples);
    if v > ROUTE_P95_MS { Err(PfCode::P95Over) } else { Ok(v) }
}

/// 仲裁立案（T07 重议输入）。
pub struct ArbitrateCase { pub subject: &'static str, pub over: &'static str }

pub struct ArbitrateBook { pub cases: Vec<ArbitrateCase> }

impl ArbitrateBook {
    pub fn new() -> Self { ArbitrateBook { cases: Vec::new() } }
    pub fn open(&mut self, subject: &'static str, over: &'static str) { self.cases.push(ArbitrateCase { subject, over }); }
    pub fn count(&self) -> usize { self.cases.len() }
}

/// 合成数据：确定性查询键序列。
pub fn synth_keys(seed: u32, n: usize) -> Vec<u64> {
    let mut h = seed as u64;
    let mut out = Vec::new();
    for _ in 0..n {
        h = h.wrapping_mul(0x0100_0193) ^ 0x9E37_79B9;
        out.push(h);
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves356_checks() -> CheckSet {
    let mut set = CheckSet::new("ves356");

    // C356-01 阈值表单源。
    chk(&mut set, "C356-01",
        HIT_RATE_FLOOR == 9_000 && COLD_BUDGET_MS == 300 && ROUTE_P95_MS == 5, "阈值表单源");

    // C356-02 命中率达标。
    let mut cb = CacheBench::new();
    for i in 0..100 { cb.record(i < 95); }
    chk(&mut set, "C356-02", cb.hit_rate() == 9_500 && cb.gate().is_ok(), "95% 命中过线");

    // C356-03 命中率劣化 → 调优判红。
    let mut cb2 = CacheBench::new();
    for i in 0..100 { cb2.record(i < 85); }
    chk(&mut set, "C356-03", cb2.hit_rate() == 8_500 && cb2.gate() == Err(PfCode::HitRateLow), "85% < 90% => 红");

    // C356-04 冷启动超预算。
    let mut cb3 = CacheBench::new();
    for _ in 0..100 { cb3.record(true); }
    cb3.cold_ms = 350;
    chk(&mut set, "C356-04", cb3.gate() == Err(PfCode::ColdSlow), "冷启 350 > 300 => 红");

    // C356-05 批量档：千规则×百地区 = 10 万工作量。
    let b = BatchBench::new(4_000);
    chk(&mut set, "C356-05", b.workload() == 100_000 && b.gate() == Ok(1), "10 万档 4s 内单片");

    // C356-06 批量超时 → 分片并行。
    let b2 = BatchBench::new(12_000);
    chk(&mut set, "C356-06", b2.gate() == Ok(3), "12s → 3 片并行");

    // C356-07 批量越阈立案。
    let mut ab = ArbitrateBook::new();
    if b2.gate() != Ok(1) { ab.open("batch-compliance", "batch-over"); }
    chk(&mut set, "C356-07", ab.count() == 1, "批量越阈仲裁立案");

    // C356-08 P95 计算：20 采样取第 19。
    let samples = [1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10];
    chk(&mut set, "C356-08", p95(&samples) == 10, "P95 = 第 19 采样 10ms");

    // C356-09 路由 P95 越阈：10ms > 5ms → 红。
    chk(&mut set, "C356-09", route_gate(&samples) == Err(PfCode::P95Over), "P95 10ms > 5ms => 红");

    // C356-10 合格路由放行。
    let fast = [1, 2, 2, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5];
    chk(&mut set, "C356-10", route_gate(&fast) == Ok(5), "P95 5ms 过线");

    // C356-11 合成语料确定性。
    let k1 = synth_keys(9, 8);
    let k2 = synth_keys(9, 8);
    chk(&mut set, "C356-11", k1 == k2 && k1.len() == 8, "合成键确定");

    // C356-12 空采样 P95 为 0。
    chk(&mut set, "C356-12", p95(&[]) == 0, "空采样安全");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves356_smoke() {
        let set = run_ves356_checks();
        assert!(set.failed_count() == 0, "ves356 red: {}", set.red_items_text());
    }

    #[test]
    fn ves356_p95() {
        assert_eq!(p95(&[7]), 7);
        assert_eq!(p95(&[1, 2]), 2);
    }
}
