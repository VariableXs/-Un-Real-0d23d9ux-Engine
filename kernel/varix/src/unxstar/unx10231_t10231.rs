//! UNX-F10231 抖动判定（三次复跑波动项定谳，误判真红检出）（0x11000 段）
//!
//! 锚点：UNX-F10231-J1 分离达成（三次复跑波动项定谳
//! 10/10 次），误判真红 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：抖动账（三跑结果序列 + 波动位判定 + 定谳核）+ 真红分离核
//! （波动不得判真红、真红不得判波动）+ 折叠指纹 +
//! 注入核（双跑下判/矛盾定谳/幽灵项/封账/波动误判真红）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x11000;
/// 诊断码：双跑下判
pub const ERR_INSUFFICIENT: u32 = SEG_BASE + 0x01;
/// 诊断码：矛盾定谳
pub const ERR_CONTRADICT: u32 = SEG_BASE + 0x02;
/// 诊断码：幽灵项
pub const ERR_GHOST_CASE: u32 = SEG_BASE + 0x03;
/// 诊断码：封账下判
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：波动误判真红
pub const ERR_FLAKY_AS_RED: u32 = SEG_BASE + 0x05;

/// 定谳结论
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunVerdict {
    /// 稳定过
    StablePass,
    /// 稳定红
    StableRed,
    /// 波动（抖动）
    Flaky,
}

/// 抖动账
pub struct FlakinessBook {
    /// 用例标识
    pub case: String,
    /// 三跑失败序列
    pub runs: Vec<bool>,
    /// 定谳
    pub verdict: Option<RunVerdict>,
    /// 封账
    pub frozen: bool,
}

impl FlakinessBook {
    /// 开账
    pub fn new(case: &str) -> Result<Self, u32> {
        if case.is_empty() {
            return Err(ERR_GHOST_CASE);
        }
        Ok(FlakinessBook { case: case.to_string(), runs: Vec::new(), verdict: None, frozen: false })
    }

    /// 登记一跑
    pub fn record(&mut self, failed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if self.runs.len() >= 3 {
            return Err(ERR_INSUFFICIENT);
        }
        self.runs.push(failed);
        Ok(())
    }

    /// 定谳：三跑同态判稳，否则判波动；双跑拒绝
    pub fn judge(&mut self) -> Result<RunVerdict, u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if self.runs.len() < 3 {
            return Err(ERR_INSUFFICIENT);
        }
        let all_pass = self.runs.iter().all(|r| !r);
        let all_fail = self.runs.iter().all(|r| *r);
        let v = if all_pass {
            RunVerdict::StablePass
        } else if all_fail {
            RunVerdict::StableRed
        } else {
            RunVerdict::Flaky
        };
        if let Some(prev) = self.verdict {
            if prev != v {
                return Err(ERR_CONTRADICT);
            }
            return Ok(v);
        }
        self.verdict = Some(v);
        Ok(v)
    }

    /// 真红分离核：波动定谳不得被当作真红入缺陷账
    pub fn is_real_red(&self) -> Result<bool, u32> {
        match self.verdict {
            Some(RunVerdict::StableRed) => Ok(true),
            Some(RunVerdict::Flaky) => Err(ERR_FLAKY_AS_RED),
            Some(RunVerdict::StablePass) => Ok(false),
            None => Err(ERR_INSUFFICIENT),
        }
    }

    /// 跑次计数
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, r) in self.runs.iter().enumerate() {
            acc = acc.wrapping_mul(3329).wrapping_add(if *r { i as u32 + 1 } else { 0 });
        }
        if let Some(v) = self.verdict {
            acc = acc.wrapping_add(match v {
                RunVerdict::StablePass => 11,
                RunVerdict::StableRed => 13,
                RunVerdict::Flaky => 17,
            });
        }
        acc
    }

    /// 诊断面：用例回读
    pub fn case_name(&self) -> &str {
        &self.case
    }

    /// 诊断面：失败密度（基点）
    pub fn fail_density_bps(&self) -> u32 {
        if self.runs.is_empty() {
            return 0;
        }
        let f = self.runs.iter().filter(|r| **r).count();
        (f * 10000 / self.runs.len()) as u32
    }

    /// 诊断面：波动位（结果序列是否含翻转）
    pub fn has_flip(&self) -> bool {
        self.runs.iter().any(|r| *r) && self.runs.iter().any(|r| !*r)
    }

    /// 诊断面：定谳摘要
    pub fn verdict_summary(&self) -> String {
        let v = match self.verdict {
            Some(RunVerdict::StablePass) => "STABLE_PASS",
            Some(RunVerdict::StableRed) => "STABLE_RED",
            Some(RunVerdict::Flaky) => "FLAKY",
            None => "PENDING",
        };
        format!("case={} runs={} verdict={}", self.case, self.runs.len(), v)
    }

    /// 诊断面：定谳名
    pub fn verdict_name(v: RunVerdict) -> &'static str {
        match v {
            RunVerdict::StablePass => "STABLE_PASS",
            RunVerdict::StableRed => "STABLE_RED",
            RunVerdict::Flaky => "FLAKY",
        }
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 双跑下判拒绝
    let mut b1 = FlakinessBook::new("i1").unwrap();
    let _ = b1.record(true);
    let _ = b1.record(true);
    out[0] = b1.judge() == Err(ERR_INSUFFICIENT);
    // 2: 矛盾定谳检出（外部接账后重判）
    let mut b2 = FlakinessBook::new("i2").unwrap();
    for _ in 0..3 {
        let _ = b2.record(false);
    }
    let _ = b2.judge();
    b2.runs[0] = true;
    out[1] = b2.judge() == Err(ERR_CONTRADICT);
    // 3: 幽灵项拒绝
    out[2] = FlakinessBook::new("").is_err();
    // 4: 封账下判拒绝
    let mut b4 = FlakinessBook::new("i4").unwrap();
    for _ in 0..3 {
        let _ = b4.record(true);
    }
    let _ = b4.judge();
    let _ = b4.seal();
    out[3] = b4.record(false) == Err(ERR_FROZEN) && b4.judge() == Err(ERR_FROZEN);
    // 5: 波动误判真红检出
    let mut b5 = FlakinessBook::new("i5").unwrap();
    let _ = b5.record(true);
    let _ = b5.record(false);
    let _ = b5.record(true);
    let _ = b5.judge();
    out[4] = b5.is_real_red() == Err(ERR_FLAKY_AS_RED);
    out
}

/// 判据函数
pub fn run_unx10231_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10231");
    let mut f = match FlakinessBook::new("ut-flaky-a") {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = f.record(true);
    let _ = f.record(false);
    s.add("双跑不下判", f.judge() == Err(ERR_INSUFFICIENT), "少于三跑定谳被拒");
    let _ = f.record(true);
    let v1 = f.judge();
    s.add("波动定谳", v1 == Ok(RunVerdict::Flaky), "两败一过判波动");
    s.add("波动不判真红", f.is_real_red() == Err(ERR_FLAKY_AS_RED), "波动项不得入真红账");
    s.add("复判同果", f.judge() == Ok(RunVerdict::Flaky), "同证据复判同果");
    let mut r = FlakinessBook::new("ut-red-b").unwrap();
    for _ in 0..3 {
        let _ = r.record(true);
    }
    s.add("真红定谳", r.judge() == Ok(RunVerdict::StableRed) && r.is_real_red() == Ok(true), "三败判真红且可入缺陷账");
    let mut p = FlakinessBook::new("ut-pass-c").unwrap();
    for _ in 0..3 {
        let _ = p.record(false);
    }
    s.add("稳定过定谳", p.judge() == Ok(RunVerdict::StablePass) && p.is_real_red() == Ok(false), "三过判稳定过");
    s.add("跑次计数", f.run_count() == 3 && p.run_count() == 3, "各账三跑留痕");
    s.add("四跑拒绝", p.record(false) == Err(ERR_INSUFFICIENT), "超出三跑额度被拒");
    s.add("用例回读", f.case_name() == "ut-flaky-a", "用例标识一致");
    s.add("失败密度", f.fail_density_bps() == 6666 && p.fail_density_bps() == 0, "波动 2/3、稳定过 0");
    s.add("波动位翻转", f.has_flip() && !p.has_flip() && !r.has_flip(), "仅波动账含翻转");
    s.add("定谳摘要", f.verdict_summary().contains("verdict=FLAKY") && p.verdict_summary().contains("STABLE_PASS"), "摘要含定谳名");
    s.add("定谳名核", FlakinessBook::verdict_name(RunVerdict::Flaky) == "FLAKY" && FlakinessBook::verdict_name(RunVerdict::StableRed) == "STABLE_RED", "定谳名正确");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = f.seal();
    s.add("封账拒绝", f.record(false) == Err(ERR_FROZEN), "封账后留痕被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flaky_never_real_red() {
        let mut b = FlakinessBook::new("t").unwrap();
        let _ = b.record(false);
        let _ = b.record(true);
        let _ = b.record(false);
        assert_eq!(b.judge(), Ok(RunVerdict::Flaky));
        assert!(b.is_real_red().is_err());
    }

    #[test]
    fn stable_red_is_real() {
        let mut b = FlakinessBook::new("t2").unwrap();
        for _ in 0..3 {
            let _ = b.record(true);
        }
        assert_eq!(b.judge(), Ok(RunVerdict::StableRed));
        assert_eq!(b.is_real_red(), Ok(true));
    }

    #[test]
    fn run_count_and_judge_repeat() {
        let mut b = FlakinessBook::new("t3").unwrap();
        let _ = b.record(true);
        let _ = b.record(false);
        let _ = b.record(false);
        assert_eq!(b.run_count(), 3);
        assert_eq!(b.judge(), Ok(RunVerdict::Flaky));
        assert_eq!(b.judge(), Ok(RunVerdict::Flaky));
        assert!(b.is_real_red().is_err());
    }

    #[test]
    fn ghost_case_rejected() {
        assert!(FlakinessBook::new("").is_err());
        let mut b = FlakinessBook::new("ok").unwrap();
        let _ = b.record(false);
        let _ = b.record(false);
        let _ = b.record(false);
        let _ = b.judge();
        let _ = b.seal();
        assert_eq!(b.judge(), Err(ERR_FROZEN));
    }

    #[test]
    fn density_and_flip_matrix() {
        let mut a = FlakinessBook::new("m1").unwrap();
        let _ = a.record(true);
        let _ = a.record(true);
        let _ = a.record(false);
        assert_eq!(a.fail_density_bps(), 6666);
        assert!(a.has_flip());
        let mut b = FlakinessBook::new("m2").unwrap();
        for _ in 0..3 {
            let _ = b.record(true);
        }
        assert_eq!(b.fail_density_bps(), 10000);
        assert!(!b.has_flip());
    }

    #[test]
    fn verdict_names_and_summary() {
        assert_eq!(FlakinessBook::verdict_name(RunVerdict::StablePass), "STABLE_PASS");
        assert_eq!(FlakinessBook::verdict_name(RunVerdict::Flaky), "FLAKY");
        let mut b = FlakinessBook::new("nm").unwrap();
        assert!(b.verdict_summary().contains("verdict=PENDING"));
        assert_eq!(b.case_name(), "nm");
    }

    #[test]
    fn stable_pass_summary_and_density() {
        let mut b = FlakinessBook::new("sp").unwrap();
        for _ in 0..3 {
            let _ = b.record(false);
        }
        let _ = b.judge();
        assert!(b.verdict_summary().contains("runs=3"));
        assert!(b.verdict_summary().contains("STABLE_PASS"));
        assert_eq!(b.fail_density_bps(), 0);
        assert!(!b.has_flip());
    }
}
