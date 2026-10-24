//! ves313_s16perf — VE-F4073 · T04 翻译性能
//! 管线四段吞吐基准（抽取/检索/机翻适配/合入各设阈值）+ 增量抽取 P95 预算 + 百万句对大库检索档 + 索引重建 + 合成语料红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8DC0;

pub const BIG_LIB_SIZE: u32 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TpCode {
    Ok = 0,
    ThroughputOver = SEG_BASE + 0x01,
    P95Over = SEG_BASE + 0x02,
    BigLibSlow = SEG_BASE + 0x03,
    RebuildNeeded = SEG_BASE + 0x04,
    SyntheticMissing = SEG_BASE + 0x05,
}

/// 四段吞吐阈值（键/秒）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage { Extract = 0, Lookup = 1, MtAdapt = 2, Merge = 3 }

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Extract, Stage::Lookup, Stage::MtAdapt, Stage::Merge];
    pub fn threshold(self) -> u32 {
        match self {
            Stage::Extract => 5000,
            Stage::Lookup => 20000,
            Stage::MtAdapt => 500,
            Stage::Merge => 8000,
        }
    }
    pub fn judge(self, measured: u32) -> Result<(), TpCode> {
        if measured < self.threshold() { return Err(TpCode::ThroughputOver); }
        Ok(())
    }
}

/// P95 预算：样本排序取 95 分位。
pub fn p95(samples: &mut Vec<u32>) -> u32 {
    if samples.is_empty() { return 0; }
    // 简单插入排序（样本量小）。
    for i in 1..samples.len() {
        let mut j = i;
        while j > 0 && samples[j] < samples[j - 1] {
            samples.swap(j, j - 1);
            j -= 1;
        }
    }
    let idx = samples.len() * 95 / 100;
    samples[idx.min(samples.len() - 1)]
}

pub fn p95_budget(samples: &mut Vec<u32>, budget_ms: u32) -> Result<(), TpCode> {
    let v = p95(samples);
    if v > budget_ms { return Err(TpCode::P95Over); }
    Ok(())
}

/// 大库检索档：百万句对下检索耗时分档。
pub fn big_lib_tier(lookup_ms: u32) -> Result<u8, TpCode> {
    if lookup_ms <= 1 { Ok(3) }
    else if lookup_ms <= 5 { Ok(2) }
    else if lookup_ms <= 20 { Ok(1) }
    else { Err(TpCode::BigLibSlow) }
}

/// 索引重建触发：命中率连续低于线。
pub struct IndexHealth {
    pub hit_rates: Vec<u32>, // 百分比序列
}

impl IndexHealth {
    pub fn new() -> Self { IndexHealth { hit_rates: Vec::new() } }
    pub fn push(&mut self, rate: u32) {
        self.hit_rates.push(rate);
    }
    /// 连续 3 次低于 50% → 重建。
    pub fn rebuild_needed(&self) -> bool {
        if self.hit_rates.len() < 3 { return false; }
        let n = self.hit_rates.len();
        self.hit_rates[n - 1] < 50 && self.hit_rates[n - 2] < 50 && self.hit_rates[n - 3] < 50
    }
}

/// 合成语料红线：基准必须声明合成。
pub fn synthetic_ok(synthetic: bool) -> Result<(), TpCode> {
    if synthetic { Ok(()) } else { Err(TpCode::SyntheticMissing) }
}

pub fn run_ves313_checks() -> CheckSet {
    let mut set = CheckSet::new("ves313_s16perf");

    // 1) 四段阈值齐。
    set.add("C4073-01", Stage::ALL.len() == 4 && Stage::Extract.threshold() == 5000 && Stage::MtAdapt.threshold() == 500, "四段阈值");

    // 2) 吞吐达标。
    set.add("C4073-02", Stage::Lookup.judge(25000).is_ok(), "吞吐达标");

    // 3) 吞吐越阈。
    set.add("C4073-03", Stage::Merge.judge(1000) == Err(TpCode::ThroughputOver), "吞吐越阈");

    // 4) P95 计算。
    let mut s = vec![10u32, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    set.add("C4073-04", p95(&mut s) == 96 || p95(&mut s) >= 95, "P95 计算");

    // 5) P95 预算。
    let mut s2 = vec![5u32, 6, 7, 8, 9, 10, 11, 12, 13, 50];
    set.add("C4073-05", p95_budget(&mut s2, 20).is_ok(), "P95 达标");
    let mut s3 = vec![10u32, 10, 10, 10, 10, 10, 10, 10, 10, 99];
    set.add("C4073-06", p95_budget(&mut s3, 50) == Err(TpCode::P95Over), "P95 超预算");

    // 6) 大库分档。
    set.add("C4073-07", big_lib_tier(1) == Ok(3) && big_lib_tier(10) == Ok(1) && big_lib_tier(50) == Err(TpCode::BigLibSlow), "大库分档");

    // 7) 大库常量。
    set.add("C4073-08", BIG_LIB_SIZE == 1_000_000, "百万句对");

    // 8) 索引重建触发。
    let mut ih = IndexHealth::new();
    ih.push(40);
    ih.push(45);
    set.add("C4073-09", !ih.rebuild_needed(), "不足三次不触发");
    ih.push(30);
    set.add("C4073-10", ih.rebuild_needed() == true, "连续三次触发重建");

    // 9) 合成语料红线。
    set.add("C4073-11", synthetic_ok(true).is_ok() && synthetic_ok(false) == Err(TpCode::SyntheticMissing), "合成语料");

    // 10) 段位自查。
    set.add("C4073-12", SEG_BASE == 0x8DC0, "段位 0x8DC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_checks() {
        assert!(Stage::Extract.judge(6000).is_ok());
        let mut s = vec![1u32, 2, 3, 4, 5];
        let v = p95(&mut s);
        assert!(v >= 4);
        assert!(big_lib_tier(3) == Ok(2));
    }
}
