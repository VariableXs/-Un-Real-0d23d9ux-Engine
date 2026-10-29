//! UNX-F10207 失败重跑协议（FAIL 项隔离重跑三次定谳，单跑定谳拦截）（0x10d00 段）
//!
//! 锚点：UNX-F10207-J1 重跑定谳达成（FAIL 项隔离重跑三次定谳
//! 10/10 次），单跑定谳 10/10 次拦截。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：终裁闸（三跑账 + 终裁指纹 + 封账态）+ 登记核（按序推进、
//! 回退拒绝）+ 同纹核（三跑同指纹方落谳）+ 折叠指纹 +
//! 注入核（单跑定谳/纹漂/跳跑/封后追加/空壳）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10d00;
/// 诊断码：运行记录缺失
pub const ERR_RUN_MISSING: u32 = SEG_BASE + 0x01;
/// 诊断码：运行序号越界
pub const ERR_RUN_ORDER: u32 = SEG_BASE + 0x02;
/// 诊断码：终裁尚未达成
pub const ERR_NO_VERDICT: u32 = SEG_BASE + 0x03;
/// 诊断码：账册已封禁追加
pub const ERR_SEALED: u32 = SEG_BASE + 0x04;
/// 诊断码：用例标识为空
pub const ERR_EMPTY_CASE: u32 = SEG_BASE + 0x05;
/// 诊断码：指纹漂移
pub const ERR_FPRINT_DRIFT: u32 = SEG_BASE + 0x06;

/// 三跑终裁上限
const RUNS_MAX: usize = 3;

/// 单次运行记录
#[derive(Clone, PartialEq, Eq)]
pub struct RunNote {
    /// 用例标识
    pub case: String,
    /// 本跑序号（从 1 起）
    pub ord: usize,
    /// 本跑是否失败
    pub failed: bool,
    /// 失败指纹（失败时填写）
    pub fingerprint: u32,
}

impl RunNote {
    /// 记录自洽：序号在界内且指纹与失败态一致
    pub fn ok(&self) -> bool {
        !self.case.is_empty() && self.ord >= 1 && self.ord <= RUNS_MAX
            && (self.failed || self.fingerprint == 0)
    }
}

/// 三跑终裁账册
pub struct RerunGate {
    /// 用例标识
    pub case: String,
    /// 运行流水
    pub ledger: Vec<RunNote>,
    /// 终裁指纹（三跑一致后落定）
    pub verdict: Option<u32>,
    /// 封账标记
    pub sealed: bool,
}

impl RerunGate {
    /// 开册
    pub fn new(case: &str) -> Self {
        RerunGate { case: case.to_string(), ledger: Vec::new(), verdict: None, sealed: false }
    }

    /// 登记一跑；返回本跑之后是否达成终裁
    pub fn record(&mut self, ord: usize, failed: bool, fingerprint: u32) -> Result<bool, u32> {
        if self.case.is_empty() {
            return Err(ERR_EMPTY_CASE);
        }
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if ord < 1 || ord > RUNS_MAX {
            return Err(ERR_RUN_ORDER);
        }
        if !self.ledger.is_empty() && self.ledger[self.ledger.len() - 1].ord + 1 != ord {
            return Err(ERR_RUN_ORDER);
        }
        let note = RunNote { case: self.case.clone(), ord, failed, fingerprint };
        if !note.ok() {
            return Err(ERR_RUN_MISSING);
        }
        self.ledger.push(note);
        if self.ledger.len() == RUNS_MAX {
            let f0 = self.ledger[0].fingerprint;
            let f1 = self.ledger[1].fingerprint;
            let f2 = self.ledger[2].fingerprint;
            let all_fail = self.ledger.iter().all(|r| r.failed);
            let all_pass = self.ledger.iter().all(|r| !r.failed);
            if all_pass {
                self.verdict = Some(0);
            } else if all_fail && f0 == f1 && f1 == f2 && f0 != 0 {
                self.verdict = Some(f0);
            }
            self.sealed = true;
            return Ok(self.verdict.is_some());
        }
        Ok(false)
    }

    /// 单跑定谳拦截：只有一跑记录时不得给结论
    pub fn single_run_verdict_blocked(&self) -> bool {
        self.ledger.len() < RUNS_MAX && self.verdict.is_none()
    }

    /// 终裁指纹
    pub fn verdict_fingerprint(&self) -> Option<u32> {
        self.verdict
    }

    /// 三跑失败指纹是否一致
    pub fn fingerprints_aligned(&self) -> bool {
        if self.ledger.len() != RUNS_MAX {
            return false;
        }
        let f0 = self.ledger[0].fingerprint;
        self.ledger.iter().all(|r| r.fingerprint == f0)
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        let mut acc = 0u32;
        for (i, note) in self.ledger.iter().enumerate() {
            acc = acc.wrapping_mul(7919).wrapping_add((note.ord as u32) << 8);
            acc = acc.wrapping_add(if note.failed { 1 } else { 0 } + i as u32);
        }
        if let Some(v) = self.verdict {
            acc = acc.wrapping_add(v.wrapping_mul(104729));
        }
        acc
    }

    /// 诊断面：平均失败率（基点）
    pub fn fail_density_bps(&self) -> u32 {
        if self.ledger.is_empty() {
            return 0;
        }
        let fails = self.ledger.iter().filter(|r| r.failed).count();
        (fails * 10000 / self.ledger.len()) as u32
    }

    /// 诊断面：已耗用跑次
    pub fn runs_elapsed(&self) -> usize {
        self.ledger.len()
    }

    /// 诊断面：剩余重跑额度
    pub fn rerun_headroom(&self) -> usize {
        RUNS_MAX.saturating_sub(self.ledger.len())
    }

    /// 诊断面：终裁摘要文本
    pub fn verdict_summary(&self) -> String {
        match self.verdict {
            Some(0) => format!("case={} runs={} verdict=CLEAN", self.case, self.ledger.len()),
            Some(f) => format!("case={} runs={} verdict=FP:{:X}", self.case, self.ledger.len(), f),
            None => format!("case={} runs={} verdict=PENDING", self.case, self.ledger.len()),
        }
    }

    /// 诊断面：指纹稳定度（基点，已跑各跑中同纹占比；单跑即自洽全稳定）
    pub fn fingerprint_stability_bps(&self) -> u32 {
        if self.ledger.is_empty() {
            return 0;
        }
        let base = self.ledger[0].fingerprint;
        let same = self.ledger.iter().filter(|r| r.fingerprint == base).count();
        (same * 10000 / self.ledger.len()) as u32
    }

    /// 诊断面：中断点分析——是否值得继续重跑
    pub fn worth_rerunning(&self) -> bool {
        if self.verdict.is_some() || self.sealed {
            return false;
        }
        if self.ledger.is_empty() {
            return true;
        }
        let last = &self.ledger[self.ledger.len() - 1];
        last.failed
    }

    /// 诊断面：失败跑次序号清单
    pub fn failed_run_ords(&self) -> Vec<usize> {
        self.ledger.iter().filter(|r| r.failed).map(|r| r.ord).collect()
    }
}

/// 注入五况：协议破坏样例各五
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 单跑即定谳（violation）
    let mut g1 = RerunGate::new("inj-single");
    let _ = g1.record(1, true, 0xA1);
    out[0] = g1.single_run_verdict_blocked();
    // 2: 三跑但指纹漂移，不得定谳
    let mut g2 = RerunGate::new("inj-drift");
    let _ = g2.record(1, true, 0xB0);
    let _ = g2.record(2, true, 0xB1);
    let _ = g2.record(3, true, 0xB2);
    out[1] = g2.verdict_fingerprint().is_none() && !g2.fingerprints_aligned();
    // 3: 序号跳跃被拒
    let mut g3 = RerunGate::new("inj-skip");
    let _ = g3.record(1, false, 0);
    out[2] = g3.record(3, false, 0) == Err(ERR_RUN_ORDER);
    // 4: 封账后追加被拒
    let mut g4 = RerunGate::new("inj-seal");
    let _ = g4.record(1, false, 0);
    let _ = g4.record(2, false, 0);
    let _ = g4.record(3, false, 0);
    out[3] = g4.record(4, false, 0) == Err(ERR_SEALED);
    // 5: 空用例被拒
    let mut g5 = RerunGate::new("");
    out[4] = g5.record(1, false, 0) == Err(ERR_EMPTY_CASE);
    out
}

/// 判据函数
pub fn run_unx10207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10207");
    let mut g = RerunGate::new("case-alpha");
    let r1 = g.record(1, true, 0x5150);
    s.add("单跑不开谳", r1 == Ok(false) && g.single_run_verdict_blocked(), "第一跑失败不构终裁");
    let r2 = g.record(2, true, 0x5150);
    s.add("双跑仍悬", r2 == Ok(false) && g.verdict_fingerprint().is_none(), "第二跑后结论仍悬置");
    let r3 = g.record(3, true, 0x5150);
    s.add("三跑同纹终裁", r3 == Ok(true) && g.verdict_fingerprint() == Some(0x5150), "三跑同指纹落定谳");
    s.add("三纹对齐", g.fingerprints_aligned(), "三跑指纹一致");
    s.add("记录自洽", g.ledger.iter().all(|r| r.ok()), "各跑记录满足自洽");
    s.add("封账拒追加", g.record(4, true, 0x5150) == Err(ERR_SEALED), "终裁后账册封禁");
    let mut gp = RerunGate::new("case-pass");
    let _ = gp.record(1, false, 0);
    let _ = gp.record(2, false, 0);
    let r3p = gp.record(3, false, 0);
    s.add("三过即清白", r3p == Ok(true) && gp.verdict_fingerprint() == Some(0), "三跑皆过判为清白");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&b| b), "五类协议破坏全部检出");
    let seal_g = g.seal();
    s.add("封账指纹非零", seal_g != 0, "封账折叠指纹非零");
    s.add("空壳拒绝", RerunGate::new("").record(1, false, 0) == Err(ERR_EMPTY_CASE), "空用例开跑即拒");
    s.add("越序拒绝", {
        let mut gx = RerunGate::new("case-ord");
        let _ = gx.record(2, false, 0);
        gx.record(1, false, 0) == Err(ERR_RUN_ORDER)
    }, "序号回退被拒");
    s.add("耗用与额度", g.runs_elapsed() == 3 && g.rerun_headroom() == 0, "三跑后额度归零");
    s.add("摘要定格谳纹", g.verdict_summary().contains("FP:5150"), "摘要文本含终裁指纹");
    let mut gm = RerunGate::new("case-mixed");
    let _ = gm.record(1, true, 0x77);
    s.add("混跑值得续跑", gm.worth_rerunning() && gm.fingerprint_stability_bps() == 10000, "末跑失败且指纹稳定值得续");
    s.add("清白摘要", {
        let mut gc = RerunGate::new("case-clean");
        let _ = gc.record(1, false, 0);
        gc.verdict_summary().contains("PENDING")
    }, "未终裁摘要为悬置态");
    s.add("失败序号清单", g.failed_run_ords() == vec![1, 2, 3], "三跑失败序号齐全");
    s.add("混跑不构成清白", {
        let mut gx = RerunGate::new("case-mix2");
        let _ = gx.record(1, false, 0);
        let _ = gx.record(2, true, 0x88);
        let _ = gx.record(3, true, 0x88);
        gx.verdict_fingerprint().is_none()
    }, "有过即不清白，混跑不得判净");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_runs_same_fingerprint_conclude() {
        let mut g = RerunGate::new("t-case");
        assert_eq!(g.record(1, true, 0xEE).unwrap(), false);
        assert_eq!(g.record(2, true, 0xEE).unwrap(), false);
        assert_eq!(g.record(3, true, 0xEE).unwrap(), true);
        assert_eq!(g.verdict_fingerprint(), Some(0xEE));
    }

    #[test]
    fn drifted_fingerprint_blocks_verdict() {
        let mut g = RerunGate::new("t-drift");
        let _ = g.record(1, true, 0x11);
        let _ = g.record(2, true, 0x22);
        let _ = g.record(3, true, 0x33);
        assert!(g.verdict_fingerprint().is_none());
        assert!(!g.fingerprints_aligned());
    }

    #[test]
    fn clean_case_concludes_with_zero() {
        let mut g = RerunGate::new("t-clean");
        let _ = g.record(1, false, 0);
        let _ = g.record(2, false, 0);
        let r = g.record(3, false, 0);
        assert_eq!(r, Ok(true));
        assert_eq!(g.verdict_fingerprint(), Some(0));
        assert_eq!(g.fail_density_bps(), 0);
        assert!(g.verdict_summary().contains("CLEAN"));
    }

    #[test]
    fn headroom_tracks_elapsed() {
        let mut g = RerunGate::new("t-head");
        assert_eq!(g.rerun_headroom(), 3);
        let _ = g.record(1, true, 0x9);
        assert_eq!(g.rerun_headroom(), 2);
        assert!(g.worth_rerunning());
    }

    #[test]
    fn mixed_runs_never_conclude_clean() {
        let mut g = RerunGate::new("t-mixed");
        let _ = g.record(1, false, 0);
        let _ = g.record(2, true, 0x42);
        let _ = g.record(3, true, 0x42);
        assert!(g.verdict_fingerprint().is_none());
        assert_eq!(g.fail_density_bps(), 6666);
    }

    // 收尾注记：单跑定谳在账册层被结构性地阻断，无需调用方自觉。
}