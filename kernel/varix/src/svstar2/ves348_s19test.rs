//! ves348_s19test — VE-F4108 · T06 地区测试
//! 三层测试（单测 地区码/格式/校验 + 集成 地区切换全链 + 回归 驻留路由不回退断言）+ 地区×语种×设备三轴矩阵抽样测试（漏格补测）+ 层间对拍 + 无障碍合规列断言（域本色）+ 语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC240;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    Ok = 0,
    RouteRegression = SEG_BASE + 0x01,
    MatrixHole = SEG_BASE + 0x02,
    LayerMismatch = SEG_BASE + 0x03,
}

/// 三层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TLayer { Unit = 0, Integration = 1, Regression = 2 }

/// 三轴矩阵抽样：地区×语种×设备。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell { pub region: &'static str, pub lang: &'static str, pub device: &'static str }

pub struct AxisMatrix {
    pub cells_tested: Vec<Cell>,
    pub regions: Vec<&'static str>,
    pub langs: Vec<&'static str>,
    pub devices: Vec<&'static str>,
}

impl AxisMatrix {
    pub fn new(regions: Vec<&'static str>, langs: Vec<&'static str>, devices: Vec<&'static str>) -> Self {
        AxisMatrix { cells_tested: Vec::new(), regions, langs, devices }
    }

    pub fn total(&self) -> usize { self.regions.len() * self.langs.len() * self.devices.len() }

    pub fn mark(&mut self, c: Cell) {
        if !self.cells_tested.contains(&c) { self.cells_tested.push(c); }
    }

    /// 抽样口径：每地区至少抽 devices.len() 格（地区全覆盖×设备全档）。
    pub fn holes_sampled(&self) -> Vec<Cell> {
        let mut out = Vec::new();
        for r in self.regions.iter() {
            for d in self.devices.iter() {
                let covered = self.cells_tested.iter().any(|c| c.region == *r && c.device == *d);
                if !covered {
                    out.push(Cell { region: r, lang: self.langs[0], device: d });
                }
            }
        }
        out
    }
}

/// 三层测试册。
pub struct LayerBook { pub passed: Vec<(TLayer, &'static str)>, pub failed: Vec<(TLayer, &'static str)> }

impl LayerBook {
    pub fn new() -> Self { LayerBook { passed: Vec::new(), failed: Vec::new() } }
    pub fn record(&mut self, l: TLayer, case: &'static str, ok: bool) {
        if ok { self.passed.push((l, case)); } else { self.failed.push((l, case)); }
    }
    /// 单测三面：地区码/格式/校验。
    pub fn unit_faces(&self) -> bool {
        ["region-code", "fmt", "validate"].iter().all(|f|
            self.passed.iter().any(|(l, c)| *l == TLayer::Unit && c.starts_with(f)))
    }
    /// 层间对拍：同名案例两层结论一致。
    pub fn cross(&self, a: &'static str, b: &'static str) -> Result<(), RgCode> {
        let pa = self.passed.iter().any(|(_, c)| *c == a) && !self.failed.iter().any(|(_, c)| *c == a);
        let pb = self.passed.iter().any(|(_, c)| *c == b) && !self.failed.iter().any(|(_, c)| *c == b);
        if pa == pb { Ok(()) } else { Err(RgCode::LayerMismatch) }
    }
}

/// 驻留路由不回退断言（回归层核心）：路由结论序列必须单调（一旦进白名单落点不得回退到暂存）。
pub fn route_regression(seq: &[bool]) -> Result<(), RgCode> {
    // seq: true=已路由到白名单落点；一旦 true 不得再 false。
    let mut seen_ok = false;
    for ok in seq.iter() {
        if *ok { seen_ok = true; }
        else if seen_ok { return Err(RgCode::RouteRegression); }
    }
    Ok(())
}

/// 无障碍合规列断言（域本色）：地区合规列必须覆盖 ADA/EN301549。
pub fn a11y_column_ok(regions: &[&str], covered: &[&str]) -> bool {
    regions.iter().all(|r| covered.iter().any(|c| c.contains(r)))
}

/// 语料合成：确定性地区语料。
pub fn synth_regions(seed: u32, n: usize) -> Vec<u32> {
    let mut h = seed;
    let mut out = Vec::new();
    for _ in 0..n {
        h = h.wrapping_mul(0x0100_0193) ^ 0x85EB_CA6B;
        out.push(h % 0xFFFF);
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves348_checks() -> CheckSet {
    let mut set = CheckSet::new("ves348");
    let mut lb = LayerBook::new();

    // C348-01 单测三面。
    lb.record(TLayer::Unit, "region-code-bcp47", true);
    lb.record(TLayer::Unit, "fmt-negative-paren", true);
    lb.record(TLayer::Unit, "validate-zip", true);
    chk(&mut set, "C348-01", lb.unit_faces(), "单测三面齐");

    // C348-02 集成层：地区切换全链。
    lb.record(TLayer::Integration, "chain-region-switch", true);
    chk(&mut set, "C348-02", lb.passed.iter().any(|(l, c)| *l == TLayer::Integration), "集成全链过");

    // C348-03 回归层：路由不回退。
    chk(&mut set, "C348-03", route_regression(&[false, true, true, true]).is_ok(), "路由单调不回退");

    // C348-04 路由回退 → 阻断。
    chk(&mut set, "C348-04", route_regression(&[true, false, true]) == Err(RgCode::RouteRegression), "true→false => 回退阻断");

    // C348-05 全暂存序列合法（未路由过）。
    chk(&mut set, "C348-05", route_regression(&[false, false]).is_ok(), "全暂存不判回退");

    // C348-06 三轴矩阵：2 地区×2 设备抽样 4 格。
    let mut m = AxisMatrix::new(vec!["US", "DE"], vec!["en", "de"], vec!["low", "high"]);
    m.mark(Cell { region: "US", lang: "en", device: "low" });
    m.mark(Cell { region: "US", lang: "en", device: "high" });
    m.mark(Cell { region: "DE", lang: "de", device: "low" });
    chk(&mut set, "C348-06", m.total() == 8 && m.holes_sampled() == vec![
        Cell { region: "DE", lang: "en", device: "high" }], "抽样 4 格缺 1（DE×high）");

    // C348-07 补测后漏格清零。
    m.mark(Cell { region: "DE", lang: "de", device: "high" });
    chk(&mut set, "C348-07", m.holes_sampled().is_empty(), "补测漏格清零");

    // C348-08 重复 mark 幂等。
    let n0 = m.cells_tested.len();
    m.mark(Cell { region: "US", lang: "en", device: "low" });
    chk(&mut set, "C348-08", m.cells_tested.len() == n0, "重复标记幂等");

    // C348-09 层间对拍一致。
    chk(&mut set, "C348-09", lb.cross("region-code-bcp47", "chain-region-switch").is_ok(), "双层同绿");

    // C348-10 层间失配检出。
    lb.record(TLayer::Regression, "route-regression", false);
    chk(&mut set, "C348-10", lb.cross("region-code-bcp47", "route-regression") == Err(RgCode::LayerMismatch), "一绿一红 => 失配");

    // C348-11 无障碍合规列断言（域本色）。
    chk(&mut set, "C348-11",
        a11y_column_ok(&["US", "EU"], &["US:ADA", "EU:EN301549"])
        && !a11y_column_ok(&["US", "JP"], &["US:ADA"]), "合规列覆盖断言双态");

    // C348-12 语料合成确定性。
    let c1 = synth_regions(7, 6);
    let c2 = synth_regions(7, 6);
    chk(&mut set, "C348-12", c1 == c2 && c1.len() == 6, "合成语料确定");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves348_smoke() {
        let set = run_ves348_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves348 red: {:?}", red);
    }

    #[test]
    fn ves348_route() {
        assert!(route_regression(&[]).is_ok());
        assert_eq!(route_regression(&[true, false]), Err(RgCode::RouteRegression));
    }
}
