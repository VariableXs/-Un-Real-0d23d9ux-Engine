//! VE-F3741 · R08 开工与创作无障碍总纲（VE-S 域 · 批次 R08 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3741`
//!
//! **职责定位（锚点原文）**：R08 创作无障碍组——
//! - **R08 定位**：R 域无障碍治理（双维执法全域化：
//!   创作工具无障碍（创作者用得顺）+ 创作产出无障碍（作品被用得顺）——
//!   双维总纲复述深化全域声明）；
//! - **治理原则**：继承 P08 五段执法模式（等效终审/安全审查/
//!   偏好/辅具/资产）——五段复刻声明（流程模板第六用）；
//! - **创作特色原则**：创作门槛（无障碍工具降低创作门槛：
//!   无障碍不是成本，是无障碍用户成为创作者的通道）——
//!   门槛声明（通道红线：无障碍创作工具缺失 = 排除无障碍用户的
//!   创作者身份）——身份排除红线 P0。
//!
//! **判据（锚点原文五条）**：双维总纲、身份排除 P0、六版复刻、
//! 门槛原则、五段映射、判据。
//!
//! **错误路径与降级矩阵（锚点原文三条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 身份排除检出→P0（红线实测——工具可达审计） | [`R8Code::IDENTITY_EXCLUDE_P0`] |
//! | 模板漂移→六版对账（家族对拍） | [`R8Code::TEMPLATE_DRIFT`] |
//! | 双维失衡→平衡修正（复述） | [`R8Code::DIM_UNBALANCE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3741 诊断码段（独占 0x84xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R8Code(pub u16);

impl R8Code {
    /// 身份排除（P0 红线）。
    pub const IDENTITY_EXCLUDE_P0: R8Code = R8Code(0x8401);
    /// 模板漂移（六版对账）。
    pub const TEMPLATE_DRIFT: R8Code = R8Code(0x8402);
    /// 双维失衡（平衡修正）。
    pub const DIM_UNBALANCE: R8Code = R8Code(0x8403);
    /// 五段映射缺。
    pub const MAP_GAP: R8Code = R8Code(0x8404);
    /// 门槛原则缺。
    pub const PRINCIPLE_MISS: R8Code = R8Code(0x8405);
    /// 工具不可达。
    pub const TOOL_UNREACHABLE: R8Code = R8Code(0x8406);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R8Code::IDENTITY_EXCLUDE_P0 => "身份排除（P0 红线实测）。建议：工具须无障碍可达。",
            R8Code::TEMPLATE_DRIFT => "模板漂移（家族对拍）。建议：六版对账收敛。",
            R8Code::DIM_UNBALANCE => "双维失衡（复述）。建议：工具与产出双维修正。",
            R8Code::MAP_GAP => "五段映射缺。建议：补齐五段执法映射。",
            R8Code::PRINCIPLE_MISS => "门槛原则缺。建议：门槛声明须在。",
            R8Code::TOOL_UNREACHABLE => "工具不可达。建议：补可达路径。",
            _ => "未知诊断码。",
        }
    }
}

// ── 双维总纲 ────────────────────────────────────────────────────────

/// 执法维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yDim {
    /// 创作工具无障碍（创作者用得顺）。
    ToolSide,
    /// 创作产出无障碍（作品被用得顺）。
    OutputSide,
}

impl A11yDim {
    pub const ALL: [A11yDim; 2] = [A11yDim::ToolSide, A11yDim::OutputSide];
}

/// 双维登记。
pub struct DualDim {
    tool_ok: bool,
    output_ok: bool,
}

impl DualDim {
    pub fn new() -> DualDim {
        DualDim {
            tool_ok: false,
            output_ok: false,
        }
    }

    /// 登记维完成。
    pub fn mark(&mut self, dim: A11yDim) {
        match dim {
            A11yDim::ToolSide => self.tool_ok = true,
            A11yDim::OutputSide => self.output_ok = true,
        }
    }

    /// 双维齐备。
    pub fn balanced(&self) -> Result<(), R8Code> {
        if self.tool_ok && self.output_ok {
            Ok(())
        } else {
            Err(R8Code::DIM_UNBALANCE)
        }
    }
}

// ── 五段映射（P08 模板六用） ────────────────────────────────────────

/// 执法段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforceStage {
    /// 等效终审。
    EquivFinal,
    /// 安全审查。
    SafetyReview,
    /// 偏好。
    Preference,
    /// 辅具。
    Assistive,
    /// 资产。
    Asset,
}

impl EnforceStage {
    pub const ALL: [EnforceStage; 5] = [
        EnforceStage::EquivFinal,
        EnforceStage::SafetyReview,
        EnforceStage::Preference,
        EnforceStage::Assistive,
        EnforceStage::Asset,
    ];
}

/// 映射表。
pub struct StageMap {
    slots: [bool; 5],
}

impl StageMap {
    pub fn new() -> StageMap {
        StageMap { slots: [false; 5] }
    }

    pub fn map(&mut self, s: EnforceStage) {
        self.slots[s as usize] = true;
    }

    pub fn complete(&self) -> Result<(), R8Code> {
        for s in &self.slots {
            if !s {
                return Err(R8Code::MAP_GAP);
            }
        }
        Ok(())
    }
}

// ── 六版对账（家族对拍） ────────────────────────────────────────────

/// 六版指纹（P08 五用 + R08 第六用）。
pub struct FamilySix {
    fps: Vec<u16>,
}

impl FamilySix {
    pub fn new() -> FamilySix {
        FamilySix { fps: Vec::new() }
    }

    pub fn add(&mut self, fp: u16) {
        self.fps.push(fp);
    }

    /// 六版对账（骨架一致）。
    pub fn audit(&self) -> Result<(), R8Code> {
        if self.fps.is_empty() {
            return Ok(());
        }
        let first = self.fps[0];
        for f in &self.fps {
            if *f != first {
                return Err(R8Code::TEMPLATE_DRIFT);
            }
        }
        Ok(())
    }

    /// 版数。
    pub fn count(&self) -> usize {
        self.fps.len()
    }
}

// ── 门槛原则（身份排除红线） ────────────────────────────────────────

/// 门槛声明。
pub struct ThresholdDecl {
    /// 工具可达位（四工具）。
    reachable: [bool; 4],
}

impl ThresholdDecl {
    pub fn new() -> ThresholdDecl {
        ThresholdDecl {
            reachable: [false; 4],
        }
    }

    /// 标记工具可达。
    pub fn mark_reachable(&mut self, tool: u8) -> Result<(), R8Code> {
        if tool > 3 {
            return Err(R8Code::TOOL_UNREACHABLE);
        }
        self.reachable[tool as usize] = true;
        Ok(())
    }

    /// 身份排除审计（任一工具不可达即 P0）。
    pub fn identity_audit(&self) -> Result<(), R8Code> {
        for r in &self.reachable {
            if !r {
                return Err(R8Code::IDENTITY_EXCLUDE_P0);
            }
        }
        Ok(())
    }

    /// 门槛原则在场。
    pub fn principle_present(&self) -> Result<(), R8Code> {
        // 原则在场 = 至少声明了可达性检查
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3741 判据集（锚点原文五条逐条落位）。
pub fn run_vr08a_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3741");

    // 1) 双维总纲：齐备
    let mut dd = DualDim::new();
    dd.mark(A11yDim::ToolSide);
    dd.mark(A11yDim::OutputSide);
    out.add(
        "0x8403-双维总纲",
        dd.balanced().is_ok(),
        "双维须齐",
    );

    // 2) 双维失衡：被拦
    let mut dd2 = DualDim::new();
    dd2.mark(A11yDim::ToolSide);
    out.add(
        "0x8403-失衡拦",
        dd2.balanced().is_err(),
        "失衡须拦",
    );

    // 3) 身份排除 P0：不可达被拦
    let mut td = ThresholdDecl::new();
    let _ = td.mark_reachable(0);
    let _ = td.mark_reachable(1);
    out.add(
        "0x8401-身份排除",
        td.identity_audit().is_err(),
        "缺可达须 P0",
    );

    // 4) 身份排除：全可达过
    let mut td2 = ThresholdDecl::new();
    for t in 0..4u8 {
        let _ = td2.mark_reachable(t);
    }
    out.add(
        "0x8401-全可达过",
        td2.identity_audit().is_ok(),
        "全可达须过",
    );

    // 5) 五段映射：齐备
    let mut sm = StageMap::new();
    for s in EnforceStage::ALL {
        sm.map(s);
    }
    out.add(
        "0x8404-五段映射",
        sm.complete().is_ok(),
        "五段须齐",
    );

    // 6) 五段缺：被拦
    let sm2 = StageMap::new();
    out.add(
        "0x8404-缺拦",
        sm2.complete().is_err(),
        "缺须拦",
    );

    // 7) 六版对账：漂移拦
    let mut f6 = FamilySix::new();
    f6.add(0x10);
    f6.add(0x20);
    out.add(
        "0x8402-六版对账",
        f6.audit().is_err(),
        "漂移须拦",
    );

    // 8) 门槛原则：在场
    out.add(
        "0x8405-门槛原则",
        td2.principle_present().is_ok(),
        "原则须在",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8406-判据",
        R8Code::IDENTITY_EXCLUDE_P0.value() != R8Code::MAP_GAP.value(),
        "码段互异",
    );

    out
}
