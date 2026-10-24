//! ves279_s12legacy — VE-F4039 · RTL 预留清账
//! T01/T02 预留项 + 旧 RTL 简化实现清退（退役 deadline 复用红线单源——简化实现不退役=双实现分叉断言）+ 三类处置复述 + 闭环复验。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x74C0;

pub const RETIRE_DEADLINE_TICK: u64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum LegacyCode {
    Ok = 0,
    Unassessed = SEG_BASE + 0x01,
    RetireOverdue = SEG_BASE + 0x02,
    DualImplement = SEG_BASE + 0x03,
    LoopMissing = SEG_BASE + 0x04,
    NotInLedger = SEG_BASE + 0x05,
}

/// 三类处置。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disposition { Retire = 0, Merge = 1, Keep = 2 }

impl Disposition {
    pub const ALL: [Disposition; 3] = [Disposition::Retire, Disposition::Merge, Disposition::Keep];
    pub fn name(self) -> &'static str {
        match self { Disposition::Retire => "清退", Disposition::Merge => "合并", Disposition::Keep => "保留" }
    }
}

/// 遗留条目：预留项或旧简化实现。
#[derive(Clone)]
pub struct LegacyItem {
    pub label: String,
    pub from_t01: bool,
    pub is_old_simplified: bool,
    pub assessed: bool,
    pub disposition: Option<Disposition>,
    /// 退役 deadline（tick）；非清退项为 0。
    pub retire_deadline: u64,
    pub retired: bool,
    pub looped: bool,
}

impl LegacyItem {
    pub fn new(label: &str, from_t01: bool, is_old: bool) -> Self {
        LegacyItem {
            label: String::from(label),
            from_t01,
            is_old_simplified: is_old,
            assessed: false,
            disposition: None,
            retire_deadline: 0,
            retired: false,
            looped: false,
        }
    }
}

pub struct LegacyBook {
    pub items: Vec<LegacyItem>,
}

impl LegacyBook {
    pub fn new() -> Self { LegacyBook { items: Vec::new() } }

    pub fn add(&mut self, it: LegacyItem) -> Result<(), LegacyCode> {
        for x in self.items.iter() {
            if x.label == it.label { return Err(LegacyCode::NotInLedger); }
        }
        self.items.push(it);
        Ok(())
    }

    /// 评估：未评估项激活处置 → 拦截。
    pub fn assess(&mut self, i: usize, d: Disposition, now: u64) -> Result<(), LegacyCode> {
        if i >= self.items.len() { return Err(LegacyCode::NotInLedger); }
        if self.items[i].assessed { return Err(LegacyCode::Unassessed); }
        self.items[i].assessed = true;
        self.items[i].disposition = Some(d);
        if d == Disposition::Retire {
            self.items[i].retire_deadline = now + RETIRE_DEADLINE_TICK;
        }
        Ok(())
    }

    /// 退役执行：超 deadline → 拒（强制先升级）。
    pub fn retire(&mut self, i: usize, now: u64) -> Result<(), LegacyCode> {
        if i >= self.items.len() { return Err(LegacyCode::NotInLedger); }
        let it = &mut self.items[i];
        if it.retire_deadline == 0 { return Err(LegacyCode::NotInLedger); }
        if now > it.retire_deadline { return Err(LegacyCode::RetireOverdue); }
        it.retired = true;
        Ok(())
    }

    /// 双实现分叉断言：清退处置的旧简化实现必须已退役。
    pub fn dual_implement_free(&self) -> bool {
        for it in self.items.iter() {
            if it.is_old_simplified && it.disposition == Some(Disposition::Retire) && !it.retired {
                return false;
            }
        }
        true
    }

    /// 闭环复验：已处置项全部闭环。
    pub fn loop_verified(&self) -> bool {
        self.items.iter().filter(|it| it.assessed).all(|it| it.looped)
    }

    pub fn close_loop(&mut self, i: usize) -> Result<(), LegacyCode> {
        if i >= self.items.len() { return Err(LegacyCode::NotInLedger); }
        if !self.items[i].assessed { return Err(LegacyCode::Unassessed); }
        self.items[i].looped = true;
        Ok(())
    }
}

/// 退役台账：deadline 单源（复用 RETIRE_DEADLINE_TICK 红线）。
pub struct RetireLedger {
    pub entries: Vec<(String, u64)>,
}

impl RetireLedger {
    pub fn new() -> Self { RetireLedger { entries: Vec::new() } }
    pub fn register(&mut self, label: &str, deadline: u64) {
        self.entries.push((String::from(label), deadline));
    }
    pub fn overdue(&self, now: u64) -> Vec<String> {
        let mut v = Vec::new();
        for e in self.entries.iter() {
            if now > e.1 { v.push(e.0.clone()); }
        }
        v
    }
    /// 台账 deadline 对齐单源 tick（含基准项零点）。
    pub fn aligned(&self) -> bool {
        self.entries.iter().all(|e| e.1 % RETIRE_DEADLINE_TICK == 0 || e.1 % RETIRE_DEADLINE_TICK == 100)
    }
}

pub fn run_ves279_checks() -> CheckSet {
    let mut set = CheckSet::new("ves279_s12legacy");
    let mut lb = LegacyBook::new();

    // 1) 遗留清单：T01/T02 预留 + 旧简化实现。
    let _ = lb.add(LegacyItem::new("T01 i18n 预留", true, false));
    let _ = lb.add(LegacyItem::new("T02 RTL 预留", false, false));
    let _ = lb.add(LegacyItem::new("旧简化 BiDi", false, true));
    set.add("C4039-01", lb.items.len() == 3, "遗留清单三条");

    // 2) 未评估激活 → 拦截。
    set.add("C4039-02", lb.close_loop(0) == Err(LegacyCode::Unassessed), "未评估拦截");

    // 3) 三类处置评估。
    let _ = lb.assess(0, Disposition::Merge, 100);
    let _ = lb.assess(1, Disposition::Keep, 100);
    let _ = lb.assess(2, Disposition::Retire, 100);
    set.add("C4039-03", lb.items.iter().all(|it| it.assessed), "三类处置评估");

    // 4) 重复评估拒。
    set.add("C4039-04", lb.assess(0, Disposition::Keep, 100) == Err(LegacyCode::Unassessed), "重复评估拒");

    // 5) 清退 deadline = now + 单源 tick。
    set.add("C4039-05", lb.items[2].retire_deadline == 100 + RETIRE_DEADLINE_TICK, "退役 deadline 单源");

    // 6) 双实现分叉：未退役简化实现检出。
    set.add("C4039-06", !lb.dual_implement_free(), "双实现分叉检出");

    // 7) 退役执行 → 分叉消除。
    let _ = lb.retire(2, 5000);
    set.add("C4039-07", lb.items[2].retired && lb.dual_implement_free(), "退役消除分叉");

    // 8) 超期退役拒。
    let mut lb2 = LegacyBook::new();
    let _ = lb2.add(LegacyItem::new("过期旧实现", false, true));
    let _ = lb2.assess(0, Disposition::Retire, 100);
    set.add("C4039-08", lb2.retire(0, 100 + RETIRE_DEADLINE_TICK + 1) == Err(LegacyCode::RetireOverdue), "超期退役拒");

    // 9) 台账超期扫描。
    let mut rl = RetireLedger::new();
    rl.register("旧简化 BiDi", 200);
    rl.register("另一旧件", 90000);
    let od = rl.overdue(1000);
    set.add("C4039-09", od.len() == 1 && od[0].contains("BiDi"), "台账超期扫描");

    // 10) 闭环复验：未闭环检出。
    set.add("C4039-10", !lb.loop_verified(), "未闭环检出");

    // 11) 逐项闭环 → 复验过。
    for i in 0..3usize { let _ = lb.close_loop(i); }
    set.add("C4039-11", lb.loop_verified(), "闭环复验过");

    // 12) 非清退项无 deadline。
    set.add("C4039-12", lb.items[0].retire_deadline == 0 && lb.items[1].retire_deadline == 0, "非清退无 deadline");

    // 13) 三类处置名双射。
    let mut names: Vec<&str> = Vec::new();
    for d in Disposition::ALL.iter() { names.push(d.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4039-13", !ndup, "三类处置名双射");

    // 14) 重复条目拒。
    set.add("C4039-14", lb.add(LegacyItem::new("T01 i18n 预留", true, false)) == Err(LegacyCode::NotInLedger), "重复条目拒");

    // 15) 段位自查。
    set.add("C4039-15", SEG_BASE == 0x74C0 && RETIRE_DEADLINE_TICK == 10_000, "段位与 deadline 单源");

    // 16) 台账对齐。
    set.add("C4039-16", rl.aligned(), "台账对齐 tick");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retire_flow() {
        let mut lb = LegacyBook::new();
        let _ = lb.add(LegacyItem::new("旧件", false, true));
        let _ = lb.assess(0, Disposition::Retire, 0);
        assert!(!lb.dual_implement_free());
        assert!(lb.retire(0, RETIRE_DEADLINE_TICK).is_ok());
        assert!(lb.dual_implement_free());
    }
}
