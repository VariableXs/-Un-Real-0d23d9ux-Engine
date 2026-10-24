//! ves429_s23docschema — VE-F4189 · T 域文档与 schema 总审
//! 总审：T 域文档总审（八组文档齐备+多语言无障碍指南专册终版+F4171 对拍收口）
//! +schema 总审（翻译/资产/地区/观测四 schema 终版+版本冻结）；
//! 文档缺章→补章；schema 漂移→对版；冻结后发现→F4196 纪律；
//! 专册终审含多语言替代文本规范（域本色）；示例无真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x11E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    Ok = 0,
    ChapterMissing = SEG_BASE + 0x01, // 0x11E1
    SchemaDrift = SEG_BASE + 0x02,    // 0x11E2
    VersionDrift = SEG_BASE + 0x03,   // 0x11E3
    NotFrozen = SEG_BASE + 0x04,      // 0x11E4
}

/// 八组文档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocGroup { G1 = 0, G2 = 1, G3 = 2, G4 = 3, G5 = 4, G6 = 5, G7 = 6, G8 = 7 }

impl DocGroup {
    pub const ALL: [DocGroup; 8] = [
        DocGroup::G1, DocGroup::G2, DocGroup::G3, DocGroup::G4,
        DocGroup::G5, DocGroup::G6, DocGroup::G7, DocGroup::G8];
}

/// 四 schema。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchemaKind { Translate = 0, Asset = 1, Region = 2, Observe = 3 }

impl SchemaKind {
    pub const ALL: [SchemaKind; 4] = [SchemaKind::Translate, SchemaKind::Asset, SchemaKind::Region, SchemaKind::Observe];
}

/// 总审册。
pub struct DsFinalBook {
    pub docs: Vec<DocGroup>,       // 齐备组
    pub a11y_guide_final: bool,    // 多语言无障碍指南专册终版（域本色）
    pub a11y_alt_text: bool,       // 专册含替代文本规范
    pub schema_ver: Vec<(SchemaKind, u32)>,
    pub frozen_ver: u32,
    pub cross_check_done: bool,    // F4171 对拍收口
}

impl DsFinalBook {
    pub fn new() -> Self {
        DsFinalBook { docs: Vec::new(), a11y_guide_final: false, a11y_alt_text: false,
            schema_ver: Vec::new(), frozen_ver: 0, cross_check_done: false }
    }
    pub fn put_doc(&mut self, g: DocGroup) { if !self.docs.contains(&g) { self.docs.push(g); } }
    pub fn missing_groups(&self) -> Vec<DocGroup> {
        DocGroup::ALL.iter().filter(|g| !self.docs.contains(g)).map(|g| *g).collect()
    }
    pub fn set_schema(&mut self, k: SchemaKind, ver: u32) {
        self.schema_ver.retain(|(kk, _)| *kk != k);
        self.schema_ver.push((k, ver));
    }
    pub fn schema_ver(&self, k: SchemaKind) -> u32 {
        self.schema_ver.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v).unwrap_or(0)
    }
    /// schema 版本冻结：四 schema 齐且版本一致+对拍收口。
    pub fn freeze(&mut self, ver: u32) -> Result<(), DsCode> {
        for k in SchemaKind::ALL {
            let v = self.schema_ver(k);
            if v == 0 { return Err(DsCode::SchemaDrift); }
            if v != ver { return Err(DsCode::VersionDrift); }
        }
        if !self.cross_check_done { return Err(DsCode::NotFrozen); }
        self.frozen_ver = ver;
        Ok(())
    }
    /// 文档总审：八组齐+专册终版+替代文本规范+示例无真实数据（此处以专册断言代）。
    pub fn doc_verdict(&self) -> Result<(), DsCode> {
        if self.missing_groups().len() > 0 { return Err(DsCode::ChapterMissing); }
        if !self.a11y_guide_final || !self.a11y_alt_text { return Err(DsCode::ChapterMissing); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves429_checks() -> CheckSet {
    let mut set = CheckSet::new("ves429");
    let mut bk = DsFinalBook::new();

    // C429-01 八组缺章→补章码（缺章定位）。
    for g in [DocGroup::G1, DocGroup::G2, DocGroup::G3, DocGroup::G4, DocGroup::G5] { bk.put_doc(g); }
    let miss = bk.missing_groups();
    chk(&mut set, "C429-01", bk.doc_verdict() == Err(DsCode::ChapterMissing) && miss.len() == 3, "缺三章定位");

    // C429-02 八组齐后仍因专册缺终版拒（域本色）。
    for g in [DocGroup::G6, DocGroup::G7, DocGroup::G8] { bk.put_doc(g); }
    chk(&mut set, "C429-02", bk.doc_verdict() == Err(DsCode::ChapterMissing), "专册未终版拒");

    // C429-03 专册终版+替代文本规范→文档总审过。
    bk.a11y_guide_final = true;
    chk(&mut set, "C429-03", bk.doc_verdict() == Err(DsCode::ChapterMissing), "替代文本规范缺拒");

    // C429-04 补规范后文档总审全过。
    bk.a11y_alt_text = true;
    chk(&mut set, "C429-04", bk.doc_verdict().is_ok(), "文档总审过");

    // C429-05 schema 漂移：缺 schema 拒冻。
    bk.set_schema(SchemaKind::Translate, 4);
    chk(&mut set, "C429-05", bk.freeze(4) == Err(DsCode::SchemaDrift), "缺 schema 拒");

    // C429-06 版本漂移拒。
    for k in [SchemaKind::Asset, SchemaKind::Region, SchemaKind::Observe] { bk.set_schema(k, 3); }
    chk(&mut set, "C429-06", bk.freeze(4) == Err(DsCode::VersionDrift), "版本漂移拒");

    // C429-07 对拍未收口拒。
    for k in SchemaKind::ALL { bk.set_schema(k, 4); }
    chk(&mut set, "C429-07", bk.freeze(4) == Err(DsCode::NotFrozen), "对拍未收口拒");

    // C429-08 对版收口后冻结通过。
    bk.cross_check_done = true;
    chk(&mut set, "C429-08", bk.freeze(4).is_ok() && bk.frozen_ver == 4, "四 schema 冻结");

    // C429-09 schema 版本查询单源。
    chk(&mut set, "C429-09", bk.schema_ver(SchemaKind::Translate) == 4
        && bk.schema_ver(SchemaKind::Observe) == 4, "版本查询");

    // C429-10 冻结幂等（重置一致版本仍可冻）。
    chk(&mut set, "C429-10", bk.freeze(4).is_ok() && bk.frozen_ver == 4, "冻结幂等");

    // C429-11 文档组幂等。
    bk.put_doc(DocGroup::G1);
    chk(&mut set, "C429-11", bk.docs.len() == 8, "文档组幂等");

    // C429-12 八组+四 schema 单源断言。
    chk(&mut set, "C429-12", DocGroup::ALL.len() == 8 && SchemaKind::ALL.len() == 4
        && SchemaKind::ALL[0] == SchemaKind::Translate, "单源断言");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves429_smoke() {
        let set = run_ves429_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves429 red: {:?}", red);
    }
}
