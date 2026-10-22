//! CGPU-F0443 · 多仲裁器协同预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 协同预留：多引擎实例场景（多 GPU/多引擎实例的仲裁协同——全局公平
//!   的协调者接口预留）；协同协议预留（实例间预算分区/借还协议概念
//!   设计）；诚实标注（一期单仲裁器、协同为预留——不假装可用）。判据：
//!   协调者接口、协议预留、诚实标注、判据。
//!
//! 设计纪律：
//! - 协调者接口预留：`Coordinator` trait（预算分区申报/借还协商两个
//!   方法）——接口定义与类型检查可用，语义实现为预留。
//! - 协议概念设计落地为数据结构：PartitionPlan（实例分区表——Σ分区
//!   ≤ 全局预算的守恒预检）与 LendNote（借还单——出借方/借入方/额度
//!   三要素），一期只做「结构合法 + 守恒预检」，不做实例间通信。
//! - 诚实标注是枚举字段（CoopReserved）不是注释：任何试图在一期
//!   "启用协同"的调用（`enable_coop`）一律 E_COO_RESERVED。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0EE0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0EE0，全仓独占）。
pub const SEG_BASE: u16 = 0x0EE0;

pub const E_COO_PART: u16 = SEG_BASE + 1; // 分区非法（Σ 超全局预算）
pub const E_COO_LEND: u16 = SEG_BASE + 2; // 借还单非法（额度/越权出借）
pub const E_COO_RESERVED: u16 = SEG_BASE + 3; // 一期预留：协同未启用
pub const E_COO_ID: u16 = SEG_BASE + 4; // 实例号非法（重复）

/// 协同成熟度（诚实标注枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoopStage {
    /// 一期：单仲裁器（本地仲裁完整可用）。
    SingleLocal,
    /// 预留：协同接口与协议结构就绪，语义未实现。
    CoopReserved,
}

/// 分区申报（实例 → 预算份额）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub inst: u32,
    pub budget_pm: u64,
}

/// 分区计划（协议概念设计①）。
#[derive(Clone, Debug, Default)]
pub struct PartitionPlan {
    pub parts: Vec<Partition>,
    /// 全局预算（千分位）。
    pub global_pm: u64,
}

impl PartitionPlan {
    /// 守恒预检：Σ 分区 ≤ 全局（协同不创造预算）。
    pub fn conserves(&self) -> bool {
        let s: u64 = self.parts.iter().map(|p| p.budget_pm).sum();
        s <= self.global_pm
    }

    /// 实例号重复拒（协同身份唯一）。
    pub fn ids_unique(&self) -> bool {
        for (i, a) in self.parts.iter().enumerate() {
            if self.parts.iter().skip(i + 1).any(|b| b.inst == a.inst) {
                return false;
            }
        }
        true
    }

    /// 提交计划：守恒 + 身份唯一双检。
    pub fn submit(&self) -> Result<(), u16> {
        if !self.ids_unique() {
            return Err(E_COO_ID);
        }
        if !self.conserves() {
            return Err(E_COO_PART);
        }
        Ok(())
    }
}

/// 借还单（协议概念设计②）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LendNote {
    /// 出借实例。
    pub from: u32,
    /// 借入实例。
    pub to: u32,
    /// 额度（千分位）。
    pub amount_pm: u64,
}

/// 协调者（接口预留：分区/借还协商）。
#[derive(Debug)]
pub struct Coordinator {
    pub stage: CoopStage,
    plan: Option<PartitionPlan>,
    notes: Vec<LendNote>,
    /// 各实例剩余预算（千分位）。
    balances: Vec<(u32, u64)>,
}

impl Coordinator {
    pub fn new(stage: CoopStage) -> Self {
        Self {
            stage,
            plan: None,
            notes: Vec::new(),
            balances: Vec::new(),
        }
    }

    /// 一期诚实标注防线：试图在一期启用协同语义 → 恒拒。
    pub fn enable_coop(&self) -> Result<(), u16> {
        Err(E_COO_RESERVED)
    }

    /// 提交分区计划（一期允许：结构预检可用——它只是本地数据校验）。
    pub fn submit_plan(&mut self, plan: PartitionPlan) -> Result<(), u16> {
        plan.submit()?;
        self.plan = Some(plan);
        Ok(())
    }

    /// 按分区实例化余额表（分区份额 → 各实例预算）。
    pub fn activate_balances(&mut self) -> Result<usize, u16> {
        let plan = self.plan.as_ref().ok_or(E_COO_PART)?;
        self.balances = plan.parts.iter().map(|p| (p.inst, p.budget_pm)).collect();
        Ok(self.balances.len())
    }

    /// 借还单录入：出借方须有余额、借入方须在册、自借拒。
    pub fn submit_lend(&mut self, note: LendNote) -> Result<(), u16> {
        if note.from == note.to || note.amount_pm == 0 {
            return Err(E_COO_LEND);
        }
        let to_ok = self.balances.iter().any(|(i, _)| *i == note.to);
        if !to_ok {
            return Err(E_COO_LEND);
        }
        let from_ok = {
            let b = self
                .balances
                .iter_mut()
                .find(|(i, _)| *i == note.from)
                .ok_or(E_COO_LEND)?;
            if b.1 < note.amount_pm {
                return Err(E_COO_LEND); // 越权出借（余额不足）。
            }
            b.1 -= note.amount_pm;
            true
        };
        if !from_ok {
            return Err(E_COO_LEND);
        }
        if let Some((_, b)) = self.balances.iter_mut().find(|(i, _)| *i == note.to) {
            *b = b.saturating_add(note.amount_pm);
        }
        self.notes.push(note);
        Ok(())
    }

    /// 借还守恒：Σ 余额不变（借还只搬运不创造）。
    pub fn balances_total(&self) -> u64 {
        self.balances.iter().map(|(_, b)| *b).sum()
    }

    pub fn plan_parts(&self) -> usize {
        self.plan.as_ref().map(|p| p.parts.len()).unwrap_or(0)
    }

    pub fn lend_count(&self) -> usize {
        self.notes.len()
    }
}

// ==========================================================================
// 判据（施工书：协调者接口、协议预留、诚实标注、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_分区计划_守恒预检() {
        let good = PartitionPlan {
            parts: vec![
                Partition { inst: 0, budget_pm: 400 },
                Partition { inst: 1, budget_pm: 350 },
                Partition { inst: 2, budget_pm: 250 },
            ],
            global_pm: 1000,
        };
        assert!(good.conserves());
        assert!(good.ids_unique());
        assert_eq!(good.submit(), Ok(()));
        // 超分拒（协同不创造预算）。
        let over = PartitionPlan {
            parts: vec![
                Partition { inst: 0, budget_pm: 600 },
                Partition { inst: 1, budget_pm: 600 },
            ],
            global_pm: 1000,
        };
        assert!(!over.conserves());
        assert_eq!(over.submit(), Err(E_COO_PART));
        // 实例号重复拒。
        let dup = PartitionPlan {
            parts: vec![
                Partition { inst: 0, budget_pm: 500 },
                Partition { inst: 0, budget_pm: 300 },
            ],
            global_pm: 1000,
        };
        assert_eq!(dup.submit(), Err(E_COO_ID));
    }

    #[test]
    fn a02_借还单_守恒搬运() {
        let mut c = Coordinator::new(CoopStage::SingleLocal);
        c.submit_plan(PartitionPlan {
            parts: vec![
                Partition { inst: 0, budget_pm: 500 },
                Partition { inst: 1, budget_pm: 500 },
            ],
            global_pm: 1000,
        })
        .unwrap();
        assert_eq!(c.activate_balances(), Ok(2));
        let before = c.balances_total();
        // 借 100：出借方 500→400，借入方 500→600，总额守恒。
        c.submit_lend(LendNote { from: 0, to: 1, amount_pm: 100 }).unwrap();
        assert_eq!(c.balances_total(), before);
        assert_eq!(c.lend_count(), 1);
        // 自借拒。
        assert_eq!(
            c.submit_lend(LendNote { from: 0, to: 0, amount_pm: 10 }),
            Err(E_COO_LEND)
        );
        // 越权出借拒。
        assert_eq!(
            c.submit_lend(LendNote { from: 0, to: 1, amount_pm: 401 }),
            Err(E_COO_LEND)
        );
        // 未知实例拒。
        assert_eq!(
            c.submit_lend(LendNote { from: 9, to: 1, amount_pm: 10 }),
            Err(E_COO_LEND)
        );
        assert_eq!(c.lend_count(), 1);
    }

    #[test]
    fn a03_诚实标注_一期协同恒拒() {
        let c = Coordinator::new(CoopStage::SingleLocal);
        // 一期试图启用协同 → E_COO_RESERVED（不假装可用）。
        assert_eq!(c.enable_coop(), Err(E_COO_RESERVED));
        // 成熟度枚举可判别。
        assert_eq!(c.stage, CoopStage::SingleLocal);
        let r = Coordinator::new(CoopStage::CoopReserved);
        assert_eq!(r.stage, CoopStage::CoopReserved);
        // 预留态同样拒（二期工具就绪前不开放）。
        assert_eq!(r.enable_coop(), Err(E_COO_RESERVED));
    }

    #[test]
    fn a04_未分区先借还拒() {
        let mut c = Coordinator::new(CoopStage::SingleLocal);
        // 未 submit_plan → activate 拒 → 借还无账可动。
        assert_eq!(
            c.submit_lend(LendNote { from: 0, to: 1, amount_pm: 1 }),
            Err(E_COO_LEND)
        );
        assert_eq!(c.activate_balances(), Err(E_COO_PART));
        assert_eq!(c.plan_parts(), 0);
        // 分区后正常。
        c.submit_plan(PartitionPlan {
            parts: vec![Partition { inst: 0, budget_pm: 1000 }],
            global_pm: 1000,
        })
        .unwrap();
        assert_eq!(c.activate_balances(), Ok(1));
        assert_eq!(c.plan_parts(), 1);
    }

    #[test]
    fn a05_借还链_多次搬运仍守恒() {
        let mut c = Coordinator::new(CoopStage::SingleLocal);
        c.submit_plan(PartitionPlan {
            parts: vec![
                Partition { inst: 0, budget_pm: 400 },
                Partition { inst: 1, budget_pm: 300 },
                Partition { inst: 2, budget_pm: 300 },
            ],
            global_pm: 1000,
        })
        .unwrap();
        c.activate_balances().unwrap();
        let t0 = c.balances_total();
        // 链式搬运：0→1→2→0 各 100。
        for (f, t) in [(0, 1), (1, 2), (2, 0)] {
            c.submit_lend(LendNote { from: f, to: t, amount_pm: 100 }).unwrap();
        }
        assert_eq!(c.balances_total(), t0);
        assert_eq!(c.lend_count(), 3);
        // 各实例余额循环回归原位。
        assert_eq!(c.balances, vec![(0, 400), (1, 300), (2, 300)]);
    }
}
