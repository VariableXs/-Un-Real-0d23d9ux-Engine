//! VE-F0736 曲线性能基准
//! 职责：求值/弧长/分割/求交四路径的成本计数器、基准档位（冒烟/标准/深档）、
//!       预算判定（每路径微秒当量上限）、基准结果入册。
//! 契约：档位升级不改变相对排序；预算只做判定不做硬等待。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 成本计数器（伪时钟：按操作复杂度计权）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cost {
    pub ops: u64,
}

impl Cost {
    pub fn new() -> Cost {
        Cost { ops: 0 }
    }
    pub fn add(&mut self, n: u64) {
        self.ops += n;
    }
    pub fn merge(&mut self, o: Cost) {
        self.ops += o.ops;
    }
}

impl Default for Cost {
    fn default() -> Self {
        Cost::new()
    }
}

/// 求值成本 = 阶数 + 1 次乘加
pub fn cost_eval(c: &Curve, n: usize) -> Cost {
    Cost { ops: (c.deg.n() + 1) as u64 * n as u64 }
}

/// Casteljau 求值成本 = n(n+1)/2 次插值
pub fn cost_casteljau(c: &Curve, n: usize) -> Cost {
    let k = c.ctrl.len();
    Cost { ops: (k * (k + 1) / 2) as u64 * n as u64 }
}

/// 弧长成本（segs 段高斯 3 点）= segs × (3 求值 + 常数)
pub fn cost_arclen(c: &Curve, segs: usize) -> Cost {
    Cost { ops: segs as u64 * 3 * (c.deg.n() + 1) as u64 }
}

/// 分割成本 = 控制点数²/2
pub fn cost_split(c: &Curve) -> Cost {
    let k = c.ctrl.len();
    Cost { ops: (k * k / 2) as u64 }
}

/// 基准档位
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BenchTier {
    Smoke,
    Standard,
    Deep,
}

impl BenchTier {
    pub fn rounds(self) -> usize {
        match self {
            BenchTier::Smoke => 64,
            BenchTier::Standard => 1024,
            BenchTier::Deep => 16384,
        }
    }
    pub fn segs(self) -> usize {
        match self {
            BenchTier::Smoke => 4,
            BenchTier::Standard => 16,
            BenchTier::Deep => 64,
        }
    }
}

/// 预算判定（每样本操作当量上限）
pub const BUDGET_EVAL: u64 = 64;
pub const BUDGET_ARCLEN: u64 = 4096;

pub fn within_budget_eval(c: &Curve, tier: BenchTier) -> bool {
    cost_eval(c, tier.rounds()).ops / tier.rounds() as u64 <= BUDGET_EVAL
}

pub fn within_budget_arclen(c: &Curve, tier: BenchTier) -> bool {
    cost_arclen(c, tier.segs()).ops <= BUDGET_ARCLEN
}

/// 基准记录（入册行）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BenchRow {
    pub name: &'static str,
    pub tier: BenchTier,
    pub ops_per_sample: u64,
}

/// 跑基准：返回入册行
pub fn run_bench(c: &Curve, tier: BenchTier, name: &'static str) -> BenchRow {
    let n = tier.rounds();
    let eval_c = cost_eval(c, n);
    let cast_c = cost_casteljau(c, n);
    let total = eval_c;
    let mut m = Cost::new();
    m.merge(total);
    m.merge(cast_c);
    BenchRow { name, tier, ops_per_sample: m.ops / n as u64 }
}

/// 档位升级保序：Deep 的单样本成本不高于 Smoke 的多档权（相同算法）→ 单样本成本恒定
pub fn tier_stable(c: &Curve) -> bool {
    let a = run_bench(c, BenchTier::Smoke, "a");
    let b = run_bench(c, BenchTier::Deep, "b");
    a.ops_per_sample == b.ops_per_sample
}

pub fn run_vec336_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec336");
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    // 族一：成本模型
    s.add("成本-求值", cost_eval(&c, 100).ops == 400, "4×100");
    s.add("成本-CAST", cost_casteljau(&c, 1).ops == 10, "4·5/2");
    s.add("成本-弧长", cost_arclen(&c, 8).ops == 8 * 3 * 4, "8 segs × 3 × 4");
    s.add("成本-分割", cost_split(&c).ops == 8, "4²/2");
    // 零样本
    s.add("成本-零样本", cost_eval(&c, 0).ops == 0, "zero rounds");
    // 计数器
    let mut m = Cost::new();
    m.add(10);
    m.add(15);
    s.add("计数-累加", m.ops == 25, "10+15");
    m.merge(cost_eval(&c, 5));
    s.add("计数-合并", m.ops == 45, "+20");
    // 族二：档位
    s.add("档位-轮数", BenchTier::Smoke.rounds() == 64 && BenchTier::Deep.rounds() == 16384, "rounds");
    s.add("档位-段数", BenchTier::Smoke.segs() == 4 && BenchTier::Deep.segs() == 64, "segs");
    s.add("档位-有序", BenchTier::Smoke < BenchTier::Standard && BenchTier::Standard < BenchTier::Deep, "ordering");
    // 族三：预算
    s.add("预算-求值过", within_budget_eval(&c, BenchTier::Deep), "4 ≤ 64");
    s.add("预算-弧长过", within_budget_arclen(&c, BenchTier::Deep), "768 ≤ 4096");
    // 高阶曲线弧长预算
    let big = Curve::build(Deg::Quart, alloc::vec![Pt::ZERO; 5]).is_none(); // 5 pts invalid for Quart? n_ctrl=5 ✓
    let _ = big;
    let q4 = Curve::build(
        Deg::Quart,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 4.0), Pt::new(2.0, -4.0), Pt::new(3.0, 4.0), Pt::new(4.0, 0.0)],
    )
    .expect("q4");
    s.add("预算-四次弧长", within_budget_arclen(&q4, BenchTier::Deep), "5×3×64=960 ≤ 4096");
    s.add("预算-冒烟档", within_budget_arclen(&q4, BenchTier::Smoke), "smoke segs cheap");
    // 族四：基准记录
    let row = run_bench(&c, BenchTier::Standard, "cube-eval");
    s.add("记录-名称", row.name == "cube-eval", "name kept");
    s.add("记录-档位", row.tier == BenchTier::Standard, "tier kept");
    s.add("记录-单样本", row.ops_per_sample == 14, "(4+10)/1");
    // 族五：档位稳定
    s.add("稳定-单样本恒定", tier_stable(&c), "per-sample cost constant");
    // 族六：多次运行确定性（成本是解析模型，非测量——完全确定）
    let r1 = run_bench(&c, BenchTier::Smoke, "x");
    let r2 = run_bench(&c, BenchTier::Smoke, "x");
    s.add("确定-重跑同值", r1 == r2, "deterministic");
    // 族七：线性曲线更便宜
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::ZERO, Pt::new(4.0, 4.0)]).expect("ln");
    s.add("线性-更便宜", cost_eval(&ln, 100).ops < cost_eval(&c, 100).ops, "line < cube");
    s
}
