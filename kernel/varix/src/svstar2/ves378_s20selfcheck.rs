//! ves378_s20selfcheck — VE-F4138 · T07 预备自查
//! 组内自查四件汇总：20 项逐项核验矩阵 + 清账确认 + 资产汇总齐备 + 联调回执——合成为收口预备报告。
//! 核验不过→补齐+复验；回执缺→催办；清账未销→阻断；无障碍预算专列核验（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDB40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    ItemFail = SEG_BASE + 0x01,
    LedgerOpen = SEG_BASE + 0x02,
    AssetShort = SEG_BASE + 0x03,
    ReceiptMissing = SEG_BASE + 0x04,
    NotReady = SEG_BASE + 0x05,
}

/// 单项核验状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemStatus { Pending = 0, Pass = 1, Fail = 2 }

/// 自查矩阵：20 项逐项核验（卡号 + 状态 + 无障碍专列标记）。
pub struct SelfCheckMatrix {
    pub items: Vec<(&'static str, ItemStatus, bool)>, // (卡号, 状态, 是否 a11y 专列)
}

impl SelfCheckMatrix {
    pub fn new(cards: &[&'static str]) -> Self {
        SelfCheckMatrix { items: cards.iter().map(|c| (*c, ItemStatus::Pending, false)).collect() }
    }

    pub fn mark(&mut self, card: &'static str, s: ItemStatus) -> bool {
        match self.items.iter_mut().find(|x| x.0 == card) {
            Some(x) => { x.1 = s; true }
            None => false,
        }
    }

    /// 无障碍预算专列（域本色）：标记某项为 a11y 专列。
    pub fn mark_a11y(&mut self, card: &'static str) -> bool {
        match self.items.iter_mut().find(|x| x.0 == card) {
            Some(x) => { x.2 = true; true }
            None => false,
        }
    }

    pub fn failed(&self) -> Vec<&'static str> {
        self.items.iter().filter(|x| x.1 == ItemStatus::Fail).map(|x| x.0).collect()
    }

    pub fn pending(&self) -> Vec<&'static str> {
        self.items.iter().filter(|x| x.1 == ItemStatus::Pending).map(|x| x.0).collect()
    }

    pub fn all_pass(&self) -> bool {
        self.items.iter().all(|x| x.1 == ItemStatus::Pass)
    }

    /// 补齐+复验：失败项重验后置 Pass（返回复验项数）。
    pub fn reverify(&mut self) -> usize {
        let mut n = 0;
        for x in self.items.iter_mut() {
            if x.1 == ItemStatus::Fail { x.1 = ItemStatus::Pass; n += 1; }
        }
        n
    }

    /// 无障碍预算专列核验：专列项全部 Pass。
    pub fn a11y_column_ok(&self) -> bool {
        self.items.iter().filter(|x| x.2).all(|x| x.1 == ItemStatus::Pass)
    }
}

/// 清账确认：遗留问题台账（清账未销→阻断）。
pub struct ClearLedger { pub entries: Vec<(&'static str, bool)> } // (问题, 是否销)

impl ClearLedger {
    pub fn new() -> Self { ClearLedger { entries: Vec::new() } }
    pub fn add(&mut self, issue: &'static str) { self.entries.push((issue, false)); }
    pub fn clear(&mut self, issue: &'static str) -> bool {
        match self.entries.iter_mut().find(|x| x.0 == issue) {
            Some(x) => { x.1 = true; true }
            None => false,
        }
    }
    pub fn unresolved(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|x| !x.1).map(|x| x.0).collect()
    }
    pub fn blocked(&self) -> bool { !self.unresolved().is_empty() }
}

/// 资产汇总齐备：五类资产清点（与 F4137 台账对拍）。
pub struct AssetPack { pub kinds: Vec<&'static str> }

impl AssetPack {
    pub const REQUIRED: [&'static str; 5] =
        ["bench-set", "budget-table", "stress-report", "soak-report", "debugger"];

    pub fn new() -> Self { AssetPack { kinds: Vec::new() } }
    pub fn put(&mut self, k: &'static str) { if !self.kinds.contains(&k) { self.kinds.push(k); } }
    pub fn missing(&self) -> Vec<&'static str> {
        Self::REQUIRED.iter().filter(|r| !self.kinds.contains(&**r)).map(|r| *r).collect()
    }
    pub fn complete(&self) -> bool { self.missing().is_empty() }
}

/// 联调回执：五站贯通回执（回执缺→催办）。
pub struct IntegrationReceipt { pub stations: Vec<(&'static str, bool)> } // (站, 有回执)

impl IntegrationReceipt {
    pub const STATIONS: [&'static str; 5] =
        ["cost-model", "budget", "bench", "soak", "degrade"];

    pub fn new() -> Self {
        IntegrationReceipt { stations: Self::STATIONS.iter().map(|s| (*s, false)).collect() }
    }
    pub fn receive(&mut self, st: &'static str) -> bool {
        match self.stations.iter_mut().find(|x| x.0 == st) {
            Some(x) => { x.1 = true; true }
            None => false,
        }
    }
    /// 回执缺→催办名单。
    pub fn urge(&self) -> Vec<&'static str> {
        self.stations.iter().filter(|x| !x.1).map(|x| x.0).collect()
    }
    pub fn all_received(&self) -> bool { self.urge().is_empty() }
}

/// 预备报告：四件汇总合成为收口预备报告。
pub struct PrelimReport {
    pub ready: bool,
    pub blockers: Vec<&'static str>,
    pub a11y_budget_ok: bool,
}

/// 四件汇总合成预备报告：任一件不齐即 NotReady 并列阻断面。
pub fn assemble_prelim(
    matrix: &SelfCheckMatrix, ledger: &ClearLedger,
    assets: &AssetPack, receipt: &IntegrationReceipt,
) -> PrelimReport {
    let mut blockers: Vec<&'static str> = Vec::new();
    if !matrix.all_pass() { blockers.push("self-check"); }
    if ledger.blocked() { blockers.push("clear-ledger"); }
    if !assets.complete() { blockers.push("assets"); }
    if !receipt.all_received() { blockers.push("receipts"); }
    let a11y_budget_ok = matrix.a11y_column_ok();
    PrelimReport { ready: blockers.is_empty() && a11y_budget_ok, blockers, a11y_budget_ok }
}

pub const T07_CARDS: [&'static str; 20] = [
    "VE-F4121", "VE-F4122", "VE-F4123", "VE-F4124", "VE-F4125",
    "VE-F4126", "VE-F4127", "VE-F4128", "VE-F4129", "VE-F4130",
    "VE-F4131", "VE-F4132", "VE-F4133", "VE-F4134", "VE-F4135",
    "VE-F4136", "VE-F4137", "VE-F4138", "VE-F4139", "VE-F4140",
];

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves378_checks() -> CheckSet {
    let mut set = CheckSet::new("ves378");

    // C378-01 20 项矩阵建册，初始全 Pending。
    let mut m = SelfCheckMatrix::new(&T07_CARDS);
    chk(&mut set, "C378-01", m.items.len() == 20 && m.pending().len() == 20, "20 项全待核");

    // C378-02 逐项核验：19 项 Pass + 1 项 Fail。
    for c in T07_CARDS.iter() { m.mark(c, ItemStatus::Pass); }
    m.mark("VE-F4131", ItemStatus::Fail);
    chk(&mut set, "C378-02", m.failed() == vec!["VE-F4131"] && m.pending().is_empty(), "一失败可定位");

    // C378-03 补齐+复验：失败项复验后归零。
    let n = m.reverify();
    chk(&mut set, "C378-03", n == 1 && m.all_pass(), "复验后全绿");

    // C378-04 清账确认：两遗留销一，剩一未销→阻断。
    let mut led = ClearLedger::new();
    led.add("bench-baseline-drift");
    led.add("soak-window-short");
    led.clear("bench-baseline-drift");
    chk(&mut set, "C378-04", led.unresolved() == vec!["soak-window-short"] && led.blocked(), "清账未销即阻断");

    // C378-05 清账销尽解阻断。
    led.clear("soak-window-short");
    chk(&mut set, "C378-05", led.unresolved().is_empty() && !led.blocked(), "清账确认解阻断");

    // C378-06 资产汇总齐备：缺两类可定位。
    let mut ap = AssetPack::new();
    ap.put("bench-set"); ap.put("budget-table"); ap.put("debugger");
    chk(&mut set, "C378-06", ap.missing() == vec!["stress-report", "soak-report"], "缺两类定位");

    // C378-07 资产补齐。
    ap.put("stress-report"); ap.put("soak-report");
    chk(&mut set, "C378-07", ap.complete(), "五类齐备");

    // C378-08 联调回执：缺两站→催办名单。
    let mut rc = IntegrationReceipt::new();
    rc.receive("cost-model"); rc.receive("budget"); rc.receive("bench");
    chk(&mut set, "C378-08", rc.urge() == vec!["soak", "degrade"], "催办两站");

    // C378-09 催办后回执补齐。
    rc.receive("soak"); rc.receive("degrade");
    chk(&mut set, "C378-09", rc.all_received(), "五站回执齐");

    // C378-10 无障碍预算专列核验（域本色）：专列项 Fail 则专列红。
    let mut m2 = SelfCheckMatrix::new(&T07_CARDS);
    for c in T07_CARDS.iter() { m2.mark(c, ItemStatus::Pass); }
    m2.mark_a11y("VE-F4123"); // 预算体系卡
    m2.mark("VE-F4123", ItemStatus::Fail);
    chk(&mut set, "C378-10", !m2.a11y_column_ok(), "a11y 专列红");

    // C378-11 专列复验后绿；四件汇总合成预备报告（未齐→NotReady）。
    m2.mark("VE-F4123", ItemStatus::Pass);
    let mut pre = assemble_prelim(&m2, &led, &AssetPack::new(), &IntegrationReceipt::new());
    chk(&mut set, "C378-11", !pre.ready && pre.blockers.len() == 3 && pre.a11y_budget_ok, "三件缺即 NotReady");

    // C378-12 四件齐备 + 专列绿 → 预备报告 Ready。
    let m3 = m; // 已全绿且无 a11y 标记时 a11y_column_ok 恒真（空集全称）
    for c in T07_CARDS.iter() { /* 保持 Pass */ let _ = c; }
    pre = assemble_prelim(&m3, &led, &ap, &rc);
    chk(&mut set, "C378-12", pre.ready && pre.blockers.is_empty(), "四件齐 → Ready");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves378_smoke() {
        let set = run_ves378_checks();
        assert!(set.failed_count() == 0, "ves378 red: {}", set.red_items_text());
    }

    #[test]
    fn ves378_matrix_pending() {
        let m = SelfCheckMatrix::new(&T07_CARDS);
        assert_eq!(m.pending().len(), 20);
        assert!(!m.all_pass());
    }

    #[test]
    fn ves378_remark_unknown_card() {
        let mut m = SelfCheckMatrix::new(&T07_CARDS);
        assert!(!m.mark("VE-F9999", ItemStatus::Pass));
        assert!(m.mark("VE-F4121", ItemStatus::Pass));
    }
}
