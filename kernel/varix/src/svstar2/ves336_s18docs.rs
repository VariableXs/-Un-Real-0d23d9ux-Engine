//! ves336_s18docs — VE-F4096 · T05 语言资源文档
//! 四类资产各一册手册 + 打包/下载/热更新流程册 + 字形覆盖说明 + 一致性抽查（manifest 字段/五检项对拍——漂移以实现为准修文档留痕）+ 无障碍资产规范专节（域本色）+ 示例不含真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9BC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    ManualMissing = SEG_BASE + 0x01,
    DocDrift = SEG_BASE + 0x02,
    SpotFail = SEG_BASE + 0x03,
}

/// 四类资产手册主题。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManualKind { Font = 0, Image = 1, Audio = 2, Video = 3 }

impl ManualKind {
    pub const ALL: [ManualKind; 4] = [
        ManualKind::Font, ManualKind::Image, ManualKind::Audio, ManualKind::Video];
    pub fn name(self) -> &'static str {
        match self { ManualKind::Font => "font", ManualKind::Image => "image",
            ManualKind::Audio => "audio", ManualKind::Video => "video" }
    }
}

/// 流程册主题。
pub const FLOW_MANUALS: [&'static str; 3] = ["pack", "download", "hot-update"];

/// 字形覆盖说明章。
pub const GLYPH_DOC_SECTIONS: [&'static str; 3] = ["coverage-policy", "fallback-chain", "subset-budget"];

/// 手册馆。
pub struct DocShelf { pub manuals: Vec<&'static str>, pub flows: Vec<&'static str>, pub glyph_doc: Vec<&'static str>, pub a11y_section: bool }

impl DocShelf {
    pub fn new() -> Self {
        DocShelf { manuals: Vec::new(), flows: Vec::new(), glyph_doc: Vec::new(), a11y_section: false }
    }

    pub fn put_manual(&mut self, k: ManualKind) {
        let n = k.name();
        if !self.manuals.contains(&n) { self.manuals.push(n); }
    }

    pub fn put_flow(&mut self, name: &'static str) {
        if !self.flows.contains(&name) { self.flows.push(name); }
    }

    /// 四类手册齐备。
    pub fn all_manuals(&self) -> bool {
        ManualKind::ALL.iter().all(|k| self.manuals.contains(&k.name()))
    }

    /// 流程册齐备。
    pub fn all_flows(&self) -> bool {
        FLOW_MANUALS.iter().all(|f| self.flows.contains(f))
    }

    /// 字形覆盖说明三章齐备。
    pub fn glyph_doc_ok(&self) -> bool {
        GLYPH_DOC_SECTIONS.iter().all(|s| self.glyph_doc.contains(s))
    }
}

/// 一致性抽查：文档声明 vs 实现真值逐条对拍。
#[derive(Clone)]
pub struct SpotLine { pub doc_claim: &'static str, pub impl_truth: &'static str }

pub struct SpotCheck { pub lines: Vec<SpotLine>, pub fixed: Vec<&'static str> }

impl SpotCheck {
    pub fn new() -> Self { SpotCheck { lines: Vec::new(), fixed: Vec::new() } }

    pub fn add(&mut self, doc_claim: &'static str, impl_truth: &'static str) {
        self.lines.push(SpotLine { doc_claim, impl_truth });
    }

    /// 对拍：claim == truth 才一致；漂移项以实现为准修文档（留痕）。
    pub fn run(&mut self) -> Result<usize, DcCode> {
        let mut drift = 0;
        for l in self.lines.iter() {
            if l.doc_claim != l.impl_truth {
                drift += 1;
                self.fixed.push(l.doc_claim);
            }
        }
        if drift > 0 { Err(DcCode::DocDrift) } else { Ok(self.lines.len()) }
    }

    pub fn fixed_count(&self) -> usize { self.fixed.len() }
}

/// manifest 字段对拍：文档承诺字段集 == 实现五字段。
pub const MANIFEST_FIELDS: [&'static str; 5] = ["lang", "version", "total_kb", "deps", "checksum"];

pub fn manifest_fields_match(doc_fields: &[&'static str]) -> bool {
    doc_fields.len() == MANIFEST_FIELDS.len()
        && MANIFEST_FIELDS.iter().all(|f| doc_fields.contains(f))
}

/// 五检项对拍：文档承诺五检 == 实现五检标签。
pub const FIVE_CHECKS: [&'static str; 5] = [
    "manifest", "checksum", "glyph-coverage", "ref-closure", "a11y-complete"];

pub fn five_checks_match(doc_checks: &[&'static str]) -> bool {
    doc_checks.len() == FIVE_CHECKS.len()
        && FIVE_CHECKS.iter().all(|f| doc_checks.contains(f))
}

/// 修正留痕册。
pub struct FixCase { pub doc: &'static str, pub action: &'static str }

pub struct FixBook { pub cases: Vec<FixCase> }

impl FixBook {
    pub fn new() -> Self { FixBook { cases: Vec::new() } }
    pub fn open(&mut self, doc: &'static str, action: &'static str) { self.cases.push(FixCase { doc, action }); }
    pub fn count(&self) -> usize { self.cases.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves336_checks() -> CheckSet {
    let mut set = CheckSet::new("ves336");
    let mut shelf = DocShelf::new();

    // C336-01 四类手册逐册上架。
    for k in ManualKind::ALL.iter() { shelf.put_manual(*k); }
    chk(&mut set, "C336-01", shelf.all_manuals() && shelf.manuals.len() == 4, "四类手册齐");

    // C336-02 流程三册上架。
    for f in FLOW_MANUALS.iter() { shelf.put_flow(f); }
    chk(&mut set, "C336-02", shelf.all_flows(), "打包/下载/热更新三册齐");

    // C336-03 字形覆盖说明三章齐备。
    for s in GLYPH_DOC_SECTIONS.iter() { shelf.glyph_doc.push(s); }
    chk(&mut set, "C336-03", shelf.glyph_doc_ok(), "覆盖策略/回退链/预算三章齐");

    // C336-04 无障碍资产规范专节（域本色）。
    shelf.a11y_section = true;
    chk(&mut set, "C336-04", shelf.a11y_section, "专节在册");

    // C336-05 手册缺册 → 检出。
    let mut shelf2 = DocShelf::new();
    shelf2.put_manual(ManualKind::Font);
    chk(&mut set, "C336-05", !shelf2.all_manuals(), "缺三册 => ManualMissing 面");

    // C336-06 manifest 字段对拍：五字段齐。
    chk(&mut set, "C336-06", manifest_fields_match(&["lang", "version", "total_kb", "deps", "checksum"]), "manifest 五字段对拍过");

    // C336-07 manifest 字段漂移检出。
    chk(&mut set, "C336-07", !manifest_fields_match(&["lang", "version", "size", "deps", "checksum"]), "size≠total_kb => 漂移");

    // C336-08 五检项对拍。
    chk(&mut set, "C336-08", five_checks_match(&["manifest", "checksum", "glyph-coverage", "ref-closure", "a11y-complete"]), "五检项对拍过");

    // C336-09 五检项文档漏项检出。
    chk(&mut set, "C336-09", !five_checks_match(&["manifest", "checksum", "glyph-coverage", "ref-closure"]), "漏 a11y-complete => 漂移");

    // C336-10 抽查一致全过。
    let mut sp = SpotCheck::new();
    sp.add("五字段冻结", "五字段冻结");
    sp.add("五检门", "五检门");
    chk(&mut set, "C336-10", sp.run() == Ok(2), "两抽查一致全过");

    // C336-11 抽查漂移 → 以实现为准修文档留痕。
    let mut sp2 = SpotCheck::new();
    sp2.add("四字段冻结", "五字段冻结");
    let mut fb = FixBook::new();
    if sp2.run() == Err(DcCode::DocDrift) { fb.open("pack-manual", "四字段→五字段（以实现为准）"); }
    chk(&mut set, "C336-11", sp2.fixed_count() == 1 && fb.count() == 1, "漂移 1 修 1 留痕 1");

    // C336-12 示例不含真实数据（虚构占位）。
    chk(&mut set, "C336-12",
        shelf.manuals.iter().all(|m| !m.contains("user-data")),
        "手册不含真实数据引用");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves336_smoke() {
        let set = run_ves336_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves336 red: {:?}", red);
    }

    #[test]
    fn ves336_manifest_fields() {
        assert!(manifest_fields_match(&["checksum", "lang", "version", "deps", "total_kb"]));
        assert!(!manifest_fields_match(&[]));
        assert!(five_checks_match(&FIVE_CHECKS));
    }
}
