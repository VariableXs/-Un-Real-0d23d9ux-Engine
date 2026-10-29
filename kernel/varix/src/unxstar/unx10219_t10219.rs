//! UNX-F10219 执行统计总表（四计数与逐条账对平，漏计检出）（0x10e80 段）
//!
//! 锚点：UNX-F10219-J1 四计数总表达成（四计数与逐条账对平
//! 10/10 次），漏计 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：总表（四计数 + 逐条终态流水 + 域分账）+ 对平核（三层：
//! 流水↔四账、四账↔总数、域分账↔总账）+ 漏计检出 + 折叠指纹 +
//! 注入核（改账漂移/域串账/漏域/封表改写/空表虚绿）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10e80;
/// 诊断码：四账失衡
pub const ERR_IMBALANCE: u32 = SEG_BASE + 0x01;
/// 诊断码：流水缺账
pub const ERR_ITEM_MISSING: u32 = SEG_BASE + 0x02;
/// 诊断码：域串账
pub const ERR_DOMAIN_CROSS: u32 = SEG_BASE + 0x03;
/// 诊断码：封表改写
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：空表虚绿
pub const ERR_EMPTY_GREEN: u32 = SEG_BASE + 0x05;

/// 逐条终态
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 通过
    Pass,
    /// 失败
    Fail,
    /// 跳过
    Skip,
}

/// 域分账
#[derive(Clone, PartialEq, Eq)]
pub struct DomainLedger {
    /// 域名
    pub domain: String,
    /// 本域计数
    pub count: usize,
}

/// 执行统计总表
pub struct StatMaster {
    /// 四计数
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    /// 逐条流水
    pub items: Vec<(String, Outcome)>,
    /// 域分账
    pub domains: Vec<DomainLedger>,
    /// 封表标记
    pub frozen: bool,
}

impl StatMaster {
    /// 空表
    pub fn new() -> Self {
        StatMaster {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            items: Vec::new(),
            domains: Vec::new(),
            frozen: false,
        }
    }

    /// 记一条（带域名）
    pub fn record(&mut self, case: &str, domain: &str, o: Outcome) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        self.total += 1;
        match o {
            Outcome::Pass => self.passed += 1,
            Outcome::Fail => self.failed += 1,
            Outcome::Skip => self.skipped += 1,
        }
        self.items.push((case.to_string(), o));
        match self.domains.iter_mut().find(|d| d.domain == domain) {
            Some(d) => d.count += 1,
            None => self.domains.push(DomainLedger { domain: domain.to_string(), count: 1 }),
        }
        Ok(())
    }

    /// 层一：四账对平
    pub fn quad_balanced(&self) -> bool {
        self.passed + self.failed + self.skipped == self.total
    }

    /// 层二：流水↔四账
    pub fn items_match(&self) -> bool {
        let p = self.items.iter().filter(|(_, o)| *o == Outcome::Pass).count();
        let f = self.items.iter().filter(|(_, o)| *o == Outcome::Fail).count();
        let k = self.items.iter().filter(|(_, o)| *o == Outcome::Skip).count();
        p == self.passed && f == self.failed && k == self.skipped
    }

    /// 层三：域分账↔总账（计数和一致且域名无重账——重名即串账）
    pub fn domains_match(&self) -> bool {
        if self.domains.iter().map(|d| d.count).sum::<usize>() != self.total {
            return false;
        }
        for a in 0..self.domains.len() {
            for b in (a + 1)..self.domains.len() {
                if self.domains[a].domain == self.domains[b].domain {
                    return false;
                }
            }
        }
        true
    }

    /// 漏计检出：任一层失衡即漏
    pub fn audit(&self) -> Result<(), u32> {
        if !self.quad_balanced() {
            return Err(ERR_IMBALANCE);
        }
        if !self.items_match() {
            return Err(ERR_ITEM_MISSING);
        }
        if !self.domains_match() {
            return Err(ERR_DOMAIN_CROSS);
        }
        if self.total == 0 {
            return Err(ERR_EMPTY_GREEN);
        }
        Ok(())
    }

    /// 全绿判定（空表不给绿）
    pub fn all_green(&self) -> bool {
        self.total > 0 && self.failed == 0 && self.audit().is_ok()
    }

    /// 某域计数
    pub fn count_of_domain(&self, domain: &str) -> usize {
        self.domains.iter().find(|d| d.domain == domain).map(|d| d.count).unwrap_or(0)
    }

    /// 通过率（基点）
    pub fn pass_ratio_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.passed * 10000 / self.total) as u32
    }

    /// 冻结总表
    pub fn freeze(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        acc = acc.wrapping_add((self.total as u32) << 2);
        acc = acc.wrapping_add((self.passed as u32) << 1);
        acc = acc.wrapping_add((self.failed as u32).wrapping_mul(3));
        acc = acc.wrapping_add(self.skipped as u32);
        for (i, d) in self.domains.iter().enumerate() {
            acc = acc.wrapping_mul(3301).wrapping_add((d.count as u32) + i as u32);
            for b in d.domain.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        acc
    }

    /// 诊断面：域清单
    pub fn domain_list(&self) -> Vec<String> {
        self.domains.iter().map(|d| d.domain.clone()).collect()
    }

    /// 诊断面：失败用例清单
    pub fn failed_cases(&self) -> Vec<String> {
        self.items.iter().filter(|(_, o)| *o == Outcome::Fail).map(|(c, _)| c.clone()).collect()
    }

    /// 诊断面：跳过密度（基点）
    pub fn skip_density_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.skipped * 10000 / self.total) as u32
    }

    /// 诊断面：总表摘要
    pub fn summary(&self) -> String {
        format!(
            "total={} pass={} fail={} skip={} domains={}",
            self.total,
            self.passed,
            self.failed,
            self.skipped,
            self.domains.len()
        )
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 改账漂移检出
    let mut m1 = StatMaster::new();
    let _ = m1.record("c", "d", Outcome::Pass);
    m1.total = 9;
    out[0] = m1.audit() == Err(ERR_IMBALANCE);
    // 2: 域串账检出
    let mut m2 = StatMaster::new();
    let _ = m2.record("c1", "dA", Outcome::Pass);
    let _ = m2.record("c2", "dB", Outcome::Pass);
    m2.domains[1].domain = "dA".to_string();
    out[1] = !m2.domains_match();
    // 3: 封表改写被拒
    let mut m3 = StatMaster::new();
    let _ = m3.record("c", "d", Outcome::Pass);
    let _ = m3.freeze();
    out[2] = m3.record("late", "d", Outcome::Pass) == Err(ERR_FROZEN);
    // 4: 空表虚绿拒绝
    out[3] = StatMaster::new().audit() == Err(ERR_EMPTY_GREEN)
        && !StatMaster::new().all_green();
    // 5: 流水缺账检出
    let mut m5 = StatMaster::new();
    let _ = m5.record("c1", "d", Outcome::Pass);
    let _ = m5.record("c2", "d", Outcome::Fail);
    m5.items.pop();
    out[4] = m5.audit() == Err(ERR_ITEM_MISSING);
    out
}

/// 判据函数
pub fn run_unx10219_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10219");
    let mut m = StatMaster::new();
    let _ = m.record("ut-1", "sys", Outcome::Pass);
    let _ = m.record("ut-2", "sys", Outcome::Pass);
    let _ = m.record("ut-3", "pty", Outcome::Pass);
    let _ = m.record("ut-4", "pty", Outcome::Fail);
    let _ = m.record("ut-5", "sched", Outcome::Skip);
    s.add("五条入总表", m.total == 5, "五条终态入账");
    s.add("四账对平", m.quad_balanced(), "3+1+1=5 对平");
    s.add("流水对账", m.items_match(), "逐条流水与四账一致");
    s.add("域分账对平", m.domains_match(), "三域计数和=总账");
    s.add("审计通过", m.audit().is_ok(), "三层审计放行");
    s.add("非全绿", !m.all_green(), "有败域不给绿");
    s.add("域计数", m.count_of_domain("sys") == 2 && m.count_of_domain("pty") == 2 && m.count_of_domain("sched") == 1, "三域计数正确");
    s.add("通过率六成", m.pass_ratio_bps() == 6000, "3/5 通过率 6000 基点");
    let mut mg = StatMaster::new();
    for i in 0..4 {
        let _ = mg.record(&format!("g{}", i), "d", Outcome::Pass);
    }
    s.add("全绿样例", mg.all_green() && mg.audit().is_ok(), "四条全过审计绿");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = m.freeze();
    s.add("封表后拒绝", m.record("late", "d", Outcome::Pass) == Err(ERR_FROZEN), "封表改写被拒");
    s.add("冻结指纹非零", fp != 0, "冻结折叠指纹非零");
    s.add("域清单", m.domain_list() == vec!["sys".to_string(), "pty".to_string(), "sched".to_string()], "域按首现序排列");
    s.add("失败清单", m.failed_cases() == vec!["ut-4".to_string()], "失败清单仅 ut-4");
    s.add("跳过密度两成", m.skip_density_bps() == 2000, "1/5 跳过 2000 基点");
    s.add("摘要对账", m.summary().contains("total=5") && m.summary().contains("domains=3"), "摘要含总数与域数");
    s.add("空表零密度", StatMaster::new().skip_density_bps() == 0, "空表跳过密度为零");
    s.add("单域样例", {
        let mut ms = StatMaster::new();
        let _ = ms.record("x", "only", Outcome::Pass);
        ms.domain_list() == vec!["only".to_string()] && ms.domains_match()
    }, "单域账自平");
    s.add("全绿审计两连", {
        let mut mg = StatMaster::new();
        let _ = mg.record("g1", "d", Outcome::Pass);
        mg.audit().is_ok() && mg.all_green() && mg.freeze() != 0
    }, "单条全绿可审计可冻结");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_layer_audit_passes() {
        let mut m = StatMaster::new();
        let _ = m.record("a", "d1", Outcome::Pass);
        let _ = m.record("b", "d2", Outcome::Fail);
        let _ = m.record("c", "d2", Outcome::Skip);
        assert!(m.quad_balanced());
        assert!(m.items_match());
        assert!(m.domains_match());
        assert!(m.audit().is_ok());
        assert!(!m.all_green());
    }

    #[test]
    fn empty_master_never_green() {
        let m = StatMaster::new();
        assert_eq!(m.audit(), Err(ERR_EMPTY_GREEN));
        assert!(!m.all_green());
        assert_eq!(m.pass_ratio_bps(), 0);
    }

    #[test]
    fn freeze_locks_table() {
        let mut m = StatMaster::new();
        let _ = m.record("a", "d", Outcome::Pass);
        let fp = m.freeze();
        assert_eq!(m.record("b", "d", Outcome::Pass), Err(ERR_FROZEN));
        assert!(fp != 0);
    }

    #[test]
    fn failed_cases_and_density() {
        let mut m = StatMaster::new();
        let _ = m.record("a", "d", Outcome::Pass);
        let _ = m.record("b", "d", Outcome::Fail);
        let _ = m.record("c", "d", Outcome::Skip);
        let _ = m.record("d2", "d", Outcome::Fail);
        assert_eq!(m.failed_cases(), vec!["b".to_string(), "d2".to_string()]);
        assert_eq!(m.skip_density_bps(), 2500);
        assert!(m.summary().contains("fail=2"));
    }

    #[test]
    fn domain_list_order_preserved() {
        let mut m = StatMaster::new();
        let _ = m.record("a", "zz", Outcome::Pass);
        let _ = m.record("b", "aa", Outcome::Pass);
        let _ = m.record("c", "mm", Outcome::Pass);
        assert_eq!(m.domain_list(), vec!["zz".to_string(), "aa".to_string(), "mm".to_string()]);
        assert!(m.audit().is_ok());
    }
}
