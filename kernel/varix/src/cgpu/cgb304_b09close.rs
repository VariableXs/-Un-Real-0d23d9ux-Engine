// CGPU-F0304 B09 组收口与 B10 移交 · cgb304_b09close.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：B09 可视化组 16 项自检全绿；检出率报告；B10 移交包（接口面/预留点/风险清单）；
//       组双签（实现+审核）。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 自检项。
#[derive(Clone, Debug)]
pub struct SelfCheckItem {
    pub id: u32,
    pub name: String,
    pub category: CheckCategory,
    pub passed: bool,
    pub evidence_id: u32,
    pub severity: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckCategory {
    Functional,
    Performance,
    Security,
    Determinism,
    Accessibility,
    Export,
    Api,
    Doc,
}

/// 移交包。
#[derive(Clone, Debug)]
pub struct HandoverPackage {
    pub from_group: String,
    pub to_group: String,
    pub interface_surface: Vec<u32>,
    pub reserved_points: Vec<u32>,
    pub risk_items: Vec<RiskItem>,
    pub signed_by_impl: Option<u32>,
    pub signed_by_review: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct RiskItem {
    pub id: u32,
    pub description_id: u32,
    pub severity: u8,
    pub mitigation_id: u32,
    pub open: bool,
}

/// 检出率报告。
#[derive(Clone, Copy, Default, Debug)]
pub struct DetectionReport {
    pub total_checks: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub critical_failed: u32,
    pub detection_rate_q8: u16,
}

/// B09 收口引擎。
pub struct B09Closeout {
    pub items: Vec<SelfCheckItem>,
    pub handover: HandoverPackage,
    pub report: DetectionReport,
    pub group_id: u32,
}

impl B09Closeout {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            handover: HandoverPackage {
                from_group: String::from("B09"),
                to_group: String::from("B10"),
                interface_surface: Vec::new(),
                reserved_points: Vec::new(),
                risk_items: Vec::new(),
                signed_by_impl: None,
                signed_by_review: None,
            },
            report: DetectionReport::default(),
            group_id: 9,
        }
    }

    /// 注册 16 项标准自检。
    pub fn load_standard_checks(&mut self) {
        let specs: [(u32, &str, CheckCategory, u8); 16] = [
            (1, "布局确定性", CheckCategory::Determinism, 1),
            (2, "渲染指令有序", CheckCategory::Functional, 1),
            (3, "命中测试确定性", CheckCategory::Functional, 1),
            (4, "交互契约零违反", CheckCategory::Functional, 1),
            (5, "帧预算达标", CheckCategory::Performance, 1),
            (6, "LOD 降级正确", CheckCategory::Performance, 2),
            (7, "导出字节确定性", CheckCategory::Export, 1),
            (8, "导出校验和正确", CheckCategory::Export, 1),
            (9, "对比度达标", CheckCategory::Accessibility, 2),
            (10, "焦点序完整", CheckCategory::Accessibility, 2),
            (11, "fuzz 零崩溃", CheckCategory::Security, 1),
            (12, "畸形输入被拒", CheckCategory::Security, 1),
            (13, "API 签名冻结", CheckCategory::Api, 1),
            (14, "文档覆盖率", CheckCategory::Doc, 2),
            (15, "diff 统计完备", CheckCategory::Functional, 2),
            (16, "重放步进确定性", CheckCategory::Determinism, 1),
        ];
        for (id, name, cat, sev) in specs {
            self.items.push(SelfCheckItem {
                id,
                name: String::from(name),
                category: cat,
                passed: false,
                evidence_id: id,
                severity: sev,
            });
        }
    }

    /// 标记通过/失败。
    pub fn set_result(&mut self, id: u32, passed: bool) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.passed = passed;
            true
        } else {
            false
        }
    }

    /// 生成检出率报告。
    pub fn generate_report(&mut self) -> DetectionReport {
        let total = self.items.len() as u32;
        let passed = self.items.iter().filter(|i| i.passed).count() as u32;
        let failed = total - passed;
        let critical_failed = self.items.iter().filter(|i| !i.passed && i.severity == 1).count() as u32;
        let rate = if total == 0 { 256 } else { ((passed as u64 * 256) / total as u64) as u16 };
        self.report = DetectionReport {
            total_checks: total,
            passed,
            failed,
            skipped: 0,
            critical_failed,
            detection_rate_q8: rate,
        };
        self.report
    }

    /// 双签。
    pub fn sign(&mut self, impl_id: u32, review_id: u32) -> bool {
        if self.report.critical_failed > 0 {
            return false;
        }
        self.handover.signed_by_impl = Some(impl_id);
        self.handover.signed_by_review = Some(review_id);
        true
    }

    pub fn add_risk(&mut self, risk: RiskItem) {
        self.handover.risk_items.push(risk);
    }

    pub fn add_interface(&mut self, id: u32) {
        self.handover.interface_surface.push(id);
    }

    pub fn add_reserved(&mut self, id: u32) {
        self.handover.reserved_points.push(id);
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0304 B09 组收口");
        cs.add_dyn("16 项自检齐全", self.items.len() == 16, &alloc::format!("n={}", self.items.len()));
        cs.add("自检 id 唯一", {
            let mut ids: Vec<u32> = self.items.iter().map(|i| i.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.items.len()
        }, "unique");
        cs.add("报告计数自洽", {
            self.report.passed + self.report.failed + self.report.skipped == self.report.total_checks
        }, "sum");
        cs.add("检出率 0..=256", self.report.detection_rate_q8 <= 256, "q8");
        cs.add("严重项零失败或已双签", {
            self.report.critical_failed == 0 || self.handover.signed_by_impl.is_some()
        }, "critical gate");
        cs.add("移交目标为 B10", self.handover.to_group == "B10", "B10");
        cs.add("接口面非空或已注明", {
            !self.handover.interface_surface.is_empty() || !self.handover.reserved_points.is_empty()
        }, "surface");
        cs.add("风险项均有缓解措施", {
            self.handover.risk_items.iter().all(|r| r.mitigation_id > 0)
        }, "mitigated");
        cs
    }
}

impl Default for B09Closeout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_16_checks() {
        let mut c = B09Closeout::new();
        c.load_standard_checks();
        assert_eq!(c.items.len(), 16);
        for i in 1..=16 {
            c.set_result(i, true);
        }
        let r = c.generate_report();
        assert_eq!(r.passed, 16);
        assert_eq!(r.critical_failed, 0);
        assert!(c.sign(1, 2));
    }

    #[test]
    fn critical_blocks_sign() {
        let mut c = B09Closeout::new();
        c.load_standard_checks();
        c.set_result(1, false); // severity 1
        c.generate_report();
        assert!(!c.sign(1, 2));
    }
}
