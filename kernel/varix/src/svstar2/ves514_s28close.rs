//! VE-F4280 · U04 组收口双签（U04 治理组）
//! 职责：家族双签模板第十用（同域沿用）+ 双签三件（台账深化完成/会话号
//! 回写/字数核对）+ 收口口径三证（轻门禁绿/账本无未销红线/证据归档齐）
//! + 字数不符禁签 + 未销红线拒收口 + 回写失败整组回滚。
//! 段位：0x2960（续表 idx514）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2960;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 字数不符（复核禁签）。
    WordMismatch = SEG_BASE,
    /// 账本有未销红线（拒绝收口）。
    RedlineOpen = SEG_BASE + 0x01,
    /// 回写失败（整组回滚）。
    WritebackFail = SEG_BASE + 0x02,
    /// 双签同人。
    SameSigner = SEG_BASE + 0x03,
    /// 证据缺。
    EvidenceLost = SEG_BASE + 0x04,
}

/// 双签三件。
#[derive(Clone, Copy, Debug)]
pub struct SignTriple {
    /// 台账行深化完成。
    pub ledger_done: bool,
    /// 会话号回写。
    pub session_back: Option<u32>,
    /// 字数核对（申报 vs 实测）。
    pub words_claim: u32,
    pub words_real: u32,
}

impl SignTriple {
    pub fn words_ok(&self) -> bool {
        self.words_claim == self.words_real
    }
    pub fn triple_ok(&self) -> bool {
        self.ledger_done && self.session_back.is_some() && self.words_ok()
    }
}

/// 收口器。
pub struct U04Closer {
    pub triple: SignTriple,
    /// 未销红线账。
    pub open_redlines: Vec<u16>,
    /// 证据归档：(证号, 已归档)。
    pub evidence: Vec<(u16, bool)>,
    /// 门禁绿灯。
    pub light_green: bool,
    pub closed: bool,
    pub rolled_back: bool,
}

impl U04Closer {
    pub fn new(t: SignTriple) -> Self {
        U04Closer { triple: t, open_redlines: Vec::new(), evidence: Vec::new(), light_green: false, closed: false, rolled_back: false }
    }

    /// 证据归档（幂等）。
    pub fn archive(&mut self, no: u16) {
        if !self.evidence.iter().any(|(n, _)| *n == no) {
            self.evidence.push((no, true));
        }
    }

    /// 三证齐备。
    pub fn evidence_ok(&self) -> bool {
        !self.evidence.is_empty() && self.evidence.iter().all(|(_, a)| *a)
    }

    /// 尝试收口：三件齐+轻门禁绿+无未销红线+三证齐。
    pub fn try_close(&mut self, a: u8, b: u8) -> Result<(), ClCode> {
        if a == b || a == 0 || b == 0 {
            return Err(ClCode::SameSigner);
        }
        if !self.triple.words_ok() {
            return Err(ClCode::WordMismatch); // 字数不符复核禁签
        }
        if !self.open_redlines.is_empty() {
            return Err(ClCode::RedlineOpen);
        }
        if !self.light_green || !self.evidence_ok() {
            return Err(ClCode::EvidenceLost);
        }
        if !self.triple.triple_ok() {
            return Err(ClCode::WritebackFail);
        }
        self.closed = true;
        Ok(())
    }

    /// 回写失败→整组回滚（不落半截）。
    pub fn rollback(&mut self) {
        self.closed = false;
        self.rolled_back = true;
        self.evidence.clear();
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C514-01..12。
pub fn run_ves514_checks() -> CheckSet {
    let mut set = CheckSet::new("ves514");

    let mut cl = U04Closer::new(SignTriple { ledger_done: true, session_back: Some(42), words_claim: 300, words_real: 295 });
    chk(&mut set, "C514-01", cl.try_close(1, 2) == Err(ClCode::WordMismatch), "字数不符复核禁签（300≠295）");

    cl.triple.words_real = 300;
    chk(&mut set, "C514-02", cl.try_close(1, 2) == Err(ClCode::RedlineOpen), "账本有未销红线拒绝收口");
    cl.open_redlines.push(9);
    chk(&mut set, "C514-03", cl.try_close(1, 2) == Err(ClCode::RedlineOpen), "红线在册即拒");
    cl.open_redlines.clear();

    chk(&mut set, "C514-04", cl.try_close(1, 2) == Err(ClCode::EvidenceLost), "三证缺拒绝");
    cl.archive(1);
    cl.archive(1);
    cl.archive(2);
    chk(&mut set, "C514-05", cl.evidence.len() == 2, "证据归档幂等");
    cl.light_green = true;
    chk(&mut set, "C514-06", cl.try_close(1, 1) == Err(ClCode::SameSigner), "双签同人作废");
    chk(&mut set, "C514-07", cl.try_close(0, 2) == Err(ClCode::SameSigner), "零号签无效");
    chk(&mut set, "C514-08", cl.try_close(1, 2).is_ok(), "三件齐+三证齐+绿灯+无红线→收口");
    chk(&mut set, "C514-09", cl.closed, "收口在册");

    // 回写失败整组回滚
    cl.triple.ledger_done = false;
    cl.rollback();
    chk(&mut set, "C514-10", cl.rolled_back && !cl.closed && cl.evidence.is_empty(), "整组回滚不落半截");

    // 三件语义
    let t = SignTriple { ledger_done: true, session_back: Some(1), words_claim: 10, words_real: 10 };
    chk(&mut set, "C514-11", t.triple_ok(), "三件套语义复述");
    let t2 = SignTriple { ledger_done: true, session_back: None, words_claim: 1, words_real: 1 };
    chk(&mut set, "C514-12", !t2.triple_ok() && t2.words_ok(), "缺会话号回写→三件不齐");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u04_close() {
        let mut cl = U04Closer::new(SignTriple { ledger_done: true, session_back: Some(1), words_claim: 5, words_real: 5 });
        assert_eq!(cl.try_close(1, 2), Err(ClCode::EvidenceLost));
        cl.archive(3);
        cl.light_green = true;
        assert!(cl.try_close(1, 2).is_ok());
    }
}
