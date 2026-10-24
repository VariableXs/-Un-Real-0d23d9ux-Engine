//! ves299_s14legacy — VE-F4059 · T03 排版预留清账
//! F2910/F2915 预留激活清账（激活未清账=预留债——deadline 台账复用单源断言）+ 三类处置复述 + 闭环复验 + 整形器激活状态核验。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8640;

pub const DEADLINE_TICK: u64 = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum LgCode {
    Ok = 0,
    Unassessed = SEG_BASE + 0x01,
    Overdue = SEG_BASE + 0x02,
    ReserveDebt = SEG_BASE + 0x03,
    LoopMissing = SEG_BASE + 0x04,
    NotInLedger = SEG_BASE + 0x05,
    NotActivated = SEG_BASE + 0x06,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disposition { Retire = 0, Merge = 1, Keep = 2 }

impl Disposition {
    pub const ALL: [Disposition; 3] = [Disposition::Retire, Disposition::Merge, Disposition::Keep];
    pub fn name(self) -> &'static str {
        match self { Disposition::Retire => "清退", Disposition::Merge => "合并", Disposition::Keep => "保留" }
    }
}

#[derive(Clone)]
pub struct LegacyItem {
    pub label: String,
    /// 预留已激活（F2910/F2915）。
    pub activated: bool,
    pub assessed: bool,
    pub disposition: Option<Disposition>,
    pub deadline: u64,
    pub settled: bool,
    pub looped: bool,
}

impl LegacyItem {
    pub fn new(label: &str, activated: bool) -> Self {
        LegacyItem {
            label: String::from(label),
            activated,
            assessed: false,
            disposition: None,
            deadline: 0,
            settled: false,
            looped: false,
        }
    }
}

pub struct LegacyBook {
    pub items: Vec<LegacyItem>,
}

impl LegacyBook {
    pub fn new() -> Self { LegacyBook { items: Vec::new() } }

    pub fn add(&mut self, it: LegacyItem) -> Result<(), LgCode> {
        for x in self.items.iter() {
            if x.label == it.label { return Err(LgCode::NotInLedger); }
        }
        self.items.push(it);
        Ok(())
    }

    /// 激活清账：预留激活后立 deadline，激活未清账 = 预留债。
    pub fn activate(&mut self, i: usize, now: u64) -> Result<(), LgCode> {
        if i >= self.items.len() { return Err(LgCode::NotInLedger); }
        if !self.items[i].activated { return Err(LgCode::NotActivated); }
        if self.items[i].assessed { return Err(LgCode::Unassessed); }
        self.items[i].assessed = true;
        self.items[i].deadline = now + DEADLINE_TICK;
        Ok(())
    }

    /// 核销：deadline 内清账。
    pub fn settle(&mut self, i: usize, now: u64) -> Result<(), LgCode> {
        if i >= self.items.len() { return Err(LgCode::NotInLedger); }
        if self.items[i].deadline == 0 { return Err(LgCode::NotInLedger); }
        if now > self.items[i].deadline { return Err(LgCode::Overdue); }
        self.items[i].settled = true;
        Ok(())
    }

    /// 预留债断言：激活且评估但未核销且超期 = 债。
    pub fn debts(&self, now: u64) -> Vec<String> {
        let mut v = Vec::new();
        for it in self.items.iter() {
            if it.activated && it.assessed && !it.settled && now > it.deadline && it.deadline > 0 {
                v.push(it.label.clone());
            }
        }
        v
    }

    /// 闭环复验。
    pub fn loop_verified(&self) -> bool {
        self.items.iter().filter(|it| it.assessed).all(|it| it.looped)
    }
    pub fn close_loop(&mut self, i: usize) -> Result<(), LgCode> {
        if i >= self.items.len() { return Err(LgCode::NotInLedger); }
        if !self.items[i].assessed { return Err(LgCode::Unassessed); }
        self.items[i].looped = true;
        Ok(())
    }
}

/// deadline 台账（复用单源）。
pub struct DeadlineLedger {
    pub entries: Vec<(String, u64)>,
}

impl DeadlineLedger {
    pub fn new() -> Self { DeadlineLedger { entries: Vec::new() } }
    pub fn register(&mut self, label: &str, dl: u64) {
        self.entries.push((String::from(label), dl));
    }
    pub fn overdue(&self, now: u64) -> Vec<String> {
        let mut v = Vec::new();
        for e in self.entries.iter() {
            if now > e.1 { v.push(e.0.clone()); }
        }
        v
    }
}

/// 整形器激活核验（F2910）。
pub fn shaper_active_checked(activated: bool, verified: bool) -> Result<(), LgCode> {
    if activated && !verified { return Err(LgCode::NotActivated); }
    Ok(())
}

pub fn run_ves299_checks() -> CheckSet {
    let mut set = CheckSet::new("ves299_s14legacy");
    let mut lb = LegacyBook::new();

    // 1) 遗留清单。
    let _ = lb.add(LegacyItem::new("F2910 整形器", true));
    let _ = lb.add(LegacyItem::new("F2915 断行深化", true));
    let _ = lb.add(LegacyItem::new("旧微排版", false));
    set.add("C4059-01", lb.items.len() == 3, "清单三条");

    // 2) 激活清账立 deadline。
    let _ = lb.activate(0, 100);
    let _ = lb.activate(1, 100);
    set.add("C4059-02", lb.items[0].deadline == 100 + DEADLINE_TICK, "deadline 单源");

    // 3) 未激活清账拒。
    set.add("C4059-03", lb.activate(2, 100) == Err(LgCode::NotActivated), "未激活拒");

    // 4) 重复评估拒。
    set.add("C4059-04", lb.activate(0, 100) == Err(LgCode::Unassessed), "重复评估拒");

    // 5) 核销。
    let _ = lb.settle(0, 5000);
    set.add("C4059-05", lb.items[0].settled, "核销");

    // 6) 超期核销拒。
    set.add("C4059-06", lb.settle(1, 100 + DEADLINE_TICK + 1) == Err(LgCode::Overdue), "超期拒");

    // 7) 预留债扫描。
    set.add("C4059-07", lb.debts(100 + DEADLINE_TICK + 2) == ["F2915 断行深化"], "预留债检出");

    // 8) 台账超期。
    let mut dl = DeadlineLedger::new();
    dl.register("F2915 断行深化", 20_101);
    dl.register("另一件", 999_999);
    set.add("C4059-08", dl.overdue(100_000).len() == 1, "台账超期");

    // 9) 闭环复验。
    set.add("C4059-09", !lb.loop_verified(), "未闭环检出");
    let _ = lb.close_loop(0);
    let _ = lb.close_loop(1);
    set.add("C4059-10", lb.loop_verified(), "闭环过");

    // 11) 未评估闭环拦截。
    set.add("C4059-11", lb.close_loop(2) == Err(LgCode::Unassessed), "未评估拦截");

    // 12) 整形器激活核验。
    set.add("C4059-12", shaper_active_checked(true, true).is_ok()
        && shaper_active_checked(true, false) == Err(LgCode::NotActivated), "激活核验");

    // 13) 三类处置名双射。
    let mut names: Vec<&str> = Vec::new();
    for d in Disposition::ALL.iter() { names.push(d.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4059-13", !ndup, "处置名双射");

    // 14) 重复条目拒。
    set.add("C4059-14", lb.add(LegacyItem::new("F2910 整形器", true)) == Err(LgCode::NotInLedger), "重复条目拒");

    // 15) 段位与 tick。
    set.add("C4059-15", SEG_BASE == 0x8640 && DEADLINE_TICK == 20_000, "段位与 tick");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_settle() {
        let mut lb = LegacyBook::new();
        let _ = lb.add(LegacyItem::new("旧件", true));
        let _ = lb.activate(0, 0);
        assert!(lb.settle(0, DEADLINE_TICK).is_ok());
        assert!(lb.debts(DEADLINE_TICK + 1).is_empty());
    }
}
