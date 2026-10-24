//! VE-F4298 · U05 联调（U05 工具集成组）
//! 职责：组内链十段贯通（总纲→CI→审查→IDE→设计→调试→看板→市场→文档→SDK）
//! + 跨组对接（上承 U04 治理闸、下启 U06 场景应用）+ 十例全链
//! （违例从 IDE 发现到市场门禁）+ 逐段探针 + 对拍仲裁。
//! 段位：0x2EE0（续表 idx532）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2EE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5Code {
    ChainBreak = SEG_BASE,
    ContractMismatch = SEG_BASE + 0x01,
    CaseFail = SEG_BASE + 0x02,
}

/// 十段链。
pub const U5_CHAIN: [&str; 10] = [
    "outline", "ci", "review", "ide", "design", "debug", "board", "market", "docs", "sdk",
];

/// U05 联调册。
pub struct U05Joint {
    pub passed: usize,
    pub receipts: Vec<(&'static str, bool)>,
    pub cases: [(bool, bool); 10],
    pub probes: Vec<u8>,
    pub up_u04: bool,
    pub down_u06: bool,
}

impl U05Joint {
    pub fn new() -> Self {
        U05Joint { passed: 0, receipts: Vec::new(), cases: [(false, false); 10], probes: Vec::new(), up_u04: false, down_u06: false }
    }

    pub fn pass_station(&mut self, name: &'static str) -> Result<(), U5Code> {
        let idx = U5_CHAIN.iter().position(|c| *c == name).ok_or(U5Code::ChainBreak)?;
        if idx != self.passed {
            self.probes.push(idx as u8);
            return Err(U5Code::ChainBreak);
        }
        self.passed += 1;
        self.receipts.push((name, true));
        Ok(())
    }

    pub fn probe_locate(&self) -> Option<u8> {
        self.probes.last().copied()
    }

    pub fn run_case(&mut self, no: usize, reached_end: bool) -> Result<(), U5Code> {
        if no >= 10 {
            return Err(U5Code::CaseFail);
        }
        self.cases[no] = (true, reached_end);
        if !reached_end {
            return Err(U5Code::CaseFail);
        }
        Ok(())
    }

    pub fn all_cases(&self) -> bool {
        self.cases.iter().all(|c| c.0 && c.1)
    }

    pub fn bridge(&mut self, up: bool, down: bool) {
        self.up_u04 = up;
        self.down_u06 = down;
    }

    pub fn releasable(&self) -> bool {
        self.passed == U5_CHAIN.len() && self.all_cases() && self.up_u04 && self.down_u06
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C532-01..12。
pub fn run_ves532_checks() -> CheckSet {
    let mut set = CheckSet::new("ves532");

    let mut jt = U05Joint::new();
    chk(&mut set, "C532-01", jt.pass_station("ci") == Err(U5Code::ChainBreak), "越站拦截（总纲先行）");
    chk(&mut set, "C532-02", jt.probe_locate() == Some(1), "探针定位断点段");
    chk(&mut set, "C532-03", jt.pass_station("rogue") == Err(U5Code::ChainBreak), "未知站拒收");

    for s in U5_CHAIN.iter() {
        let _ = jt.pass_station(s);
    }
    chk(&mut set, "C532-04", jt.passed == 10, "十段全贯通");
    chk(&mut set, "C532-05", jt.receipts.len() == 10, "十段回执");

    for i in 0..10 {
        let _ = jt.run_case(i, true);
    }
    chk(&mut set, "C532-06", jt.all_cases(), "十例全链通（IDE 发现→市场门禁）");
    chk(&mut set, "C532-07", jt.run_case(5, false) == Err(U5Code::CaseFail), "未达终链失败");
    jt.bridge(true, true);
    chk(&mut set, "C532-08", !jt.releasable(), "例5失败阻断收口");
    let _ = jt.run_case(5, true);
    chk(&mut set, "C532-09", jt.releasable(), "修复后放行（十段+十例+双桥）");

    chk(&mut set, "C532-10", jt.run_case(10, true) == Err(U5Code::CaseFail), "越界例号");
    chk(&mut set, "C532-11", U5_CHAIN.len() == 10 && U5_CHAIN[0] == "outline" && U5_CHAIN[9] == "sdk", "十段链序自洽");
    chk(&mut set, "C532-12", jt.probes.len() == 2, "探针账保序");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_joint() {
        let mut jt = U05Joint::new();
        assert!(jt.pass_station("outline").is_ok());
        assert_eq!(jt.pass_station("sdk"), Err(U5Code::ChainBreak));
        assert!(!jt.releasable());
    }
}
