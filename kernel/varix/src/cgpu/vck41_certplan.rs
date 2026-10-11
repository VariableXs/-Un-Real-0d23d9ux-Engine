//! CGPU-F1722 · 异构认证计划
//!
//! 判据锚点（施工书原文摘录）：
//! 认证：认证体系（异构认证（设备认证（能力达标/场景认证（五场景达标——认证两级
//! （认证判据（公开可复现——判据公开；流程（送测/测试/报告/徽标——流程（流程透明；
//! 维持（认证维持（驱动更新复测——维持机制；撤销（不达标撤销（撤销公开——撤销纪律；
//! 测试（两级/判据/流程/维持/撤销五组）。判据：两级、判据公开、流程、维持、撤销、
//! 五组、判据。
//!
//! 诊断码段 0xA501~0xA505（独占）：
//!   0xA501 认证两级缺项（设备/场景级任一判据缺席）
//!   0xA502 判据不公开（判据无公开锚/不可复现）
//!   0xA503 流程断裂（四站越序/回退/缺站）
//!   0xA504 维持失测（驱动更新后未触发复测）
//!   0xA505 撤销失公开（不达标撤销未公示）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const CERT_PLAN_VERSION: &str = "K41-cert-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA501~0xA505）
// ---------------------------------------------------------------------------

pub const XC_CE_LEVEL_MISSING: u32 = 0xA501;
pub const XC_CE_CRITERIA_CLOSED: u32 = 0xA502;
pub const XC_CE_FLOW_BROKEN: u32 = 0xA503;
pub const XC_CE_MAINT_LOST: u32 = 0xA504;
pub const XC_CE_REVOKE_HIDDEN: u32 = 0xA505;

// ---------------------------------------------------------------------------
// 一、认证两级（设备级 + 场景级）
// ---------------------------------------------------------------------------

/// 认证级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertLevel {
    /// 设备级：能力达标（能力清单全绿——挂 F1601 能力目录口径）。
    Device,
    /// 场景级：五场景达标（游戏/创作/生产力/AI PC/基准——挂 K08 场景族）。
    Scenario,
}

impl CertLevel {
    pub const ALL: [CertLevel; 2] = [CertLevel::Device, CertLevel::Scenario];

    pub const fn name(self) -> &'static str {
        match self {
            CertLevel::Device => "device",
            CertLevel::Scenario => "scenario",
        }
    }

    /// 该级判据项数下界（两级缺项检测用）。
    pub const fn min_criteria(self) -> u16 {
        match self {
            CertLevel::Device => 4,
            CertLevel::Scenario => 5,
        }
    }
}

/// 设备级判据四项（能力达标闭集）。
pub const DEVICE_CRITERIA: [&str; 4] =
    ["capability_catalog", "zero_unsafe", "no_std_alloc", "diag_code_domain"];

/// 场景级判据五项（五场景达标闭集）。
pub const SCENARIO_CRITERIA: [&str; 5] = ["game", "create", "productivity", "aipc", "bench"];

/// 两级判据总账：两级全在册且项数达下界。
pub struct LevelLedger;

impl LevelLedger {
    pub fn complete() -> Result<(), u32> {
        if (DEVICE_CRITERIA.len() as u16) < CertLevel::Device.min_criteria() {
            return Err(XC_CE_LEVEL_MISSING);
        }
        if (SCENARIO_CRITERIA.len() as u16) < CertLevel::Scenario.min_criteria() {
            return Err(XC_CE_LEVEL_MISSING);
        }
        for c in DEVICE_CRITERIA.iter() {
            if c.is_empty() {
                return Err(XC_CE_LEVEL_MISSING);
            }
        }
        for c in SCENARIO_CRITERIA.iter() {
            if c.is_empty() {
                return Err(XC_CE_LEVEL_MISSING);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、判据公开（公开可复现）
// ---------------------------------------------------------------------------

/// 判据公开账：每条判据有公开锚 + 第三方可复跑标志。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CriteriaEntry {
    pub name: &'static str,
    /// 公开锚（文档路径标识——判据不公开即失格）。
    pub doc_anchor: &'static str,
    /// 第三方可复跑（公开测试/脚本在册）。
    pub third_party_rerunnable: bool,
}

impl CriteriaEntry {
    pub fn open(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.doc_anchor.is_empty() || !self.third_party_rerunnable {
            return Err(XC_CE_CRITERIA_CLOSED);
        }
        Ok(())
    }
}

/// 全判据公开审计：设备四条 + 场景五条全公开。
pub fn criteria_open_audit(entries: &[CriteriaEntry], expect: usize) -> Result<(), u32> {
    if entries.len() < expect {
        return Err(XC_CE_CRITERIA_CLOSED);
    }
    for e in entries {
        e.open()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、认证流程（送测/测试/报告/徽标 四站单调）
// ---------------------------------------------------------------------------

/// 流程站闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertStage {
    Submit,
    Test,
    Report,
    Badge,
}

impl CertStage {
    pub const ALL: [CertStage; 4] =
        [CertStage::Submit, CertStage::Test, CertStage::Report, CertStage::Badge];

    /// 站序（单调推进用）。
    pub const fn ord(self) -> u8 {
        match self {
            CertStage::Submit => 0,
            CertStage::Test => 1,
            CertStage::Report => 2,
            CertStage::Badge => 3,
        }
    }
}

/// 流程状态机：只许按站序单调前进，越序/回退即断。
pub struct CertFlow {
    pub stage: CertStage,
}

impl CertFlow {
    pub fn begin() -> Self {
        CertFlow { stage: CertStage::Submit }
    }

    pub fn advance(&mut self, next: CertStage) -> Result<(), u32> {
        if next.ord() != self.stage.ord() + 1 {
            return Err(XC_CE_FLOW_BROKEN);
        }
        self.stage = next;
        Ok(())
    }

    /// 徽标发放前提：流程走满四站。
    pub fn badged(&self) -> Result<(), u32> {
        if self.stage != CertStage::Badge {
            return Err(XC_CE_FLOW_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、维持（驱动更新复测）与撤销（不达标撤销公开）
// ---------------------------------------------------------------------------

/// 维持机制：驱动版本变更必须触发复测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MaintRule {
    /// 认证时的驱动版本。
    pub cert_driver_ver: u32,
    /// 当前驱动版本。
    pub cur_driver_ver: u32,
    /// 复测完成标志。
    pub retested: bool,
}

impl MaintRule {
    /// 维持判：驱动未变或已复测——变更未复测即失维持。
    pub fn maintained(&self) -> Result<(), u32> {
        if self.cert_driver_ver != self.cur_driver_ver && !self.retested {
            return Err(XC_CE_MAINT_LOST);
        }
        if self.cert_driver_ver == 0 || self.cur_driver_ver == 0 {
            return Err(XC_CE_MAINT_LOST);
        }
        Ok(())
    }
}

/// 撤销纪律：不达标撤销 + 撤销公示（撤销不公开即双违）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RevokeRule {
    /// 是否触发撤销（复测不达标）。
    pub triggered: bool,
    /// 撤销是否已公示。
    pub published: bool,
    /// 撤销原因码（公示内容非空前提）。
    pub reason: &'static str,
}

impl RevokeRule {
    pub fn disclosed(&self) -> Result<(), u32> {
        if self.triggered {
            if !self.published || self.reason.is_empty() {
                return Err(XC_CE_REVOKE_HIDDEN);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：两级/判据/流程/维持/撤销）
// ---------------------------------------------------------------------------

/// 认证计划引擎。
pub struct CertEngine;

impl CertEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：两级判据总账。
        LevelLedger::complete()?;
        // 组二：判据公开——九条全公开。
        let mut entries: [CriteriaEntry; 9] = [
            CriteriaEntry { name: DEVICE_CRITERIA[0], doc_anchor: "docs/cert/dev-0", third_party_rerunnable: true },
            CriteriaEntry { name: DEVICE_CRITERIA[1], doc_anchor: "docs/cert/dev-1", third_party_rerunnable: true },
            CriteriaEntry { name: DEVICE_CRITERIA[2], doc_anchor: "docs/cert/dev-2", third_party_rerunnable: true },
            CriteriaEntry { name: DEVICE_CRITERIA[3], doc_anchor: "docs/cert/dev-3", third_party_rerunnable: true },
            CriteriaEntry { name: SCENARIO_CRITERIA[0], doc_anchor: "docs/cert/scn-0", third_party_rerunnable: true },
            CriteriaEntry { name: SCENARIO_CRITERIA[1], doc_anchor: "docs/cert/scn-1", third_party_rerunnable: true },
            CriteriaEntry { name: SCENARIO_CRITERIA[2], doc_anchor: "docs/cert/scn-2", third_party_rerunnable: true },
            CriteriaEntry { name: SCENARIO_CRITERIA[3], doc_anchor: "docs/cert/scn-3", third_party_rerunnable: true },
            CriteriaEntry { name: SCENARIO_CRITERIA[4], doc_anchor: "docs/cert/scn-4", third_party_rerunnable: true },
        ];
        criteria_open_audit(&entries, 9)?;
        // 反例：判据不公开（锚空/不可复跑）拒。
        entries[4] = CriteriaEntry { name: "game", doc_anchor: "", third_party_rerunnable: true };
        if criteria_open_audit(&entries, 9).is_ok() {
            return Err(XC_CE_CRITERIA_CLOSED);
        }
        // 组三：流程四站单调走满 + 越序拒。
        let mut flow = CertFlow::begin();
        flow.advance(CertStage::Test)?;
        flow.advance(CertStage::Report)?;
        flow.advance(CertStage::Badge)?;
        flow.badged()?;
        let mut jump = CertFlow::begin();
        if jump.advance(CertStage::Report).is_ok() || jump.advance(CertStage::Submit).is_ok() {
            return Err(XC_CE_FLOW_BROKEN);
        }
        // 组四：维持——驱动变更未复测拒，复测后过。
        let stale = MaintRule { cert_driver_ver: 10, cur_driver_ver: 11, retested: false };
        if stale.maintained().is_ok() {
            return Err(XC_CE_MAINT_LOST);
        }
        MaintRule { cert_driver_ver: 10, cur_driver_ver: 11, retested: true }.maintained()?;
        MaintRule { cert_driver_ver: 10, cur_driver_ver: 10, retested: false }.maintained()?;
        // 组五：撤销公示——触发未公示拒，公示后过。
        if (RevokeRule { triggered: true, published: false, reason: "复测不达标" })
            .disclosed()
            .is_ok()
        {
            return Err(XC_CE_REVOKE_HIDDEN);
        }
        RevokeRule { triggered: true, published: true, reason: "复测不达标" }.disclosed()?;
        RevokeRule { triggered: false, published: false, reason: "" }.disclosed()?;
        Ok(())
    }

    /// 段对账：五码全落 0xA500..0xA600 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CE_LEVEL_MISSING,
            XC_CE_CRITERIA_CLOSED,
            XC_CE_FLOW_BROKEN,
            XC_CE_MAINT_LOST,
            XC_CE_REVOKE_HIDDEN,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA500 || codes[i] >= 0xA600 {
                return Err(XC_CE_LEVEL_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_CE_LEVEL_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
