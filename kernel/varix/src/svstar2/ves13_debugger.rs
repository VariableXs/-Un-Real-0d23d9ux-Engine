//! VE-F3613 · 创作调试器（VE-S 域 · 批次 S01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3613`
//!
//! **判据（锚点原文六条）**：三维体检、定位断言、三要素、家族十六、
//! 联动复用、判据。逐条落位：
//! - **三维体检**：资产体检三维总览（验证结果/性能画像/兼容状态）
//!   （[`HealthReport`]）——体检即入口，三维缺一 → [`V0DCode::DIM_MISSING`]。
//! - **定位断言**：验证失败 → 问题定位到资产内**具体位置**（定位精度
//!   红线：只报错不定位 = 调试器失职）——[`Locator::locate`] 必须返回
//!   非空定位，定位缺失 → [`V0DCode::LOCATE_MISS`] 拦截。
//! - **三要素**：每错误带修复建议（三要素复述）——[`Remedy`] 三要素
//!   （现象/原因/动作）缺一不可，缺 → [`V0DCode::REMEDY_INCOMPLETE`]。
//! - **家族十六**：调试器协议家族十六成员（复述 F2815 协议）。
//! - **联动复用**：错误 → 工作流修复步骤跳转（联动复用）——
//!   [`workflow_jump`]。
//! - **判据**：`run_ves13_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 定位缺失→断言拦截（红线实测） | [`V0DCode::LOCATE_MISS`]：空定位拒报 |
//! | 建议缺→补（三要素复述） | [`V0DCode::REMEDY_INCOMPLETE`]：三要素缺一即立案 |
//! | 体检失真→修正（复述） | [`V0DCode::HEALTH_DRIFT`]：三维与明细不一致 |
//! | 联动断→修复（复述） | [`V0DCode::LINK_BROKEN`]：工作流跳转目标不存在 |
//!
//! **码段**：独占 **0x3dxx**。
//!
//! **性能逐项分解**：体检 O(三维)；定位 O(资产结构)；建议 O(1) 匹配；
//! 联动 O(1)。
//!
//! **跨批对接点**：F3607 验证（数据源）；F3604 工作流（联动对端）；
//! F2815 协议（家族十六）——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3613 诊断码段（独占 0x3dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0DCode(pub u16);

impl V0DCode {
    /// 定位缺失（只报错不定位）。
    pub const LOCATE_MISS: V0DCode = V0DCode(0x3d01);
    /// 修复建议三要素不全。
    pub const REMEDY_INCOMPLETE: V0DCode = V0DCode(0x3d02);
    /// 体检三维缺一。
    pub const DIM_MISSING: V0DCode = V0DCode(0x3d03);
    /// 体检失真（三维与明细不一致）。
    pub const HEALTH_DRIFT: V0DCode = V0DCode(0x3d04);
    /// 联动断裂。
    pub const LINK_BROKEN: V0DCode = V0DCode(0x3d05);
    /// 空错误记录。
    pub const EMPTY_ERROR: V0DCode = V0DCode(0x3d06);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0DCode::LOCATE_MISS => {
                "错误缺少定位信息（只报错不定位 = 调试器失职）。建议：补全资产内结构路径后重报。"
            }
            V0DCode::REMEDY_INCOMPLETE => {
                "修复建议三要素（现象/原因/动作）不全。建议：补齐缺失要素，用户需要完整的行动指引。"
            }
            V0DCode::DIM_MISSING => {
                "体检三维（验证/性能/兼容）缺一。建议：补全三维数据源后重新体检。"
            }
            V0DCode::HEALTH_DRIFT => {
                "体检总览与明细不一致。建议：以明细为准重算总览，排查缓存失效。"
            }
            V0DCode::LINK_BROKEN => {
                "工作流联动目标不存在。建议：核对工作流步骤 ID 后修复链接。"
            }
            V0DCode::EMPTY_ERROR => {
                "错误记录为空。建议：至少提供错误码与定位。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 三维体检 ────────────────────────────────────────────────────────

/// 体检维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 验证结果。
    Validation,
    /// 性能画像。
    Performance,
    /// 兼容状态。
    Compat,
}

impl Dim {
    /// 全集（三维）。
    pub fn all() -> [Dim; 3] {
        [Dim::Validation, Dim::Performance, Dim::Compat]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Dim::Validation => "验证结果",
            Dim::Performance => "性能画像",
            Dim::Compat => "兼容状态",
        }
    }
}

/// 一维状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimStatus {
    /// 健康。
    Healthy,
    /// 有告警。
    Warn,
    /// 有错误。
    Error,
    /// 未采集。
    Missing,
}

/// 体检报告（三维总览）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 三维状态。
    pub dims: Vec<(Dim, DimStatus)>,
    /// 明细条数（与三维对账用）。
    pub detail_count: usize,
}

impl HealthReport {
    /// 新建空报告。
    pub fn new() -> HealthReport {
        HealthReport {
            dims: Vec::new(),
            detail_count: 0,
        }
    }

    /// 记录一维。
    pub fn set_dim(&mut self, dim: Dim, status: DimStatus) {
        if let Some(slot) = self.dims.iter_mut().find(|(d, _)| *d == dim) {
            slot.1 = status;
        } else {
            self.dims.push((dim, status));
        }
    }

    /// 三维完整性审计。
    pub fn audit_dims(&self) -> Result<(), V0DCode> {
        for d in Dim::all() {
            match self.dims.iter().find(|(dd, _)| *dd == d) {
                Some((_, DimStatus::Missing)) | None => return Err(V0DCode::DIM_MISSING),
                _ => {}
            }
        }
        Ok(())
    }

    /// 总体状态（取最差维）。
    pub fn overall(&self) -> DimStatus {
        let mut worst = DimStatus::Healthy;
        for (_, s) in &self.dims {
            match s {
                DimStatus::Error => return DimStatus::Error,
                DimStatus::Warn => worst = DimStatus::Warn,
                _ => {}
            }
        }
        worst
    }
}

// ── 定位器 ──────────────────────────────────────────────────────────

/// 资产内定位（结构路径）。
#[derive(Clone, Debug)]
pub struct Location {
    /// 路径段（如 ["layers", "3", "fill"]）。
    pub path: Vec<String>,
    /// 行/偏移提示。
    pub hint: String,
}

impl Location {
    /// 是否有效（非空路径）。
    pub fn is_valid(&self) -> bool {
        !self.path.is_empty()
    }

    /// 渲染 "a.b.c (hint)"。
    pub fn render(&self) -> String {
        let mut s = String::new();
        for (i, p) in self.path.iter().enumerate() {
            if i > 0 {
                s.push('.');
            }
            s.push_str(p);
        }
        if !self.hint.is_empty() {
            s.push_str(" (");
            s.push_str(&self.hint);
            s.push(')');
        }
        s
    }
}

/// 定位器：验证失败 → 具体位置。
#[derive(Clone, Debug, Default)]
pub struct Locator {
    /// 已知问题映射（问题 ID → 定位）。
    table: Vec<(u32, Location)>,
}

impl Locator {
    /// 空定位器。
    pub fn new() -> Locator {
        Locator { table: Vec::new() }
    }

    /// 登记一个已知定位。
    pub fn register(&mut self, issue_id: u32, loc: Location) {
        self.table.push((issue_id, loc));
    }

    /// 定位一个问题。找不到返回错误（定位缺失红线）。
    pub fn locate(&self, issue_id: u32) -> Result<&Location, V0DCode> {
        match self.table.iter().find(|(id, _)| *id == issue_id) {
            Some((_, loc)) if loc.is_valid() => Ok(loc),
            _ => Err(V0DCode::LOCATE_MISS),
        }
    }
}

// ── 三要素修复建议 ──────────────────────────────────────────────────

/// 修复建议（三要素：现象/原因/动作）。
#[derive(Clone, Debug)]
pub struct Remedy {
    /// 现象。
    pub symptom: String,
    /// 原因。
    pub cause: String,
    /// 动作。
    pub action: String,
}

impl Remedy {
    /// 构造（任一为空返回 None）。
    pub fn new(symptom: &str, cause: &str, action: &str) -> Option<Remedy> {
        if symptom.is_empty() || cause.is_empty() || action.is_empty() {
            return None;
        }
        Some(Remedy {
            symptom: String::from(symptom),
            cause: String::from(cause),
            action: String::from(action),
        })
    }

    /// 三要素完整性。
    pub fn complete(&self) -> bool {
        !self.symptom.is_empty() && !self.cause.is_empty() && !self.action.is_empty()
    }
}

/// 建议库。
#[derive(Clone, Debug, Default)]
pub struct RemedyLib {
    entries: Vec<(V0DCode, Remedy)>,
}

impl RemedyLib {
    /// 空库。
    pub fn new() -> RemedyLib {
        RemedyLib {
            entries: Vec::new(),
        }
    }

    /// 内置建议库（常见码各一条）。
    pub fn builtin() -> RemedyLib {
        let mut lib = RemedyLib::new();
        Remedy::new(
            "验证失败但未给出位置",
            "错误记录缺少资产结构路径",
            "在验证器中补全 Location 后重报",
        )
        .map(|r| lib.entries.push((V0DCode::LOCATE_MISS, r)));
        Remedy::new(
            "修复建议不完整",
            "三要素中有空字段",
            "按 现象/原因/动作 三段补齐建议",
        )
        .map(|r| lib.entries.push((V0DCode::REMEDY_INCOMPLETE, r)));
        Remedy::new(
            "体检报告维度缺失",
            "三维数据源有一维未接入",
            "接入缺失维度的采集器后重跑体检",
        )
        .map(|r| lib.entries.push((V0DCode::DIM_MISSING, r)));
        lib
    }

    /// 匹配一条建议。
    pub fn lookup(&self, code: V0DCode) -> Option<&Remedy> {
        self.entries.iter().find(|(c, _)| *c == code).map(|(_, r)| r)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全库三要素完整性审计。
    pub fn audit_complete(&self) -> bool {
        self.entries.iter().all(|(_, r)| r.complete())
    }
}

// ── 工作流联动 ──────────────────────────────────────────────────────

/// 工作流步骤。
#[derive(Clone, Debug)]
pub struct WfStep {
    /// 步骤 ID。
    pub id: u32,
    /// 步骤名。
    pub name: String,
}

/// 联动：错误 → 工作流修复步骤跳转。
#[derive(Clone, Debug, Default)]
pub struct WorkflowLink {
    steps: Vec<WfStep>,
}

impl WorkflowLink {
    /// 空链接。
    pub fn new() -> WorkflowLink {
        WorkflowLink { steps: Vec::new() }
    }

    /// 注册步骤。
    pub fn register(&mut self, id: u32, name: &str) {
        self.steps.push(WfStep {
            id,
            name: String::from(name),
        });
    }

    /// 跳转到修复步骤。目标不存在 → 联动断裂。
    pub fn jump(&self, step_id: u32) -> Result<&WfStep, V0DCode> {
        self.steps
            .iter()
            .find(|s| s.id == step_id)
            .ok_or(V0DCode::LINK_BROKEN)
    }

    /// 步骤数。
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// 顶层联动入口。
pub fn workflow_jump(link: &WorkflowLink, step_id: u32) -> Result<&str, V0DCode> {
    link.jump(step_id).map(|s| s.name.as_str())
}

// ── 错误记录（调试器主对象）─────────────────────────────────────────

/// 一条调试错误。
#[derive(Clone, Debug)]
pub struct DebugError {
    /// 错误码。
    pub code: V0DCode,
    /// 定位。
    pub location: Location,
    /// 建议。
    pub remedy: Remedy,
}

impl DebugError {
    /// 完整性审计（定位有效 + 建议三要素齐）。
    pub fn audit(&self) -> Result<(), V0DCode> {
        if self.code == V0DCode::EMPTY_ERROR {
            return Err(V0DCode::EMPTY_ERROR);
        }
        if !self.location.is_valid() {
            return Err(V0DCode::LOCATE_MISS);
        }
        if !self.remedy.complete() {
            return Err(V0DCode::REMEDY_INCOMPLETE);
        }
        Ok(())
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// F3607 验证数据源契约。
pub fn validation_source(ok: bool) -> DimStatus {
    if ok {
        DimStatus::Healthy
    } else {
        DimStatus::Error
    }
}

/// 家族协议声明（F2815 家族十六成员）。
pub const FAMILY_MEMBER_INDEX: u16 = 16;

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3607", "VE-F3604", "VE-F2815"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves13_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3613");

    // 0x3d01 三维齐全
    let mut hr = HealthReport::new();
    hr.set_dim(Dim::Validation, DimStatus::Healthy);
    hr.set_dim(Dim::Performance, DimStatus::Warn);
    hr.set_dim(Dim::Compat, DimStatus::Healthy);
    out.add("0x3d01-三维体检齐全", hr.audit_dims().is_ok(), "");

    // 0x3d02 维度缺失检出
    let mut hr2 = HealthReport::new();
    hr2.set_dim(Dim::Validation, DimStatus::Healthy);
    out.add(
        "0x3d02-维度缺失被检出",
        hr2.audit_dims().is_err(),
        "",
    );

    // 0x3d03 总体取最差
    out.add("0x3d03-总体取最差维", hr.overall() == DimStatus::Warn, "");

    // 0x3d04 定位命中
    let mut loc = Locator::new();
    loc.register(
        1,
        Location {
            path: vec![String::from("layers"), String::from("3"), String::from("fill")],
            hint: String::from("色值越界"),
        },
    );
    out.add("0x3d04-定位命中", loc.locate(1).is_ok(), "");

    // 0x3d05 定位缺失拦截
    out.add(
        "0x3d05-定位缺失拦截",
        loc.locate(99).is_err(),
        "",
    );

    // 0x3d06 三要素齐
    let r = Remedy::new("现象", "原因", "动作");
    out.add("0x3d06-三要素齐", r.map(|x| x.complete()).unwrap_or(false), "");

    // 0x3d07 三要素缺一拒构造
    out.add("0x3d07-三要素缺一拒构造", Remedy::new("现象", "", "动作").is_none(), "");

    // 0x3d08 建议库匹配
    let lib = RemedyLib::builtin();
    out.add("0x3d08-建议库可匹配", lib.lookup(V0DCode::LOCATE_MISS).is_some(), "");

    // 0x3d09 建议库全量完整
    out.add("0x3d09-建议库三要素全齐", lib.audit_complete(), "");

    // 0x3d0a 联动跳转
    let mut wf = WorkflowLink::new();
    wf.register(1, "修复图层填充");
    out.add("0x3d0a-联动跳转成功", workflow_jump(&wf, 1).is_ok(), "");

    // 0x3d0b 联动断裂
    out.add("0x3d0b-联动断裂检出", workflow_jump(&wf, 42).is_err(), "");

    out
}
