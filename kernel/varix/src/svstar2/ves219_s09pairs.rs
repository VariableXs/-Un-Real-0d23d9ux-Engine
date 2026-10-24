//! VE-F3965 · 组间两两对账
//!
//! 八组间 28 对两两对账（O09/P09/Q09/R09 对账机制第九用声明——家族治理
//! 复用）。对账五维：接口引用一致/色标口径一致/心流指标口径一致/降级链
//! 衔接/测试语料共享。对账证据挂指针。
//! 错误路径：口径分歧→仲裁（家族仲裁流程复述）；引用断裂→补链；缺对→
//! 补齐。
//! 性能：对账 O(28×维)；仲裁 O(分歧)；补链 O(断点)。
//! 跨批对接：八组档案；F3962-F3964 核验上游；F3973 汇总表下游。
//! 无障碍与隐私：口径对账含双维无障碍口径专列（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3140）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3140;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9ECode(pub u16);

impl S9ECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CALIBER_SPLIT: Self = Self(SEG_BASE + 1);
    pub const REF_BREAK: Self = Self(SEG_BASE + 2);
    pub const PAIR_MISSING: Self = Self(SEG_BASE + 3);
    pub const MECH_OVERUSE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CALIBER_SPLIT => "口径分歧→仲裁",
            Self::REF_BREAK => "引用断裂→补链",
            Self::PAIR_MISSING => "缺对→补齐",
            Self::MECH_OVERUSE => "机制越用→第十用须另立",
            _ => "未知",
        }
    }
}

// ── 28 对对账矩阵 ────────────────────────────────────────────────────

/// 组数 8 → 两两组合数 28。
pub const GROUPS: usize = 8;
pub const PAIRS: usize = 28;

/// 组号对 → 对下标（C(n,2) 组合展开）。
pub fn pair_index(a: usize, b: usize) -> Option<usize> {
    if a == b || a >= GROUPS || b >= GROUPS {
        return None;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    // sum_{k=1}^{lo} (8-k) + (hi - lo - 1)
    let mut idx = 0;
    for k in 0..lo {
        idx += GROUPS - 1 - k;
    }
    Some(idx + (hi - lo - 1))
}

/// 五维口径。
pub const FIVE_DIMS: [&str; 5] = ["接口引用", "色标口径", "心流口径", "降级链衔接", "语料共享"];

/// 对账记录：对 × 五维结论。
#[derive(Debug, Clone)]
pub struct PairAudit {
    pub pair: usize,
    /// 五维通过状态（下标对齐 FIVE_DIMS）。
    pub dims: [bool; 5],
    /// 证据指针。
    pub evidence: Option<&'static str>,
}

/// 对账矩阵。
#[derive(Debug, Clone)]
pub struct PairMatrix {
    pub rows: Vec<PairAudit>,
    /// 机制使用计数（第九用上限）。
    pub mech_uses: u8,
}

pub const MECH_USE_LIMIT: u8 = 9;

impl PairMatrix {
    pub fn new() -> Self {
        Self { rows: Vec::new(), mech_uses: 0 }
    }

    pub fn use_mechanism(&mut self) -> u8 {
        self.mech_uses += 1;
        self.mech_uses
    }

    pub fn mech_legal(&self) -> bool {
        self.mech_uses <= MECH_USE_LIMIT
    }

    /// 登记对账行（缺对→补齐：重复登记拒收）。
    pub fn record(&mut self, a: PairAudit) -> S9ECode {
        if a.pair >= PAIRS {
            return S9ECode::PAIR_MISSING;
        }
        if self.rows.iter().any(|r| r.pair == a.pair) {
            return S9ECode::PAIR_MISSING;
        }
        self.rows.push(a);
        S9ECode::OK
    }

    /// 缺对清单。
    pub fn missing_pairs(&self) -> Vec<usize> {
        (0..PAIRS).filter(|p| !self.rows.iter().any(|r| r.pair == *p)).collect()
    }

    /// 口径分歧检出：任一维不过的行。
    pub fn splits(&self) -> Vec<usize> {
        self.rows.iter().filter(|r| r.dims.iter().any(|d| !d)).map(|r| r.pair).collect()
    }

    /// 证据缺清单。
    pub fn no_evidence(&self) -> Vec<usize> {
        self.rows.iter().filter(|r| r.evidence.is_none()).map(|r| r.pair).collect()
    }

    pub fn complete(&self) -> bool {
        self.missing_pairs().is_empty() && self.splits().is_empty() && self.no_evidence().is_empty()
    }
}

impl Default for PairMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ── 家族仲裁（口径分歧）──────────────────────────────────────────────

/// 仲裁记录：分歧对 → 裁决值 + 留痕。
#[derive(Debug, Clone)]
pub struct Arbitration {
    pub pair: usize,
    pub dim: usize,
    /// 裁决采纳方（0=甲方 1=乙方）。
    pub adopt: u8,
    pub note: &'static str,
}

/// 仲裁台账：留痕断言（note 非空）。
#[derive(Debug, Clone)]
pub struct ArbBook {
    pub items: Vec<Arbitration>,
}

impl ArbBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, a: Arbitration) -> S9ECode {
        if a.note.is_empty() {
            return S9ECode::CALIBER_SPLIT;
        }
        self.items.push(a);
        S9ECode::OK
    }

    pub fn has(&self, pair: usize, dim: usize) -> bool {
        self.items.iter().any(|a| a.pair == pair && a.dim == dim)
    }
}

impl Default for ArbBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 引用补链 ─────────────────────────────────────────────────────────

/// 断链修补：登记断点 → 补链确认。
#[derive(Debug, Clone)]
pub struct RefChain {
    /// 断点（pair, dim）。
    pub breaks: Vec<(usize, usize)>,
    /// 已补链。
    pub mended: Vec<(usize, usize)>,
}

impl RefChain {
    pub fn new() -> Self {
        Self { breaks: Vec::new(), mended: Vec::new() }
    }

    pub fn report(&mut self, pair: usize, dim: usize) {
        self.breaks.push((pair, dim));
    }

    pub fn mend(&mut self, pair: usize, dim: usize) -> S9ECode {
        if let Some(pos) = self.breaks.iter().position(|b| *b == (pair, dim)) {
            self.breaks.remove(pos);
            self.mended.push((pair, dim));
            S9ECode::OK
        } else {
            S9ECode::REF_BREAK
        }
    }

    pub fn clean(&self) -> bool {
        self.breaks.is_empty()
    }
}

impl Default for RefChain {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves219_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves219_s09pairs");

    // 01 28 对守恒：pair_index 双射
    {
        let mut seen = [false; PAIRS];
        let mut all_ok = true;
        for a in 0..GROUPS {
            for b in (a + 1)..GROUPS {
                match pair_index(a, b) {
                    Some(i) if !seen[i] => seen[i] = true,
                    _ => all_ok = false,
                }
            }
        }
        chk(&mut set, "C3965-01", PAIRS == 28 && all_ok && seen.iter().all(|s| *s), "err");
    }
    // 02 非法对拒收
    {
        chk(&mut set, "C3965-02", pair_index(3, 3).is_none() && pair_index(8, 0).is_none() && pair_index(9, 1).is_none(), "err");
    }
    // 03 对称对同下标
    {
        chk(&mut set, "C3965-03", pair_index(2, 5) == pair_index(5, 2) && pair_index(0, 7).is_some(), "err");
    }
    // 04 五维口径守恒
    {
        chk(&mut set, "C3965-04", FIVE_DIMS.len() == 5 && FIVE_DIMS[4] == "语料共享", "err");
    }
    // 05 缺对→补齐实测
    {
        let mut m = PairMatrix::new();
        m.record(PairAudit { pair: 0, dims: [true; 5], evidence: Some("e") });
        let miss = m.missing_pairs();
        chk(&mut set, "C3965-05", miss.len() == 27 && !m.complete() && m.record(PairAudit { pair: 0, dims: [true; 5], evidence: Some("e") }) == S9ECode::PAIR_MISSING, "err");
    }
    // 06 口径分歧检出→仲裁
    {
        let mut m = PairMatrix::new();
        let mut dims = [true; 5];
        dims[2] = false;
        m.record(PairAudit { pair: 3, dims, evidence: Some("e") });
        let splits = m.splits();
        let mut arb = ArbBook::new();
        chk(&mut set, "C3965-06", splits == vec![3] && arb.add(Arbitration { pair: 3, dim: 2, adopt: 0, note: "以甲方为准" }) == S9ECode::OK && arb.has(3, 2), "err");
    }
    // 07 仲裁留痕必填
    {
        let mut arb = ArbBook::new();
        chk(&mut set, "C3965-07", arb.add(Arbitration { pair: 1, dim: 0, adopt: 1, note: "" }) == S9ECode::CALIBER_SPLIT, "err");
    }
    // 08 引用断裂→补链实测
    {
        let mut c = RefChain::new();
        c.report(4, 1);
        chk(&mut set, "C3965-08", !c.clean() && c.mend(4, 1) == S9ECode::OK && c.clean(), "err");
    }
    // 09 补不存在断点拒收
    {
        let mut c = RefChain::new();
        chk(&mut set, "C3965-09", c.mend(9, 9) == S9ECode::REF_BREAK, "err");
    }
    // 10 机制第九用上限
    {
        let mut m = PairMatrix::new();
        for _ in 0..9 {
            m.use_mechanism();
        }
        chk(&mut set, "C3965-10", m.mech_legal() && { m.use_mechanism(); !m.mech_legal() }, "err");
    }
    // 11 证据缺清单
    {
        let mut m = PairMatrix::new();
        m.record(PairAudit { pair: 1, dims: [true; 5], evidence: None });
        m.record(PairAudit { pair: 2, dims: [true; 5], evidence: Some("指针") });
        chk(&mut set, "C3965-11", m.no_evidence() == vec![1], "err");
    }
    // 12 全量登记后矩阵完备
    {
        let mut m = PairMatrix::new();
        for p in 0..PAIRS {
            m.record(PairAudit { pair: p, dims: [true; 5], evidence: Some("e") });
        }
        chk(&mut set, "C3965-12", m.complete() && m.rows.len() == 28, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3965-13", S9ECode::MECH_OVERUSE.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9ECode::OK, S9ECode::CALIBER_SPLIT, S9ECode::REF_BREAK, S9ECode::PAIR_MISSING, S9ECode::MECH_OVERUSE];
        chk(&mut set, "C3965-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_pairs_all_green() {
        let s = run_ves219_checks();
        assert!(s.all_passed(), "ves219 red: {}", s.red_items_text());
    }

    #[test]
    fn test_pair_index_edges() {
        assert_eq!(pair_index(0, 1), Some(0));
        assert_eq!(pair_index(0, 7), Some(7));
        assert_eq!(pair_index(1, 2), Some(8));
        assert_eq!(pair_index(6, 7), Some(27));
    }

    #[test]
    fn test_matrix_full_cycle() {
        let mut m = PairMatrix::new();
        for p in 0..PAIRS {
            m.record(PairAudit { pair: p, dims: [true; 5], evidence: Some("x") }).ok();
        }
        assert!(m.complete());
        assert!(m.missing_pairs().is_empty());
    }
}
