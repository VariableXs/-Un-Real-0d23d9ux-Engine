//! VE-F4255 · 验证示例（U03 一致性验证平台）
//! 职责：六类验证各一完整示例（目标+规则+发现项+报告四件套）+ 误报正反例
//! 对照 + 示例可执行跑通即验收 + 示例跑挂=示例即缺陷 + 无障碍示范（域本色）。
//! 段位：0x1DE0（续表 idx489）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1DE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 示例跑挂（示例即缺陷）。
    ExampleBroken = SEG_BASE,
    /// 误报反例未复现（引擎缺陷立案）。
    FpNotRepro = SEG_BASE + 0x01,
    /// 四件套残缺。
    KitBroken = SEG_BASE + 0x02,
    /// 与引擎失配（修订）。
    EngineMismatch = SEG_BASE + 0x03,
}

/// 示例四件套。
#[derive(Clone, Debug)]
pub struct ExampleKit {
    pub kind: u8,      // 六类验证 0..5
    pub target: u16,   // 目标
    pub rule: u16,     // 规则
    pub expect_finding: bool, // 预期发现项
    pub report_ref: u16,      // 报告引用
    pub a11y_demo: bool,      // 无障碍示范（域本色）
}

impl ExampleKit {
    pub fn kit_ok(&self) -> bool {
        self.target != 0 && self.rule != 0 && self.report_ref != 0
    }
}

/// 示例库。
pub struct ExampleBook {
    pub kits: Vec<ExampleKit>,
    /// 正反例对照：(规则, 正例触发, 反例不触发)。
    pub fp_pairs: Vec<(u16, bool, bool)>,
    /// 引擎模拟器：规则→是否检出。
    pub engine: Vec<(u16, bool)>,
    /// 验收账。
    pub accepted: u32,
    pub defects: u32,
}

impl ExampleBook {
    pub fn new() -> Self {
        ExampleBook { kits: Vec::new(), fp_pairs: Vec::new(), engine: Vec::new(), accepted: 0, defects: 0 }
    }

    /// 引擎行为登记（示例对拍基准）。
    pub fn register_engine(&mut self, rule: u16, detects: bool) {
        if !self.engine.iter().any(|(r, _)| *r == rule) {
            self.engine.push((rule, detects));
        }
    }

    /// 示例登记。
    pub fn add(&mut self, k: ExampleKit) -> Result<(), ExCode> {
        if !k.kit_ok() {
            return Err(ExCode::KitBroken);
        }
        self.kits.push(k);
        Ok(())
    }

    /// 执行验收：跑通且与引擎一致→accepted；跑挂→示例即缺陷。
    pub fn execute(&mut self, kind: u8) -> Result<(), ExCode> {
        let k = self.kits.iter().find(|k| k.kind == kind).ok_or(ExCode::KitBroken)?;
        let engine_says = self.engine.iter().find(|(r, _)| *r == k.rule).map(|(_, d)| *d);
        match engine_says {
            None => Err(ExCode::EngineMismatch),
            Some(d) if d == k.expect_finding => {
                self.accepted += 1;
                Ok(())
            }
            Some(_) => {
                self.defects += 1;
                Err(ExCode::ExampleBroken)
            }
        }
    }

    /// 误报正反例对照：正例须触发、反例须不触发；反例被触发=误报未复现→引擎缺陷。
    pub fn check_fp_pair(&mut self, rule: u16, pos_hits: bool, neg_hits: bool) -> Result<(), ExCode> {
        self.fp_pairs.push((rule, pos_hits, neg_hits));
        if !pos_hits {
            return Err(ExCode::FpNotRepro);
        }
        if neg_hits {
            self.defects += 1; // 误报复现=引擎缺陷立案
            return Err(ExCode::FpNotRepro);
        }
        Ok(())
    }

    /// 六类覆盖。
    pub fn all_kinds(&self) -> bool {
        (0..6u8).all(|k| self.kits.iter().any(|x| x.kind == k))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C489-01..12。
pub fn run_ves489_checks() -> CheckSet {
    let mut set = CheckSet::new("ves489");

    let mut eb = ExampleBook::new();
    for (k, rule, det, find) in [
        (0u8, 10u16, true, true),   // 静态
        (1, 20, true, false),       // 动态
        (2, 30, true, true),        // 视觉
        (3, 40, false, false),      // 交互
        (4, 50, true, true),        // 性能
        (5, 60, true, false),       // 无障碍（示范）
    ] {
        eb.register_engine(rule, det);
        let r = eb.add(ExampleKit { kind: k, target: 100 + k as u16, rule, expect_finding: find, report_ref: 9, a11y_demo: k == 5 });
        debug_assert!(r.is_ok());
    }
    chk(&mut set, "C489-01", eb.all_kinds(), "六类示例齐备");
    chk(&mut set, "C489-02", eb.kits.iter().filter(|k| k.a11y_demo).count() == 1, "无障碍示范唯一（域本色）");

    chk(&mut set, "C489-03", eb.add(ExampleKit { kind: 9, target: 0, rule: 1, expect_finding: false, report_ref: 0, a11y_demo: false }) == Err(ExCode::KitBroken), "四件套残缺拒收");

    for k in 0..6u8 {
        let _ = eb.execute(k);
    }
    chk(&mut set, "C489-04", eb.accepted == 6, "六例全部跑通即验收");
    chk(&mut set, "C489-05", eb.defects == 0, "零缺陷");

    // 引擎失配
    eb.register_engine(70, false);
    let _ = eb.add(ExampleKit { kind: 6, target: 5, rule: 70, expect_finding: true, report_ref: 9, a11y_demo: false });
    chk(&mut set, "C489-06", eb.execute(6) == Err(ExCode::EngineMismatch), "示例与引擎失配→修订");

    // 示例即缺陷
    eb.register_engine(80, false);
    let _ = eb.add(ExampleKit { kind: 7, target: 6, rule: 80, expect_finding: true, report_ref: 9, a11y_demo: false });
    chk(&mut set, "C489-07", eb.execute(7) == Err(ExCode::ExampleBroken), "示例跑挂=示例即缺陷");
    chk(&mut set, "C489-08", eb.defects == 1, "缺陷账累计");

    // 误报正反例
    chk(&mut set, "C489-09", eb.check_fp_pair(40, true, false).is_ok(), "正例触发+反例不触发=对照通过");
    chk(&mut set, "C489-10", eb.check_fp_pair(41, true, true) == Err(ExCode::FpNotRepro), "反例被触发→误报未复现→引擎缺陷");
    chk(&mut set, "C489-11", eb.check_fp_pair(42, false, false) == Err(ExCode::FpNotRepro), "正例不触发→对照失败");
    chk(&mut set, "C489-12", eb.defects == 2 && eb.fp_pairs.len() == 3, "误报对照账与缺陷账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_acceptance() {
        let mut eb = ExampleBook::new();
        eb.register_engine(1, true);
        assert!(eb.add(ExampleKit { kind: 0, target: 1, rule: 1, expect_finding: true, report_ref: 2, a11y_demo: false }).is_ok());
        assert!(eb.execute(0).is_ok());
        assert!(eb.add(ExampleKit { kind: 1, target: 0, rule: 1, expect_finding: false, report_ref: 2, a11y_demo: false }).is_err());
    }
}
