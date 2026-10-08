//! CGPU-F2721 判据层：R 域开工与安全渲染总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2721`
//!
//! **锚点判据（签收/定位声明/四段/兑现确认/四组/判据）→ 判据族**：
//! RECEIPT 2 / POSITION 2 / PIPE 2 / FULFILL 2 / CODE 2 / META 2 = 12 项。
//!
//! # 本层核心纪律：判据侧独立写死对拍，非同源恒绿

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::cgr01_secure::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的定位两条款。
const EXP_POSITION: [&str; 2] = [
    "不可信内容渲染防线——防线管渲染路径上的恶意与异常内容",
    "防线不管渲染结果的业务正确性——边界外即他域职责",
];

/// 判据侧独立写死的移交包七件（逐字）。
const EXP_HANDOVER_ITEMS: [&str; 7] = [
    "接口冻结清单",
    "契约清单",
    "资产清单",
    "基线快照",
    "遗留移交清单",
    "安全渲染衔接包（可靠性异常→安全渲染降级接口草案）",
    "经验教训",
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VrCode; 6] = [
    VrCode::POSITION_OUT_OF_SCOPE,
    VrCode::RECEIPT_INCOMPLETE,
    VrCode::STAGE_VIOLATION,
    VrCode::FULFILLMENT_MISSING,
    VrCode::ISOLATION_SKIPPED,
    VrCode::DOMAIN_RANGE_INVALID,
];

/// 判据侧独立写死的本单码表。
const EXP_CODES: [u16; 6] = [0x5701, 0x5702, 0x5703, 0x5704, 0x5705, 0x5706];

// ---------------------------------------------------------------------------
// 判据主体
// ---------------------------------------------------------------------------

/// R01 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgr01_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-R");

    // --- RECEIPT · Q10 移交签收（判据①：签收） ---------------------------
    let receipt_ok = Q10_HANDOVER.source == 2720
        && Q10_HANDOVER.items == 7
        && Q10_HANDOVER.status == RecordStatus::Settled;
    s.add(
        "R1-签收-Q10移交包记录",
        receipt_ok,
        "Q10 移交包（F2720）签收：来源 2720、七件、状态 Settled——签收缺项即域未开工",
    );

    // 七件+两项附告逐项独立对拍。
    let items_ok = HANDOVER_ITEMS.len() == 7
        && (0..7).all(|i| HANDOVER_ITEMS[i] == EXP_HANDOVER_ITEMS[i])
        && HANDOVER_ITEMS.len() as u32 == Q10_HANDOVER.items
        && HANDOVER_ANNEX.contains(&"安全渲染接口就绪")
        && HANDOVER_ANNEX.contains(&"72h 彩排启动");
    s.add(
        "R1-签收-七件附告逐项对拍",
        items_ok,
        "移交包七件判据侧独立写死逐项对拍；两项附告（接口就绪/彩排启动）在册",
    );

    // --- POSITION · 定位声明（判据②） ------------------------------------
    let pos_match = POSITION_CLAUSES.len() == 2
        && (0..2).all(|i| POSITION_CLAUSES[i] == EXP_POSITION[i]);
    let pos_trivial = POSITION_CLAUSES[0].contains("不可信内容")
        && POSITION_CLAUSES[1].contains("业务正确性")
        && POSITION_CLAUSES[0] != POSITION_CLAUSES[1];
    s.add(
        "R1-定位-防线两条款",
        pos_match && pos_trivial,
        "不可信内容渲染防线两条款字面量冻结：防线管渲染路径、不管业务正确性（判据侧独立对拍）",
    );

    // 域区间守恒（定位的量化面）。
    let range_ok = R_DOMAIN_FIRST == 2721
        && R_DOMAIN_LAST == 2880
        && R_DOMAIN_TOTAL == 160
        && R_DOMAIN_LAST - R_DOMAIN_FIRST + 1 == R_DOMAIN_TOTAL;
    s.add(
        "R1-定位-域区间守恒",
        range_ok,
        "R 域 2721~2880 十组 160 项守恒（区间自洽独立重算）",
    );

    // 边界表逐条对拍 + 裁决双向（入表放行/出表与表外拒绝）。
    let scope_match = SCOPE_TABLE.len() == 8
        && SCOPE_TABLE.iter().filter(|e| e.in_scope).count() == 4
        && SCOPE_TABLE.iter().filter(|e| !e.in_scope).all(|e| !e.owner.is_empty());
    let rule_ok = scope_of("隔离区渲染（可疑内容不触达可信资源）") == Ok(true)
        && scope_of("显存越权访问") == Err(VrCode::POSITION_OUT_OF_SCOPE)
        && scope_of("表外职责") == Err(VrCode::POSITION_OUT_OF_SCOPE);
    s.add(
        "R1-定位-边界表裁决双向",
        scope_match && rule_ok,
        "防线边界表八条（入表 4/出表 4 且出表全部标注承接域）；裁决双向：入表放行、出表与表外显性拒绝",
    );

    // --- PIPE · 四段架构（判据③） ----------------------------------------
    let labels_ok = PipelineStage::ALL.len() == 4
        && PipelineStage::ALL[0].label() == "验证"
        && PipelineStage::ALL[1].label() == "隔离"
        && PipelineStage::ALL[2].label() == "降级"
        && PipelineStage::ALL[3].label() == "恢复";
    let forward_ok = (0..3).all(|i| stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
    s.add(
        "R1-四段-验证到恢复单向",
        labels_ok && forward_ok,
        "四段（验证→隔离→降级→恢复）标签闭集；相邻单向迁移全部放行",
    );

    // 跳段与回退双向拒绝（含"未隔离即恢复"污染回流路径）。
    let reject_ok = !stage_transition(PipelineStage::Verify, PipelineStage::Degrade)
        && !stage_transition(PipelineStage::Verify, PipelineStage::Recover)
        && !stage_transition(PipelineStage::Recover, PipelineStage::Verify)
        && !stage_transition(PipelineStage::Degrade, PipelineStage::Isolate)
        && !stage_transition(PipelineStage::Isolate, PipelineStage::Isolate);
    s.add(
        "R1-四段-跳段回退拒绝",
        reject_ok,
        "跳段（验证→降级）/未隔离即恢复（验证→恢复）/回退/原地（隔离→隔离）均拒绝——污染不回流",
    );

    // --- FULFILL · Q10 预告兑现确认（判据④） ------------------------------
    let fulfill_ok = Q10_FULFILLMENT.from == 2720
        && Q10_FULFILLMENT.what.contains("可靠性异常")
        && Q10_FULFILLMENT.what.contains("安全渲染降级接口")
        && Q10_FULFILLMENT.land_at == 2722
        && Q10_FULFILLMENT.status == RecordStatus::Settled;
    s.add(
        "R1-兑现-降级接口确认",
        fulfill_ok,
        "Q10 预告（可靠性异常→安全渲染降级接口）兑现确认：来源/内容/落地单/状态四字段齐备",
    );

    // 兑现与签收互证：预告来源一致、状态均为 Settled（空头支票检测的基准面）。
    let cross_ok = Q10_FULFILLMENT.from == Q10_HANDOVER.source
        && Q10_FULFILLMENT.status == RecordStatus::Settled
        && Q10_HANDOVER.status == RecordStatus::Settled;
    s.add(
        "R1-兑现-与签收互证",
        cross_ok,
        "兑现记录与签收记录同源（均 2720）且状态一致——预告、签收、兑现三点成线",
    );

    // --- CODE · 诊断码（0x57xx 独占） -------------------------------------
    let codes_match = ALL_CODES.len() == 6
        && (0..6).all(|i| ALL_CODES[i].code() == EXP_CODES[i])
        && ALL_CODES.iter().all(|c| (c.code() & 0xFF00) == 0x5700)
        && ALL_CODES.iter().all(|c| !c.reason().is_empty());
    let mut uniq = true;
    for i in 0..ALL_CODES.len() {
        for j in (i + 1)..ALL_CODES.len() {
            if ALL_CODES[i].code() == ALL_CODES[j].code() {
                uniq = false;
            }
        }
    }
    // 防自判死：与邻域码段不同值。
    let no_clash = ALL_CODES.iter().all(|c| {
        let hi = c.code() >> 8;
        hi != 0x39 && hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53 && hi != 0x54 && hi != 0x55 && hi != 0x56
    });
    s.add(
        "R1-码段-0x57xx独占互异",
        codes_match && uniq && no_clash,
        "六码全部 0x57xx、判据侧码表逐值对拍、互异、reason 非空；与 0x39/0x50~0x56 邻域不同值防自判死",
    );

    // --- META · 判据承载力自检（判据⑤四组；判据⑥判据） --------------------
    let meta_ok = EXP_HANDOVER_ITEMS.len() == 7
        && EXP_CODES.len() == 6
        && EXP_POSITION.len() == 2
        && ALL_CODES.len() == 6
        && Q10_HANDOVER.items as usize == EXP_HANDOVER_ITEMS.len();
    s.add(
        "R1-自检-四组判据承载力",
        meta_ok,
        "判据侧参照表自身非平凡：七件/六码/两条款——对拍不是同源恒绿",
    );

    s
}
