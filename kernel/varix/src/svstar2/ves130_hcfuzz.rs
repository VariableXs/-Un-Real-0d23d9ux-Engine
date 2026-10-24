//! VE-F3830 · 高对比 fuzz
//!
//! 映射 fuzz（色值变异/映射规则变异/forced 色板畸形）。
//! 三变异（色值/规则/时序）。四不变量（映射完整/门不豁免/语义保持/白名单有效）。
//! 缺色/语义破 = P0。最小化+闭环。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB2xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB2Code(pub u16);

impl CB2Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const COLOR_MISSING: Self = Self(SEG_BASE + 1);
    pub const SEMANTIC_BROKEN: Self = Self(SEG_BASE + 2);
    pub const GATE_EXEMPT: Self = Self(SEG_BASE + 3);
    pub const WHITELIST_ABUSE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COLOR_MISSING => "缺色 fuzz 检出（P0 红线实测）",
            Self::SEMANTIC_BROKEN => "语义破（P0 复用分级实测）",
            Self::GATE_EXEMPT => "门豁免检出（不变量违例）",
            Self::WHITELIST_ABUSE => "白名单滥用（不变量违例）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 变异级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutLevel {
    ColorValue,
    Rule,
    Timing,
}

/// fuzz 案例
#[derive(Debug, Clone)]
pub struct HcFuzzCase {
    pub id: u32,
    pub level: MutLevel,
    pub map_complete: bool,
    pub gate_enforced: bool,
    pub semantic_ok: bool,
    pub whitelist_ok: bool,
    pub terminated: bool,
}

/// 四不变量
#[derive(Debug, Clone, Copy)]
pub struct FourInvariants {
    pub map_complete: bool,
    pub gate_enforced: bool,
    pub semantic_ok: bool,
    pub whitelist_ok: bool,
}

impl FourInvariants {
    pub fn all_ok(&self) -> bool {
        self.map_complete && self.gate_enforced && self.semantic_ok && self.whitelist_ok
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcFuzzer {
    corpus: Vec<HcFuzzCase>,
    violations: u32,
    watchdog_ok: bool,
}

impl HcFuzzer {
    pub fn new() -> Self {
        Self { corpus: Vec::new(), violations: 0, watchdog_ok: true }
    }

    pub fn add_case(&mut self, c: HcFuzzCase) {
        self.corpus.push(c);
    }

    /// 四不变量检查
    pub fn check_invariants(&mut self) -> Result<FourInvariants, CB2Code> {
        for c in &self.corpus {
            let inv = FourInvariants {
                map_complete: c.map_complete,
                gate_enforced: c.gate_enforced,
                semantic_ok: c.semantic_ok,
                whitelist_ok: c.whitelist_ok,
            };
            if !inv.map_complete {
                self.violations += 1;
                return Err(CB2Code::COLOR_MISSING);
            }
            if !inv.semantic_ok {
                self.violations += 1;
                return Err(CB2Code::SEMANTIC_BROKEN);
            }
            if !inv.gate_enforced {
                self.violations += 1;
                return Err(CB2Code::GATE_EXEMPT);
            }
            if !inv.whitelist_ok {
                self.violations += 1;
                return Err(CB2Code::WHITELIST_ABUSE);
            }
        }
        Ok(FourInvariants { map_complete: true, gate_enforced: true, semantic_ok: true, whitelist_ok: true })
    }

    /// 看门狗
    pub fn assert_watchdog(&self) -> Result<(), CB2Code> {
        if self.corpus.iter().any(|c| !c.terminated) && !self.watchdog_ok {
            Err(CB2Code::GATE_EXEMPT)
        } else {
            Ok(())
        }
    }

    pub fn set_watchdog(&mut self, ok: bool) {
        self.watchdog_ok = ok;
    }

    pub fn violation_count(&self) -> u32 {
        self.violations
    }

    /// 三变异覆盖
    pub fn assert_three_levels(&self) -> bool {
        let has_color = self.corpus.iter().any(|c| c.level == MutLevel::ColorValue);
        let has_rule = self.corpus.iter().any(|c| c.level == MutLevel::Rule);
        let has_timing = self.corpus.iter().any(|c| c.level == MutLevel::Timing);
        has_color && has_rule && has_timing
    }
}

impl Default for HcFuzzer {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves130_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves130_hcfuzz");

    fn good_case(id: u32, level: MutLevel) -> HcFuzzCase {
        HcFuzzCase { id, level, map_complete: true, gate_enforced: true, semantic_ok: true, whitelist_ok: true, terminated: true }
    }

    // 01 缺色 P0
    {
        let mut f = HcFuzzer::new();
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::ColorValue, map_complete: false, gate_enforced: true, semantic_ok: true, whitelist_ok: true, terminated: true });
        chk(&mut set, "C3830-01", matches!(f.check_invariants(), Err(CB2Code::COLOR_MISSING)), "err");
    }
    // 02 语义破 P0
    {
        let mut f = HcFuzzer::new();
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::Rule, map_complete: true, gate_enforced: true, semantic_ok: false, whitelist_ok: true, terminated: true });
        chk(&mut set, "C3830-02", matches!(f.check_invariants(), Err(CB2Code::SEMANTIC_BROKEN)), "err");
    }
    // 03 门豁免
    {
        let mut f = HcFuzzer::new();
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::Timing, map_complete: true, gate_enforced: false, semantic_ok: true, whitelist_ok: true, terminated: true });
        chk(&mut set, "C3830-03", matches!(f.check_invariants(), Err(CB2Code::GATE_EXEMPT)), "err");
    }
    // 04 白名单滥用
    {
        let mut f = HcFuzzer::new();
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::Rule, map_complete: true, gate_enforced: true, semantic_ok: true, whitelist_ok: false, terminated: true });
        chk(&mut set, "C3830-04", matches!(f.check_invariants(), Err(CB2Code::WHITELIST_ABUSE)), "err");
    }
    // 05 全绿
    {
        let mut f = HcFuzzer::new();
        f.add_case(good_case(1, MutLevel::ColorValue));
        chk(&mut set, "C3830-05", f.check_invariants().is_ok(), "err");
    }
    // 06 三变异覆盖
    {
        let mut f = HcFuzzer::new();
        f.add_case(good_case(1, MutLevel::ColorValue));
        f.add_case(good_case(2, MutLevel::Rule));
        f.add_case(good_case(3, MutLevel::Timing));
        chk(&mut set, "C3830-06", f.assert_three_levels(), "err");
    }
    // 07 违例计数
    {
        let mut f = HcFuzzer::new();
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::ColorValue, map_complete: false, gate_enforced: true, semantic_ok: true, whitelist_ok: true, terminated: true });
        let _ = f.check_invariants();
        chk(&mut set, "C3830-07", f.violation_count() == 1, "err");
    }
    // 08 看门狗
    {
        let mut f = HcFuzzer::new();
        f.set_watchdog(false);
        f.add_case(HcFuzzCase { id: 1, level: MutLevel::Timing, map_complete: true, gate_enforced: true, semantic_ok: true, whitelist_ok: true, terminated: false });
        chk(&mut set, "C3830-08", f.assert_watchdog().is_err(), "err");
    }
    // 09 看门狗正常
    {
        let f = HcFuzzer::new();
        chk(&mut set, "C3830-09", f.assert_watchdog().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3830-10", !CB2Code::COLOR_MISSING.explain().is_empty(), "err");
    }

    set
}
