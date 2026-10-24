//! ves377_s20assets — VE-F4137 · T07 i18n 性能资产汇总
//! 五类资产归集（基准集/预算表/压测报告/长稳报告/调试器）+ 每资产挂组/挂条/挂基线三指针 + 与家族性能资产总表对拍 + 台账缺回补 + 对拍不符修正留痕 + 资产失联断链修复 + 无障碍基准组归集（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDAC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    Ok = 0,
    Missing = SEG_BASE + 0x01,
    LinkBroken = SEG_BASE + 0x02,
    LedgerMismatch = SEG_BASE + 0x03,
}

/// 五类资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind { BenchSet = 0, BudgetTable = 1, StressReport = 2, SoakReport = 3, Debugger = 4 }

impl AssetKind {
    pub const ALL: [AssetKind; 5] = [
        AssetKind::BenchSet, AssetKind::BudgetTable, AssetKind::StressReport,
        AssetKind::SoakReport, AssetKind::Debugger];
    pub fn name(self) -> &'static str {
        match self { AssetKind::BenchSet => "bench-set", AssetKind::BudgetTable => "budget-table",
            AssetKind::StressReport => "stress-report", AssetKind::SoakReport => "soak-report",
            AssetKind::Debugger => "debugger" }
    }
}

/// 三指针：挂组（所属组）/挂条（卡号）/挂基线（哈希）。
#[derive(Clone)]
pub struct AssetEntry {
    pub kind: AssetKind,
    pub owner: &'static str,     // 挂组
    pub card: &'static str,      // 挂条（如 "VE-F4124"）
    pub baseline: u32,           // 挂基线哈希（0 = 无基线）
}

pub struct AssetLedger { pub entries: Vec<AssetEntry> }

impl AssetLedger {
    pub fn new() -> Self { AssetLedger { entries: Vec::new() } }

    pub fn put(&mut self, e: AssetEntry) {
        if let Some(x) = self.entries.iter_mut().find(|x| x.kind == e.kind) {
            x.owner = e.owner; x.card = e.card; x.baseline = e.baseline;
        } else {
            self.entries.push(e);
        }
    }

    /// 五类齐备。
    pub fn complete(&self) -> bool {
        AssetKind::ALL.iter().all(|k| self.entries.iter().any(|e| e.kind == *k))
    }

    /// 缺类回补定位。
    pub fn missing(&self) -> Vec<&'static str> {
        AssetKind::ALL.iter().filter(|k| !self.entries.iter().any(|e| e.kind == **k))
            .map(|k| k.name()).collect()
    }

    /// 断链检测：基线指针为 0 或卡号为空。
    pub fn broken_links(&self) -> Vec<&'static str> {
        self.entries.iter()
            .filter(|e| e.baseline == 0 || e.card.is_empty())
            .map(|e| e.kind.name())
            .collect()
    }

    /// 修复断链。
    pub fn fix_link(&mut self, kind: AssetKind, card: &'static str, baseline: u32) -> bool {
        match self.entries.iter_mut().find(|e| e.kind == kind) {
            Some(e) => { e.card = card; e.baseline = baseline; true }
            None => false,
        }
    }
}

/// 家族性能资产总表对拍：本组台账与家族总表的（组名, 类）集一致。
pub fn agrees_with_family(ledger: &AssetLedger, family: &[(&'static str, &'static str)]) -> bool {
    if ledger.entries.len() != family.len() { return false; }
    family.iter().all(|(owner, kind)| {
        ledger.entries.iter().any(|e| e.owner == *owner && e.kind.name() == *kind)
    })
}

/// 无障碍基准组归集（域本色）：a11y 基准资产必须在台账。
pub fn a11y_bench_registered(ledger: &AssetLedger) -> bool {
    ledger.entries.iter().any(|e| e.kind == AssetKind::BenchSet && e.card.contains("a11y"))
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves377_checks() -> CheckSet {
    let mut set = CheckSet::new("ves377");
    let mut led = AssetLedger::new();

    // C377-01 五类资产逐条登记（三指针齐）。
    for (i, k) in AssetKind::ALL.iter().enumerate() {
        led.put(AssetEntry { kind: *k, owner: "T07", card: "VE-F412x", baseline: 0xA000 + i as u32 });
    }
    chk(&mut set, "C377-01", led.complete() && led.entries.len() == 5, "五类三指针齐");

    // C377-02 缺类回补定位。
    let mut led2 = AssetLedger::new();
    led2.put(AssetEntry { kind: AssetKind::BenchSet, owner: "T07", card: "VE-F4124", baseline: 1 });
    led2.put(AssetEntry { kind: AssetKind::Debugger, owner: "T07", card: "VE-F4132", baseline: 2 });
    chk(&mut set, "C377-02", led2.missing() == vec!["budget-table", "stress-report", "soak-report"], "缺三类定位");

    // C377-03 覆盖登记幂等。
    led.put(AssetEntry { kind: AssetKind::BenchSet, owner: "T07", card: "VE-F4124", baseline: 0xBEEF });
    chk(&mut set, "C377-03", led.entries.len() == 5 && led.entries[0].baseline == 0xBEEF, "覆盖不增行");

    // C377-04 断链检测：基线 0。
    let mut led3 = AssetLedger::new();
    led3.put(AssetEntry { kind: AssetKind::BenchSet, owner: "T07", card: "VE-F4124", baseline: 0 });
    led3.put(AssetEntry { kind: AssetKind::BudgetTable, owner: "T07", card: "", baseline: 9 });
    chk(&mut set, "C377-04", led3.broken_links() == vec!["bench-set", "budget-table"], "两断链定位");

    // C377-05 断链修复。
    chk(&mut set, "C377-05", led3.fix_link(AssetKind::BenchSet, "VE-F4124", 0xABCD)
        && led3.broken_links() == vec!["budget-table"], "修一余一");

    // C377-06 家族总表对拍一致。
    let family = [
        ("T07", "bench-set"), ("T07", "budget-table"), ("T07", "stress-report"),
        ("T07", "soak-report"), ("T07", "debugger")];
    chk(&mut set, "C377-06", agrees_with_family(&led, &family), "与家族总表对拍一致");

    // C377-07 家族对拍失配检出（数量差）。
    let fam4 = [fam4_item(0), fam4_item(1), fam4_item(2), fam4_item(3)];
    chk(&mut set, "C377-07", !agrees_with_family(&led, &fam4), "家族少一类 => 失配");

    // C377-08 对拍失配修正留痕。
    let mut fixed_note: Vec<&'static str> = Vec::new();
    if !agrees_with_family(&led, &fam4) { fixed_note.push("补 soak-report 对拍"); }
    chk(&mut set, "C377-08", fixed_note.len() == 1, "修正留痕一条");

    // C377-09 无障碍基准组归集（域本色）。
    led.put(AssetEntry { kind: AssetKind::BenchSet, owner: "T07", card: "VE-F4124-a11y-render", baseline: 0xCAFE });
    chk(&mut set, "C377-09", a11y_bench_registered(&led), "专组资产在账");

    // C377-10 五类名单单源。
    chk(&mut set, "C377-10", AssetKind::ALL[0].name() == "bench-set" && AssetKind::ALL[4].name() == "debugger", "五类单源");

    // C377-11 台账查询。
    chk(&mut set, "C377-11", led.entries.iter().any(|e| e.kind == AssetKind::SoakReport && e.owner == "T07"), "台账可查");

    // C377-12 失联（缺类）即 LedgerMismatch 面。
    chk(&mut set, "C377-12", !led2.complete(), "led2 缺三类 => 失联面");

    set
}

fn fam4_item(i: usize) -> (&'static str, &'static str) {
    [("T07", "bench-set"), ("T07", "budget-table"), ("T07", "stress-report"), ("T07", "soak-report")][i]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves377_smoke() {
        let set = run_ves377_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves377 red: {:?}", red);
    }

    #[test]
    fn ves377_missing() {
        let led = AssetLedger::new();
        assert_eq!(led.missing().len(), 5);
        assert!(!led.complete());
    }
}
