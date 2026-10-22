//! CGPU-F0438 · 仲裁形式化验证接口（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 形式化：状态机导出接口（仲裁器状态机/不变量导出为形式化工具可读
//!   格式——模型检验预留）；不变量声明形式化（守恒/无饥饿/无超卖的
//!   形式化表达——供工具核验）；诚实标注（一期导出接口、形式化证明为
//!   预留——不假装已证明）；导出与实现一致性校验。判据：导出接口、
//!   形式化表达、诚实标注、一致性、判据。
//!
//! 设计纪律：
//! - 导出格式：SMT-LIB 风格的声明文本（常量/函数/断言三段）——
//!   模型检验工具可读（预留接口的"可读"必须有格式承诺）。
//! - 三不变量形式化表达：守恒（Σ分配 ≤ Σ预算）、无饥饿（每账户
//!   最长等待有界）、无超卖（单窗份额 ≤ 容量）。每条不变量带
//!   状态（verified_by_tool=false——诚实：声明≠证明）。
//! - 诚实标注是字段不是注释：`FormalStatus::DeclaredOnly` 进结构体，
//!   任何"已证明"口径必须改字段+过工具核验（一期无工具 → 恒 DeclaredOnly）。
//! - 一致性校验：导出文本中的账户/预算数值与实现快照逐项对表
//!   （导出的是快照的影子——影子漂移即红）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E90 独占。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E90，全仓独占）。
pub const SEG_BASE: u16 = 0x0E90;

pub const E_FRM_EMPTY: u16 = SEG_BASE + 1; // 空状态机导出（无账户）
pub const E_FRM_DRIFT: u16 = SEG_BASE + 2; // 导出与实现漂移
pub const E_FRM_INV: u16 = SEG_BASE + 3; // 不变量表达非法（空表达）
pub const E_FRM_HONEST: u16 = SEG_BASE + 4; // 诚实标注违规（未证明冒充已证明）

/// 形式化状态（诚实标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormalStatus {
    /// 仅声明（一期常态——工具核验预留）。
    DeclaredOnly,
    /// 已被工具核验（预留态——本期不应出现）。
    ToolVerified,
}

/// 不变量条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invariant {
    /// 不变量名（conservation/no-starvation/no-oversell）。
    pub name: &'static str,
    /// 形式化表达（SMT-LIB 风格断言文本）。
    pub expr: String,
    /// 诚实标注。
    pub status: FormalStatus,
}

/// 仲裁器状态快照（导出的事实源）。
#[derive(Clone, Debug, Default)]
pub struct ArbSnapshot {
    /// 账户预算（千分位）。
    pub budgets: Vec<(u32, u64)>,
    /// 本窗已分配（千分位）。
    pub granted: Vec<(u32, u64)>,
    /// 最长等待 tick（无饥饿的观测面）。
    pub max_wait: u64,
    /// 等待上界承诺。
    pub wait_bound: u64,
    /// 窗口容量（千分位）。
    pub capacity_pm: u64,
}

impl ArbSnapshot {
    /// 守恒自检：Σ granted ≤ Σ budgets。
    pub fn conservation_ok(&self) -> bool {
        let g: u64 = self.granted.iter().map(|(_, v)| *v).sum();
        let b: u64 = self.budgets.iter().map(|(_, v)| *v).sum();
        g <= b
    }

    /// 无饥饿自检：最长等待 ≤ 承诺上界。
    pub fn no_starvation_ok(&self) -> bool {
        self.max_wait <= self.wait_bound
    }

    /// 无超卖自检：Σ granted ≤ 容量。
    pub fn no_oversell_ok(&self) -> bool {
        let g: u64 = self.granted.iter().map(|(_, v)| *v).sum();
        g <= self.capacity_pm
    }
}

/// 形式化导出器。
#[derive(Debug, Default)]
pub struct FormalExporter {
    invariants: Vec<Invariant>,
    exports: u64,
}

impl FormalExporter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 三不变量声明（表达式由快照参数化——带具体数值可核验）。
    pub fn declare_invariants(&mut self, snap: &ArbSnapshot) -> Result<(), u16> {
        if snap.budgets.is_empty() {
            return Err(E_FRM_EMPTY);
        }
        let bsum: u64 = snap.budgets.iter().map(|(_, v)| *v).sum();
        let gsum: u64 = snap.granted.iter().map(|(_, v)| *v).sum();
        let n = snap.budgets.len();
        self.invariants.clear();
        self.invariants.push(Invariant {
            name: "conservation",
            expr: alloc::format!(
                "(assert (<= (sum granted) {})) ; granted_sum={} budget_sum={}",
                bsum, gsum, bsum
            ),
            status: FormalStatus::DeclaredOnly,
        });
        self.invariants.push(Invariant {
            name: "no-starvation",
            expr: alloc::format!(
                "(assert (<= max_wait {})) ; observed={}",
                snap.wait_bound, snap.max_wait
            ),
            status: FormalStatus::DeclaredOnly,
        });
        self.invariants.push(Invariant {
            name: "no-oversell",
            expr: alloc::format!(
                "(assert (<= (sum granted) {})) ; capacity_pm={}",
                snap.capacity_pm, snap.capacity_pm
            ),
            status: FormalStatus::DeclaredOnly,
        });
        let _ = n;
        self.exports += 1;
        Ok(())
    }

    /// 状态机导出（SMT-LIB 风格三段文本——模型检验可读）。
    pub fn export(&self, snap: &ArbSnapshot) -> Result<String, u16> {
        if snap.budgets.is_empty() {
            return Err(E_FRM_EMPTY);
        }
        let mut s = String::from("; CGPU arbiter state export v1 (format: smt-lib style)\n");
        s.push_str("; --- constants ---\n");
        for (a, b) in &snap.budgets {
            s.push_str(&alloc::format!("(declare-const budget_{a} Int) ; = {b}\n"));
        }
        s.push_str("; --- functions ---\n");
        s.push_str("(declare-fun sum-granted () Int)\n");
        s.push_str("; --- assertions ---\n");
        for inv in &self.invariants {
            s.push_str(&inv.expr);
            s.push('\n');
        }
        Ok(s)
    }

    /// 导出与实现一致性校验：导出文本中的数值与快照逐项对表。
    pub fn consistency_check(&self, exported: &str, snap: &ArbSnapshot) -> Result<(), u16> {
        // ① 守恒数值在文本中。
        let bsum: u64 = snap.budgets.iter().map(|(_, v)| *v).sum();
        if !exported.contains(&alloc::format!("budget_sum={}", bsum)) {
            return Err(E_FRM_DRIFT);
        }
        // ② 饥饿上界在文本中。
        if !exported.contains(&alloc::format!("max_wait {}", snap.wait_bound)) {
            return Err(E_FRM_DRIFT);
        }
        // ③ 容量在文本中。
        if !exported.contains(&alloc::format!("capacity_pm={}", snap.capacity_pm)) {
            return Err(E_FRM_DRIFT);
        }
        // ④ 每账户预算行在场。
        for (a, b) in &snap.budgets {
            if !exported.contains(&alloc::format!("budget_{a} Int) ; = {b}")) {
                return Err(E_FRM_DRIFT);
            }
        }
        Ok(())
    }

    /// 诚实标注防线：未证明状态不得冒充已证明。
    pub fn assert_honest(&self) -> Result<(), u16> {
        for inv in &self.invariants {
            if inv.status == FormalStatus::ToolVerified {
                // 一期无工具核验通道——出现 ToolVerified 即违规。
                return Err(E_FRM_HONEST);
            }
        }
        Ok(())
    }

    /// 不变量表达完整性（三条全在且非空表达）。
    pub fn invariants_complete(&self) -> Result<usize, u16> {
        let want = ["conservation", "no-starvation", "no-oversell"];
        for w in want {
            match self.invariants.iter().find(|i| i.name == w) {
                Some(i) if !i.expr.is_empty() => {}
                _ => return Err(E_FRM_INV),
            }
        }
        Ok(self.invariants.len())
    }

    pub fn invariant_count(&self) -> usize {
        self.invariants.len()
    }
}

// ==========================================================================
// 判据（施工书：导出接口、形式化表达、诚实标注、一致性、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> ArbSnapshot {
        ArbSnapshot {
            budgets: vec![(1, 400), (2, 350), (3, 250)],
            granted: vec![(1, 300), (2, 250), (3, 150)],
            max_wait: 12,
            wait_bound: 20,
            capacity_pm: 1000,
        }
    }

    #[test]
    fn a01_状态机导出_SMT风格可读() {
        let mut fx = FormalExporter::new();
        let s = snap();
        fx.declare_invariants(&s).unwrap();
        let text = fx.export(&s).unwrap();
        // 三段齐备：常量/函数/断言。
        assert!(text.contains("declare-const budget_1 Int) ; = 400"));
        assert!(text.contains("declare-fun sum-granted"));
        assert!(text.contains("(assert"));
        assert!(text.starts_with("; CGPU arbiter state export v1"));
        // 空快照拒导出。
        let e = ArbSnapshot::default();
        assert_eq!(fx.export(&e), Err(E_FRM_EMPTY));
        assert_eq!(fx.declare_invariants(&e), Err(E_FRM_EMPTY));
    }

    #[test]
    fn a02_三不变量形式化表达() {
        let mut fx = FormalExporter::new();
        let s = snap();
        fx.declare_invariants(&s).unwrap();
        assert_eq!(fx.invariants_complete(), Ok(3));
        // 守恒：gsum 700 ≤ bsum 1000。
        assert!(s.conservation_ok());
        // 无饥饿：12 ≤ 20。
        assert!(s.no_starvation_ok());
        // 无超卖：700 ≤ 1000。
        assert!(s.no_oversell_ok());
        // 违例快照：三项各自红。
        let mut bad = snap();
        bad.granted[0].1 = 900;
        assert!(!bad.conservation_ok());
        assert!(!bad.no_oversell_ok());
        let mut bad2 = snap();
        bad2.max_wait = 21;
        assert!(!bad2.no_starvation_ok());
    }

    #[test]
    fn a03_诚实标注_声明不冒充证明() {
        let mut fx = FormalExporter::new();
        fx.declare_invariants(&snap()).unwrap();
        // 一期常态：全部 DeclaredOnly → 诚实通过。
        assert_eq!(fx.assert_honest(), Ok(()));
        for i in &fx.invariants {
            assert_eq!(i.status, FormalStatus::DeclaredOnly);
        }
        // 冒充已证明 → 红（一期无工具核验通道）。
        let mut fx2 = FormalExporter::new();
        fx2.declare_invariants(&snap()).unwrap();
        fx2.invariants[0].status = FormalStatus::ToolVerified;
        assert_eq!(fx2.assert_honest(), Err(E_FRM_HONEST));
    }

    #[test]
    fn a04_导出与实现一致性() {
        let mut fx = FormalExporter::new();
        let s = snap();
        fx.declare_invariants(&s).unwrap();
        let text = fx.export(&s).unwrap();
        // 同源导出 → 一致。
        assert_eq!(fx.consistency_check(&text, &s), Ok(()));
        // 篡改快照（预算漂移）→ 红。
        let mut drift = snap();
        drift.budgets[0].1 = 401;
        assert_eq!(fx.consistency_check(&text, &drift), Err(E_FRM_DRIFT));
        // 上界漂移 → 红。
        let mut drift2 = snap();
        drift2.wait_bound = 21;
        assert_eq!(fx.consistency_check(&text, &drift2), Err(E_FRM_DRIFT));
        // 截断的导出文本 → 红。
        assert_eq!(fx.consistency_check("(assert", &s), Err(E_FRM_DRIFT));
    }

    #[test]
    fn a05_重复导出与计数() {
        let mut fx = FormalExporter::new();
        assert_eq!(fx.invariant_count(), 0);
        fx.declare_invariants(&snap()).unwrap();
        assert_eq!(fx.invariant_count(), 3);
        // 重复声明覆盖（不叠加——导出的是当前快照影子）。
        let mut s2 = snap();
        s2.max_wait = 5;
        fx.declare_invariants(&s2).unwrap();
        assert_eq!(fx.invariant_count(), 3);
        assert!(fx.invariants[1].expr.contains("observed=5"));
    }
}
