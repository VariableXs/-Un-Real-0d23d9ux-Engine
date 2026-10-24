//! ves440_s23declare — VE-F4200 · T 域收官宣告
//! 宣告：域级收官宣告——总对账（200 项实测 200/200——硬门）；八组收口 8/8；三基线冻结哈希确认；
//! 硬门六重宣告（完备/冻结/零🔴/GO/长稳/心流——T 域六重）；20 维三 90 确认；条件核销；
//! 最终宣告（T 域深化与规格收官——第十七个域宣告——家族第十七位登记）；
//! 任一硬门未过→阻断；失实→撤回+立案；逾期→复议。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x13E0;
pub const DOMAIN_ITEMS: usize = 200;
pub const DOMAIN_SEQ: usize = 17; // 家族第十七位

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    GateBlocked = SEG_BASE + 0x01,  // 0x13E1
    FalseClaim = SEG_BASE + 0x02,   // 0x13E2 失实撤回
    OverdueReview = SEG_BASE + 0x03, // 0x13E3 逾期复议
}

/// 六重硬门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HardGate { Completeness = 0, Frozen = 1, ZeroRed = 2, GoSign = 3, LongStable = 4, Flow = 5 }

impl HardGate {
    pub const ALL: [HardGate; 6] = [
        HardGate::Completeness, HardGate::Frozen, HardGate::ZeroRed,
        HardGate::GoSign, HardGate::LongStable, HardGate::Flow];
    pub fn name(self) -> &'static str {
        match self {
            HardGate::Completeness => "complete", HardGate::Frozen => "frozen", HardGate::ZeroRed => "zero-red",
            HardGate::GoSign => "go", HardGate::LongStable => "long-stable", HardGate::Flow => "flow",
        }
    }
}

/// 收官宣告册。
pub struct DeclareBook {
    pub items_done: usize,       // 总对账实测完成项
    pub groups_closed: usize,    // 八组收口数（0..8）
    pub freeze_hash_ok: bool,    // 三基线冻结哈希确认
    pub gates: Vec<HardGate>,    // 六重宣告已过门
    pub three90_ok: bool,        // 20 维三 90 确认
    pub conditions_cleared: Vec<&'static str>, // 条件核销
    pub declared: bool,
    pub withdrawn: bool,         // 失实撤回
    pub filed: u32,
}

impl DeclareBook {
    pub fn new() -> Self {
        DeclareBook { items_done: 0, groups_closed: 0, freeze_hash_ok: false, gates: Vec::new(),
            three90_ok: false, conditions_cleared: Vec::new(), declared: false, withdrawn: false, filed: 0 }
    }
    pub fn pass_gate(&mut self, g: HardGate) { if !self.gates.contains(&g) { self.gates.push(g); } }
    pub fn clear_condition(&mut self, c: &'static str) {
        if !self.conditions_cleared.contains(&c) { self.conditions_cleared.push(c); }
    }
    /// 总对账硬门：200/200。
    pub fn ledger_gate(&self) -> bool { self.items_done == DOMAIN_ITEMS }
    /// 八组收口 8/8。
    pub fn groups_gate(&self) -> bool { self.groups_closed == 8 }
    /// 六重宣告核验：六门全过。
    pub fn gates_full(&self) -> bool {
        HardGate::ALL.iter().all(|g| self.gates.contains(g))
    }
    /// 宣告裁决：全硬门→宣告；任一未过→阻断。
    pub fn declare(&mut self) -> Result<(), DcCode> {
        if self.withdrawn { return Err(DcCode::FalseClaim); }
        if !self.ledger_gate() || !self.groups_gate() || !self.freeze_hash_ok
            || !self.gates_full() || !self.three90_ok {
            self.filed += 1;
            return Err(DcCode::GateBlocked);
        }
        self.declared = true;
        Ok(())
    }
    /// 失实→撤回+立案。
    pub fn withdraw(&mut self) {
        self.declared = false;
        self.withdrawn = true;
        self.filed += 1;
    }
    /// 撤回后整改复告：清撤回位。
    pub fn clear_withdrawal(&mut self) { self.withdrawn = false; }
    /// 逾期→复议。
    pub fn overdue_review(&mut self) { self.filed += 1; }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves440_checks() -> CheckSet {
    let mut set = CheckSet::new("ves440");
    let mut bk = DeclareBook::new();

    // C440-01 空册宣告→阻断（总对账未过）。
    chk(&mut set, "C440-01", bk.declare() == Err(DcCode::GateBlocked) && bk.filed == 1, "空册阻断");

    // C440-02 总对账 200/200 过但八组未收口→仍阻断。
    bk.items_done = DOMAIN_ITEMS;
    chk(&mut set, "C440-02", bk.declare() == Err(DcCode::GateBlocked), "八组缺阻断");

    // C440-03 八组收口 8/8 + 冻结哈希确认。
    bk.groups_closed = 8;
    bk.freeze_hash_ok = true;
    chk(&mut set, "C440-03", bk.declare() == Err(DcCode::GateBlocked), "六重缺阻断");

    // C440-04 六重门逐重宣告（逐门推进核验）。
    for g in HardGate::ALL { bk.pass_gate(g); }
    chk(&mut set, "C440-04", bk.declare() == Err(DcCode::GateBlocked), "三 90 缺阻断");

    // C440-05 三 90 确认+条件核销→宣告通过。
    bk.three90_ok = true;
    bk.clear_condition("a11y-terminal");
    chk(&mut set, "C440-05", bk.declare().is_ok() && bk.declared, "宣告通过");

    // C440-06 六重门名单单源（六重全过断言）。
    chk(&mut set, "C440-06", bk.gates_full() && bk.gates.len() == 6, "六重全过");

    // C440-07 失实→撤回+立案。
    bk.withdraw();
    chk(&mut set, "C440-07", !bk.declared && bk.withdrawn && bk.filed == 1, "失实撤回立案");

    // C440-08 撤回态下再宣告→拒（FalseClaim）。
    chk(&mut set, "C440-08", bk.declare() == Err(DcCode::FalseClaim), "撤回态拒宣");

    // C440-09 整改复告复通。
    bk.clear_withdrawal();
    chk(&mut set, "C440-09", bk.declare().is_ok() && bk.declared, "复告复通");

    // C440-10 逾期→复议留痕。
    bk.overdue_review();
    chk(&mut set, "C440-10", bk.filed == 2, "逾期复议");

    // C440-11 六重门名单单源断言。
    chk(&mut set, "C440-11", HardGate::ALL.len() == 6
        && HardGate::ALL[0].name() == "complete"
        && HardGate::ALL[5].name() == "flow", "六重单源");

    // C440-12 家族第十七位登记断言（200 项 + 8 组 + 17 域序）。
    chk(&mut set, "C440-12", DOMAIN_ITEMS == 200 && DOMAIN_SEQ == 17
        && bk.ledger_gate() && bk.groups_gate(), "第十七域宣告");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves440_smoke() {
        let set = run_ves440_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves440 red: {:?}", red);
    }
}
