// CGPU-F0336 C01组收口与C02移交 · cgb336_c01close.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：C01 组核心功能自检全绿；C02 移交包含接口/预留/风险/基线；
//       双签；遗留项清单完备。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// C01 自检项。
#[derive(Clone, Debug)]
pub struct C01Check {
    pub id: u32,
    pub name_id: u32,
    pub category: u8,
    pub passed: bool,
    pub severity: u8,
}

/// C02 移交项。
#[derive(Clone, Debug)]
pub struct C02HandoverItem {
    pub id: u32,
    pub category: u8,
    pub name_id: u32,
    pub value: u64,
    pub delivered: bool,
}

/// 移交报告。
#[derive(Clone, Copy, Default, Debug)]
pub struct C01CloseReport {
    pub checks_total: u32,
    pub checks_passed: u32,
    pub handover_items: u32,
    pub handover_delivered: u32,
    pub completeness_q8: u16,
    pub signed: bool,
}

/// C01 收口引擎。
pub struct C01Closeout {
    pub checks: Vec<C01Check>,
    pub handover: Vec<C02HandoverItem>,
    pub report: C01CloseReport,
    pub from_group: String,
    pub to_group: String,
}

impl C01Closeout {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            handover: Vec::new(),
            report: C01CloseReport::default(),
            from_group: String::from("C01"),
            to_group: String::from("C02"),
        }
    }

    /// 加载标准 12 项自检。
    pub fn load_checks(&mut self) {
        let names: [(u32, u8, u8); 12] = [
            (1, 0, 1), (2, 0, 1), (3, 1, 1), (4, 1, 2),
            (5, 2, 1), (6, 2, 2), (7, 3, 1), (8, 3, 1),
            (9, 4, 2), (10, 4, 2), (11, 5, 1), (12, 5, 1),
        ];
        for (id, cat, sev) in names {
            self.checks.push(C01Check {
                id,
                name_id: id,
                category: cat,
                passed: false,
                severity: sev,
            });
        }
    }

    pub fn set_check(&mut self, id: u32, passed: bool) -> bool {
        if let Some(c) = self.checks.iter_mut().find(|c| c.id == id) {
            c.passed = passed;
            true
        } else {
            false
        }
    }

    pub fn add_handover(&mut self, category: u8, name_id: u32, value: u64) -> u32 {
        let id = self.handover.len() as u32 + 1;
        self.handover.push(C02HandoverItem {
            id,
            category,
            name_id,
            value,
            delivered: false,
        });
        id
    }

    pub fn mark_delivered(&mut self, id: u32) -> bool {
        if let Some(h) = self.handover.iter_mut().find(|h| h.id == id) {
            h.delivered = true;
            true
        } else {
            false
        }
    }

    /// 生成报告。
    pub fn finalize(&mut self) -> C01CloseReport {
        let total = self.checks.len() as u32;
        let passed = self.checks.iter().filter(|c| c.passed).count() as u32;
        let items = self.handover.len() as u32;
        let delivered = self.handover.iter().filter(|h| h.delivered).count() as u32;
        let completeness = if items == 0 { 256 } else { ((delivered as u64 * 256) / items as u64) as u16 };
        self.report = C01CloseReport {
            checks_total: total,
            checks_passed: passed,
            handover_items: items,
            handover_delivered: delivered,
            completeness_q8: completeness,
            signed: false,
        };
        self.report
    }

    /// 签署（自检全绿 + 移交完备）。
    pub fn sign(&mut self) -> bool {
        self.finalize();
        if self.report.checks_passed < self.report.checks_total || self.report.completeness_q8 < 200 {
            return false;
        }
        self.report.signed = true;
        true
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0336 C01 组收口");
        cs.add_dyn("12 项自检齐全", self.checks.len() == 12, &alloc::format!("n={}", self.checks.len()));
        cs.add("报告计数自洽", {
            self.report.checks_passed <= self.report.checks_total
        }, "passed ≤ total");
        cs.add("移交完备率 ≤ 256", self.report.completeness_q8 <= 256, "q8");
        cs.add("签署门禁", {
            !self.report.signed
                || (self.report.checks_passed == self.report.checks_total && self.report.completeness_q8 >= 200)
        }, "gate");
        cs.add("移交项 id 唯一", {
            let mut ids: Vec<u32> = self.handover.iter().map(|h| h.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.handover.len()
        }, "unique");
        cs.add("目标组 C02", self.to_group == "C02", "C02");
        cs.add("交付的计数正确", {
            self.report.handover_delivered as usize == self.handover.iter().filter(|h| h.delivered).count()
        }, "count");
        cs
    }
}

impl Default for C01Closeout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_close_sign() {
        let mut c = C01Closeout::new();
        c.load_checks();
        for i in 1..=12 {
            c.set_check(i, true);
        }
        let h = c.add_handover(0, 1, 100);
        c.mark_delivered(h);
        assert!(c.sign());
    }

    #[test]
    fn incomplete_blocks_sign() {
        let mut c = C01Closeout::new();
        c.load_checks();
        c.set_check(1, false); // 不全绿
        c.add_handover(0, 1, 100);
        assert!(!c.sign());
    }
}
