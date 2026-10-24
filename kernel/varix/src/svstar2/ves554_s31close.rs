//! VE-F4320 · U06 组收口双签（U06 场景应用组）
//! 职责：U06 组收口双签（家族双签模板同域沿用）——三件套（预备报告 +
//! 字数核对 + 台账回写）+ U06 交付声明（七景全部可用 + ROI 模型上线）
//! + 字数不符复核禁签 + 红线未销拒绝收口 + 回写失败整组回滚。
//! 段位：0x3320（续表 idx554）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3320;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 字数不符（复核禁签）。
    WordCount = SEG_BASE,
    /// 红线未销（拒绝收口）。
    RedlineOpen = SEG_BASE + 0x01,
    /// 回写失败（整组回滚）。
    WriteFail = SEG_BASE + 0x02,
    /// 缺件拒签（三件套未齐）。
    Missing = SEG_BASE + 0x03,
    /// 同人双签（禁）。
    SameSigner = SEG_BASE + 0x04,
    /// 零号签视同无效。
    ZeroSign = SEG_BASE + 0x05,
}

/// 三件套。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Triple {
    pub report: bool,
    pub words_match: bool,
    pub ledger: bool,
}

/// 收口台。
#[derive(Default)]
pub struct U06Close {
    pub triple: Option<Triple>,
    /// 红线在销：(红线号, 已销)。
    pub redlines: Vec<(u32, bool)>,
    /// 双签：(签人, 时序)。
    pub signs: Vec<(u32, u32)>,
    /// 交付声明。
    pub declared: bool,
    pub scenes_live: u8, // 七景可用数
    pub roi_online: bool,
    pub rollback_done: bool,
    seq: u32,
}

impl U06Close {
    pub fn new() -> Self {
        U06Close { ..Default::default() }
    }

    /// 三件套提交（覆盖式）。
    pub fn submit(&mut self, t: Triple) {
        self.triple = Some(t);
    }

    /// 红线登记 / 销号。
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

    /// 交付声明：七景全部可用 + ROI 模型上线才可声明。
    pub fn declare(&mut self) -> Result<(), ClCode> {
        if self.scenes_live < 7 || !self.roi_online {
            return Err(ClCode::Missing);
        }
        self.declared = true;
        Ok(())
    }

    /// 签核前置：三件齐 + 字数符 + 红线全销 + 交付声明。
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

    /// 收口完成（双签齐）。
    pub fn closed(&self) -> bool {
        self.signs.len() >= 2
    }

    /// 台账回写：失败即整组回滚（回滚留痕——可恢复）。
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

pub fn run_ves554_checks() -> CheckSet {
    let mut set = CheckSet::new("ves554");
    let mut cl = U06Close::new();

    // C554-01 缺件拒签（未提交三件套）。
    chk(&mut set, "C554-01", cl.sign(1) == Err(ClCode::Missing), "缺件拒签");

    // C554-02 字数不符复核禁签。
    cl.submit(Triple { report: true, words_match: false, ledger: true });
    chk(&mut set, "C554-02", cl.sign(1) == Err(ClCode::WordCount), "字数不符禁签");

    // C554-03 红线未销拒绝。
    cl.submit(Triple { report: true, words_match: true, ledger: true });
    cl.redline(701);
    cl.scenes_live = 7;
    cl.roi_online = true;
    let _ = cl.declare();
    chk(&mut set, "C554-03", cl.sign(1) == Err(ClCode::RedlineOpen), "红线未销拒");

    // C554-04 红线销号后首签成功。
    cl.clear_redline(701);
    chk(&mut set, "C554-04", cl.sign(1).is_ok() && !cl.closed(), "销号后首签");

    // C554-05 同人双签禁。
    chk(&mut set, "C554-05", cl.sign(1) == Err(ClCode::SameSigner), "同人禁签");

    // C554-06 零号签无效。
    chk(&mut set, "C554-06", cl.sign(0) == Err(ClCode::ZeroSign), "零号签无效");

    // C554-07 二签后收口完成。
    chk(&mut set, "C554-07", cl.sign(2).is_ok() && cl.closed(), "双签收口");

    // C554-08 交付声明前置（缺 ROI 拒）。
    let mut cl2 = U06Close::new();
    cl2.scenes_live = 7;
    chk(&mut set, "C554-08", cl2.declare() == Err(ClCode::Missing), "缺 ROI 拒声明");

    // C554-09 七景 + ROI 齐后声明成功。
    cl2.roi_online = true;
    chk(&mut set, "C554-09", cl2.declare().is_ok() && cl2.declared, "齐件声明成");

    // C554-10 回写失败整组回滚（双签清空 + 留痕）。
    let rf = cl.write_ledger(false);
    chk(&mut set, "C554-10", rf == Err(ClCode::WriteFail) && cl.rollback_done && cl.signs.is_empty(), "失败整组回滚");

    // C554-11 回滚后可重走流程（双签重建收口）。
    let _ = cl.sign(1);
    let _ = cl.sign(2);
    chk(&mut set, "C554-11", cl.closed() && cl.write_ledger(true).is_ok(), "回滚后重建");

    // C554-12 签序单调（回滚重建后时序重排不倒挂）。
    let mono = cl.signs.windows(2).all(|w| w[0].1 < w[1].1);
    chk(&mut set, "C554-12", mono && cl.signs.len() == 2, "签序单调");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_sign_flow() {
        let mut cl = U06Close::new();
        cl.submit(Triple { report: true, words_match: true, ledger: true });
        cl.scenes_live = 7;
        cl.roi_online = true;
        assert!(cl.declare().is_ok());
        assert_eq!(cl.sign(0), Err(ClCode::ZeroSign));
        assert!(cl.sign(3).is_ok());
        assert_eq!(cl.sign(3), Err(ClCode::SameSigner));
        assert!(cl.sign(4).is_ok());
        assert!(cl.closed());
    }

    #[test]
    fn redline_blocks() {
        let mut cl = U06Close::new();
        cl.submit(Triple { report: true, words_match: true, ledger: true });
        cl.scenes_live = 7;
        cl.roi_online = true;
        let _ = cl.declare();
        cl.redline(9);
        assert_eq!(cl.sign(1), Err(ClCode::RedlineOpen));
        cl.clear_redline(9);
        assert!(cl.sign(1).is_ok());
    }

    #[test]
    fn write_fail_rollback() {
        let mut cl = U06Close::new();
        cl.submit(Triple { report: true, words_match: true, ledger: true });
        cl.scenes_live = 7;
        cl.roi_online = true;
        let _ = cl.declare();
        let _ = cl.sign(1);
        let _ = cl.sign(2);
        assert_eq!(cl.write_ledger(false), Err(ClCode::WriteFail));
        assert!(cl.signs.is_empty());
    }
}
