//! CGPU-F3521 判据层：W 域开工与 SDK 文档总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3521`
//!
//! **锚点判据（签收/定位声明/四段/兑现确认/四组/判据）→ 判据族**：
//! RECEIPT 2 / POSITION 2 / PIPE 2 / FULFILL 3 / CODE 2 / META 2 = 13 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 签收七件与定位两条款**判据侧独立写死逐字对拍**；四段推进恰一步/
//! 跳段/回退三向分账 + 发布终端闸（未过质量关即发布拒）；示例映射表
//! 逐行独立对拍 + **与 V 域 vcv01 映射表跨单元同源对拍**（表外判据
//! 拒，V10 兑现不是空头支票）；码段判据 `!=` 防自判死（0x50..0x5B
//! 全排除）。

use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcw01_sdkdoc::*;
use crate::cgpu::vcv01_realverify::BENCH_CRITERIA_MAP;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的签收七件（与被测 HANDOVER_ITEMS 逐字对拍）。
const EXP_HANDOVER_ITEMS: [&str; 7] = [
    "SDK 参考索引",
    "教程清单",
    "文档质量台账",
    "站点结构图",
    "API 冻结签名清单",
    "示例代码集",
    "经验教训",
];

/// 判据侧独立写死的签收来源（V 域收官宣告）。
const EXP_HANDOVER_SOURCE: u32 = 3520;

/// 判据侧独立写死的定位两条款。
const EXP_POSITION: [&str; 2] = [
    "SDK 文档是十年承诺接口的说明书：冻结签名才进参考文档，未冻结不立页",
    "文档即契约：示例口径与实现一致，漂移即缺陷",
];

/// 判据侧独立写死的兑现落点（W02 参考单）。
const EXP_LAND_AT: u32 = 3522;

/// 判据侧独立写死的示例映射表（逐行对拍）。
const EXP_EXAMPLES: [(&str, &str); 3] = [
    ("VC-帧稳-万分比", "examples/render/frame_stability"),
    ("VC-首帧-毫秒上限", "examples/render/first_frame"),
    ("VC-合成-CPU 万分比", "examples/compositor/load_budget"),
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [WwCode; 6] = [
    WwCode::HANDOVER_INCOMPLETE,
    WwCode::STAGE_JUMP,
    WwCode::STAGE_BACKWARD,
    WwCode::CRITERION_UNLISTED,
    WwCode::PUBLISH_PREMATURE,
    WwCode::POSITION_CONFLICT,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 13;

// ---------------------------------------------------------------------------
// 判据主入口
// ---------------------------------------------------------------------------

pub fn run_vcw01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-sdkdoc");

    // ================= 一、签收（RECEIPT） =================

    // RECEIPT-1：七件逐件对账（判据侧写死）+ 数量/状态结构核对。
    let mut receipt_ok = V10_HANDOVER.source == EXP_HANDOVER_SOURCE
        && V10_HANDOVER.items == 7
        && V10_HANDOVER.status == RecordStatus::Settled
        && HANDOVER_ITEMS.len() == 7;
    for i in 0..7 {
        if HANDOVER_ITEMS[i] != EXP_HANDOVER_ITEMS[i] {
            receipt_ok = false;
        }
    }
    s.add("W1-RECEIPT-七件逐件对账", receipt_ok, "");

    // RECEIPT-2：反向语料——缺件/错名必拒（双向）。
    let mut short: Vec<&str> = EXP_HANDOVER_ITEMS.to_vec();
    short.truncate(6);
    let mut renamed = EXP_HANDOVER_ITEMS;
    renamed[0] = "SDK 参考索引（改）";
    let reject_ok = verify_handover(&short) == Err(WwCode::HANDOVER_INCOMPLETE)
        && verify_handover(&renamed) == Err(WwCode::HANDOVER_INCOMPLETE)
        && verify_handover(&EXP_HANDOVER_ITEMS) == Ok(());
    s.add("W1-RECEIPT-缺件错名反向必拒", reject_ok, "");

    // ================= 二、定位声明（POSITION） =================

    // POSITION-1：两条款逐字对拍（判据侧写死）。
    let mut pos_ok = POSITION_CLAUSES.len() == 2;
    for i in 0..2 {
        if POSITION_CLAUSES[i] != EXP_POSITION[i] {
            pos_ok = false;
        }
    }
    s.add("W1-POSITION-两条款逐字对拍", pos_ok, "");

    // POSITION-2：反向语料——篡改「冻结签名才立页」必拒。
    let mut bad = EXP_POSITION;
    bad[0] = "SDK 文档是十年承诺接口的说明书：未冻结签名也进参考文档";
    let mut short2: Vec<&str> = EXP_POSITION.to_vec();
    short2.truncate(1);
    let reject_ok = verify_position(&bad) == Err(WwCode::POSITION_CONFLICT)
        && verify_position(&short2) == Err(WwCode::POSITION_CONFLICT)
        && verify_position(&EXP_POSITION) == Ok(());
    s.add("W1-POSITION-口径篡改反向必拒", reject_ok, "");

    // ================= 三、四段架构（PIPE） =================

    // PIPE-1：全序列恰一步推进全过（三步链：0→1→2→3）。
    let mut fwd_ok = true;
    for i in 0..PipelineStage::ALL.len() - 1 {
        if stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]).is_err() {
            fwd_ok = false;
        }
    }
    s.add("W1-PIPE-四段恰一步全过", fwd_ok, "");

    // PIPE-2：跳段与回退双向拒 + 发布终端闸（未过质量关即发布拒）。
    let jump = stage_transition(PipelineStage::ALL[0], PipelineStage::ALL[2])
        == Err(WwCode::STAGE_JUMP)
        && stage_transition(PipelineStage::ALL[0], PipelineStage::ALL[3])
            == Err(WwCode::STAGE_JUMP)
        && stage_transition(PipelineStage::ALL[3], PipelineStage::ALL[0])
            == Err(WwCode::STAGE_BACKWARD)
        && stage_transition(PipelineStage::ALL[2], PipelineStage::ALL[1])
            == Err(WwCode::STAGE_BACKWARD)
        && stage_transition(PipelineStage::ALL[1], PipelineStage::ALL[1])
            == Err(WwCode::STAGE_BACKWARD)
        && publish_allowed(PipelineStage::Quality).is_ok()
        && publish_allowed(PipelineStage::Site).is_ok()
        && publish_allowed(PipelineStage::Tutorial) == Err(WwCode::PUBLISH_PREMATURE)
        && publish_allowed(PipelineStage::Reference) == Err(WwCode::PUBLISH_PREMATURE);
    s.add("W1-PIPE-跳段回退双向拒与发布闸", jump, "");

    // ================= 四、兑现确认（FULFILL） =================

    // FULFILL-1：兑现结构核对（from/land_at/Settled 判据侧写死）。
    let fulfill_ok = V10_FULFILLMENT.from == EXP_HANDOVER_SOURCE
        && V10_FULFILLMENT.land_at == EXP_LAND_AT
        && V10_FULFILLMENT.status == RecordStatus::Settled
        && V10_FULFILLMENT.land_at > V10_FULFILLMENT.from;
    s.add("W1-FULFILL-兑现结构与落点核对", fulfill_ok, "");

    // FULFILL-2：映射表逐行对拍（判据侧 EXP_EXAMPLES 独立写死）+ 查询全通。
    let mut map_ok = CRITERION_EXAMPLES.len() == 3;
    for i in 0..3 {
        let row = &CRITERION_EXAMPLES[i];
        if row.criterion != EXP_EXAMPLES[i].0 || row.example != EXP_EXAMPLES[i].1 {
            map_ok = false;
        }
        if example_of(row.criterion) != Ok(row.example) {
            map_ok = false;
        }
    }
    s.add("W1-FULFILL-映射表逐行独立对拍", map_ok, "");

    // FULFILL-3：跨单元同源——示例判据 id 必须全部在 V 域映射表在册
    //（V10「验收判据入文档示例」兑现的跨单元证据，表外判据拒 0x5C04）。
    let mut cross_ok = true;
    for row in CRITERION_EXAMPLES.iter() {
        let listed = BENCH_CRITERIA_MAP.iter().any(|b| b.criterion == row.criterion);
        if !listed {
            cross_ok = false;
        }
    }
    if example_of("VC-表外判据") != Err(WwCode::CRITERION_UNLISTED) {
        cross_ok = false;
    }
    s.add("W1-FULFILL-判据id与V域映射表同源", cross_ok, "");

    // ================= 五、码段（CODE） =================

    // CODE-1：码段独占——全部 0x5Cxx，且 != 0x50..0x5B（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5C)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53 && hi != 0x54
                && hi != 0x55 && hi != 0x56 && hi != 0x57 && hi != 0x58
                && hi != 0x59 && hi != 0x5A && hi != 0x5B && hi != 0x5D
        });
    s.add("W1-CODE-码段0x5C独占", section_ok, "");

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
    s.add("W1-CODE-码互异原因非空", code_ok, "");

    // ================= 六、判据自检（META） =================

    // META-1：域守恒独立重算（10 组 × 16 项 = 160）。
    let meta_ok = W_DOMAIN_TOTAL == 160
        && 10 * 16 == W_DOMAIN_TOTAL
        && ALL_CODES.len() == 6
        && (ALL_CODES[0].code() == 0x5C01 && ALL_CODES[5].code() == 0x5C06);
    s.add("W1-META-域守恒与六码恰连续", meta_ok, "");

    // META-2：判据条数对账（放末位：此时 len 应为 12，加自身恰 13）。
    s.add("W1-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
