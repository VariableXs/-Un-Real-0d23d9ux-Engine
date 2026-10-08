//! CGPU-F2561 判据层：Q 域开工与可靠性总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2561`
//!
//! **锚点判据（恢复起点/签收/映射/五段/交接/设施/红线/风险/八组）→
//! 判据族**：MISSION 2 / HAND 2 / THEME 3 / PIPE 2 / BRIDGE 2 /
//! REDLINE 2 / RISK 2 / META 4 = 19 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 使命条款/七件清单/红线句/交接三位/复用对**判据侧独立写死字面量
//! 对拍**；五主题覆盖率判据侧重算（漏一主题即红）；跳段回退双向；码
//! 段判据 `!=` 防自判死（0x50..0x55 全排除）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcq01_reliability::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的域使命三条款。
const EXP_MISSION: [&str; 3] = [
    "可靠性——让系统永远能恢复",
    "崩溃不是终点而是恢复的起点",
    "任何单点故障不得导致数据丢失或系统不可恢复",
];

/// 判据侧独立写死的五官方主题。
const EXP_THEMES: [&str; 5] = ["可靠性模型", "崩溃恢复", "看门狗", "健康检查", "容错"];

/// 判据侧独立写死的十组（官方批次标题逐字 + 区间）。
const EXP_GROUPS: [(u8, &str, u32, u32); 10] = [
    (1, "域开工与可靠性总架构组", 2561, 2576),
    (2, "崩溃恢复与容错组", 2577, 2592),
    (3, "看门狗与健康检查组", 2593, 2608),
    (4, "可靠性测试与资产组", 2609, 2624),
    (5, "驱动崩溃隔离组", 2625, 2640),
    (6, "长稳运行与漂移组", 2641, 2656),
    (7, "可靠性治理与演进组", 2657, 2672),
    (8, "可靠性生态与工具组", 2673, 2688),
    (9, "Q 域预备与自查组", 2689, 2704),
    (10, "Q 域收口组 · CGPU-Q 域 160 项收官", 2705, 2720),
];

/// 判据侧独立写死的移交包七件。
const EXP_HANDOVER_ITEMS: [&str; 7] = [
    "接口冻结清单",
    "契约清单",
    "资产清单",
    "基线快照",
    "遗留移交清单",
    "可靠性衔接包",
    "经验教训十条",
];

/// 判据侧独立写死的交接三位（peer/what/rule）。
const EXP_BRIDGE: (&str, &str, &str) = ("O 域", "降级管\"性能退\"、Q 管\"活着\"", "降级失败→容错接管");

/// 判据侧独立写死的设施复用对（上游/兑现）。
const EXP_CHAOS: (u32, u32) = (2465, 2609);

/// 判据侧独立写死的不可恢复红线句。
const EXP_REDLINE: &str = "不可恢复=最高缺陷——任何单点故障不得导致数据丢失或系统不可恢复";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VqCode; 6] = [
    VqCode::MISSION_DRIFT,
    VqCode::RECEIPT_INCOMPLETE,
    VqCode::THEME_OUT_OF_TABLE,
    VqCode::STAGE_VIOLATION,
    VqCode::HANDOVER_DRIFT,
    VqCode::REDLINE_BREACH,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 19;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// Q01 域自检入口。
pub fn run_vcq01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-reliability");

    // ================= 一、恢复起点哲学（MISSION） =================

    // MISS-1：域使命三条款字面量逐字对拍。
    let mission_ok = Q_MISSION.len() == 3
        && Q_MISSION[0] == EXP_MISSION[0]
        && Q_MISSION[1] == EXP_MISSION[1]
        && Q_MISSION[2] == EXP_MISSION[2];
    s.add("Q01-MISS-恢复起点三条款对拍", mission_ok, "");

    // MISS-2：域界宣言（F2561 开工、F2720 收官、160 项守恒）。
    s.add(
        "Q01-MISS-域界宣言",
        Q_DOMAIN_FIRST == 2561
            && Q_DOMAIN_LAST == 2720
            && Q_DOMAIN_TOTAL == 160
            && Q_DOMAIN_LAST - Q_DOMAIN_FIRST + 1 == Q_DOMAIN_TOTAL,
        "",
    );

    // ================= 二、P 域移交签收（HAND） =================

    // HAND-1：P2557 签收（来源/七件/状态已定——判据侧写死）。
    s.add(
        "Q01-HAND-P域移交签收",
        P_HANDOVER.source == 2557
            && P_HANDOVER.items == 7
            && P_HANDOVER.status == RecordStatus::Settled,
        "",
    );

    // HAND-2：七件清单逐字对拍（判据侧独立写死）。
    let mut items_ok = HANDOVER_ITEMS_Q.len() == 7;
    for i in 0..7 {
        if HANDOVER_ITEMS_Q[i] != EXP_HANDOVER_ITEMS[i] {
            items_ok = false;
        }
    }
    s.add("Q01-HAND-移交七件逐字对拍", items_ok, "");

    // ================= 三、主题映射（THEME） =================

    // THEME-1：五官方主题判据侧独立对拍。
    let mut theme_ok = OFFICIAL_THEMES_Q.len() == 5;
    for i in 0..5 {
        if OFFICIAL_THEMES_Q[i] != EXP_THEMES[i] {
            theme_ok = false;
        }
    }
    s.add("Q01-THEME-五主题闭集对拍", theme_ok, "");

    // THEME-2：十组官方名+区间逐字对拍。
    let mut group_ok = GROUP_PLANS_Q.len() == 10;
    for (i, g) in GROUP_PLANS_Q.iter().enumerate() {
        let e = EXP_GROUPS[i];
        if g.group != e.0 || g.name != e.1 || g.first != e.2 || g.last != e.3 {
            group_ok = false;
        }
    }
    s.add("Q01-THEME-十组官方名逐字对拍", group_ok, "");

    // THEME-3：连续守恒 + 五主题全覆盖（判据侧重算）。
    let mut cont_ok = GROUP_PLANS_Q[0].first == 2561 && GROUP_PLANS_Q[9].last == 2720;
    let mut total = 0u32;
    for i in 0..GROUP_PLANS_Q.len() {
        let g = &GROUP_PLANS_Q[i];
        total += g.last - g.first + 1;
        if i > 0 && g.first != GROUP_PLANS_Q[i - 1].last + 1 {
            cont_ok = false;
        }
    }
    let mut covered = [false; 5];
    for g in GROUP_PLANS_Q.iter() {
        for t in g.themes {
            if *t < 5 {
                covered[*t] = true;
            }
        }
    }
    let all_covered = covered.iter().all(|c| *c);
    s.add(
        "Q01-THEME-连续守恒与主题全覆盖",
        cont_ok && total == Q_DOMAIN_TOTAL && all_covered,
        "",
    );

    // ================= 四、五段架构（PIPE） =================

    // PIPE-1：五段闭集 + 单向相邻放行。
    let mut pipe_ok = PipelineStage::ALL.len() == 5;
    for (i, st) in PipelineStage::ALL.iter().enumerate() {
        if st.index() != i || st.label().is_empty() {
            pipe_ok = false;
        }
        if i < 4 && !stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]) {
            pipe_ok = false;
        }
    }
    s.add("Q01-PIPE-五段闭集单向放行", pipe_ok, "");

    // PIPE-2：跳段与回退双向拒绝。
    let skip = stage_transition(PipelineStage::FaultClassify, PipelineStage::HealthMonitor);
    let back = stage_transition(PipelineStage::ContinuousVerify, PipelineStage::FaultClassify);
    s.add("Q01-PIPE-跳段回退双向拒绝", !skip && !back, "");

    // ================= 五、交接与设施复用（BRIDGE） =================

    // BRIDGE-1：O 域分工交接声明三位逐字对拍。
    s.add(
        "Q01-BRIDGE-O域交接声明对拍",
        O_DOMAIN_HANDOVER.peer == EXP_BRIDGE.0
            && O_DOMAIN_HANDOVER.what == EXP_BRIDGE.1
            && O_DOMAIN_HANDOVER.rule == EXP_BRIDGE.2,
        "",
    );

    // BRIDGE-2：混沌设施复用对拍（上游 O05/兑现 Q04——兑现落域内）。
    s.add(
        "Q01-BRIDGE-混沌设施复用对拍",
        CHAOS_FACILITY_REUSE.upstream == EXP_CHAOS.0
            && CHAOS_FACILITY_REUSE.extend_at == EXP_CHAOS.1
            && CHAOS_FACILITY_REUSE.extend_at >= Q_DOMAIN_FIRST
            && CHAOS_FACILITY_REUSE.extend_at <= Q_DOMAIN_LAST
            && !CHAOS_FACILITY_REUSE.what.is_empty(),
        "",
    );

    // ================= 六、不可恢复红线（REDLINE） =================

    // RED-1：红线句字面量独立对拍。
    s.add("Q01-RED-红线句字面量对拍", UNRECOVERABLE_REDLINE == EXP_REDLINE, "");

    // RED-2：红线有立案通道——专属码在账且人话原因非空（无通道的红线=没红线）。
    s.add(
        "Q01-RED-红线立案通道在账",
        VqCode::REDLINE_BREACH.code() != 0
            && !VqCode::REDLINE_BREACH.reason().is_empty()
            && (VqCode::REDLINE_BREACH.code() >> 8) == 0x56,
        "",
    );

    // ================= 七、风险（RISK） =================

    // RISK-1：四风险闭集（判据侧独立点名）。
    let risk_ok = RiskKind::ALL.len() == RISK_COUNT
        && RISK_COUNT == 4
        && RiskKind::ALL[0] == RiskKind::StateLoss
        && RiskKind::ALL[3] == RiskKind::CascadingFailure;
    s.add("Q01-RISK-四风险闭集", risk_ok, "");

    // RISK-2：预案两两互异且非空（同话术=没预案）。
    let f: Vec<String> = RiskKind::ALL.iter().map(|r| r.fallback()).collect();
    let distinct = f[0] != f[1] && f[1] != f[2] && f[2] != f[3] && f[0] != f[2];
    s.add(
        "Q01-RISK-预案互异非空",
        f.iter().all(|x| !x.is_empty()) && distinct,
        "",
    );

    // ================= 八、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("Q01-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x56xx，且 != 0x50..0x55（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x56)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53 && hi != 0x54 && hi != 0x55
        });
    s.add("Q01-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
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
    s.add("Q01-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 18，加自身恰 19）。
    s.add("Q01-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
