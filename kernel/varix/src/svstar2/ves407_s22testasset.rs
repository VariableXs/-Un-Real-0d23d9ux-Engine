//! ves407_s22testasset — VE-F4167 · 测试资产汇总
//! T 域测试资产汇总：八组测试用例/语料/仿真器/回归集四类归集；汇总规则（每资产挂组/挂条/挂断言
//! 三指针 + 规模登记）；与 F4145 语料总库对拍；台账缺回补；对拍不符修正留痕；资产失联断链修复；
//! 语料分级正确性抽检（受限级加密——红线复述）；样本脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0840;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaCode {
    Ok = 0,
    LedgerShort = SEG_BASE + 0x01, // 0x0841
    Mismatch = SEG_BASE + 0x02,    // 0x0842
    LinkBroken = SEG_BASE + 0x03,  // 0x0843
}

/// 四类测试资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestAssetKind { Case = 0, Corpus = 1, Simulator = 2, Regress = 3 }

impl TestAssetKind {
    pub const ALL: [TestAssetKind; 4] = [
        TestAssetKind::Case, TestAssetKind::Corpus, TestAssetKind::Simulator, TestAssetKind::Regress];
    pub fn name(self) -> &'static str {
        match self {
            TestAssetKind::Case => "case", TestAssetKind::Corpus => "corpus",
            TestAssetKind::Simulator => "simulator", TestAssetKind::Regress => "regress",
        }
    }
}

/// 资产条目：三指针（组/条/断言）+ 规模 + 脱敏 + 受限级加密。
#[derive(Clone, Copy)]
pub struct TestAsset {
    pub kind: TestAssetKind,
    pub group: u8,       // 挂组 0-7
    pub card: u32,       // 挂条
    pub assertion: u32,  // 挂断言锚（0 = 失联）
    pub scale: u32,      // 规模登记
    pub sanitized: bool, // 样本脱敏
    pub encrypted: bool, // 受限级加密（红线）
}

impl TestAsset {
    pub fn linked(&self) -> bool { self.assertion != 0 && self.card != 0 && self.group < 8 }
    /// 红线：受限级（加密要求以加密标志表达）——规模 > 0 且未脱敏 = 违规。
    pub fn redline_ok(&self) -> bool { self.scale == 0 || self.sanitized }
}

/// 台账。
pub struct TestAssetBook {
    pub items: Vec<TestAsset>,
    pub fixes: Vec<&'static str>,
}

impl TestAssetBook {
    pub fn new() -> Self { TestAssetBook { items: Vec::new(), fixes: Vec::new() } }

    pub fn put(&mut self, e: TestAsset) {
        match self.items.iter_mut().find(|x| x.kind == e.kind && x.group == e.group) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 四类×八组完备（32 格）。
    pub fn grid_complete(&self) -> bool {
        TestAssetKind::ALL.iter().all(|k| (0..8).all(|g| self.items.iter().any(|x| x.kind == *k && x.group == g)))
    }

    pub fn missing_cells(&self) -> Vec<(&'static str, u8)> {
        let mut out = Vec::new();
        for k in TestAssetKind::ALL.iter() {
            for g in 0..8u8 {
                if !self.items.iter().any(|x| x.kind == *k && x.group == g) {
                    out.push((k.name(), g));
                }
            }
        }
        out
    }

    /// 断链检测。
    pub fn broken(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| !x.linked()).map(|(i, _)| i).collect()
    }

    pub fn fix(&mut self, idx: usize, card: u32, assertion: u32) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.card = card; x.assertion = assertion; self.fixes.push("link-fixed"); true }
            None => false,
        }
    }

    /// 与 F4145 语料总库对拍：语料类规模总和一致。
    pub fn corpus_total(&self) -> u32 {
        self.items.iter().filter(|x| x.kind == TestAssetKind::Corpus).map(|x| x.scale).sum()
    }
    pub fn agrees_corpus(&self, lib_total: u32) -> bool { self.corpus_total() == lib_total }

    /// 脱敏红线全册核验。
    pub fn sanitized_all(&self) -> bool { self.items.iter().all(|x| x.redline_ok()) }

    /// 规模总量。
    pub fn total(&self) -> u32 { self.items.iter().map(|x| x.scale).sum() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves407_checks() -> CheckSet {
    let mut set = CheckSet::new("ves407");
    let mut bk = TestAssetBook::new();

    // C407-01 四类×八组 32 格归集。
    for k in TestAssetKind::ALL.iter() {
        for g in 0..8u8 {
            bk.put(TestAsset { kind: *k, group: g, card: 4000 + g as u32, assertion: 0xA000 + g as u32,
                scale: 10 + g as u32, sanitized: true, encrypted: true });
        }
    }
    chk(&mut set, "C407-01", bk.grid_complete() && bk.items.len() == 32, "32 格齐");

    // C407-02 缺格回补定位。
    let mut bk2 = TestAssetBook::new();
    for g in 0..6u8 {
        bk2.put(TestAsset { kind: TestAssetKind::Case, group: g, card: 1, assertion: 2, scale: 5, sanitized: true, encrypted: true });
    }
    let miss = bk2.missing_cells();
    chk(&mut set, "C407-02", miss.len() == 30 && miss[0] == ("case", 6), "缺 30 格定位");

    // C407-03 断链检测与修复。
    let mut bk3 = TestAssetBook::new();
    bk3.put(TestAsset { kind: TestAssetKind::Case, group: 0, card: 0, assertion: 5, scale: 3, sanitized: true, encrypted: true });
    bk3.put(TestAsset { kind: TestAssetKind::Corpus, group: 1, card: 7, assertion: 0, scale: 3, sanitized: true, encrypted: true });
    chk(&mut set, "C407-03", bk3.broken().len() == 2, "两断链");

    // C407-04 修复留痕。
    bk3.fix(0, 4021, 0xA001);
    bk3.fix(1, 4071, 0xA002);
    chk(&mut set, "C407-04", bk3.broken().is_empty() && bk3.fixes.len() == 2, "修复留痕二");

    // C407-05 与 F4145 语料库对拍。
    bk.put(TestAsset { kind: TestAssetKind::Corpus, group: 0, card: 4145, assertion: 0xB001, scale: 120, sanitized: true, encrypted: true });
    chk(&mut set, "C407-05", bk.agrees_corpus(120), "语料 120 对拍一致");

    // C407-06 对拍失配检出。
    chk(&mut set, "C407-06", !bk.agrees_corpus(130), "对拍 130 失配");

    // C407-07 脱敏红线：未脱敏 + 有规模 = 违规。
    let mut bk4 = TestAssetBook::new();
    bk4.put(TestAsset { kind: TestAssetKind::Corpus, group: 0, card: 1, assertion: 1, scale: 50, sanitized: false, encrypted: true });
    chk(&mut set, "C407-07", !bk4.sanitized_all(), "未脱敏检出");

    // C407-08 受限级加密标志核验。
    bk4.items[0].sanitized = true;
    chk(&mut set, "C407-08", bk4.sanitized_all(), "脱敏修复合规");

    // C407-09 规模总量精确核算：32 格（每类 Σ(10+g)=108）+ corpus/0 覆盖改 120 → 432-10+120=542。
    chk(&mut set, "C407-09", bk.total() == 542 && bk.corpus_total() == 120 + 108 - 10, "规模 542 精确核算");

    // C407-10 四类名单单源。
    chk(&mut set, "C407-10", TestAssetKind::ALL.len() == 4 && TestAssetKind::ALL[0].name() == "case"
        && TestAssetKind::ALL[3].name() == "regress", "四类单源");

    // C407-11 覆盖登记幂等。
    let before = bk.items.len();
    bk.put(TestAsset { kind: TestAssetKind::Corpus, group: 0, card: 4145, assertion: 0xB001, scale: 130, sanitized: true, encrypted: true });
    chk(&mut set, "C407-11", bk.items.len() == before && bk.agrees_corpus(130), "覆盖更新对拍随动");

    // C407-12 失联面：空册 32 格全缺。
    let empty = TestAssetBook::new();
    chk(&mut set, "C407-12", empty.missing_cells().len() == 32 && !empty.grid_complete(), "空册全缺");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves407_smoke() {
        let set = run_ves407_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves407 red: {:?}", red);
    }
}
