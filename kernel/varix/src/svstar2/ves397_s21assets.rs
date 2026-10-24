//! ves397_s21assets — VE-F4157 · i18n 资产汇总
//! T 域六维资产汇总（测试/语料/示例/文档/工具/性能——T04-T08 归集）；每资产挂组/挂条/挂指针；
//! 与家族资产总表对拍；台账缺回补；对拍不符修正留痕；失联断链修复；无障碍 i18n 专项列（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    Ok = 0,
    LedgerShort = SEG_BASE + 0x01,   // 0x0041
    Mismatch = SEG_BASE + 0x02,      // 0x0042
    LinkBroken = SEG_BASE + 0x03,    // 0x0043
}

/// 六维资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetDim { Test = 0, Corpus = 1, Example = 2, Doc = 3, Tool = 4, Perf = 5 }

impl AssetDim {
    pub const ALL: [AssetDim; 6] = [
        AssetDim::Test, AssetDim::Corpus, AssetDim::Example,
        AssetDim::Doc, AssetDim::Tool, AssetDim::Perf];
    pub fn name(self) -> &'static str {
        match self {
            AssetDim::Test => "test", AssetDim::Corpus => "corpus", AssetDim::Example => "example",
            AssetDim::Doc => "doc", AssetDim::Tool => "tool", AssetDim::Perf => "perf",
        }
    }
}

/// 资产条目：三指针（挂组/挂条/挂指针）+ 专项列标记。
#[derive(Clone, Copy)]
pub struct AssetEntry {
    pub dim: AssetDim,
    pub owner: &'static str,  // 挂组（T04-T08）
    pub card: &'static str,   // 挂条（卡号）
    pub pointer: u32,         // 挂指针（资产锚，0 = 失联）
    pub a11y: bool,           // 无障碍 i18n 专项列（域本色）
}

impl AssetEntry {
    pub fn linked(&self) -> bool { self.pointer != 0 && !self.card.is_empty() }
}

/// 资产台账。
pub struct AssetBook { pub entries: Vec<AssetEntry> }

impl AssetBook {
    pub fn new() -> Self { AssetBook { entries: Vec::new() } }

    /// 覆盖登记（幂等：同维同卡覆盖）。
    pub fn put(&mut self, e: AssetEntry) {
        match self.entries.iter_mut().find(|x| x.dim == e.dim && x.card == e.card) {
            Some(x) => *x = e,
            None => self.entries.push(e),
        }
    }

    /// 六维齐备核验。
    pub fn dims_complete(&self) -> bool {
        AssetDim::ALL.iter().all(|d| self.entries.iter().any(|e| e.dim == *d))
    }

    /// 台账缺回补定位。
    pub fn missing_dims(&self) -> Vec<&'static str> {
        AssetDim::ALL.iter()
            .filter(|d| !self.entries.iter().any(|e| e.dim == **d))
            .map(|d| d.name()).collect()
    }

    /// 失联断链检测。
    pub fn broken_links(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| !e.linked()).map(|e| e.dim.name()).collect()
    }

    /// 断链修复。
    pub fn fix(&mut self, dim: AssetDim, card: &'static str, pointer: u32) -> bool {
        match self.entries.iter_mut().find(|e| e.dim == dim) {
            Some(e) => { e.card = card; e.pointer = pointer; true }
            None => false,
        }
    }

    /// 与家族资产总表对拍：六维×组名一致。
    pub fn agrees_with_family(&self, family: &[(&'static str, &'static str)]) -> bool {
        family.len() == self.entries.len()
            && family.iter().all(|(owner, dim)| {
                self.entries.iter().any(|e| e.owner == *owner && e.dim.name() == *dim)
            })
    }

    /// 域本色：专项列资产在账。
    pub fn a11y_present(&self) -> bool {
        self.entries.iter().any(|e| e.a11y && e.linked())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves397_checks() -> CheckSet {
    let mut set = CheckSet::new("ves397");
    let mut bk = AssetBook::new();

    // C397-01 六维逐条登记（三指针齐）。
    let dims = AssetDim::ALL;
    for (i, d) in dims.iter().enumerate() {
        bk.put(AssetEntry { dim: *d, owner: "T04", card: "VE-F407x", pointer: 0xA000 + i as u32, a11y: false });
    }
    chk(&mut set, "C397-01", bk.dims_complete() && bk.entries.len() == 6, "六维三指针齐");

    // C397-02 台账缺回补定位。
    let mut bk2 = AssetBook::new();
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T04", card: "VE-F4075", pointer: 1, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Perf, owner: "T07", card: "VE-F4137", pointer: 2, a11y: false });
    chk(&mut set, "C397-02", bk2.missing_dims() == vec!["corpus", "example", "doc", "tool"], "缺四维定位");

    // C397-03 补齐归整。
    for d in bk2.missing_dims() { let _ = d; }
    bk2.put(AssetEntry { dim: AssetDim::Corpus, owner: "T04", card: "VE-F4145", pointer: 3, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Example, owner: "T04", card: "VE-F4150", pointer: 4, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Doc, owner: "T08", card: "VE-F4151", pointer: 5, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "VE-F4141", pointer: 6, a11y: false });
    chk(&mut set, "C397-03", bk2.dims_complete(), "六维补齐");

    // C397-04 断链检测（指针 0 或卡号空）。
    bk2.put(AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "", pointer: 6, a11y: false });
    bk2.put(AssetEntry { dim: AssetDim::Perf, owner: "T07", card: "VE-F4137", pointer: 0, a11y: false });
    chk(&mut set, "C397-04", bk2.broken_links().len() == 2, "两断链检出");

    // C397-05 断链修复。
    chk(&mut set, "C397-05", bk2.fix(AssetDim::Tool, "VE-F4141", 6)
        && bk2.fix(AssetDim::Perf, "VE-F4137", 7) && bk2.broken_links().is_empty(), "断链全修");

    // C397-06 家族总表对拍一致。
    let family = [
        ("T04", "test"), ("T04", "corpus"), ("T04", "example"),
        ("T08", "doc"), ("T08", "tool"), ("T07", "perf")];
    chk(&mut set, "C397-06", bk2.agrees_with_family(&family), "对拍一致");

    // C397-07 对拍失配检出（家族少一维）。
    let fam5 = [&family[0], &family[1], &family[2], &family[3], &family[4]];
    let f5: Vec<(&'static str, &'static str)> = fam5.iter().map(|x| **x).collect();
    chk(&mut set, "C397-07", !bk2.agrees_with_family(&f5), "家族缺维失配");

    // C397-08 修正留痕。
    let mut notes: Vec<&'static str> = Vec::new();
    if !bk2.agrees_with_family(&f5) { notes.push("补 perf 维对拍"); }
    chk(&mut set, "C397-08", notes.len() == 1, "留痕一条");

    // C397-09 域本色：无障碍专项列资产在账。
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T08", card: "VE-F4142-a11y-sr", pointer: 9, a11y: true });
    chk(&mut set, "C397-09", bk2.a11y_present(), "专项列在账");

    // C397-10 覆盖登记幂等。
    let before = bk2.entries.len();
    bk2.put(AssetEntry { dim: AssetDim::Test, owner: "T08", card: "VE-F4142-a11y-sr", pointer: 10, a11y: true });
    chk(&mut set, "C397-10", bk2.entries.len() == before, "覆盖不增行");

    // C397-11 六维名单单源。
    chk(&mut set, "C397-11", AssetDim::ALL.len() == 6 && AssetDim::ALL[0].name() == "test"
        && AssetDim::ALL[5].name() == "perf", "六维单源");

    // C397-12 失联面（空册六维全缺）。
    let empty = AssetBook::new();
    chk(&mut set, "C397-12", empty.missing_dims().len() == 6 && !empty.dims_complete(), "空册全缺");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves397_smoke() {
        let set = run_ves397_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves397 red: {:?}", red);
    }

    #[test]
    fn ves397_link_def() {
        let e = AssetEntry { dim: AssetDim::Tool, owner: "T08", card: "", pointer: 5, a11y: false };
        assert!(!e.linked());
    }
}
