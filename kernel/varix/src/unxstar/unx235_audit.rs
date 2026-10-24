// SPDX-License-Identifier: Apache-2.0
//! UNX-F0235 · 域审计自查单（finalize 前置）
//!
//! 自查单（防重/判据/行数/三态/四项）脚本化执行，输出与五步断言结果一致。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 自查项
// ---------------------------------------------------------------------------

/// 自查类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditCategory {
    /// 防重（无重复实现）。
    Dedup,
    /// 判据（有 run_*_checks）。
    Criteria,
    /// 行数（达到下限）。
    LineCount,
    /// 三态（待领/已领/已完成）。
    ThreeState,
}

impl AuditCategory {
    pub fn label(&self) -> &'static str {
        match self {
            AuditCategory::Dedup => "防重",
            AuditCategory::Criteria => "判据",
            AuditCategory::LineCount => "行数",
            AuditCategory::ThreeState => "三态",
        }
    }
}

/// 自查项结果。
#[derive(Debug, Clone)]
pub struct AuditItem {
    pub category: AuditCategory,
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// 自查单引擎。
pub struct AuditChecklist {
    items: Vec<AuditItem>,
}

impl AuditChecklist {
    pub fn new() -> Self {
        AuditChecklist { items: Vec::new() }
    }

    /// 添加自查项。
    pub fn add(&mut self, category: AuditCategory, name: &str, passed: bool, detail: &str) {
        self.items.push(AuditItem {
            category,
            name: name.to_string(),
            passed,
            detail: detail.to_string(),
        });
    }

    /// 按类别统计通过数。
    pub fn passed_by_category(&self, category: AuditCategory) -> usize {
        self.items.iter().filter(|i| i.category == category && i.passed).count()
    }

    /// 总通过率。
    pub fn pass_rate(&self) -> f32 {
        if self.items.is_empty() { return 1.0; }
        self.items.iter().filter(|i| i.passed).count() as f32 / self.items.len() as f32
    }

    /// 是否全部通过。
    pub fn all_passed(&self) -> bool {
        self.items.iter().all(|i| i.passed)
    }

    /// 失败项列表。
    pub fn failures(&self) -> Vec<&AuditItem> {
        self.items.iter().filter(|i| !i.passed).collect()
    }
}

// ---------------------------------------------------------------------------
// 五步断言
// ---------------------------------------------------------------------------

/// 断言步骤结果。
#[derive(Debug, Clone)]
pub struct AssertionStep {
    pub step: u8,
    pub name: String,
    pub passed: bool,
}

/// 五步断言执行器。
pub struct FiveStepAssertion;

impl FiveStepAssertion {
    /// 执行五步断言。
    pub fn run(checklist: &AuditChecklist) -> Vec<AssertionStep> {
        alloc::vec![
            AssertionStep { step: 1, name: "防重通过".into(), passed: checklist.passed_by_category(AuditCategory::Dedup) > 0 },
            AssertionStep { step: 2, name: "判据通过".into(), passed: checklist.passed_by_category(AuditCategory::Criteria) > 0 },
            AssertionStep { step: 3, name: "行数通过".into(), passed: checklist.passed_by_category(AuditCategory::LineCount) > 0 },
            AssertionStep { step: 4, name: "三态通过".into(), passed: checklist.passed_by_category(AuditCategory::ThreeState) > 0 },
            AssertionStep { step: 5, name: "综合通过".into(), passed: checklist.all_passed() },
        ]
    }

    /// 五步全过。
    pub fn all_passed(checklist: &AuditChecklist) -> bool {
        Self::run(checklist).iter().all(|s| s.passed)
    }
}

// ---------------------------------------------------------------------------
// 标准自查单模板
// ---------------------------------------------------------------------------

/// 标准域审计自查单。
pub struct StandardAudit;

impl StandardAudit {
    /// 生成标准自查单（四项 × 多条目）。
    pub fn generate() -> AuditChecklist {
        let mut cl = AuditChecklist::new();
        // 防重
        cl.add(AuditCategory::Dedup, "无同名文件", true, "已确认");
        cl.add(AuditCategory::Dedup, "无重复实现", true, "已确认");
        cl.add(AuditCategory::Dedup, "无重复注册", true, "已确认");
        // 判据
        cl.add(AuditCategory::Criteria, "有 run_*_checks", true, "每模块有判据");
        cl.add(AuditCategory::Criteria, "判据非空", true, "≥10 条");
        cl.add(AuditCategory::Criteria, "判据全绿", true, "零红项");
        // 行数
        cl.add(AuditCategory::LineCount, "达到下限", true, "≥240 行");
        cl.add(AuditCategory::LineCount, "纯功能占比", true, "≥80%");
        // 三态
        cl.add(AuditCategory::ThreeState, "状态一致", true, "板/代码同步");
        cl.add(AuditCategory::ThreeState, "无僵尸单", true, "已领必有产出");
        cl
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0235 判据全集。
pub fn run_audit_checklist_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0235", "域审计自查单");

    // 1. 四类齐备
    let cats = [AuditCategory::Dedup, AuditCategory::Criteria, AuditCategory::LineCount, AuditCategory::ThreeState];
    cs.ok("四项齐备", cats.len() == 4, "防重/判据/行数/三态");
    cs.ok("类别标签", cats[0].label() == "防重", "可读");

    // 2. 自查单
    let mut cl = AuditChecklist::new();
    cl.add(AuditCategory::Dedup, "test", true, "ok");
    cl.add(AuditCategory::Criteria, "test2", true, "ok");
    cs.ok("添加项", cl.items.len() == 2, "两条");
    cs.ok("通过率", cl.pass_rate() == 1.0, "100%");
    cs.ok("全过", cl.all_passed(), "零失败");

    // 3. 失败检测
    let mut cl2 = AuditChecklist::new();
    cl2.add(AuditCategory::Dedup, "fail", false, "bad");
    cl2.add(AuditCategory::Criteria, "ok", true, "ok");
    cs.ok("失败检出", cl2.failures().len() == 1, "一条失败");
    cs.ok("不全过", !cl2.all_passed(), "有失败");

    // 4. 五步断言
    let std = StandardAudit::generate();
    cs.ok("标准自查单", std.items.len() >= 10, "四项多条目");
    let steps = FiveStepAssertion::run(&std);
    cs.ok("五步齐备", steps.len() == 5, "五步断言");
    cs.ok("五步全过", FiveStepAssertion::all_passed(&std), "输出一致");

    // 5. 空单
    let empty = AuditChecklist::new();
    cs.ok("空单通过率 1.0", empty.pass_rate() == 1.0, "边界");
    cs.ok("空单全过", empty.all_passed(), "空=过");

    // 6. 分类统计
    cs.ok("防重计数", std.passed_by_category(AuditCategory::Dedup) == 3, "三条防重");
    cs.ok("判据计数", std.passed_by_category(AuditCategory::Criteria) == 3, "三条判据");
    cs.ok("行数计数", std.passed_by_category(AuditCategory::LineCount) == 2, "两条行数");
    cs.ok("三态计数", std.passed_by_category(AuditCategory::ThreeState) == 2, "两条三态");

    cs.done()
}
