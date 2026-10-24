//! ves321_s17arch — VE-F4081 · T05 多语言资产总纲
//! 资产全生命周期（字体/图片/音频/视频/打包/下载/热更新）+ 与 T04 分工复述单源 + 资产登记三戳（语种/版本/体积）+ 缺戳补登 + 失配隔离审查。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x91C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    Ok = 0,
    StampMissing = SEG_BASE + 0x01,
    LangMismatch = SEG_BASE + 0x02,
    OwnerOverreach = SEG_BASE + 0x03,
    DupAsset = SEG_BASE + 0x04,
}

/// 资产类（非文本——文本归 T04）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssetKind { Font = 0, Image = 1, Audio = 2, Video = 3, Package = 4, Download = 5, HotUpdate = 6 }

impl AssetKind {
    pub const ALL: [AssetKind; 7] = [
        AssetKind::Font, AssetKind::Image, AssetKind::Audio, AssetKind::Video,
        AssetKind::Package, AssetKind::Download, AssetKind::HotUpdate];
}

/// 三戳登记。
#[derive(Clone)]
pub struct AssetEntry {
    pub id: u32,
    pub kind: AssetKind,
    pub lang: &'static str,
    pub version: u32,
    pub size_kb: u32,
    /// 无障碍替代资产项。
    pub has_alt: bool,
}

impl AssetEntry {
    pub fn stamped(&self) -> bool {
        !self.lang.is_empty() && self.version > 0 && self.size_kb > 0
    }
}

pub struct AssetRegistry {
    pub entries: Vec<AssetEntry>,
}

impl AssetRegistry {
    pub fn new() -> Self { AssetRegistry { entries: Vec::new() } }

    pub fn add(&mut self, e: AssetEntry) -> Result<(), AsCode> {
        if !e.stamped() { return Err(AsCode::StampMissing); }
        for x in self.entries.iter() {
            if x.id == e.id { return Err(AsCode::DupAsset); }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 缺戳补登。
    pub fn restamp(&mut self, id: u32, version: u32, size_kb: u32) -> Result<(), AsCode> {
        for e in self.entries.iter_mut() {
            if e.id == id {
                e.version = version;
                e.size_kb = size_kb;
                return Ok(());
            }
        }
        Err(AsCode::StampMissing)
    }

    /// 语种失配隔离审查清单。
    pub fn mismatched(&self, lang: &str) -> Vec<u32> {
        let mut v = Vec::new();
        for e in self.entries.iter() {
            if e.lang != lang && e.lang != "*" { v.push(e.id); }
        }
        v
    }

    /// 无障碍替代资产齐备。
    pub fn alt_missing(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for e in self.entries.iter() {
            if !e.has_alt { v.push(e.id); }
        }
        v
    }
}

/// 分工单源：T04 管文本翻译、T05 管非文本资产。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner { T04Text = 0, T05Asset = 1 }

pub fn division_ok(item: Owner, claimed: Owner) -> Result<(), AsCode> {
    if item == claimed { Ok(()) } else { Err(AsCode::OwnerOverreach) }
}

pub fn run_ves321_checks() -> CheckSet {
    let mut set = CheckSet::new("ves321_s17arch");
    let mut ar = AssetRegistry::new();

    // 1) 缺戳拒。
    set.add("C4081-01", ar.add(AssetEntry { id: 1, kind: AssetKind::Font, lang: "zh", version: 0, size_kb: 0, has_alt: true }) == Err(AsCode::StampMissing), "缺戳拒");

    // 2) 正常登记。
    let _ = ar.add(AssetEntry { id: 1, kind: AssetKind::Font, lang: "zh", version: 2, size_kb: 900, has_alt: true });
    let _ = ar.add(AssetEntry { id: 2, kind: AssetKind::Image, lang: "ar", version: 1, size_kb: 120, has_alt: false });
    set.add("C4081-02", ar.entries.len() == 2, "登记");

    // 3) 重复 id 拒。
    set.add("C4081-03", ar.add(AssetEntry { id: 1, kind: AssetKind::Font, lang: "zh", version: 3, size_kb: 900, has_alt: true }) == Err(AsCode::DupAsset), "重复拒");

    // 4) 补登。
    let _ = ar.restamp(1, 3, 950);
    set.add("C4081-04", ar.entries[0].version == 3 && ar.entries[0].size_kb == 950, "补登");

    // 5) 补登查无拒。
    set.add("C4081-05", ar.restamp(99, 1, 1) == Err(AsCode::StampMissing), "补登查无拒");

    // 6) 失配隔离清单。
    set.add("C4081-06", ar.mismatched("zh") == [2], "失配隔离");

    // 7) 无障碍替代资产缺清单。
    set.add("C4081-07", ar.alt_missing() == [2], "替代资产缺");

    // 8) 分工单源。
    set.add("C4081-08", division_ok(Owner::T05Asset, Owner::T05Asset).is_ok()
        && division_ok(Owner::T04Text, Owner::T05Asset) == Err(AsCode::OwnerOverreach), "分工单源");

    // 9) 七类资产齐。
    set.add("C4081-09", AssetKind::ALL.len() == 7, "七类资产");

    // 10) 段位自查。
    set.add("C4081-10", SEG_BASE == 0x91C0, "段位 0x91C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_flow() {
        let mut ar = AssetRegistry::new();
        assert_eq!(ar.add(AssetEntry { id: 1, kind: AssetKind::Audio, lang: "en", version: 0, size_kb: 5, has_alt: true }), Err(AsCode::StampMissing));
        let _ = ar.add(AssetEntry { id: 1, kind: AssetKind::Audio, lang: "en", version: 1, size_kb: 5, has_alt: true });
        assert!(ar.restamp(1, 2, 6).is_ok());
    }
}
