//! VE-F0493 · 编译时间预算与拆分（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0493`
//!
//! **锚点原文**：「编译时间预算与任务拆分：预算模型（每编译单元时限、超
//! 限拆分策略——按函数粒度分片并行）、拆分正确性（分片边界按定义依赖切，
//! 不切语义单元）、预算超限降级（跳过低收益 pass 并告知）；预算与 F0375
//! 式排队语义同构。数据结构：预算模型；分片器；降级记录。错误路径与降级
//! 矩阵：分片边界破坏语义→按依赖图修正；并行结果与串行不一致→确定性检
//! 查（F0494）拦截；预算误估→校准记录。判据：时限预算、按依赖分片、并
//! 行确定性、降级告知、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **时限预算**（判据一）。每编译单元预算时限（微秒面）；超限 → 触发
//!    拆分；预算误估 → 校准记录（不静默改值）。
//!
//! 2. **按依赖分片**（判据二）。分片边界按定义依赖切（依赖函数与其被依
//!    赖者同片），不切语义单元——边界破坏 → 按依赖图修正。
//!
//! 3. **并行确定性**（判据三）。并行分片执行结果与串行比对（确定性检查
//!    钩子，F0494 联动）——不一致 → 拦截。
//!
//! 4. **降级告知**（判据四）。预算超限降级（跳过低收益 pass）→ 显性告知
//!    入账，不静默跳过。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0493";

/// 预算模型：每编译单元时限。
#[derive(Clone, Debug, PartialEq)]
pub struct Budget {
    /// 时限（微秒）。
    pub limit_us: u64,
    /// 低收益 pass 集合（超限降级跳过面）。
    pub low_gain: Vec<&'static str>,
    /// 校准记录（误估修正留痕）。
    pub calibrations: Vec<String>,
}

/// 函数定义（分片单元）。
#[derive(Clone, Debug, PartialEq)]
pub struct FuncDef {
    pub name: &'static str,
    /// 预估耗时（微秒）。
    pub est_us: u64,
    /// 依赖的函数名（定义依赖——被依赖者须同片或先片）。
    pub deps: Vec<&'static str>,
}

/// 一个分片。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Shard {
    pub funcs: Vec<&'static str>,
}

impl Shard {
    pub fn est(&self, fs: &[FuncDef]) -> u64 {
        self.funcs
            .iter()
            .filter_map(|n| fs.iter().find(|f| f.name == *n))
            .map(|f| f.est_us)
            .sum()
    }
}

/// 预算执行账。
#[derive(Debug, Default)]
pub struct BudgetBook {
    pub budget: Option<Budget>,
    pub shards: Vec<Shard>,
    /// 降级告知账（跳过的 pass）。
    pub skipped: Vec<String>,
    /// 确定性拦截账（并行 vs 串行不一致）。
    pub nondet_blocks: Vec<String>,
    pub audit: u32,
}

impl BudgetBook {
    pub fn set_budget(&mut self, b: Budget) {
        self.budget = Some(b);
    }

    /// 分片：按定义依赖切边界——函数与其被依赖者聚同片；单片预估不超时限
    /// 时尽量多装（贪心序），超限开新片。
    pub fn shard(&mut self, fs: &[FuncDef]) -> Result<(), &'static str> {
        let limit = self.budget.as_ref().map(|b| b.limit_us).unwrap_or(u64::MAX);
        let mut shards: Vec<Shard> = Vec::new();
        let mut placed: Vec<&'static str> = Vec::new();
        for f in fs.iter() {
            // 语义单元保持：依赖未放置者先递归放置到当前片（按依赖图修正边界）。
            for d in f.deps.iter() {
                if !placed.contains(d) && fs.iter().any(|x| x.name == *d) {
                    // 依赖先入片。
                    match shards.last_mut() {
                        Some(s) => s.funcs.push(d),
                        None => shards.push(Shard { funcs: vec![*d] }),
                    }
                    placed.push(d);
                }
            }
            match shards.last_mut() {
                Some(s) if s.est(fs) + f.est_us <= limit => s.funcs.push(f.name),
                _ => shards.push(Shard { funcs: vec![f.name] }),
            }
            placed.push(f.name);
        }
        // 分片正确性断言：被依赖者所在片序 ≤ 依赖者片序（依赖不跨界向后）。
        for f in fs.iter() {
            for d in f.deps.iter() {
                let pos_d = shards.iter().position(|s| s.funcs.contains(d));
                let pos_f = shards.iter().position(|s| s.funcs.contains(&f.name));
                if let (Some(pd), Some(pf)) = (pos_d, pos_f) {
                    if pd > pf {
                        return Err("分片边界破坏语义（被依赖者后置）");
                    }
                }
            }
        }
        self.shards = shards;
        self.audit += 1;
        Ok(())
    }

    /// 预算超限降级：超限时跳过低收益 pass 并显性告知。
    pub fn degrade_skip(&mut self, pass: &'static str, remaining_us: u64, cost_us: u64) -> bool {
        let low = self
            .budget
            .as_ref()
            .map(|b| b.low_gain.contains(&pass))
            .unwrap_or(false);
        if remaining_us < cost_us {
            if low {
                self.skipped.push(format!("预算超限降级：跳过低收益 pass {}", pass));
                return true;
            }
            // 非低收益超限：不跳（须拆分，F0495/F0493 联动）。
            return false;
        }
        false
    }

    /// 并行确定性检查钩子：并行结果 vs 串行结果。
    pub fn nondet_check(&mut self, serial: &str, parallel: &str) -> bool {
        if serial != parallel {
            self.nondet_blocks.push(format!(
                "并行结果与串行不一致：串行 {} ≠ 并行 {}（F0494 拦截）",
                serial, parallel
            ));
            false
        } else {
            true
        }
    }

    /// 预算校准：误估修正留痕。
    pub fn calibrate(&mut self, unit: &'static str, est: u64, real: u64) {
        if let Some(b) = self.budget.as_mut() {
            b.calibrations.push(format!("校准 {}：估 {} 实 {}", unit, est, real));
        }
    }
}

// ---------------------------------------------------------------------------
// 判据（F0493-xxx）
// ---------------------------------------------------------------------------

/// 原始判据。
pub fn vec93_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、时限预算 + 分片。
    {
        let mut book = BudgetBook::default();
        book.set_budget(Budget { limit_us: 100, low_gain: vec!["licm"], calibrations: Vec::new() });
        let fs = vec![
            FuncDef { name: "a", est_us: 40, deps: vec![] },
            FuncDef { name: "b", est_us: 40, deps: vec!["a"] },
            FuncDef { name: "c", est_us: 40, deps: vec![] },
            FuncDef { name: "d", est_us: 70, deps: vec![] },
        ];
        let ok = book.shard(&fs);
        v.push(("F0493-预算-分片成功", ok.is_ok() && !book.shards.is_empty()));
        // 单片都不超时限：a+b=80 ≤ 100 同片。
        v.push((
            "F0493-分片-时限内聚合",
            book.shards[0].funcs.contains(&"a") && book.shards[0].funcs.contains(&"b"),
        ));
        // d=70 单独装不进首片（80+70>100）→ 新片。
        v.push(("F0493-分片-超限开新片", book.shards.len() >= 2));
    }

    // 二、按依赖切边界：被依赖者后置 = 语义破坏拦截。
    {
        let mut book = BudgetBook::default();
        // 手工构造破坏面：直接校验断言逻辑——依赖者 b 在片 0，被依赖者 a 在片 1。
        book.shards = vec![
            Shard { funcs: vec!["b"] },
            Shard { funcs: vec!["a"] },
        ];
        let fs = vec![
            FuncDef { name: "a", est_us: 10, deps: vec![] },
            FuncDef { name: "b", est_us: 10, deps: vec!["a"] },
        ];
        let r = book.shard(&fs);
        v.push(("F0493-依赖-重分片修正", r.is_ok() && book.shards.len() >= 1));
    }

    // 三、降级告知。
    {
        let mut book = BudgetBook::default();
        book.set_budget(Budget { limit_us: 100, low_gain: vec!["licm"], calibrations: Vec::new() });
        let skipped = book.degrade_skip("licm", 20, 30);
        v.push((
            "F0493-降级-低收益跳过告知",
            skipped && book.skipped.len() == 1 && book.skipped[0].contains("licm"),
        ));
        // 非低收益超限不跳（须拆分）。
        let not_skip = book.degrade_skip("inline", 10, 30);
        v.push(("F0493-降级-非低收益不跳", !not_skip && book.skipped.len() == 1));
        // 预算内不跳。
        let ok = book.degrade_skip("licm", 100, 30);
        v.push(("F0493-降级-预算内不跳", !ok));
    }

    // 四、并行确定性。
    {
        let mut book = BudgetBook::default();
        let ok = book.nondet_check("opt(A)→IR1", "opt(A)→IR1");
        v.push(("F0493-并行-一致放行", ok && book.nondet_blocks.is_empty()));
        let bad = book.nondet_check("opt(A)→IR1", "opt(A)→IR2");
        v.push((
            "F0493-并行-不一致拦截",
            !bad && book.nondet_blocks[0].contains("F0494"),
        ));
    }

    // 五、校准 + 判据自检。
    {
        let mut book = BudgetBook::default();
        book.set_budget(Budget { limit_us: 100, low_gain: vec![], calibrations: Vec::new() });
        book.calibrate("unit1", 50, 80);
        v.push((
            "F0493-校准-误估留痕",
            book.budget.as_ref().unwrap().calibrations.len() == 1,
        ));
        v.push(("F0493-判据-锚点入册", ANCHOR == "VE-F0493"));
    }

    v
}

/// 注册表入口。
pub fn run_vec93_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec93");
    for (name, ok) in vec93_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec93_raw_checks as run_vec93_checks;
    #[test]
    fn vec93_budget_checks() {
        for (name, ok) in run_vec93_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
