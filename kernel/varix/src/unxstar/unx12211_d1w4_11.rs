//! UNX-F12211 展开器性能预算与压测（SEG 0x64160..0x64180 段窗）
//!
//! 卡面：展开器性能预算账——单跑成本上限、总预算耗尽拒止、压测
//! 批次记账、利用率口径；超支与坏成本拒止全链过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 单跑记账：成本入账累计。
//! - J2 零成本拒止：cost=0 必拒 PB_BAD_COST。
//! - J3 单跑上限拒止：cost 超单跑上限 64 必拒 PB_LIMIT。
//! - J4 预算耗尽拒止：余量不足必拒 PB_OVER。
//! - J5 压测批次：批量记账返回完成笔数。
//! - J6 压测恒等：压测后 spent = Σcost。
//! - J7 余量口径：budget-spent 恒一致。
//! - J8 利用率口径：spent*100/budget 可回读。
//! - J9 恰好贴界：总耗恰等预算放行，再超一拒。
//! - J10 印账冻结：seal 后记账必拒 PB_FROZEN。
//! - J11 指纹幂等：同态同指纹。
//! - J12 注入五况全抓。
//!
//! 结构：PerfBudget 预算账（总预算/已耗/跑次账/操作账）+ 记账核
//! （record_run）+ 压测核（stress）+ 折叠指纹 + 注入核（零成本/超
//! 单跑限/超预算/压测超支/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64160 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x64160;
pub const PB_OVER: u32 = SEG_BASE + 0x01; // 预算耗尽
pub const PB_BAD_COST: u32 = SEG_BASE + 0x02; // 零成本
pub const PB_LIMIT: u32 = SEG_BASE + 0x03; // 超单跑上限
pub const PB_FROZEN: u32 = SEG_BASE + 0x04; // 印账后操作
pub const PB_NO_RUNS: u32 = SEG_BASE + 0x05; // 空压测

/// 单跑成本上限。
pub const RUN_LIMIT: u32 = 64;

/// 预算账。
pub struct PerfBudget {
    pub budget: u32,
    pub spent: u32,
    pub runs: Vec<u32>,
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl PerfBudget {
    pub fn new(budget: u32) -> PerfBudget {
        PerfBudget { budget, spent: 0, runs: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 余量。
    pub fn left(&self) -> u32 {
        self.budget - self.spent
    }

    /// 记账核。
    pub fn record_run(&mut self, cost: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PB_FROZEN);
        }
        if cost == 0 {
            return Err(PB_BAD_COST);
        }
        if cost > RUN_LIMIT {
            return Err(PB_LIMIT);
        }
        if cost > self.left() {
            return Err(PB_OVER);
        }
        self.spent += cost;
        self.runs.push(cost);
        self.ops.push(format!("run{}", cost));
        Ok(())
    }

    /// 压测核：逐笔记账，任一拒即停，返回完成笔数。
    pub fn stress(&mut self, costs: Vec<u32>) -> Result<usize, u32> {
        if self.sealed {
            return Err(PB_FROZEN);
        }
        if costs.is_empty() {
            return Err(PB_NO_RUNS);
        }
        let mut done = 0usize;
        for c in costs {
            match self.record_run(c) {
                Ok(()) => done += 1,
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    /// 利用率（百分比，向下取整）。
    pub fn utilization(&self) -> u32 {
        if self.budget == 0 {
            return 0;
        }
        self.spent * 100 / self.budget
    }

    /// 跑次账条数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.budget);
        h = h.wrapping_mul(211).wrapping_add(self.spent);
        for r in &self.runs {
            h = h.wrapping_mul(211).wrapping_add(*r);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("b={} s={} n={}", self.budget, self.spent, self.run_count())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零成本必拒。
    got[0] = PerfBudget::new(100).record_run(0).err() == Some(PB_BAD_COST);
    // 2) 超单跑上限必拒。
    got[1] = PerfBudget::new(100).record_run(65).err() == Some(PB_LIMIT);
    // 3) 预算耗尽必拒。
    let mut b = PerfBudget::new(10);
    let _ = b.record_run(8);
    got[2] = b.record_run(8).err() == Some(PB_OVER);
    // 4) 压测中途超支必拒。
    let mut b = PerfBudget::new(10);
    got[3] = b.stress(vec![4, 4, 4]) == Err(PB_OVER) && b.run_count() == 2;
    // 5) 印账后操作必抓。
    let mut f = PerfBudget::new(100);
    f.seal();
    got[4] = f.record_run(1).err() == Some(PB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12211_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12211");

    // J1 单跑记账。
    let j1 = {
        let mut b = PerfBudget::new(100);
        b.record_run(10).is_ok() && b.spent == 10 && b.run_count() == 1
    };
    s.add("J1-单跑记账", j1, "成本累计入账");

    // J2 零成本拒止。
    let j2 = PerfBudget::new(100).record_run(0).err() == Some(PB_BAD_COST);
    s.add("J2-零成本拒止", j2, "cost=0 必拒");

    // J3 单跑上限拒止。
    let j3 = {
        let lim_ok = PerfBudget::new(100).record_run(RUN_LIMIT).is_ok();
        lim_ok && PerfBudget::new(100).record_run(RUN_LIMIT + 1).err() == Some(PB_LIMIT)
    };
    s.add("J3-单跑上限拒止", j3, "恰界放行、越界拒");

    // J4 预算耗尽拒止。
    let j4 = {
        let mut b = PerfBudget::new(20);
        let _ = b.record_run(20);
        b.record_run(1).err() == Some(PB_OVER)
    };
    s.add("J4-预算耗尽拒止", j4, "余量 0 后必拒");

    // J5 压测批次。
    let j5 = {
        let mut b = PerfBudget::new(100);
        b.stress(vec![8, 8, 8, 8]) == Ok(4)
    };
    s.add("J5-压测批次", j5, "四笔全成");

    // J6 压测恒等。
    let j6 = {
        let mut b = PerfBudget::new(100);
        let _ = b.stress(vec![10, 20, 30]);
        b.spent == 60 && b.run_count() == 3
    };
    s.add("J6-压测恒等", j6, "spent=Σcost");

    // J7 余量口径。
    let j7 = {
        let mut b = PerfBudget::new(50);
        let _ = b.record_run(30);
        b.left() == 20
    };
    s.add("J7-余量口径", j7, "budget-spent 一致");

    // J8 利用率口径。
    let j8 = {
        let mut b = PerfBudget::new(200);
        let _ = b.record_run(50);
        b.utilization() == 25
    };
    s.add("J8-利用率口径", j8, "25% 回读");

    // J9 恰好贴界。
    let j9 = {
        let mut b = PerfBudget::new(30);
        let a = b.record_run(15).is_ok();
        let c = b.record_run(15).is_ok();
        let d = b.record_run(1).err() == Some(PB_OVER);
        a && c && d
    };
    s.add("J9-恰好贴界", j9, "贴界放行、超一拒");

    // J10 印账冻结。
    let j10 = {
        let mut b = PerfBudget::new(100);
        let _ = b.record_run(1);
        b.seal();
        b.record_run(1).err() == Some(PB_FROZEN)
            && b.stress(vec![1]).err() == Some(PB_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后记账/压测皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = PerfBudget::new(100);
        let _ = b.record_run(7);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 空压测拒止。
    let j12 = PerfBudget::new(100).stress(Vec::new()).err() == Some(PB_NO_RUNS);
    s.add("J12-空压测拒止", j12, "无笔压测必拒");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-摘要回读", {
        let mut b = PerfBudget::new(64);
        let _ = b.record_run(64);
        b.summary() == "b=64 s=64 n=1"
    }, "预算/已耗/跑次回读");
    s.add("J15-指纹非零", {
        let mut b = PerfBudget::new(100);
        let _ = b.record_run(3);
        b.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12211_budget_flow() {
        let mut b = PerfBudget::new(100);
        assert!(b.record_run(30).is_ok());
        assert_eq!(b.record_run(0), Err(PB_BAD_COST));
        assert_eq!(b.record_run(65), Err(PB_LIMIT));
        assert_eq!(b.stress(vec![70]), Err(PB_OVER));
        assert_eq!(b.left(), 70);
    }

    #[test]
    fn unx12211_checks_green() {
        let s = run_unx12211_checks();
        assert!(s.all_passed(), "UNX-F12211 判据未全绿");
    }

    #[test]
    fn unx12211_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
