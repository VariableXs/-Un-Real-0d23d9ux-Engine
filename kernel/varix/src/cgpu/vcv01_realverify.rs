//! CGPU-F3361 · V 域开工与真机验收总架构（CGPU-V 域 · V01 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3361`
//!
//! 域开工：V 域开工（签收 U10 移交包——真机验收衔接包签收，衔接确认）；
//! 真机验收定位（用手不用测试报告——定位声明，十五章呼应）；四段架构
//! （矩阵/判据/走查/报告）；U10 预告兑现（Bench 分数与验收判据映射——
//! 兑现确认）；测试（签收/定位/四段/兑现四组）。
//!
//! ## 要点一：签收是开工的前置事实，不是仪式
//!
//! U10 移交包七件逐件登记，缺一件即 V 域未开工（签收状态 Settled 但
//! 内容缺项 = 空头签收，判据族 RECEIPT 反向语料拒绝）。
//!
//! ## 要点二：定位条款——「用手不用测试报告」
//!
//! 真机验收以真实使用走查为主证据；测试报告（含 Bench）只是辅助证据
//! 与回归基线。定位两条款字面量冻结：任一与「用手」主证据地位的矛盾
//! 表述都进不了账（机制化裁决而非口头诚实）。
//!
//! ## 要点三：四段单向架构——矩阵→判据→走查→报告
//!
//! 验收矩阵圈定范围、验收判据量化口径、用手走查产生主证据、验收报告
//! 汇总裁决。四段单向推进：未走查完出报告 = 0x5B05 拒绝；回退重做走
//! 显性码（不允许暗改矩阵后沿用旧走查结论）。
//!
//! ## 要点四：兑现确认——Bench 分数与验收判据映射
//!
//! U10 预告的「Bench 分数进验收」以映射表兑现：每个 Bench 套件阈值
//! 绑定一条验收判据 id，分数恰阈值过、差一分拒（整数口径，无浮点比
//! 较歧义）；表外 Bench 套件不得自动进验收（未映射即拒）。
//!
//! ## 要点五：零 panic 面
//!
//! 无 unwrap/expect/裸下标越界；缺件、跳段、未映射、过早报告一律专
//! 属码拒绝。
//!
//! ## 要点六：诊断码独占 0x5Bxx 段
//!
//! 全仓 grep 零占用后选定；与 vcj01（0x52）/vcl01（0x53）/cgm01+
//! （0x54）/vco01（0x55）/vcq01（0x56）/cgr01（0x57）/vcq02（0x58）/
//! vco03（0x59）/vct01（0x5A）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、域守恒与签收（U10 移交包——衔接确认）
// ---------------------------------------------------------------------------

/// V 域任务总数守恒（10 组 × 16 项，F3361~F3520）。
pub const V_DOMAIN_TOTAL: u32 = 160;

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收/已兑现。
    Settled,
    /// 待兑现。
    Pending,
}

/// U10 移交包签收记录（真机验收衔接包——衔接确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（U 域收官宣告 F3360）。
    pub source: u32,
    /// 移交包内容项数（七件）。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// 移交包七件（F3360 收官宣告口径——判据独立对拍）。
pub const HANDOVER_ITEMS: [&str; 7] = [
    "Bench 分数台账",
    "验收矩阵草案",
    "判据映射表",
    "走查脚本清单",
    "验收报告模板",
    "真机设备画像",
    "经验教训",
];

/// U10 移交包签收（字面量钉死——判据独立写死）。
pub const U10_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 3360,
    items: 7,
    status: RecordStatus::Settled,
};

/// 签收裁决：七件逐件在册即过；缺一件即 0x5B01 拒（空头签收防线）。
pub fn verify_handover(items: &[&str]) -> Result<(), VvCode> {
    if items.len() != HANDOVER_ITEMS.len() {
        return Err(VvCode::HANDOVER_INCOMPLETE);
    }
    for i in 0..HANDOVER_ITEMS.len() {
        if items[i] != HANDOVER_ITEMS[i] {
            return Err(VvCode::HANDOVER_INCOMPLETE);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、定位声明（用手不用测试报告——两条款字面量冻结）
// ---------------------------------------------------------------------------

/// 真机验收定位两条款（字面量冻结——判据独立写死对拍）。
pub const POSITION_CLAUSES: [&str; 2] = [
    "真机验收以真实使用为准：用手走查是主证据，测试报告只是辅助证据",
    "验收口径与第十五章「用手验收」逐字呼应，不另立第二口径",
];

/// 定位裁决：两条声明与冻结条款逐字一致才入账，矛盾即 0x5B02 拒。
pub fn verify_position(clauses: &[&str]) -> Result<(), VvCode> {
    if clauses.len() != POSITION_CLAUSES.len() {
        return Err(VvCode::POSITION_CONFLICT);
    }
    for i in 0..POSITION_CLAUSES.len() {
        if clauses[i] != POSITION_CLAUSES[i] {
            return Err(VvCode::POSITION_CONFLICT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、四段架构（单向流水线：矩阵→判据→走查→报告）
// ---------------------------------------------------------------------------

/// 验收四段（单向：上一段产出是下一段输入，回退即显性码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 验收矩阵（范围圈定：子系统 × 场景）。
    Matrix,
    /// 验收判据（量化口径：整数阈值 + 判据 id）。
    Criteria,
    /// 用手走查（主证据产生：真实使用记录）。
    Walkthrough,
    /// 验收报告（汇总裁决：只汇总不造证据）。
    Report,
}

impl PipelineStage {
    /// 全序列（单向推进的参照链）。
    pub const ALL: [PipelineStage; 4] = [
        PipelineStage::Matrix,
        PipelineStage::Criteria,
        PipelineStage::Walkthrough,
        PipelineStage::Report,
    ];

    /// 序号（判据用）。
    pub const fn ordinal(self) -> usize {
        match self {
            PipelineStage::Matrix => 0,
            PipelineStage::Criteria => 1,
            PipelineStage::Walkthrough => 2,
            PipelineStage::Report => 3,
        }
    }

    /// 人话段名。
    pub const fn name(self) -> &'static str {
        match self {
            PipelineStage::Matrix => "矩阵",
            PipelineStage::Criteria => "判据",
            PipelineStage::Walkthrough => "走查",
            PipelineStage::Report => "报告",
        }
    }
}

/// 段迁移裁决：仅允许沿 ALL 单向恰进一步；跳段 0x5B02、回退 0x5B03。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> Result<(), VvCode> {
    let f = from.ordinal();
    let t = to.ordinal();
    if t == f + 1 {
        Ok(())
    } else if t > f + 1 {
        Err(VvCode::STAGE_JUMP)
    } else {
        Err(VvCode::STAGE_BACKWARD)
    }
}

/// 报告前置裁决：未走查到位出报告 = 0x5B05 拒（四段纪律的终端闸）。
pub fn report_allowed(reached: PipelineStage) -> Result<(), VvCode> {
    if reached.ordinal() >= PipelineStage::Walkthrough.ordinal() {
        Ok(())
    } else {
        Err(VvCode::REPORT_PREMATURE)
    }
}

// ---------------------------------------------------------------------------
// 四、兑现确认（Bench 分数与验收判据映射——整数口径）
// ---------------------------------------------------------------------------

/// U10 预告兑现确认（Bench 分数进验收——land_at = V02 验收矩阵落地）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fulfillment {
    /// 预告来源（U10 收官宣告）。
    pub from: u32,
    /// 兑现落点（V02 矩阵单）。
    pub land_at: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// 兑现记录（字面量钉死——判据独立写死）。
pub const U10_FULFILLMENT: Fulfillment = Fulfillment {
    from: 3360,
    land_at: 3362,
    status: RecordStatus::Settled,
};

/// 一条映射：Bench 套件阈值 ↔ 验收判据 id（分数进验收的唯一通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchCriterion {
    /// Bench 套件名。
    pub suite: &'static str,
    /// 通过阈值（整数分数，恰阈值过、差一分拒）。
    pub threshold: u32,
    /// 绑定的验收判据 id。
    pub criterion: &'static str,
}

/// 映射表（U10 兑现的最小在账集——判据侧独立写死对拍）。
pub const BENCH_CRITERIA_MAP: [BenchCriterion; 3] = [
    BenchCriterion { suite: "帧率稳定性", threshold: 9000, criterion: "VC-帧稳-万分比" },
    BenchCriterion { suite: "首帧延迟", threshold: 250, criterion: "VC-首帧-毫秒上限" },
    BenchCriterion { suite: "合成器负载", threshold: 6000, criterion: "VC-合成-CPU 万分比" },
];

/// 分数裁决：恰阈值过（>=）、差一分拒（整数口径无双标）。
pub fn score_verdict(threshold: u32, score: u32) -> bool {
    score >= threshold
}

/// 套件映射查询：表外套件不得进验收（未映射即 0x5B04 拒）。
pub fn criterion_of(suite: &str) -> Result<&'static str, VvCode> {
    for row in BENCH_CRITERIA_MAP.iter() {
        if row.suite == suite {
            return Ok(row.criterion);
        }
    }
    Err(VvCode::MATRIX_UNMAPPED)
}

// ---------------------------------------------------------------------------
// 五、错误契约（独占 0x5Bxx 段）
// ---------------------------------------------------------------------------

/// vcv01 诊断码。独占 `0x5Bxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VvCode(pub u16);

impl VvCode {
    /// 签收缺项（七件之外/件名不符 = 空头签收）。
    pub const HANDOVER_INCOMPLETE: VvCode = VvCode(0x5B01);
    /// 跳段（四段单向流水线越过恰下一步）。
    pub const STAGE_JUMP: VvCode = VvCode(0x5B02);
    /// 回退（走查结论不允许在暗改矩阵后沿用）。
    pub const STAGE_BACKWARD: VvCode = VvCode(0x5B03);
    /// 表外 Bench 套件未映射即进验收。
    pub const MATRIX_UNMAPPED: VvCode = VvCode(0x5B04);
    /// 未走查到位出报告。
    pub const REPORT_PREMATURE: VvCode = VvCode(0x5B05);
    /// 定位条款与冻结口径矛盾。
    pub const POSITION_CONFLICT: VvCode = VvCode(0x5B06);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VvCode::HANDOVER_INCOMPLETE => "签收缺项：七件逐件对账不过 = 空头签收".into(),
            VvCode::STAGE_JUMP => "跳段：四段单向流水线只允许恰进一步".into(),
            VvCode::STAGE_BACKWARD => "回退：走查结论不允许在暗改矩阵后沿用".into(),
            VvCode::MATRIX_UNMAPPED => "表外 Bench 套件未映射：不得自动进验收".into(),
            VvCode::REPORT_PREMATURE => "未走查到位出报告：报告只汇总不造证据".into(),
            VvCode::POSITION_CONFLICT => "定位条款与冻结口径矛盾：「用手」主证据地位不可动摇".into(),
            VvCode(_) => "未知 vcv01 真机验收域诊断码".into(),
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
        assert_eq!(verify_handover(&HANDOVER_ITEMS[..6]), Err(VvCode::HANDOVER_INCOMPLETE));
    }

    #[test]
    fn 定位两条款逐字过与篡改拒() {
        assert_eq!(verify_position(&POSITION_CLAUSES), Ok(()));
        let bad = ["测试报告是主证据", POSITION_CLAUSES[1]];
        assert_eq!(verify_position(&bad), Err(VvCode::POSITION_CONFLICT));
    }

    #[test]
    fn 四段恰进一步跳段回退双向拒() {
        assert_eq!(stage_transition(PipelineStage::Matrix, PipelineStage::Criteria), Ok(()));
        assert_eq!(
            stage_transition(PipelineStage::Matrix, PipelineStage::Report),
            Err(VvCode::STAGE_JUMP)
        );
        assert_eq!(
            stage_transition(PipelineStage::Report, PipelineStage::Walkthrough),
            Err(VvCode::STAGE_BACKWARD)
        );
    }

    #[test]
    fn 分数恰阈值过差一分拒() {
        assert!(score_verdict(9000, 9000));
        assert!(!score_verdict(9000, 8999));
        assert_eq!(criterion_of("帧率稳定性"), Ok("VC-帧稳-万分比"));
        assert_eq!(criterion_of("表外套件"), Err(VvCode::MATRIX_UNMAPPED));
    }
}
