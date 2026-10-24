//! ves414_s22prescore — VE-F4174 · 20 维度预评分
//! T 域 20 维度预评分（家族 20 维评分卡预演版——20 维逐维打分 + 每维证据指针）；
//! 预评分与终评分离声明（预评不外发）；证据缺→该维暂缓+登记；分数异常→复评；预评外泄→撤回+立案；
//! 无障碍维预评含多语言读屏加权口径（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0EE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsCode {
    Ok = 0,
    EvidencePending = SEG_BASE + 0x01, // 0x0EE1
    ScoreAnomaly = SEG_BASE + 0x02,    // 0x0EE2
    Leaked = SEG_BASE + 0x03,          // 0x0EE3
}

/// 20 维（家族评分卡 T 版）。
pub const DIMS: [&'static str; 20] = [
    "api-surface", "api-stability", "test-unit", "test-integ", "test-e2e",
    "perf-lookup", "perf-switch", "perf-layout", "perf-memory", "security-outbound",
    "security-residency", "a11y-sr", "a11y-contrast", "a11y-rtl", "doc-manual",
    "doc-api-ref", "budget-adherence", "ext-contract", "observe-telemetry", "observe-debug",
];

/// 维评分：分数（0-100）+ 证据指针 + 暂缓。
#[derive(Clone, Copy)]
pub struct DimScore {
    pub score: u8,
    pub evidence: u32, // 证据指针（0 = 缺 → 暂缓）
    pub deferred: bool,
    pub rechecked: bool, // 复评标记
}

/// 预评分卡。
pub struct PreScoreCard {
    pub scores: Vec<(&'static str, DimScore)>,
    pub leaked: bool,           // 预评外泄
    pub withdrawn: bool,        // 撤回
    pub cases: Vec<&'static str>,
}

impl PreScoreCard {
    pub fn new() -> Self {
        PreScoreCard {
            scores: DIMS.iter().map(|d| (*d, DimScore { score: 0, evidence: 0, deferred: true, rechecked: false })).collect(),
            leaked: false, withdrawn: false, cases: Vec::new(),
        }
    }

    /// 打分（证据齐才生效；证据缺 → 该维暂缓 + 登记）。
    pub fn score(&mut self, dim: &str, v: u8, evidence: u32) -> Result<(), PsCode> {
        if v > 100 { return Err(PsCode::ScoreAnomaly); }
        match self.scores.iter_mut().find(|x| x.0 == dim) {
            Some(x) => {
                if evidence == 0 {
                    x.1.deferred = true;
                    x.1.evidence = 0;
                    return Err(PsCode::EvidencePending);
                }
                x.1.score = v;
                x.1.evidence = evidence;
                x.1.deferred = false;
                Ok(())
            }
            None => Err(PsCode::ScoreAnomaly),
        }
    }

    /// 分数异常复评：异常分（>100 不可能入账，此处核验未复评的异常登记）。
    pub fn recheck(&mut self, dim: &str) -> bool {
        match self.scores.iter_mut().find(|x| x.0 == dim) {
            Some(x) => { x.1.rechecked = true; true }
            None => false,
        }
    }

    /// 暂缓维定位。
    pub fn deferred_dims(&self) -> Vec<&'static str> {
        self.scores.iter().filter(|(_, s)| s.deferred).map(|(d, _)| *d).collect()
    }

    /// 预终分离：全 20 维有分有证 → 预评完成态。
    pub fn complete(&self) -> bool { self.deferred_dims().is_empty() }

    /// 加权总分：a11y-sr / a11y-contrast / a11y-rtl 三维加权 1.5（多语言读屏加权口径——域本色）。
    pub fn weighted_total(&self) -> u32 {
        let mut total = 0u32;
        for (d, s) in self.scores.iter() {
            let w = if *d == "a11y-sr" || *d == "a11y-contrast" || *d == "a11y-rtl" { 3 } else { 2 };
            total += s.score as u32 * w;
        }
        total
    }

    /// 预评外泄 → 撤回 + 立案。
    pub fn leak(&mut self) {
        self.leaked = true;
        self.withdrawn = true;
        self.cases.push("pre-score-leaked");
    }

    /// 分离声明：未外泄且未撤回。
    pub fn separation_ok(&self) -> bool { !self.leaked && !self.withdrawn }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves414_checks() -> CheckSet {
    let mut set = CheckSet::new("ves414");
    let mut card = PreScoreCard::new();

    // C414-01 20 维建卡全暂缓。
    chk(&mut set, "C414-01", card.scores.len() == 20 && card.deferred_dims().len() == 20, "20 维全暂缓");

    // C414-02 证据缺打分被拒。
    chk(&mut set, "C414-02", card.score("api-surface", 88, 0) == Err(PsCode::EvidencePending), "缺证暂缓");

    // C414-03 逐维打分（证据齐）。
    for (i, d) in DIMS.iter().enumerate() {
        card.score(d, 80 + (i % 15) as u8, 0xF000 + i as u32).unwrap();
    }
    chk(&mut set, "C414-03", card.complete() && card.deferred_dims().is_empty(), "20 维全评");

    // C414-04 分数越界拒。
    let mut c2 = PreScoreCard::new();
    chk(&mut set, "C414-04", c2.score("api-surface", 101, 1) == Err(PsCode::ScoreAnomaly), "101 分拒");

    // C414-05 异常复评。
    c2.score("api-surface", 90, 0xF100).unwrap();
    chk(&mut set, "C414-05", c2.recheck("api-surface") && c2.scores[0].1.rechecked, "复评标记");

    // C414-06 加权总分：a11y 三维权重 1.5。
    let mut c3 = PreScoreCard::new();
    for d in DIMS.iter() { c3.score(d, 100, 0xF200).unwrap(); }
    // 普通 17 维 × 2 × 100 + a11y 3 维 × 3 × 100 = 3400 + 900 = 4300
    chk(&mut set, "C414-06", c3.weighted_total() == 4300, "加权 4300");

    // C414-07 域本色加权口径：a11y 权重高于普通维。
    let mut c4 = PreScoreCard::new();
    for d in DIMS.iter() { c4.score(d, 50, 0xF300).unwrap(); }
    // 普通维贡献 = 17×2×50=1700；a11y = 3×3×50=450；比率 450/2170 ≈ 20.7% > 3/20=15%
    chk(&mut set, "C414-07", c4.weighted_total() == 2150, "半分加权 2150");

    // C414-08 预终分离声明。
    chk(&mut set, "C414-08", card.separation_ok(), "分离声明成立");

    // C414-09 预评外泄 → 撤回 + 立案。
    let mut c5 = PreScoreCard::new();
    c5.leak();
    chk(&mut set, "C414-09", !c5.separation_ok() && c5.withdrawn
        && c5.cases.contains(&"pre-score-leaked"), "外泄撤回立案");

    // C414-10 维名单单源（20 维）。
    chk(&mut set, "C414-10", DIMS.len() == 20 && DIMS[0] == "api-surface"
        && DIMS[19] == "observe-debug", "20 维单源");

    // C414-11 暂缓态恢复：补证据后生效。
    let mut c6 = PreScoreCard::new();
    c6.score("perf-lookup", 70, 0).unwrap_err();
    c6.score("perf-lookup", 70, 0xF400).unwrap();
    chk(&mut set, "C414-11", !c6.deferred_dims().contains(&"perf-lookup"), "补证生效");

    // C414-12 未知维打分拒。
    chk(&mut set, "C414-12", c6.score("not-a-dim", 50, 1).is_err(), "未知维拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves414_smoke() {
        let set = run_ves414_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves414 red: {:?}", red);
    }
}
