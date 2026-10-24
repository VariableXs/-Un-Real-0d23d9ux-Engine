//! VE-F3615 · 创作生态扩展点（VE-S 域 · 批次 S01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3615`
//!
//! **判据（锚点原文六条）**：五点三段、预算隔离、治理单源、十年稳定、
//! 复述硬门、判据。逐条落位：
//! - **五点三段**：五扩展点（资产类型注册/工作流步骤插件/验证规则插件/
//!   预览渲染插件/导入格式插件），每点三段（签名/沙箱/失效）——三段
//!   缺一不可发布（[`V0FCode::TRIAD_MISS`]，复述硬门）。
//! - **预算隔离**：扩展破预算 → 隔离（[`BudgetIsolator`]，红线实测）。
//! - **治理单源**：F3109 治理单源复述（[`F3109_RESTATED_NOTE`]）。
//! - **十年稳定**：API 稳定承诺（[`StabilityPromise`]），破坏 → ADR。
//! - **复述硬门**：三段结构是发布硬门不是建议。
//! - **判据**：`run_ves15_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 三段缺→不可发布（复述硬门） | [`V0FCode::TRIAD_MISS`]：签名/沙箱/失效缺一拒发布 |
//! | 破预算→隔离（复述红线实测） | [`V0FCode::BUDGET_BREAK`]：超份额即隔离 |
//! | 绕治理→P0（复述） | [`V0FCode::GOV_BYPASS`]：绕过治理登记即 P0 |
//! | API 破坏→ADR（复述） | [`V0FCode::API_BREAK`]：稳定承诺破坏立案 |
//!
//! **码段**：独占 **0x3fxx**。
//!
//! **性能逐项分解**：清单 O(点)；隔离 O(扩展)；治理 O(1)；
//! 预算 O(份额)。
//!
//! **跨批对接点**：F3109/F3347/F3204 三单源复用；F3320 模式复用——
//! 钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3615 诊断码段（独占 0x3fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0FCode(pub u16);

impl V0FCode {
    /// 三段缺一（签名/沙箱/失效）。
    pub const TRIAD_MISS: V0FCode = V0FCode(0x3f01);
    /// 扩展破预算。
    pub const BUDGET_BREAK: V0FCode = V0FCode(0x3f02);
    /// 绕过治理登记。
    pub const GOV_BYPASS: V0FCode = V0FCode(0x3f03);
    /// 稳定承诺破坏。
    pub const API_BREAK: V0FCode = V0FCode(0x3f04);
    /// 未知扩展点类型。
    pub const UNKNOWN_POINT: V0FCode = V0FCode(0x3f05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0FCode::TRIAD_MISS => {
                "扩展三段（签名/沙箱/失效）缺一，不可发布（硬门）。建议：补齐缺失段后重新审查。"
            }
            V0FCode::BUDGET_BREAK => {
                "扩展超出分配预算，已隔离。建议：优化扩展资源占用或申请更高份额。"
            }
            V0FCode::GOV_BYPASS => {
                "绕过治理登记（P0）。所有扩展必须走 F3109 治理通道，无旁路。"
            }
            V0FCode::API_BREAK => {
                "稳定承诺 API 被破坏。建议：走 ADR 流程显式宣布破坏性变更。"
            }
            V0FCode::UNKNOWN_POINT => {
                "未知扩展点类型。建议：在五点清单中登记新类型后再注册扩展。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 五扩展点 ────────────────────────────────────────────────────────

/// 扩展点类型（五点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtensionPoint {
    /// 资产类型注册。
    AssetType,
    /// 工作流步骤插件。
    WorkflowStep,
    /// 验证规则插件。
    ValidationRule,
    /// 预览渲染插件。
    PreviewRender,
    /// 导入格式插件。
    ImportFormat,
}

impl ExtensionPoint {
    /// 全集。
    pub fn all() -> [ExtensionPoint; 5] {
        [
            ExtensionPoint::AssetType,
            ExtensionPoint::WorkflowStep,
            ExtensionPoint::ValidationRule,
            ExtensionPoint::PreviewRender,
            ExtensionPoint::ImportFormat,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ExtensionPoint::AssetType => "资产类型注册",
            ExtensionPoint::WorkflowStep => "工作流步骤插件",
            ExtensionPoint::ValidationRule => "验证规则插件",
            ExtensionPoint::PreviewRender => "预览渲染插件",
            ExtensionPoint::ImportFormat => "导入格式插件",
        }
    }
}

// ── 三段结构（签名/沙箱/失效）───────────────────────────────────────

/// 三段之一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Triad {
    /// 签名。
    Signature,
    /// 沙箱。
    Sandbox,
    /// 失效处理。
    Failure,
}

impl Triad {
    /// 全集。
    pub fn all() -> [Triad; 3] {
        [Triad::Signature, Triad::Sandbox, Triad::Failure]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Triad::Signature => "签名",
            Triad::Sandbox => "沙箱",
            Triad::Failure => "失效处理",
        }
    }
}

/// 一条扩展注册。
#[derive(Clone, Debug)]
pub struct ExtensionReg {
    /// 扩展点。
    pub point: ExtensionPoint,
    /// 扩展名。
    pub name: String,
    /// 三段完备位图（bit0 签名 / bit1 沙箱 / bit2 失效）。
    pub triad_mask: u8,
    /// 是否走治理登记。
    pub governed: bool,
    /// 预算份额（0..100）。
    pub budget_share: u8,
    /// 实际用量（0..100）。
    pub usage: u8,
}

impl ExtensionReg {
    /// 三段齐全。
    pub fn triad_complete(&self) -> bool {
        self.triad_mask == 0b111
    }

    /// 发布硬门：三段齐 + 治理已登记。
    pub fn publishable(&self) -> Result<(), V0FCode> {
        if !self.triad_complete() {
            return Err(V0FCode::TRIAD_MISS);
        }
        if !self.governed {
            return Err(V0FCode::GOV_BYPASS);
        }
        Ok(())
    }
}

/// 扩展注册表。
#[derive(Clone, Debug, Default)]
pub struct ExtensionRegistry {
    regs: Vec<ExtensionReg>,
}

impl ExtensionRegistry {
    /// 空表。
    pub fn new() -> ExtensionRegistry {
        ExtensionRegistry { regs: Vec::new() }
    }

    /// 注册。
    pub fn register(&mut self, reg: ExtensionReg) {
        self.regs.push(reg);
    }

    /// 全表发布审计。
    pub fn audit_publish(&self) -> Result<(), V0FCode> {
        for r in &self.regs {
            r.publishable()?;
        }
        Ok(())
    }

    /// 三段缺一清单。
    pub fn triad_gaps(&self) -> Vec<&str> {
        self.regs
            .iter()
            .filter(|r| !r.triad_complete())
            .map(|r| r.name.as_str())
            .collect()
    }

    /// 扩展数。
    pub fn len(&self) -> usize {
        self.regs.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.regs.is_empty()
    }
}

// ── 预算隔离 ────────────────────────────────────────────────────────

/// 预算隔离器。
#[derive(Clone, Debug, Default)]
pub struct BudgetIsolator {
    isolated: Vec<String>,
}

impl BudgetIsolator {
    /// 新建。
    pub fn new() -> BudgetIsolator {
        BudgetIsolator {
            isolated: Vec::new(),
        }
    }

    /// 检查并隔离超预算扩展。
    pub fn enforce(&mut self, reg: &ExtensionReg) -> Result<(), V0FCode> {
        if reg.usage > reg.budget_share {
            self.isolated.push(reg.name.clone());
            return Err(V0FCode::BUDGET_BREAK);
        }
        Ok(())
    }

    /// 已隔离清单。
    pub fn isolated_list(&self) -> &[String] {
        &self.isolated
    }

    /// 隔离数。
    pub fn isolated_count(&self) -> usize {
        self.isolated.len()
    }
}

// ── 稳定承诺 ────────────────────────────────────────────────────────

/// 稳定承诺（十年期）。
#[derive(Clone, Debug)]
pub struct StabilityPromise {
    /// 承诺的 API 名。
    pub api: String,
    /// 承诺年限（默认 10）。
    pub years: u32,
    /// 是否已被破坏。
    pub broken: bool,
}

impl StabilityPromise {
    /// 新建十年承诺。
    pub fn new(api: &str) -> StabilityPromise {
        StabilityPromise {
            api: String::from(api),
            years: 10,
            broken: false,
        }
    }

    /// 标记破坏（须走 ADR）。
    pub fn break_with_adr(&mut self, _adr_id: &str) {
        self.broken = true;
    }

    /// 审计。
    pub fn audit(&self) -> Result<(), V0FCode> {
        if self.broken {
            Err(V0FCode::API_BREAK)
        } else {
            Ok(())
        }
    }
}

// ── 单源复述声明 ────────────────────────────────────────────────────

/// F3109 治理单源复述件。
pub const F3109_RESTATED_NOTE: &str =
    "F3109（扩展治理）单源复述：治理登记/审查/下线语义按 F3109 锚点复述、本地实现；\
     绕治理 = P0 无旁路；F3109 落库后由对账钩子接管。";

/// 对接点清单。
pub const DOCKS: [&str; 4] = ["VE-F3109", "VE-F3347", "VE-F3204", "VE-F3320"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves15_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3615");

    // 0x3f01 三段齐可发布
    let good = ExtensionReg {
        point: ExtensionPoint::PreviewRender,
        name: String::from("ext-good"),
        triad_mask: 0b111,
        governed: true,
        budget_share: 30,
        usage: 10,
    };
    out.add("0x3f01-三段齐可发布", good.publishable().is_ok(), "");

    // 0x3f02 三段缺一拒发布
    let bad = ExtensionReg {
        triad_mask: 0b101,
        name: String::from("ext-bad"),
        ..good.clone()
    };
    out.add("0x3f02-三段缺一拒发布", bad.publishable().is_err(), "");

    // 0x3f03 绕治理拒发布
    let ungov = ExtensionReg {
        governed: false,
        name: String::from("ext-ungov"),
        ..good.clone()
    };
    out.add("0x3f03-绕治理拒发布", ungov.publishable().is_err(), "");

    // 0x3f04 预算隔离
    let mut iso = BudgetIsolator::new();
    let over = ExtensionReg {
        usage: 80,
        budget_share: 50,
        name: String::from("ext-over"),
        ..good.clone()
    };
    out.add("0x3f04-超预算即隔离", iso.enforce(&over).is_err() && iso.isolated_count() == 1, "");

    // 0x3f05 份额内放行
    let mut iso2 = BudgetIsolator::new();
    out.add("0x3f05-份额内放行", iso2.enforce(&good).is_ok(), "");

    // 0x3f06 稳定承诺
    let p = StabilityPromise::new("create_asset");
    out.add("0x3f06-稳定承诺未破坏", p.audit().is_ok(), "");

    // 0x3f07 破坏立案
    let mut p2 = StabilityPromise::new("create_asset");
    p2.break_with_adr("ADR-42");
    out.add("0x3f07-破坏立案", p2.audit().is_err(), "");

    // 0x3f08 注册表全表审计
    let mut reg = ExtensionRegistry::new();
    reg.register(good.clone());
    out.add("0x3f08-全表可发布", reg.audit_publish().is_ok(), "");

    // 0x3f09 缺口清单
    let mut reg2 = ExtensionRegistry::new();
    reg2.register(bad.clone());
    out.add("0x3f09-缺口清单正确", reg2.triad_gaps().len() == 1, "");

    out
}
