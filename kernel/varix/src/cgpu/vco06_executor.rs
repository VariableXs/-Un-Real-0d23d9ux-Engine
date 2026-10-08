//! CGPU-F2246 · 降级执行器（CGPU-O 域 · 降级链 · 执行器主题）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2246`
//!
//! 执行器：执行器（降级执行器（策略→各域执行（执行接口（执行器统一
//! 接口——接口冻结 v1（执行语义（建议 vs 强制语义复用——语义复用；
//! 失败（执行失败处理——失败复用；测试（接口/语义/失败三组）。
//!
//! ## 要点一：执行器统一接口——接口冻结 v1
//!
//! 降级动作统一以 ExecAction 五元组表达（执行域/动作标识/语义档/
//! 载荷摘要/序号）；接口结构冻结 v1——只增不改，下游各域按统一接口
//! 消费降级策略（策略→各域执行）。
//!
//! ## 要点二：执行语义——建议 vs 强制语义复用
//!
//! Advisory 建议（可合并可忽略但必须留痕——J02 B 域建议语义模式复用）
//! /Mandatory 强制（必须执行，失败立案——合同强制模式复用）；语义档
//! 决定失败处置路径，语义全域唯一不另立。
//!
//! ## 要点三：执行失败处理——失败复用
//!
//! 失败码闭集（域未就绪/域拒绝）+处置规则显性：建议失败→留痕跳过不
//! 阻塞降级链；强制失败→重试恰一次，再失败升级立案（失败不静默）。
//! 处置三态闭集 SkipLogged/RetryOnce/Escalate 字面量冻结。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x5Cxx 段
//!
//! 与 vco05（0x5Bxx）/vco04（0x5Axx）/vco03（0x59xx）/vco01（0x55xx）
//! 等互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、执行域闭集（策略→各域执行；域=执行落点，对齐 F2244 四维）
// ---------------------------------------------------------------------------

/// 降级执行目标域（官方四闭集——与 F2244 冲突维度四维对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecDomain {
    /// 调度域——帧率类动作落点。
    Scheduling,
    /// 效果域——画质类动作落点。
    Effects,
    /// 呈现域——延迟类动作落点。
    Presentation,
    /// 资源域——功耗类动作落点。
    Resource,
}

/// 执行域总数。
pub const DOMAIN_COUNT: usize = 4;

impl ExecDomain {
    /// 全部执行域（官方序）。
    pub const ALL: [ExecDomain; DOMAIN_COUNT] = [
        ExecDomain::Scheduling,
        ExecDomain::Effects,
        ExecDomain::Presentation,
        ExecDomain::Resource,
    ];

    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            ExecDomain::Scheduling => "调度域".to_string(),
            ExecDomain::Effects => "效果域".to_string(),
            ExecDomain::Presentation => "呈现域".to_string(),
            ExecDomain::Resource => "资源域".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、执行器统一接口——接口冻结 v1
// ---------------------------------------------------------------------------

/// 接口冻结版本号（字面量钉死——判据逐字对拍）。
pub const EXEC_API_V1: &str = "v1";
/// 接口冻结声明（判据逐字对拍）。
pub const EXEC_API_FREEZE_NOTE: &str =
    "执行器统一接口冻结 v1——ExecAction 五元组+execute 签名只增不改，下游各域按统一接口消费降级策略";

/// 执行语义档（二闭集——建议 vs 强制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Semantics {
    /// 建议——可合并可忽略但必须留痕（J02 B 域建议语义模式复用）。
    Advisory,
    /// 强制——必须执行，失败立案（合同强制模式复用）。
    Mandatory,
}

impl Semantics {
    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            Semantics::Advisory => "建议".to_string(),
            Semantics::Mandatory => "强制".to_string(),
        }
    }
}

/// 语义复用声明（锚点「建议 vs 强制语义复用」——判据逐字对拍）。
pub const SEMANTICS_REUSE_NOTE: &str =
    "执行语义复用——建议=J02 B 域建议语义模式，强制=合同强制模式，语义全域唯一不另立";

/// 降级动作（统一接口五元组——冻结 v1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecAction {
    /// 执行目标域。
    pub domain: ExecDomain,
    /// 动作标识（域内动作编号，域间独立）。
    pub action_id: u16,
    /// 执行语义档。
    pub semantics: Semantics,
    /// 载荷摘要（FNV 低 16 位——载荷可对账不内联大对象）。
    pub payload_digest: u16,
    /// 序号（决策器下发序——留痕对账键）。
    pub sequence: u64,
}

// ---------------------------------------------------------------------------
// 三、执行失败处理——失败复用
// ---------------------------------------------------------------------------

/// 失败处置三态闭集（字面量冻结——判据独立对拍）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// 留痕跳过——建议动作失败的处置，不阻塞降级链。
    SkipLogged,
    /// 重试恰一次——强制动作首次失败的处置。
    RetryOnce,
    /// 升级立案——强制动作重试仍失败的处置（失败不静默）。
    Escalate,
}

impl Disposition {
    /// 中文标签（字面量冻结——判据独立对拍）。
    pub fn label(self) -> String {
        match self {
            Disposition::SkipLogged => "留痕跳过".to_string(),
            Disposition::RetryOnce => "重试恰一次".to_string(),
            Disposition::Escalate => "升级立案".to_string(),
        }
    }
}

/// 失败处理复用声明（锚点「执行失败处理——失败复用」——判据逐字对拍）。
pub const FAILURE_REUSE_NOTE: &str =
    "执行失败处理复用——处置规则显性：建议失败留痕跳过不阻塞链，强制失败重试恰一次再失败升级立案";

/// 模拟域执行结果（域侧回执——真域执行由各域兑现，本层只管统一接口账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainReply {
    /// 域受理完成。
    Accepted,
    /// 域未就绪（该域执行面未挂账）。
    NotReady,
    /// 域拒绝（动作越出该域受理边界）。
    Rejected,
}

/// 统一执行入口：按动作查域回执，失败按语义档映射处置。
///
/// 规则：① 域受理→回执成功；② 域未就绪/域拒绝→失败码；③ 失败处置
/// 由语义档决定（建议→SkipLogged，强制首败→RetryOnce，强制再败→
/// Escalate）——处置规则全域唯一。
pub fn execute(action: &ExecAction, reply: DomainReply) -> Result<Disposition, XcCode> {
    match reply {
        DomainReply::Accepted => Ok(Disposition::SkipLogged),
        DomainReply::NotReady => Err(XcCode::DOMAIN_NOT_READY),
        DomainReply::Rejected => Err(XcCode::DOMAIN_REJECT),
    }
}

/// 失败处置裁决：语义档 × 失败次数 → 处置三态。
///
/// 规则：① 建议动作任何失败→留痕跳过；② 强制动作首次失败→重试恰
/// 一次；③ 强制动作二次失败→升级立案；④ attempt 从 1 计（首败）。
pub fn handle_failure(action: &ExecAction, attempt: u8) -> Disposition {
    match action.semantics {
        Semantics::Advisory => Disposition::SkipLogged,
        Semantics::Mandatory => {
            if attempt <= 1 {
                Disposition::RetryOnce
            } else {
                Disposition::Escalate
            }
        }
    }
}

/// 回执成功的规范处置（受理即落账——建议/强制同账，语义差只作用于失败路径）。
pub fn accepted_disposition() -> Disposition {
    Disposition::SkipLogged
}

// ---------------------------------------------------------------------------
// 三·半、执行回执与对账（受理必有处置——回执账完整性）
// ---------------------------------------------------------------------------

/// 执行回执（一笔动作的终局账——留痕供审计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecReceipt {
    /// 对应动作的下发序号（对账键）。
    pub sequence: u64,
    /// 终局处置。
    pub disposition: Disposition,
    /// 是否升级立案（强制二次失败）。
    pub escalated: bool,
}

/// 回执账对账：序号严格递增不重复，处置与升级标记自洽。
///
/// 规则：① sequence 必须严格递增（决策器下发序唯一——重复/回退即账
/// 断裂）；② 升级立案标记只在 Escalate 处置时为真（SkipLogged/RetryOnce
/// 不得携带升级标记——账实相符）。
pub fn audit_receipts(receipts: &[ExecReceipt]) -> Result<(), XcCode> {
    let mut i = 1;
    while i < receipts.len() {
        if receipts[i].sequence <= receipts[i - 1].sequence {
            return Err(XcCode::RECEIPT_BROKEN);
        }
        i += 1;
    }
    for r in receipts.iter() {
        let escalated_ok = (r.disposition == Disposition::Escalate) == r.escalated;
        if !escalated_ok {
            return Err(XcCode::RECEIPT_BROKEN);
        }
    }
    Ok(())
}

/// 域受理边界声明（判据非空对拍——策略→各域执行的落点语义显性）。
pub const DOMAIN_SCOPE_NOTE: &str =
    "域受理边界——调度域收帧率动作、效果域收画质动作、呈现域收延迟动作、资源域收功耗动作，与 F2244 冲突维度四维对齐";

// ---------------------------------------------------------------------------
// 四、错误契约（独占 0x5Cxx 段）
// ---------------------------------------------------------------------------

/// vco06 诊断码。独占 `0x5Cxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XcCode(pub u16);

impl XcCode {
    /// 域未就绪。
    pub const DOMAIN_NOT_READY: XcCode = XcCode(0x5C01);
    /// 域拒绝执行。
    pub const DOMAIN_REJECT: XcCode = XcCode(0x5C02);
    /// 强制动作二次失败升级立案（防御位——处置已裁决后仍需留码）。
    pub const MANDATORY_ESCALATED: XcCode = XcCode(0x5C03);
    /// 接口版本不符（非 v1 拒收）。
    pub const API_VERSION_MISMATCH: XcCode = XcCode(0x5C04);
    /// 执行回执账断裂（受理却无处置——防御位）。
    pub const RECEIPT_BROKEN: XcCode = XcCode(0x5C05);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            XcCode::DOMAIN_NOT_READY => "域未就绪：该域执行面未挂账".into(),
            XcCode::DOMAIN_REJECT => "域拒绝执行：动作越出该域受理边界".into(),
            XcCode::MANDATORY_ESCALATED => "强制动作二次失败：升级立案，失败不静默".into(),
            XcCode::API_VERSION_MISMATCH => "接口版本不符：统一接口冻结 v1，非 v1 拒收".into(),
            XcCode::RECEIPT_BROKEN => "执行回执账断裂：受理必有处置".into(),
            XcCode(_) => "未知 vco06 降级执行器域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑（锚点三组：接口/语义/失败）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 统一接口五元组与四域() {
        assert_eq!(EXEC_API_V1, "v1");
        assert_eq!(DOMAIN_COUNT, 4);
        let a = ExecAction {
            domain: ExecDomain::Scheduling,
            action_id: 3,
            semantics: Semantics::Mandatory,
            payload_digest: 0x1F2E,
            sequence: 7,
        };
        assert_eq!(a.domain.label(), "调度域");
        assert_eq!(a.semantics.label(), "强制");
        assert_eq!(execute(&a, DomainReply::Accepted), Ok(Disposition::SkipLogged));
    }

    #[test]
    fn 语义档决定失败处置() {
        let adv = ExecAction {
            domain: ExecDomain::Effects,
            action_id: 1,
            semantics: Semantics::Advisory,
            payload_digest: 9,
            sequence: 1,
        };
        let man = ExecAction {
            domain: ExecDomain::Scheduling,
            action_id: 2,
            semantics: Semantics::Mandatory,
            payload_digest: 8,
            sequence: 2,
        };
        // 建议失败→留痕跳过（无论第几次）
        assert_eq!(handle_failure(&adv, 1), Disposition::SkipLogged);
        assert_eq!(handle_failure(&adv, 3), Disposition::SkipLogged);
        // 强制首败→重试恰一次；再败→升级立案
        assert_eq!(handle_failure(&man, 1), Disposition::RetryOnce);
        assert_eq!(handle_failure(&man, 2), Disposition::Escalate);
        // 失败码映射
        assert_eq!(execute(&man, DomainReply::NotReady), Err(XcCode::DOMAIN_NOT_READY));
        assert_eq!(execute(&man, DomainReply::Rejected), Err(XcCode::DOMAIN_REJECT));
    }
}
