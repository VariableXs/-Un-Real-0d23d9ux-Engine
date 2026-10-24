// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves96_ecotest
// VE-F3696 · 生态测试（R05 组 · 生态运营组）
//
// 锚点：VE-F3696
// 判据原文：受损 P0、沙盘前置、A/B 伦理、八类断言、经济模拟、判据。
//
// 难点分析：
//   1. A/B 实验红线——实验组体验受损=拿用户做实验，受损禁令是最高级 P0。
//   2. 经济变更必须先沙盘后上线，沙盘跳过即拦截。
//   3. 八类断言（指标口径/结算正确/评分模型/推荐公平/治理流程/活动奖励/反刷有效/隐私合规）是运营系统回归底线。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9000 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9000;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C90Code(pub u16);

impl C90Code {
    pub const EXPERIMENT_HARM: Self = Self(SEG_BASE);      // 实验组受损 P0
    pub const SANDBOX_SKIPPED: Self = Self(SEG_BASE + 1);  // 沙盘跳过
    pub const ASSERT_FAIL: Self = Self(SEG_BASE + 2);      // 八类断言失败
    pub const ECON_UNBALANCED: Self = Self(SEG_BASE + 3);  // 经济失衡
    pub const CONTROL_UNDERSPEC: Self = Self(SEG_BASE + 4); // 对照组保底不足
    pub const SANDBOX_PASS: Self = Self(SEG_BASE + 5);     // 沙盘通过
    pub const ETHICS_PASS: Self = Self(SEG_BASE + 6);      // 伦理审计通过

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::EXPERIMENT_HARM => "实验组体验受损——受损禁令 P0（红线最高级）",
            Self::SANDBOX_SKIPPED => "经济变更未过沙盘——沙盘前置红线拦截",
            Self::ASSERT_FAIL => "八类断言失败——运营回归底线",
            Self::ECON_UNBALANCED => "经济模拟失衡——平衡验证未过",
            Self::CONTROL_UNDERSPEC => "对照组保底不足——保底红线",
            Self::SANDBOX_PASS => "沙盘模拟通过，允许上线",
            Self::ETHICS_PASS => "A/B 伦理审计通过",
            _ => "未知诊断",
        }
    }
}

// ── 八类断言 ──────────────────────────────────────────────────────

/// 断言类别（八类全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertClass {
    /// 指标口径。
    Metric,
    /// 结算正确。
    Settle,
    /// 评分模型。
    Rating,
    /// 推荐公平。
    Fairness,
    /// 治理流程。
    Governance,
    /// 活动奖励。
    Reward,
    /// 反刷有效。
    AntiSpam,
    /// 隐私合规。
    Privacy,
}

impl AssertClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::Metric => "指标口径",
            Self::Settle => "结算正确",
            Self::Rating => "评分模型",
            Self::Fairness => "推荐公平",
            Self::Governance => "治理流程",
            Self::Reward => "活动奖励",
            Self::AntiSpam => "反刷有效",
            Self::Privacy => "隐私合规",
        }
    }

    /// 八类全量。
    pub fn all() -> [AssertClass; 8] {
        [
            Self::Metric, Self::Settle, Self::Rating, Self::Fairness,
            Self::Governance, Self::Reward, Self::AntiSpam, Self::Privacy,
        ]
    }
}

/// 断言生成器。
pub struct AssertGenerator {
    /// 每类是否已生成。
    generated: [bool; 8],
    /// 每类通过/失败。
    results: [Option<bool>; 8],
}

impl AssertGenerator {
    pub fn new() -> Self {
        Self { generated: [false; 8], results: [None; 8] }
    }

    fn idx(c: AssertClass) -> usize {
        match c {
            AssertClass::Metric => 0,
            AssertClass::Settle => 1,
            AssertClass::Rating => 2,
            AssertClass::Fairness => 3,
            AssertClass::Governance => 4,
            AssertClass::Reward => 5,
            AssertClass::AntiSpam => 6,
            AssertClass::Privacy => 7,
        }
    }

    /// 生成某类断言。
    pub fn generate(&mut self, c: AssertClass) {
        self.generated[Self::idx(c)] = true;
    }

    /// 记录某类结果。
    pub fn record(&mut self, c: AssertClass, ok: bool) -> Result<(), C90Code> {
        let i = Self::idx(c);
        self.generated[i] = true;
        self.results[i] = Some(ok);
        if !ok {
            return Err(C90Code::ASSERT_FAIL);
        }
        Ok(())
    }

    /// 全类已生成？
    pub fn all_generated(&self) -> bool {
        self.generated.iter().all(|&b| b)
    }

    /// 全类通过？
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|&r| r == Some(true))
    }

    /// 通过类数。
    pub fn pass_count(&self) -> usize {
        self.results.iter().filter(|&&r| r == Some(true)).count()
    }
}

impl Default for AssertGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ── 经济沙盘 ──────────────────────────────────────────────────────

/// 经济参数变更。
pub struct EconChange {
    /// 分成比例（基点，10000=100%）。
    pub split_bp: u16,
    /// 奖励池。
    pub reward_pool: u64,
    /// 参与人数假设。
    pub participants: u32,
}

impl EconChange {
    pub fn new(split_bp: u16, reward_pool: u64, participants: u32) -> Self {
        Self { split_bp, reward_pool, participants }
    }
}

/// 沙盘模拟器——变更上线前必过。
pub struct EconSandbox {
    /// 已过沙盘的变更指纹。
    approved: Vec<u64>,
    /// 被拦截的跳过次数。
    skips_blocked: u32,
}

impl EconSandbox {
    pub fn new() -> Self {
        Self { approved: Vec::new(), skips_blocked: 0 }
    }

    /// 简易指纹。
    pub fn fingerprint(c: &EconChange) -> u64 {
        (c.split_bp as u64) << 32 ^ c.reward_pool ^ (c.participants as u64) << 16
    }

    /// 跑沙盘：分成比例不能超过 50%，奖励池不能为 0，人均不能为 0。
    pub fn simulate(&mut self, c: &EconChange) -> Result<(), C90Code> {
        if c.reward_pool == 0 || c.participants == 0 {
            return Err(C90Code::ECON_UNBALANCED);
        }
        if c.split_bp > 5000 {
            return Err(C90Code::ECON_UNBALANCED);
        }
        self.approved.push(Self::fingerprint(c));
        Ok(())
    }

    /// 上线检查：必须已过沙盘。
    pub fn gate_launch(&mut self, c: &EconChange) -> Result<(), C90Code> {
        let fp = Self::fingerprint(c);
        if self.approved.contains(&fp) {
            return Ok(());
        }
        self.skips_blocked = self.skips_blocked.saturating_add(1);
        Err(C90Code::SANDBOX_SKIPPED)
    }

    pub fn skips_blocked(&self) -> u32 {
        self.skips_blocked
    }
}

impl Default for EconSandbox {
    fn default() -> Self {
        Self::new()
    }
}

// ── A/B 伦理 ──────────────────────────────────────────────────────

/// 实验组体验指标。
pub struct ExperienceMetric {
    /// 崩溃率（基点）。
    pub crash_bp: u16,
    /// 延迟劣化（基点）。
    pub latency_bp: u16,
    /// 功能可用率（基点）。
    pub availability_bp: u16,
}

impl ExperienceMetric {
    pub fn new(crash_bp: u16, latency_bp: u16, availability_bp: u16) -> Self {
        Self { crash_bp, latency_bp, availability_bp }
    }

    /// 是否「受损」：崩溃率或延迟超阈值，或可用率低于基线。
    pub fn is_harmed(&self) -> bool {
        self.crash_bp > 100 || self.latency_bp > 2000 || self.availability_bp < 9500
    }

    /// 对照组保底是否达标（可用率 ≥ 95% 且崩溃 ≤ 1%）。
    pub fn control_ok(&self) -> bool {
        self.availability_bp >= 9500 && self.crash_bp <= 100
    }
}

/// A/B 伦理审计器。
pub struct AbEthicsAuditor {
    /// 受损检出数。
    harms: u32,
    /// 保底不足数。
    underspec: u32,
}

impl AbEthicsAuditor {
    pub fn new() -> Self {
        Self { harms: 0, underspec: 0 }
    }

    /// 审计实验组：受损即 P0。
    pub fn audit_experiment(&mut self, m: &ExperienceMetric) -> Result<(), C90Code> {
        if m.is_harmed() {
            self.harms = self.harms.saturating_add(1);
            return Err(C90Code::EXPERIMENT_HARM);
        }
        Ok(())
    }

    /// 审计对照组：保底不达即红线。
    pub fn audit_control(&mut self, m: &ExperienceMetric) -> Result<(), C90Code> {
        if !m.control_ok() {
            self.underspec = self.underspec.saturating_add(1);
            return Err(C90Code::CONTROL_UNDERSPEC);
        }
        Ok(())
    }

    /// 双审计计。
    pub fn audit_both(&mut self, exp: &ExperienceMetric, ctl: &ExperienceMetric) -> Result<(), C90Code> {
        self.audit_experiment(exp)?;
        self.audit_control(ctl)?;
        Ok(())
    }

    pub fn harms(&self) -> u32 {
        self.harms
    }

    pub fn underspec(&self) -> u32 {
        self.underspec
    }
}

impl Default for AbEthicsAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 生态测试总线 ──────────────────────────────────────────────────

/// 生态测试总线。
pub struct EcoTestBus {
    pub asserts: AssertGenerator,
    pub sandbox: EconSandbox,
    pub ethics: AbEthicsAuditor,
}

impl EcoTestBus {
    pub fn new() -> Self {
        Self {
            asserts: AssertGenerator::new(),
            sandbox: EconSandbox::new(),
            ethics: AbEthicsAuditor::new(),
        }
    }

    /// 全绿判定：八类全过 + 伦理零违规。
    pub fn all_green(&self) -> bool {
        self.asserts.all_passed() && self.ethics.harms() == 0 && self.ethics.underspec() == 0
    }
}

impl Default for EcoTestBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves96_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves96_ecotest");
    {
        // 01 八类全量可生成
        let mut g = AssertGenerator::new();
        for c in AssertClass::all() {
            g.generate(c);
        }
        chk(&mut set, "C390-01", g.all_generated() && AssertClass::all().len() == 8, "八类断言全量生成");
    }
    {
        // 02 断言失败报错
        let mut g = AssertGenerator::new();
        let r = g.record(AssertClass::Settle, false);
        chk(
            &mut set,
            "C390-02",
            r.err() == Some(C90Code::ASSERT_FAIL),
            "断言失败须报 ASSERT_FAIL",
        );
    }
    {
        // 03 全类通过判定
        let mut g = AssertGenerator::new();
        for c in AssertClass::all() {
            assert!(g.record(c, true).is_ok());
        }
        chk(&mut set, "C390-03", g.all_passed() && g.pass_count() == 8, "八类全过才 all_passed");
    }
    {
        // 04 沙盘通过后可上线
        let mut sb = EconSandbox::new();
        let c = EconChange::new(3000, 10000, 100);
        assert!(sb.simulate(&c).is_ok());
        let r = sb.gate_launch(&c);
        chk(&mut set, "C390-04", r.is_ok(), "沙盘通过→放行上线");
    }
    {
        // 05 沙盘跳过即拦截
        let mut sb = EconSandbox::new();
        let c = EconChange::new(3000, 10000, 100);
        let r = sb.gate_launch(&c);
        chk(
            &mut set,
            "C390-05",
            r.err() == Some(C90Code::SANDBOX_SKIPPED) && sb.skips_blocked() == 1,
            "未过沙盘即上线须拦截（沙盘前置红线）",
        );
    }
    {
        // 06 分成超限失衡
        let mut sb = EconSandbox::new();
        let c = EconChange::new(6000, 10000, 100); // 60%
        let r = sb.simulate(&c);
        chk(&mut set, "C390-06", r.err() == Some(C90Code::ECON_UNBALANCED), "分成>50% 经济失衡");
    }
    {
        // 07 实验组受损即 P0
        let mut a = AbEthicsAuditor::new();
        let harm = ExperienceMetric::new(500, 100, 9000); // 崩溃 5% + 可用 90%
        let r = a.audit_experiment(&harm);
        chk(
            &mut set,
            "C390-07",
            r.err() == Some(C90Code::EXPERIMENT_HARM) && a.harms() == 1,
            "实验组受损=P0（受损禁令红线最高级）",
        );
    }
    {
        // 08 对照组保底
        let mut a = AbEthicsAuditor::new();
        let bad_ctl = ExperienceMetric::new(200, 100, 9000);
        let r = a.audit_control(&bad_ctl);
        chk(
            &mut set,
            "C390-08",
            r.err() == Some(C90Code::CONTROL_UNDERSPEC),
            "对照组保底不足须拦截（保底红线）",
        );
    }
    {
        // 09 双审计全绿
        let mut a = AbEthicsAuditor::new();
        let ok = ExperienceMetric::new(50, 500, 9900);
        let r = a.audit_both(&ok, &ok);
        chk(&mut set, "C390-09", r.is_ok(), "实验+对照双绿放行");
    }
    {
        // 10 空池失衡
        let mut sb = EconSandbox::new();
        let c = EconChange::new(1000, 0, 100);
        chk(
            &mut set,
            "C390-10",
            sb.simulate(&c).err() == Some(C90Code::ECON_UNBALANCED),
            "空奖励池经济失衡",
        );
    }
    {
        // 11 总线全绿
        let mut bus = EcoTestBus::new();
        for c in AssertClass::all() {
            assert!(bus.asserts.record(c, true).is_ok());
        }
        let ok = ExperienceMetric::new(10, 100, 9950);
        assert!(bus.ethics.audit_both(&ok, &ok).is_ok());
        chk(&mut set, "C390-11", bus.all_green(), "总线全绿判定正确");
    }
    {
        // 12 指纹区分不同变更
        let c1 = EconChange::new(1000, 100, 10);
        let c2 = EconChange::new(2000, 100, 10);
        chk(
            &mut set,
            "C390-12",
            EconSandbox::fingerprint(&c1) != EconSandbox::fingerprint(&c2),
            "变更指纹区分不同参数",
        );
    }
    set
}
