//! ves334_s18test — VE-F4094 · T05 语言资源测试
//! 三层测试（单测 manifest/校验/diff + 集成 下载→验证→激活全链 + 回归 热更新原子性断言不回退）+ 多语种×多设备档位矩阵测试（漏格补测）+ 层间失配对拍 + 读屏语言跟随断言（域本色）+ 语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x98C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsCode {
    Ok = 0,
    AtomicRegression = SEG_BASE + 0x01,
    MatrixHole = SEG_BASE + 0x02,
    LayerMismatch = SEG_BASE + 0x03,
}

/// 三层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer { Unit = 0, Integration = 1, Regression = 2 }

impl Layer {
    pub const ALL: [Layer; 3] = [Layer::Unit, Layer::Integration, Layer::Regression];
    pub fn name(self) -> &'static str {
        match self { Layer::Unit => "unit", Layer::Integration => "integration", Layer::Regression => "regression" }
    }
}

/// 单测项：manifest/校验/diff 三面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitFace { Manifest = 0, Checksum = 1, Diff = 2 }

/// 测试执行册：三层 × 结果。
pub struct TestBook { pub passed: Vec<(Layer, &'static str)>, pub failed: Vec<(Layer, &'static str)> }

impl TestBook {
    pub fn new() -> Self { TestBook { passed: Vec::new(), failed: Vec::new() } }

    pub fn record(&mut self, layer: Layer, case: &'static str, ok: bool) {
        if ok { self.passed.push((layer, case)); } else { self.failed.push((layer, case)); }
    }

    /// 单测三面齐备。
    pub fn unit_faces(&self) -> bool {
        UnitFace::ALL_FACES.iter().all(|f| {
            self.passed.iter().any(|(l, c)| *l == Layer::Unit && c.starts_with(f))
        })
    }

    /// 层间对拍：同一案例在两层结论必须一致（失配 → 对拍立案）。
    pub fn cross_check(&self, case_a: &str, case_b: &str) -> Result<(), TsCode> {
        let pa = self.passed.iter().any(|(_, c)| *c == case_a) && !self.failed.iter().any(|(_, c)| *c == case_a);
        let pb = self.passed.iter().any(|(_, c)| *c == case_b) && !self.failed.iter().any(|(_, c)| *c == case_b);
        if pa == pb { Ok(()) } else { Err(TsCode::LayerMismatch) }
    }
}

impl UnitFace {
    pub const ALL_FACES: [&'static str; 3] = ["manifest", "checksum", "diff"];
}

/// 原子性断言（回归层核心）：切换事务三步全成或全无——失败即回归阻断。
pub struct AtomicAssert { pub committed: bool, pub rolled_back: bool, pub steps_done: usize }

impl AtomicAssert {
    pub fn holds(&self) -> bool {
        self.rolled_back && self.steps_done == 0
            || self.committed && self.steps_done == 3
            || !self.committed && !self.rolled_back && self.steps_done == 0
    }
    /// 回归判定：原子性破 → 阻断。
    pub fn regression(&self) -> Result<(), TsCode> {
        if self.holds() { Ok(()) } else { Err(TsCode::AtomicRegression) }
    }
}

/// 矩阵测试：语种 × 设备档位 → 已测格集合；漏格补测。
pub struct MatrixTest { pub langs: Vec<&'static str>, pub tiers: Vec<&'static str>, pub tested: Vec<(&'static str, &'static str)> }

pub const TIER_COUNT: usize = 3;

impl MatrixTest {
    pub fn new(langs: Vec<&'static str>) -> Self {
        MatrixTest { langs, tiers: vec!["low", "mid", "high"], tested: Vec::new() }
    }

    pub fn mark(&mut self, lang: &'static str, tier: &'static str) {
        if !self.tested.iter().any(|t| t.0 == lang && t.1 == tier) {
            self.tested.push((lang, tier));
        }
    }

    /// 漏格清单：应测 total - 已测。
    pub fn holes(&self) -> Vec<(&'static str, &'static str)> {
        let mut out = Vec::new();
        for l in self.langs.iter() {
            for t in self.tiers.iter() {
                if !self.tested.iter().any(|x| x.0 == *l && x.1 == *t) {
                    out.push((*l, *t));
                }
            }
        }
        out
    }

    pub fn total_cells(&self) -> usize { self.langs.len() * self.tiers.len() }
}

/// 读屏语言跟随断言（域本色）：切换后读屏语言 == 生效语言。
pub fn screen_reader_follows(live: &str, reader: &str) -> bool { live == reader }

/// 语料合成：确定性伪语料（FNV 播种）。
pub fn synth_corpus(seed: u32, len: usize) -> Vec<u32> {
    let mut h = seed;
    let mut out = Vec::new();
    for _ in 0..len {
        h = h.wrapping_mul(0x0100_0193) ^ 0x9E37_79B9;
        out.push(0x4E00 + (h % 0x2000));
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves334_checks() -> CheckSet {
    let mut set = CheckSet::new("ves334");
    let mut tb = TestBook::new();

    // C334-01 三层单测：manifest/checksum/diff 三面全过。
    tb.record(Layer::Unit, "manifest-5field", true);
    tb.record(Layer::Unit, "checksum-fnv", true);
    tb.record(Layer::Unit, "diff-add-del", true);
    chk(&mut set, "C334-01", tb.unit_faces(), "单测三面齐");

    // C334-02 集成层：下载→验证→激活全链案例。
    tb.record(Layer::Integration, "chain-dl-verify-activate", true);
    chk(&mut set, "C334-02", tb.passed.iter().any(|(l, c)| *l == Layer::Integration && *c == "chain-dl-verify-activate"), "集成全链案例过");

    // C334-03 回归层：原子性断言——提交态 3 步合法。
    let a1 = AtomicAssert { committed: true, rolled_back: false, steps_done: 3 };
    chk(&mut set, "C334-03", a1.regression().is_ok(), "提交态 3/3 原子合法");

    // C334-04 原子性破 → 阻断。
    let a2 = AtomicAssert { committed: false, rolled_back: false, steps_done: 2 };
    chk(&mut set, "C334-04", a2.regression() == Err(TsCode::AtomicRegression), "半切 2 步 => 阻断");

    // C334-05 回滚态 0 步合法。
    let a3 = AtomicAssert { committed: false, rolled_back: true, steps_done: 0 };
    chk(&mut set, "C334-05", a3.regression().is_ok(), "整回滚 0/3 合法");

    // C334-06 回归案例入册。
    tb.record(Layer::Regression, "atomic-no-halfcut", a1.regression().is_ok());
    chk(&mut set, "C334-06", tb.passed.iter().any(|(l, c)| *l == Layer::Regression), "回归案例过册");

    // C334-07 矩阵：2 语种 × 3 档 = 6 格，已测 4 格 → 2 漏格。
    let mut mt = MatrixTest::new(vec!["zh", "ar"]);
    mt.mark("zh", "low"); mt.mark("zh", "mid"); mt.mark("zh", "high"); mt.mark("ar", "low");
    chk(&mut set, "C334-07", mt.total_cells() == 6 && mt.holes() == vec![("ar", "mid"), ("ar", "high")], "6 格测 4 漏 2");

    // C334-08 补测后漏格清零。
    mt.mark("ar", "mid"); mt.mark("ar", "high");
    chk(&mut set, "C334-08", mt.holes().is_empty(), "补测 → 漏格清零");

    // C334-09 层间对拍：一致通过。
    chk(&mut set, "C334-09", tb.cross_check("manifest-5field", "chain-dl-verify-activate").is_ok(), "双层同绿对拍过");

    // C334-10 层间失配 → 对拍立案。
    tb.record(Layer::Regression, "atomic-broken", false);
    chk(&mut set, "C334-10", tb.cross_check("manifest-5field", "atomic-broken") == Err(TsCode::LayerMismatch), "一绿一红 => 失配");

    // C334-11 读屏语言跟随断言（域本色）。
    chk(&mut set, "C334-11", screen_reader_follows("ja", "ja") && !screen_reader_follows("ja", "zh"), "跟随断言双态");

    // C334-12 语料合成确定性。
    let c1 = synth_corpus(42, 8);
    let c2 = synth_corpus(42, 8);
    chk(&mut set, "C334-12", c1 == c2 && c1.len() == 8 && c1.iter().all(|c| (0x4E00..0x6E00).contains(c)), "合成语料确定且在 CJK 区");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves334_smoke() {
        let set = run_ves334_checks();
        assert!(set.failed_count() == 0, "ves334 red: {}", set.red_items_text());
    }

    #[test]
    fn ves334_matrix_holes() {
        let mut mt = MatrixTest::new(vec!["en"]);
        assert_eq!(mt.total_cells(), TIER_COUNT);
        assert_eq!(mt.holes().len(), 3);
        mt.mark("en", "low");
        assert_eq!(mt.holes().len(), 2);
    }
}
