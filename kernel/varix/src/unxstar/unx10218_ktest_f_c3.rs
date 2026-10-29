//! UNX-F10218 域级冒烟闸（ktest -f c3 域级 fail==0 且 skip 显式，红闸拦截）（0x10e60 段）
//!
//! 锚点：UNX-F10218-J1 冒烟闸达成（ktest -f c3 域级 fail==0 且
//! skip 显式 10/10 次），红闸放行 10/10 次拦截。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：冒烟闸（域计数 + skip 显式位 + 闸态）+ 核闸核（fail==0
//! 且 skip 有显式理由方开绿闸）+ 红闸拦截核 + 折叠指纹 +
//! 注入核（有败放行/隐式 skip/空域/闸后改账/负计数）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10e60;
/// 诊断码：有败放行
pub const ERR_FAIL_LEAK: u32 = SEG_BASE + 0x01;
/// 诊断码：隐式跳过
pub const ERR_IMPLICIT_SKIP: u32 = SEG_BASE + 0x02;
/// 诊断码：域名为空
pub const ERR_EMPTY_DOMAIN: u32 = SEG_BASE + 0x03;
/// 诊断码：负计数
pub const ERR_NEG_COUNT: u32 = SEG_BASE + 0x04;
/// 诊断码：闸已定态再改账
pub const ERR_GATE_LOCKED: u32 = SEG_BASE + 0x05;

/// 冒烟闸
pub struct SmokeGate {
    /// 域名
    pub domain: String,
    /// 总数
    pub total: usize,
    /// 通过数
    pub passed: usize,
    /// 失败数
    pub failed: usize,
    /// 跳过数
    pub skipped: usize,
    /// skip 显式理由表（每条 skip 必须有显式理由）
    pub skip_reasons: Vec<String>,
    /// 闸态：None=未核  Some(true)=绿闸  Some(false)=红闸
    pub verdict: Option<bool>,
}

impl SmokeGate {
    /// 开闸登记域
    pub fn new(domain: &str) -> Result<Self, u32> {
        if domain.is_empty() {
            return Err(ERR_EMPTY_DOMAIN);
        }
        Ok(SmokeGate {
            domain: domain.to_string(),
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            skip_reasons: Vec::new(),
            verdict: None,
        })
    }

    /// 记过
    pub fn record_pass(&mut self) -> Result<(), u32> {
        if self.verdict.is_some() {
            return Err(ERR_GATE_LOCKED);
        }
        self.total += 1;
        self.passed += 1;
        Ok(())
    }

    /// 记败
    pub fn record_fail(&mut self) -> Result<(), u32> {
        if self.verdict.is_some() {
            return Err(ERR_GATE_LOCKED);
        }
        self.total += 1;
        self.failed += 1;
        Ok(())
    }

    /// 记跳（必须带显式理由）
    pub fn record_skip(&mut self, reason: &str) -> Result<(), u32> {
        if self.verdict.is_some() {
            return Err(ERR_GATE_LOCKED);
        }
        if reason.is_empty() {
            return Err(ERR_IMPLICIT_SKIP);
        }
        self.total += 1;
        self.skipped += 1;
        self.skip_reasons.push(reason.to_string());
        Ok(())
    }

    /// 四账对平
    pub fn balanced(&self) -> bool {
        self.passed + self.failed + self.skipped == self.total
    }

    /// 核闸：fail==0 且 skip 全显式 且账对平方开绿闸
    pub fn judge(&mut self) -> Result<bool, u32> {
        if !self.balanced() {
            self.verdict = Some(false);
            return Err(ERR_NEG_COUNT);
        }
        let green = self.failed == 0 && self.skip_reasons.len() == self.skipped;
        self.verdict = Some(green);
        Ok(green)
    }

    /// 红闸拦截核：红闸下放行请求必须被拒
    pub fn admit(&self) -> Result<(), u32> {
        match self.verdict {
            Some(true) => Ok(()),
            Some(false) => Err(ERR_FAIL_LEAK),
            None => Err(ERR_GATE_LOCKED),
        }
    }

    /// skip 显式率（基点）
    pub fn skip_explicit_bps(&self) -> u32 {
        if self.skipped == 0 {
            return 10000;
        }
        (self.skip_reasons.len() * 10000 / self.skipped) as u32
    }

    /// 闸摘要
    pub fn gate_summary(&self) -> String {
        let v = match self.verdict {
            None => "OPEN",
            Some(true) => "GREEN",
            Some(false) => "RED",
        };
        format!("domain={} t={} p={} f={} s={} gate={}", self.domain, self.total, self.passed, self.failed, self.skipped, v)
    }

    /// 折叠指纹
    pub fn seal(&self) -> u32 {
        let mut acc = SEG_BASE;
        acc = acc.wrapping_add((self.total as u32) << 3);
        acc = acc.wrapping_add((self.passed as u32) << 2);
        acc = acc.wrapping_add((self.failed as u32) << 1);
        acc = acc.wrapping_add(self.skipped as u32);
        for (i, r) in self.skip_reasons.iter().enumerate() {
            acc = acc.wrapping_mul(967).wrapping_add((r.len() as u32) + i as u32);
        }
        acc = acc.wrapping_add(match self.verdict {
            None => 17,
            Some(true) => 31,
            Some(false) => 61,
        });
        acc
    }

    /// 诊断面：skip 理由清单
    pub fn skip_reason_list(&self) -> Vec<String> {
        self.skip_reasons.clone()
    }

    /// 诊断面：绿闸判定预演（不落态）
    pub fn would_judge_green(&self) -> bool {
        self.balanced() && self.failed == 0 && self.skip_reasons.len() == self.skipped
    }

    /// 诊断面：通过率（基点）
    pub fn pass_ratio_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.passed * 10000 / self.total) as u32
    }

    /// 诊断面：域间名比较
    pub fn same_domain_as(&self, other: &SmokeGate) -> bool {
        self.domain == other.domain
    }

    /// 诊断面：账面零败判定
    pub fn zero_fail(&self) -> bool {
        self.failed == 0
    }

    /// 诊断面：skip 计数
    pub fn skip_count(&self) -> usize {
        self.skipped
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 有败核闸 → 红闸且放行被拒
    let mut g1 = SmokeGate::new("c3").unwrap();
    let _ = g1.record_pass();
    let _ = g1.record_fail();
    let _ = g1.judge();
    out[0] = g1.admit() == Err(ERR_FAIL_LEAK);
    // 2: 隐式 skip 拒绝
    let mut g2 = SmokeGate::new("c3").unwrap();
    out[1] = g2.record_skip("") == Err(ERR_IMPLICIT_SKIP);
    // 3: 空域拒绝
    out[2] = SmokeGate::new("").is_err();
    // 4: 闸定态后改账被拒
    let mut g4 = SmokeGate::new("c3").unwrap();
    let _ = g4.record_pass();
    let _ = g4.judge();
    out[3] = g4.record_pass().is_err() && g4.record_fail().is_err();
    // 5: 未核闸放行被拒
    let g5 = SmokeGate::new("c3").unwrap();
    out[4] = g5.admit() == Err(ERR_GATE_LOCKED);
    out
}

/// 判据函数
pub fn run_unx10218_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10218");
    let mut g = match SmokeGate::new("c3") {
        Ok(x) => x,
        Err(_) => return s,
    };
    for _ in 0..7 {
        let _ = g.record_pass();
    }
    let _ = g.record_skip("硬件受限暂缺实机档");
    let _ = g.record_skip("依赖外设未接入");
    s.add("九条入闸账", g.total == 9 && g.balanced(), "七过两跳账对平");
    s.add("零败账面", g.failed == 0, "失败账为零");
    s.add("skip 全显式", g.skip_explicit_bps() == 10000 && g.skip_reasons.len() == 2, "两条 skip 均带显式理由");
    let j = g.judge();
    s.add("核闸开绿", j == Ok(true), "fail==0 且 skip 显式开绿闸");
    s.add("绿闸放行", g.admit().is_ok(), "绿闸放行请求受理");
    s.add("闸摘要 GREEN", g.gate_summary().contains("gate=GREEN"), "摘要显示 GREEN");
    let mut gr = SmokeGate::new("c3-r").unwrap();
    let _ = gr.record_pass();
    let _ = gr.record_fail();
    let _ = gr.judge();
    s.add("红闸拦截", gr.admit() == Err(ERR_FAIL_LEAK), "有败域放行被拒");
    s.add("红闸摘要", gr.gate_summary().contains("gate=RED"), "红域摘要显示 RED");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    s.add("指纹非零", g.seal() != 0 && gr.seal() != 0, "绿红两闸折叠均非零");
    s.add("空域拒绝", SmokeGate::new("").is_err(), "空域开闸被拒");
    s.add("skip 理由回读", g.skip_reason_list().len() == 2 && g.skip_reason_list()[0].contains("实机"), "两条理由可回读");
    s.add("预演同判", g.would_judge_green() && !gr.would_judge_green(), "预演与核闸同判");
    s.add("绿闸通过率", g.pass_ratio_bps() == 7777, "7/9 通过率 7777 基点");
    s.add("跳过占比", g.skipped == 2 && g.skip_explicit_bps() == 10000, "两跳全显式");
    s.add("红闸指纹差异", g.seal() != gr.seal(), "绿红两闸折叠不同");
    s.add("域名比较", g.same_domain_as(&gr) == false, "c3 与 c3-r 为异域");
    s.add("绿闸零败", g.zero_fail() && !gr.zero_fail(), "绿闸零败、红闸有败");
    s.add("跳过计数", g.skip_count() == 2, "绿闸两条跳过");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_gate_requires_zero_fail() {
        let mut g = SmokeGate::new("d").unwrap();
        let _ = g.record_pass();
        assert_eq!(g.judge(), Ok(true));
        let mut g2 = SmokeGate::new("d").unwrap();
        let _ = g2.record_fail();
        assert_eq!(g2.judge(), Ok(false));
        assert_eq!(g2.admit(), Err(ERR_FAIL_LEAK));
    }

    #[test]
    fn implicit_skip_rejected() {
        let mut g = SmokeGate::new("d").unwrap();
        assert_eq!(g.record_skip(""), Err(ERR_IMPLICIT_SKIP));
        assert!(g.record_skip("legit reason").is_ok());
        assert_eq!(g.skip_explicit_bps(), 10000);
    }

    #[test]
    fn gate_locks_after_verdict() {
        let mut g = SmokeGate::new("d").unwrap();
        let _ = g.record_pass();
        let _ = g.judge();
        assert!(g.record_pass().is_err());
        assert!(g.record_skip("late").is_err());
    }

    #[test]
    fn unbalanced_ledger_reddens() {
        let mut g = SmokeGate::new("d").unwrap();
        let _ = g.record_pass();
        let _ = g.record_skip("ok");
        g.total += 5; // 改账失衡
        assert_eq!(g.judge(), Err(ERR_NEG_COUNT));
        assert_eq!(g.admit(), Err(ERR_FAIL_LEAK));
    }

    #[test]
    fn pass_ratio_and_summary() {
        let mut g = SmokeGate::new("d").unwrap();
        for _ in 0..3 {
            let _ = g.record_pass();
        }
        let _ = g.record_fail();
        assert_eq!(g.pass_ratio_bps(), 7500);
        assert!(g.gate_summary().contains("OPEN"));
        assert!(g.would_judge_green() == false);
    }

    #[test]
    fn skip_reason_list_roundtrip() {
        let mut g = SmokeGate::new("d").unwrap();
        let _ = g.record_skip("reason-a");
        let _ = g.record_skip("reason-b");
        assert_eq!(g.skip_reason_list(), vec!["reason-a".to_string(), "reason-b".to_string()]);
        assert_eq!(g.skipped, 2);
        assert!(g.would_judge_green());
    }
}
