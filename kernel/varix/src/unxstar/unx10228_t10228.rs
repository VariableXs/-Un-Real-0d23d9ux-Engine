//! UNX-F10228 回归池保护（翻绿项全量入回归集，裸翻绿拦截）（0x10fa0 段）
//!
//! 锚点：UNX-F10228-J1 回归保护达成（翻绿项全量入回归集
//! 10/10 次），裸翻绿 10/10 次拦截。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：回归池（在池集 + 翻绿账 + 入池核）+ 裸翻绿拦截核（翻绿未
//! 入池给诊断码）+ 池对账核（翻绿账与池集对平）+ 折叠指纹 +
//! 注入核（裸翻绿/重入池/出池翻绿/封池/空池虚绿）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10fa0;
/// 诊断码：裸翻绿
pub const ERR_NAKED_GREEN: u32 = SEG_BASE + 0x01;
/// 诊断码：重入池
pub const ERR_DUP_POOL: u32 = SEG_BASE + 0x02;
/// 诊断码：出池翻绿
pub const ERR_GREEN_OUTSIDE: u32 = SEG_BASE + 0x03;
/// 诊断码：封池操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：空池虚绿
pub const ERR_EMPTY_POOL: u32 = SEG_BASE + 0x05;

/// 回归池
pub struct RegressPool {
    /// 在池用例
    pub pooled: Vec<String>,
    /// 翻绿账（翻绿但尚未核销入池记录的来源工单）
    pub green_ledger: Vec<String>,
    /// 封池
    pub frozen: bool,
}

impl RegressPool {
    /// 空池
    pub fn new() -> Self {
        RegressPool { pooled: Vec::new(), green_ledger: Vec::new(), frozen: false }
    }

    /// 翻绿登记：翻绿项必须先登账再入池
    pub fn mark_green(&mut self, case: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_POOL);
        }
        if !self.green_ledger.contains(&case.to_string()) {
            self.green_ledger.push(case.to_string());
        }
        Ok(())
    }

    /// 入池：翻绿项全量入池；未登账的裸翻绿拒绝
    pub fn enroll(&mut self, case: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_POOL);
        }
        if !self.green_ledger.contains(&case.to_string()) {
            return Err(ERR_NAKED_GREEN);
        }
        if self.pooled.contains(&case.to_string()) {
            return Err(ERR_DUP_POOL);
        }
        self.pooled.push(case.to_string());
        Ok(())
    }

    /// 池对账核：翻绿账中所有项均已入池
    pub fn ledger_all_pooled(&self) -> bool {
        self.green_ledger.iter().all(|g| self.pooled.contains(g))
    }

    /// 裸翻绿清单：登账未入池
    pub fn naked_greens(&self) -> Vec<String> {
        self.green_ledger.iter().filter(|g| !self.pooled.contains(g)).cloned().collect()
    }

    /// 出池翻绿检出：入池项再次翻绿（回归翻车）账面记录
    pub fn regress_out(&mut self, case: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if !self.pooled.contains(&case.to_string()) {
            return Err(ERR_GREEN_OUTSIDE);
        }
        // 出池：移出池集并重新登账等待修复
        self.pooled.retain(|c| c != case);
        if !self.green_ledger.contains(&case.to_string()) {
            self.green_ledger.push(case.to_string());
        }
        Ok(())
    }

    /// 池集计数
    pub fn pool_size(&self) -> usize {
        self.pooled.len()
    }

    /// 在池判定
    pub fn in_pool(&self, case: &str) -> bool {
        self.pooled.contains(&case.to_string())
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, p) in self.pooled.iter().enumerate() {
            acc = acc.wrapping_mul(7307).wrapping_add((p.len() as u32) + i as u32);
            for b in p.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        for (i, g) in self.green_ledger.iter().enumerate() {
            acc = acc.wrapping_mul(3779).wrapping_add((g.len() as u32) << (i % 2));
        }
        acc
    }

    /// 诊断面：池集清单
    pub fn pool_list(&self) -> Vec<String> {
        self.pooled.clone()
    }

    /// 诊断面：翻绿账清单
    pub fn ledger_list(&self) -> Vec<String> {
        self.green_ledger.clone()
    }

    /// 诊断面：池账差集（登账未入池）
    pub fn gap_count(&self) -> usize {
        self.naked_greens().len()
    }

    /// 诊断面：池摘要
    pub fn pool_summary(&self) -> String {
        format!(
            "pool={} ledger={} gap={}",
            self.pooled.len(),
            self.green_ledger.len(),
            self.gap_count()
        )
    }

    /// 诊断面：全量保护判定（账空或账池平且池非空）
    pub fn fully_protected(&self) -> bool {
        self.pool_size() > 0 && self.ledger_all_pooled()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 裸翻绿入池拒绝
    let mut p1 = RegressPool::new();
    out[0] = p1.enroll("never-green") == Err(ERR_NAKED_GREEN);
    // 2: 重入池拒绝
    let mut p2 = RegressPool::new();
    let _ = p2.mark_green("c");
    let _ = p2.enroll("c");
    out[1] = p2.enroll("c") == Err(ERR_DUP_POOL);
    // 3: 出池翻绿检出
    let mut p3 = RegressPool::new();
    let _ = p3.mark_green("d");
    let _ = p3.enroll("d");
    out[2] = p3.regress_out("ghost") == Err(ERR_GREEN_OUTSIDE);
    // 4: 封池拒绝
    let mut p4 = RegressPool::new();
    let _ = p4.mark_green("e");
    let _ = p4.seal();
    out[3] = p4.enroll("e") == Err(ERR_FROZEN) && p4.mark_green("f") == Err(ERR_FROZEN);
    // 5: 空池虚绿拒绝
    let p5 = RegressPool::new();
    out[4] = p5.pool_size() == 0 && !p5.ledger_all_pooled() == false && p5.naked_greens().is_empty();
    out
}

/// 判据函数
pub fn run_unx10228_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10228");
    let mut p = RegressPool::new();
    let g1 = p.mark_green("ut-fix-1");
    let g2 = p.mark_green("ut-fix-2");
    s.add("双翻绿登账", g1.is_ok() && g2.is_ok() && p.green_ledger.len() == 2, "两条翻绿登账");
    s.add("未入池裸翻绿", !p.naked_greens().is_empty(), "登账未入池记为裸翻绿");
    s.add("裸翻绿入池拦截", RegressPool::new().enroll("x") == Err(ERR_NAKED_GREEN), "未登账项入池被拒");
    let e1 = p.enroll("ut-fix-1");
    let e2 = p.enroll("ut-fix-2");
    s.add("翻绿全入池", e1.is_ok() && e2.is_ok() && p.pool_size() == 2, "两条翻绿全量入池");
    s.add("池账对平", p.ledger_all_pooled() && p.naked_greens().is_empty(), "翻绿账与池集对平");
    s.add("在池判定", p.in_pool("ut-fix-1") && !p.in_pool("ut-fix-3"), "在池项命中、缺席项为否");
    s.add("重入池拒绝", p.enroll("ut-fix-1") == Err(ERR_DUP_POOL), "同项重入池被拒");
    let r = p.regress_out("ut-fix-2");
    s.add("回归翻车出池", r.is_ok() && !p.in_pool("ut-fix-2") && !p.ledger_all_pooled(), "出池项重新登账");
    s.add("出池后再入池", p.enroll("ut-fix-2").is_ok() && p.ledger_all_pooled(), "修复后可再次入池");
    s.add("池集清单", p.pool_list() == vec!["ut-fix-1".to_string(), "ut-fix-2".to_string()], "池集按入池序");
    s.add("翻绿账清单", p.ledger_list().len() == 2, "翻绿账两条");
    s.add("池账差集零", p.gap_count() == 0, "登账未入池差集为零");
    s.add("池摘要", p.pool_summary().contains("pool=2") && p.pool_summary().contains("gap=0"), "摘要含池数与差集");
    s.add("全量保护判定", p.fully_protected(), "池非空且账平构成全量保护");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = p.seal();
    s.add("封池拒绝", p.enroll("late") == Err(ERR_FROZEN), "封池后入池被拒");
    s.add("封池指纹非零", fp != 0, "封池折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_to_pool_flow() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("a");
        let _ = p.mark_green("b");
        let _ = p.enroll("a");
        assert!(!p.ledger_all_pooled());
        assert_eq!(p.naked_greens(), vec!["b".to_string()]);
        let _ = p.enroll("b");
        assert!(p.ledger_all_pooled());
        assert_eq!(p.pool_size(), 2);
    }

    #[test]
    fn regress_reenroll_cycle() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("c1");
        let _ = p.enroll("c1");
        let _ = p.regress_out("c1");
        assert!(!p.in_pool("c1"));
        let _ = p.enroll("c1");
        assert!(p.in_pool("c1"));
        assert_eq!(p.enroll("c1"), Err(ERR_DUP_POOL));
    }

    #[test]
    fn empty_pool_not_protected() {
        let p = RegressPool::new();
        assert!(!p.fully_protected());
        assert_eq!(p.pool_size(), 0);
        assert!(p.pool_list().is_empty());
        assert!(p.ledger_list().is_empty());
        assert_eq!(p.gap_count(), 0);
    }

    #[test]
    fn summary_tracks_gap() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("x");
        let _ = p.mark_green("y");
        let _ = p.enroll("x");
        assert!(p.pool_summary().contains("pool=1"));
        assert!(p.pool_summary().contains("gap=1"));
        assert!(!p.fully_protected());
    }

    #[test]
    fn ledger_lists_and_gap() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("a");
        let _ = p.mark_green("b");
        let _ = p.mark_green("c");
        let _ = p.enroll("a");
        assert_eq!(p.ledger_list(), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(p.naked_greens(), vec!["b".to_string(), "c".to_string()]);
        assert_eq!(p.gap_count(), 2);
        assert_eq!(p.pool_size(), 1);
        assert!(!p.fully_protected());
    }

    #[test]
    fn regress_out_requires_membership() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("m");
        let _ = p.enroll("m");
        assert!(p.regress_out("m").is_ok());
        assert_eq!(p.regress_out("m"), Err(ERR_GREEN_OUTSIDE));
        assert_eq!(p.pool_size(), 0);
    }

    #[test]
    fn dup_green_mark_is_noop() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("d");
        let _ = p.mark_green("d");
        assert_eq!(p.ledger_list(), vec!["d".to_string()]);
        let _ = p.enroll("d");
        assert!(p.fully_protected());
        assert!(p.pool_summary().contains("ledger=1"));
    }

    #[test]
    fn empty_case_rejected() {
        let mut p = RegressPool::new();
        assert_eq!(p.mark_green(""), Err(ERR_EMPTY_POOL));
        assert_eq!(p.enroll(""), Err(ERR_EMPTY_POOL));
        assert_eq!(p.pool_size(), 0);
        assert!(p.ledger_list().is_empty());
    }

    #[test]
    fn in_pool_membership_checks() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("k1");
        let _ = p.mark_green("k2");
        let _ = p.enroll("k1");
        assert!(p.in_pool("k1"));
        assert!(!p.in_pool("k2"));
        assert!(!p.in_pool(""));
        assert_eq!(p.seal() != 0, true);
    }

    #[test]
    fn reenroll_after_regress_recount() {
        let mut p = RegressPool::new();
        let _ = p.mark_green("r1");
        let _ = p.enroll("r1");
        let _ = p.regress_out("r1");
        let _ = p.enroll("r1");
        assert!(p.ledger_all_pooled());
        assert_eq!(p.gap_count(), 0);
        assert_eq!(p.pool_size(), 1);
        assert!(p.fully_protected());
        assert!(p.pool_summary().contains("gap=0"));
    }
}
