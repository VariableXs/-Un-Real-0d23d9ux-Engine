//! ves411_s22docasset — VE-F4171 · 文档资产汇总
//! T 域文档资产汇总：八组文档/schema/示例三类归集；汇总规则（每资产挂组/挂条/挂版本三指针 +
//! 读者画像标注）；与 F4151 文档总纲对拍；版本漂移→对版修正；画像缺→补标；对拍不符→留痕修正；
//! 多语言无障碍指南专册（域本色）；示例不含真实用户数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0BE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaCode {
    Ok = 0,
    VersionDrift = SEG_BASE + 0x01, // 0x0BE1
    PortraitMissing = SEG_BASE + 0x02, // 0x0BE2
    Mismatch = SEG_BASE + 0x03,     // 0x0BE3
}

/// 三类文档资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind { Manual = 0, Schema = 1, Example = 2 }

impl DocKind {
    pub const ALL: [DocKind; 3] = [DocKind::Manual, DocKind::Schema, DocKind::Example];
    pub fn name(self) -> &'static str {
        match self { DocKind::Manual => "manual", DocKind::Schema => "schema", DocKind::Example => "example" }
    }
}

/// 读者画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Portrait { Dev = 0, Translator = 1, LocaleOps = 2, A11y = 3 }

impl Portrait {
    pub fn name(self) -> &'static str {
        match self { Portrait::Dev => "dev", Portrait::Translator => "translator",
            Portrait::LocaleOps => "locale-ops", Portrait::A11y => "a11y" }
    }
}

/// 条目：三指针（组/条/版本）+ 画像 + 域本色专册标记。
#[derive(Clone, Copy)]
pub struct DocAsset {
    pub kind: DocKind,
    pub group: u8,
    pub card: u32,
    pub version: u32,      // 挂版本（0 = 缺）
    pub portraits: u8,     // 画像位图 bit0=dev bit1=translator bit2=locale-ops bit3=a11y
    pub a11y_handbook: bool, // 多语言无障碍指南专册（域本色）
    pub clean_example: bool, // 示例不含真实用户数据
}

impl DocAsset {
    pub fn has_portrait(&self, p: Portrait) -> bool {
        self.portraits & (1 << p as u8) != 0
    }
    pub fn linked(&self) -> bool { self.version != 0 && self.card != 0 && self.group < 8 }
}

/// 台账。
pub struct DocBook {
    pub items: Vec<DocAsset>,
    pub fixes: Vec<&'static str>,
}

impl DocBook {
    pub fn new() -> Self { DocBook { items: Vec::new(), fixes: Vec::new() } }

    pub fn put(&mut self, e: DocAsset) {
        match self.items.iter_mut().find(|x| x.kind == e.kind && x.group == e.group) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 三类×八组完备（24 格）。
    pub fn grid_complete(&self) -> bool {
        DocKind::ALL.iter().all(|k| (0..8).all(|g| self.items.iter().any(|x| x.kind == *k && x.group == g)))
    }

    /// 版本三方对拍（指定三格版本一致）。
    pub fn versions_aligned(&self, cells: &[(DocKind, u8)], expect: u32) -> bool {
        cells.iter().all(|(k, g)| {
            self.items.iter().any(|x| x.kind == *k && x.group == *g && x.version == expect)
        })
    }

    /// 对版修正。
    pub fn align(&mut self, cells: &[(DocKind, u8)], expect: u32) -> usize {
        let mut n = 0;
        for x in self.items.iter_mut() {
            if cells.contains(&(x.kind, x.group)) && x.version != expect {
                x.version = expect;
                self.fixes.push("version-aligned");
                n += 1;
            }
        }
        n
    }

    /// 画像缺补标。
    pub fn add_portrait(&mut self, idx: usize, p: Portrait) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.portraits |= 1 << p as u8; self.fixes.push("portrait-added"); true }
            None => false,
        }
    }

    /// 与 F4151 文档总纲对拍：指定格存在即一致。
    pub fn agrees_master(&self, cells: &[(DocKind, u8)]) -> bool {
        cells.iter().all(|(k, g)| self.items.iter().any(|x| x.kind == *k && x.group == *g))
    }

    /// 域本色：无障碍指南专册在账。
    pub fn a11y_handbook_present(&self) -> bool {
        self.items.iter().any(|x| x.a11y_handbook && x.linked())
    }

    /// 示例数据洁净核验。
    pub fn examples_clean(&self) -> bool {
        self.items.iter().filter(|x| x.kind == DocKind::Example).all(|x| x.clean_example)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves411_checks() -> CheckSet {
    let mut set = CheckSet::new("ves411");
    let mut bk = DocBook::new();

    // C411-01 三类×八组 24 格归集（版本 3 + 全画像）。
    for k in DocKind::ALL.iter() {
        for g in 0..8u8 {
            bk.put(DocAsset { kind: *k, group: g, card: 4000 + g as u32, version: 3,
                portraits: 0x0F, a11y_handbook: false, clean_example: true });
        }
    }
    chk(&mut set, "C411-01", bk.grid_complete() && bk.items.len() == 24, "24 格齐");

    // C411-02 版本三方对拍一致。
    let cells = [(DocKind::Manual, 0u8), (DocKind::Schema, 1u8), (DocKind::Example, 2u8)];
    chk(&mut set, "C411-02", bk.versions_aligned(&cells, 3), "三方版本 3 一致");

    // C411-03 版本漂移检出与对版修正。
    bk.put(DocAsset { kind: DocKind::Manual, group: 0, card: 4000, version: 4, portraits: 0x0F, a11y_handbook: false, clean_example: true });
    chk(&mut set, "C411-03", !bk.versions_aligned(&cells, 3), "漂移检出");

    // C411-04 修正留痕。
    let n = bk.align(&cells, 3);
    chk(&mut set, "C411-04", n == 1 && bk.versions_aligned(&cells, 3)
        && bk.fixes.contains(&"version-aligned"), "对版留痕");

    // C411-05 画像缺补标。
    let mut bk2 = DocBook::new();
    bk2.put(DocAsset { kind: DocKind::Manual, group: 0, card: 1, version: 2, portraits: 0x01, a11y_handbook: false, clean_example: true });
    let has_a11y_before = bk2.items[0].has_portrait(Portrait::A11y);
    bk2.add_portrait(0, Portrait::A11y);
    chk(&mut set, "C411-05", !has_a11y_before && bk2.items[0].has_portrait(Portrait::A11y)
        && bk2.fixes.contains(&"portrait-added"), "画像补标留痕");

    // C411-06 与 F4151 总纲对拍。
    chk(&mut set, "C411-06", bk.agrees_master(&cells), "总纲对拍一致");

    // C411-07 域本色：无障碍指南专册在账。
    bk.put(DocAsset { kind: DocKind::Manual, group: 7, card: 4151, version: 3, portraits: 0x09, a11y_handbook: true, clean_example: true });
    chk(&mut set, "C411-07", bk.a11y_handbook_present(), "专册在账");

    // C411-08 示例洁净核验。
    chk(&mut set, "C411-08", bk.examples_clean(), "示例全虚构");

    // C411-09 污染示例检出。
    let mut bk3 = DocBook::new();
    bk3.put(DocAsset { kind: DocKind::Example, group: 0, card: 1, version: 1, portraits: 1, a11y_handbook: false, clean_example: false });
    chk(&mut set, "C411-09", !bk3.examples_clean(), "真实数据检出");

    // C411-10 三类名单单源。
    chk(&mut set, "C411-10", DocKind::ALL.len() == 3 && DocKind::ALL[0].name() == "manual"
        && DocKind::ALL[2].name() == "example", "三类单源");

    // C411-11 画像名单单源。
    chk(&mut set, "C411-11", Portrait::Dev.name() == "dev" && Portrait::A11y.name() == "a11y", "画像单源");

    // C411-12 空册红。
    let empty = DocBook::new();
    chk(&mut set, "C411-12", !empty.grid_complete() && !empty.a11y_handbook_present(), "空册红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves411_smoke() {
        let set = run_ves411_checks();
        assert!(set.failed_count() == 0, "ves411 red: {}", set.red_items_text());
    }
}
