//! ves292_s14corpus — VE-F4052 · T03 排版测试语料
//! 四语言×文档类型×边界样文语料（复用语料三要素全域化断言）+ 语言覆盖断言（缺一阻断）+ 语料版本锚定 + 依据复用撤回。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7E40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum CorpusCode {
    Ok = 0,
    ThreeElementsMissing = SEG_BASE + 0x01,
    LangNoCorpus = SEG_BASE + 0x02,
    VersionDrift = SEG_BASE + 0x03,
    BasisMissing = SEG_BASE + 0x04,
    DupCorpus = SEG_BASE + 0x05,
}

/// 四语言族。
pub const CORPUS_LANGS: [&str; 4] = ["latin", "cjk", "arabic", "indic"];

/// 文档类型。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DocType { Typical = 0, Mixed = 1, Edge = 2 }

impl DocType {
    pub const ALL: [DocType; 3] = [DocType::Typical, DocType::Mixed, DocType::Edge];
    pub fn name(self) -> &'static str {
        match self { DocType::Typical => "典型", DocType::Mixed => "混排", DocType::Edge => "边界" }
    }
}

/// 语料条目：三要素（样文/依据/版本）全域化断言。
#[derive(Clone)]
pub struct CorpusItem {
    pub id: u32,
    pub lang: &'static str,
    pub doc_type: DocType,
    pub sample: String,
    /// 依据（来源红线）。
    pub basis: String,
    /// 版本锚。
    pub version: u32,
}

impl CorpusItem {
    /// 三要素断言：样文/依据/版本全非空。
    pub fn three_elements(&self) -> bool {
        !self.sample.is_empty() && !self.basis.is_empty() && self.version > 0
    }
}

pub struct CorpusBook {
    pub items: Vec<CorpusItem>,
    pub anchor_version: u32,
}

impl CorpusBook {
    pub fn new() -> Self { CorpusBook { items: Vec::new(), anchor_version: 0 } }

    pub fn add(&mut self, it: CorpusItem) -> Result<(), CorpusCode> {
        if !it.three_elements() { return Err(CorpusCode::ThreeElementsMissing); }
        if !CORPUS_LANGS.contains(&it.lang) { return Err(CorpusCode::LangNoCorpus); }
        for e in self.items.iter() {
            if e.id == it.id { return Err(CorpusCode::DupCorpus); }
        }
        self.items.push(it);
        Ok(())
    }

    /// 语言覆盖断言：四语言各有料，缺一即阻断名单。
    pub fn lang_gaps(&self) -> Vec<&'static str> {
        let mut g = Vec::new();
        for l in CORPUS_LANGS.iter() {
            if !self.items.iter().any(|e| e.lang == *l) { g.push(*l); }
        }
        g
    }

    /// 文档类型覆盖：三型齐。
    pub fn type_gaps(&self) -> Vec<&'static str> {
        let mut g = Vec::new();
        for t in DocType::ALL.iter() {
            if !self.items.iter().any(|e| e.doc_type == *t) { g.push(t.name()); }
        }
        g
    }

    /// 版本锚定：全部条目版本等于锚。
    pub fn version_anchored(&self) -> bool {
        if self.anchor_version == 0 { return false; }
        self.items.iter().all(|e| e.version == self.anchor_version)
    }

    pub fn set_anchor(&mut self, v: u32) {
        self.anchor_version = v;
    }

    /// 依据撤回：依据缺失条目清单。
    pub fn basis_gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for e in self.items.iter() {
            if e.basis.is_empty() { g.push(e.id); }
        }
        g
    }
}

pub fn run_ves292_checks() -> CheckSet {
    let mut set = CheckSet::new("ves292_s14corpus");
    let mut cb = CorpusBook::new();

    // 1) 三要素缺拒。
    set.add("C4052-01", cb.add(CorpusItem { id: 1, lang: "cjk", doc_type: DocType::Typical, sample: String::new(), basis: String::from("标准"), version: 3 }) == Err(CorpusCode::ThreeElementsMissing), "三要素缺拒");

    // 2) 非法语言拒。
    set.add("C4052-02", cb.add(CorpusItem { id: 1, lang: "xx", doc_type: DocType::Typical, sample: String::from("s"), basis: String::from("b"), version: 3 }) == Err(CorpusCode::LangNoCorpus), "非法语言拒");

    // 3) 四语言入库。
    for (i, l) in CORPUS_LANGS.iter().enumerate() {
        let _ = cb.add(CorpusItem { id: i as u32 + 1, lang: l, doc_type: DocType::Typical, sample: String::from("样"), basis: String::from("UAX/JIS"), version: 3 });
    }
    set.add("C4052-03", cb.items.len() == 4 && cb.lang_gaps().is_empty(), "四语言齐");

    // 4) 重复 id 拒。
    set.add("C4052-04", cb.add(CorpusItem { id: 1, lang: "cjk", doc_type: DocType::Mixed, sample: String::from("s"), basis: String::from("b"), version: 3 }) == Err(CorpusCode::DupCorpus), "重复拒");

    // 5) 版本锚定。
    cb.set_anchor(3);
    set.add("C4052-05", cb.version_anchored(), "版本锚定");

    // 6) 版本漂检出。
    cb.set_anchor(4);
    set.add("C4052-06", !cb.version_anchored(), "版本漂检出");
    cb.set_anchor(3);

    // 7) 未锚定不可判。
    let cb2 = CorpusBook::new();
    set.add("C4052-07", !cb2.version_anchored(), "未锚定不可判");

    // 8) 三型覆盖。
    set.add("C4052-08", cb.type_gaps() == ["混排", "边界"], "三型缺口如实");

    // 9) 三要素断言单条。
    let it = CorpusItem { id: 9, lang: "latin", doc_type: DocType::Edge, sample: String::from("a"), basis: String::from("spec"), version: 1 };
    set.add("C4052-09", it.three_elements(), "三要素断言");

    // 10) 依据撤回清单：空依据检出。
    let mut cb3 = CorpusBook::new();
    let _ = cb3.add(CorpusItem { id: 5, lang: "cjk", doc_type: DocType::Typical, sample: String::from("s"), basis: String::from("b"), version: 1 });
    let _ = cb3.add(CorpusItem { id: 6, lang: "cjk", doc_type: DocType::Typical, sample: String::from("s"), basis: String::from("b2"), version: 1 });
    cb3.items[1].basis = String::new();
    set.add("C4052-10", cb3.basis_gaps() == [6], "依据撤回检出");

    // 11) 段位自查。
    set.add("C4052-11", SEG_BASE == 0x7E40, "段位 0x7E40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_flow() {
        let mut cb = CorpusBook::new();
        assert_eq!(cb.add(CorpusItem { id: 1, lang: "cjk", doc_type: DocType::Typical, sample: String::new(), basis: String::from("b"), version: 1 }), Err(CorpusCode::ThreeElementsMissing));
        let _ = cb.add(CorpusItem { id: 1, lang: "cjk", doc_type: DocType::Typical, sample: String::from("s"), basis: String::from("b"), version: 1 });
        assert_eq!(cb.lang_gaps().len(), 3);
    }
}
