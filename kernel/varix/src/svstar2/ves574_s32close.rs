//! VE-F4340 · U07 组收口双签（U07 性能与预算组）
//! 职责：U07 组收口双签（家族双签模板同域沿用）——三件套（预备报告 +
//! 字数核对 + 台账回写）+ U07 交付声明（预算 / 基准 / 回归门 / 降级全线
//! 可用）+ 字数不符复核禁签 + 红线未销拒绝收口 + 回写失败整组回滚。
//! 段位：0x3BC0（续表 idx574）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3BC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 字数不符（复核禁签）。
    WordCount = SEG_BASE,
    /// 红线未销（拒绝收口）。
    RedlineOpen = SEG_BASE + 0x01,
    /// 回写失败（整组回滚）。
    WriteFail = SEG_BASE + 0x02,
    /// 缺件拒签。
    Missing = SEG_BASE + 0x03,
    /// 同人双签（禁）。
    SameSigner = SEG_BASE + 0x04,
    /// 零号签无效。
    ZeroSign = SEG_BASE + 0x05,
    /// 全线可用声明缺项。
    LineMissing = SEG_BASE + 0x06,
}

/// 四条线可用位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lines {
    pub budget: bool,
    pub bench: bool,
    pub gate: bool,
    pub degrade: bool,
}

impl Lines {
    pub fn all(&self) -> bool {
        self.budget && self.bench && self.gate && self.degrade
    }
}

/// 三件套。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Triple {
    pub report: bool,
    pub words_match: bool,
    pub ledger: bool,
}

/// U07 收口台。
#[derive(Default)]
pub struct U07Close {
    pub triple: Option<Triple>,
    pub lines: Option<Lines>,
    pub redlines: Vec<(u32, bool)>,
    pub signs: Vec<(u32, u32)>,
    pub declared: bool,
    pub rollback_done: bool,
    seq: u32,
}

impl U07Close {
    pub fn new() -> Self {
        U07Close { ..Default::default() }
    }

    pub fn submit(&mut self, t: Triple) {
        self.triple = Some(t);
    }

    pub fn redline(&mut self, id: u32) {
        if !self.redlines.iter().any(|r| r.0 == id) {
            self.redlines.push((id, false));
        }
    }

    pub fn clear_redline(&mut self, id: u32) {
        if let Some(r) = self.redlines.iter_mut().find(|r| r.0 == id) {
            r.1 = true;
        }
    }

    /// 交付声明：四条线全可用才可声明（缺线拒绝并指明缺项）。
    pub fn declare(&mut self, l: Lines) -> Result<(), ClCode> {
        if !l.all() {
            return Err(ClCode::LineMissing);
        }
        self.lines = Some(l);
        self.declared = true;
        Ok(())
    }

    fn sign_ready(&self) -> Result<(), ClCode> {
        let t = self.triple.as_ref().ok_or(ClCode::Missing)?;
        if !t.report || !t.ledger {
            return Err(ClCode::Missing);
        }
        if !t.words_match {
            return Err(ClCode::WordCount);
        }
        if self.redlines.iter().any(|r| !r.1) {
            return Err(ClCode::RedlineOpen);
        }
        if !self.declared {
            return Err(ClCode::Missing);
        }
        Ok(())
    }

    /// 双签：不同人两签；零号无效；同人禁。
    pub fn sign(&mut self, signer: u32) -> Result<(), ClCode> {
        self.sign_ready()?;
        if signer == 0 {
            return Err(ClCode::ZeroSign);
        }
        if self.signs.iter().any(|s| s.0 == signer) {
            return Err(ClCode::SameSigner);
        }
        self.seq += 1;
        self.signs.push((signer, self.seq));
        Ok(())
    }

    pub fn closed(&self) -> bool {
        self.signs.len() >= 2
    }

    /// 台账回写：失败整组回滚。
    pub fn write_ledger(&mut self, ok: bool) -> Result<(), ClCode> {
        if !ok {
            self.rollback_done = true;
            self.signs.clear();
            return Err(ClCode::WriteFail);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves574_checks() -> CheckSet {
    let mut set = CheckSet::new("ves574");
    let mut cl = U07Close::new();

    // C574-01 缺件拒签。
    chk(&mut set, "C574-01", cl.sign(1) == Err(ClCode::Missing), "缺件拒签");

    // C574-02 字数不符复核禁签。
    cl.submit(Triple { report: true, words_match: false, ledger: true });
    chk(&mut set, "C574-02", cl.sign(1) == Err(ClCode::WordCount), "字数不符禁签");

    // C574-03 交付声明缺线拒绝（降级线缺）。
    chk(&mut set, "C574-03", cl.declare(Lines { budget: true, bench: true, gate: true, degrade: false })
        == Err(ClCode::LineMissing), "缺线拒声明");

    // C574-04 全线可用声明成功。
    chk(&mut set, "C574-04", cl.declare(Lines { budget: true, bench: true, gate: true, degrade: true }).is_ok()
        && cl.declared, "全线声明成");

    // C574-05 红线未销拒绝。
    cl.redline(801);
    cl.triple = Some(Triple { report: true, words_match: true, ledger: true });
    chk(&mut set, "C574-05", cl.sign(1) == Err(ClCode::RedlineOpen), "红线未销拒");

    // C574-06 销号后首签 + 零号无效 + 同人禁。
    cl.clear_redline(801);
    chk(&mut set, "C574-06", cl.sign(0) == Err(ClCode::ZeroSign) && cl.sign(1).is_ok(), "销号首签+零号拒");
    chk(&mut set, "C574-06b", cl.sign(1) == Err(ClCode::SameSigner), "同人禁");

    // C574-07 二签收口。
    chk(&mut set, "C574-07", cl.sign(2).is_ok() && cl.closed(), "双签收口");

    // C574-08 回写失败整组回滚。
    chk(&mut set, "C574-08", cl.write_ledger(false) == Err(ClCode::WriteFail)
        && cl.rollback_done && cl.signs.is_empty(), "失败整组回滚");

    // C574-09 回滚后重建收口。
    let _ = cl.sign(1);
    let _ = cl.sign(2);
    chk(&mut set, "C574-09", cl.closed() && cl.write_ledger(true).is_ok(), "回滚重建");

    // C574-10 签序单调。
    chk(&mut set, "C574-10", cl.signs.windows(2).all(|w| w[0].1 < w[1].1) && cl.signs.len() == 2, "签序单调");

    // C574-11 未声明签核拒绝（缺声明路径）。
    let mut cl2 = U07Close::new();
    cl2.submit(Triple { report: true, words_match: true, ledger: true });
    cl2.redline(1);
    cl2.clear_redline(1);
    chk(&mut set, "C574-11", cl2.sign(1) == Err(ClCode::Missing), "未声明拒签");

    // C574-12 声明幂等（重声明覆盖不膨胀）。
    let _ = cl.declare(Lines { budget: true, bench: true, gate: true, degrade: true });
    chk(&mut set, "C574-12", cl.declared && cl.lines.map(|l| l.all()) == Some(true), "声明幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_sign_full_flow() {
        let mut cl = U07Close::new();
        cl.submit(Triple { report: true, words_match: true, ledger: true });
        assert_eq!(cl.declare(Lines { budget: true, bench: false, gate: true, degrade: true }), Err(ClCode::LineMissing));
        assert!(cl.declare(Lines { budget: true, bench: true, gate: true, degrade: true }).is_ok());
        assert_eq!(cl.sign(0), Err(ClCode::ZeroSign));
        assert!(cl.sign(3).is_ok());
        assert!(cl.sign(3).is_err());
        assert!(cl.sign(4).is_ok());
        assert!(cl.closed());
    }

    #[test]
    fn redline_blocks_and_rollback() {
        let mut cl = U07Close::new();
        cl.submit(Triple { report: true, words_match: true, ledger: true });
        let _ = cl.declare(Lines { budget: true, bench: true, gate: true, degrade: true });
        cl.redline(9);
        assert_eq!(cl.sign(1), Err(ClCode::RedlineOpen));
        cl.clear_redline(9);
        let _ = cl.sign(1);
        let _ = cl.sign(2);
        assert_eq!(cl.write_ledger(false), Err(ClCode::WriteFail));
        assert!(cl.signs.is_empty());
    }
}
