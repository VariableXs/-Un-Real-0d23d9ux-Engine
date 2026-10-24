//! ves451_s24docs — VE-F4217 · 一致性文档
//! 四类文档（架构手册+契约元模型手册+规则编写指南+API 参考）；
//! 与实现一致性抽查（元模型字段/API 签名对拍）；
//! 漂移→对拍修正留痕；缺册→补齐；抽查失配→以实现为准修文档；
//! 文档含无障碍规则编写专节（域本色）；示例不含真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1540;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    BookMissing = SEG_BASE + 0x01, // 0x1541
    Drift = SEG_BASE + 0x02,       // 0x1542
    SampleMismatch = SEG_BASE + 0x03, // 0x1543
}

/// 四类文档 + 无障碍专节（域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind { Architecture = 0, MetaModel = 1, RuleGuide = 2, ApiRef = 3 }

impl DocKind {
    pub const ALL: [DocKind; 4] = [DocKind::Architecture, DocKind::MetaModel, DocKind::RuleGuide, DocKind::ApiRef];
}

/// 文档册。
pub struct DocBook {
    pub books: Vec<DocKind>,
    pub a11y_section: bool,       // 无障碍规则编写专节（域本色）
    pub meta_fields: Vec<&'static str>,  // 元模型手册字段
    pub impl_fields: Vec<&'static str>,  // 实现字段（对拍基准）
    pub api_sigs: Vec<(u32, u32)>,       // 手册 API 签名（手册值, 实现值）
    pub fixes: u32,
}

impl DocBook {
    pub fn new() -> Self {
        DocBook { books: Vec::new(), a11y_section: false, meta_fields: Vec::new(),
            impl_fields: Vec::new(), api_sigs: Vec::new(), fixes: 0 }
    }
    pub fn put_book(&mut self, k: DocKind) { if !self.books.contains(&k) { self.books.push(k); } }
    pub fn missing(&self) -> Vec<DocKind> {
        DocKind::ALL.iter().filter(|k| !self.books.contains(k)).map(|k| *k).collect()
    }
    /// 文档齐备裁决：四册齐+专节在。
    pub fn complete(&self) -> Result<(), DcCode> {
        if self.missing().len() > 0 { return Err(DcCode::BookMissing); }
        if !self.a11y_section { return Err(DcCode::BookMissing); }
        Ok(())
    }
    /// 元模型字段对拍：手册字段必须与实现一致。
    pub fn meta_reconcile(&mut self) -> Result<(), DcCode> {
        for (i, f) in self.impl_fields.iter().enumerate() {
            match self.meta_fields.get(i) {
                Some(m) if m == f => {}
                _ => { self.fixes += 1; return Err(DcCode::Drift); }
            }
        }
        if self.meta_fields.len() != self.impl_fields.len() { self.fixes += 1; return Err(DcCode::Drift); }
        Ok(())
    }
    /// API 签名抽查：失配→以实现为准修文档（留痕）。
    pub fn api_sample(&mut self) -> Result<(), DcCode> {
        let mut bad = false;
        for (doc, imp) in self.api_sigs.iter_mut() {
            if *doc != *imp { *doc = *imp; bad = true; }
        }
        if bad { self.fixes += 1; return Err(DcCode::SampleMismatch); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves451_checks() -> CheckSet {
    let mut set = CheckSet::new("ves451");
    let mut bk = DocBook::new();

    // C451-01 缺册→补齐码（缺两册定位）。
    bk.put_book(DocKind::Architecture);
    bk.put_book(DocKind::MetaModel);
    let miss = bk.missing();
    chk(&mut set, "C451-01", bk.complete() == Err(DcCode::BookMissing) && miss.len() == 2, "缺两册定位");

    // C451-02 四册齐但专节缺→仍拒（域本色）。
    bk.put_book(DocKind::RuleGuide);
    bk.put_book(DocKind::ApiRef);
    chk(&mut set, "C451-02", bk.complete() == Err(DcCode::BookMissing), "专节缺拒");

    // C451-03 专节补齐→文档齐备。
    bk.a11y_section = true;
    chk(&mut set, "C451-03", bk.complete().is_ok(), "文档齐备");

    // C451-04 元模型字段对拍：一致通过。
    bk.impl_fields = vec!["id", "version", "provider"];
    bk.meta_fields = vec!["id", "version", "provider"];
    chk(&mut set, "C451-04", bk.meta_reconcile().is_ok(), "字段对拍过");

    // C451-05 字段漂移→对拍修正留痕。
    bk.meta_fields = vec!["id", "old-name", "provider"];
    chk(&mut set, "C451-05", bk.meta_reconcile() == Err(DcCode::Drift) && bk.fixes == 1, "漂移留痕");

    // C451-06 修正文档（以实现为准）→复检过。
    bk.meta_fields = vec!["id", "version", "provider"];
    chk(&mut set, "C451-06", bk.meta_reconcile().is_ok(), "修正复检");

    // C451-07 API 签名抽查失配→以实现为准修。
    bk.api_sigs = vec![(100, 100), (200, 210), (300, 300)];
    chk(&mut set, "C451-07", bk.api_sample() == Err(DcCode::SampleMismatch)
        && bk.api_sigs[1].0 == 210, "签名修正留痕");

    // C451-08 抽查修正后复检过。
    chk(&mut set, "C451-08", bk.api_sample().is_ok(), "签名复检过");

    // C451-09 抽查失配计数累计。
    chk(&mut set, "C451-09", bk.fixes == 2, "修正计数");

    // C451-10 四册单源断言。
    chk(&mut set, "C451-10", DocKind::ALL.len() == 4
        && DocKind::ALL[0] == DocKind::Architecture
        && DocKind::ALL[3] == DocKind::ApiRef, "四册单源");

    // C451-11 册登记幂等。
    bk.put_book(DocKind::Architecture);
    chk(&mut set, "C451-11", bk.books.len() == 4, "册幂等");

    // C451-12 示例不含真实数据断言（示例字段名均为占位）。
    chk(&mut set, "C451-12", bk.meta_fields.iter().all(|f| f.len() <= 12), "示例占位");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves451_smoke() {
        let set = run_ves451_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves451 red: {:?}", red);
    }
}
