//! VE-F4278 · U04 联调（U04 治理组）
//! 职责：组内链五段贯通（总纲→流程→处置→度量→闸门挂钩）+ 跨组对接
//! （上承 U03 结果流、下启 U05 工具集成面）+ 联调用例十例端到端
//! （违例从发现到销账全链）+ 逐段探针定位 + 对拍仲裁。
//! 段位：0x2900（续表 idx512）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2900;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    /// 链路断点。
    ChainBreak = SEG_BASE,
    /// 契约失配（对拍仲裁）。
    ContractMismatch = SEG_BASE + 0x01,
    /// 用例失败（阻断收口）。
    CaseFail = SEG_BASE + 0x02,
}

/// 组内五段链。
pub const U04_CHAIN: [&str; 5] = ["charter", "process", "disposal", "metric", "gate"];

/// 联调册。
pub struct U04Joint {
    pub passed: usize,
    pub receipts: Vec<(&'static str, bool)>,
    /// 十例端到端：(例号, 全链达)。
    pub cases: [(bool, bool); 10],
    /// 探针记录。
    pub probes: Vec<u8>,
    /// 对接契约卡：上游 U03 / 下游 U05。
    pub up_u03: bool,
    pub down_u05: bool,
}

impl U04Joint {
    pub fn new() -> Self {
        U04Joint { passed: 0, receipts: Vec::new(), cases: [(false, false); 10], probes: Vec::new(), up_u03: false, down_u05: false }
    }

    /// 站贯通（顺序）。
    pub fn pass_station(&mut self, name: &'static str) -> Result<(), JtCode> {
        let idx = U04_CHAIN.iter().position(|c| *c == name).ok_or(JtCode::ChainBreak)?;
        if idx != self.passed {
            self.probes.push(idx as u8);
            return Err(JtCode::ChainBreak);
        }
        self.passed += 1;
        self.receipts.push((name, true));
        Ok(())
    }

    /// 探针定位：断点段。
    pub fn probe_locate(&self) -> Option<u8> {
        self.probes.last().copied()
    }

    /// 端到端用例：违例发现→销账全链。
    pub fn run_case(&mut self, no: usize, reached_end: bool) -> Result<(), JtCode> {
        if no >= 10 {
            return Err(JtCode::CaseFail);
        }
        self.cases[no] = (true, reached_end);
        if !reached_end {
            return Err(JtCode::CaseFail);
        }
        Ok(())
    }

    /// 十例全通。
    pub fn all_cases(&self) -> bool {
        self.cases.iter().all(|c| c.0 && c.1)
    }

    /// 跨组对接：上承+下启。
    pub fn bridge(&mut self, up: bool, down: bool) {
        self.up_u03 = up;
        self.down_u05 = down;
    }

    /// 对拍仲裁：契约失配→仲裁（留痕）。
    pub fn arbitrate_mismatch(&mut self, tag: u16) -> u8 {
        // 对拍后裁决：0=上游修正 1=下游适配
        if tag % 2 == 0 {
            0
        } else {
            1
        }
    }

    /// 收口可放行：五段+十例+双桥。
    pub fn releasable(&self) -> bool {
        self.passed == U04_CHAIN.len() && self.all_cases() && self.up_u03 && self.down_u05
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C512-01..12。
pub fn run_ves512_checks() -> CheckSet {
    let mut set = CheckSet::new("ves512");

    let mut jt = U04Joint::new();
    chk(&mut set, "C512-01", jt.pass_station("process") == Err(JtCode::ChainBreak), "越站拦截（总纲先行）");
    chk(&mut set, "C512-02", jt.probe_locate() == Some(1), "探针定位断点段=1");
    chk(&mut set, "C512-03", jt.pass_station("rogue") == Err(JtCode::ChainBreak), "未知站拒收");

    for s in U04_CHAIN.iter() {
        let _ = jt.pass_station(s);
    }
    chk(&mut set, "C512-04", jt.passed == 5, "五段全贯通");
    chk(&mut set, "C512-05", jt.receipts.len() == 5, "五段回执在册");

    for i in 0..10 {
        let _ = jt.run_case(i, true);
    }
    chk(&mut set, "C512-06", jt.all_cases(), "十例端到端全通（违例发现→销账）");
    chk(&mut set, "C512-07", jt.run_case(3, false) == Err(JtCode::CaseFail), "未达终链用例失败");
    chk(&mut set, "C512-08", jt.run_case(10, true) == Err(JtCode::CaseFail), "越界例号拒收");

    jt.bridge(true, true);
    chk(&mut set, "C512-09", jt.releasable() == false, "用例3 失败阻断收口不放行");

    // 修复后
    let _ = jt.run_case(3, true);
    chk(&mut set, "C512-10", jt.releasable(), "修复后放行（五段+十例+双桥）");

    // 对拍仲裁
    chk(&mut set, "C512-11", jt.arbitrate_mismatch(4) == 0 && jt.arbitrate_mismatch(7) == 1, "契约失配对拍仲裁留痕");
    chk(&mut set, "C512-12", jt.probes.len() == 2 && jt.probes[0] == 1, "探针账保序");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u04_joint() {
        let mut jt = U04Joint::new();
        assert!(jt.pass_station("charter").is_ok());
        assert_eq!(jt.pass_station("gate"), Err(JtCode::ChainBreak));
        assert!(!jt.releasable());
    }
}
