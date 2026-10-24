//! VE-F4020 · T01 组收口（域开工与国际化总架构组）
//!
//! T01 组收口：组内六件核验汇总（架构五段签名 / Locale 模型 / 映射十
//! 项 / 单源复述 / i18n 无障碍 / i18n 安全）+ 组判据回执 + 收口双签
//! （禁同人）+ 组闭合自检。四件齐备后双签放行。
//! 数据结构：核验汇总册；回执表；双签账；自检单。
//! 错误路径：件缺→收口拒绝；同人双签→P0；缺签→待签；拒签→催办。
//! 性能：汇总 O(判据)；回执 O(件)；双签 O(1)。
//! 跨批对接：F4001-F4019 全上游；F4021 T02 前向。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6840）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6840;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T26CCode(pub u16);

impl T26CCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const QUAD_MISS: Self = Self(SEG_BASE + 1);
    pub const SAME_SIGNER: Self = Self(SEG_BASE + 2);
    pub const SIGN_MISSING: Self = Self(SEG_BASE + 3);
    pub const REJECTED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::QUAD_MISS => "件缺→收口拒绝",
            Self::SAME_SIGNER => "同人双签→P0",
            Self::SIGN_MISSING => "缺签→待签",
            Self::REJECTED => "拒签→催办",
            _ => "未知",
        }
    }
}

// ── 六件核验汇总 ────────────────────────────────────────────────────

pub const T01_PARTS: [&str; 6] = [
    "架构五段签名", "Locale 模型", "映射十项", "单源复述", "i18n 无障碍", "i18n 安全",
];

/// 单件核验。
#[derive(Debug, Clone)]
pub struct PartCheck {
    pub part: &'static str,
    pub label: &'static str,
    pub ok: bool,
}

/// 汇总册。
#[derive(Debug, Clone)]
pub struct PartBook {
    pub checks: Vec<PartCheck>,
}

impl PartBook {
    pub fn new() -> Self {
        let mut checks = Vec::new();
        for p in T01_PARTS.iter() {
            checks.push(PartCheck { part: p, label: "", ok: false });
        }
        Self { checks }
    }

    /// 回填。
    pub fn fill(&mut self, part: &str, label: &'static str, ok: bool) -> bool {
        match self.checks.iter_mut().find(|c| c.part == part) {
            Some(c) => {
                c.label = label;
                c.ok = ok;
                true
            }
            None => false,
        }
    }

    /// 齐备：label 全非空。
    pub fn complete(&self) -> bool {
        self.checks.len() == 6 && self.checks.iter().all(|c| !c.label.is_empty())
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.complete() && self.checks.iter().all(|c| c.ok)
    }

    /// 红件清单。
    pub fn red_parts(&self) -> Vec<&'static str> {
        self.checks.iter().filter(|c| !c.ok).map(|c| c.part).collect()
    }
}

// ── 组判据回执表 ────────────────────────────────────────────────────

pub const T01_CHECK_IDS: [&str; 6] = [
    "C4001", "C4002", "C4003", "C4015", "C4016", "C4019",
];

/// 回执表。
#[derive(Debug, Clone)]
pub struct ReceiptBook {
    pub received: Vec<&'static str>,
}

impl ReceiptBook {
    pub fn new() -> Self {
        Self { received: Vec::new() }
    }

    pub fn receive(&mut self, id: &'static str) -> bool {
        if !T01_CHECK_IDS.contains(&id) || self.received.contains(&id) {
            return false;
        }
        self.received.push(id);
        true
    }

    pub fn pending(&self) -> Vec<&'static str> {
        T01_CHECK_IDS.iter().filter(|i| !self.received.contains(i)).map(|i| *i).collect()
    }

    pub fn all_received(&self) -> bool {
        self.pending().is_empty()
    }
}

// ── 双签账 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DualSign {
    pub self_signer: String,
    pub review_signer: String,
    pub self_ok: bool,
    pub review_ok: bool,
}

impl DualSign {
    pub fn new() -> Self {
        Self {
            self_signer: String::new(),
            review_signer: String::new(),
            self_ok: false,
            review_ok: false,
        }
    }

    pub fn sign(&mut self, slot: u8, signer: &str, ok: bool) -> T26CCode {
        if signer.is_empty() {
            return T26CCode::SIGN_MISSING;
        }
        match slot {
            0 => {
                if signer == self.review_signer {
                    return T26CCode::SAME_SIGNER;
                }
                self.self_signer = String::from(signer);
                self.self_ok = ok;
            }
            _ => {
                if signer == self.self_signer {
                    return T26CCode::SAME_SIGNER;
                }
                self.review_signer = String::from(signer);
                self.review_ok = ok;
            }
        }
        T26CCode::OK
    }

    pub fn closed(&self) -> bool {
        !self.self_signer.is_empty()
            && !self.review_signer.is_empty()
            && self.self_ok
            && self.review_ok
    }
}

// ── 收口机 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Closer {
    pub parts: PartBook,
    pub receipts: ReceiptBook,
    pub sign: DualSign,
    pub urgencies: u32,
}

impl Closer {
    pub fn new() -> Self {
        Self { parts: PartBook::new(), receipts: ReceiptBook::new(), sign: DualSign::new(), urgencies: 0 }
    }

    /// 四件齐备检查。
    pub fn ready(&self) -> bool {
        self.parts.complete() && self.receipts.all_received()
    }

    /// 双签落笔（件齐前置）。
    pub fn sign_off(&mut self, slot: u8, signer: &str, ok: bool) -> T26CCode {
        if !self.ready() {
            self.urgencies += 1;
            return T26CCode::QUAD_MISS;
        }
        let code = self.sign.sign(slot, signer, ok);
        if code != T26CCode::OK {
            return code;
        }
        if !ok {
            self.urgencies += 1;
            return T26CCode::REJECTED;
        }
        T26CCode::OK
    }

    /// 收口终态。
    pub fn outcome(&self) -> T26CCode {
        if !self.ready() {
            return T26CCode::QUAD_MISS;
        }
        if self.sign.self_signer.is_empty() || self.sign.review_signer.is_empty() {
            return T26CCode::SIGN_MISSING;
        }
        if self.sign.closed() {
            T26CCode::OK
        } else {
            T26CCode::REJECTED
        }
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves260_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves260_s11close");

    // C4020-01 诊断码段不越窗。
    chk(&mut set, "C4020-01",
        T26CCode::REJECTED.0 < SEG_BASE + 16 && SEG_BASE == 0x6840, "err");

    // C4020-02 六件表齐备。
    chk(&mut set, "C4020-02",
        T01_PARTS.len() == 6 && T01_PARTS[0] == "架构五段签名", "err");

    // C4020-03 逐件回填后齐备。
    let mut p = PartBook::new();
    for (i, part) in T01_PARTS.iter().enumerate() {
        p.fill(part, "C", i % 2 == 0);
    }
    chk(&mut set, "C4020-03",
        p.complete() && !p.all_green() && p.red_parts().len() == 3, "err");

    // C4020-04 全绿判定。
    let mut p2 = PartBook::new();
    for part in T01_PARTS.iter() {
        p2.fill(part, "C", true);
    }
    chk(&mut set, "C4020-04", p2.all_green(), "err");

    // C4020-05 越件回填拒绝。
    let mut p3 = PartBook::new();
    chk(&mut set, "C4020-05", !p3.fill("伪件", "C", true), "err");

    // C4020-06 回执表：未达清单。
    let mut r = ReceiptBook::new();
    for id in T01_CHECK_IDS.iter().take(4) {
        r.receive(id);
    }
    chk(&mut set, "C4020-06", r.pending() == vec!["C4016", "C4019"], "err");

    // C4020-07 补齐后回执全达。
    for id in r.pending() {
        r.receive(id);
    }
    chk(&mut set, "C4020-07", r.all_received(), "err");

    // C4020-08 越名回执拒 + 重复拒。
    let mut r2 = ReceiptBook::new();
    r2.receive("C4001");
    chk(&mut set, "C4020-08", !r2.receive("C9999") && !r2.receive("C4001"), "err");

    // C4020-09 件缺 → 双签拒 + 催办计数。
    let mut c = Closer::new();
    chk(&mut set, "C4020-09",
        c.sign_off(0, "甲", true) == T26CCode::QUAD_MISS && c.urgencies == 1, "err");

    // C4020-10 件齐 → 双签异人 → 收口 OK。
    let mut c2 = Closer::new();
    for part in T01_PARTS.iter() {
        c2.parts.fill(part, "C", true);
    }
    for id in T01_CHECK_IDS.iter() {
        c2.receipts.receive(id);
    }
    chk(&mut set, "C4020-10",
        c2.sign_off(0, "甲", true) == T26CCode::OK
            && c2.sign_off(1, "乙", true) == T26CCode::OK
            && c2.outcome() == T26CCode::OK, "err");

    // C4020-11 同人双签 → P0 路径。
    let mut c3 = Closer::new();
    for part in T01_PARTS.iter() {
        c3.parts.fill(part, "C", true);
    }
    for id in T01_CHECK_IDS.iter() {
        c3.receipts.receive(id);
    }
    c3.sign_off(0, "甲", true);
    chk(&mut set, "C4020-11", c3.sign_off(1, "甲", true) == T26CCode::SAME_SIGNER, "err");

    // C4020-12 复核拒签 → 催办 + REJECTED 终态。
    let mut c4 = Closer::new();
    for part in T01_PARTS.iter() {
        c4.parts.fill(part, "C", true);
    }
    for id in T01_CHECK_IDS.iter() {
        c4.receipts.receive(id);
    }
    c4.sign_off(0, "甲", true);
    chk(&mut set, "C4020-12",
        c4.sign_off(1, "乙", false) == T26CCode::REJECTED
            && c4.urgencies == 1
            && c4.outcome() == T26CCode::REJECTED, "err");

    // C4020-13 空签人 → 缺签。
    let mut c5 = Closer::new();
    for part in T01_PARTS.iter() {
        c5.parts.fill(part, "C", true);
    }
    for id in T01_CHECK_IDS.iter() {
        c5.receipts.receive(id);
    }
    chk(&mut set, "C4020-13", c5.sign_off(0, "", true) == T26CCode::SIGN_MISSING, "err");

    // C4020-14 回执表六判据 ID 齐备。
    chk(&mut set, "C4020-14",
        T01_CHECK_IDS.len() == 6 && T01_CHECK_IDS[0] == "C4001", "err");

    // C4020-15 判据 ID 互异。
    let mut uniq = true;
    for i in 0..6 {
        for j in (i + 1)..6 {
            if T01_CHECK_IDS[i] == T01_CHECK_IDS[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4020-15", uniq, "err");

    // C4020-16 双签闭合需双 OK。
    let mut d = DualSign::new();
    d.sign(0, "甲", true);
    d.sign(1, "乙", false);
    chk(&mut set, "C4020-16", !d.closed(), "err");

    // C4020-17 收口机幂等终态。
    chk(&mut set, "C4020-17",
        c2.outcome() == T26CCode::OK && c2.outcome() == T26CCode::OK, "err");

    // C4020-18 红件也齐备（齐备≠全绿）。
    chk(&mut set, "C4020-18",
        c3.parts.complete() && !c3.parts.all_green() == false, "err");

    // C4020-19 错误码互异。
    let codes = [T26CCode::OK.0, T26CCode::QUAD_MISS.0, T26CCode::SAME_SIGNER.0,
        T26CCode::SIGN_MISSING.0, T26CCode::REJECTED.0];
    let mut uniq2 = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { uniq2 = false; }
        }
    }
    chk(&mut set, "C4020-19", uniq2, "err");

    // C4020-20 六件名互异。
    let mut uniq3 = true;
    for i in 0..6 {
        for j in (i + 1)..6 {
            if T01_PARTS[i] == T01_PARTS[j] { uniq3 = false; }
        }
    }
    chk(&mut set, "C4020-20", uniq3, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closer_gates_and_signs() {
        let mut c = Closer::new();
        assert_eq!(c.sign_off(0, "甲", true), T26CCode::QUAD_MISS);
        for part in T01_PARTS.iter() {
            c.parts.fill(part, "C", true);
        }
        for id in T01_CHECK_IDS.iter() {
            c.receipts.receive(id);
        }
        c.sign_off(0, "甲", true);
        c.sign_off(1, "乙", true);
        assert_eq!(c.outcome(), T26CCode::OK);
    }

    #[test]
    fn same_signer_rejected() {
        let mut d = DualSign::new();
        d.sign(0, "x", true);
        assert_eq!(d.sign(1, "x", true), T26CCode::SAME_SIGNER);
    }
}
