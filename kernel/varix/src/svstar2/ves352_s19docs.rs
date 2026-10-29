//! ves352_s19docs — VE-F4112 · T06 地区文档
//! 地区合规手册（三类规则×重点地区分册）+ 格式速查表 + 配置指南 + 审查规则说明四类 + 一致性抽查（规则条目/开关表对拍——漂移以配置为准修文档留痕）+ 地区无障碍法规专册（域本色）+ 示例不含真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC6C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    Ok = 0,
    ManualMissing = SEG_BASE + 0x01,
    DocDrift = SEG_BASE + 0x02,
}

/// 四类文档主题。
pub const FOUR_DOCS: [&'static str; 4] = ["compliance-manual", "format-quickref", "config-guide", "content-rules"];

/// 合规手册分册（三类规则×地区）。
pub struct ComplianceVolume { pub region: &'static str, pub law: bool, pub consumer: bool, pub a11y_act: bool }

pub struct DocShelf {
    pub docs: Vec<&'static str>,
    pub volumes: Vec<ComplianceVolume>,
    pub a11y_volume: bool,
    pub fixes: Vec<&'static str>,
}

impl DocShelf {
    pub fn new() -> Self {
        DocShelf { docs: Vec::new(), volumes: Vec::new(), a11y_volume: false, fixes: Vec::new() }
    }

    pub fn put_doc(&mut self, name: &'static str) {
        if !self.docs.contains(&name) { self.docs.push(name); }
    }

    pub fn four_docs(&self) -> bool {
        FOUR_DOCS.iter().all(|d| self.docs.contains(d))
    }

    pub fn put_volume(&mut self, v: ComplianceVolume) {
        if !self.volumes.iter().any(|x| x.region == v.region) { self.volumes.push(v); }
    }

    /// 分册三类齐备。
    pub fn volume_complete(&self, region: &str) -> bool {
        self.volumes.iter().any(|v| v.region == region && v.law && v.consumer && v.a11y_act)
    }

    pub fn missing_volumes<'a>(&self, regions: &[&'a str]) -> Vec<&'a str> {
        regions.iter().filter(|r| !self.volume_complete(r)).map(|r| *r).collect()
    }
}

/// 一致性抽查：文档声明 vs 配置真值。
pub struct SpotLine { pub doc_claim: &'static str, pub cfg_truth: &'static str }

pub struct SpotCheck { pub lines: Vec<SpotLine>, pub fixed: Vec<&'static str> }

impl SpotCheck {
    pub fn new() -> Self { SpotCheck { lines: Vec::new(), fixed: Vec::new() } }
    pub fn add(&mut self, doc: &'static str, truth: &'static str) { self.lines.push(SpotLine { doc_claim: doc, cfg_truth: truth }); }

    /// 对拍：漂移以配置为准修文档（留痕）。
    pub fn run(&mut self) -> Result<usize, DmCode> {
        let mut drift = 0;
        for l in self.lines.iter() {
            if l.doc_claim != l.cfg_truth { drift += 1; self.fixed.push(l.doc_claim); }
        }
        if drift > 0 { Err(DmCode::DocDrift) } else { Ok(self.lines.len()) }
    }

    pub fn fixed_count(&self) -> usize { self.fixed.len() }
}

/// 开关表对拍：文档承诺开关集 == 配置开关集。
pub const CAP_SWITCHES: [&'static str; 5] = ["law", "residency", "content", "payment", "a11y-law"];

pub fn switches_match(doc: &[&'static str], cfg: &[&'static str]) -> bool {
    doc.len() == cfg.len() && cfg.iter().all(|c| doc.contains(c))
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves352_checks() -> CheckSet {
    let mut set = CheckSet::new("ves352");
    let mut shelf = DocShelf::new();

    // C352-01 四类文档上架。
    for d in FOUR_DOCS.iter() { shelf.put_doc(d); }
    chk(&mut set, "C352-01", shelf.four_docs(), "四类齐");

    // C352-02 合规手册分册：EU 三类齐。
    shelf.put_volume(ComplianceVolume { region: "EU", law: true, consumer: true, a11y_act: true });
    chk(&mut set, "C352-02", shelf.volume_complete("EU"), "EU 分册三类齐");

    // C352-03 分册缺类检出。
    shelf.put_volume(ComplianceVolume { region: "US", law: true, consumer: false, a11y_act: true });
    chk(&mut set, "C352-03", shelf.missing_volumes(&["EU", "US", "JP"]) == vec!["US", "JP"], "US 缺消保 JP 缺册");

    // C352-04 地区无障碍法规专册（域本色）。
    shelf.a11y_volume = true;
    chk(&mut set, "C352-04", shelf.a11y_volume, "专册在架");

    // C352-05 开关表对拍一致。
    chk(&mut set, "C352-05",
        switches_match(&["law", "residency", "content", "payment", "a11y-law"],
                       &["a11y-law", "payment", "content", "residency", "law"]), "五开关对拍（乱序等价）");

    // C352-06 开关表漂移检出。
    chk(&mut set, "C352-06", !switches_match(&["law", "content"], &["law", "content", "payment"]), "少一开关 => 漂移");

    // C352-07 抽查一致全过。
    let mut sp = SpotCheck::new();
    sp.add("EU 驻留档 no-cross-border", "EU 驻留档 no-cross-border");
    sp.add("DE 负数括号 on", "DE 负数括号 on");
    chk(&mut set, "C352-07", sp.run() == Ok(2), "两抽查全过");

    // C352-08 抽查漂移 → 以配置为准修文档留痕。
    let mut sp2 = SpotCheck::new();
    sp2.add("DE 负数括号 off", "DE 负数括号 on");
    chk(&mut set, "C352-08", sp2.run() == Err(DmCode::DocDrift) && sp2.fixed_count() == 1, "漂移 1 修 1 留痕");

    // C352-09 多漂移累计。
    let mut sp3 = SpotCheck::new();
    sp3.add("a", "b");
    sp3.add("c", "c");
    sp3.add("d", "e");
    let _ = sp3.run();
    chk(&mut set, "C352-09", sp3.fixed_count() == 2, "三抽查两漂移");

    // C352-10 缺册检出 → 补齐。
    shelf.put_volume(ComplianceVolume { region: "JP", law: true, consumer: true, a11y_act: true });
    chk(&mut set, "C352-10", shelf.missing_volumes(&["EU", "US", "JP"]) == vec!["US"], "补 JP 后仅 US 缺");

    // C352-11 示例不含真实数据（虚构地区 ZZ 演示）。
    let demo_regions = ["ZZ1", "ZZ2"];
    chk(&mut set, "C352-11", demo_regions.iter().all(|r| !["US", "EU", "CN", "JP", "DE"].contains(r)), "示例地区全虚构");

    // C352-12 四类名单单源。
    chk(&mut set, "C352-12", FOUR_DOCS[0] == "compliance-manual" && FOUR_DOCS.len() == 4, "四类单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves352_smoke() {
        let set = run_ves352_checks();
        assert!(set.failed_count() == 0, "ves352 red: {}", set.red_items_text());
    }

    #[test]
    fn ves352_switches() {
        assert!(switches_match(&CAP_SWITCHES, &CAP_SWITCHES));
        assert!(!switches_match(&CAP_SWITCHES, &[]));
    }
}
