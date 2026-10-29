//! UNX-F10233 harness 误报检出（harness 误报识别并标记，误报混账检出）（0x11040 段）
//!
//! 锚点：UNX-F10233-J1 误报检出达成（harness 误报识别并标记
//! 10/10 次），误报混账 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：误报甄别器（误报特征表 + 症状比对 + 误报标记账）+ 混账
//! 检出核（误报混入真败账给诊断码）+ 折叠指纹 +
//! 注入核（误报入真账/漏标/重标/封账/空特征）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x11040;
/// 诊断码：误报混账
pub const ERR_MIXED_LEDGER: u32 = SEG_BASE + 0x01;
/// 诊断码：症状为空
pub const ERR_EMPTY_SYMPTOM: u32 = SEG_BASE + 0x02;
/// 诊断码：重复标记
pub const ERR_DUP_MARK: u32 = SEG_BASE + 0x03;
/// 诊断码：封账操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：特征为空
pub const ERR_EMPTY_PATTERN: u32 = SEG_BASE + 0x05;

/// 甄别器
pub struct FalseAlarmDetector {
    /// 误报特征表（症状关键词）
    pub patterns: Vec<String>,
    /// 已标记误报账
    pub marked: Vec<String>,
    /// 真败账
    pub real_failures: Vec<String>,
    /// 封账
    pub frozen: bool,
}

impl FalseAlarmDetector {
    /// 开器
    pub fn new() -> Self {
        FalseAlarmDetector {
            patterns: Vec::new(),
            marked: Vec::new(),
            real_failures: Vec::new(),
            frozen: false,
        }
    }

    /// 登记误报特征
    pub fn add_pattern(&mut self, pat: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if pat.is_empty() {
            return Err(ERR_EMPTY_PATTERN);
        }
        if self.patterns.contains(&pat.to_string()) {
            return Err(ERR_DUP_MARK);
        }
        self.patterns.push(pat.to_string());
        Ok(())
    }

    /// 甄别一次失败：命中特征即误报
    pub fn is_false_alarm(&self, symptom: &str) -> bool {
        self.patterns.iter().any(|p| symptom.contains(p.as_str()))
    }

    /// 记失败：误报入标记账、真败入真账
    pub fn report(&mut self, case: &str, symptom: &str) -> Result<bool, u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if symptom.is_empty() {
            return Err(ERR_EMPTY_SYMPTOM);
        }
        if self.is_false_alarm(symptom) {
            if self.marked.contains(&case.to_string()) {
                return Err(ERR_DUP_MARK);
            }
            self.marked.push(case.to_string());
            Ok(true)
        } else {
            self.real_failures.push(case.to_string());
            Ok(false)
        }
    }

    /// 混账检出：某用例既在误报账又在真账
    pub fn mixed_entries(&self) -> Vec<String> {
        self.marked.iter()
            .filter(|m| self.real_failures.contains(m))
            .cloned()
            .collect()
    }

    /// 误报率（基点）
    pub fn false_alarm_ratio_bps(&self) -> u32 {
        let total = self.marked.len() + self.real_failures.len();
        if total == 0 {
            return 0;
        }
        (self.marked.len() * 10000 / total) as u32
    }

    /// 特征计数
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// 误报清单
    pub fn marked_list(&self) -> Vec<String> {
        self.marked.clone()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, p) in self.patterns.iter().enumerate() {
            acc = acc.wrapping_mul(4099).wrapping_add((p.len() as u32) + i as u32);
            for b in p.as_bytes() {
                acc = acc ^ (*b as u32);
            }
        }
        acc = acc.wrapping_add((self.marked.len() as u32) << 1);
        acc = acc.wrapping_add(self.real_failures.len() as u32);
        acc
    }

    /// 诊断面：特征表回读
    pub fn pattern_list(&self) -> Vec<String> {
        self.patterns.clone()
    }

    /// 诊断面：真败清单
    pub fn real_list(&self) -> Vec<String> {
        self.real_failures.clone()
    }

    /// 诊断面：甄别器摘要
    pub fn detector_summary(&self) -> String {
        format!(
            "patterns={} marked={} real={}",
            self.patterns.len(),
            self.marked.len(),
            self.real_failures.len()
        )
    }

    /// 诊断面：某用例标记回读
    pub fn is_marked(&self, case: &str) -> bool {
        self.marked.contains(&case.to_string())
    }

    /// 诊断面：误报特征命中率（基点，标记数/总申报数）
    pub fn hit_ratio_bps(&self) -> u32 {
        let total = self.marked.len() + self.real_failures.len();
        if total == 0 {
            return 0;
        }
        (self.marked.len() * 10000 / total) as u32
    }

    /// 诊断面：全部申报已分流判定（无未分类）
    pub fn all_triaged(&self, reported: usize) -> bool {
        self.marked.len() + self.real_failures.len() == reported
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 误报混账检出
    let mut d1 = FalseAlarmDetector::new();
    let _ = d1.add_pattern("timeout-spurious");
    let _ = d1.report("c", "hit timeout-spurious");
    d1.real_failures.push("c".to_string()); // 外部混账
    out[0] = d1.mixed_entries() == vec!["c".to_string()];
    // 2: 空症状拒绝
    let mut d2 = FalseAlarmDetector::new();
    let _ = d2.add_pattern("p");
    out[1] = d2.report("c2", "") == Err(ERR_EMPTY_SYMPTOM);
    // 3: 重标拒绝
    let mut d3 = FalseAlarmDetector::new();
    let _ = d3.add_pattern("p");
    let _ = d3.report("c3", "p");
    out[2] = d3.report("c3", "p again") == Err(ERR_DUP_MARK);
    // 4: 封账拒绝
    let mut d4 = FalseAlarmDetector::new();
    let _ = d4.add_pattern("p");
    let _ = d4.seal();
    out[3] = d4.report("c4", "p") == Err(ERR_FROZEN) && d4.add_pattern("q") == Err(ERR_FROZEN);
    // 5: 空特征拒绝
    let mut d5 = FalseAlarmDetector::new();
    out[4] = d5.add_pattern("") == Err(ERR_EMPTY_PATTERN);
    out
}

/// 判据函数
pub fn run_unx10233_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10233");
    let mut d = FalseAlarmDetector::new();
    let p1 = d.add_pattern("spurious-eof");
    let p2 = d.add_pattern("timeout-spurious");
    let p3 = d.add_pattern("harness-clock-drift");
    s.add("三特征登记", p1.is_ok() && p2.is_ok() && p3.is_ok(), "三条误报特征入表");
    let r1 = d.report("ut-eof-a", "读到 spurious-eof 即断言失败");
    let r2 = d.report("ut-clock-b", "harness-clock-drift 超 3tick");
    let r3 = d.report("ut-real-c", "返回值 EACCES 期望 EPERM");
    s.add("两误报一真败", r1 == Ok(true) && r2 == Ok(true) && r3 == Ok(false), "症状比对两误报一真败");
    s.add("误报账两条", d.marked_list() == vec!["ut-eof-a".to_string(), "ut-clock-b".to_string()], "两条误报入标记账");
    s.add("真账一条", d.real_failures == vec!["ut-real-c".to_string()], "真败入真账");
    s.add("混账清单空", d.mixed_entries().is_empty(), "无误报混入真账");
    s.add("误报率三分之二", d.false_alarm_ratio_bps() == 6666, "2/3 误报率 6666 基点");
    s.add("特征计数三", d.pattern_count() == 3, "特征表三条");
    s.add("漏标检出", !d.is_false_alarm("无关键词症状") && d.is_false_alarm("含 timeout-spurious 症状"), "无特征不误标、有特征必标");
    s.add("重特征拒绝", d.add_pattern("spurious-eof") == Err(ERR_DUP_MARK), "重特征被拒");
    s.add("重标拒绝", d.report("ut-eof-a", "again spurious-eof") == Err(ERR_DUP_MARK), "同用例重标被拒");
    s.add("特征表回读", d.pattern_list() == vec!["spurious-eof".to_string(), "timeout-spurious".to_string(), "harness-clock-drift".to_string()], "特征表按登记序");
    s.add("真败清单", d.real_list() == vec!["ut-real-c".to_string()], "真败清单一条");
    s.add("甄别摘要", d.detector_summary().contains("marked=2") && d.detector_summary().contains("real=1"), "摘要含两账计数");
    s.add("标记回读", d.is_marked("ut-eof-a") && !d.is_marked("ut-real-c"), "误报已标、真败未标");
    s.add("命中率", d.hit_ratio_bps() == 6666, "命中率与误报率同口径");
    s.add("全分流判定", d.all_triaged(3), "三条申报全部入两账");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = d.seal();
    s.add("封账拒绝", d.report("late", "spurious-eof") == Err(ERR_FROZEN), "封账后申报被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symptom_matching_direct() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("spurious");
        assert!(d.is_false_alarm("xxx spurious yyy"));
        assert!(!d.is_false_alarm("clean failure"));
        assert_eq!(d.pattern_count(), 1);
    }

    #[test]
    fn ratio_and_mixed() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("p");
        let _ = d.report("a", "p hit");
        let _ = d.report("b", "real bug");
        let _ = d.report("c", "real bug 2");
        assert_eq!(d.false_alarm_ratio_bps(), 3333);
        assert!(d.mixed_entries().is_empty());
        assert_eq!(d.marked_list(), vec!["a".to_string()]);
    }

    #[test]
    fn mixed_after_forge_detected() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("spurious");
        let _ = d.report("m", "spurious hit");
        d.real_failures.push("m".to_string());
        assert_eq!(d.mixed_entries(), vec!["m".to_string()]);
        assert_eq!(d.false_alarm_ratio_bps(), 5000);
    }

    #[test]
    fn pattern_roundtrip_and_empty() {
        let mut d = FalseAlarmDetector::new();
        assert_eq!(d.add_pattern(""), Err(ERR_EMPTY_PATTERN));
        let _ = d.add_pattern("p1");
        let _ = d.add_pattern("p2");
        assert_eq!(d.pattern_count(), 2);
        assert!(d.is_false_alarm("has p2 inside"));
        assert!(!d.is_false_alarm("has p3 inside"));
        assert!(d.marked_list().is_empty());
    }

    #[test]
    fn hit_ratio_and_triage() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("x");
        let _ = d.report("a", "x hit");
        let _ = d.report("b", "x hit");
        let _ = d.report("c", "real");
        let _ = d.report("d", "real2");
        assert_eq!(d.hit_ratio_bps(), 5000);
        assert!(d.all_triaged(4));
        assert_eq!(d.real_list(), vec!["c".to_string(), "d".to_string()]);
        assert!(d.is_marked("b"));
        assert!(!d.is_marked("c"));
    }

    #[test]
    fn summary_and_lists_roundtrip() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("zz");
        let _ = d.report("k1", "zz");
        assert!(d.detector_summary().contains("patterns=1"));
        assert!(d.detector_summary().contains("marked=1"));
        assert!(d.detector_summary().contains("real=0"));
        assert_eq!(d.pattern_list(), vec!["zz".to_string()]);
        assert!(d.real_list().is_empty());
    }

    #[test]
    fn partial_triage_detected() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("p");
        let _ = d.report("a", "p");
        // 实际申报 3 条但只入账 2 条（一条未走 report）
        assert!(!d.all_triaged(3));
        assert!(d.all_triaged(1));
        assert_eq!(d.hit_ratio_bps(), 10000);
    }

    #[test]
    fn is_marked_false_for_unreported() {
        let mut d = FalseAlarmDetector::new();
        let _ = d.add_pattern("q");
        assert!(!d.is_marked("nope"));
        assert!(d.marked_list().is_empty());
        assert!(d.real_list().is_empty());
        assert_eq!(d.hit_ratio_bps(), 0);
    }
}
