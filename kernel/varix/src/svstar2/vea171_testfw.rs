//! VE-F0171 · 渲染单元测试框架（VE-Test · 目标 420 行）
//!
//! 铁律：用例可登记，断言可累计，预算可约束，隔离可传染，裁决可追溯。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8F1;
pub const XC_TEST_FAILED: u16 = SEG_BASE | 0x01;
pub const XC_CASE_DUPLICATE: u16 = SEG_BASE | 0x02;
pub const XC_CASE_MISSING: u16 = SEG_BASE | 0x03;
pub const XC_BUDGET_EXCEEDED: u16 = SEG_BASE | 0x04;
pub const XC_SUITE_EMPTY: u16 = SEG_BASE | 0x05;
pub const XC_ASSERT_MISMATCH: u16 = SEG_BASE | 0x06;
pub const XC_QUARANTINED: u16 = SEG_BASE | 0x07;
pub const XC_FRAME_DRIFT: u16 = SEG_BASE | 0x08;

/// 用例裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
    Skip,
    Quarantine,
}

/// 渲染用例登记项。
#[derive(Clone, Copy)]
pub struct RenderCase {
    pub id: u32,
    pub suite: u16,
    pub budget_us: u32,
    pub expected_hash: u64,
    pub quarantined: bool,
    pub critical: bool,
}

/// 单条用例执行结果。
#[derive(Clone, Copy)]
pub struct CaseResult {
    pub id: u32,
    pub verdict: Verdict,
    pub actual_hash: u64,
    pub elapsed_us: u32,
    pub code: u16,
}

/// 断言累计器：一次运行内所有断言的通过/失败账。
#[derive(Clone, Copy)]
pub struct AssertStat {
    pub total: u64,
    pub failed: u64,
}

impl AssertStat {
    pub const fn new() -> Self {
        Self { total: 0, failed: 0 }
    }
    pub fn record(&mut self, ok: bool) {
        self.total += 1;
        if !ok {
            self.failed += 1;
        }
    }
    pub fn reset(&mut self) {
        self.total = 0;
        self.failed = 0;
    }
    /// 通过率（基点，万分制；空账按全过计）。
    pub fn pass_rate_bps(&self) -> u32 {
        if self.total == 0 {
            return 10_000;
        }
        (((self.total - self.failed) * 10_000) / self.total) as u32
    }
}

/// 渲染单元测试框架。
pub struct RenderTestFramework {
    cases: Vec<RenderCase>,
    results: Vec<CaseResult>,
    stat: AssertStat,
    runs: u32,
    suite_filter: u16,
    use_filter: bool,
    last_code: u16,
}

impl RenderTestFramework {
    pub fn new() -> Self {
        Self {
            cases: Vec::new(),
            results: Vec::new(),
            stat: AssertStat::new(),
            runs: 0,
            suite_filter: 0,
            use_filter: false,
            last_code: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 登记用例；同 id 二次登记判重。
    pub fn register(&mut self, case: RenderCase) -> Result<u32, u16> {
        if self.cases.iter().any(|c| c.id == case.id) {
            self.last_code = XC_CASE_DUPLICATE;
            return Err(XC_CASE_DUPLICATE);
        }
        self.cases.push(case);
        self.last_code = 0;
        Ok(case.id)
    }

    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 取登记项只读引用。
    pub fn get(&self, id: u32) -> Option<&RenderCase> {
        self.cases.iter().find(|c| c.id == id)
    }

    /// 标记/解除隔离：隔离用例不进主裁决，防止传染。
    pub fn quarantine(&mut self, id: u32, on: bool) -> Result<bool, u16> {
        for c in self.cases.iter_mut() {
            if c.id == id {
                c.quarantined = on;
                self.last_code = 0;
                return Ok(true);
            }
        }
        self.last_code = XC_CASE_MISSING;
        Err(XC_CASE_MISSING)
    }

    /// 设定套件过滤；`clear` 为真时恢复全量运行。
    pub fn filter_suite(&mut self, suite: u16, clear: bool) {
        self.suite_filter = suite;
        self.use_filter = !clear;
    }

    /// 单条用例执行：hash 对不齐 / 超预算 / 隔离，分别给出裁决与诊断码。
    pub fn run_case(&mut self, id: u32, actual_hash: u64, elapsed_us: u32) -> Result<Verdict, u16> {
        let idx = match self.cases.iter().position(|c| c.id == id) {
            Some(i) => i,
            None => {
                self.last_code = XC_CASE_MISSING;
                return Err(XC_CASE_MISSING);
            }
        };
        let case = self.cases[idx];
        let (verdict, code) = if case.quarantined {
            (Verdict::Quarantine, XC_QUARANTINED)
        } else if elapsed_us > case.budget_us {
            (Verdict::Fail, XC_BUDGET_EXCEEDED)
        } else if actual_hash != case.expected_hash {
            (Verdict::Fail, XC_ASSERT_MISMATCH)
        } else {
            (Verdict::Pass, 0)
        };
        self.results.push(CaseResult { id, verdict, actual_hash, elapsed_us, code });
        self.stat.record(verdict == Verdict::Pass);
        self.last_code = code;
        Ok(verdict)
    }

    /// 跑整个套件（或过滤集）；入参为 (id, hash, us) 流水。
    pub fn run_suite(&mut self, inputs: &[(u32, u64, u32)]) -> Result<u32, u16> {
        if inputs.is_empty() {
            self.last_code = XC_SUITE_EMPTY;
            return Err(XC_SUITE_EMPTY);
        }
        let mut executed = 0u32;
        for &(id, hash, us) in inputs {
            if self.use_filter {
                let in_suite = match self.cases.iter().find(|c| c.id == id) {
                    Some(c) => c.suite == self.suite_filter,
                    None => {
                        self.last_code = XC_CASE_MISSING;
                        return Err(XC_CASE_MISSING);
                    }
                };
                if !in_suite {
                    continue;
                }
            }
            self.run_case(id, hash, us)?;
            executed += 1;
        }
        if executed == 0 {
            self.last_code = XC_SUITE_EMPTY;
            return Err(XC_SUITE_EMPTY);
        }
        self.runs += 1;
        self.last_code = 0;
        Ok(executed)
    }

    /// 帧漂移断言：两帧 hash 不一致即记失败（供关键路径点名断言）。
    pub fn assert_frame_stable(&mut self, a: u64, b: u64) -> bool {
        let ok = a == b;
        self.stat.record(ok);
        if !ok {
            self.last_code = XC_FRAME_DRIFT;
        }
        ok
    }

    /// 通用布尔断言。
    pub fn assert_true(&mut self, cond: bool) -> bool {
        self.stat.record(cond);
        if !cond {
            self.last_code = XC_ASSERT_MISMATCH;
        }
        cond
    }

    /// 期望 hash 断言（登记外点验）。
    pub fn assert_hash(&mut self, expected: u64, actual: u64) -> bool {
        let ok = expected == actual;
        self.stat.record(ok);
        if !ok {
            self.last_code = XC_ASSERT_MISMATCH;
        }
        ok
    }

    /// 预算断言：耗时不得超过上限。
    pub fn assert_budget(&mut self, elapsed_us: u32, budget_us: u32) -> bool {
        let ok = elapsed_us <= budget_us;
        self.stat.record(ok);
        if !ok {
            self.last_code = XC_BUDGET_EXCEEDED;
        }
        ok
    }

    pub fn runs(&self) -> u32 {
        self.runs
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// 最近一条结果。
    pub fn last_result(&self) -> Option<&CaseResult> {
        self.results.last()
    }

    /// 按 id 查结果。
    pub fn result_of(&self, id: u32) -> Option<&CaseResult> {
        self.results.iter().find(|r| r.id == id)
    }

    pub fn assert_stat(&self) -> &AssertStat {
        &self.stat
    }

    /// 失败断言数。
    pub fn failed(&self) -> u64 {
        self.stat.failed
    }

    /// 结果层通过率（基点）：隔离与跳过不计入分母。
    pub fn verdict_rate_bps(&self) -> u32 {
        let judged: Vec<&CaseResult> = self
            .results
            .iter()
            .filter(|r| r.verdict == Verdict::Pass || r.verdict == Verdict::Fail)
            .collect();
        if judged.is_empty() {
            return 10_000;
        }
        let pass = judged.iter().filter(|r| r.verdict == Verdict::Pass).count();
        ((pass as u64 * 10_000) / judged.len() as u64) as u32
    }

    /// 关键用例是否全过；无关键用例按全过计。
    pub fn critical_green(&self) -> bool {
        for c in self.cases.iter().filter(|c| c.critical) {
            match self.result_of(c.id) {
                Some(r) => {
                    if r.verdict == Verdict::Fail {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 清空结果账（保留登记），便于下一版本重跑。
    pub fn reset_results(&mut self) {
        self.results.clear();
        self.stat.reset();
        self.last_code = 0;
    }

    /// 总评：任一失败用例即整体 Fail（隔离除外）。
    pub fn overall(&self) -> Verdict {
        if self.results.iter().any(|r| r.verdict == Verdict::Fail) {
            Verdict::Fail
        } else if self.results.is_empty() {
            Verdict::Skip
        } else {
            Verdict::Pass
        }
    }
}

pub fn run_vea171_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("vea171_testfw");
    let mk = |id: u32, suite: u16| RenderCase {
        id,
        suite,
        budget_us: 1000,
        expected_hash: 0xA11CE,
        quarantined: false,
        critical: id < 100,
    };
    {
        let f = RenderTestFramework::new();
        chk(&mut set, "C171-01", f.case_count() == 0 && f.runs() == 0, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        let r = f.register(mk(1, 0));
        chk(&mut set, "C171-02", r.is_ok() && f.case_count() == 1, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        let r = f.register(mk(1, 0));
        chk(&mut set, "C171-03", r.err() == Some(XC_CASE_DUPLICATE), "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        let v = f.run_case(1, 0xA11CE, 10);
        chk(&mut set, "C171-04", v == Ok(Verdict::Pass), "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        let v = f.run_case(1, 0xBAD, 10);
        chk(&mut set, "C171-05", v == Ok(Verdict::Fail) && f.last_code() == XC_ASSERT_MISMATCH, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        let v = f.run_case(1, 0xA11CE, 99999);
        chk(&mut set, "C171-06", v == Ok(Verdict::Fail) && f.last_code() == XC_BUDGET_EXCEEDED, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        f.quarantine(1, true);
        let v = f.run_case(1, 0xBAD, 10);
        chk(&mut set, "C171-07", v == Ok(Verdict::Quarantine), "err");
    }
    {
        let mut f = RenderTestFramework::new();
        let r = f.run_case(42, 0, 0);
        chk(&mut set, "C171-08", r.err() == Some(XC_CASE_MISSING), "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(mk(1, 0));
        f.register(mk(2, 0));
        let n = f.run_suite(&[(1, 0xA11CE, 5), (2, 0xA11CE, 5)]);
        chk(&mut set, "C171-09", n == Ok(2) && f.result_count() == 2, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        let r = f.run_suite(&[]);
        chk(&mut set, "C171-10", r.err() == Some(XC_SUITE_EMPTY), "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(RenderCase { id: 7, suite: 0, budget_us: 10, expected_hash: 1, quarantined: false, critical: true });
        f.run_case(7, 2, 1);
        chk(&mut set, "C171-11", !f.critical_green() && f.overall() == Verdict::Fail, "err");
    }
    {
        let mut f = RenderTestFramework::new();
        f.register(RenderCase { id: 7, suite: 0, budget_us: 10, expected_hash: 1, quarantined: false, critical: true });
        f.run_case(7, 1, 1);
        chk(&mut set, "C171-12", f.critical_green() && f.verdict_rate_bps() == 10_000, "err");
    }
    set
}
