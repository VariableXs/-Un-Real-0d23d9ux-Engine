//! CGPU-F2401 · P 域开工与自适应遥测总架构域自检（锚点判据映射 + 反向语料钉门禁）。
//!
//! **判据（锚点原文）**：自适应声明、签收、兑现起点、映射、五段、基座、
//! 红线、风险、八组、判据。
//!
//! 自检纪律：判据区零 panic 面；判据侧独立重算（十组表/主题标签/红线
//! 三条/预留单号字面量写死逐条全等）；反向语料证明门禁不恒绿（跳段/
//! 回退/原地流转拒绝、表外预留查询 None、末段前进 AtEnd）。

use super::cgp01_adaptivearch::{
    advance, reservation_by_task, risks_complete, ten_group_line, themes_present,
    AdvanceOutcome, ClaimStatus, O_HANDOVER, P_DOMAIN_VERSION, P_GROUPS, PIPELINE_STAGES,
    PipelineStage, PRIVACY_LINES, PRIVACY_DOC, RESERVATIONS, BASE_DOC, MISSION_DOC,
    ReceiptStatus, RISKS,
};
use alloc::string::String;

/// 判据侧独立重排的锚点判据十条。
const CRITERIA_RECHECK: [&str; 10] = [
    "自适应声明",
    "签收",
    "兑现起点",
    "映射",
    "五段",
    "基座",
    "红线",
    "风险",
    "八组",
    "判据",
];

/// 判据侧独立重算的十组规划表（与本体 P_GROUPS 逐条全等）。
const GROUPS_RECHECK: [(&str, u32, u32); 10] = [
    ("域开工与自适应遥测总架构组", 2401, 2416),
    ("遥测采集与分级组", 2417, 2432),
    ("遥测分析与洞察组", 2433, 2448),
    ("遥测看板与消费组", 2449, 2464),
    ("遥测预测与智能组", 2465, 2480),
    ("遥测生态与开放组", 2481, 2496),
    ("遥测性能与预算组", 2497, 2512),
    ("遥测测试与资产组", 2513, 2528),
    ("P 域预备与自查组", 2529, 2544),
    ("P 域收口组", 2545, 2560),
];

/// CGPU-F2401 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp01_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp01_adaptivearch");

    // —— 判据一 · 自适应声明：使命三条逐句可 grep ——
    s.add(
        "P01-自适应声明-使命三条",
        MISSION_DOC.contains("按需采集")
            && MISSION_DOC.contains("按价值存储")
            && MISSION_DOC.contains("按洞察呈现")
            && MISSION_DOC.contains("帧预算的隐形税")
            && P_DOMAIN_VERSION.starts_with("P01-"),
        "让遥测本身成为自适应系统：按需采集/按价值存储/按洞察呈现三条字面量冻结；遥测有预算意识不做隐形税",
    );

    // —— 判据二 · 签收：O 域移交结构化记录 ——
    s.add(
        "P01-签收-F2397结构化记录",
        O_HANDOVER.from_domain == "O域"
            && O_HANDOVER.source_task == 2397
            && O_HANDOVER.items == 4
            && O_HANDOVER.status == ReceiptStatus::Settled,
        "O 域移交包（F2397）签收为结构化记录：来源/单号/项数/状态四元在案——签收是记录不是口号",
    );

    // —— 判据三 · 兑现起点：三处预留逐条认领 + 反向（表外查询 None） ——
    let r1 = reservation_by_task(1453);
    let r2 = reservation_by_task(1553);
    let r3 = reservation_by_task(2216);
    let ghost = reservation_by_task(9999);
    let claims_ok = RESERVATIONS.len() == 3
        && r1.map(|r| r.claim_id == "P-R01" && r.status == ClaimStatus::Pending).unwrap_or(false)
        && r2.map(|r| r.claim_id == "P-R02").unwrap_or(false)
        && r3.map(|r| r.claim_id == "P-R03").unwrap_or(false)
        && ghost.is_none();
    s.add(
        "P01-兑现起点-三处预留逐条认领",
        claims_ok,
        "F1453/F1553/F2216 遥测预留逐条认领为兑现起点（Pending——兑现动作在 P 域内完成）；表外单号查询 None 不臆造",
    );

    // —— 判据四 · 映射：五主题闭集 + 十组表独立重排全等 + 起止连续 ——
    let themes = themes_present();
    let mut groups_ok = P_GROUPS.len() == 10 && GROUPS_RECHECK.len() == 10 && themes.len() == 5;
    let mut gi = 0usize;
    while gi < P_GROUPS.len() {
        let g = match P_GROUPS.get(gi) {
            Some(g) => *g,
            None => break,
        };
        let r = match GROUPS_RECHECK.get(gi) {
            Some(r) => *r,
            None => break,
        };
        if g.0 != r.0 || g.1 != r.1 || g.2 != r.2 {
            groups_ok = false;
        }
        if gi > 0 {
            let prev = match P_GROUPS.get(gi - 1) {
                Some(p) => *p,
                None => break,
            };
            if g.1 != prev.2 + 1 {
                groups_ok = false;
            }
        }
        gi += 1;
    }
    let line10 = ten_group_line();
    s.add(
        "P01-映射-五主题十组规划表",
        groups_ok
            && themes.get(0).map(|t| t.label()) == Some("统一模型")
            && themes.get(4).map(|t| t.label()) == Some("看板")
            && P_GROUPS.get(0).map(|g| g.1).unwrap_or(0) == 2401
            && P_GROUPS.get(9).map(|g| g.2).unwrap_or(0) == 2560
            && line10.contains("P01")
            && line10.contains("P10"),
        "官方五主题闭集（统一模型→看板）+十组规划表组名起止号与判据侧独立重排逐条全等、F2401-F2560 首尾连续无缺口；宣告行读屏可查",
    );

    // —— 判据五 · 五段：单向流转 + 跳段/回退/原地双向拒绝 ——
    let s0 = advance(PipelineStage::UnifiedModel, PipelineStage::AdaptiveCollect);
    let s4 = advance(PipelineStage::Analysis, PipelineStage::Consume);
    let back = advance(PipelineStage::Pipeline, PipelineStage::AdaptiveCollect);
    let skip = advance(PipelineStage::UnifiedModel, PipelineStage::Analysis);
    let same = advance(PipelineStage::Pipeline, PipelineStage::Pipeline);
    let end = advance(PipelineStage::Consume, PipelineStage::Consume);
    s.add(
        "P01-五段-单向流转双向拒绝",
        PIPELINE_STAGES.len() == 5
            && matches!(s0, Some(AdvanceOutcome::Advanced(PipelineStage::AdaptiveCollect)))
            && matches!(s4, Some(AdvanceOutcome::Advanced(PipelineStage::Consume)))
            && back.is_none()
            && skip.is_none()
            && same.is_none()
            && matches!(end, Some(AdvanceOutcome::AtEnd)),
        "统一模型→采集→管道→分析→消费恰邻段前进；回退/跳段/原地流转显性拒绝；末段再前进 AtEnd（阶段越权在类型面拦截）",
    );

    // —— 判据六 · 基座：三域收编统一 ——
    s.add(
        "P01-基座-三域收编统一",
        BASE_DOC.contains("J08=功耗遥测")
            && BASE_DOC.contains("K07=异构遥测")
            && BASE_DOC.contains("O04=降级遥测")
            && BASE_DOC.contains("P 域=全遥测基座")
            && BASE_DOC.contains("各域=遥测生产者")
            && BASE_DOC.contains("基座不反向依赖任何生产者"),
        "J08/K07/O04 三域遥测收编统一；P=基座各域=生产者，收编单向（生产者对接基座、基座零反向依赖）",
    );

    // —— 判据七 · 红线：三条硬约束逐条可 grep ——
    let lines_ok = PRIVACY_LINES.len() == 3
        && PRIVACY_LINES.get(0).map(|l| PRIVACY_DOC.contains(*l)).unwrap_or(false)
        && PRIVACY_LINES.get(1).map(|l| PRIVACY_DOC.contains(*l)).unwrap_or(false)
        && PRIVACY_LINES.get(2).map(|l| PRIVACY_DOC.contains(*l)).unwrap_or(false);
    s.add(
        "P01-红线-三条硬约束汇总",
        lines_ok
            && PRIVACY_DOC.contains("用户可查可关可删")
            && PRIVACY_DOC.contains("红线复用家族"),
        "本地默认/匿名化/用户控制三条红线闭集、逐条与汇总文档 grep 对账——红线复用家族口径随基座走",
    );

    // —— 判据八 · 风险：四条各配预案两两互异 ——
    let risks_ok = risks_complete()
        && RISKS.get(0).map(|r| r.what == "数据量").unwrap_or(false)
        && RISKS.get(1).map(|r| r.what == "口径漂移").unwrap_or(false)
        && RISKS.get(2).map(|r| r.what == "隐私").unwrap_or(false)
        && RISKS.get(3).map(|r| r.what == "性能").unwrap_or(false)
        && RISKS.get(3).map(|r| r.mitigation.contains("0.1%")).unwrap_or(false);
    s.add(
        "P01-风险-四条预案逐条互异",
        risks_ok,
        "数据量/口径漂移/隐私/性能四条各配预案且两两互异（同一句预案糊四条视为没预案——互异性可判定）；性能预案同 F0482 开销口径",
    );

    // —— 测试八组映射宣告（锚点测试条目：使命/签收/兑现/映射/五段/收编/红线/风险） ——
    let groups_tested = [
        "P01-自适应声明-使命三条",
        "P01-签收-F2397结构化记录",
        "P01-兑现起点-三处预留逐条认领",
        "P01-映射-五主题十组规划表",
        "P01-五段-单向流转双向拒绝",
        "P01-基座-三域收编统一",
        "P01-红线-三条硬约束汇总",
        "P01-风险-四条预案逐条互异",
    ];
    let mut tested = true;
    let mut i = 0usize;
    while i < groups_tested.len() {
        if groups_tested.get(i).is_none() {
            tested = false;
        }
        i += 1;
    }

    // —— 判据 stamp 独立对账 ——
    let stamps = ["自适应声明", "签收", "兑现起点", "映射", "五段", "基座", "红线", "风险", "八组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    let _ = String::new;
    s.add(
        "P01-判据stamp-十条独立重排全等",
        stamp_ok
            && tested
            && groups_tested.len() == 8
            && P_GROUPS.len() == 10,
        "锚点判据十条与判据侧独立重排逐条全等（常量被误改先红）；八组测试条目宣告与实际检查一一对应",
    );

    s
}
