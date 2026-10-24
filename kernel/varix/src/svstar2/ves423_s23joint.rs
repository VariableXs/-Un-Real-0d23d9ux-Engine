//! ves423_s23joint — VE-F4183 · T 域联测（40 场景四断言）
//! T 域联测：40 场景跨组贯通（基础-方向-排版-翻译-资产-地区-性能-工具八链路组合场景）；
//! 每场景四断言（功能正确+无障碍达标+性能达标+审计留痕）；
//! 任一断言不过→场景红+立案；场景间干扰→隔离复测；环境漂移→重跑；
//! 40 场景全部含多语言无障碍断言（域本色硬门）；场景数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1020;
pub const SCENARIO_N: usize = 40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    Ok = 0,
    AssertFail = SEG_BASE + 0x01, // 0x1021
    Interference = SEG_BASE + 0x02, // 0x1022
    EnvDrift = SEG_BASE + 0x03,   // 0x1023
}

/// 八链路。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chain { Base = 0, Dir = 1, Layout = 2, Translate = 3, Asset = 4, Region = 5, Perf = 6, Tool = 7 }

impl Chain {
    pub const ALL: [Chain; 8] = [
        Chain::Base, Chain::Dir, Chain::Layout, Chain::Translate,
        Chain::Asset, Chain::Region, Chain::Perf, Chain::Tool];
}

/// 四断言（功能/无障碍/性能/留痕）——无障碍为域本色硬门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FourAsserts { pub func: bool, pub a11y: bool, pub perf: bool, pub audit: bool }

impl FourAsserts {
    pub const PASS: FourAsserts = FourAsserts { func: true, a11y: true, perf: true, audit: true };
    pub fn all_pass(self) -> bool { self.func && self.a11y && self.perf && self.audit }
    pub fn failed_names(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.func { v.push("func"); }
        if !self.a11y { v.push("a11y"); }
        if !self.perf { v.push("perf"); }
        if !self.audit { v.push("audit"); }
        v
    }
}

/// 联测场景。
pub struct Scenario {
    pub id: u16,
    pub chain: Chain,
    pub asserts: FourAsserts,
    pub isolated_retest: bool, // 干扰后隔离复测
}

pub struct JointBook { pub scenes: Vec<Scenario>, pub filed: Vec<u16>, pub reruns: u32 }

impl JointBook {
    pub fn new() -> Self { JointBook { scenes: Vec::new(), filed: Vec::new(), reruns: 0 } }
    pub fn add(&mut self, id: u16, chain: Chain, a: FourAsserts) {
        self.scenes.push(Scenario { id, chain, asserts: a, isolated_retest: false });
    }
    /// 执行裁决：任一断言不过→红+立案；返回红场景 id。
    pub fn run(&mut self) -> Vec<u16> {
        let mut red = Vec::new();
        for s in &self.scenes {
            if !s.asserts.all_pass() {
                red.push(s.id);
                if !self.filed.contains(&s.id) { self.filed.push(s.id); }
            }
        }
        red
    }
    /// 隔离复测：红场景标记隔离后按新断言复测。
    pub fn isolate_retest(&mut self, id: u16, new_asserts: FourAsserts) -> bool {
        for s in self.scenes.iter_mut() {
            if s.id == id {
                s.isolated_retest = true;
                s.asserts = new_asserts;
                self.filed.retain(|x| *x != id);
                return new_asserts.all_pass();
            }
        }
        false
    }
    /// 环境漂移重跑（幂等计数）。
    pub fn rerun_drift(&mut self) { self.reruns += 1; }
    /// 域本色硬门：全部 40 场景必须含无障碍断言位（此处以 a11y 全真核）。
    pub fn a11y_hard_gate(&self) -> bool {
        self.scenes.len() == SCENARIO_N && self.scenes.iter().all(|s| s.asserts.a11y)
    }
    /// 八链路覆盖断言：每条链至少 1 场景。
    pub fn chains_covered(&self) -> bool {
        Chain::ALL.iter().all(|c| self.scenes.iter().any(|s| s.chain == *c))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves423_checks() -> CheckSet {
    let mut set = CheckSet::new("ves423");
    let mut bk = JointBook::new();

    // C423-01 40 场景八链路构造（数据合成）。
    for i in 0..SCENARIO_N {
        let chain = Chain::ALL[i % 8];
        bk.add(1000 + i as u16, chain, FourAsserts::PASS);
    }
    chk(&mut set, "C423-01", bk.scenes.len() == SCENARIO_N, "40 场景构造");

    // C423-02 全绿执行：零红。
    let red = bk.run();
    chk(&mut set, "C423-02", red.is_empty() && bk.filed.is_empty(), "全绿零红");

    // C423-03 无障碍硬门：40 场景 a11y 全真。
    chk(&mut set, "C423-03", bk.a11y_hard_gate(), "a11y 硬门全真");

    // C423-04 八链路覆盖断言。
    chk(&mut set, "C423-04", bk.chains_covered(), "八链路覆盖");

    // C423-05 功能断言红→立案。
    bk.scenes[7].asserts.func = false;
    let red = bk.run();
    chk(&mut set, "C423-05", red == vec![1007] && bk.filed.contains(&1007), "功能红立案");

    // C423-06 失败断言名定位。
    let names = bk.scenes[7].asserts.failed_names();
    chk(&mut set, "C423-06", names == vec!["func"], "失败项定位");

    // C423-07 多断言红并列定位。
    bk.scenes[9].asserts.a11y = false;
    bk.scenes[9].asserts.perf = false;
    let names9 = bk.scenes[9].asserts.failed_names();
    chk(&mut set, "C423-07", names9 == vec!["a11y", "perf"], "多红并列");

    // C423-08 隔离复测复绿→销案。
    let ok = bk.isolate_retest(1007, FourAsserts::PASS);
    chk(&mut set, "C423-08", ok && !bk.filed.contains(&1007), "隔离复测复绿");

    // C423-09 隔离复测仍红→维持立案。
    bk.scenes[9].isolated_retest = false;
    let mut still = FourAsserts::PASS;
    still.a11y = false;
    let ok = bk.isolate_retest(1009, still);
    chk(&mut set, "C423-09", !ok && bk.filed.contains(&1009), "复测仍红立案");

    // C423-10 环境漂移重跑幂等计数。
    bk.rerun_drift();
    bk.rerun_drift();
    chk(&mut set, "C423-10", bk.reruns == 2, "重跑计数");

    // C423-11 立案清单与红集合对账。
    let red = bk.run();
    chk(&mut set, "C423-11", red == vec![1009] && bk.filed == vec![1009], "立案对账");

    // C423-12 四断言全真判据单源。
    chk(&mut set, "C423-12", FourAsserts::PASS.all_pass()
        && !FourAsserts { func: true, a11y: true, perf: false, audit: true }.all_pass(), "四断言单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves423_smoke() {
        let set = run_ves423_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves423 red: {:?}", red);
    }
}
