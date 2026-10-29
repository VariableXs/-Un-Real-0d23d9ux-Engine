//! UNX-F10214 豁免项自动隔离（豁免不计入分母，计入分母检出）（0x10de0 段）
//!
//! 锚点：UNX-F10214-J1 豁免隔离达成（豁免项自动隔离不计入分母
//! 10/10 次），计入分母 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：隔离闸（隔离区 + 在册失败区 + 批次序）+ 豁免核（先
//! 失败后豁免迁移）+ 口径核（有效失败剔除豁免）+ 折叠指纹 +
//! 注入核（空理由/未败先豁/重复豁免/额度触顶/隔离未生效）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10de0;
/// 诊断码：豁免理由为空
pub const ERR_EMPTY_REASON: u32 = SEG_BASE + 0x01;
/// 诊断码：重复豁免
pub const ERR_DUP_EXEMPT: u32 = SEG_BASE + 0x02;
/// 诊断码：隔离未生效
pub const ERR_NOT_ISOLATED: u32 = SEG_BASE + 0x03;
/// 诊断码：豁免越权
pub const ERR_EXEMPT_OVERFLOW: u32 = SEG_BASE + 0x04;
/// 诊断码：封账
pub const ERR_SEALED: u32 = SEG_BASE + 0x05;

/// 豁免额度上限
const EXEMPT_CAP: usize = 16;

/// 隔离区条目
#[derive(Clone, PartialEq, Eq)]
pub struct ExemptItem {
    /// 用例标识
    pub case: String,
    /// 豁免理由（非空）
    pub reason: String,
    /// 隔离批次号
    pub quarantine_batch: usize,
}

impl ExemptItem {
    /// 条目自洽
    pub fn ok(&self) -> bool {
        !self.case.is_empty() && !self.reason.is_empty() && self.quarantine_batch >= 1
    }
}

/// 豁免隔离闸
pub struct ExemptGate {
    /// 隔离区
    pub quarantined: Vec<ExemptItem>,
    /// 在册失败（未豁免）
    pub active_failures: Vec<String>,
    /// 隔离批次计数
    pub batch_seq: usize,
    /// 封账标记
    pub sealed: bool,
}

impl ExemptGate {
    /// 开闸
    pub fn new() -> Self {
        ExemptGate { quarantined: Vec::new(), active_failures: Vec::new(), batch_seq: 0, sealed: false }
    }

    /// 登记一次失败（先入在册失败区）
    pub fn report_failure(&mut self, case: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if case.is_empty() {
            return Err(ERR_EMPTY_REASON);
        }
        self.active_failures.push(case.to_string());
        Ok(())
    }

    /// 豁免某失败项：移入隔离区
    pub fn exempt(&mut self, case: &str, reason: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if reason.is_empty() {
            return Err(ERR_EMPTY_REASON);
        }
        if self.quarantined.len() >= EXEMPT_CAP {
            return Err(ERR_EXEMPT_OVERFLOW);
        }
        if self.quarantined.iter().any(|q| q.case == case) {
            return Err(ERR_DUP_EXEMPT);
        }
        let pos = match self.active_failures.iter().position(|c| c == case) {
            Some(p) => p,
            None => return Err(ERR_NOT_ISOLATED),
        };
        self.active_failures.remove(pos);
        self.batch_seq += 1;
        self.quarantined.push(ExemptItem {
            case: case.to_string(),
            reason: reason.to_string(),
            quarantine_batch: self.batch_seq,
        });
        Ok(())
    }

    /// 隔离生效：某用例既不在失败区也不重复
    pub fn isolated(&self, case: &str) -> bool {
        self.quarantined.iter().any(|q| q.case == case) && !self.active_failures.iter().any(|c| c == case)
    }

    /// 统计口径：有效失败数（在册失败）
    pub fn effective_failure_count(&self) -> usize {
        self.active_failures.len()
    }

    /// 豁免率（基点）
    pub fn exempt_ratio_bps(&self) -> u32 {
        let total = self.quarantined.len() + self.active_failures.len();
        if total == 0 {
            return 0;
        }
        (self.quarantined.len() * 10000 / total) as u32
    }

    /// 豁免额度余量
    pub fn cap_headroom(&self) -> usize {
        EXEMPT_CAP - self.quarantined.len()
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        let mut acc = 0x10de0u32;
        for (i, q) in self.quarantined.iter().enumerate() {
            acc = acc.wrapping_mul(4649).wrapping_add((q.case.len() as u32) << (i % 4));
            acc = acc.wrapping_add(q.quarantine_batch as u32);
            acc = acc.wrapping_add(q.reason.len() as u32);
        }
        acc = acc.wrapping_add(self.active_failures.len() as u32);
        acc
    }

    /// 诊断面：隔离理由平均长度
    pub fn avg_reason_len(&self) -> usize {
        if self.quarantined.is_empty() {
            return 0;
        }
        let sum: usize = self.quarantined.iter().map(|q| q.reason.len()).sum();
        sum / self.quarantined.len()
    }

    /// 诊断面：某用例豁免理由
    pub fn reason_of(&self, case: &str) -> Option<&String> {
        self.quarantined.iter().find(|q| q.case == case).map(|q| &q.reason)
    }

    /// 诊断面：隔离批次清单
    pub fn quarantine_batches(&self) -> Vec<usize> {
        self.quarantined.iter().map(|q| q.quarantine_batch).collect()
    }

    /// 诊断面：总口径（在册+隔离）
    pub fn total_seen(&self) -> usize {
        self.quarantined.len() + self.active_failures.len()
    }

    /// 诊断面：在册失败清单
    pub fn active_failure_list(&self) -> Vec<String> {
        self.active_failures.clone()
    }

    /// 诊断面：隔离区是否含某理由关键词
    pub fn any_reason_contains(&self, kw: &str) -> bool {
        self.quarantined.iter().any(|q| q.reason.contains(kw))
    }

    /// 诊断面：空闸判定
    pub fn is_empty(&self) -> bool {
        self.quarantined.is_empty() && self.active_failures.is_empty()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 空理由豁免拒绝
    let mut g1 = ExemptGate::new();
    let _ = g1.report_failure("f1");
    out[0] = g1.exempt("f1", "") == Err(ERR_EMPTY_REASON);
    // 2: 未失败项直接豁免拒绝
    let mut g2 = ExemptGate::new();
    out[1] = g2.exempt("ghost", "no such failure") == Err(ERR_NOT_ISOLATED);
    // 3: 重复豁免拒绝
    let mut g3 = ExemptGate::new();
    let _ = g3.report_failure("f3");
    let _ = g3.exempt("f3", "r3");
    out[2] = g3.exempt("f3", "again") == Err(ERR_DUP_EXEMPT);
    // 4: 豁免额度耗尽拒绝
    let mut g4 = ExemptGate::new();
    for i in 0..EXEMPT_CAP {
        let c = g4.active_failures.len();
        let _ = g4.active_failures.insert(c, format!("f-{}", i));
    }
    let mut last_err = Ok(());
    for i in 0..EXEMPT_CAP {
        last_err = g4.exempt(&format!("f-{}", i), "bulk");
    }
    let _ = last_err;
    let _ = g4.report_failure("overflow");
    out[3] = g4.exempt("overflow", "too many") == Err(ERR_EXEMPT_OVERFLOW);
    // 5: 隔离未生效检出：失败仍在册
    let mut g5 = ExemptGate::new();
    let _ = g5.report_failure("f5");
    out[4] = !g5.isolated("f5");
    out
}

/// 判据函数
pub fn run_unx10214_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10214");
    let mut g = ExemptGate::new();
    let _ = g.report_failure("flaky-net");
    let _ = g.report_failure("hard-bug");
    let _ = g.report_failure("env-noise");
    s.add("三失败在册", g.active_failures.len() == 3, "三条失败先入在册区");
    let e1 = g.exempt("env-noise", "环境噪声非内核缺陷");
    s.add("豁免入隔离区", e1.is_ok() && g.quarantined.len() == 1, "豁免条目入隔离区");
    s.add("隔离生效", g.isolated("env-noise"), "豁免项不在失败区");
    s.add("有效失败减一", g.effective_failure_count() == 2, "统计口径剔除豁免");
    s.add("豁免率三分之一", g.exempt_ratio_bps() == 3333, "1/3 豁免率 3333 基点");
    s.add("条目自洽", g.quarantined.iter().all(|q| q.ok()), "隔离条目字段自洽");
    s.add("理由均长大于零", g.avg_reason_len() > 0, "豁免理由均长非零");
    s.add("额度余量十五", g.cap_headroom() == EXEMPT_CAP - 1, "额度余量正确");
    s.add("未豁免失败不混入", g.active_failures.contains(&"hard-bug".to_string()), "未豁免失败保留在册");
    s.add("理由查询", g.reason_of("env-noise").is_some() && g.reason_of("hard-bug").is_none(), "豁免项可查理由、未豁免为 None");
    s.add("隔离批次递增", g.quarantine_batches() == vec![1usize], "首笔隔离批次号为 1");
    s.add("总口径三笔", g.total_seen() == 3, "在册 2+隔离 1 共 3");
    s.add("在册清单两条", g.active_failure_list().len() == 2, "在册失败清单两条");
    s.add("理由关键词命中", g.any_reason_contains("环境"), "隔离理由含环境关键词");
    s.add("空闸判定", ExemptGate::new().is_empty() && !g.is_empty(), "新闸为空、工作闸非空");
    s.add("逐条豁免批次递增", {
        let mut g2 = ExemptGate::new();
        let _ = g2.report_failure("q1");
        let _ = g2.report_failure("q2");
        let _ = g2.exempt("q1", "r1");
        let _ = g2.exempt("q2", "r2");
        g2.quarantine_batches() == vec![1, 2]
    }, "两笔豁免批次号依次为 1、2");
    s.add("额度余量递减", {
        let mut g3 = ExemptGate::new();
        let _ = g3.report_failure("z1");
        let h0 = g3.cap_headroom();
        let _ = g3.exempt("z1", "zz");
        h0 - 1 == g3.cap_headroom()
    }, "豁免一笔余量减一");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let seal_v = g.seal();
    s.add("封账后豁免拒绝", g.exempt("hard-bug", "late") == Err(ERR_SEALED), "封账后操作被拒");
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exempt_moves_case_out_of_failures() {
        let mut g = ExemptGate::new();
        let _ = g.report_failure("x");
        assert!(g.exempt("x", "known flake").is_ok());
        assert!(g.isolated("x"));
        assert_eq!(g.effective_failure_count(), 0);
    }

    #[test]
    fn dup_exempt_rejected() {
        let mut g = ExemptGate::new();
        let _ = g.report_failure("y");
        let _ = g.exempt("y", "r");
        assert_eq!(g.exempt("y", "r2"), Err(ERR_DUP_EXEMPT));
    }

    #[test]
    fn reason_and_batches_query() {
        let mut g = ExemptGate::new();
        let _ = g.report_failure("a");
        let _ = g.report_failure("b");
        let _ = g.exempt("a", "flake");
        assert_eq!(g.reason_of("a"), Some(&"flake".to_string()));
        assert_eq!(g.quarantine_batches(), vec![1]);
        assert_eq!(g.total_seen(), 2);
        assert_eq!(g.avg_reason_len(), 5);
    }

    #[test]
    fn unknown_case_exempt_rejected() {
        let mut g = ExemptGate::new();
        assert_eq!(g.exempt("ghost", "nope"), Err(ERR_NOT_ISOLATED));
        assert!(g.quarantined.is_empty());
    }

    #[test]
    fn batches_increment_and_reason_kw() {
        let mut g = ExemptGate::new();
        let _ = g.report_failure("a");
        let _ = g.report_failure("b");
        let _ = g.exempt("a", "known flake");
        let _ = g.exempt("b", "env drift");
        assert_eq!(g.quarantine_batches(), vec![1, 2]);
        assert!(g.any_reason_contains("flake"));
        assert!(g.any_reason_contains("drift"));
        assert_eq!(g.total_seen(), 2);
    }

    // 收尾注记：豁免迁移保证在册区与隔离区互斥，分母口径随迁随变。
}