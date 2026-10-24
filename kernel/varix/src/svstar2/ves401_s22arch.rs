//! ves401_s22arch — VE-F4161 · T09 开工与自查范围
//! T 域自查组开工（O09-P09-Q09-R09-S09 模板第九用声明——家族治理流程复用）；
//! 范围声明：T01-T08 八组×20 项=160 + 组间 28 对对账 + 七维汇总（API/测试/性能/安全/无障碍/文档/预算）
//! + T 域三特色增维声明（翻译质量维+地区合规维+多语言读屏维——增维登记红线复述）；证据引用制复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x00C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    EvidenceMissing = SEG_BASE + 0x01, // 0x00C1
    DimUnregistered = SEG_BASE + 0x02, // 0x00C2
}

/// 八组。
pub const GROUPS: [&'static str; 8] =
    ["T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08"];

/// 七维汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumDim { Api = 0, Test = 1, Perf = 2, Security = 3, A11y = 4, Doc = 5, Budget = 6 }

impl SumDim {
    pub const ALL: [SumDim; 7] = [
        SumDim::Api, SumDim::Test, SumDim::Perf, SumDim::Security,
        SumDim::A11y, SumDim::Doc, SumDim::Budget];
    pub fn name(self) -> &'static str {
        match self {
            SumDim::Api => "api", SumDim::Test => "test", SumDim::Perf => "perf",
            SumDim::Security => "security", SumDim::A11y => "a11y", SumDim::Doc => "doc",
            SumDim::Budget => "budget",
        }
    }
}

/// 三特色增维（T 域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtraDim { TranslationQuality = 0, RegionCompliance = 1, MultiLangScreenReader = 2 }

impl ExtraDim {
    pub const ALL: [ExtraDim; 3] = [
        ExtraDim::TranslationQuality, ExtraDim::RegionCompliance, ExtraDim::MultiLangScreenReader];
    pub fn name(self) -> &'static str {
        match self {
            ExtraDim::TranslationQuality => "translation-quality",
            ExtraDim::RegionCompliance => "region-compliance",
            ExtraDim::MultiLangScreenReader => "multi-lang-sr",
        }
    }
}

/// 自查计划：组×维矩阵（160 项 + 28 对 + 七维 + 增维）。
pub struct SelfCheckPlan {
    pub items: Vec<(&'static str, SumDim, bool)>, // (组, 维, 有证据)
    pub pair_audits: Vec<(&'static str, &'static str)>, // 组间对账对
    pub extra_registered: Vec<ExtraDim>,           // 增维登记
}

impl SelfCheckPlan {
    pub fn new() -> Self {
        SelfCheckPlan { items: Vec::new(), pair_audits: Vec::new(), extra_registered: Vec::new() }
    }

    /// 全量铺计划：8 组 × 7 维 = 56 计划行（每行代表组内多项）。
    pub fn lay_out(&mut self) {
        for g in GROUPS.iter() {
            for d in SumDim::ALL.iter() {
                if !self.items.iter().any(|x| x.0 == *g && x.1 == *d) {
                    self.items.push((*g, *d, false));
                }
            }
        }
    }

    pub fn attach_evidence(&mut self, g: &'static str, d: SumDim) -> bool {
        match self.items.iter_mut().find(|x| x.0 == g && x.1 == d) {
            Some(x) => { x.2 = true; true }
            None => false,
        }
    }

    /// 证据引用制：无证据计划行 → 阻断名单。
    pub fn evidence_gaps(&self) -> Vec<(&'static str, &'static str)> {
        self.items.iter().filter(|x| !x.2)
            .map(|x| (x.0, x.1.name())).collect()
    }

    /// 组间 28 对对账铺排（C(8,2)）。
    pub fn lay_pairs(&mut self) {
        for i in 0..GROUPS.len() {
            for j in (i + 1)..GROUPS.len() {
                let p = (GROUPS[i], GROUPS[j]);
                if !self.pair_audits.contains(&p) { self.pair_audits.push(p); }
            }
        }
    }

    pub fn pairs_complete(&self) -> bool { self.pair_audits.len() == 28 }

    /// 增维登记（红线：增维无标准→先立标准，未登记 → 阻断）。
    pub fn register_extra(&mut self, d: ExtraDim) -> bool {
        if self.extra_registered.contains(&d) { return false; }
        self.extra_registered.push(d);
        true
    }

    pub fn extras_complete(&self) -> bool { self.extra_registered.len() == 3 }

    /// 模板第九用声明核验。
    pub fn template_ninth_use(&self) -> bool {
        self.items.len() == 56 && self.pairs_complete() && self.extras_complete()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves401_checks() -> CheckSet {
    let mut set = CheckSet::new("ves401");
    let mut p = SelfCheckPlan::new();

    // C401-01 全量铺计划 56 行。
    p.lay_out();
    chk(&mut set, "C401-01", p.items.len() == 56, "8 组×7 维=56");

    // C401-02 铺排幂等。
    p.lay_out();
    chk(&mut set, "C401-02", p.items.len() == 56, "重复铺排不增行");

    // C401-03 证据缺口：初始全缺 56。
    chk(&mut set, "C401-03", p.evidence_gaps().len() == 56, "全缺证据");

    // C401-04 逐组挂证据。
    for g in GROUPS.iter() {
        for d in SumDim::ALL.iter() { p.attach_evidence(g, *d); }
    }
    chk(&mut set, "C401-04", p.evidence_gaps().is_empty(), "证据全挂");

    // C401-05 未登记组拒绝挂证据。
    chk(&mut set, "C401-05", !p.attach_evidence("T09", SumDim::Api), "组外挂证拒");

    // C401-06 组间 28 对对账铺排。
    p.lay_pairs();
    chk(&mut set, "C401-06", p.pairs_complete() && p.pair_audits[0] == ("T01", "T02")
        && p.pair_audits[27] == ("T07", "T08"), "28 对齐");

    // C401-07 三特色增维登记（红线）。
    for d in ExtraDim::ALL.iter() { p.register_extra(*d); }
    chk(&mut set, "C401-07", p.extras_complete(), "三增维登记");

    // C401-08 增维登记幂等。
    chk(&mut set, "C401-08", !p.register_extra(ExtraDim::TranslationQuality), "重复登记拒");

    // C401-09 模板第九用声明成立。
    chk(&mut set, "C401-09", p.template_ninth_use(), "第九用成立");

    // C401-10 七维名单单源。
    chk(&mut set, "C401-10", SumDim::ALL.len() == 7 && SumDim::ALL[0].name() == "api"
        && SumDim::ALL[6].name() == "budget", "七维单源");

    // C401-11 增维名单单源（域本色）。
    chk(&mut set, "C401-11", ExtraDim::ALL.len() == 3
        && ExtraDim::ALL[2].name() == "multi-lang-sr", "三增维单源");

    // C401-12 八组名单单源。
    chk(&mut set, "C401-12", GROUPS.len() == 8 && GROUPS[0] == "T01" && GROUPS[7] == "T08", "八组单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves401_smoke() {
        let set = run_ves401_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves401 red: {:?}", red);
    }

    #[test]
    fn ves401_pairs_count() {
        let mut p = SelfCheckPlan::new();
        p.lay_pairs();
        assert_eq!(p.pair_audits.len(), 28); // C(8,2)
    }
}
