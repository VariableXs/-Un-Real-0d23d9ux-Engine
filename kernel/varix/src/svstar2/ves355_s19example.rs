//! ves355_s19example — VE-F4115 · T06 地区示例
//! 示例集：新地区接入全流程样例（规则配置+格式+合规检查）+ 地区切换模拟示例 + 可执行跑通即验收 + 示例跑挂即缺陷 + 地区缺示例接入阻断 + 无障碍合规检查示范（域本色）+ 示例数据虚构。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC840;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    Ok = 0,
    RunFail = SEG_BASE + 0x01,
    BlockNoSample = SEG_BASE + 0x02,
    DupExample = SEG_BASE + 0x03,
}

/// 接入全流程四主题。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExTheme { RuleConfig = 0, Format = 1, ComplianceCheck = 2, RegionSwitch = 3 }

impl ExTheme {
    pub const ALL: [ExTheme; 4] = [
        ExTheme::RuleConfig, ExTheme::Format, ExTheme::ComplianceCheck, ExTheme::RegionSwitch];
    pub fn name(self) -> &'static str {
        match self { ExTheme::RuleConfig => "rule-config", ExTheme::Format => "format",
            ExTheme::ComplianceCheck => "compliance-check", ExTheme::RegionSwitch => "region-switch" }
    }
}

/// 一条示例：虚构地区 + 脚本产出。
#[derive(Clone)]
pub struct Example {
    pub theme: ExTheme,
    pub region: &'static str,   // 虚构地区码如 "Z1"
    pub artifact: &'static str, // 示例产物名
    pub a11y_demo: bool,        // 无障碍合规检查示范（域本色）
}

pub struct ExampleBook { pub examples: Vec<Example> }

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { examples: Vec::new() } }

    pub fn add(&mut self, e: Example) -> Result<(), ExCode> {
        if e.region.is_empty() { return Err(ExCode::BlockNoSample); }
        if self.examples.iter().any(|x| x.theme == e.theme && x.region == e.region) {
            return Err(ExCode::DupExample);
        }
        self.examples.push(e);
        Ok(())
    }

    /// 全流程覆盖：四主题齐备。
    pub fn region_covered(&self, region: &str) -> bool {
        ExTheme::ALL.iter().all(|t| self.examples.iter().any(|e| e.theme == *t && e.region == region))
    }

    /// 接入门：四主题齐 + 合规示范在 → 放行；缺则阻断。
    pub fn admission_gate(&self, region: &str) -> Result<(), ExCode> {
        if !self.region_covered(region) { return Err(ExCode::BlockNoSample); }
        if !self.examples.iter().filter(|e| e.region == region)
            .any(|e| e.theme == ExTheme::ComplianceCheck && e.a11y_demo) {
            return Err(ExCode::BlockNoSample);
        }
        Ok(())
    }

    /// 可执行验收。
    pub fn execute_all(&self, script: impl Fn(&Example) -> bool) -> Result<usize, ExCode> {
        let mut n = 0;
        for e in self.examples.iter() {
            if !script(e) { return Err(ExCode::RunFail); }
            n += 1;
        }
        Ok(n)
    }
}

/// 阻断立案册。
pub struct BlockBook { pub cases: Vec<(&'static str, &'static str)> }

impl BlockBook {
    pub fn new() -> Self { BlockBook { cases: Vec::new() } }
    pub fn open(&mut self, region: &'static str, missing: &'static str) { self.cases.push((region, missing)); }
    pub fn count(&self) -> usize { self.cases.len() }
}

/// 地区切换模拟：模拟器内部序列（不写真实配置）。
pub struct SwitchSim { pub steps: Vec<(&'static str, bool)>, pub real_touched: bool }

impl SwitchSim {
    pub fn new() -> Self { SwitchSim { steps: Vec::new(), real_touched: false } }
    pub fn step(&mut self, name: &'static str, ok: bool) {
        self.steps.push((name, ok));
        if !ok { self.real_touched = true; } // 失败步污染即示例缺陷信号
    }
    pub fn all_ok(&self) -> bool { !self.steps.is_empty() && self.steps.iter().all(|(_, ok)| *ok) && !self.real_touched }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves355_checks() -> CheckSet {
    let mut set = CheckSet::new("ves355");
    let mut book = ExampleBook::new();

    let mk = |t: ExTheme| Example { theme: t, region: "Z1", artifact: "demo", a11y_demo: t == ExTheme::ComplianceCheck };

    // C355-01 四主题示例逐条入库。
    for t in ExTheme::ALL.iter() { let _ = book.add(mk(*t)); }
    chk(&mut set, "C355-01", book.examples.len() == 4, "Z1 四主题全登");

    // C355-02 重复示例拒。
    chk(&mut set, "C355-02", book.add(mk(ExTheme::Format)) == Err(ExCode::DupExample), "同主题同地区 => Dup");

    // C355-03 全流程覆盖判定。
    chk(&mut set, "C355-03", book.region_covered("Z1") && !book.region_covered("Z2"), "Z1 齐 Z2 缺");

    // C355-04 接入门放行（合规示范在）。
    chk(&mut set, "C355-04", book.admission_gate("Z1").is_ok(), "Z1 接入放行");

    // C355-05 缺合规示范 → 阻断。
    let mut book2 = ExampleBook::new();
    for t in ExTheme::ALL.iter() {
        let _ = book2.add(Example { theme: *t, region: "Z2", artifact: "d", a11y_demo: false });
    }
    let mut bb = BlockBook::new();
    match book2.admission_gate("Z2") {
        Err(ExCode::BlockNoSample) => bb.open("Z2", "a11y-demo"),
        _ => {}
    }
    chk(&mut set, "C355-05", bb.count() == 1, "无障碍示范缺 → 阻断立案");

    // C355-06 可执行验收：全跑通。
    let ok_script = |_: &Example| true;
    chk(&mut set, "C355-06", book.execute_all(ok_script) == Ok(4), "四例全跑通");

    // C355-07 示例跑挂即缺陷。
    let bad_script = |e: &Example| e.theme != ExTheme::RegionSwitch;
    chk(&mut set, "C355-07", book.execute_all(bad_script) == Err(ExCode::RunFail), "一例挂 => 缺陷");

    // C355-08 主题名单单源。
    chk(&mut set, "C355-08", ExTheme::ALL[0].name() == "rule-config" && ExTheme::ALL[3].name() == "region-switch", "四主题单源");

    // C355-09 切换模拟：全步成功且不碰真实。
    let mut ss = SwitchSim::new();
    ss.step("load-cfg", true);
    ss.step("apply-fmt", true);
    ss.step("verify-compliance", true);
    chk(&mut set, "C355-09", ss.all_ok() && !ss.real_touched, "三步切换模拟全绿");

    // C355-10 失败步污染检出。
    let mut ss2 = SwitchSim::new();
    ss2.step("load-cfg", true);
    ss2.step("apply-fmt", false);
    chk(&mut set, "C355-10", !ss2.all_ok() && ss2.real_touched, "失败步 → 示例缺陷信号");

    // C355-11 空模拟不判绿。
    chk(&mut set, "C355-11", !SwitchSim::new().all_ok(), "空步序不判绿");

    // C355-12 示例地区全虚构。
    chk(&mut set, "C355-12", book.examples.iter().all(|e| e.region == "Z1" && e.region.len() == 2), "虚构地区码 Z1");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves355_smoke() {
        let set = run_ves355_checks();
        assert!(set.failed_count() == 0, "ves355 red: {}", set.red_items_text());
    }

    #[test]
    fn ves355_block() {
        let book = ExampleBook::new();
        assert_eq!(book.admission_gate("Z9"), Err(ExCode::BlockNoSample));
    }
}
