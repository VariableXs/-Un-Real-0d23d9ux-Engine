//! CGPU-F0442 · 仲裁决策解释器（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 解释器：决策自然语言解释（"为什么它先跑"——每笔仲裁决策生成人话
//!   解释：优先级/预算/保底三因子归因）；解释层级（单决策/整帧摘要
//!   两级）；解释可查（检查器 F0385 集成展示）；解释与审计一致（解释与
//!   F0345 记录同源——不自相矛盾）。判据：三因子、两级、集成、同源、判据。
//!
//! 设计纪律：
//! - 三因子归因：每笔决策拆成 优先级贡献/预算余量贡献/保底下限贡献
//!   三个数值因子，解释文本 = 因子排序第一名的主因 + 完整三行明细
//!   （主因一句话 + 全因子可查——不隐藏次要因子）。
//! - 两级：单决策解释（一笔一行）与整帧摘要（Top-N + 总量归因）。
//! - 同源纪律：解释器不自己算结果——输入就是审计记录（F0345 同构
//!   DecRec），解释只是同一事实的另一呈现（改解释=改记录，一对一）。
//! - 检查器集成：解释行带 F0385 检查器可检索的键（决策号）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0ED0 独占。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0ED0，全仓独占）。
pub const SEG_BASE: u16 = 0x0ED0;

pub const E_EXP_DEC: u16 = SEG_BASE + 1; // 决策记录非法
pub const E_EXP_NEG: u16 = SEG_BASE + 2; // 因子为负（账目脏）
pub const E_EXP_SUM: u16 = SEG_BASE + 3; // 因子和不等于决策值（同源破）

/// 与 F0345 审计同构的决策记录（解释器的事实源——不自算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecRec {
    /// 决策号（检查器检索键）。
    pub id: u64,
    /// 账户。
    pub acct: u32,
    /// 最终裁决（份额千分位）。
    pub verdict_pm: u64,
    /// 优先级贡献。
    pub prio_pm: u64,
    /// 预算余量贡献。
    pub budget_pm: u64,
    /// 保底下限贡献。
    pub floor_pm: u64,
}

impl DecRec {
    fn factors_total(&self) -> u64 {
        self.prio_pm.saturating_add(self.budget_pm).saturating_add(self.floor_pm)
    }
}

/// 解释器。
#[derive(Debug, Default)]
pub struct DecisionExplainer {
    records: Vec<DecRec>,
    explained: u64,
}

impl DecisionExplainer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂审计记录（同源入口——解释器只吃这个）。
    pub fn attach(&mut self, r: DecRec) -> Result<(), u16> {
        if r.factors_total() != r.verdict_pm {
            return Err(E_EXP_SUM);
        }
        if self.records.iter().any(|x| x.id == r.id) {
            return Err(E_EXP_DEC);
        }
        self.records.push(r);
        Ok(())
    }

    /// 单决策解释：主因一句话 + 三因子明细（同源数字原样呈现）。
    pub fn explain_one(&mut self, id: u64) -> Result<String, u16> {
        let r = self.records.iter().copied().find(|x| x.id == id).ok_or(E_EXP_DEC)?;
        let main = if r.prio_pm >= r.budget_pm && r.prio_pm >= r.floor_pm {
            "优先级主导"
        } else if r.budget_pm >= r.floor_pm {
            "预算余量主导"
        } else {
            "保底下限主导"
        };
        self.explained += 1;
        Ok(alloc::format!(
            "决策#{} 账户{} 获 {}‰ —— {}（优先级 {} + 预算 {} + 保底 {}）",
            r.id, r.acct, r.verdict_pm, main, r.prio_pm, r.budget_pm, r.floor_pm
        ))
    }

    /// 整帧摘要：Top-N 主因归因 + 总量守恒行。
    pub fn explain_frame(&self, top_n: usize) -> Result<String, u16> {
        if self.records.is_empty() {
            return Err(E_EXP_DEC);
        }
        let mut sorted = self.records.clone();
        sorted.sort_by(|a, b| b.verdict_pm.cmp(&a.verdict_pm));
        let total: u64 = self.records.iter().map(|r| r.verdict_pm).sum();
        let mut s = alloc::format!("整帧摘要：{} 笔决策合计 {}‰\n", self.records.len(), total);
        for r in sorted.iter().take(top_n) {
            s.push_str(&alloc::format!("  #{} 账户{} {}‰\n", r.id, r.acct, r.verdict_pm));
        }
        Ok(s)
    }

    /// 检查器（F0385）集成口：按决策号取记录（同源直取——解释器是透镜不是账本）。
    pub fn lookup(&self, id: u64) -> Result<DecRec, u16> {
        self.records.iter().copied().find(|x| x.id == id).ok_or(E_EXP_DEC)
    }

    /// 同源一致性：解释文本中的三因子数字必须与记录逐一在场。
    pub fn explain_consistent(text: &str, r: &DecRec) -> bool {
        text.contains(&alloc::format!("{}", r.prio_pm))
            && text.contains(&alloc::format!("{}", r.budget_pm))
            && text.contains(&alloc::format!("{}", r.floor_pm))
            && text.contains(&alloc::format!("{}", r.verdict_pm))
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn explained_count(&self) -> u64 {
        self.explained
    }
}

// ==========================================================================
// 判据（施工书：三因子、两级、集成、同源、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: u64, acct: u32, v: u64, p: u64, b: u64, f: u64) -> DecRec {
        DecRec { id, acct, verdict_pm: v, prio_pm: p, budget_pm: b, floor_pm: f }
    }

    #[test]
    fn a01_三因子归因_主因识别() {
        let mut e = DecisionExplainer::new();
        e.attach(rec(1, 7, 500, 350, 100, 50)).unwrap();
        let t = e.explain_one(1).unwrap();
        assert!(t.contains("优先级主导"));
        // 预算主导。
        e.attach(rec(2, 8, 400, 50, 300, 50)).unwrap();
        assert!(e.explain_one(2).unwrap().contains("预算余量主导"));
        // 保底主导。
        e.attach(rec(3, 9, 300, 20, 30, 250)).unwrap();
        assert!(e.explain_one(3).unwrap().contains("保底下限主导"));
        // 未挂记录的解释拒。
        assert_eq!(e.explain_one(99), Err(E_EXP_DEC));
    }

    #[test]
    fn a02_同源闸_因子和不等于裁决即拒() {
        let mut e = DecisionExplainer::new();
        // 350+100+50 = 500 ✓。
        e.attach(rec(1, 1, 500, 350, 100, 50)).unwrap();
        // 脏账：因子和 600 ≠ 500。
        assert_eq!(e.attach(rec(2, 1, 500, 400, 150, 50)), Err(E_EXP_SUM));
        // 重复决策号拒。
        assert_eq!(e.attach(rec(1, 2, 100, 50, 30, 20)), Err(E_EXP_DEC));
        assert_eq!(e.record_count(), 1);
    }

    #[test]
    fn a03_两级解释_单决策与整帧摘要() {
        let mut e = DecisionExplainer::new();
        for i in 0..5u64 {
            e.attach(rec(i + 1, i as u32 + 1, 200 - i * 10, 100 - i * 20, 50 + i * 10, 50)).unwrap();
        }
        // 单决策级。
        let one = e.explain_one(1).unwrap();
        assert!(one.contains("决策#1"));
        // 整帧摘要级：Top-2。
        let frame = e.explain_frame(2).unwrap();
        assert!(frame.contains("整帧摘要"));
        assert!(frame.contains("5 笔决策合计 900"));
        assert!(frame.contains("#1 账户1 200"));
        assert!(frame.contains("#2 账户2 190"));
        assert!(!frame.contains("#3 账户3 180"));
        // 空记录摘要拒。
        let e2 = DecisionExplainer::new();
        assert_eq!(e2.explain_frame(3), Err(E_EXP_DEC));
    }

    #[test]
    fn a04_同源一致_解释数字与记录对表() {
        let mut e = DecisionExplainer::new();
        let r = rec(1, 7, 500, 350, 100, 50);
        e.attach(r).unwrap();
        let text = e.explain_one(1).unwrap();
        // 四个数字全在场（同源呈现）。
        assert!(DecisionExplainer::explain_consistent(&text, &r));
        // 解释器 lookup 与审计记录逐字段相等（透镜语义）。
        assert_eq!(e.lookup(1), Ok(r));
        assert_eq!(e.lookup(2), Err(E_EXP_DEC));
        // 篡改文本 → 不一致。
        assert!(!DecisionExplainer::explain_consistent("决策#1 获 100‰", &r));
    }

    #[test]
    fn a05_检查器集成_决策号检索() {
        let mut e = DecisionExplainer::new();
        e.attach(rec(42, 3, 250, 100, 100, 50)).unwrap();
        let got = e.lookup(42).unwrap();
        assert_eq!(got.acct, 3);
        assert_eq!(got.verdict_pm, 250);
        // 解释计数推进（可观测）。
        e.explain_one(42).ok();
        assert_eq!(e.explained_count(), 1);
        // 决策号越界检索不炸（返回 Err）。
        assert!(e.lookup(u64::MAX).is_err());
    }
}
