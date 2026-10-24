//! VE-F3620 · R01 组收口双签（VE-S 域 · 批次 S01 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3620`
//!
//! **判据（锚点原文六条）**：十三族、收敛硬门、许可硬门、保真证据、
//! 双签、判据。逐条落位：
//! - **十三族**：R01 冻结 API 十三族签名表（资产模型/工作流/对接/预览/
//!   验证/导入/版本/预算/无障碍/安全/调试/扩展/分发），[`ApiLedger`]。
//!   映射缺 → 阻断。
//! - **收敛硬门**：零私消费断言（收敛破 → P1 重构）。
//! - **许可硬门**：零侵权（侵权检出 → 阻断）。
//! - **保真证据**：保真对拍全绿证据留存（[`FidelityEvidence`]）。
//! - **双签**：双签结构（内部签 + 外部签），缺一不可开工下一组。
//! - **判据**：`run_ves20_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 映射缺→阻断 | [`V14Code::MAP_MISS`] |
//! | 收敛破→P1 重构（收敛硬门） | [`V14Code::CONVERGE_BREAK`] |
//! | 侵权检出→阻断（许可硬门） | [`V14Code::LICENSE_HIT`] |
//! | 联动分歧→两轮上限（复述） | [`V14Code::RECONCILE_LIMIT`] |
//!
//! **码段**：独占 **0x44xx**。
//!
//! **性能逐项分解**：盘点 O(API)；映射 O(1)；联动 O(断言)。
//!
//! **跨批对接点**：R02 开工闸；台账流转；F2473 词典——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3620 诊断码段（独占 0x44xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V14Code(pub u16);

impl V14Code {
    /// 映射缺失。
    pub const MAP_MISS: V14Code = V14Code(0x4401);
    /// 收敛被破坏。
    pub const CONVERGE_BREAK: V14Code = V14Code(0x4402);
    /// 许可侵权检出。
    pub const LICENSE_HIT: V14Code = V14Code(0x4403);
    /// 联动对账超两轮。
    pub const RECONCILE_LIMIT: V14Code = V14Code(0x4404);
    /// 双签缺一。
    pub const SIGN_MISS: V14Code = V14Code(0x4405);
    /// 保真证据缺失。
    pub const FIDELITY_EVIDENCE_MISS: V14Code = V14Code(0x4406);
    /// 族缺一。
    pub const FAMILY_GAP: V14Code = V14Code(0x4407);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V14Code::MAP_MISS => {
                "签名映射缺失。建议：补全十三族映射后重新盘点。"
            }
            V14Code::CONVERGE_BREAK => {
                "收敛被破坏（检出私消费）。建议：重构为统一管线消费后重新收口。"
            }
            V14Code::LICENSE_HIT => {
                "许可审计检出侵权（硬门）。建议：下架侵权资产后重新收口。"
            }
            V14Code::RECONCILE_LIMIT => {
                "联动对账超两轮上限。建议：人工介入裁决分歧。"
            }
            V14Code::SIGN_MISS => {
                "双签缺一。建议：补齐内部签与外部签后方可开工下一组。"
            }
            V14Code::FIDELITY_EVIDENCE_MISS => {
                "保真证据缺失。建议：补跑保真对拍并留存证据。"
            }
            V14Code::FAMILY_GAP => {
                "十三族缺一。建议：补齐缺失族的 API 盘点。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 十三族 API 总账 ─────────────────────────────────────────────────

/// API 族（十三）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 资产模型。
    AssetModel,
    /// 工作流。
    Workflow,
    /// 对接。
    Dock,
    /// 预览。
    Preview,
    /// 验证。
    Verify,
    /// 导入。
    Import,
    /// 版本。
    Version,
    /// 预算。
    Budget,
    /// 无障碍。
    A11y,
    /// 安全。
    Security,
    /// 调试。
    Debug,
    /// 扩展。
    Extension,
    /// 分发。
    Distribute,
}

impl ApiFamily {
    /// 十三族全集。
    pub fn all() -> [ApiFamily; 13] {
        [
            ApiFamily::AssetModel,
            ApiFamily::Workflow,
            ApiFamily::Dock,
            ApiFamily::Preview,
            ApiFamily::Verify,
            ApiFamily::Import,
            ApiFamily::Version,
            ApiFamily::Budget,
            ApiFamily::A11y,
            ApiFamily::Security,
            ApiFamily::Debug,
            ApiFamily::Extension,
            ApiFamily::Distribute,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::AssetModel => "资产模型",
            ApiFamily::Workflow => "工作流",
            ApiFamily::Dock => "对接",
            ApiFamily::Preview => "预览",
            ApiFamily::Verify => "验证",
            ApiFamily::Import => "导入",
            ApiFamily::Version => "版本",
            ApiFamily::Budget => "预算",
            ApiFamily::A11y => "无障碍",
            ApiFamily::Security => "安全",
            ApiFamily::Debug => "调试",
            ApiFamily::Extension => "扩展",
            ApiFamily::Distribute => "分发",
        }
    }
}

/// 一行签名。
#[derive(Clone, Debug)]
pub struct ApiSignature {
    /// 族。
    pub family: ApiFamily,
    /// 函数名。
    pub func: String,
    /// 是否有映射（映射缺→阻断）。
    pub mapped: bool,
}

/// API 总账。
#[derive(Clone, Debug, Default)]
pub struct ApiLedger {
    sigs: Vec<ApiSignature>,
}

impl ApiLedger {
    /// 空账。
    pub fn new() -> ApiLedger {
        ApiLedger { sigs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, family: ApiFamily, func: &str, mapped: bool) {
        self.sigs.push(ApiSignature {
            family,
            func: String::from(func),
            mapped,
        });
    }

    /// 族覆盖审计（十三族缺一不可）。
    pub fn audit_families(&self) -> Result<(), V14Code> {
        for f in ApiFamily::all() {
            if !self.sigs.iter().any(|s| s.family == f) {
                return Err(V14Code::FAMILY_GAP);
            }
        }
        Ok(())
    }

    /// 映射审计（全部签名都有映射）。
    pub fn audit_mapping(&self) -> Result<(), V14Code> {
        if self.sigs.iter().any(|s| !s.mapped) {
            Err(V14Code::MAP_MISS)
        } else {
            Ok(())
        }
    }

    /// 签名数。
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }
}

// ── 收敛断言（零私消费）─────────────────────────────────────────────

/// 收敛审计器。
#[derive(Clone, Debug, Default)]
pub struct ConvergenceAudit {
    private_consumers: u32,
}

impl ConvergenceAudit {
    /// 新建。
    pub fn new() -> ConvergenceAudit {
        ConvergenceAudit {
            private_consumers: 0,
        }
    }

    /// 记一次私消费（违规）。
    pub fn record_private(&mut self) {
        self.private_consumers = self.private_consumers.wrapping_add(1);
    }

    /// 收敛断言。
    pub fn assert_clean(&self) -> Result<(), V14Code> {
        if self.private_consumers == 0 {
            Ok(())
        } else {
            Err(V14Code::CONVERGE_BREAK)
        }
    }
}

// ── 许可硬门 ────────────────────────────────────────────────────────

/// 许可审计器。
#[derive(Clone, Debug, Default)]
pub struct LicenseAudit {
    hits: u32,
}

impl LicenseAudit {
    /// 新建。
    pub fn new() -> LicenseAudit {
        LicenseAudit { hits: 0 }
    }

    /// 记一次侵权命中。
    pub fn record_hit(&mut self) {
        self.hits = self.hits.wrapping_add(1);
    }

    /// 零侵权断言。
    pub fn assert_clean(&self) -> Result<(), V14Code> {
        if self.hits == 0 {
            Ok(())
        } else {
            Err(V14Code::LICENSE_HIT)
        }
    }
}

// ── 保真证据 ────────────────────────────────────────────────────────

/// 保真证据。
#[derive(Clone, Debug, Default)]
pub struct FidelityEvidence {
    /// 对拍次数。
    pub samples: u32,
    /// 漂移次数。
    pub drifted: u32,
    /// 证据是否留存。
    pub stored: bool,
}

impl FidelityEvidence {
    /// 新建。
    pub fn new() -> FidelityEvidence {
        FidelityEvidence {
            samples: 0,
            drifted: 0,
            stored: false,
        }
    }

    /// 记一次对拍。
    pub fn record(&mut self, ok: bool) {
        self.samples = self.samples.wrapping_add(1);
        if !ok {
            self.drifted = self.drifted.wrapping_add(1);
        }
    }

    /// 留存证据。
    pub fn store(&mut self) {
        self.stored = true;
    }

    /// 证据审计：有样本、零漂移、已留存。
    pub fn audit(&self) -> Result<(), V14Code> {
        if self.samples > 0 && self.drifted == 0 && self.stored {
            Ok(())
        } else {
            Err(V14Code::FIDELITY_EVIDENCE_MISS)
        }
    }
}

// ── 双签 ────────────────────────────────────────────────────────────

/// 签名角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignRole {
    /// 内部签（施工方）。
    Internal,
    /// 外部签（验收方）。
    External,
}

/// 双签结构。
#[derive(Clone, Debug, Default)]
pub struct DualSign {
    internal: bool,
    external: bool,
}

impl DualSign {
    /// 新建未签。
    pub fn new() -> DualSign {
        DualSign {
            internal: false,
            external: false,
        }
    }

    /// 签一注。
    pub fn sign(&mut self, role: SignRole) {
        match role {
            SignRole::Internal => self.internal = true,
            SignRole::External => self.external = true,
        }
    }

    /// 双签齐备断言。
    pub fn audit(&self) -> Result<(), V14Code> {
        if self.internal && self.external {
            Ok(())
        } else {
            Err(V14Code::SIGN_MISS)
        }
    }

    /// 是否可开工下一组。
    pub fn opens_next_group(&self) -> bool {
        self.internal && self.external
    }
}

// ── 联动对账（两轮上限）─────────────────────────────────────────────

/// 对账轮次上限（独立写死）。
pub const RECONCILE_MAX_ROUNDS: u32 = 2;

/// 对账器。
#[derive(Clone, Debug, Default)]
pub struct Reconciler {
    rounds: u32,
}

impl Reconciler {
    /// 新建。
    pub fn new() -> Reconciler {
        Reconciler { rounds: 0 }
    }

    /// 走一轮。
    pub fn round(&mut self) -> Result<(), V14Code> {
        self.rounds = self.rounds.wrapping_add(1);
        if self.rounds > RECONCILE_MAX_ROUNDS {
            Err(V14Code::RECONCILE_LIMIT)
        } else {
            Ok(())
        }
    }

    /// 轮次。
    pub fn rounds(&self) -> u32 {
        self.rounds
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["R02-GATE", "LEDGER", "VE-F2473"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves20_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3620");

    // 0x4401 十三族齐
    let mut ledger = ApiLedger::new();
    for f in ApiFamily::all() {
        ledger.add(f, "api_fn", true);
    }
    out.add("0x4401-十三族齐", ledger.audit_families().is_ok(), "");

    // 0x4402 族缺检出
    let mut ledger2 = ApiLedger::new();
    ledger2.add(ApiFamily::AssetModel, "f", true);
    out.add("0x4402-族缺检出", ledger2.audit_families().is_err(), "");

    // 0x4403 映射缺检出
    let mut ledger3 = ApiLedger::new();
    for f in ApiFamily::all() {
        ledger3.add(f, "api_fn", f != ApiFamily::Debug);
    }
    out.add("0x4403-映射缺检出", ledger3.audit_mapping().is_err(), "");

    // 0x4404 收敛干净
    let ca = ConvergenceAudit::new();
    out.add("0x4404-收敛干净", ca.assert_clean().is_ok(), "");

    // 0x4405 收敛破坏
    let mut ca2 = ConvergenceAudit::new();
    ca2.record_private();
    out.add("0x4405-收敛破坏检出", ca2.assert_clean().is_err(), "");

    // 0x4406 许可零侵权
    let la = LicenseAudit::new();
    out.add("0x4406-许可零侵权", la.assert_clean().is_ok(), "");

    // 0x4407 侵权检出
    let mut la2 = LicenseAudit::new();
    la2.record_hit();
    out.add("0x4407-侵权检出阻断", la2.assert_clean().is_err(), "");

    // 0x4408 保真证据齐
    let mut fe = FidelityEvidence::new();
    fe.record(true);
    fe.store();
    out.add("0x4408-保真证据齐", fe.audit().is_ok(), "");

    // 0x4409 双签齐
    let mut ds = DualSign::new();
    ds.sign(SignRole::Internal);
    ds.sign(SignRole::External);
    out.add("0x4409-双签齐可开工", ds.audit().is_ok() && ds.opens_next_group(), "");

    // 0x440a 双签缺
    let ds2 = DualSign::new();
    out.add("0x440a-双签缺检出", ds2.audit().is_err(), "");

    // 0x440b 两轮上限
    let mut rc = Reconciler::new();
    rc.round().ok();
    rc.round().ok();
    out.add("0x440b-两轮上限", rc.round().is_err(), "");

    out
}
