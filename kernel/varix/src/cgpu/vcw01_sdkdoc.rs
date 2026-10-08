//! CGPU-F3521 · W 域开工与 SDK 文档总架构（CGPU-W 域 · W01 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3521`
//!
//! 域开工：W 域开工（签收 V10 移交包——文档衔接包签收，衔接确认）；
//! SDK 文档定位（十年接口的说明书——定位声明）；四段架构（参考/教程/
//! 质量/站点）；V10 预告兑现（验收判据入文档示例——兑现确认）；测试
//! （签收/定位/四段/兑现四组）。
//!
//! ## 要点一：签收七件逐件对账，缺件即域未开工
//!
//! V10 文档衔接包七件在册（Bench 判据映射承接件在其中），缺一件的
//! Settled 状态 = 空头签收，专属码拒绝。
//!
//! ## 要点二：定位条款——十年接口的说明书
//!
//! SDK 文档覆盖的每一个签名都是十年承诺：冻结签名才进参考文档，未
//! 冻结不立页；文档即契约——示例口径与实现一致，漂移即缺陷。两条款
//! 字面量冻结，矛盾表述进不了账。
//!
//! ## 要点三：四段单向架构——参考→教程→质量→站点
//!
//! 参考文档立签名口径、教程给上手路径、质量关做一致性与覆盖审计、
//! 站点关做发布组织。四段单向推进：跳段/回退/未过质量关即发布（过
//! 早发布）一律显性码拒绝。
//!
//! ## 要点四：兑现确认——验收判据入文档示例
//!
//! V10 预告的「验收判据入文档示例」以映射表兑现：每条验收判据 id 绑
//! 定一个文档示例位；判据 id 必须在 V 域映射表（vcv01 BENCH_CRITERIA_
//! MAP）在册——跨单元同源对拍，表外判据不得凭空入文档。
//!
//! ## 要点五：零 panic 面
//!
//! 无 unwrap/expect/裸下标越界；缺件、跳段、未过质量关即发布、表外
//! 判据一律专属码拒绝。
//!
//! ## 要点六：诊断码独占 0x5Cxx 段
//!
//! 全仓 grep 零占用后选定；与 vcj01（0x52）/vcl01（0x53）/cgm01+
//! （0x54）/vco01（0x55）/vcq01（0x56）/cgr01（0x57）/vcq02（0x58）/
//! vco03（0x59）/vct01（0x5A）/vcv01（0x5B）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、域守恒与签收（V10 移交包——衔接确认）
// ---------------------------------------------------------------------------

/// W 域任务总数守恒（10 组 × 16 项，F3521~F3680）。
pub const W_DOMAIN_TOTAL: u32 = 160;

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收/已兑现。
    Settled,
    /// 待兑现。
    Pending,
}

/// V10 移交包签收记录（文档衔接包——衔接确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（V 域收官宣告 F3520）。
    pub source: u32,
    /// 移交包内容项数（七件）。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// 移交包七件（F3520 收官宣告口径——判据独立对拍）。
pub const HANDOVER_ITEMS: [&str; 7] = [
    "SDK 参考索引",
    "教程清单",
    "文档质量台账",
    "站点结构图",
    "API 冻结签名清单",
    "示例代码集",
    "经验教训",
];

/// V10 移交包签收（字面量钉死——判据独立写死）。
pub const V10_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 3520,
    items: 7,
    status: RecordStatus::Settled,
};

/// 签收裁决：七件逐件在册即过；缺一件/错一件即 0x5C01 拒。
pub fn verify_handover(items: &[&str]) -> Result<(), WwCode> {
    if items.len() != HANDOVER_ITEMS.len() {
        return Err(WwCode::HANDOVER_INCOMPLETE);
    }
    for i in 0..HANDOVER_ITEMS.len() {
        if items[i] != HANDOVER_ITEMS[i] {
            return Err(WwCode::HANDOVER_INCOMPLETE);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、定位声明（十年接口的说明书——两条款字面量冻结）
// ---------------------------------------------------------------------------

/// SDK 文档定位两条款（字面量冻结——判据独立写死对拍）。
pub const POSITION_CLAUSES: [&str; 2] = [
    "SDK 文档是十年承诺接口的说明书：冻结签名才进参考文档，未冻结不立页",
    "文档即契约：示例口径与实现一致，漂移即缺陷",
];

/// 定位裁决：两条声明与冻结条款逐字一致才入账，矛盾即 0x5C02 拒。
pub fn verify_position(clauses: &[&str]) -> Result<(), WwCode> {
    if clauses.len() != POSITION_CLAUSES.len() {
        return Err(WwCode::POSITION_CONFLICT);
    }
    for i in 0..POSITION_CLAUSES.len() {
        if clauses[i] != POSITION_CLAUSES[i] {
            return Err(WwCode::POSITION_CONFLICT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、四段架构（单向流水线：参考→教程→质量→站点）
// ---------------------------------------------------------------------------

/// 文档四段（单向：上一段产出是下一段输入，跳段/回退显性码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 参考（冻结签名口径立页）。
    Reference,
    /// 教程（上手路径：示例驱动）。
    Tutorial,
    /// 质量（一致性与覆盖审计：文档-实现同源核对）。
    Quality,
    /// 站点（发布组织：导航与版本化）。
    Site,
}

impl PipelineStage {
    /// 全序列（单向推进的参照链）。
    pub const ALL: [PipelineStage; 4] = [
        PipelineStage::Reference,
        PipelineStage::Tutorial,
        PipelineStage::Quality,
        PipelineStage::Site,
    ];

    /// 序号（判据用）。
    pub const fn ordinal(self) -> usize {
        match self {
            PipelineStage::Reference => 0,
            PipelineStage::Tutorial => 1,
            PipelineStage::Quality => 2,
            PipelineStage::Site => 3,
        }
    }

    /// 人话段名。
    pub const fn name(self) -> &'static str {
        match self {
            PipelineStage::Reference => "参考",
            PipelineStage::Tutorial => "教程",
            PipelineStage::Quality => "质量",
            PipelineStage::Site => "站点",
        }
    }
}

/// 段迁移裁决：仅允许沿 ALL 单向恰进一步；跳段 0x5C02、回退 0x5C03。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> Result<(), WwCode> {
    let f = from.ordinal();
    let t = to.ordinal();
    if t == f + 1 {
        Ok(())
    } else if t > f + 1 {
        Err(WwCode::STAGE_JUMP)
    } else {
        Err(WwCode::STAGE_BACKWARD)
    }
}

/// 发布前置裁决：未过质量关即发布 = 0x5C05 拒（四段纪律的终端闸）。
pub fn publish_allowed(reached: PipelineStage) -> Result<(), WwCode> {
    if reached.ordinal() >= PipelineStage::Quality.ordinal() {
        Ok(())
    } else {
        Err(WwCode::PUBLISH_PREMATURE)
    }
}

// ---------------------------------------------------------------------------
// 四、兑现确认（验收判据入文档示例——与 V 域映射表跨单元同源）
// ---------------------------------------------------------------------------

/// V10 预告兑现确认（验收判据入文档示例——land_at = W02 参考单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fulfillment {
    /// 预告来源（V10 收官宣告）。
    pub from: u32,
    /// 兑现落点（W02 参考单）。
    pub land_at: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// 兑现记录（字面量钉死——判据独立写死）。
pub const V10_FULFILLMENT: Fulfillment = Fulfillment {
    from: 3520,
    land_at: 3522,
    status: RecordStatus::Settled,
};

/// 一条示例映射：验收判据 id ↔ 文档示例位（判据进文档的唯一通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriterionExample {
    /// 验收判据 id（必须与 V 域映射表在账 id 同源）。
    pub criterion: &'static str,
    /// 文档示例位（教程/参考中的示例路径）。
    pub example: &'static str,
}

/// 示例映射表（V10 兑现的最小在账集——判据 id 与 vcv01 映射表同源）。
pub const CRITERION_EXAMPLES: [CriterionExample; 3] = [
    CriterionExample { criterion: "VC-帧稳-万分比", example: "examples/render/frame_stability" },
    CriterionExample { criterion: "VC-首帧-毫秒上限", example: "examples/render/first_frame" },
    CriterionExample { criterion: "VC-合成-CPU 万分比", example: "examples/compositor/load_budget" },
];

/// 示例查询：表外判据不得凭空入文档（未在册即 0x5C04 拒）。
pub fn example_of(criterion: &str) -> Result<&'static str, WwCode> {
    for row in CRITERION_EXAMPLES.iter() {
        if row.criterion == criterion {
            return Ok(row.example);
        }
    }
    Err(WwCode::CRITERION_UNLISTED)
}

// ---------------------------------------------------------------------------
// 五、错误契约（独占 0x5Cxx 段）
// ---------------------------------------------------------------------------

/// vcw01 诊断码。独占 `0x5Cxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WwCode(pub u16);

impl WwCode {
    /// 签收缺项（七件之外/件名不符 = 空头签收）。
    pub const HANDOVER_INCOMPLETE: WwCode = WwCode(0x5C01);
    /// 跳段（四段单向流水线越过恰下一步）。
    pub const STAGE_JUMP: WwCode = WwCode(0x5C02);
    /// 回退（发布后不允许暗改教程路径沿用旧站点结论）。
    pub const STAGE_BACKWARD: WwCode = WwCode(0x5C03);
    /// 表外判据未在册即入文档示例。
    pub const CRITERION_UNLISTED: WwCode = WwCode(0x5C04);
    /// 未过质量关即发布。
    pub const PUBLISH_PREMATURE: WwCode = WwCode(0x5C05);
    /// 定位条款与冻结口径矛盾。
    pub const POSITION_CONFLICT: WwCode = WwCode(0x5C06);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            WwCode::HANDOVER_INCOMPLETE => "签收缺项：七件逐件对账不过 = 空头签收".into(),
            WwCode::STAGE_JUMP => "跳段：四段单向流水线只允许恰进一步".into(),
            WwCode::STAGE_BACKWARD => "回退：站点结论不允许在暗改教程路径后沿用".into(),
            WwCode::CRITERION_UNLISTED => "表外判据未在册：不得凭空入文档示例".into(),
            WwCode::PUBLISH_PREMATURE => "未过质量关即发布：站点只组织不造质量".into(),
            WwCode::POSITION_CONFLICT => "定位条款与冻结口径矛盾：十年说明书口径不可动摇".into(),
            WwCode(_) => "未知 vcw01 SDK 文档域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（签收/定位/四段/兑现四组）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 签收七件全过与缺件拒() {
        assert_eq!(verify_handover(&HANDOVER_ITEMS), Ok(()));
        assert_eq!(verify_handover(&HANDOVER_ITEMS[..6]), Err(WwCode::HANDOVER_INCOMPLETE));
    }

    #[test]
    fn 定位两条款逐字过与篡改拒() {
        assert_eq!(verify_position(&POSITION_CLAUSES), Ok(()));
        let bad = ["未冻结签名也进参考文档", POSITION_CLAUSES[1]];
        assert_eq!(verify_position(&bad), Err(WwCode::POSITION_CONFLICT));
    }

    #[test]
    fn 四段恰进一步跳段回退双向拒() {
        assert_eq!(stage_transition(PipelineStage::Reference, PipelineStage::Tutorial), Ok(()));
        assert_eq!(
            stage_transition(PipelineStage::Reference, PipelineStage::Site),
            Err(WwCode::STAGE_JUMP)
        );
        assert_eq!(
            stage_transition(PipelineStage::Site, PipelineStage::Quality),
            Err(WwCode::STAGE_BACKWARD)
        );
    }

    #[test]
    fn 示例映射在册过与表外拒() {
        assert_eq!(example_of("VC-帧稳-万分比"), Ok("examples/render/frame_stability"));
        assert_eq!(example_of("表外判据"), Err(WwCode::CRITERION_UNLISTED));
    }
}
