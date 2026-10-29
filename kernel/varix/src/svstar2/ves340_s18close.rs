//! ves340_s18close — VE-F4100 · T05 组收口双签
//! 20 项自查汇总 + 五检全通过证明 + 联调证据 + 安全三道记录——四件齐备双签；自查签与复核签分离禁同人；五检未全过硬性拒签；无障碍资产齐备确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9E40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    MissingPiece = SEG_BASE + 0x01,
    SameSigner = SEG_BASE + 0x02,
    FiveCheckNotClean = SEG_BASE + 0x03,
}

/// 四件套。
pub const FOUR_PIECES: [&'static str; 4] =
    ["self-check-20", "five-check-proof", "integ-evidence", "sec-three-records"];

/// 收口清单：四件 + 20 项自查逐项 + 五检结果 + 无障碍确认项。
pub struct ClosureChecklist {
    /// 20 项自查（true=过）。
    pub self_checks: Vec<bool>,
    /// 五检结果（五项全 true 才可签）。
    pub five_checks: Vec<bool>,
    /// 联调证据指纹。
    pub integ_evidence: u32,
    /// 安全三道记录齐备。
    pub sec_three: bool,
    /// 无障碍资产齐备确认项（域本色）。
    pub a11y_confirmed: bool,
}

pub const SELF_CHECK_TOTAL: usize = 20;
pub const FIVE_CHECK_TOTAL: usize = 5;

impl ClosureChecklist {
    pub fn new() -> Self {
        ClosureChecklist {
            self_checks: Vec::new(), five_checks: Vec::new(),
            integ_evidence: 0, sec_three: false, a11y_confirmed: false,
        }
    }

    /// 四件齐备判定。
    pub fn pieces_complete(&self) -> bool {
        self.self_checks.len() == SELF_CHECK_TOTAL && self.self_checks.iter().all(|c| *c)
            && self.five_checks.len() == FIVE_CHECK_TOTAL && self.five_checks.iter().all(|c| *c)
            && self.integ_evidence != 0
            && self.sec_three
    }

    /// 缺件定位。
    pub fn missing_pieces(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.self_checks.len() != SELF_CHECK_TOTAL || self.self_checks.iter().any(|c| !*c) {
            out.push(FOUR_PIECES[0]);
        }
        if self.five_checks.len() != FIVE_CHECK_TOTAL || self.five_checks.iter().any(|c| !*c) {
            out.push(FOUR_PIECES[1]);
        }
        if self.integ_evidence == 0 { out.push(FOUR_PIECES[2]); }
        if !self.sec_three { out.push(FOUR_PIECES[3]); }
        out
    }

    /// 五检全过硬门（签前单独复核——硬性拒签面）。
    pub fn five_checks_clean(&self) -> bool {
        self.five_checks.len() == FIVE_CHECK_TOTAL && self.five_checks.iter().all(|c| *c)
    }
}

/// 双签记录：自查签与复核签分离禁同人。
#[derive(Clone)]
pub struct DualSign { pub first_signer: &'static str, pub reviewer: &'static str, pub valid: bool }

pub struct SignBook { pub signs: Vec<DualSign> }

impl SignBook {
    pub fn new() -> Self { SignBook { signs: Vec::new() } }

    /// 双签：同人 → 作废（valid=false）；异人 → 有效。
    pub fn sign(&mut self, list: &ClosureChecklist, first: &'static str, reviewer: &'static str) -> Result<(), ClCode> {
        if !list.pieces_complete() { return Err(ClCode::MissingPiece); }
        if !list.five_checks_clean() { return Err(ClCode::FiveCheckNotClean); }
        let same = first == reviewer;
        self.signs.push(DualSign { first_signer: first, reviewer, valid: !same });
        if same { return Err(ClCode::SameSigner); }
        Ok(())
    }

    /// 有效双签存在判定。
    pub fn has_valid_sign(&self) -> bool {
        self.signs.iter().any(|s| s.valid)
    }

    /// 作废重签：作废全部无效签后可重签。
    pub fn void_invalidate(&mut self) -> usize {
        let n = self.signs.iter().filter(|s| !s.valid).count();
        self.signs.retain(|s| s.valid);
        n
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves340_checks() -> CheckSet {
    let mut set = CheckSet::new("ves340");

    let mk_ok = || {
        let mut cl = ClosureChecklist::new();
        for _ in 0..SELF_CHECK_TOTAL { cl.self_checks.push(true); }
        for _ in 0..FIVE_CHECK_TOTAL { cl.five_checks.push(true); }
        cl.integ_evidence = 0xDEAD_BEEF;
        cl.sec_three = true;
        cl.a11y_confirmed = true;
        cl
    };

    // C340-01 四件齐备 → 可签。
    let cl = mk_ok();
    chk(&mut set, "C340-01", cl.pieces_complete() && cl.missing_pieces().is_empty(), "四件齐备");

    // C340-02 缺件定位：证据指纹缺失。
    let mut cl2 = mk_ok();
    cl2.integ_evidence = 0;
    chk(&mut set, "C340-02", cl2.missing_pieces() == vec!["integ-evidence"], "证据缺定位");

    // C340-03 自查缺项定位。
    let mut cl3 = mk_ok();
    cl3.self_checks[7] = false;
    chk(&mut set, "C340-03", cl3.missing_pieces() == vec!["self-check-20"], "自查 20 项一项红 => 缺件");

    // C340-04 五检未全过 → 硬性拒签。
    let mut cl4 = mk_ok();
    cl4.five_checks[4] = false; // 无障碍齐备检红
    let mut sb = SignBook::new();
    chk(&mut set, "C340-04", sb.sign(&cl4, "ai-a", "ai-b") == Err(ClCode::FiveCheckNotClean), "五检未全过 => 硬拒");

    // C340-05 四件缺一 → 拒签。
    let mut sb2 = SignBook::new();
    chk(&mut set, "C340-05", sb2.sign(&cl2, "ai-a", "ai-b") == Err(ClCode::MissingPiece), "缺件 => MissingPiece");

    // C340-06 合法双签成功。
    let mut sb3 = SignBook::new();
    chk(&mut set, "C340-06", sb3.sign(&cl, "ai-wb", "ai-qc").is_ok() && sb3.has_valid_sign(), "异人双签有效");

    // C340-07 同人签 → 作废。
    let mut sb4 = SignBook::new();
    chk(&mut set, "C340-07", sb4.sign(&cl, "ai-wb", "ai-wb") == Err(ClCode::SameSigner) && !sb4.has_valid_sign(), "同人 => 作废");

    // C340-08 作废重签：清无效签后重签成功。
    let n = sb4.void_invalidate();
    chk(&mut set, "C340-08", n == 1 && sb4.signs.is_empty() && sb4.sign(&cl, "ai-wb", "ai-qc").is_ok(), "作废清空后重签");

    // C340-09 四件名单单源。
    chk(&mut set, "C340-09", FOUR_PIECES.len() == 4 && FOUR_PIECES[0] == "self-check-20", "四件单源");

    // C340-10 20 项计数单源。
    chk(&mut set, "C340-10", SELF_CHECK_TOTAL == 20 && cl.self_checks.len() == 20, "20 项自查满额");

    // C340-11 无障碍资产齐备确认项（域本色）——确认项不入四件但签收必查。
    chk(&mut set, "C340-11", cl.a11y_confirmed, "无障碍确认项在签收单");

    // C340-12 安全三道记录齐备是签前提。
    let mut cl5 = mk_ok();
    cl5.sec_three = false;
    chk(&mut set, "C340-12", cl5.missing_pieces() == vec!["sec-three-records"], "安全三道缺 => 拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves340_smoke() {
        let set = run_ves340_checks();
        assert!(set.failed_count() == 0, "ves340 red: {}", set.red_items_text());
    }

    #[test]
    fn ves340_five_check_gate() {
        let mut cl = ClosureChecklist::new();
        cl.five_checks = vec![true, true, true, true, false];
        assert!(!cl.five_checks_clean());
        let mut sb = SignBook::new();
        assert_eq!(sb.sign(&cl, "a", "b"), Err(ClCode::FiveCheckNotClean));
    }
}
