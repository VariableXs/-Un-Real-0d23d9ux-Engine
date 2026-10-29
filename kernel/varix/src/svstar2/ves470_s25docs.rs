//! ves470_s25docs — VE-F4236 · 契约文档
//! 四类手册（DSL 语言手册+验证规则手册+版本管理指南+冲突仲裁手册）；
//! 与实现一致性抽查（语法/六检/版本语义对拍）；漂移→对拍修正留痕；缺册→补齐；抽查失配→以实现为准修文档；
//! 手册含无障碍判据编写专节（域本色）；示例不含真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1920;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    BookMissing = SEG_BASE + 0x01, // 0x1921
    Drift = SEG_BASE + 0x02,       // 0x1922
    SampleMismatch = SEG_BASE + 0x03, // 0x1923
}

/// 四类手册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Manual { DslLang = 0, ValidateRule = 1, VersionGuide = 2, ConflictArb = 3 }

impl Manual {
    pub const ALL: [Manual; 4] = [Manual::DslLang, Manual::ValidateRule, Manual::VersionGuide, Manual::ConflictArb];
}

/// 手册册。
pub struct ManualBook {
    pub books: Vec<Manual>,
    pub a11y_section: bool, // 无障碍判据编写专节（域本色）
    pub syntax_doc: &'static str,  // 语法手册记录
    pub syntax_impl: &'static str, // 实现语法（对拍基准）
    pub six_doc: [bool; 6],        // 手册六检描述
    pub six_impl: [bool; 6],       // 实现六检
    pub ver_doc: u32,              // 手册版本语义
    pub ver_impl: u32,
    pub fixes: u32,
}

impl ManualBook {
    pub fn new() -> Self {
        ManualBook { books: Vec::new(), a11y_section: false, syntax_doc: "", syntax_impl: "",
            six_doc: [true; 6], six_impl: [true; 6], ver_doc: 1, ver_impl: 1, fixes: 0 }
    }
    pub fn put(&mut self, m: Manual) { if !self.books.contains(&m) { self.books.push(m); } }
    pub fn missing(&self) -> Vec<Manual> {
        Manual::ALL.iter().filter(|m| !self.books.contains(m)).map(|m| *m).collect()
    }
    pub fn complete(&self) -> Result<(), DcCode> {
        if self.missing().len() > 0 { return Err(DcCode::BookMissing); }
        if !self.a11y_section { return Err(DcCode::BookMissing); }
        Ok(())
    }
    /// 语法对拍：失配→以实现为准修（留痕）。
    pub fn syntax_reconcile(&mut self) -> Result<(), DcCode> {
        if self.syntax_doc != self.syntax_impl {
            self.syntax_doc = self.syntax_impl;
            self.fixes += 1;
            return Err(DcCode::Drift);
        }
        Ok(())
    }
    /// 六检对拍。
    pub fn six_reconcile(&mut self) -> Result<(), DcCode> {
        if self.six_doc != self.six_impl {
            self.six_doc = self.six_impl;
            self.fixes += 1;
            return Err(DcCode::SampleMismatch);
        }
        Ok(())
    }
    /// 版本语义对拍。
    pub fn ver_reconcile(&mut self) -> Result<(), DcCode> {
        if self.ver_doc != self.ver_impl {
            self.ver_doc = self.ver_impl;
            self.fixes += 1;
            return Err(DcCode::Drift);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves470_checks() -> CheckSet {
    let mut set = CheckSet::new("ves470");
    let mut bk = ManualBook::new();

    // C470-01 缺册定位（缺两册）。
    bk.put(Manual::DslLang);
    bk.put(Manual::ValidateRule);
    let miss = bk.missing();
    chk(&mut set, "C470-01", bk.complete() == Err(DcCode::BookMissing) && miss.len() == 2, "缺两册");

    // C470-02 四册齐但专节缺→拒（域本色）。
    bk.put(Manual::VersionGuide);
    bk.put(Manual::ConflictArb);
    chk(&mut set, "C470-02", bk.complete() == Err(DcCode::BookMissing), "专节缺拒");

    // C470-03 专节补齐→齐备。
    bk.a11y_section = true;
    chk(&mut set, "C470-03", bk.complete().is_ok(), "齐备");

    // C470-04 语法对拍：一致过。
    bk.syntax_doc = "v3-grammar"; bk.syntax_impl = "v3-grammar";
    chk(&mut set, "C470-04", bk.syntax_reconcile().is_ok(), "语法一致");

    // C470-05 语法漂移→以实现为准修+留痕。
    bk.syntax_impl = "v3.1-grammar";
    chk(&mut set, "C470-05", bk.syntax_reconcile() == Err(DcCode::Drift)
        && bk.syntax_doc == "v3.1-grammar" && bk.fixes == 1, "漂移修正");

    // C470-06 六检对拍失配→修正。
    bk.six_impl[4] = false;
    chk(&mut set, "C470-06", bk.six_reconcile() == Err(DcCode::SampleMismatch) && bk.fixes == 2, "六检修正");

    // C470-07 六检复检过。
    chk(&mut set, "C470-07", bk.six_reconcile().is_ok(), "六检复检");

    // C470-08 版本语义对拍失配→修正。
    bk.ver_impl = 2;
    chk(&mut set, "C470-08", bk.ver_reconcile() == Err(DcCode::Drift) && bk.ver_doc == 2, "版本修正");

    // C470-09 三对拍全绿终态。
    chk(&mut set, "C470-09", bk.syntax_reconcile().is_ok() && bk.six_reconcile().is_ok()
        && bk.ver_reconcile().is_ok(), "三对拍全绿");

    // C470-10 四册单源断言。
    chk(&mut set, "C470-10", Manual::ALL.len() == 4
        && Manual::ALL[0] == Manual::DslLang
        && Manual::ALL[3] == Manual::ConflictArb, "四册单源");

    // C470-11 册登记幂等。
    bk.put(Manual::DslLang);
    chk(&mut set, "C470-11", bk.books.len() == 4, "册幂等");

    // C470-12 修正计数累计。
    chk(&mut set, "C470-12", bk.fixes == 3, "修正计数");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves470_smoke() {
        let set = run_ves470_checks();
        assert!(set.failed_count() == 0, "ves470 red: {}", set.red_items_text());
    }
}
