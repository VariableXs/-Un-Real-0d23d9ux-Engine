//! VE-F4274 · 治理测试（U04 治理组）
//! 职责：治理面测试四块（流程状态机/分级逻辑/闸门挂钩/自动化策略）+
//! 测试法三件（状态机模型测试/分级等价类/闸门契约测试）+ 版本戳断言绑定
//! + 漏测等价类审计 + 套件失败阻断治理发版。
//! 段位：0x2840（续表 idx508）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2840;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GtCode {
    /// 测试与规则漂移（版本戳断言拦截）。
    VerDrift = SEG_BASE,
    /// 漏测分级（等价类补全）。
    ClassMiss = SEG_BASE + 0x01,
    /// 套件失败（阻断发版）。
    SuiteFail = SEG_BASE + 0x02,
}

/// 测试对象四块。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestTarget {
    FlowSm = 0,
    Grading = 1,
    GateHook = 2,
    AutoPolicy = 3,
}
pub const TARGET_N: usize = 4;

/// 测试用例（绑规则版本戳）。
#[derive(Clone, Copy, Debug)]
pub struct GtCase {
    pub target: TestTarget,
    pub bound_ver: u32, // 绑定的规则版本戳
    pub pass: bool,
}

/// 分级等价类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GradeClass {
    Green = 0,
    Yellow = 1,
    Red = 2,
}
pub const CLASS_N: usize = 3;

/// 治理测试器。
pub struct GovTest {
    pub cases: Vec<GtCase>,
    /// 规则当前版本戳。
    pub rule_ver: u32,
    /// 等价类覆盖位图。
    pub classes_hit: [bool; CLASS_N],
    pub suite_failed: bool,
}

impl GovTest {
    pub fn new(rule_ver: u32) -> Self {
        GovTest { cases: Vec::new(), rule_ver, classes_hit: [false; CLASS_N], suite_failed: false }
    }

    /// 用例登记：版本戳不符→拦截（漂移）。
    pub fn add_case(&mut self, c: GtCase) -> Result<(), GtCode> {
        if c.bound_ver != self.rule_ver {
            return Err(GtCode::VerDrift);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 等价类标记。
    pub fn mark_class(&mut self, c: GradeClass) {
        self.classes_hit[c as usize] = true;
    }

    /// 等价类补全审计：未覆盖类返回列表。
    pub fn class_gaps(&self) -> Vec<u8> {
        (0..CLASS_N).filter(|c| !self.classes_hit[*c]).map(|c| c as u8).collect()
    }

    /// 套件运行：任一失败→阻断发版。
    pub fn run_suite(&mut self) -> Result<usize, GtCode> {
        let n = self.cases.len();
        if self.cases.iter().any(|c| !c.pass) {
            self.suite_failed = true;
            return Err(GtCode::SuiteFail);
        }
        Ok(n)
    }

    /// 发版闸：套件失败即阻断。
    pub fn release_ok(&self) -> bool {
        !self.suite_failed
    }

    /// 规则演进后：旧版本用例全部失效（须重新绑定）。
    pub fn stale_cases(&self) -> usize {
        self.cases.iter().filter(|c| c.bound_ver != self.rule_ver).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C508-01..12。
pub fn run_ves508_checks() -> CheckSet {
    let mut set = CheckSet::new("ves508");

    let mut gt = GovTest::new(7);
    // 版本戳绑定
    for t in [TestTarget::FlowSm, TestTarget::Grading, TestTarget::GateHook, TestTarget::AutoPolicy] {
        let _ = gt.add_case(GtCase { target: t, bound_ver: 7, pass: true });
    }
    chk(&mut set, "C508-01", gt.cases.len() == 4, "四块测试对象在册");
    chk(&mut set, "C508-02", gt.add_case(GtCase { target: TestTarget::FlowSm, bound_ver: 6, pass: true }) == Err(GtCode::VerDrift), "旧版本戳用例拦截（漂移）");
    chk(&mut set, "C508-03", gt.cases.len() == 4, "拦截不入册");

    // 等价类
    gt.mark_class(GradeClass::Green);
    gt.mark_class(GradeClass::Yellow);
    let gaps = gt.class_gaps();
    chk(&mut set, "C508-04", gaps == alloc::vec![2], "漏测分级=红类（补全审计）");
    gt.mark_class(GradeClass::Red);
    chk(&mut set, "C508-05", gt.class_gaps().is_empty(), "三级等价类补全");

    // 套件
    chk(&mut set, "C508-06", gt.run_suite() == Ok(4), "全绿套件通过");
    chk(&mut set, "C508-07", gt.release_ok(), "发版闸放行");

    gt.add_case(GtCase { target: TestTarget::FlowSm, bound_ver: 7, pass: false }).unwrap_or(());
    chk(&mut set, "C508-08", gt.run_suite() == Err(GtCode::SuiteFail), "失败用例→套件红");
    chk(&mut set, "C508-09", !gt.release_ok(), "套件失败阻断治理发版");

    // 版本演进
    gt.rule_ver = 8;
    chk(&mut set, "C508-10", gt.stale_cases() == 5, "演进后 5 例全部失效待重绑");
    let mut gt2 = GovTest::new(1);
    chk(&mut set, "C508-11", gt2.run_suite().is_ok() && gt2.class_gaps().len() == 3, "空册语义（无失败+全类未覆盖）");
    chk(&mut set, "C508-12", gt2.release_ok(), "空套件不阻断（保守放行）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_test_suite() {
        let mut gt = GovTest::new(1);
        assert!(gt.add_case(GtCase { target: TestTarget::Grading, bound_ver: 1, pass: true }).is_ok());
        assert_eq!(gt.add_case(GtCase { target: TestTarget::Grading, bound_ver: 0, pass: true }), Err(GtCode::VerDrift));
        assert_eq!(gt.run_suite(), Ok(1));
        gt.mark_class(GradeClass::Red);
        assert_eq!(gt.class_gaps().len(), 2);
    }
}
