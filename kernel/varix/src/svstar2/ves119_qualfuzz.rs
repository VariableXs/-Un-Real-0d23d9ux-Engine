//! VE-F3719 · 质量 fuzz
//!
//! 扫描器 fuzz：畸形主题（极端令牌/循环引用/超大资产）。
//! 三变异（结构/参数/组合）。三不变量（评分有界 0-100 / 扫描终止 / 不崩溃）。
//! 对抗联动（F3714）。评分越界 P1。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA7xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA7Code(pub u16);

impl CA7Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SCORE_OOB: Self = Self(SEG_BASE + 1);
    pub const CRASH: Self = Self(SEG_BASE + 2);
    pub const HANG: Self = Self(SEG_BASE + 3);
    pub const LOOP_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SCORE_OOB => "评分越界（P1 红线实测）：必须 0-100",
            Self::CRASH => "崩溃检出，最小化入库",
            Self::HANG => "挂起检出，看门狗介入",
            Self::LOOP_MISSING => "闭环缺失，阻断",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 变异级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationLevel {
    Structural,
    Parametric,
    Combinatorial,
}

/// fuzz 案例
#[derive(Debug, Clone)]
pub struct FuzzCase {
    pub id: u32,
    pub level: MutationLevel,
    pub score: i32,
    pub terminated: bool,
    pub crashed: bool,
}

/// 三不变量
#[derive(Debug, Clone, Copy)]
pub struct Invariants {
    pub score_bounded: bool,
    pub scan_terminated: bool,
    pub no_crash: bool,
}

impl Invariants {
    pub fn all_ok(&self) -> bool {
        self.score_bounded && self.scan_terminated && self.no_crash
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualFuzzer {
    corpus: Vec<FuzzCase>,
    violations: u32,
    adversarial_linked: bool,
}

impl QualFuzzer {
    pub fn new() -> Self {
        Self { corpus: Vec::new(), violations: 0, adversarial_linked: true }
    }

    pub fn add_case(&mut self, c: FuzzCase) {
        self.corpus.push(c);
    }

    /// 评分有界断言 P1：0-100
    pub fn assert_score_bounded(&self) -> Result<(), CA7Code> {
        for c in &self.corpus {
            if c.score < 0 || c.score > 100 {
                return Err(CA7Code::SCORE_OOB);
            }
        }
        Ok(())
    }

    /// 扫描终止断言
    pub fn assert_terminated(&self) -> Result<(), CA7Code> {
        if self.corpus.iter().any(|c| !c.terminated) {
            Err(CA7Code::HANG)
        } else {
            Ok(())
        }
    }

    /// 不崩溃断言
    pub fn assert_no_crash(&self) -> Result<(), CA7Code> {
        if self.corpus.iter().any(|c| c.crashed) {
            Err(CA7Code::CRASH)
        } else {
            Ok(())
        }
    }

    /// 三不变量总断言
    pub fn check_invariants(&mut self) -> Result<Invariants, CA7Code> {
        let inv = Invariants {
            score_bounded: self.assert_score_bounded().is_ok(),
            scan_terminated: self.assert_terminated().is_ok(),
            no_crash: self.assert_no_crash().is_ok(),
        };
        if inv.all_ok() {
            Ok(inv)
        } else {
            self.record_violation();
            Err(CA7Code::SCORE_OOB)
        }
    }

    pub fn record_violation(&mut self) {
        self.violations += 1;
    }

    /// 对抗联动断言
    pub fn assert_adversarial_link(&self) -> Result<(), CA7Code> {
        if self.adversarial_linked {
            Ok(())
        } else {
            Err(CA7Code::LOOP_MISSING)
        }
    }

    pub fn set_adversarial_link(&mut self, linked: bool) {
        self.adversarial_linked = linked;
    }

    pub fn violation_count(&self) -> u32 {
        self.violations
    }
}

impl Default for QualFuzzer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves119_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves119_qualfuzz");

    // 01 评分越界 P1
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Structural, score: 150, terminated: true, crashed: false });
        chk(&mut set, "C3719-01", f.assert_score_bounded() == Err(CA7Code::SCORE_OOB), "err");
    }
    // 02 评分有界
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Parametric, score: 100, terminated: true, crashed: false });
        chk(&mut set, "C3719-02", f.assert_score_bounded().is_ok(), "err");
    }
    // 03 挂起
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Combinatorial, score: 50, terminated: false, crashed: false });
        chk(&mut set, "C3719-03", f.assert_terminated() == Err(CA7Code::HANG), "err");
    }
    // 04 崩溃
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Structural, score: 50, terminated: true, crashed: true });
        chk(&mut set, "C3719-04", f.assert_no_crash() == Err(CA7Code::CRASH), "err");
    }
    // 05 三不变量全绿
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Structural, score: 80, terminated: true, crashed: false });
        chk(&mut set, "C3719-05", f.check_invariants().is_ok(), "err");
    }
    // 06 不变量违例
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Structural, score: -1, terminated: true, crashed: false });
        chk(&mut set, "C3719-06", f.check_invariants().is_err(), "err");
    }
    // 07 对抗联动缺失
    {
        let mut f = QualFuzzer::new();
        f.set_adversarial_link(false);
        chk(&mut set, "C3719-07", f.assert_adversarial_link() == Err(CA7Code::LOOP_MISSING), "err");
    }
    // 08 对抗联动正常
    {
        let f = QualFuzzer::new();
        chk(&mut set, "C3719-08", f.assert_adversarial_link().is_ok(), "err");
    }
    // 09 违例计数
    {
        let mut f = QualFuzzer::new();
        f.add_case(FuzzCase { id: 1, level: MutationLevel::Structural, score: 200, terminated: true, crashed: false });
        let _ = f.check_invariants();
        chk(&mut set, "C3719-09", f.violation_count() == 1, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3719-10", !CA7Code::SCORE_OOB.explain().is_empty(), "err");
    }

    set
}
