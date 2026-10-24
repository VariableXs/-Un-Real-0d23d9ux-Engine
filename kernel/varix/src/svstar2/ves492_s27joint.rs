//! VE-F4258 · U03 联调（U03 一致性验证平台）
//! 职责：20 条目贯通演练（静态→动态→视觉→交互→性能→无障碍六验证全链跑通）
//! + 故障注入三演练（基准投毒/闸门死锁/回放失真）+ 证据挂 F4248 报告平台。
//! 段位：0x1E60（续表 idx492）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1E60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    /// 链断。
    ChainBreak = SEG_BASE,
    /// 演练失败（预案未验证）。
    DrillFail = SEG_BASE + 0x01,
    /// 证据缺。
    EvidenceLost = SEG_BASE + 0x02,
}

/// 六验证全链站序。
pub const CHAIN: [&str; 6] = ["static", "dynamic", "visual", "interact", "perf", "a11y"];

/// 故障注入类别。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    BaselinePoison = 0,
    GateDeadlock = 1,
    ReplayDrift = 2,
}
pub const FAULT_N: usize = 3;

/// 联调册。
pub struct JointBook {
    /// 贯通进度（已通站数）。
    pub passed: usize,
    /// 各站回执。
    pub receipts: Vec<(&'static str, bool)>,
    /// 演练结果：(故障, 预案生效)。
    pub drills: Vec<(Fault, bool)>,
    /// 证据链：(链站, 证据号)。
    pub evidence: Vec<(&'static str, u32)>,
    pub reruns: u32,
}

impl JointBook {
    pub fn new() -> Self {
        JointBook { passed: 0, receipts: Vec::new(), drills: Vec::new(), evidence: Vec::new(), reruns: 0 }
    }

    /// 站贯通：顺序推进（静态先行，a11y 永不跳过——抽演在末站强制）。
    pub fn pass_station(&mut self, name: &'static str) -> Result<(), JtCode> {
        let idx = CHAIN.iter().position(|c| *c == name).ok_or(JtCode::ChainBreak)?;
        if idx != self.passed {
            return Err(JtCode::ChainBreak);
        }
        self.passed += 1;
        self.receipts.push((name, true));
        Ok(())
    }

    /// 无障碍抽演：末站强制核验。
    pub fn a11y_rehearsed(&self) -> bool {
        self.passed == CHAIN.len() && self.receipts.iter().any(|(n, _)| *n == "a11y")
    }

    /// 故障演练登记（预案生效才算过）。
    pub fn drill(&mut self, f: Fault, plan_ok: bool) {
        if !self.drills.iter().any(|(a, _)| *a == f) {
            self.drills.push((f, plan_ok));
        }
    }

    /// 三演练齐备且全过。
    pub fn drills_ok(&self) -> bool {
        (0..FAULT_N).all(|f| self.drills.iter().any(|(a, ok)| *a as usize == f && *ok))
    }

    /// 证据挂报告平台（幂等）。
    pub fn attach(&mut self, station: &'static str, ev: u32) {
        if !self.evidence.iter().any(|(s, e)| *s == station && *e == ev) {
            self.evidence.push((station, ev));
        }
    }

    /// 证据缺站补跑。
    pub fn missing_evidence(&self) -> Vec<&'static str> {
        let passed: Vec<&'static str> = CHAIN.iter().take(self.passed).copied().collect();
        passed.into_iter().filter(|s| !self.evidence.iter().any(|(es, _)| es == s)).collect()
    }

    /// 重跑登记。
    pub fn rerun(&mut self) {
        self.reruns += 1;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C492-01..12。
pub fn run_ves492_checks() -> CheckSet {
    let mut set = CheckSet::new("ves492");

    let mut jb = JointBook::new();
    chk(&mut set, "C492-01", jb.pass_station("dynamic") == Err(JtCode::ChainBreak), "越站推进拦截（静态先行）");
    chk(&mut set, "C492-02", jb.pass_station("rogue") == Err(JtCode::ChainBreak), "未知站拒收");

    for s in CHAIN.iter() {
        let _ = jb.pass_station(s);
    }
    chk(&mut set, "C492-03", jb.passed == 6, "六验证全链贯通");
    chk(&mut set, "C492-04", jb.a11y_rehearsed(), "无障碍抽演在册（域本色）");

    // 三演练
    jb.drill(Fault::BaselinePoison, true);
    jb.drill(Fault::GateDeadlock, false);
    chk(&mut set, "C492-05", jb.drills_ok() == false, "演练未全过=联调未收");
    jb.drill(Fault::GateDeadlock, true);
    jb.drill(Fault::ReplayDrift, true);
    chk(&mut set, "C492-06", jb.drills.len() == 3 && jb.drills_ok(), "三演练齐备全过（预案验证）");
    jb.drill(Fault::GateDeadlock, false);
    chk(&mut set, "C492-07", jb.drills.iter().filter(|(f, _)| *f == Fault::GateDeadlock).count() == 1, "演练幂等登记");

    // 证据链
    jb.attach("static", 1);
    jb.attach("a11y", 6);
    jb.attach("static", 1);
    chk(&mut set, "C492-08", jb.evidence.len() == 2, "证据挂证幂等");
    let miss = jb.missing_evidence();
    chk(&mut set, "C492-09", miss.len() == 4 && miss.contains(&"visual"), "缺证四站显性列出");
    jb.rerun();
    chk(&mut set, "C492-10", jb.reruns == 1, "缺证重跑登记");

    // 补齐证据
    for s in ["dynamic", "visual", "interact", "perf"] {
        jb.attach(s, 99);
    }
    chk(&mut set, "C492-11", jb.missing_evidence().is_empty(), "补齐后零缺证");
    chk(&mut set, "C492-12", jb.receipts.len() == 6 && jb.evidence.len() == 6, "回执与证据一一对应");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joint_chain() {
        let mut jb = JointBook::new();
        assert_eq!(jb.pass_station("a11y"), Err(JtCode::ChainBreak));
        for s in CHAIN {
            assert!(jb.pass_station(s).is_ok());
        }
        assert!(jb.a11y_rehearsed());
        assert!(jb.missing_evidence().len() == 6);
    }
}
