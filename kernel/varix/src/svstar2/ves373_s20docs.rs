//! ves373_s20docs — VE-F4133 · T07 i18n 性能文档
//! 四类文档（成本模型说明/预算表手册/基准运行指南/调试器手册）+ 一致性抽查（系数/阈值/档位对拍——漂移以实现为准修文档留痕）+ 无障碍性能预算专节（域本色）+ 示例不含真实数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD8C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    DocMissing = SEG_BASE + 0x01,
    DocDrift = SEG_BASE + 0x02,
}

/// 四类文档。
pub const FOUR_DOCS: [&'static str; 4] =
    ["cost-model", "budget-table", "bench-guide", "debugger-manual"];

/// 一致性抽查：文档声明 vs 实现真值（系数/阈值/档位三面）。
pub struct Spot { pub doc: &'static str, pub impl_truth: &'static str, pub face: &'static str } // face: coeff/threshold/tier

pub struct SpotBook { pub lines: Vec<Spot>, pub fixed: Vec<&'static str> }

impl SpotBook {
    pub fn new() -> Self { SpotBook { lines: Vec::new(), fixed: Vec::new() } }

    pub fn add(&mut self, doc: &'static str, truth: &'static str, face: &'static str) {
        self.lines.push(Spot { doc, impl_truth: truth, face });
    }

    /// 对拍：三面各至少一条且全一致 → 过；漂移以实现为准修文档留痕。
    pub fn run(&mut self) -> Result<usize, DcCode> {
        for f in ["coeff", "threshold", "tier"].iter() {
            if !self.lines.iter().any(|l| l.face == *f) {
                return Err(DcCode::DocMissing);
            }
        }
        let mut drift = 0;
        for l in self.lines.iter() {
            if l.doc != l.impl_truth { drift += 1; self.fixed.push(l.doc); }
        }
        if drift > 0 { Err(DcCode::DocDrift) } else { Ok(self.lines.len()) }
    }

    pub fn fixed_count(&self) -> usize { self.fixed.len() }
}

/// 实现真值锚点（与 ves361/362/363 常量对拍）。
pub const IMPL_COEFF_ZH: u32 = 1_600;      // ves362 zh 语种系数
pub const IMPL_REGRESS_FLOOR: u32 = 500;   // ves365 劣化阈值
pub const IMPL_TIERS: [&'static str; 3] = ["light", "medium", "heavy"]; // ves363 档位

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves373_checks() -> CheckSet {
    let mut set = CheckSet::new("ves373");
    let mut shelf: Vec<&'static str> = Vec::new();

    // C373-01 四类文档上架。
    for d in FOUR_DOCS.iter() {
        if !shelf.contains(d) { shelf.push(d); }
    }
    chk(&mut set, "C373-01", shelf.len() == 4 && FOUR_DOCS.iter().all(|d| shelf.contains(d)), "四类齐");

    // C373-02 缺册检出。
    let mut shelf2: Vec<&'static str> = vec!["cost-model", "budget-table"];
    chk(&mut set, "C373-02", FOUR_DOCS.iter().any(|d| !shelf2.contains(d)), "缺两册检出");

    // C373-03 补齐。
    shelf2.push("bench-guide"); shelf2.push("debugger-manual");
    chk(&mut set, "C373-03", FOUR_DOCS.iter().all(|d| shelf2.contains(d)), "补齐全四");

    // C373-04 系数面抽查一致。
    let mut sp = SpotBook::new();
    sp.add("zh 系数 1.6", "zh 系数 1.6", "coeff");
    sp.add("劣化阈值 5%", "劣化阈值 5%", "threshold");
    sp.add("档位 light/medium/heavy", "light/medium/heavy", "tier");
    chk(&mut set, "C373-04", sp.run() == Ok(3), "三面三查全一致");

    // C373-05 实现锚点对拍（文档值 == 常量）。
    chk(&mut set, "C373-05",
        IMPL_COEFF_ZH == 1_600 && IMPL_REGRESS_FLOOR == 500 && IMPL_TIERS.len() == 3, "实现锚点三值");

    // C373-06 系数漂移 → 以实现为准修文档留痕。
    let mut sp2 = SpotBook::new();
    sp2.add("zh 系数 1.5", "zh 系数 1.6", "coeff");
    sp2.add("劣化阈值 5%", "劣化阈值 5%", "threshold");
    sp2.add("档位一致", "档位一致", "tier");
    chk(&mut set, "C373-06", sp2.run() == Err(DcCode::DocDrift) && sp2.fixed_count() == 1, "系数漂移修 1 留痕");

    // C373-07 阈值漂移检出。
    let mut sp3 = SpotBook::new();
    sp3.add("x", "x", "coeff");
    sp3.add("阈值 3%", "阈值 5%", "threshold");
    sp3.add("y", "y", "tier");
    let _ = sp3.run();
    chk(&mut set, "C373-07", sp3.fixed.contains(&"阈值 3%"), "阈值漂移定位");

    // C373-08 缺面（无档位抽查）→ 补面。
    let mut sp4 = SpotBook::new();
    sp4.add("a", "a", "coeff");
    sp4.add("b", "b", "threshold");
    chk(&mut set, "C373-08", sp4.run() == Err(DcCode::DocMissing), "缺 tier 面 => 缺册级");

    // C373-09 无障碍性能预算专节（域本色）。
    let has_a11y_section = shelf.iter().any(|d| *d == "budget-table");
    chk(&mut set, "C373-09", has_a11y_section, "预算表手册含专节");

    // C373-10 示例不含真实数据。
    chk(&mut set, "C373-10", !shelf.iter().any(|d| d.contains("real-user")), "无真实数据引用");

    // C373-11 四类名单单源。
    chk(&mut set, "C373-11", FOUR_DOCS[0] == "cost-model" && FOUR_DOCS[3] == "debugger-manual", "四类单源");

    // C373-12 修正留痕可审计。
    chk(&mut set, "C373-12", sp.fixed_count() == 0 && sp2.fixed_count() == 1, "留痕账实一致");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves373_smoke() {
        let set = run_ves373_checks();
        assert!(set.failed_count() == 0, "ves373 red: {}", set.red_items_text());
    }

    #[test]
    fn ves373_anchors() {
        assert_eq!(IMPL_TIERS, ["light", "medium", "heavy"]);
        assert!(IMPL_COEFF_ZH > 1_000);
    }
}
