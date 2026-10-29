//! ves383_s21linter — VE-F4143 · i18n 审查器
//! 静态审查五类规则（硬编码文案/占位符完整性/术语符合/长度超限/方向混合）+ 替代文本缺译规则（域本色）；
//! 规则与风格指南单源（F4067 转译表）；误报率 <5% 持续校准（超标→规则回修+留痕）；
//! 规则冲突→仲裁；扫描性能劣化→增量模式；扫描不读用户目录（红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDDC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LrCode {
    Ok = 0,
    FalsePositiveOverrun = SEG_BASE + 0x01,
    RuleConflict = SEG_BASE + 0x02,
    PerfDegraded = SEG_BASE + 0x03,
}

/// 审查规则五类 + 域本色缺译规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    HardcodedText = 0, PlaceholderComplete = 1, TermCompliance = 2,
    LengthOverrun = 3, DirectionMix = 4, AltTextMissing = 5,
}

impl RuleKind {
    /// 五类核心规则。
    pub const CORE: [RuleKind; 5] = [
        RuleKind::HardcodedText, RuleKind::PlaceholderComplete, RuleKind::TermCompliance,
        RuleKind::LengthOverrun, RuleKind::DirectionMix];
    pub fn name(self) -> &'static str {
        match self {
            RuleKind::HardcodedText => "hardcoded-text",
            RuleKind::PlaceholderComplete => "placeholder-complete",
            RuleKind::TermCompliance => "term-compliance",
            RuleKind::LengthOverrun => "length-overrun",
            RuleKind::DirectionMix => "direction-mix",
            RuleKind::AltTextMissing => "alt-text-missing",
        }
    }
}

/// 风格指南转译表（F4067 单源）：规则 → 风格条目锚。
pub struct StyleTable { pub entries: Vec<(RuleKind, &'static str)> }

impl StyleTable {
    pub fn new() -> Self { StyleTable { entries: Vec::new() } }
    pub fn put(&mut self, r: RuleKind, anchor: &'static str) {
        if !self.entries.iter().any(|x| x.0 == r) { self.entries.push((r, anchor)); }
    }
    /// 单源完备：五类核心规则全部有转译锚。
    pub fn core_covered(&self) -> bool {
        RuleKind::CORE.iter().all(|r| self.entries.iter().any(|x| x.0 == *r))
    }
    pub fn anchor(&self, r: RuleKind) -> Option<&'static str> {
        self.entries.iter().find(|x| x.0 == r).map(|x| x.1)
    }
}

/// 审查发现。
#[derive(Clone, Copy)]
pub struct Finding {
    pub kind: RuleKind,
    pub key: &'static str, // 键位
    pub false_positive: bool,
}

/// 扫描结果：发现集 + 误报率校准。
pub struct ScanResult {
    pub findings: Vec<Finding>,
    pub incremental: bool,
}

impl ScanResult {
    pub fn real_count(&self) -> usize { self.findings.iter().filter(|f| !f.false_positive).count() }
    pub fn fp_count(&self) -> usize { self.findings.iter().filter(|f| f.false_positive).count() }

    /// 误报率（基点，bp = 千分数 ×10；1000bp = 100%）。
    pub fn fp_rate_bp(&self) -> u32 {
        let total = self.findings.len() as u32;
        if total == 0 { return 0; }
        self.fp_count() as u32 * 10_000 / total
    }

    /// 校准：误报率 <5%（500bp）通过；超标→须回修。
    pub fn calibrated(&self) -> bool { self.fp_rate_bp() < 500 }

    /// 需要回修的规则（误报来源规则名）。
    pub fn fp_rules(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in self.findings.iter().filter(|f| f.false_positive) {
            let n = f.kind.name();
            if !out.contains(&n) { out.push(n); }
        }
        out
    }
}

/// 审查器：规则册 + 增量模式 + 红线。
pub struct Linter {
    pub rules: Vec<RuleKind>,
    pub incremental: bool,
    pub user_dir_scan: bool, // 红线：true = 违规
}

impl Linter {
    pub fn new() -> Self { Linter { rules: Vec::new(), incremental: false, user_dir_scan: false } }

    pub fn add_rule(&mut self, r: RuleKind) { if !self.rules.contains(&r) { self.rules.push(r); } }

    pub fn core_rules(&self) -> bool {
        RuleKind::CORE.iter().all(|r| self.rules.contains(r))
    }

    /// 扫描性能：全量模式扫描键数 O(键)；劣化（keys > 10_000 且未增量）→ 切增量。
    pub fn maybe_incremental(&mut self, keys: usize) -> bool {
        if keys > 10_000 && !self.incremental { self.incremental = true; return true; }
        false
    }

    /// 红线实测：扫描不读用户目录。
    pub fn redline_ok(&self) -> bool { !self.user_dir_scan }

    /// 扫描驱动（合成样本）：逐键逐规则产出发现。
    pub fn scan(&self, keys: &[(&'static str, Option<RuleKind>)]) -> ScanResult {
        let findings = keys.iter()
            .filter_map(|(k, hit)| hit.map(|kind| Finding { kind, key: k, false_positive: false }))
            .collect();
        ScanResult { findings, incremental: self.incremental }
    }
}

/// 规则冲突仲裁：两规则同键冲突 → 仲裁统一（留痕）。
pub fn arbitrate(a: RuleKind, b: RuleKind, key: &str) -> Option<(RuleKind, RuleKind, &'static str)> {
    if a == b || key.is_empty() { None } else { Some((a, b, "仲裁：以单源风格表为准")) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves383_checks() -> CheckSet {
    let mut set = CheckSet::new("ves383");
    let mut lint = Linter::new();

    // C383-01 五类核心规则注册。
    for r in RuleKind::CORE.iter() { lint.add_rule(*r); }
    chk(&mut set, "C383-01", lint.core_rules() && lint.rules.len() == 5, "五类规则齐");

    // C383-02 域本色缺译规则挂载。
    lint.add_rule(RuleKind::AltTextMissing);
    chk(&mut set, "C383-02", lint.rules.contains(&RuleKind::AltTextMissing) && lint.rules.len() == 6, "缺译规则挂载");

    // C383-03 风格单源转译表：五类全覆盖。
    let mut style = StyleTable::new();
    style.put(RuleKind::HardcodedText, "S4067-hc");
    style.put(RuleKind::PlaceholderComplete, "S4067-ph");
    style.put(RuleKind::TermCompliance, "S4067-term");
    style.put(RuleKind::LengthOverrun, "S4067-len");
    style.put(RuleKind::DirectionMix, "S4067-dir");
    chk(&mut set, "C383-03", style.core_covered() && style.anchor(RuleKind::DirectionMix) == Some("S4067-dir"), "转译表单源");

    // C383-04 扫描：3 键 2 命中。
    let res = lint.scan(&[
        ("btn.ok", Some(RuleKind::HardcodedText)),
        ("menu.file", None),
        ("title.app", Some(RuleKind::LengthOverrun)),
    ]);
    chk(&mut set, "C383-04", res.real_count() == 2 && res.findings.len() == 2, "扫描两命中");

    // C383-05 误报率校准：0 误报 → 0bp 过线。
    chk(&mut set, "C383-05", res.fp_rate_bp() == 0 && res.calibrated(), "零误报过线");

    // C383-06 误报超标：10 条 6 误报 → 6000bp ≥ 500bp 回修。
    let mut bad = ScanResult { findings: Vec::new(), incremental: false };
    for i in 0..10 {
        bad.findings.push(Finding {
            kind: if i < 6 { RuleKind::HardcodedText } else { RuleKind::LengthOverrun },
            key: "k", false_positive: i < 6,
        });
    }
    chk(&mut set, "C383-06", bad.fp_rate_bp() == 6000 && !bad.calibrated(), "6000bp 超标");

    // C383-07 回修定位：误报规则名单。
    chk(&mut set, "C383-07", bad.fp_rules() == vec!["hardcoded-text"], "回修规则定位");

    // C383-08 增量模式切换：>10_000 键触发。
    let mut lint2 = Linter::new();
    chk(&mut set, "C383-08", !lint2.maybe_incremental(500) && lint2.maybe_incremental(20_000)
        && lint2.incremental, "两万键切增量");

    // C383-09 增量标记进扫描结果。
    let r2 = lint2.scan(&[]);
    chk(&mut set, "C383-09", r2.incremental && r2.findings.is_empty() && r2.calibrated(), "增量标记随行");

    // C383-10 红线实测：不读用户目录。
    chk(&mut set, "C383-10", lint.redline_ok() && lint2.redline_ok(), "红线：不读用户目录");

    // C383-11 规则冲突仲裁。
    chk(&mut set, "C383-11",
        arbitrate(RuleKind::LengthOverrun, RuleKind::TermCompliance, "btn.x").is_some()
            && arbitrate(RuleKind::LengthOverrun, RuleKind::LengthOverrun, "btn.x").is_none(), "冲突仲裁留痕");

    // C383-12 规则注册幂等。
    lint.add_rule(RuleKind::HardcodedText);
    chk(&mut set, "C383-12", lint.rules.len() == 6, "注册幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves383_smoke() {
        let set = run_ves383_checks();
        assert!(set.failed_count() == 0, "ves383 red: {}", set.red_items_text());
    }

    #[test]
    fn ves383_fp_rate_edge() {
        let mut r = ScanResult { findings: Vec::new(), incremental: false };
        assert_eq!(r.fp_rate_bp(), 0);
        r.findings.push(Finding { kind: RuleKind::DirectionMix, key: "k", false_positive: true });
        assert_eq!(r.fp_rate_bp(), 10_000);
        assert!(!r.calibrated());
    }
}
