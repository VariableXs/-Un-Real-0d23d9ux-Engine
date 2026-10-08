//! CGPU-F2721 · R 域开工与安全渲染总架构（CGPU-R 域 · 安全渲染 · 批次 R01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2721`
//!
//! 域开工：R 域开工（签收 Q10 移交包——安全渲染衔接包签收，衔接确认）；
//! 安全渲染定位（**不可信内容渲染防线**——定位声明）；四段架构（验证→
//! 隔离→降级→恢复）；Q10 预告兑现（可靠性异常→安全渲染降级接口——兑现
//! 确认）；测试（签收/定位/四段/兑现四组）。
//!
//! ## 要点一：定位是防线的边界
//!
//! "不可信内容渲染防线"两条款字面量冻结：防线管渲染路径上的恶意与
//! 异常内容（资源内嵌数据/越界描述/超限负载），防线不管渲染结果的
//! 业务正确性（那是数据验证域 R02 的事）——边界外扩即越权，内收即
//! 失防。
//!
//! ## 要点二：签收是衔接确认不是过场
//!
//! Q10 移交包（F2720 收官宣告所列）签收为结构化记录：七件+安全渲染
//! 接口就绪+72h 彩排启动逐项登记——签收缺项即域未开工。
//!
//! ## 要点三：四段单向且降级是恢复的前置
//!
//! 验证→隔离→降级→恢复：不可信内容先验证（不过闸不进管线）、验证
//! 失败或可疑先隔离（不污染可信资源）、隔离后按档降级（安全档渲染）、
//! 最后恢复（回到正常路径或显性失败）。跳段与回退均违约——"没隔离
//! 就恢复"等于把污染带回来。
//!
//! ## 要点四：兑现是确认不是假设
//!
//! Q10 预告的"可靠性异常→安全渲染降级接口"以结构化兑现记录确认：
//! 接口就绪状态、对接任务号、指纹——预告没有兑现记录就是空头支票。
//!
//! ## 要点五：诊断码独占 0x57xx 段
//!
//! 与 F1121（0x50xx）/cga02（0x51xx 实测）/F1441（0x52xx）/L01（0x53xx）/
//! M01（0x54xx）/O01（0x55xx）/Q01（0x56xx）/F0001（0x39xx 细分）互不
//! 重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、域定位：不可信内容渲染防线（字面量冻结）
// ---------------------------------------------------------------------------

/// R 域定位两条款（字面量冻结——判据独立对拍）。
pub const POSITION_CLAUSES: [&str; 2] = [
    "不可信内容渲染防线——防线管渲染路径上的恶意与异常内容",
    "防线不管渲染结果的业务正确性——边界外即他域职责",
];

/// R 域起始任务号（本单）。
pub const R_DOMAIN_FIRST: u32 = 2721;
/// R 域终止任务号（含）。
pub const R_DOMAIN_LAST: u32 = 2880;
/// 域任务总数守恒（10 组 × 16 项）。
pub const R_DOMAIN_TOTAL: u32 = 160;

// ---------------------------------------------------------------------------
// 二、Q10 移交包签收（衔接确认）
// ---------------------------------------------------------------------------

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收/已兑现。
    Settled,
    /// 待兑现。
    Pending,
}

/// Q10 移交包签收记录（安全渲染衔接包——衔接确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（Q 域收官宣告）。
    pub source: u32,
    /// 移交包内容项数（七件）。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// 移交包七件（F2720 收官宣告口径——判据独立对拍）。
pub const HANDOVER_ITEMS: [&str; 7] = [
    "接口冻结清单",
    "契约清单",
    "资产清单",
    "基线快照",
    "遗留移交清单",
    "安全渲染衔接包（可靠性异常→安全渲染降级接口草案）",
    "经验教训",
];

/// Q10 移交包签收（字面量钉死——判据独立写死）。
pub const Q10_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 2720,
    items: 7,
    status: RecordStatus::Settled,
};

/// 附带交付宣告（七件之外的两项：接口就绪+彩排启动）。
pub const HANDOVER_ANNEX: [&str; 2] = ["安全渲染接口就绪", "72h 彩排启动"];

// ---------------------------------------------------------------------------
// 三、四段架构（单向流水线：验证→隔离→降级→恢复）
// ---------------------------------------------------------------------------

/// 安全渲染四段（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 验证（不可信内容过闸）。
    Verify,
    /// 隔离（可疑内容不污染可信资源）。
    Isolate,
    /// 降级（安全档渲染）。
    Degrade,
    /// 恢复（回正常路径或显性失败）。
    Recover,
}

impl PipelineStage {
    /// 全部四段（单向序）。
    pub const ALL: [PipelineStage; 4] = [
        PipelineStage::Verify,
        PipelineStage::Isolate,
        PipelineStage::Degrade,
        PipelineStage::Recover,
    ];

    /// 段下标。
    pub const fn index(self) -> usize {
        match self {
            PipelineStage::Verify => 0,
            PipelineStage::Isolate => 1,
            PipelineStage::Degrade => 2,
            PipelineStage::Recover => 3,
        }
    }

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PipelineStage::Verify => "验证",
            PipelineStage::Isolate => "隔离",
            PipelineStage::Degrade => "降级",
            PipelineStage::Recover => "恢复",
        }
        .to_string()
    }
}

/// 相邻段迁移合法性（只许 i → i+1；跳段/回退均非法）。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> bool {
    to.index() == from.index() + 1
}

// ---------------------------------------------------------------------------
// 四、Q10 预告兑现确认（兑现记录——预告没有兑现记录就是空头支票）
// ---------------------------------------------------------------------------

/// 兑现记录（Q 域预告→R 域确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FulfillmentRecord {
    /// 预告来源任务号。
    pub from: u32,
    /// 兑现内容。
    pub what: &'static str,
    /// 对接任务号（本域承载兑现的单）。
    pub land_at: u32,
    /// 状态（本单确认即 Settled）。
    pub status: RecordStatus,
}

/// Q10 预告兑现确认（字面量钉死——判据独立对拍）。
pub const Q10_FULFILLMENT: FulfillmentRecord = FulfillmentRecord {
    from: 2720,
    what: "可靠性异常→安全渲染降级接口",
    land_at: 2722,
    status: RecordStatus::Settled,
};

// ---------------------------------------------------------------------------
// 五、防线边界表（定位声明的可机检面：职责入表/出表闭集）
// ---------------------------------------------------------------------------

/// 防线边界条目（职责闭集——表外职责即越界 POSITION_OUT_OF_SCOPE）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEntry {
    /// 职责描述。
    pub what: &'static str,
    /// true=防线管（入表），false=他域管（出表）。
    pub in_scope: bool,
    /// 出表职责的承接域（入表条目为空串）。
    pub owner: &'static str,
}

/// 防线边界表（判据独立对拍——入表管渲染路径安全、出表显性移交）。
pub const SCOPE_TABLE: [ScopeEntry; 8] = [
    ScopeEntry { what: "资源内嵌恶意数据在渲染路径上的拦截", in_scope: true, owner: "" },
    ScopeEntry { what: "越界几何/属性描述的拒绝与立案", in_scope: true, owner: "" },
    ScopeEntry { what: "超限负载（着色器复杂度/纹理尺寸）降级", in_scope: true, owner: "" },
    ScopeEntry { what: "隔离区渲染（可疑内容不触达可信资源）", in_scope: true, owner: "" },
    ScopeEntry { what: "渲染结果的业务正确性", in_scope: false, owner: "R02 渲染数据验证" },
    ScopeEntry { what: "内容来源的网络安全（传输层）", in_scope: false, owner: "N 域" },
    ScopeEntry { what: "显存越权访问", in_scope: false, owner: "H 域显存预算池" },
    ScopeEntry { what: "系统级崩溃恢复", in_scope: false, owner: "Q 域" },
];

/// 职责裁决：在表内查职责归属——查无此职责或出表职责由防线承接均违约。
pub fn scope_of(what: &str) -> Result<bool, VrCode> {
    let mut i = 0;
    while i < SCOPE_TABLE.len() {
        if SCOPE_TABLE[i].what == what {
            if SCOPE_TABLE[i].in_scope {
                return Ok(true);
            }
            return Err(VrCode::POSITION_OUT_OF_SCOPE);
        }
        i += 1;
    }
    Err(VrCode::POSITION_OUT_OF_SCOPE)
}

// ---------------------------------------------------------------------------
// 六、错误契约（独占 0x57xx 段）
// ---------------------------------------------------------------------------

/// R01 诊断码。独占 `0x57xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VrCode(pub u16);

impl VrCode {
    /// 定位越界（防线声明表外职责）。
    pub const POSITION_OUT_OF_SCOPE: VrCode = VrCode(0x5701);
    /// 签收缺项（移交包未签收即域未开工）。
    pub const RECEIPT_INCOMPLETE: VrCode = VrCode(0x5702);
    /// 流水线违约（跳段/回退）。
    pub const STAGE_VIOLATION: VrCode = VrCode(0x5703);
    /// 兑现悬空（预告无兑现记录）。
    pub const FULFILLMENT_MISSING: VrCode = VrCode(0x5704);
    /// 隔离缺位（未隔离即恢复——污染回流）。
    pub const ISOLATION_SKIPPED: VrCode = VrCode(0x5705);
    /// 域区间越界。
    pub const DOMAIN_RANGE_INVALID: VrCode = VrCode(0x5706);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VrCode::POSITION_OUT_OF_SCOPE => "定位越界：防线不承接声明外职责".into(),
            VrCode::RECEIPT_INCOMPLETE => "移交包签收缺项：域未开工".into(),
            VrCode::STAGE_VIOLATION => "流水线违约：四段只许单向相邻迁移".into(),
            VrCode::FULFILLMENT_MISSING => "兑现悬空：Q10 预告无兑现记录".into(),
            VrCode::ISOLATION_SKIPPED => "隔离缺位：未隔离即恢复=污染回流".into(),
            VrCode::DOMAIN_RANGE_INVALID => "域区间越界：R 域 2721~2880 表外".into(),
            VrCode(_) => "未知 R01 安全渲染域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 签收七件齐且已定() {
        assert_eq!(Q10_HANDOVER.source, 2720);
        assert_eq!(Q10_HANDOVER.items, HANDOVER_ITEMS.len() as u32);
        assert_eq!(Q10_HANDOVER.status, RecordStatus::Settled);
        assert_eq!(HANDOVER_ANNEX.len(), 2);
    }

    #[test]
    fn 四段单向链() {
        for i in 0..3 {
            assert!(stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
        }
        assert!(!stage_transition(PipelineStage::Verify, PipelineStage::Degrade));
        assert!(!stage_transition(PipelineStage::Recover, PipelineStage::Isolate));
    }

    #[test]
    fn 兑现已确认() {
        assert_eq!(Q10_FULFILLMENT.from, 2720);
        assert_eq!(Q10_FULFILLMENT.land_at, 2722);
        assert_eq!(Q10_FULFILLMENT.status, RecordStatus::Settled);
    }

    #[test]
    fn 定位两条款互异() {
        assert_ne!(POSITION_CLAUSES[0], POSITION_CLAUSES[1]);
        assert!(POSITION_CLAUSES[0].contains("不可信内容"));
    }

    #[test]
    fn 边界表入出表裁决() {
        assert_eq!(
            scope_of("越界几何/属性描述的拒绝与立案"),
            Ok(true)
        );
        assert_eq!(
            scope_of("渲染结果的业务正确性"),
            Err(VrCode::POSITION_OUT_OF_SCOPE)
        );
        assert_eq!(scope_of("表外职责"), Err(VrCode::POSITION_OUT_OF_SCOPE));
    }
}
