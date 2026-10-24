//! VE-F4251 · 验证性能（U03 一致性验证平台）
//! 职责：平台吞吐基准（六类合并跑批时长预算）+ 增量验证延迟 P95 + 资源占用
//! 三线 + 跑批风暴分片并行 + 资源超限降级采样 + 无障碍延迟单列（不缩水）。
//! 段位：0x1D40（续表 idx485）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1D40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 跑批越预算。
    BatchOver = SEG_BASE,
    /// 增量 P95 越阈。
    P95Over = SEG_BASE + 0x01,
    /// 资源超限（触发降采样）。
    ResOver = SEG_BASE + 0x02,
    /// 无障碍延迟被缩水（红线）。
    A11yShrink = SEG_BASE + 0x03,
}

/// 三线预算（bp/单位自定，统一口径 bp）。
pub struct PerfBench {
    /// 跑批时长预算。
    pub batch_budget: u32,
    /// 增量 P95 上限。
    pub p95_cap: u32,
    /// 资源占用上限（百分比）。
    pub res_cap_pct: u8,
    /// 无障碍延迟上限（单列——永不放宽）。
    pub a11y_cap: u32,
    pub over_budgets: Vec<u8>,
    pub samples: Vec<u32>,
}

impl PerfBench {
    pub fn new(batch_budget: u32, p95_cap: u32, res_cap_pct: u8, a11y_cap: u32) -> Self {
        PerfBench { batch_budget, p95_cap, res_cap_pct, a11y_cap, over_budgets: Vec::new(), samples: Vec::new() }
    }

    /// 跑批核验：越阈定位并立案（0 批 1 P95 2 资源 3 无障碍）。
    pub fn check(&mut self, batch_ms: u32, p95_ms: u32, res_pct: u8, a11y_ms: u32) -> u8 {
        self.over_budgets.clear();
        if batch_ms > self.batch_budget {
            self.over_budgets.push(0);
        }
        if p95_ms > self.p95_cap {
            self.over_budgets.push(1);
        }
        if res_pct > self.res_cap_pct {
            self.over_budgets.push(2);
        }
        if a11y_ms > self.a11y_cap {
            self.over_budgets.push(3); // 无障碍单列越阈——单独立案不与其他合并
        }
        self.samples.push(p95_ms);
        if self.over_budgets.is_empty() { 0xFF } else { self.over_budgets[0] }
    }

    /// 跑批风暴：分片并行（每片容量 cap，返回分片数）。
    pub fn shard(&self, tasks: u32, cap: u32) -> u32 {
        if cap == 0 {
            return 0;
        }
        (tasks + cap - 1) / cap
    }

    /// 资源超限降采样：按步长 stride 抽样。
    pub fn downsample(&self, stride: usize) -> Vec<u32> {
        self.samples.iter().step_by(stride.max(1)).copied().collect()
    }

    /// P95 计算（样本升序第 95 百分位，简单插值）。
    pub fn p95(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut s = self.samples.clone();
        s.sort_unstable();
        let idx = (s.len() * 95 / 100).saturating_sub(1).min(s.len() - 1);
        s[idx]
    }

    /// 预算重议：新预算登记（无障碍上限永不放宽）。
    pub fn rebench(&mut self, batch: u32, p95: u32, a11y: u32) -> Result<(), PfCode> {
        if a11y > self.a11y_cap {
            return Err(PfCode::A11yShrink);
        }
        self.batch_budget = batch;
        self.p95_cap = p95;
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C485-01..12。
pub fn run_ves485_checks() -> CheckSet {
    let mut set = CheckSet::new("ves485");

    let mut b = PerfBench::new(1000, 200, 80, 150);
    chk(&mut set, "C485-01", b.check(900, 180, 70, 140) == 0xFF, "三线全绿");
    chk(&mut set, "C485-02", b.check(1200, 180, 70, 140) == 0, "跑批越阈定位=0");
    chk(&mut set, "C485-03", b.check(900, 260, 70, 140) == 1, "P95 越阈定位=1");
    chk(&mut set, "C485-04", b.check(900, 180, 90, 140) == 2, "资源超限定位置=2");
    chk(&mut set, "C485-05", b.over_budgets.contains(&3) == false, "无障碍延迟未越阈");

    let r = b.check(900, 180, 70, 200);
    chk(&mut set, "C485-06", r == 3, "无障碍单列越阈独立定位=3");
    chk(&mut set, "C485-07", b.over_budgets.len() == 1, "单列不与其他合并");

    chk(&mut set, "C485-08", b.shard(101, 10) == 11, "分片并行向上取整");
    chk(&mut set, "C485-09", b.shard(100, 10) == 10 && b.shard(100, 0) == 0, "整除与零容量防护");

    for i in 0..20u32 {
        b.samples.push(100 + i);
    }
    chk(&mut set, "C485-10", b.p95() >= 118 && b.p95() <= 119, "P95 高位取值");
    let ds = b.downsample(5);
    chk(&mut set, "C485-11", ds.len() == 4 && ds[0] == 100, "降采样步长 5 → 20/5=4 样本");

    chk(&mut set, "C485-12", b.rebench(800, 180, 300) == Err(PfCode::A11yShrink), "重议不得放宽无障碍上限");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_three_lines() {
        let mut b = PerfBench::new(100, 100, 100, 50);
        assert_eq!(b.check(200, 100, 100, 50), 0);
        assert_eq!(b.check(200, 100, 100, 60), 3);
        assert_eq!(b.rebench(100, 100, 60), Err(PfCode::A11yShrink));
        assert_eq!(b.shard(7, 3), 3);
    }
}
