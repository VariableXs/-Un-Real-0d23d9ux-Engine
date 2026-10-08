//! CGPU-F1921 判据层：M 域开工与显示输出总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1921`
//!
//! **锚点判据（最后一厘米/签收/映射/五段/贯通/协同/终端声明/风险/
//! 八组/判据）→ 判据族**：MISSION 2 / HANDOVER 2 / THEME 3 / PIPE 2 /
//! BRIDGE 2 / INVARIANT 2 / RISK 2 / CODE 2 / META 2 = 19 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 十组表/五主题/签收记录/贯通声明全部**判据侧独立写死字面量对拍**；
//! 主题覆盖率与区间守恒判据侧重算（漏一主题即红）；流水线双向（相邻
//! 迁移放行/跳段回退拒绝）；码段判据 `!=` 防自判死。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::cgm01_display::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的域使命三条款。
const EXP_MISSION: [&str; 3] = [
    "GPU 产出的一切都要正确、准时、好看地出现在物理屏上",
    "渲染的最后一厘米——显示输出域使命声明",
    "呈现不破坏 80 帧合同——合同终端声明",
];

/// 判据侧独立写死的五官方主题。
const EXP_THEMES: [&str; 5] = ["枚举热插拔", "模式时序", "色彩 HDR", "多显", "安全功耗"];

/// 判据侧独立写死的十组（官方批次标题逐字 + 区间）。
const EXP_GROUPS: [(u8, &str, u32, u32); 10] = [
    (1, "显示输出总架构与枚举组", 1921, 1936),
    (2, "显示模式与时序组", 1937, 1952),
    (3, "色彩与 HDR 输出组", 1953, 1968),
    (4, "显示体验与无障碍组", 1969, 1984),
    (5, "多显示器与拼接组", 1985, 2000),
    (6, "显示功耗与电源组", 2001, 2016),
    (7, "显示遥测与调试组", 2017, 2032),
    (8, "显示生态与认证组", 2033, 2048),
    (9, "M 域预备与自查组", 2049, 2064),
    (10, "M 域收口组", 2065, 2080),
];

/// 判据侧独立写死的移交包七件（逐字）。
const EXP_HANDOVER_ITEMS: [&str; 7] = [
    "接口冻结清单（73 函数+四资产）",
    "契约清单（十域契约）",
    "资产清单（五账）",
    "基线快照（总册+损耗表+精度表）",
    "遗留移交清单（SR-IOV 迁移/机密计算/8K）",
    "显示输出衔接包（虚拟显示路径→M 域物理显示输出接口草案）",
    "经验教训十条",
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VmCode; 7] = [
    VmCode::THEME_OUT_OF_TABLE,
    VmCode::GROUP_OUT_OF_TABLE,
    VmCode::RECEIPT_INCOMPLETE,
    VmCode::STAGE_VIOLATION,
    VmCode::BRIDGE_DRIFT,
    VmCode::TERMINAL_VIOLATION,
    VmCode::RISK_NO_PLAN,
];

// ---------------------------------------------------------------------------
// 判据主体
// ---------------------------------------------------------------------------

/// M01 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgm01_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-M");

    // --- MISSION · 最后一厘米声明（判据①：最后一厘米） --------------------
    let mission_match = MISSION_CLAUSES.len() == 3
        && (0..3).all(|i| MISSION_CLAUSES[i] == EXP_MISSION[i]);
    let mission_trivial = MISSION_CLAUSES.iter().all(|c| !c.is_empty())
        && MISSION_CLAUSES[0] != MISSION_CLAUSES[1]
        && MISSION_CLAUSES[1] != MISSION_CLAUSES[2]
        && MISSION_CLAUSES[0] != MISSION_CLAUSES[2];
    s.add(
        "M1-使命-最后一厘米三条款",
        mission_match && mission_trivial,
        "域使命三条款字面量冻结：正确/准时/好看逐条与判据侧独立写死对拍；三条款互异非空",
    );

    // 使命基准非平凡（判据侧自检：对拍参照自身非零）。
    let mission_ref_ok = EXP_MISSION.iter().any(|c| c.contains("最后一厘米"))
        && EXP_MISSION.iter().any(|c| c.contains("80 帧合同"));
    s.add(
        "M1-使命-判据侧参照非平凡",
        mission_ref_ok,
        "判据侧独立参照含「最后一厘米」与「80 帧合同」关键词——对拍不是空转",
    );

    // --- HANDOVER · L 域移交签收（判据②：签收） ---------------------------
    let receipt_ok = L_HANDOVER.source == 1917
        && L_HANDOVER.items == 7
        && L_HANDOVER.status == RecordStatus::Settled;
    s.add(
        "M1-签收-L域移交包记录",
        receipt_ok,
        "L 域移交包（F1917）签收：来源 1917、七件、状态 Settled——签收缺项即域未开工",
    );

    // 七件内容逐件独立对拍（判据侧写死）。
    let items_match = HANDOVER_ITEMS.len() == 7
        && (0..7).all(|i| HANDOVER_ITEMS[i] == EXP_HANDOVER_ITEMS[i])
        && HANDOVER_ITEMS.len() as u32 == L_HANDOVER.items;
    s.add(
        "M1-签收-七件逐件对拍",
        items_match,
        "移交包七件内容判据侧独立写死逐件对拍；件数与签收记录一致",
    );

    // --- THEME · 官方主题与十组映射（判据③：映射） ------------------------
    let themes_match = OFFICIAL_THEMES.len() == 5
        && (0..5).all(|i| OFFICIAL_THEMES[i] == EXP_THEMES[i]);
    s.add(
        "M1-映射-五主题对拍",
        themes_match,
        "官方五主题（枚举热插拔/模式时序/色彩 HDR/多显/安全功耗）判据侧独立写死对拍",
    );

    // 十组表逐条对拍：组名逐字+区间逐值。
    let groups_match = GROUP_PLANS.len() == 10
        && (0..10).all(|i| {
            let g = &GROUP_PLANS[i];
            let e = &EXP_GROUPS[i];
            g.group == e.0 && g.name == e.1 && g.first == e.2 && g.last == e.3
        });
    s.add(
        "M1-映射-十组逐条对拍",
        groups_match,
        "十组规划表组名取官方批次标题逐字、区间逐值对拍（M01..M10）",
    );

    // 覆盖率与守恒：五主题各有挂靠组；区间连续无缺口；总数守恒 160。
    let mut covered = [false; 5];
    let mut total = 0u32;
    let mut contiguous = true;
    for (i, g) in GROUP_PLANS.iter().enumerate() {
        total += g.last - g.first + 1;
        if i > 0 && g.first != GROUP_PLANS[i - 1].last + 1 {
            contiguous = false;
        }
        for &t in g.themes {
            if t < 5 {
                covered[t] = true;
            }
        }
    }
    let coverage_ok = covered.iter().all(|c| *c)
        && contiguous
        && total == M_DOMAIN_TOTAL
        && M_DOMAIN_FIRST == 1921
        && M_DOMAIN_LAST == 2080;
    s.add(
        "M1-映射-覆盖守恒连续",
        coverage_ok,
        "五主题全覆盖（漏一主题即红）；十组区间连续无缺口；160 项守恒（10 组×16 项）",
    );

    // --- PIPE · 架构五段（判据④：五段） ----------------------------------
    let labels_ok = PipelineStage::ALL.len() == 5
        && PipelineStage::ALL[0].label() == "枚举"
        && PipelineStage::ALL[2].label() == "呈现"
        && PipelineStage::ALL[4].label() == "观测";
    let forward_ok = (0..4).all(|i| stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
    s.add(
        "M1-五段-枚举到观测单向",
        labels_ok && forward_ok,
        "五段（枚举→模式→呈现→色彩→观测）标签闭集；相邻单向迁移全部放行",
    );

    // 跳段与回退双向拒绝（跳两段/回退/同段）。
    let reject_ok = !stage_transition(PipelineStage::Enumerate, PipelineStage::Color)
        && !stage_transition(PipelineStage::Color, PipelineStage::Mode)
        && !stage_transition(PipelineStage::Present, PipelineStage::Present)
        && !stage_transition(PipelineStage::Observe, PipelineStage::Enumerate);
    s.add(
        "M1-五段-跳段回退拒绝",
        reject_ok,
        "跳两段（枚举→色彩）/回退（色彩→模式）/同段/全回退均拒绝——流水线单向",
    );

    // --- BRIDGE · 贯通与协同（判据⑤：贯通；判据⑥：协同） ------------------
    let bridge_ok = VDISPLAY_BRIDGE.upstream == 1768
        && VDISPLAY_BRIDGE.what.contains("虚拟显示")
        && VDISPLAY_BRIDGE.what.contains("物理显示")
        && VDISPLAY_BRIDGE.status == RecordStatus::Pending;
    s.add(
        "M1-贯通-F1768上游钉死",
        bridge_ok,
        "虚拟显示→物理显示贯通声明：上游 F1768 钉死、跨卷显示契约、衔接任务待本域兑现",
    );

    let synergy_ok = VDOMAIN_SYNERGY.what.contains("V 域")
        && VDOMAIN_SYNERGY.what.contains("色彩管理")
        && VDOMAIN_SYNERGY.what != VDISPLAY_BRIDGE.what
        && VDISPLAY_BRIDGE.upstream != VDOMAIN_SYNERGY.upstream;
    s.add(
        "M1-协同-V域色彩跨卷",
        synergy_ok,
        "V 域协同声明（色彩管理跨卷）与贯通声明互异——本域不私设色彩语义",
    );

    // --- INVARIANT · 合同终端（判据⑦：终端声明） --------------------------
    let budget_ok = PRESENT_BUDGET.budget_us == 1_500
        && PRESENT_BUDGET.on_breach.contains("合同终端")
        && PRESENT_BUDGET.on_breach.contains("不静默吞帧");
    s.add(
        "M1-终端-呈现预算登记",
        budget_ok,
        "呈现段预算 1.5ms/帧（80 帧合同 12.5ms 的份额）；违约显性入帧日志联动降质链不静默",
    );

    // 终端声明反向：预算口径独立重算（1.5ms/12.5ms 占比 ≈ 12%）。
    let ratio_bp = PRESENT_BUDGET.budget_us * 10_000 / 12_500;
    let ratio_ok = ratio_bp >= 1_100 && ratio_bp <= 1_200;
    s.add(
        "M1-终端-预算占比重算",
        ratio_ok,
        "判据侧独立重算呈现份额占比（万分比 1100~1200 即 11%~12% 口径）",
    );

    // --- RISK · 风险四条（判据⑧：风险） ----------------------------------
    let risks_ok = RiskKind::ALL.len() == RISK_COUNT
        && RISK_COUNT == 4
        && (0..4).all(|i| {
            let a = RiskKind::ALL[i].fallback();
            !a.is_empty() && a.contains("预案")
        });
    let mut plans_differ = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if RiskKind::ALL[i].fallback() == RiskKind::ALL[j].fallback() {
                plans_differ = false;
            }
        }
    }
    s.add(
        "M1-风险-四条预案互异",
        risks_ok && plans_differ,
        "热插拔竞态/HDR 兼容/多屏同步/驱动差异四条预案逐条非空含「预案」且两两互异",
    );

    // --- CODE · 诊断码（码段 0x54xx 独占） --------------------------------
    let codes_ok = ALL_CODES.len() == 7
        && ALL_CODES.iter().all(|c| (c.code() & 0xFF00) == 0x5400)
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
        hi != 0x39 && hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
    });
    s.add(
        "M1-码段-0x54xx独占互异",
        codes_ok && uniq && no_clash,
        "七码全部 0x54xx、逐码互异、reason 非空；与 0x39/0x50/0x51/0x52/0x53 邻域不同值防自判死",
    );

    // --- META · 判据承载力自检（判据⑨：八组；判据⑩：判据） ----------------
    let meta_ok = EXP_GROUPS.len() == 10
        && EXP_THEMES.len() == 5
        && EXP_HANDOVER_ITEMS.len() == 7
        && ALL_CODES.len() == 7
        && total == 160;
    s.add(
        "M1-自检-八组判据承载力",
        meta_ok,
        "判据侧参照表自身非平凡：十组/五主题/七件/七码/160 守恒——对拍不是同源恒绿",
    );

    s
}
