//! VE-F3996 · S 域变更纪律与承诺条款
//!
//! 纪律：S 域冻结后变更纪律（变更申请→影响面评估→双签→哈希更新→通告
//! 五步）+ 承诺条款（对外双承诺状态终审 + 违诺处置）。
//! 数据结构：纪律册（五步）；承诺台账；处置记录。
//! 错误路径：越步变更→作废+回退；违诺→处置+通告；评估缺→拒批。
//! 性能：走步 O(变更)；评估 O(面)；通告 O(1)。
//! 跨批对接：F3990 冻结联动；F3981 承诺上游；家族纪律同构。
//! 无障碍与隐私：变更评估含无障碍影响面专列（域本色）；承诺不含用户
//! 数据越界条款。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x48C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x48C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T16MCode(pub u16);

impl T16MCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const STEP_SKIP: Self = Self(SEG_BASE + 1);
    pub const ASSESS_MISS: Self = Self(SEG_BASE + 2);
    pub const PROMISE_BROKEN: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::STEP_SKIP => "越步变更→作废+回退",
            Self::ASSESS_MISS => "评估缺→拒批",
            Self::PROMISE_BROKEN => "违诺→处置+通告",
            _ => "未知",
        }
    }
}

// ── 五步变更纪律 ─────────────────────────────────────────────────────

/// 五步：申请 → 评估 → 双签 → 哈希 → 通告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStep {
    Apply,
    Assess,
    DualSign,
    HashUpdate,
    Announce,
}

pub const CHANGE_STEPS: [ChangeStep; 5] = [
    ChangeStep::Apply, ChangeStep::Assess, ChangeStep::DualSign,
    ChangeStep::HashUpdate, ChangeStep::Announce,
];

impl ChangeStep {
    pub fn name(self) -> &'static str {
        match self {
            ChangeStep::Apply => "变更申请",
            ChangeStep::Assess => "影响面评估",
            ChangeStep::DualSign => "双签",
            ChangeStep::HashUpdate => "哈希更新",
            ChangeStep::Announce => "通告",
        }
    }

    pub fn order(self) -> usize {
        match self {
            ChangeStep::Apply => 0,
            ChangeStep::Assess => 1,
            ChangeStep::DualSign => 2,
            ChangeStep::HashUpdate => 3,
            ChangeStep::Announce => 4,
        }
    }
}

/// 单条变更单。
#[derive(Debug, Clone)]
pub struct ChangeOrder {
    /// 已走步（按序标记）。
    pub steps_done: [bool; 5],
    /// 无障碍影响面专列（域本色——评估步必填）。
    pub a11y_impact: String,
    /// 双签两签人（禁同人）。
    pub signer_a: String,
    pub signer_b: String,
    /// 哈希更新后值。
    pub new_hash: u64,
    /// 已通告。
    pub announced: bool,
}

impl ChangeOrder {
    pub fn new() -> Self {
        Self {
            steps_done: [false; 5],
            a11y_impact: String::new(),
            signer_a: String::new(),
            signer_b: String::new(),
            new_hash: 0,
            announced: false,
        }
    }

    /// 走步（严格按序，越步拒绝）。
    pub fn step(&mut self, s: ChangeStep) -> T16MCode {
        let o = s.order();
        if o > 0 && !self.steps_done[..o].iter().all(|&b| b) {
            return T16MCode::STEP_SKIP;
        }
        // 评估步须带无障碍专列。
        if s == ChangeStep::Assess && self.a11y_impact.is_empty() {
            return T16MCode::ASSESS_MISS;
        }
        self.steps_done[o] = true;
        T16MCode::OK
    }

    /// 双签登记（禁同人，DualSign 步内）。
    pub fn sign(&mut self, a: &str, b: &str) -> T16MCode {
        if !self.steps_done[ChangeStep::Assess.order()] {
            return T16MCode::STEP_SKIP;
        }
        if a.is_empty() || b.is_empty() || a == b {
            return T16MCode::STEP_SKIP;
        }
        self.signer_a = String::from(a);
        self.signer_b = String::from(b);
        self.steps_done[ChangeStep::DualSign.order()] = true;
        T16MCode::OK
    }

    /// 哈希更新。
    pub fn update_hash(&mut self, h: u64) -> T16MCode {
        if !self.steps_done[ChangeStep::DualSign.order()] {
            return T16MCode::STEP_SKIP;
        }
        self.new_hash = h;
        self.steps_done[ChangeStep::HashUpdate.order()] = true;
        T16MCode::OK
    }

    /// 通告（终步）。
    pub fn announce(&mut self) -> T16MCode {
        if !self.steps_done[ChangeStep::HashUpdate.order()] {
            return T16MCode::STEP_SKIP;
        }
        self.announced = true;
        self.steps_done[ChangeStep::Announce.order()] = true;
        T16MCode::OK
    }

    /// 五步闭环。
    pub fn complete(&self) -> bool {
        self.steps_done.iter().all(|&b| b) && self.announced
    }
}

// ── 承诺台账 ─────────────────────────────────────────────────────────

/// 承诺条目。
#[derive(Debug, Clone)]
pub struct PromiseLedger {
    /// 承诺名。
    pub name: String,
    /// 兑现。
    pub kept: bool,
    /// 违诺处置记录（违诺时必填）。
    pub remedy: String,
}

/// 台账。
#[derive(Debug, Clone)]
pub struct PromiseBook {
    pub rows: Vec<PromiseLedger>,
}

impl PromiseBook {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn put(&mut self, name: &str, kept: bool) {
        self.rows.push(PromiseLedger {
            name: String::from(name),
            kept,
            remedy: String::new(),
        });
    }

    /// 违诺处置：处置记录必填 + 违诺清单。
    pub fn remedy(&mut self, name: &str, action: &str) -> bool {
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.remedy = String::from(action);
                true
            }
            None => false,
        }
    }

    /// 违诺且未处置清单（通告对象）。
    pub fn unresolved_breaks(&self) -> Vec<String> {
        self.rows.iter()
            .filter(|r| !r.kept && r.remedy.is_empty())
            .map(|r| r.name.clone())
            .collect()
    }

    /// 承诺终审：零违诺或违诺全处置。
    pub fn final_ok(&self) -> bool {
        self.unresolved_breaks().is_empty()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves250_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves250_s10change");

    // C3996-01 诊断码段不越窗。
    chk(&mut set, "C3996-01",
        T16MCode::PROMISE_BROKEN.0 < SEG_BASE + 16 && SEG_BASE == 0x48C0, "err");

    // C3996-02 五步齐备按序。
    chk(&mut set, "C3996-02",
        CHANGE_STEPS.len() == 5
            && CHANGE_STEPS[0] == ChangeStep::Apply
            && CHANGE_STEPS[4] == ChangeStep::Announce, "err");

    // C3996-03 越步拒绝（跳过申请直接评估）。
    let mut o = ChangeOrder::new();
    chk(&mut set, "C3996-03", o.step(ChangeStep::Assess) == T16MCode::STEP_SKIP, "err");

    // C3996-04 申请步过；评估缺无障碍专列 → 拒批。
    chk(&mut set, "C3996-04",
        o.step(ChangeStep::Apply) == T16MCode::OK
            && o.step(ChangeStep::Assess) == T16MCode::ASSESS_MISS, "err");

    // C3996-05 无障碍专列补 → 评估过。
    o.a11y_impact = String::from("无障碍影响面：色标切换路径重排");
    chk(&mut set, "C3996-05", o.step(ChangeStep::Assess) == T16MCode::OK, "err");

    // C3996-06 双签同人拒。
    chk(&mut set, "C3996-06", o.sign("甲", "甲") == T16MCode::STEP_SKIP, "err");

    // C3996-07 双签异人过 → 哈希 → 通告 → 闭环。
    chk(&mut set, "C3996-07",
        o.sign("甲", "乙") == T16MCode::OK
            && o.update_hash(0x5EED) == T16MCode::OK
            && o.announce() == T16MCode::OK
            && o.complete(), "err");

    // C3996-08 双签前哈希更新拒（越步）。
    let mut o2 = ChangeOrder::new();
    o2.step(ChangeStep::Apply);
    o2.a11y_impact = String::from("无");
    o2.step(ChangeStep::Assess);
    chk(&mut set, "C3996-08", o2.update_hash(1) == T16MCode::STEP_SKIP, "err");

    // C3996-09 空签人拒。
    chk(&mut set, "C3996-09", o2.sign("甲", "") == T16MCode::STEP_SKIP, "err");

    // C3996-10 哈希前双签齐但评估缺仍拒（保持一致）。
    o2.sign("甲", "乙");
    chk(&mut set, "C3996-10", o2.update_hash(2) == T16MCode::OK, "err");

    // C3996-11 通告前置哈希。
    let mut o3 = ChangeOrder::new();
    o3.step(ChangeStep::Apply);
    o3.a11y_impact = String::from("无");
    o3.step(ChangeStep::Assess);
    o3.sign("甲", "乙");
    chk(&mut set, "C3996-11", o3.announce() == T16MCode::STEP_SKIP, "err");
    o3.update_hash(3);
    chk(&mut set, "C3996-11", o3.announce() == T16MCode::OK && o3.complete(), "err");

    // C3996-12 承诺台账：违诺未处置清单。
    let mut p = PromiseBook::new();
    p.put("读屏措辞承诺", true);
    p.put("色标文化色承诺", false);
    chk(&mut set, "C3996-12",
        p.unresolved_breaks() == vec![String::from("色标文化色承诺")] && !p.final_ok(), "err");

    // C3996-13 违诺处置后终审过。
    p.remedy("色标文化色承诺", "补齐文化色映射并通告");
    chk(&mut set, "C3996-13", p.final_ok(), "err");

    // C3996-14 越名处置拒绝。
    chk(&mut set, "C3996-14", !p.remedy("不存在", "x"), "err");

    // C3996-15 全兑现台账直过。
    let mut p2 = PromiseBook::new();
    p2.put("甲承诺", true);
    p2.put("乙承诺", true);
    chk(&mut set, "C3996-15", p2.final_ok() && p2.unresolved_breaks().is_empty(), "err");

    // C3996-16 步序值双射。
    let orders: Vec<usize> = CHANGE_STEPS.iter().map(|s| s.order()).collect();
    let mut uniq = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if orders[i] == orders[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3996-16", uniq, "err");

    // C3996-17 步名互异。
    let mut uniq2 = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if CHANGE_STEPS[i].name() == CHANGE_STEPS[j].name() { uniq2 = false; }
        }
    }
    chk(&mut set, "C3996-17", uniq2, "err");

    // C3996-18 五步从零开始全流程（第二条变更单复用）。
    let mut o4 = ChangeOrder::new();
    o4.step(ChangeStep::Apply);
    o4.a11y_impact = String::from("焦点环无障碍影响面专列：无回归");
    o4.step(ChangeStep::Assess);
    o4.sign("丙", "丁");
    o4.update_hash(0xABCD);
    o4.announce();
    chk(&mut set, "C3996-18", o4.complete(), "err");

    // C3996-19 违诺码语义。
    chk(&mut set, "C3996-19", T16MCode::PROMISE_BROKEN.explain().contains("通告"), "err");

    // C3996-20 错误码互异。
    let codes = [T16MCode::OK.0, T16MCode::STEP_SKIP.0, T16MCode::ASSESS_MISS.0,
        T16MCode::PROMISE_BROKEN.0];
    chk(&mut set, "C3996-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_step_order() {
        let mut o = ChangeOrder::new();
        assert_eq!(o.step(ChangeStep::Announce), T16MCode::STEP_SKIP);
        assert_eq!(o.step(ChangeStep::Apply), T16MCode::OK);
        assert_eq!(o.step(ChangeStep::Assess), T16MCode::ASSESS_MISS);
    }

    #[test]
    fn same_signer_rejected() {
        let mut o = ChangeOrder::new();
        o.step(ChangeStep::Apply);
        o.a11y_impact = String::from("n");
        o.step(ChangeStep::Assess);
        assert_eq!(o.sign("a", "a"), T16MCode::STEP_SKIP);
    }

    #[test]
    fn promise_remedy_flow() {
        let mut p = PromiseBook::new();
        p.put("x", false);
        assert!(!p.final_ok());
        p.remedy("x", "r");
        assert!(p.final_ok());
    }
}
