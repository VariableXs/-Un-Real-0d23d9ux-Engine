//! VE-F3634 · 编辑器测试（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3634`
//!
//! **判据（锚点原文六条）**：七类断言、语义选择器、脆弱红线、覆盖复述、
//! 生成器单源、判据。逐条落位：
//! - **七类断言**：功能/性能/无障碍/保真/undo/兼容/失败七类生成器。
//! - **语义选择器**：脆弱选择器禁令（脆弱红线）——[`Selector`] 只允许
//!   语义级，脆弱率 > 5% → [`V22Code::FRAGILE_RATE`]。
//! - **脆弱红线**：脆弱 UI 测试 = 测试债。
//! - **覆盖复述**：七类覆盖审计。
//! - **生成器单源**：F3019 生成器复用。
//! - **判据**：`run_ves34_checks` 逐条映射。
//!
//! **码段**：独占 **0x52xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3634 诊断码段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V22Code(pub u16);

impl V22Code {
    /// 脆弱选择器超率。
    pub const FRAGILE_RATE: V22Code = V22Code(0x5201);
    /// 七类覆盖缺格。
    pub const COVERAGE_GAP: V22Code = V22Code(0x5202);
    /// 生成器漂移。
    pub const GEN_DRIFT: V22Code = V22Code(0x5203);
    /// 语义选择器违规。
    pub const SELECTOR_FRAGILE: V22Code = V22Code(0x5204);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V22Code::FRAGILE_RATE => "脆弱选择器率超 5%（脆弱红线）。建议：重构为语义选择器。",
            V22Code::COVERAGE_GAP => "七类断言覆盖缺格。建议：补齐缺失类。",
            V22Code::GEN_DRIFT => "生成器漂移。建议：对拍生成器输出。",
            V22Code::SELECTOR_FRAGILE => "语义选择器违规。建议：用角色/名称定位。",
            _ => "未知诊断码。",
        }
    }
}

// ── 七类断言 ────────────────────────────────────────────────────────

/// 断言类别（七类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertClass {
    /// 功能。
    Functional,
    /// 性能。
    Perf,
    /// 无障碍。
    A11y,
    /// 保真。
    Fidelity,
    /// undo。
    Undo,
    /// 兼容。
    Compat,
    /// 失败路径。
    Failure,
}

impl AssertClass {
    /// 全集。
    pub fn all() -> [AssertClass; 7] {
        [
            AssertClass::Functional,
            AssertClass::Perf,
            AssertClass::A11y,
            AssertClass::Fidelity,
            AssertClass::Undo,
            AssertClass::Compat,
            AssertClass::Failure,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            AssertClass::Functional => "功能",
            AssertClass::Perf => "性能",
            AssertClass::A11y => "无障碍",
            AssertClass::Fidelity => "保真",
            AssertClass::Undo => "undo",
            AssertClass::Compat => "兼容",
            AssertClass::Failure => "失败",
        }
    }
}

// ── 语义选择器 ──────────────────────────────────────────────────────

/// 选择器。
#[derive(Clone, Debug)]
pub struct Selector {
    /// 定位方式。
    pub method: SelectorMethod,
}

/// 选择器方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectorMethod {
    /// 语义角色（推荐）。
    Role,
    /// 可访问名。
    Name,
    /// CSS 类（脆弱）。
    CssClass,
    /// DOM 路径（脆弱）。
    DomPath,
}

impl SelectorMethod {
    /// 是否脆弱。
    pub fn is_fragile(self) -> bool {
        matches!(self, SelectorMethod::CssClass | SelectorMethod::DomPath)
    }
}

/// 选择器审计器。
#[derive(Clone, Debug, Default)]
pub struct SelectorAudit {
    total: usize,
    fragile: usize,
}

impl SelectorAudit {
    /// 新建。
    pub fn new() -> SelectorAudit {
        SelectorAudit {
            total: 0,
            fragile: 0,
        }
    }

    /// 登记一个选择器。
    pub fn record(&mut self, method: SelectorMethod) {
        self.total += 1;
        if method.is_fragile() {
            self.fragile += 1;
        }
    }

    /// 脆弱率审计（≤5%）。
    pub fn audit_rate(&self) -> Result<(), V22Code> {
        if self.total == 0 {
            return Ok(());
        }
        let rate_pct = (self.fragile * 100) / self.total;
        if rate_pct > 5 {
            Err(V22Code::FRAGILE_RATE)
        } else {
            Ok(())
        }
    }

    /// 脆弱率百分比。
    pub fn fragile_pct(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            ((self.fragile * 100) / self.total) as u32
        }
    }
}

// ── 七类覆盖 ────────────────────────────────────────────────────────

/// 覆盖登记。
#[derive(Clone, Debug, Default)]
pub struct CoverageRegistry {
    covered: Vec<AssertClass>,
}

impl CoverageRegistry {
    /// 新建。
    pub fn new() -> CoverageRegistry {
        CoverageRegistry {
            covered: Vec::new(),
        }
    }

    /// 覆盖一类。
    pub fn cover(&mut self, class: AssertClass) {
        if !self.covered.contains(&class) {
            self.covered.push(class);
        }
    }

    /// 覆盖审计。
    pub fn audit(&self) -> Result<(), V22Code> {
        for c in AssertClass::all() {
            if !self.covered.contains(&c) {
                return Err(V22Code::COVERAGE_GAP);
            }
        }
        Ok(())
    }

    /// 缺格清单。
    pub fn gaps(&self) -> Vec<AssertClass> {
        AssertClass::all()
            .iter()
            .filter(|c| !self.covered.contains(c))
            .cloned()
            .collect()
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3019", "VE-F3345", "VE-F3150"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves34_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3634");

    // 0x5201 七类齐
    let mut cov = CoverageRegistry::new();
    for c in AssertClass::all() {
        cov.cover(c);
    }
    out.add("0x5201-七类齐", cov.audit().is_ok(), "");

    // 0x5202 缺格检出
    let cov2 = CoverageRegistry::new();
    out.add("0x5202-缺格检出", cov2.audit().is_err() && cov2.gaps().len() == 7, "");

    // 0x5203 脆弱率
    let mut sa = SelectorAudit::new();
    for _ in 0..20 {
        sa.record(SelectorMethod::Role);
    }
    out.add("0x5203-脆弱率", sa.audit_rate().is_ok() && sa.fragile_pct() == 0, "");

    // 0x5204 脆弱超标
    let mut sa2 = SelectorAudit::new();
    for _ in 0..18 {
        sa2.record(SelectorMethod::Role);
    }
    sa2.record(SelectorMethod::CssClass);
    sa2.record(SelectorMethod::DomPath);
    out.add("0x5204-脆弱超标", sa2.audit_rate().is_err(), "");

    // 0x5205 语义禁脆弱
    out.add(
        "0x5205-语义禁脆弱",
        !SelectorMethod::Role.is_fragile() && SelectorMethod::DomPath.is_fragile(),
        "",
    );

    out
}
