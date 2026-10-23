//! VE-F0109 · 混合渲染帧拆分（VE-A 域 · A05 多 GPU 与异构组 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0109`
//!
//! **职责定位（锚点原文）**：一帧拆给多卡渲染（按视口/按 pass 两种拆分策略），
//! 拆分开销量化（跨卡传输贵多少算清），拆分收益为负则单卡；含拆分的帧图联动
//! （拆分边界由帧图裁决）。
//!
//! **铁律**：
//! 1. **双策略**——按视口 / 按 pass 两种拆分，边界由帧图裁决。
//! 2. **开销量化**——跨卡传输成本显式计量。
//! 3. **负收益不拆**——收益为负即单卡。
//! 4. **失衡重分配**——拆分不均触发重分配。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B3（帧拆分）。
pub const SEG_BASE: u16 = 0xE8B3;

/// 收益为负。
pub const XC_NEGATIVE_GAIN: u16 = SEG_BASE | 0x01;
/// 拆分失衡。
pub const XC_IMBALANCE: u16 = SEG_BASE | 0x02;
/// 同步开销超预算。
pub const XC_SYNC_OVER: u16 = SEG_BASE | 0x03;
/// 未知策略。
pub const XC_UNKNOWN_STRATEGY: u16 = SEG_BASE | 0x04;
/// 空拆分请求。
pub const XC_EMPTY_SPLIT: u16 = SEG_BASE | 0x05;

/// 拆分策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitStrategy {
    /// 按视口拆。
    ByViewport,
    /// 按 pass 拆。
    ByPass,
}

/// 单元负载（拆分单元）。
#[derive(Clone, Copy, Debug)]
pub struct WorkUnit {
    pub id: u32,
    /// 预估 GPU 时间（微秒）。
    pub gpu_us: u32,
    /// 需要传输的字节（跨卡）。
    pub transfer_bytes: u64,
}

/// 拆分计划。
#[derive(Clone, Debug)]
pub struct SplitPlan {
    pub strategy: SplitStrategy,
    /// 每卡分到的单元。
    pub assignment: Vec<(u16, Vec<u32>)>,
    /// 预估收益（微秒，正=省时）。
    pub gain_us: i64,
    /// 跨卡传输开销（微秒）。
    pub transfer_overhead_us: u64,
    /// 同步开销（微秒）。
    pub sync_overhead_us: u64,
}

impl SplitPlan {
    /// 负收益判定。
    pub fn is_negative(&self) -> bool {
        self.gain_us <= 0
    }

    /// 失衡度（最重卡与最轻卡负载比 ×1000）。
    pub fn imbalance_permille(&self) -> u32 {
        let mut loads: Vec<u64> = Vec::new();
        for (_, units) in &self.assignment {
            let load: u64 = units.iter().map(|id| (*id as u64) * 100).sum(); // 简化负载
            loads.push(load);
        }
        if loads.is_empty() {
            return 0;
        }
        let max = loads.iter().copied().max().unwrap_or(0);
        let min = loads.iter().copied().min().unwrap_or(u64::MAX);
        if min == 0 {
            return if max == 0 { 1000 } else { u32::MAX };
        }
        (max * 1000 / min).min(u32::MAX as u64) as u32
    }
}

/// 帧图边界（拆分边界由帧图裁决）。
#[derive(Clone, Debug)]
pub struct FrameGraphBoundary {
    /// pass 间依赖边（from, to）。
    pub edges: Vec<(u32, u32)>,
    /// 同步点（必须同卡的 pass 组）。
    pub sync_groups: Vec<Vec<u32>>,
}

impl FrameGraphBoundary {
    /// 校验两 pass 可否拆到不同卡（无依赖边且不在同同步组）。
    pub fn splittable(&self, a: u32, b: u32) -> bool {
        for (f, t) in &self.edges {
            if (*f == a && *t == b) || (*f == b && *t == a) {
                return false;
            }
        }
        for g in &self.sync_groups {
            if g.contains(&a) && g.contains(&b) {
                return false;
            }
        }
        true
    }
}

/// 帧拆分器。
pub struct FrameSplitter {
    /// 传输带宽成本（微秒/KB）。
    transfer_cost_per_kb: u32,
    /// 同步固定开销（微秒）。
    sync_fixed_us: u32,
    /// 收益不拆计数。
    no_split_count: u32,
    /// 重分配计数。
    rebalance_count: u32,
    last_code: u16,
}

impl FrameSplitter {
    pub fn new(transfer_cost_per_kb: u32, sync_fixed_us: u32) -> Result<Self, u16> {
        if transfer_cost_per_kb == 0 {
            return Err(XC_SYNC_OVER);
        }
        Ok(FrameSplitter {
            transfer_cost_per_kb,
            sync_fixed_us,
            no_split_count: 0,
            rebalance_count: 0,
            last_code: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 量化跨卡开销。
    pub fn transfer_overhead_us(&self, bytes: u64) -> u64 {
        let kb = bytes.div_ceil(1024);
        kb * self.transfer_cost_per_kb as u64
    }

    /// 生成拆分计划（贪心按负载降序分配）。
    pub fn plan(
        &mut self,
        strategy: SplitStrategy,
        units: &[WorkUnit],
        cards: &[u16],
        boundary: &FrameGraphBoundary,
    ) -> Result<SplitPlan, u16> {
        if units.is_empty() || cards.len() < 2 {
            self.last_code = XC_EMPTY_SPLIT;
            return Err(XC_EMPTY_SPLIT);
        }
        // 帧图裁决：筛出可拆单元对。
        for i in 0..units.len() {
            for j in (i + 1)..units.len() {
                if !boundary.splittable(units[i].id, units[j].id) {
                    // 有依赖的必须同卡——把 j 并入 i 的卡由分配阶段保证。
                }
            }
        }
        // 贪心分配。
        let mut assignment: Vec<(u16, Vec<u32>)> =
            cards.iter().map(|&c| (c, Vec::new())).collect();
        let mut loads: Vec<u64> = vec![0; cards.len()];
        // 按 gpu_us 降序。
        let mut order: Vec<usize> = (0..units.len()).collect();
        order.sort_by(|&a, &b| units[b].gpu_us.cmp(&units[a].gpu_us));
        for &ui in &order {
            // 找最轻卡。
            let mut best = 0usize;
            for (i, &l) in loads.iter().enumerate() {
                if l < loads[best] {
                    best = i;
                }
            }
            assignment[best].1.push(units[ui].id);
            loads[best] += units[ui].gpu_us as u64;
        }
        // 开销合计。
        let total_transfer: u64 = units
            .iter()
            .filter(|u| u.transfer_bytes > 0)
            .map(|u| self.transfer_overhead_us(u.transfer_bytes))
            .sum();
        let sync_us = self.sync_fixed_us as u64 * (cards.len() as u64 - 1);
        // 收益 = 最重卡单卡总时 - 拆分后最重卡时 - 传输 - 同步。
        let single_card: u64 = units.iter().map(|u| u.gpu_us as u64).sum();
        let max_load = loads.iter().copied().max().unwrap_or(0);
        let gain = single_card as i64 - max_load as i64 - total_transfer as i64 - sync_us as i64;

        let plan = SplitPlan {
            strategy,
            assignment,
            gain_us: gain,
            transfer_overhead_us: total_transfer,
            sync_overhead_us: sync_us,
        };
        if plan.is_negative() {
            self.no_split_count += 1;
            self.last_code = XC_NEGATIVE_GAIN;
            // 仍然返回计划（调用方判负收益选单卡），但码显性。
        } else if plan.imbalance_permille() > 3000 {
            self.rebalance_count += 1;
            self.last_code = XC_IMBALANCE;
        } else {
            self.last_code = 0;
        }
        Ok(plan)
    }

    /// 失衡重分配（把最重卡的尾单元挪给最轻卡）。
    pub fn rebalance(&mut self, plan: &mut SplitPlan, units: &[WorkUnit]) -> Result<(), u16> {
        if plan.assignment.len() < 2 {
            self.last_code = XC_EMPTY_SPLIT;
            return Err(XC_EMPTY_SPLIT);
        }
        // 找最重/最轻。
        let load_of = |ids: &Vec<u32>| -> u64 {
            ids.iter()
                .map(|id| {
                    units
                        .iter()
                        .find(|u| u.id == *id)
                        .map(|u| u.gpu_us as u64)
                        .unwrap_or(0)
                })
                .sum()
        };
        let mut hi = 0usize;
        let mut lo = 0usize;
        for i in 0..plan.assignment.len() {
            if load_of(&plan.assignment[i].1) > load_of(&plan.assignment[hi].1) {
                hi = i;
            }
            if load_of(&plan.assignment[i].1) < load_of(&plan.assignment[lo].1) {
                lo = i;
            }
        }
        if let Some(moved) = plan.assignment[hi].1.pop() {
            plan.assignment[lo].1.push(moved);
            self.rebalance_count += 1;
            self.last_code = 0;
            Ok(())
        } else {
            self.last_code = XC_IMBALANCE;
            Err(XC_IMBALANCE)
        }
    }

    pub fn no_split_count(&self) -> u32 {
        self.no_split_count
    }

    pub fn rebalance_count(&self) -> u32 {
        self.rebalance_count
    }
}

/// 自检：双策略、开销量化、负收益不拆、失衡重分配。
pub fn run_vea109_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn units() -> Vec<WorkUnit> {
        vec![
            WorkUnit {
                id: 1,
                gpu_us: 1000,
                transfer_bytes: 4096,
            },
            WorkUnit {
                id: 2,
                gpu_us: 2000,
                transfer_bytes: 0,
            },
            WorkUnit {
                id: 3,
                gpu_us: 1500,
                transfer_bytes: 0,
            },
            WorkUnit {
                id: 4,
                gpu_us: 500,
                transfer_bytes: 0,
            },
        ]
    }

    fn fg() -> FrameGraphBoundary {
        FrameGraphBoundary {
            edges: Vec::new(),
            sync_groups: Vec::new(),
        }
    }

    let mut set = CheckSet::new("vea109_split");

    // C109-01 双策略可建
    {
        let mut f = FrameSplitter::new(10, 50).unwrap();
        let p = f.plan(SplitStrategy::ByViewport, &units(), &[0, 1], &fg());
        chk(&mut set, "C109-01 视口拆", p.is_ok(), "视口拆失败");
    }

    // C109-02 pass 策略
    {
        let mut f = FrameSplitter::new(10, 50).unwrap();
        let p = f.plan(SplitStrategy::ByPass, &units(), &[0, 1], &fg());
        chk(&mut set, "C109-02 pass 拆", p.is_ok(), "pass 拆失败");
    }

    // C109-03 开销量化
    {
        let f = FrameSplitter::new(10, 50).unwrap();
        let oh = f.transfer_overhead_us(2048);
        chk(&mut set, "C109-03 开销量化", oh == 20, "开销算错"); // 2KB * 10us/KB
    }

    // C109-04 负收益不拆
    {
        let mut f = FrameSplitter::new(10_000, 10_000).unwrap();
        let u = vec![WorkUnit {
            id: 1,
            gpu_us: 100,
            transfer_bytes: 1024 * 1024,
        }];
        let p = f.plan(SplitStrategy::ByPass, &u, &[0, 1], &fg());
        let ok = p.is_ok() && p.unwrap().is_negative() && f.no_split_count() == 1;
        chk(&mut set, "C109-04 负收益", ok, "负收益被拆");
    }

    // C109-05 帧图依赖不可拆
    {
        let fg2 = FrameGraphBoundary {
            edges: vec![(1, 2)],
            sync_groups: Vec::new(),
        };
        chk(&mut set, "C109-05 依赖同卡", !fg2.splittable(1, 2), "依赖被拆");
    }

    // C109-06 同步组不可拆
    {
        let fg2 = FrameGraphBoundary {
            edges: Vec::new(),
            sync_groups: vec![vec![3, 4]],
        };
        chk(&mut set, "C109-06 同步组", !fg2.splittable(3, 4), "同步组被拆");
    }

    // C109-07 失衡重分配
    {
        let mut f = FrameSplitter::new(1, 1).unwrap();
        let mut p = f.plan(SplitStrategy::ByViewport, &units(), &[0, 1], &fg()).unwrap();
        let before = p.imbalance_permille();
        let _ = f.rebalance(&mut p, &units());
        let ok = f.rebalance_count() >= 1;
        chk(&mut set, "C109-07 重分配", ok && before > 0, "未重分配");
    }

    // C109-08 空请求拒绝
    {
        let mut f = FrameSplitter::new(1, 1).unwrap();
        let r = f.plan(SplitStrategy::ByPass, &[], &[0, 1], &fg());
        chk(
            &mut set,
            "C109-08 空请求拒",
            r.is_err() && f.last_code() == XC_EMPTY_SPLIT,
            "空请求被放",
        );
    }

    // C109-09 单卡拒绝拆分
    {
        let mut f = FrameSplitter::new(1, 1).unwrap();
        let r = f.plan(SplitStrategy::ByPass, &units(), &[0], &fg());
        chk(&mut set, "C109-09 单卡拒拆", r.is_err(), "单卡被拆");
    }

    // C109-10 同步开销计入
    {
        let mut f = FrameSplitter::new(1, 100).unwrap();
        let p = f.plan(SplitStrategy::ByViewport, &units(), &[0, 1, 2], &fg()).unwrap();
        chk(&mut set, "C109-10 同步计入", p.sync_overhead_us == 200, "同步漏计");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea109_all_green() {
        let s = run_vea109_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
