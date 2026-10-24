//! VE-F4359 · U08 联调贯通（U08 测试与资产组）
//! 职责：U08 十八段联调贯通——十八段链逐段点验（段段有通过记录，
//! 跳段即拦）+ 十例 U 域全链用例（用例逐段打点，断链即拦）+ 跨组
//! 对拍（联调结论与协作组结论双侧一致）+ 无障碍复验（读屏 / 高
//! 对比 / 焦点序三通道复验，域色核心）+ 联调通过才可进收口。
//! 段位：0x4660（续表 idx593）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4660;
/// 十八段链总段数。
pub const CHAIN_LEN: usize = 18;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    /// 跳段（链上断点）。
    SkippedSeg = SEG_BASE,
    /// 用例断链（全链未贯通）。
    CaseBroken = SEG_BASE + 0x01,
    /// 跨组对拍失配。
    CrossMismatch = SEG_BASE + 0x02,
    /// 无障碍复验未过。
    A11yFail = SEG_BASE + 0x03,
}

/// 十八段链：段位 0..=17。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainSeg {
    A(u8),
}

impl ChainSeg {
    pub fn idx(self) -> u8 {
        match self {
            ChainSeg::A(i) => i,
        }
    }
}

/// 联调台。
#[derive(Default)]
pub struct Joint {
    /// 已通过段位（有通过记录）。
    pub passed: Vec<u8>,
    /// 十例用例：每例打点记录的段位集。
    pub cases: Vec<Vec<u8>>,
    /// 跨组对拍：本侧结论 -> 协作侧结论（组号, 一致）。
    pub cross: Vec<(u32, bool)>,
    /// 无障碍三通道复验：读屏 / 高对比 / 焦点序。
    pub a11y: (bool, bool, bool),
    pub audit: u32,
}

impl Joint {
    pub fn new() -> Self {
        Default::default()
    }

    /// 段通过登记：必须按序贯通——登 n 前要求 0..n 全部已过（跳段即拦）。
    pub fn pass_seg(&mut self, n: u8) -> Result<(), JtCode> {
        if (n as usize) >= CHAIN_LEN {
            return Err(JtCode::SkippedSeg);
        }
        for i in 0..n {
            if !self.passed.contains(&i) {
                return Err(JtCode::SkippedSeg);
            }
        }
        if !self.passed.contains(&n) {
            self.passed.push(n);
            self.audit += 1;
        }
        Ok(())
    }

    /// 十八段贯通断言。
    pub fn chain_done(&self) -> bool {
        self.passed.len() == CHAIN_LEN
    }

    /// 用例打点：要求打点段已通过且无漏点（全链覆盖）。
    pub fn trace_case(&mut self, marks: Vec<u8>) -> Result<(), JtCode> {
        let mut sorted = marks.clone();
        sorted.sort();
        sorted.dedup();
        for (i, m) in sorted.iter().enumerate() {
            if *m != i as u8 {
                return Err(JtCode::CaseBroken);
            }
            if !self.passed.contains(m) {
                return Err(JtCode::CaseBroken);
            }
        }
        if sorted.len() < CHAIN_LEN {
            return Err(JtCode::CaseBroken);
        }
        self.cases.push(sorted);
        Ok(())
    }

    /// 十例全链断言。
    pub fn cases_done(&self) -> bool {
        self.cases.len() >= 10
    }

    /// 跨组对拍登记。
    pub fn cross_reg(&mut self, group: u32, agree: bool) {
        if !self.cross.iter().any(|(g, _)| *g == group) {
            self.cross.push((group, agree));
        }
    }

    /// 对拍全一致断言（任何一组不一致即失配）。
    pub fn cross_ok(&self) -> bool {
        !self.cross.is_empty() && self.cross.iter().all(|(_, a)| *a)
    }

    /// 无障碍三通道复验登记。
    pub fn a11y_reg(&mut self, screen_reader: bool, high_contrast: bool, focus_order: bool) {
        self.a11y = (screen_reader, high_contrast, focus_order);
    }

    /// 无障碍复验通过断言（域色核心：三通道缺一即拒）。
    pub fn a11y_ok(&self) -> bool {
        self.a11y == (true, true, true)
    }

    /// 联调放行：链贯通 + 十例全链 + 对拍一致 + 无障碍三通道。
    pub fn gate(&self) -> Result<(), JtCode> {
        if !self.chain_done() {
            return Err(JtCode::SkippedSeg);
        }
        if !self.cases_done() {
            return Err(JtCode::CaseBroken);
        }
        if !self.cross_ok() {
            return Err(JtCode::CrossMismatch);
        }
        if !self.a11y_ok() {
            return Err(JtCode::A11yFail);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves593_checks() -> CheckSet {
    let mut set = CheckSet::new("ves593");
    let mut jt = Joint::new();

    // C593-01 起段可直接通过。
    chk(&mut set, "C593-01", jt.pass_seg(0).is_ok(), "起段通过");

    // C593-02 跳段拦截（未过段 1 直接登段 2）。
    chk(&mut set, "C593-02", jt.pass_seg(2) == Err(JtCode::SkippedSeg), "跳段拦截");

    // C593-03 顺序贯通到十八段。
    let mut ok = true;
    for i in 1..CHAIN_LEN as u8 {
        ok &= jt.pass_seg(i).is_ok();
    }
    chk(&mut set, "C593-03", ok && jt.chain_done(), "十八段贯通");

    // C593-04 越界段拒。
    chk(&mut set, "C593-04", jt.pass_seg(18).is_err(), "越界段拒");

    // C593-05 全链打点用例通过。
    let full: Vec<u8> = (0..CHAIN_LEN as u8).collect();
    chk(&mut set, "C593-05", jt.trace_case(full.clone()).is_ok(), "全链用例过");

    // C593-06 漏点用例拦（少一段）。
    let short: Vec<u8> = (0..CHAIN_LEN as u8 - 1).collect();
    chk(&mut set, "C593-06", jt.trace_case(short).is_err(), "漏点用例拦");

    // C593-07 乱序打点排序后仍过。
    let mut shuffled = full.clone();
    shuffled.reverse();
    chk(&mut set, "C593-07", jt.trace_case(shuffled).is_ok(), "乱序排序过");

    // C593-08 未通过段打点拦。
    chk(&mut set, "C593-08", jt.trace_case(alloc::vec![0, 1, 99]).is_err(), "未过段点拦");

    // C593-09 跨组对拍：一致组登记。
    jt.cross_reg(7, true);
    chk(&mut set, "C593-09", !jt.cross_ok(), "仅一组不判全一致");

    // C593-10 二组失配即全局失配。
    jt.cross_reg(9, false);
    chk(&mut set, "C593-10", !jt.cross_ok(), "失配即拦");

    // C593-11 失配改判后一致。
    jt.cross_reg(9, true);
    chk(&mut set, "C593-11", jt.cross_ok(), "改判一致");

    // C593-12 无障碍缺通道拦 + 三通道齐放行。
    jt.a11y_reg(true, true, false);
    chk(&mut set, "C593-12", !jt.a11y_ok(), "缺焦点序拦");
    jt.a11y_reg(true, true, true);
    chk(&mut set, "C593-12b", jt.gate().is_ok() && jt.a11y_ok(), "三通道齐放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_gate() {
        let mut jt = Joint::new();
        assert_eq!(jt.pass_seg(3), Err(JtCode::SkippedSeg));
        assert!(jt.pass_seg(0).is_ok());
        assert!(jt.pass_seg(1).is_ok());
    }

    #[test]
    fn a11y_gate() {
        let mut jt = Joint::new();
        jt.a11y_reg(true, false, true);
        assert!(!jt.a11y_ok());
        jt.a11y_reg(true, true, true);
        assert!(jt.a11y_ok());
    }
}
