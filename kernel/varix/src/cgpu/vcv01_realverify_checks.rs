//! CGPU-F3361 判据层：V 域开工与真机验收总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3361`
//!
//! **锚点判据（签收/定位声明/四段/兑现确认/四组/判据）→ 判据族**：
//! RECEIPT 2 / POSITION 2 / PIPE 2 / FULFILL 3 / CODE 2 / META 2 = 13 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 签收七件与定位两条款**判据侧独立写死逐字对拍**（四组测试中被测自
//! 报的不算）；四段推进恰一步/跳段/回退三向分账（恰边界双向）；映射
//! 表逐行阈值↔判据 id 独立对拍 + 恰阈值过差一分拒（整数口径双闸）；
//! 码段判据 `!=` 防自判死（0x50..0x5A 全排除）。

use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcv01_realverify::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的签收七件（与被测 HANDOVER_ITEMS 逐字对拍）。
const EXP_HANDOVER_ITEMS: [&str; 7] = [
    "Bench 分数台账",
    "验收矩阵草案",
    "判据映射表",
    "走查脚本清单",
    "验收报告模板",
    "真机设备画像",
    "经验教训",
];

/// 判据侧独立写死的签收来源（U 域收官宣告）。
const EXP_HANDOVER_SOURCE: u32 = 3360;

/// 判据侧独立写死的定位两条款。
const EXP_POSITION: [&str; 2] = [
    "真机验收以真实使用为准：用手走查是主证据，测试报告只是辅助证据",
    "验收口径与第十五章「用手验收」逐字呼应，不另立第二口径",
];

/// 判据侧独立写死的兑现落点（V02 矩阵单）。
const EXP_LAND_AT: u32 = 3362;

/// 判据侧独立写死的映射表（逐行对拍）。
const EXP_MAP: [(&str, u32, &str); 3] = [
    ("帧率稳定性", 9000, "VC-帧稳-万分比"),
    ("首帧延迟", 250, "VC-首帧-毫秒上限"),
    ("合成器负载", 6000, "VC-合成-CPU 万分比"),
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VvCode; 6] = [
    VvCode::HANDOVER_INCOMPLETE,
    VvCode::STAGE_JUMP,
    VvCode::STAGE_BACKWARD,
    VvCode::MATRIX_UNMAPPED,
    VvCode::REPORT_PREMATURE,
    VvCode::POSITION_CONFLICT,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vcv01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-realverify");

    // ================= 一、签收（RECEIPT） =================

    // RECEIPT-1：七件逐件对账（判据侧写死）+ 数量/状态结构核对。
    let mut receipt_ok = U10_HANDOVER.source == EXP_HANDOVER_SOURCE
        && U10_HANDOVER.items == 7
        && U10_HANDOVER.status == RecordStatus::Settled
        && HANDOVER_ITEMS.len() == 7;
    for i in 0..7 {
        if HANDOVER_ITEMS[i] != EXP_HANDOVER_ITEMS[i] {
            receipt_ok = false;
        }
    }
    s.add("V1-RECEIPT-七件逐件对账", receipt_ok, "");

    // RECEIPT-2：反向语料——缺件/错名/空头签收必拒（双向）。
    let mut short: Vec<&str> = EXP_HANDOVER_ITEMS.to_vec();
    short.truncate(6);
    let mut renamed = EXP_HANDOVER_ITEMS;
    renamed[2] = "判据映射表（改）";
    let reject_ok = verify_handover(&short) == Err(VvCode::HANDOVER_INCOMPLETE)
        && verify_handover(&renamed) == Err(VvCode::HANDOVER_INCOMPLETE)
        && verify_handover(&EXP_HANDOVER_ITEMS) == Ok(());
    s.add("V1-RECEIPT-缺件错名反向必拒", reject_ok, "");

    // ================= 二、定位声明（POSITION） =================

    // POSITION-1：两条款逐字对拍（判据侧写死；「用手」主证据地位在册）。
    let mut pos_ok = POSITION_CLAUSES.len() == 2;
    for i in 0..2 {
        if POSITION_CLAUSES[i] != EXP_POSITION[i] {
            pos_ok = false;
        }
    }
    s.add("V1-POSITION-两条款逐字对拍", pos_ok, "");

    // POSITION-2：反向语料——篡改「用手」主证据地位必拒。
    let mut bad = EXP_POSITION;
    bad[0] = "真机验收以测试报告为准：报告是主证据，用手走查只是辅助证据";
    let mut short2: Vec<&str> = EXP_POSITION.to_vec();
    short2.truncate(1);
    let reject_ok = verify_position(&bad) == Err(VvCode::POSITION_CONFLICT)
        && verify_position(&short2) == Err(VvCode::POSITION_CONFLICT)
        && verify_position(&EXP_POSITION) == Ok(());
    s.add("V1-POSITION-主证据篡改反向必拒", reject_ok, "");

    // ================= 三、四段架构（PIPE） =================

    // PIPE-1：全序列恰一步推进全过（三步链：0→1→2→3）。
    let mut fwd_ok = true;
    for i in 0..PipelineStage::ALL.len() - 1 {
        if stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]).is_err() {
            fwd_ok = false;
        }
    }
    s.add("V1-PIPE-四段恰一步全过", fwd_ok, "");

    // PIPE-2：跳段与回退双向拒 + 报告终端闸（未走查出报告拒）。
    let jump = stage_transition(PipelineStage::ALL[0], PipelineStage::ALL[2])
        == Err(VvCode::STAGE_JUMP)
        && stage_transition(PipelineStage::ALL[0], PipelineStage::ALL[3])
            == Err(VvCode::STAGE_JUMP)
        && stage_transition(PipelineStage::ALL[3], PipelineStage::ALL[0])
            == Err(VvCode::STAGE_BACKWARD)
        && stage_transition(PipelineStage::ALL[2], PipelineStage::ALL[1])
            == Err(VvCode::STAGE_BACKWARD)
        && stage_transition(PipelineStage::ALL[1], PipelineStage::ALL[1])
            == Err(VvCode::STAGE_BACKWARD)
        && report_allowed(PipelineStage::Walkthrough).is_ok()
        && report_allowed(PipelineStage::Report).is_ok()
        && report_allowed(PipelineStage::Criteria) == Err(VvCode::REPORT_PREMATURE)
        && report_allowed(PipelineStage::Matrix) == Err(VvCode::REPORT_PREMATURE);
    s.add("V1-PIPE-跳段回退双向拒与报告闸", jump, "");

    // ================= 四、兑现确认（FULFILL） =================

    // FULFILL-1：兑现结构核对（from/land_at/Settled 判据侧写死）。
    let fulfill_ok = U10_FULFILLMENT.from == EXP_HANDOVER_SOURCE
        && U10_FULFILLMENT.land_at == EXP_LAND_AT
        && U10_FULFILLMENT.status == RecordStatus::Settled
        && U10_FULFILLMENT.land_at > U10_FULFILLMENT.from;
    s.add("V1-FULFILL-兑现结构与落点核对", fulfill_ok, "");

    // FULFILL-2：映射表逐行对拍（判据侧 EXP_MAP 独立写死）+ 映射查询全通。
    let mut map_ok = BENCH_CRITERIA_MAP.len() == 3;
    for i in 0..3 {
        let row = &BENCH_CRITERIA_MAP[i];
        if row.suite != EXP_MAP[i].0
            || row.threshold != EXP_MAP[i].1
            || row.criterion != EXP_MAP[i].2
        {
            map_ok = false;
        }
        if criterion_of(row.suite) != Ok(row.criterion) {
            map_ok = false;
        }
    }
    s.add("V1-FULFILL-映射表逐行独立对拍", map_ok, "");

    // FULFILL-3：整数口径恰边界双向 + 表外套件拒（0x5B04）。
    let edge_ok = score_verdict(9000, 9000)
        && !score_verdict(9000, 8999)
        && score_verdict(250, 250)
        && !score_verdict(250, 249)
        && score_verdict(6000, 12345)
        && criterion_of("表外套件") == Err(VvCode::MATRIX_UNMAPPED);
    s.add("V1-FULFILL-恰阈值双向与表外拒", edge_ok, "");

    // ================= 五、码段（CODE） =================

    // CODE-1：码段独占——全部 0x5Bxx，且 != 0x50..0x5A（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5B)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53 && hi != 0x54
                && hi != 0x55 && hi != 0x56 && hi != 0x57 && hi != 0x58
                && hi != 0x59 && hi != 0x5A && hi != 0x5C
        });
    s.add("V1-CODE-码段0x5B独占", section_ok, "");

    // CODE-2：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("V1-CODE-码互异原因非空", code_ok, "");

    // ================= 六、判据自检（META） =================

    // META-1：域守恒独立重算（10 组 × 16 项 = 160）。
    let meta_ok = V_DOMAIN_TOTAL == 160
        && 10 * 16 == V_DOMAIN_TOTAL
        && ALL_CODES.len() == 6
        && (ALL_CODES[0].code() == 0x5B01 && ALL_CODES[5].code() == 0x5B06);
    s.add("V1-META-域守恒与六码恰连续", meta_ok, "");

    // META-2：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("V1-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
