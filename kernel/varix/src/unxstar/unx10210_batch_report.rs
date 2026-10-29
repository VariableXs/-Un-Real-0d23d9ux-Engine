//! UNX-F10210 批次报告聚合（四计数与逐条账对平，计数漂移检出）（0x10d60 段）
//!
//! 锚点：UNX-F10210-J1 报告聚合达成（四计数与逐条账对平
//! 10/10 次），计数漂移 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：四账（总/过/败/跳）+ 逐条终态流水 + 对平核（账账相抵
//! 与流水回放双验）+ 审计核（漂移给诊断码）+ 折叠指纹 +
//! 注入核（改总失衡/空批/封后记账/越界账/平凡虚报）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10d60;
/// 诊断码：四账失衡
pub const ERR_LEDGER_IMBALANCE: u32 = SEG_BASE + 0x01;
/// 诊断码：批次为空
pub const ERR_EMPTY_BATCH: u32 = SEG_BASE + 0x02;
/// 诊断码：负计数
pub const ERR_NEGATIVE: u32 = SEG_BASE + 0x03;
/// 诊断码：报告已封
pub const ERR_SEALED: u32 = SEG_BASE + 0x04;
/// 诊断码：条目缺账
pub const ERR_ITEM_ORPHAN: u32 = SEG_BASE + 0x05;

/// 用例终态
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaseEnd {
    /// 通过
    Pass,
    /// 失败
    Fail,
    /// 跳过
    Skip,
}

/// 批次报告（四账）
pub struct BatchReport {
    /// 批次标识
    pub batch: String,
    /// 总数账
    pub total: usize,
    /// 通过账
    pub passed: usize,
    /// 失败账
    pub failed: usize,
    /// 跳过账
    pub skipped: usize,
    /// 逐条终态流水
    pub items: Vec<CaseEnd>,
    /// 封账标记
    pub sealed: bool,
}

impl BatchReport {
    /// 开账
    pub fn new(batch: &str) -> Self {
        BatchReport {
            batch: batch.to_string(),
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            items: Vec::new(),
            sealed: false,
        }
    }

    /// 记一条终态
    pub fn record(&mut self, end: CaseEnd) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if self.batch.is_empty() {
            return Err(ERR_EMPTY_BATCH);
        }
        self.total += 1;
        match end {
            CaseEnd::Pass => self.passed += 1,
            CaseEnd::Fail => self.failed += 1,
            CaseEnd::Skip => self.skipped += 1,
        }
        self.items.push(end);
        Ok(())
    }

    /// 四账对平检查
    pub fn balanced(&self) -> bool {
        self.passed + self.failed + self.skipped == self.total
    }

    /// 逐条流水与四账一致
    pub fn items_match_ledgers(&self) -> bool {
        let p = self.items.iter().filter(|&&e| e == CaseEnd::Pass).count();
        let f = self.items.iter().filter(|&&e| e == CaseEnd::Fail).count();
        let k = self.items.iter().filter(|&&e| e == CaseEnd::Skip).count();
        p == self.passed && f == self.failed && k == self.skipped
    }

    /// 硬校验：四账对平且流水一致，否则给诊断码
    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(ERR_LEDGER_IMBALANCE);
        }
        if !self.items_match_ledgers() {
            return Err(ERR_ITEM_ORPHAN);
        }
        Ok(())
    }

    /// 通过率（基点）
    pub fn pass_ratio_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.passed * 10000 / self.total) as u32
    }

    /// 直接改账（仅注入场景使用）
    pub fn poke_total(&mut self, v: usize) {
        self.total = v;
    }

    /// 负值检出：任何账目为负即错（usize 下恒假，此处校验饱和回绕痕迹）
    pub fn negative_free(&self) -> bool {
        self.passed <= self.total && self.failed <= self.total && self.skipped <= self.total
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        let mut acc = 0x10d60u32;
        acc = acc.wrapping_add((self.total as u32) << 3);
        acc = acc.wrapping_add((self.passed as u32) << 2);
        acc = acc.wrapping_add((self.failed as u32) << 1);
        acc = acc.wrapping_add(self.skipped as u32);
        for (i, e) in self.items.iter().enumerate() {
            let v: u32 = match e {
                CaseEnd::Pass => 1,
                CaseEnd::Fail => 2,
                CaseEnd::Skip => 3,
            };
            acc = acc.wrapping_mul(5741).wrapping_add(v.wrapping_add(i as u32));
        }
        acc
    }

    /// 诊断面：失败密度（基点）
    pub fn fail_density_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.failed * 10000 / self.total) as u32
    }

    /// 诊断面：跳过密度（基点）
    pub fn skip_density_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.skipped * 10000 / self.total) as u32
    }

    /// 诊断面：报告摘要文本
    pub fn summary(&self) -> String {
        format!(
            "batch={} total={} pass={} fail={} skip={}",
            self.batch, self.total, self.passed, self.failed, self.skipped
        )
    }

    /// 诊断面：是否存在失败账
    pub fn has_failures(&self) -> bool {
        self.failed > 0
    }

    /// 诊断面：全过判定
    pub fn all_passed(&self) -> bool {
        self.total > 0 && self.passed == self.total
    }

    /// 诊断面：连续通过游标（从头数最长的 Pass 段）
    pub fn leading_pass_streak(&self) -> usize {
        let mut n = 0;
        for e in &self.items {
            if *e == CaseEnd::Pass {
                n += 1;
            } else {
                break;
            }
        }
        n
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 直接改总数造成四账失衡 → 审计拒绝
    let mut r1 = BatchReport::new("inj-bal");
    let _ = r1.record(CaseEnd::Pass);
    r1.poke_total(9);
    out[0] = r1.audit() == Err(ERR_LEDGER_IMBALANCE);
    // 2: 空批次记账 → 拒绝
    let mut r2 = BatchReport::new("");
    out[1] = r2.record(CaseEnd::Pass) == Err(ERR_EMPTY_BATCH);
    // 3: 封账后记账 → 拒绝
    let mut r3 = BatchReport::new("inj-seal");
    let _ = r3.record(CaseEnd::Skip);
    let _ = r3.seal();
    out[2] = r3.record(CaseEnd::Pass) == Err(ERR_SEALED);
    // 4: 账目越界（过账>总账）→ 负值检出命中
    let mut r4 = BatchReport::new("inj-neg");
    let _ = r4.record(CaseEnd::Pass);
    r4.poke_total(0);
    out[3] = !r4.negative_free();
    // 5: 空报告通过率为 0 且对平（平凡账不许虚报）
    let r5 = BatchReport::new("inj-empty");
    out[4] = r5.balanced() && r5.pass_ratio_bps() == 0 && r5.audit().is_ok();
    out
}

/// 判据函数
pub fn run_unx10210_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10210");
    let mut r = BatchReport::new("bt-agg-1");
    for _ in 0..6 {
        let _ = r.record(CaseEnd::Pass);
    }
    let _ = r.record(CaseEnd::Fail);
    let _ = r.record(CaseEnd::Fail);
    let _ = r.record(CaseEnd::Skip);
    s.add("九条记账", r.total == 9, "九条终态全部入账");
    s.add("四账对平", r.balanced(), "过+败+跳=总");
    s.add("流水对账", r.items_match_ledgers(), "逐条流水与四账一致");
    s.add("审计通过", r.audit().is_ok(), "硬校验放行");
    s.add("通过率三分之二", r.pass_ratio_bps() == 6666, "6/9 通过率 6666 基点");
    s.add("失败密度两成二", r.fail_density_bps() == 2222, "2/9 失败密度 2222 基点");
    s.add("账目无越界", r.negative_free(), "各分账不超总账");
    s.add("跳过密度一一基点", r.skip_density_bps() == 1111, "1/9 跳过 1111 基点");
    s.add("摘要对账", r.summary().contains("total=9") && r.summary().contains("fail=2"), "摘要文本与四账一致");
    s.add("存在失败账", r.has_failures() && !r.all_passed(), "有失败且非全过");
    s.add("前置通过段为六", r.leading_pass_streak() == 6, "开头连续六条 Pass");
    let mut rc = BatchReport::new("bt-clean");
    for _ in 0..4 {
        let _ = rc.record(CaseEnd::Pass);
    }
    s.add("全过判定", rc.all_passed() && rc.fail_density_bps() == 0, "全过批密度为零");
    s.add("空批平凡对平", BatchReport::new("bt-none").balanced() && !BatchReport::new("bt-none").has_failures(), "空批四账皆零对平");
    s.add("流水枚举", {
        let mut re_ = BatchReport::new("bt-enum");
        let _ = re_.record(CaseEnd::Skip);
        let _ = re_.record(CaseEnd::Skip);
        re_.items_match_ledgers() && re_.skip_density_bps() == 10000
    }, "全跳批流水与账一致");
    let mut r2 = BatchReport::new("bt-agg-2");
    let _ = r2.record(CaseEnd::Fail);
    s.add("失衡审计拒绝", {
        r2.poke_total(5);
        r2.audit() == Err(ERR_LEDGER_IMBALANCE)
    }, "总数被改后审计拒绝");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&b| b), "五类违规全部检出");
    let seal_v = r.seal();
    s.add("封账后记账拒绝", r.record(CaseEnd::Pass) == Err(ERR_SEALED), "封账后追加被拒");
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_ledgers_balance() {
        let mut r = BatchReport::new("t");
        let _ = r.record(CaseEnd::Pass);
        let _ = r.record(CaseEnd::Fail);
        let _ = r.record(CaseEnd::Skip);
        assert!(r.balanced());
        assert!(r.audit().is_ok());
        assert_eq!(r.total, 3);
    }

    #[test]
    fn imbalance_detected() {
        let mut r = BatchReport::new("t2");
        let _ = r.record(CaseEnd::Pass);
        r.poke_total(4);
        assert_eq!(r.audit(), Err(ERR_LEDGER_IMBALANCE));
    }

    #[test]
    fn streak_and_summary() {
        let mut r = BatchReport::new("t3");
        let _ = r.record(CaseEnd::Pass);
        let _ = r.record(CaseEnd::Pass);
        let _ = r.record(CaseEnd::Skip);
        assert_eq!(r.leading_pass_streak(), 2);
        assert!(r.summary().contains("skip=1"));
        assert_eq!(r.skip_density_bps(), 3333);
    }

    #[test]
    fn empty_report_trivially_balanced() {
        let r = BatchReport::new("t4");
        assert!(r.balanced());
        assert!(!r.all_passed());
        assert!(!r.has_failures());
    }

    #[test]
    fn all_skip_batch_matches() {
        let mut r = BatchReport::new("t5");
        let _ = r.record(CaseEnd::Skip);
        let _ = r.record(CaseEnd::Skip);
        assert!(r.items_match_ledgers());
        assert_eq!(r.skip_density_bps(), 10000);
        assert_eq!(r.leading_pass_streak(), 0);
    }

    #[test]
    fn sealed_ledger_rejects_record() {
        let mut r = BatchReport::new("t");
        let _ = r.record(CaseEnd::Pass);
        let _ = r.seal();
        assert_eq!(r.record(CaseEnd::Skip), Err(ERR_SEALED));
    }
}