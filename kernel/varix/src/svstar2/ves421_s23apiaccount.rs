//! ves421_s23apiaccount — VE-F4181 · T 域接口总账与主题映射终审
//! 终审：总账终版对账（册版本冻结+修正留痕+对外承诺表回执状态）；主题映射终审（二十项→落条唯一性断言）；
//! 六域映射家族对拍（O/P/Q/R/S 五域映射同构复核）；对账非冻结→阻断；映射缺/重→修正；回执未达→催办；
//! 总账含多语言文本接口章（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0FE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AaCode {
    Ok = 0,
    NotFrozen = SEG_BASE + 0x01,   // 0x0FE1
    MapDuplicate = SEG_BASE + 0x02, // 0x0FE2
    MapMissing = SEG_BASE + 0x03,  // 0x0FE3
    ReceiptMissing = SEG_BASE + 0x04, // 0x0FE4
}

/// 接口总账终版。
pub struct LedgerFinal {
    pub version: u32,
    pub frozen: bool,
    pub amendments: Vec<(u32, String)>, // 修正留痕：(轮次, 说明)
    pub promises: Vec<(&'static str, bool)>, // 对外承诺表（名, 回执达）
    pub i18n_text_iface: bool,          // 多语言文本接口章（域本色）
}

impl LedgerFinal {
    pub fn new(version: u32) -> Self {
        LedgerFinal { version, frozen: false, amendments: Vec::new(), promises: Vec::new(), i18n_text_iface: false }
    }
    pub fn amend(&mut self, round: u32, note: &str) {
        self.amendments.push((round, String::from(note)));
    }
    pub fn freeze(&mut self) -> Result<(), AaCode> {
        if !self.i18n_text_iface { return Err(AaCode::NotFrozen); }
        self.frozen = true;
        Ok(())
    }
    pub fn promise_receipt(&self, name: &str) -> bool {
        self.promises.iter().any(|(n, ok)| *n == name && *ok)
    }
    /// 回执未达清单（催办源）。
    pub fn unmet_promises(&self) -> Vec<&'static str> {
        self.promises.iter().filter(|(_, ok)| !*ok).map(|(n, _)| *n).collect()
    }
}

/// 主题映射（二十项→落条）唯一性终审。
pub struct TopicMap { pub pairs: Vec<(u8, u16)> } // (主题项 0..20, 落条 id)

impl TopicMap {
    pub fn new() -> Self { TopicMap { pairs: Vec::new() } }
    pub fn put(&mut self, topic: u8, entry: u16) { self.pairs.push((topic, entry)); }
    /// 唯一性断言：主题不得缺、落条不得重。
    pub fn audit(&self, topics: u8) -> Result<(), AaCode> {
        for t in 0..topics {
            let c = self.pairs.iter().filter(|(tt, _)| *tt == t).count();
            if c == 0 { return Err(AaCode::MapMissing); }
            if c > 1 { return Err(AaCode::MapDuplicate); }
        }
        let mut entries: Vec<u16> = self.pairs.iter().map(|(_, e)| *e).collect();
        entries.sort_unstable();
        for w in entries.windows(2) {
            if w[0] == w[1] { return Err(AaCode::MapDuplicate); }
        }
        Ok(())
    }
}

/// 六域映射家族对拍：O/P/Q/R/S 五域映射与本域同构（每域项数一致且无重落条）。
pub struct FamilyMap { pub members: Vec<(&'static str, Vec<(u8, u16)>)> }

impl FamilyMap {
    pub fn new() -> Self { FamilyMap { members: Vec::new() } }
    pub fn add(&mut self, dom: &'static str, map: Vec<(u8, u16)>) { self.members.push((dom, map)); }
    /// 同构复核：与基准 map 项数一致且各域内部无重落条。
    pub fn isomorphic(&self, base: &TopicMap) -> bool {
        if self.members.len() < 5 { return false; }
        for (_, m) in &self.members {
            if m.len() != base.pairs.len() { return false; }
            let mut es: Vec<u16> = m.iter().map(|(_, e)| *e).collect();
            es.sort_unstable();
            for w in es.windows(2) { if w[0] == w[1] { return false; } }
        }
        true
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves421_checks() -> CheckSet {
    let mut set = CheckSet::new("ves421");

    // C421-01 未含 i18n 文本接口章冻结被拒（域本色门）。
    let mut lg = LedgerFinal::new(7);
    chk(&mut set, "C421-01", lg.freeze() == Err(AaCode::NotFrozen) && !lg.frozen, "缺 i18n 章拒冻");

    // C421-02 补章后冻结通过。
    lg.i18n_text_iface = true;
    chk(&mut set, "C421-02", lg.freeze().is_ok() && lg.frozen, "补章冻结");

    // C421-03 修正留痕可追溯。
    lg.amend(1, "fix iface 12");
    lg.amend(2, "fix iface 19");
    chk(&mut set, "C421-03", lg.amendments.len() == 2 && lg.amendments[1].0 == 2, "修正留痕");

    // C421-04 承诺回执未达清单（催办源）。
    lg.promises.push(("iface-a", true));
    lg.promises.push(("iface-b", false));
    chk(&mut set, "C421-04", lg.unmet_promises() == vec!["iface-b"], "未达回执定位");

    // C421-05 回执已达查询。
    chk(&mut set, "C421-05", lg.promise_receipt("iface-a") && !lg.promise_receipt("iface-b"), "回执状态");

    // C421-06 主题映射缺项被拒。
    let mut tm = TopicMap::new();
    tm.put(0, 100); tm.put(1, 101);
    chk(&mut set, "C421-06", tm.audit(4) == Err(AaCode::MapMissing), "映射缺项拒");

    // C421-07 落条重复被拒。
    let mut tm2 = TopicMap::new();
    tm2.put(0, 100); tm2.put(1, 100);
    chk(&mut set, "C421-07", tm2.audit(2) == Err(AaCode::MapDuplicate), "落条重复拒");

    // C421-08 二十项一一映射通过（唯一性断言）。
    let mut tm3 = TopicMap::new();
    for t in 0..20u8 { tm3.put(t, 1000 + t as u16); }
    chk(&mut set, "C421-08", tm3.audit(20).is_ok(), "20 项唯一映射");

    // C421-09 主题重复映射拒（一项两落条）。
    let mut tm4 = TopicMap::new();
    tm4.put(3, 5); tm4.put(3, 6);
    chk(&mut set, "C421-09", tm4.audit(4) == Err(AaCode::MapDuplicate), "主题双落条拒");

    // C421-10 家族对拍：五域同构通过。
    let mut fam = FamilyMap::new();
    for dom in ["O", "P", "Q", "R", "S"] {
        let m: Vec<(u8, u16)> = (0..20u8).map(|t| (t, 2000 + t as u16)).collect();
        fam.add(dom, m);
    }
    chk(&mut set, "C421-10", fam.isomorphic(&tm3), "五域同构");

    // C421-11 家族成员不足五域拒。
    let mut fam2 = FamilyMap::new();
    for dom in ["O", "P", "Q"] {
        let m: Vec<(u8, u16)> = (0..20u8).map(|t| (t, 2000 + t as u16)).collect();
        fam2.add(dom, m);
    }
    chk(&mut set, "C421-11", !fam2.isomorphic(&tm3), "不足五域拒");

    // C421-12 家族内部落条重复拒。
    let mut fam3 = FamilyMap::new();
    for dom in ["O", "P", "Q", "R", "S"] {
        let m: Vec<(u8, u16)> = (0..20u8).map(|t| (t, 2000 + t as u16)).collect();
        fam3.add(dom, m);
    }
    fam3.members[2].1[1].1 = fam3.members[2].1[0].1;
    chk(&mut set, "C421-12", !fam3.isomorphic(&tm3), "域内重落条拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves421_smoke() {
        let set = run_ves421_checks();
        assert!(set.failed_count() == 0, "ves421 red: {}", set.red_items_text());
    }
}
